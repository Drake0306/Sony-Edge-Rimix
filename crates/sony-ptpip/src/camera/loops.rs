//! The supervisor and the three loops it runs while connected. Each loop
//! returns only when the connection is gone, with the reason as text.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::broadcast::{self, error::RecvError};
use tokio::task::JoinSet;
use tokio::time::{interval, sleep, MissedTickBehavior};

use super::{connect_once, lock, CameraEvent, Inner, RecentShot};
use crate::capture::{captured_ready, receive_one, Destination};
use crate::error::Error;
use crate::liveview::next_frame;
use crate::ptp::codes::{
    EV_SONY_DEVICE_PROP_CHANGED, HANDLE_CAPTURED_STILL, OP_GET_OBJECT_INFO, PROP_CAPTURED_READY,
};
use crate::ptp::parse_object_info;
use crate::ptp::PtpValue;
use crate::{PtpEvent, PtpIpConnection};

/// Shortest time between two reads of the property table.
const PROP_REFRESH: Duration = Duration::from_millis(100);
/// Longest time between two reads of the property table, so a change the
/// camera announces with no event (such as a new shot waiting, 0xD215) is
/// still seen.
const PROP_POLL: Duration = Duration::from_millis(250);
/// Wait before asking for another live view frame when none was ready.
const NO_FRAME_WAIT: Duration = Duration::from_millis(15);
/// Wait before the next live view request after `consecutive_failures`
/// results in a row with no frame: short at first, then slower so a camera
/// that keeps refusing is not hammered.
pub(super) fn live_view_wait(consecutive_failures: u32) -> Duration {
    match consecutive_failures {
        0..=4 => NO_FRAME_WAIT,
        5..=9 => Duration::from_secs(1),
        _ => Duration::from_secs(3),
    }
}
/// Window the live view frame rate is measured over.
const FPS_WINDOW: Duration = Duration::from_secs(1);
/// How often the capture loop looks for waiting shots in the snapshot.
const CAPTURE_POLL: Duration = Duration::from_millis(250);
/// Pause after a failed delivery, so a full disk does not fail four times a second.
const CAPTURE_RETRY: Duration = Duration::from_secs(2);
/// How many saved shots are remembered for spotting a repeated delivery.
const RECENT_SHOTS: usize = 50;
/// Longest wait between reconnect attempts.
const MAX_BACKOFF: Duration = Duration::from_secs(15);

/// True when `e` means the connection cannot be used any more.
fn lost(conn: &PtpIpConnection, e: &Error) -> bool {
    matches!(e, Error::Disconnected) || conn.is_disconnected()
}

/// Delay before reconnect attempt `attempt` (from 1): 1, 2, 4, 8, then 15 s.
fn backoff(attempt: u32) -> Duration {
    let secs = 1u64 << attempt.saturating_sub(1).min(4);
    Duration::from_secs(secs).min(MAX_BACKOFF)
}

/// Runs the loops while connected and reconnects when the link drops, until
/// [`super::Camera::disconnect`] or drop aborts it. A reconnect refused by
/// the camera's SSH server (bad login or changed host key) ends it, with a
/// Disconnected event carrying that error's text.
pub(super) async fn supervise(inner: Arc<Inner>, mut events: broadcast::Receiver<PtpEvent>) {
    loop {
        let reason = run_session(&inner, events).await;
        tracing::info!(%reason, "camera connection lost");
        inner.lose_connection(reason).await;
        if !inner.reconnect {
            return;
        }
        let mut attempt = 0;
        events = loop {
            attempt += 1;
            sleep(backoff(attempt)).await;
            if inner.stopped() {
                return;
            }
            if attempt >= 2 {
                if let Some(hook) = &inner.before_reconnect {
                    hook(attempt).await;
                    if inner.stopped() {
                        return;
                    }
                }
            }
            inner.emit(CameraEvent::Reconnecting { attempt });
            match connect_once(&inner).await {
                Ok(session) => {
                    let ready = inner.install(session).await;
                    inner.emit(CameraEvent::Connected);
                    inner.emit_state();
                    break ready;
                }
                // Retrying a refused login could trip the camera's lockout,
                // and a changed host key needs the user: stop reconnecting.
                Err(e @ (Error::SshAuth | Error::HostKeyMismatch { .. })) => {
                    tracing::warn!(attempt, error = %e, "reconnect refused by the camera's SSH server; giving up");
                    lock(&inner.snapshot).connected = false;
                    inner.emit(CameraEvent::Disconnected {
                        reason: e.to_string(),
                    });
                    inner.emit_state();
                    return;
                }
                Err(e) => tracing::warn!(attempt, error = %e, "reconnect failed"),
            }
        };
    }
}

/// Runs the loops until the first one finds the connection gone, then stops
/// the rest and returns the reason.
async fn run_session(inner: &Arc<Inner>, events: broadcast::Receiver<PtpEvent>) -> String {
    let mut loops = JoinSet::new();
    loops.spawn(event_loop(inner.clone(), events));
    loops.spawn(live_view_loop(inner.clone()));
    if inner.auto_receive {
        loops.spawn(capture_loop(inner.clone()));
    }
    let reason = match loops.join_next().await {
        Some(Ok(reason)) => reason,
        Some(Err(e)) => format!("camera task failed: {e}"),
        None => "no camera tasks".into(),
    };
    loops.shutdown().await;
    reason
}

/// Keeps the snapshot's property table fresh: reads the whole table after
/// DevicePropChanged events, at most once per [`PROP_REFRESH`], and at
/// least once per [`PROP_POLL`] without them, and emits StateChanged when a
/// property changed. Between reads it checks the connection, so a drop is
/// noticed even with live view and capture off.
///
/// The table is read with 0x9209 and no parameters and compared with the
/// snapshot here, the way libgphoto2 polls Sony bodies
/// (`ptp_sony_getalldevicepropdesc` in `camlibs/ptp2/ptp.c`). The "changed
/// only" form (0x9209 with parameter 1) is not used: the ZV-E10 never
/// answers it, and every read then timed out and dropped the session.
async fn event_loop(inner: Arc<Inner>, mut events: broadcast::Receiver<PtpEvent>) -> String {
    let mut dirty = false;
    // The table was read while connecting.
    let mut last_read = Instant::now();
    let mut tick = interval(PROP_REFRESH);
    tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            event = events.recv() => match event {
                Ok(e) if e.code == EV_SONY_DEVICE_PROP_CHANGED => dirty = true,
                Ok(_) => {}
                // Missed events may have been property changes.
                Err(RecvError::Lagged(_)) => dirty = true,
                Err(RecvError::Closed) => return "event channel closed".into(),
            },
            _ = tick.tick() => {
                if dirty || last_read.elapsed() >= PROP_POLL {
                    let mut guard = inner.conn.lock().await;
                    let Some(conn) = guard.as_mut() else {
                        return "connection closed".into();
                    };
                    last_read = Instant::now();
                    match inner.refresh(conn).await {
                        Ok(_) => dirty = false,
                        Err(e) if lost(conn, &e) => return e.to_string(),
                        Err(e) => tracing::debug!(error = %e, "reading the properties failed"),
                    }
                } else if let Ok(guard) = inner.conn.try_lock() {
                    // Only checked when nobody else is using the connection,
                    // so no transaction is in flight.
                    if guard.as_ref().is_none_or(|c| c.is_disconnected()) {
                        return "camera disconnected".into();
                    }
                }
            }
        }
    }
}

/// Fetches live view frames one at a time, releasing the connection between
/// frames so commands and the capture loop get their turn.
async fn live_view_loop(inner: Arc<Inner>) -> String {
    let mut window_start = Instant::now();
    let mut frames = 0u32;
    let mut consecutive_failures = 0u32;
    loop {
        let frame = {
            let mut guard = inner.conn.lock().await;
            let Some(conn) = guard.as_mut() else {
                return "connection closed".into();
            };
            match next_frame(conn).await {
                Ok(frame) => frame,
                Err(e) if lost(conn, &e) => return e.to_string(),
                Err(e) => {
                    tracing::debug!(error = %e, "live view frame failed");
                    None
                }
            }
        };
        match frame {
            Some(frame) => {
                consecutive_failures = 0;
                *lock(&inner.frame) = Some(Arc::new(frame));
                frames += 1;
                inner.emit(CameraEvent::Frame);
            }
            None => {
                consecutive_failures = consecutive_failures.saturating_add(1);
                sleep(live_view_wait(consecutive_failures)).await;
            }
        }
        let elapsed = window_start.elapsed();
        if elapsed >= FPS_WINDOW {
            lock(&inner.snapshot).live_view_fps = frames as f32 / elapsed.as_secs_f32();
            frames = 0;
            window_start = Instant::now();
        }
        // The connection mutex is fair, so yielding lets a waiting command in.
        tokio::task::yield_now().await;
    }
}

/// Saves new shots as the camera reports them waiting.
///
/// The waiting count (0xD215) comes from the snapshot's property table,
/// which the event loop keeps fresh; nothing is asked of the camera until a
/// shot waits. After each delivery the table is read again here, so the
/// shot just fetched is not taken for another one.
async fn capture_loop(inner: Arc<Inner>) -> String {
    let mut tick = interval(CAPTURE_POLL);
    tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        tick.tick().await;
        loop {
            let ready = match captured_ready(&lock(&inner.snapshot).props) {
                Ok(n) => n,
                Err(e) => {
                    tracing::debug!(error = %e, "reading the capture count failed");
                    break;
                }
            };
            if ready == 0 {
                break;
            }
            let mut guard = inner.conn.lock().await;
            let Some(conn) = guard.as_mut() else {
                return "connection closed".into();
            };
            match receive_next(&inner, conn).await {
                Ok(()) => {
                    // Forget the fetched shot now, so a failed read below
                    // cannot leave the old count in place for the next tick.
                    let mut snap = lock(&inner.snapshot);
                    if let Some(p) = snap
                        .props
                        .iter_mut()
                        .find(|p| p.code == PROP_CAPTURED_READY)
                    {
                        p.current = PtpValue::U16(0);
                    }
                }
                Err(e) if lost(conn, &e) => return e.to_string(),
                Err(_) => {
                    drop(guard);
                    sleep(CAPTURE_RETRY).await;
                    break;
                }
            }
            match inner.refresh(conn).await {
                Ok(_) => {}
                Err(e) if lost(conn, &e) => return e.to_string(),
                Err(e) => {
                    tracing::debug!(error = %e, "reading the properties failed");
                    break;
                }
            }
            drop(guard);
            tokio::task::yield_now().await;
        }
    }
}

/// Fetches the oldest waiting shot and emits its events.
///
/// A camera can offer a shot again, for example when the link dropped before
/// it learned the transfer finished. Such a repeat must still be fetched, or
/// the camera keeps offering it, so it is downloaded through [`receive_one`]
/// like any other. If one of the last [`RECENT_SHOTS`] saved has the same
/// camera file name and size, and its file on disk holds exactly the same
/// bytes, the new copy is deleted and [`CameraEvent::CaptureSkipped`] is
/// emitted. Anything less than an exact match is kept: numbering restarts
/// after a card format, and raw files are nearly constant in size.
///
/// The file is written by `receive_one`, and compared, with blocking
/// `std::fs` calls on this task.
async fn receive_next(inner: &Inner, conn: &mut PtpIpConnection) -> Result<(), Error> {
    let (info, _) = conn
        .command_data_in(OP_GET_OBJECT_INFO, &[HANDLE_CAPTURED_STILL])
        .await?;
    let info = parse_object_info(&info)?;
    let filename = info.filename;
    let size = u64::from(info.compressed_size);
    let dest = lock(&inner.destination).clone();
    // Nothing is fetched while the folder cannot be written: the shot stays
    // on the camera and is tried again after a pause.
    if let Err(e) = dest.ensure_writable(chrono::Local::now().date_naive()) {
        return Err(report_failure(inner, &dest, filename, e));
    }
    inner.emit(CameraEvent::CaptureStarted {
        filename: filename.clone(),
    });
    let mut on_progress = |received, total| {
        inner.emit(CameraEvent::CaptureProgress {
            filename: filename.clone(),
            received,
            total,
        });
    };
    match receive_one(conn, &dest, &mut on_progress).await {
        Ok(saved) => {
            let earlier: Vec<PathBuf> = lock(&inner.recent)
                .iter()
                .filter(|r| r.filename == filename && r.size == size)
                .map(|r| r.path.clone())
                .collect();
            if earlier.iter().any(|path| same_contents(path, &saved.path)) {
                tracing::info!(file = %filename, "camera offered a shot again; removing the repeat");
                if let Err(e) = std::fs::remove_file(&saved.path) {
                    tracing::warn!(path = %saved.path.display(), error = %e, "could not remove a repeated shot");
                }
                lock(&inner.last_failure).take();
                inner.emit(CameraEvent::CaptureSkipped { filename });
                return Ok(());
            }
            {
                let mut recent = lock(&inner.recent);
                if recent.len() == RECENT_SHOTS {
                    recent.pop_front();
                }
                recent.push_back(RecentShot {
                    filename,
                    size,
                    path: saved.path.clone(),
                });
            }
            lock(&inner.last_failure).take();
            inner.mark_shot();
            inner.emit(CameraEvent::CaptureSaved(saved));
            inner.emit_state();
            Ok(())
        }
        Err(e) => Err(report_failure(inner, &dest, filename, e)),
    }
}

/// Emits [`CameraEvent::CaptureFailed`] for `e`, unless the last failure
/// had the same file name and text (a folder that stays unwritable fails
/// the same way on every retry); returns `e`.
fn report_failure(inner: &Inner, dest: &Destination, filename: String, e: Error) -> Error {
    let error = failure_text(dest, &e);
    let failure = (filename, error);
    let repeat = lock(&inner.last_failure).replace(failure.clone()).as_ref() == Some(&failure);
    if repeat {
        tracing::debug!(file = %failure.0, error = %failure.1, "receiving a shot failed again");
    } else {
        tracing::warn!(file = %failure.0, error = %failure.1, "receiving a shot failed");
        let (filename, error) = failure;
        inner.emit(CameraEvent::CaptureFailed { filename, error });
    }
    e
}

/// The text of a failed delivery, naming the folder when saving failed.
fn failure_text(dest: &Destination, e: &Error) -> String {
    let dir = || dest.target_dir(chrono::Local::now().date_naive());
    match e {
        Error::Io(io) => format!("Cannot save to {}: {io}", dir().display()),
        Error::Recovered { path, source } => format!(
            "Cannot save to {}: {source}. The shot was kept in {}",
            dir().display(),
            path.display()
        ),
        other => other.to_string(),
    }
}

/// True only if both files can be read and hold the same bytes.
fn same_contents(a: &Path, b: &Path) -> bool {
    match (std::fs::read(a), std::fs::read(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_identical_readable_files_match() {
        let tmp = tempfile::tempdir().unwrap();
        let [a, b, c] = ["a", "b", "c"].map(|n| tmp.path().join(n));
        std::fs::write(&a, b"same").unwrap();
        std::fs::write(&b, b"same").unwrap();
        std::fs::write(&c, b"diff").unwrap();
        assert!(same_contents(&a, &b));
        assert!(!same_contents(&a, &c));
        assert!(!same_contents(&tmp.path().join("missing"), &b));
    }

    #[test]
    fn live_view_wait_backs_off_after_repeated_failures() {
        assert_eq!(live_view_wait(0), NO_FRAME_WAIT);
        assert_eq!(live_view_wait(4), NO_FRAME_WAIT);
        assert_eq!(live_view_wait(5), Duration::from_secs(1));
        assert_eq!(live_view_wait(10), Duration::from_secs(3));
        assert_eq!(live_view_wait(500), Duration::from_secs(3));
    }

    #[test]
    fn backoff_doubles_then_caps() {
        let secs: Vec<u64> = (1..=7).map(|a| backoff(a).as_secs()).collect();
        assert_eq!(secs, [1, 2, 4, 8, 15, 15, 15]);
        assert_eq!(backoff(u32::MAX).as_secs(), 15);
    }
}

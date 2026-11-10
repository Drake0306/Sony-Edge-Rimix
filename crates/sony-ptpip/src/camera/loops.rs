//! The supervisor and the three loops it runs while connected. Each loop
//! returns only when the connection is gone, with the reason as text.

use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::broadcast::{self, error::RecvError};
use tokio::task::JoinSet;
use tokio::time::{interval, sleep, MissedTickBehavior};

use super::{connect_once, lock, CameraEvent, Inner};
use crate::capture::{captured_ready, receive_one};
use crate::error::Error;
use crate::liveview::next_frame;
use crate::ptp::codes::{EV_SONY_DEVICE_PROP_CHANGED, HANDLE_CAPTURED_STILL, OP_GET_OBJECT_INFO};
use crate::ptp::parse_object_info;
use crate::sony::property::read_changed_props;
use crate::{PtpEvent, PtpIpConnection};

/// Shortest time between two reads of changed properties.
const PROP_REFRESH: Duration = Duration::from_millis(100);
/// Wait before asking for another live view frame when none was ready.
const NO_FRAME_WAIT: Duration = Duration::from_millis(15);
/// Window the live view frame rate is measured over.
const FPS_WINDOW: Duration = Duration::from_secs(1);
/// How often the capture loop asks whether shots are waiting.
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
/// [`super::Camera::disconnect`] or drop aborts it.
pub(super) async fn supervise(
    inner: Arc<Inner>,
    mut events: broadcast::Receiver<PtpEvent>,
    mut live_view: bool,
) {
    loop {
        let reason = run_session(&inner, events, live_view).await;
        tracing::info!(%reason, "camera connection lost");
        inner.lose_connection(reason).await;
        if !inner.reconnect {
            return;
        }
        let mut attempt = 0;
        (events, live_view) = loop {
            attempt += 1;
            sleep(backoff(attempt)).await;
            if inner.stopped() {
                return;
            }
            inner.emit(CameraEvent::Reconnecting { attempt });
            match connect_once(&inner).await {
                Ok(session) => {
                    let ready = inner.install(session).await;
                    inner.emit(CameraEvent::Connected);
                    inner.emit_state();
                    break ready;
                }
                Err(e) => tracing::warn!(attempt, error = %e, "reconnect failed"),
            }
        };
    }
}

/// Runs the loops until the first one finds the connection gone, then stops
/// the rest and returns the reason.
async fn run_session(
    inner: &Arc<Inner>,
    events: broadcast::Receiver<PtpEvent>,
    live_view: bool,
) -> String {
    let mut loops = JoinSet::new();
    loops.spawn(event_loop(inner.clone(), events));
    if live_view {
        loops.spawn(live_view_loop(inner.clone()));
    }
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

/// Re-reads changed properties after DevicePropChanged events, at most once
/// per [`PROP_REFRESH`], and emits StateChanged. When idle it also checks the
/// connection, so a drop is noticed even with live view and capture off.
async fn event_loop(inner: Arc<Inner>, mut events: broadcast::Receiver<PtpEvent>) -> String {
    let mut dirty = false;
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
                if dirty {
                    let mut guard = inner.conn.lock().await;
                    let Some(conn) = guard.as_mut() else {
                        return "connection closed".into();
                    };
                    match read_changed_props(conn).await {
                        Ok(changed) => {
                            drop(guard);
                            dirty = false;
                            inner.merge(changed);
                            inner.emit_state();
                        }
                        Err(e) if lost(conn, &e) => return e.to_string(),
                        Err(e) => tracing::debug!(error = %e, "reading changed properties failed"),
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
                *lock(&inner.frame) = Some(Arc::new(frame));
                frames += 1;
                inner.emit(CameraEvent::Frame);
            }
            None => sleep(NO_FRAME_WAIT).await,
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
async fn capture_loop(inner: Arc<Inner>) -> String {
    let mut tick = interval(CAPTURE_POLL);
    tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        tick.tick().await;
        loop {
            let mut guard = inner.conn.lock().await;
            let Some(conn) = guard.as_mut() else {
                return "connection closed".into();
            };
            let ready = match captured_ready(conn).await {
                Ok(n) => n,
                Err(e) if lost(conn, &e) => return e.to_string(),
                Err(e) => {
                    tracing::debug!(error = %e, "reading the capture count failed");
                    break;
                }
            };
            if ready == 0 {
                break;
            }
            match receive_next(&inner, conn).await {
                Ok(()) => {}
                Err(e) if lost(conn, &e) => return e.to_string(),
                Err(_) => {
                    drop(guard);
                    sleep(CAPTURE_RETRY).await;
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
/// it learned the transfer finished. Such a repeat (same camera file name
/// and size as one of the last [`RECENT_SHOTS`] saved) must still be fetched,
/// or the camera keeps offering it, so it is downloaded through
/// [`receive_one`] like any other and the saved copy is then deleted, with
/// no events.
///
/// The file is written by `receive_one` with blocking `std::fs` calls on this
/// task.
async fn receive_next(inner: &Inner, conn: &mut PtpIpConnection) -> Result<(), Error> {
    let (info, _) = conn
        .command_data_in(OP_GET_OBJECT_INFO, &[HANDLE_CAPTURED_STILL])
        .await?;
    let info = parse_object_info(&info)?;
    let filename = info.filename;
    let key = (filename.clone(), u64::from(info.compressed_size));
    let repeat = lock(&inner.recent).contains(&key);
    if !repeat {
        inner.emit(CameraEvent::CaptureStarted {
            filename: filename.clone(),
        });
    }
    let dest = lock(&inner.destination).clone();
    let mut on_progress = |received, total| {
        if !repeat {
            inner.emit(CameraEvent::CaptureProgress {
                filename: filename.clone(),
                received,
                total,
            });
        }
    };
    match receive_one(conn, &dest, &mut on_progress).await {
        Ok(saved) if repeat => {
            tracing::info!(file = %filename, "camera offered a shot again; removing the repeat");
            if let Err(e) = std::fs::remove_file(&saved.path) {
                tracing::warn!(path = %saved.path.display(), error = %e, "could not remove a repeated shot");
            }
            Ok(())
        }
        Ok(saved) => {
            {
                let mut recent = lock(&inner.recent);
                if recent.len() == RECENT_SHOTS {
                    recent.pop_front();
                }
                recent.push_back(key);
            }
            inner.mark_shot();
            inner.emit(CameraEvent::CaptureSaved(saved));
            inner.emit_state();
            Ok(())
        }
        Err(e) => {
            let error = match &e {
                Error::Io(_) => {
                    let dir = dest.target_dir(chrono::Local::now().date_naive());
                    format!("{e} (saving to {})", dir.display())
                }
                _ => e.to_string(),
            };
            tracing::warn!(file = %filename, %error, "receiving a shot failed");
            inner.emit(CameraEvent::CaptureFailed { filename, error });
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_then_caps() {
        let secs: Vec<u64> = (1..=7).map(|a| backoff(a).as_secs()).collect();
        assert_eq!(secs, [1, 2, 4, 8, 15, 15, 15]);
        assert_eq!(backoff(u32::MAX).as_secs(), 15);
    }
}

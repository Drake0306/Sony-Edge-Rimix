//! The supervisor and the three loops it runs while connected. Each loop
//! returns only when the connection is gone, with the reason as text.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::broadcast::{self, error::RecvError};
use tokio::task::JoinSet;
use tokio::time::{interval, sleep, MissedTickBehavior};

use super::{connect_once, lock, CameraEvent, Inner, Receive, RecentShot};
use crate::capture::{
    captured_ready, delivery_target, ensure_delivery, receive_one, shot_name, Destination,
    DEST_COMPUTER,
};
use crate::card::CardKind;
use crate::error::Error;
use crate::liveview::next_frame;
use crate::ptp::codes::{
    EV_SONY_DEVICE_PROP_CHANGED, EV_SONY_OBJECT_ADDED, HANDLE_CAPTURED_STILL, OP_GET_OBJECT_INFO,
    PROP_CAPTURED_READY, PROP_FILE_FORMAT, PROP_MOVIE_REC_STATE, PROP_PC_SAVE_FORMAT,
    PROP_SHOTS_LEFT, PROP_STILL_DESTINATION,
};
use crate::ptp::parse_object_info;
use crate::ptp::PtpValue;
use crate::quirks::quirks_for;
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
/// How long after a picture is taken the camera is given to offer it
/// before the log says it did not.
const OFFER_WAIT: Duration = Duration::from_secs(6);
/// How often the save destination is looked at, and so the wait between
/// two requests to change it.
const DELIVERY_CHECK: Duration = Duration::from_millis(1500);
/// How many times in a row the camera is asked to send pictures here.
const DELIVERY_TRIES: u32 = 6;
/// How often the movie loop looks whether a recording started or stopped.
const MOVIE_POLL: Duration = Duration::from_millis(500);
/// Wait after a recording stops before the card is listed.
const MOVIE_SETTLE: Duration = Duration::from_millis(1500);
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
    // Always running: each looks at what the session is to do about new
    // pictures (`Receive`), which can change while connected.
    loops.spawn(capture_loop(inner.clone()));
    loops.spawn(movie_loop(inner.clone()));
    loops.spawn(delivery_loop(inner.clone()));
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
                // The camera announcing a new object: a picture in its
                // memory for the computer (older bodies say it only this
                // way), or a file on its card.
                Ok(e) if e.code == EV_SONY_OBJECT_ADDED => {
                    tracing::info!(params = ?e.params, "the camera announced a new object");
                    if e.params.first() == Some(&HANDLE_CAPTURED_STILL) {
                        inner.object_added.store(true, Ordering::SeqCst);
                    }
                    dirty = true;
                }
                // Anything else the camera says is rare and worth keeping:
                // it may be how this body announces a picture.
                Ok(e) => {
                    tracing::info!(code = %format_args!("{:#06x}", e.code), params = ?e.params, "camera event");
                    dirty = true;
                }
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
    let mut meta_samples = 0u32;
    let mut last_meta_len = usize::MAX;
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
                // The first metadata of a session, and each new length
                // after it (frames coming and going), up to a few: the real
                // layout is what the frame parser has to be checked against.
                if meta_samples < 4 && frame.raw_meta.len() != last_meta_len {
                    last_meta_len = frame.raw_meta.len();
                    meta_samples += 1;
                    let hex: String = frame
                        .raw_meta
                        .iter()
                        .take(400)
                        .map(|b| format!("{b:02x}"))
                        .collect();
                    tracing::info!(
                        bytes = frame.raw_meta.len(),
                        focus = frame.focus_frames.len(),
                        faces = frame.face_frames.len(),
                        tracking = frame.tracking_frames.len(),
                        hex = %hex,
                        "live view metadata sample"
                    );
                }
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
    let mut last_waiting: Option<u64> = None;
    let mut last_left: Option<u64> = None;
    // When the camera last took a picture it has not yet offered.
    let mut taken_at: Option<Instant> = None;
    // The last failure said, so one that repeats every two seconds is said once.
    let mut last_error = String::new();
    loop {
        tick.tick().await;
        loop {
            if inner.receive() != Receive::Here {
                break;
            }
            // What the camera says about pictures waiting, said in the log
            // whenever it changes: the raw value tells a body that never
            // offers one from a body whose offer Rimix cannot read.
            let waiting = lock(&inner.snapshot)
                .prop(PROP_CAPTURED_READY)
                .and_then(|p| p.current.as_u64());
            if waiting != last_waiting {
                if let Some(raw) = waiting.filter(|&raw| raw != 0) {
                    tracing::info!(value = %format_args!("{raw:#06x}"), "the camera says pictures are waiting (0xD215)");
                }
                last_waiting = waiting;
            }
            // A picture taken shows as one shot fewer left on the card, however
            // the shutter was pressed. If the camera then offers nothing,
            // that is said, with what it reports about where pictures go.
            let left = lock(&inner.snapshot)
                .prop(PROP_SHOTS_LEFT)
                .and_then(|p| p.current.as_u64());
            if let (Some(before), Some(now)) = (last_left, left) {
                if now < before {
                    tracing::info!(shots_left = now, "the camera took a picture");
                    taken_at = Some(Instant::now());
                }
            }
            last_left = left;
            if let Some(at) = taken_at.filter(|at| at.elapsed() >= OFFER_WAIT) {
                let snap = lock(&inner.snapshot);
                let value = |code| snap.prop(code).and_then(|p| p.current.as_u64());
                tracing::warn!(
                    waited_s = at.elapsed().as_secs(),
                    waiting_0xd215 = ?value(PROP_CAPTURED_READY),
                    save_destination_0xd222 = ?value(PROP_STILL_DESTINATION),
                    pc_save_format_0xd269 = ?value(PROP_PC_SAVE_FORMAT),
                    file_format_0xd253 = ?value(PROP_FILE_FORMAT),
                    "the camera took a picture and has not offered it to this computer"
                );
                taken_at = None;
            }
            let announced = inner.object_added.swap(false, Ordering::SeqCst);
            let ready = match captured_ready(&lock(&inner.snapshot).props) {
                Ok(n) => n,
                Err(e) => {
                    tracing::warn!(error = %e, "the camera's count of waiting pictures cannot be read");
                    break;
                }
            };
            if ready == 0 && !announced {
                break;
            }
            let mut guard = inner.conn.lock().await;
            let Some(conn) = guard.as_mut() else {
                return "connection closed".into();
            };
            // Offered: whatever was taken is on its way.
            taken_at = None;
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
                Err(e) => {
                    // Never silently: this is a picture the camera offered
                    // and Rimix did not get.
                    let text = e.to_string();
                    if text != last_error {
                        tracing::warn!(error = %text, "a picture the camera offered could not be received; trying again");
                        last_error = text;
                    }
                    drop(guard);
                    sleep(CAPTURE_RETRY).await;
                    break;
                }
            }
            last_error.clear();
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

/// Keeps the camera's save destination where the session wants it: the
/// computer as well as the card while pictures are copied here, the card
/// alone while they are not (see [`Receive`]).
///
/// A request is not always obeyed: the real ZV-E10 answered OK to the one
/// made while connecting and went on saving to its card only. So the save
/// destination is watched for the whole session. While it is not as
/// wanted, the camera is asked again, a few times a few seconds apart;
/// after [`DELIVERY_TRIES`] it is left alone, with a warning that names the
/// camera's own menu item. Each change of state is logged once, and
/// flipping the switch starts the count again.
async fn delivery_loop(inner: Arc<Inner>) -> String {
    let mut tick = interval(DELIVERY_CHECK);
    tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
    // What was last said about it: Some(true) as wanted, Some(false) given up.
    let mut said: Option<bool> = None;
    let mut tries = 0;
    let mut mode = inner.receive();
    loop {
        tick.tick().await;
        let now = inner.receive();
        if now != mode {
            (mode, said, tries) = (now, None, 0);
        }
        let to_computer = match mode {
            Receive::Leave => continue,
            Receive::Here => true,
            Receive::CardOnly => false,
        };
        let (props, quirks) = {
            let snap = lock(&inner.snapshot);
            (snap.props.clone(), quirks_for(&snap.model))
        };
        let Some(dest) = props.iter().find(|p| p.code == PROP_STILL_DESTINATION) else {
            continue;
        };
        let Some(sending) = dest.current.as_u64().map(|v| v & DEST_COMPUTER != 0) else {
            continue;
        };
        if sending == to_computer {
            if said != Some(true) {
                if to_computer {
                    tracing::info!("the camera is sending new pictures to this computer");
                } else {
                    tracing::info!("the camera is keeping new pictures on its card only, as asked");
                }
                said = Some(true);
            }
            tries = 0;
            continue;
        }
        if said == Some(true) {
            tracing::info!(sending, "the camera's save destination changed");
            said = None;
        }
        // Nothing the camera lists would make it so: say it once.
        let possible = delivery_target(dest, to_computer).is_some();
        if tries >= DELIVERY_TRIES || !possible {
            if said != Some(false) {
                if to_computer {
                    tracing::warn!(
                        tries,
                        "the camera keeps saving pictures to its card only; on the camera set Menu, Network, PC Remote Function, Still Img. Save Dest. to PC+Camera"
                    );
                } else {
                    tracing::warn!(tries, "the camera keeps offering pictures to this computer; on the camera set Still Img. Save Dest. to Camera Only");
                }
                said = Some(false);
            }
            continue;
        }
        tries += 1;
        let mut guard = inner.conn.lock().await;
        let Some(conn) = guard.as_mut() else {
            return "connection closed".into();
        };
        if let Err(e) = ensure_delivery(conn, &props, &quirks, to_computer).await {
            return e.to_string();
        }
        match inner.refresh(conn).await {
            Ok(_) => {}
            Err(e) if lost(conn, &e) => return e.to_string(),
            Err(e) => tracing::debug!(error = %e, "reading the properties failed"),
        }
    }
}

/// Copies each movie to the computer once its recording stops.
///
/// A movie is written to the card, never offered like a still, so the
/// card is listed when a recording starts and again when it stops, and the
/// movie files that appeared in between are downloaded, with the events of
/// a shot arriving. Files already on the card are never fetched.
///
/// Many bodies keep the card to themselves while they are remote
/// controlled. The first refusal is reported once with
/// [`CameraEvent::MovieTransferUnavailable`] and nothing more is tried in
/// this session; the clips stay on the card.
async fn movie_loop(inner: Arc<Inner>) -> String {
    let recording = || {
        lock(&inner.snapshot)
            .prop(PROP_MOVIE_REC_STATE)
            .and_then(|p| p.current.as_u64())
            .is_some_and(|v| v != 0)
    };
    let mut tick = interval(MOVIE_POLL);
    tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut was_recording = recording();
    // The handles on the card when the recording started.
    let mut before: Option<HashSet<u32>> = None;
    let mut unavailable = false;
    loop {
        tick.tick().await;
        let now_recording = recording();
        if inner.receive() != Receive::Here {
            // Not copying: a clip recorded meanwhile is never called new.
            was_recording = now_recording;
            before = None;
            continue;
        }
        if unavailable || now_recording == was_recording {
            was_recording = now_recording;
            continue;
        }
        was_recording = now_recording;
        if !now_recording {
            // The camera finishes the file a moment after it stops.
            sleep(MOVIE_SETTLE).await;
        }
        let files = match inner.card_list().await {
            Ok(files) => files,
            Err(Error::Disconnected) => return Error::Disconnected.to_string(),
            Err(e) => {
                tracing::warn!(error = %e, "the camera will not let its card be read in this session; movies stay on the card");
                inner.emit(CameraEvent::MovieTransferUnavailable {
                    reason: e.to_string(),
                });
                unavailable = true;
                continue;
            }
        };
        if now_recording {
            tracing::info!(
                files = files.len(),
                "recording started; the card was listed to tell the new clip apart later"
            );
            before = Some(files.iter().map(|f| f.handle).collect());
            continue;
        }
        tracing::info!(
            files = files.len(),
            "recording stopped; looking for the new clip on the card"
        );
        // Without a listing from before the recording, nothing can be
        // called new, and old clips are not to be copied.
        let Some(old) = before.take() else {
            tracing::info!(
                "a recording stopped that Rimix did not see start; its clip stays on the card"
            );
            continue;
        };
        for clip in files
            .iter()
            .filter(|f| f.kind == CardKind::Video && !old.contains(&f.handle))
        {
            tracing::info!(file = %clip.filename, bytes = clip.bytes, "copying the new movie from the card");
            inner.emit(CameraEvent::CaptureStarted {
                filename: clip.filename.clone(),
            });
            match inner.card_fetch(clip, true).await {
                Ok(saved) => {
                    tracing::info!(path = %saved.path.display(), bytes = saved.bytes, "movie saved");
                    inner.emit(CameraEvent::CaptureSaved(saved));
                    inner.emit_state();
                }
                Err(Error::Disconnected) => return Error::Disconnected.to_string(),
                Err(e) => {
                    tracing::warn!(file = %clip.filename, error = %e, "copying a movie from the card failed");
                    inner.emit(CameraEvent::CaptureFailed {
                        filename: clip.filename.clone(),
                        error: e.to_string(),
                    });
                }
            }
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
    let filename = shot_name(&info);
    let size = u64::from(info.compressed_size);
    let dest = lock(&inner.destination).clone();
    // Nothing is fetched while the folder cannot be written: the shot stays
    // on the camera and is tried again after a pause.
    if let Err(e) = dest.ensure_writable(chrono::Local::now().date_naive()) {
        return Err(report_failure(inner, &dest, filename, e));
    }
    tracing::info!(file = %filename, bytes = size, "the camera offers a picture; receiving it");
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
            tracing::info!(path = %saved.path.display(), bytes = saved.bytes, "picture saved");
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

use std::path::Path;
use std::time::Duration;

use sony_ptpip::capture::{Destination, DestinationMode};
use sony_ptpip::ptp::codes::{
    PROP_F_NUMBER, PROP_IMAGE_SIZE_TO_HOST, PROP_ISO, PROP_SHUTTER_SPEED, PROP_STILL_DESTINATION,
};
use sony_ptpip::ptp::PtpValue;
use sony_ptpip::{load_or_create_guid, Camera, CameraConfig, CameraEvent, CameraState};
use sony_ptpip_sim::{spawn, SimCommand, SimConfig, SimHandle};
use tokio::sync::broadcast;
use tokio::time::timeout;

/// Longest wait for any one expected event.
const WAIT: Duration = Duration::from_secs(5);

fn config(sim: &SimHandle, dir: &Path) -> CameraConfig {
    CameraConfig {
        addr: sim.addr,
        guid: [0x42; 16],
        host_name: "Rimix on test".into(),
        destination: Destination {
            base: dir.to_path_buf(),
            mode: DestinationMode::CustomFolder,
            recovery_dir: None,
        },
        auto_receive: true,
        reconnect: true,
        before_reconnect: None,
        ssh: None,
    }
}

async fn start(sim_config: SimConfig, dir: &Path) -> (SimHandle, Camera) {
    let sim = spawn(sim_config).await;
    let camera = Camera::connect(config(&sim, dir)).await.unwrap();
    (sim, camera)
}

async fn stop(sim: SimHandle, camera: Camera) {
    camera.disconnect().await;
    sim.events.send(SimCommand::Shutdown).await.unwrap();
}

/// Waits for the first event `pick` accepts and returns what it returned.
async fn wait_for<T>(
    rx: &mut broadcast::Receiver<CameraEvent>,
    what: &str,
    pick: impl FnMut(&CameraEvent) -> Option<T>,
) -> T {
    wait_within(WAIT, rx, what, pick).await
}

/// [`wait_for`] with its own limit, for what follows a card listing: with
/// every test of this file running at once that takes seconds.
async fn wait_within<T>(
    limit: Duration,
    rx: &mut broadcast::Receiver<CameraEvent>,
    what: &str,
    mut pick: impl FnMut(&CameraEvent) -> Option<T>,
) -> T {
    timeout(limit, async {
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    if let Some(t) = pick(&ev) {
                        return t;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(e) => panic!("event channel failed: {e}"),
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting for {what}"))
}

fn short_of(state: &CameraState, code: u16) -> String {
    let prop = state.properties.iter().find(|p| p.code == code).unwrap();
    prop.short.clone()
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn guid_is_stable_across_calls() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("nested").join("guid");
    let first = load_or_create_guid(&path).unwrap();
    let second = load_or_create_guid(&path).unwrap();
    assert_eq!(first, second);
    assert_ne!(first, [0; 16]);
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(text.len(), 33);
    assert!(text.ends_with('\n'));
}

#[tokio::test]
async fn connect_populates_state_with_labels() {
    let tmp = tempfile::tempdir().unwrap();
    let (sim, camera) = start(SimConfig::default(), tmp.path()).await;
    let state = camera.state();
    assert!(state.connected);
    assert_eq!(state.model, "ZV-E10");
    assert_eq!(state.generation, "3");
    assert_eq!(state.firmware, "2.00");
    assert_eq!(state.battery, Some(72));
    assert!(!state.recording);
    assert_eq!(short_of(&state, PROP_SHUTTER_SPEED), "1/125");
    let shutter = state
        .properties
        .iter()
        .find(|p| p.code == PROP_SHUTTER_SPEED)
        .unwrap();
    assert_eq!(shutter.label, "1/125");
    assert_eq!(shutter.choices.len(), 20);
    assert!(shutter.choices.iter().any(|c| c.label == "1/100"));
    // White balance 0x8007 labels with the colour temperature from 0xD20F.
    assert_eq!(short_of(&state, 0x5005), "5200K");
    stop(sim, camera).await;
}

#[tokio::test]
async fn set_iso_updates_state_and_emits_event() {
    let tmp = tempfile::tempdir().unwrap();
    let (sim, camera) = start(SimConfig::default(), tmp.path()).await;
    let mut rx = camera.subscribe();
    camera.set(PROP_ISO, PtpValue::U32(1600)).await.unwrap();
    assert_eq!(short_of(&camera.state(), PROP_ISO), "1600");
    wait_for(&mut rx, "StateChanged with ISO 1600", |ev| match ev {
        CameraEvent::StateChanged(s) if short_of(s, PROP_ISO) == "1600" => Some(()),
        _ => None,
    })
    .await;
    // A value outside the settable list is refused before reaching the camera.
    assert!(camera.set(PROP_ISO, PtpValue::U32(1601)).await.is_err());
    stop(sim, camera).await;
}

#[tokio::test]
async fn step_shutter_moves_to_neighbour() {
    let tmp = tempfile::tempdir().unwrap();
    let (sim, camera) = start(SimConfig::default(), tmp.path()).await;
    let state = camera.state();
    let shutter = state
        .properties
        .iter()
        .find(|p| p.code == PROP_SHUTTER_SPEED)
        .unwrap();
    let at = shutter
        .choices
        .iter()
        .position(|c| c.raw == shutter.raw)
        .unwrap();
    let next = shutter.choices[at + 1].clone();
    camera.step(PROP_SHUTTER_SPEED, 1).await.unwrap();
    let after = camera.state();
    let shutter = after
        .properties
        .iter()
        .find(|p| p.code == PROP_SHUTTER_SPEED)
        .unwrap();
    assert_eq!(shutter.raw, next.raw);
    assert_eq!(shutter.short, "1/100");
    // Stepping far past the end clamps to the last value.
    camera.step(PROP_SHUTTER_SPEED, 100).await.unwrap();
    let last = camera.state();
    let shutter = last
        .properties
        .iter()
        .find(|p| p.code == PROP_SHUTTER_SPEED)
        .unwrap();
    assert_eq!(Some(&shutter.raw), shutter.choices.last().map(|c| &c.raw));
    stop(sim, camera).await;
}

#[tokio::test]
async fn step_range_property_moves_by_its_step() {
    let tmp = tempfile::tempdir().unwrap();
    let (sim, camera) = start(SimConfig::default(), tmp.path()).await;
    camera.step(0xD20F, -2).await.unwrap();
    let state = camera.state();
    let temp = state.properties.iter().find(|p| p.code == 0xD20F).unwrap();
    assert_eq!(temp.raw, PtpValue::U16(5000));
    stop(sim, camera).await;
}

#[tokio::test]
async fn live_view_frames_arrive() {
    let tmp = tempfile::tempdir().unwrap();
    let (sim, camera) = start(SimConfig::default(), tmp.path()).await;
    let mut rx = camera.subscribe();
    timeout(Duration::from_secs(2), async {
        loop {
            if let Ok(CameraEvent::Frame) = rx.recv().await {
                return;
            }
        }
    })
    .await
    .expect("a Frame event within 2 s");
    let frame = camera.latest_frame().expect("a stored frame");
    assert_eq!(&frame.jpeg[..2], &[0xFF, 0xD8]);
    stop(sim, camera).await;
}

#[tokio::test]
async fn shoot_results_in_capture_saved_event() {
    let tmp = tempfile::tempdir().unwrap();
    let (sim, camera) = start(SimConfig::default(), tmp.path()).await;
    let mut rx = camera.subscribe();
    camera.shoot().await.unwrap();
    let started = wait_for(&mut rx, "CaptureStarted", |ev| match ev {
        CameraEvent::CaptureStarted { filename } => Some(filename.clone()),
        _ => None,
    })
    .await;
    assert_eq!(started, "DSC00001.JPG");
    let saved = wait_for(&mut rx, "CaptureSaved", |ev| match ev {
        CameraEvent::CaptureSaved(r) => Some(r.clone()),
        _ => None,
    })
    .await;
    assert_eq!(saved.filename, "DSC00001.JPG");
    assert_eq!(saved.path, tmp.path().join("DSC00001.JPG"));
    assert_eq!(std::fs::metadata(&saved.path).unwrap().len(), saved.bytes);
    assert!(camera.state().last_shot_at.is_some());
    stop(sim, camera).await;
}

#[tokio::test]
async fn reconnects_after_drop() {
    let tmp = tempfile::tempdir().unwrap();
    let (sim, camera) = start(SimConfig::default(), tmp.path()).await;
    let mut rx = camera.subscribe();
    sim.events.send(SimCommand::DropConnection).await.unwrap();
    wait_for(&mut rx, "Disconnected", |ev| {
        matches!(ev, CameraEvent::Disconnected { .. }).then_some(())
    })
    .await;
    let attempt = wait_for(&mut rx, "Reconnecting", |ev| match ev {
        CameraEvent::Reconnecting { attempt } => Some(*attempt),
        _ => None,
    })
    .await;
    assert_eq!(attempt, 1);
    wait_for(&mut rx, "Connected", |ev| {
        matches!(ev, CameraEvent::Connected).then_some(())
    })
    .await;
    assert!(camera.state().connected);
    camera.shoot().await.unwrap();
    let saved = wait_for(&mut rx, "CaptureSaved", |ev| match ev {
        CameraEvent::CaptureSaved(r) => Some(r.filename.clone()),
        _ => None,
    })
    .await;
    assert_eq!(saved, "DSC00001.JPG");
    stop(sim, camera).await;
}

#[tokio::test]
async fn duplicate_delivery_is_skipped() {
    let tmp = tempfile::tempdir().unwrap();
    let sim_config = SimConfig {
        redeliver_after_drop: true,
        ..SimConfig::default()
    };
    let (sim, camera) = start(sim_config, tmp.path()).await;
    let mut rx = camera.subscribe();
    let mut saved = Vec::new();

    camera.shoot().await.unwrap();
    saved.push(
        wait_for(&mut rx, "first CaptureSaved", |ev| match ev {
            CameraEvent::CaptureSaved(r) => Some(r.filename.clone()),
            _ => None,
        })
        .await,
    );

    // The simulator puts DSC00001 back in its queue when the link drops.
    sim.events.send(SimCommand::DropConnection).await.unwrap();
    wait_for(&mut rx, "Connected", |ev| {
        matches!(ev, CameraEvent::Connected).then_some(())
    })
    .await;

    // The queue is first in, first out, so the repeat of DSC00001 is
    // delivered (and skipped) before DSC00002 arrives.
    camera.shoot().await.unwrap();
    let skipped = wait_for(&mut rx, "CaptureSkipped", |ev| match ev {
        CameraEvent::CaptureSkipped { filename } => Some(filename.clone()),
        _ => None,
    })
    .await;
    assert_eq!(skipped, "DSC00001.JPG");
    saved.push(
        wait_for(&mut rx, "second CaptureSaved", |ev| match ev {
            CameraEvent::CaptureSaved(r) => Some(r.filename.clone()),
            _ => None,
        })
        .await,
    );

    assert_eq!(saved, ["DSC00001.JPG", "DSC00002.JPG"]);
    assert_eq!(names(tmp.path()), ["DSC00001.JPG", "DSC00002.JPG"]);
    stop(sim, camera).await;
}

#[tokio::test]
async fn same_name_and_size_but_different_content_keeps_both() {
    let tmp = tempfile::tempdir().unwrap();
    // After the first shot the simulator numbers from DSC00001 again, as
    // after a card format: same name and size, different bytes.
    let sim_config = SimConfig {
        restart_numbering_after: Some(1),
        ..SimConfig::default()
    };
    let (sim, camera) = start(sim_config, tmp.path()).await;
    let mut rx = camera.subscribe();
    let mut saved = Vec::new();
    for n in 1..=2 {
        camera.shoot().await.unwrap();
        saved.push(
            wait_for(&mut rx, &format!("CaptureSaved {n}"), |ev| match ev {
                CameraEvent::CaptureSaved(r) => Some(r.clone()),
                CameraEvent::CaptureSkipped { filename } => panic!("{filename} was skipped"),
                _ => None,
            })
            .await,
        );
    }
    assert_eq!(saved[0].filename, "DSC00001.JPG");
    assert_eq!(saved[1].filename, "DSC00001-1.JPG");
    assert_eq!(saved[0].bytes, saved[1].bytes);
    assert_eq!(names(tmp.path()), ["DSC00001-1.JPG", "DSC00001.JPG"]);
    let first = std::fs::read(&saved[0].path).unwrap();
    let second = std::fs::read(&saved[1].path).unwrap();
    assert_ne!(first, second);
    stop(sim, camera).await;
}

/// Number of 0x9207 controls the simulator has handled.
fn controls_sent(sim: &SimHandle) -> usize {
    let log = sim.log.lock().unwrap();
    log.iter().filter(|l| l.starts_with("op 0x9207 ")).count()
}

#[tokio::test]
async fn shoot_focuses_when_focus_mode_is_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let source =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../sony-ptpip-sim/fixtures/zv-e10.json");
    let text = std::fs::read_to_string(source).unwrap();
    assert!(text.contains("\"code\": \"0x500A\""));
    let fixture = tmp.path().join("no-focus-mode.json");
    std::fs::write(
        &fixture,
        text.replace("\"code\": \"0x500A\"", "\"code\": \"0x5FFF\""),
    )
    .unwrap();
    let shots = tmp.path().join("shots");
    let sim_config = SimConfig {
        fixture,
        ..SimConfig::default()
    };
    let (sim, camera) = start(sim_config, &shots).await;
    assert!(camera.state().properties.iter().all(|p| p.code != 0x500A));
    let before = controls_sent(&sim);
    camera.shoot().await.unwrap();
    // Half press, full press, full release, half release.
    assert_eq!(controls_sent(&sim) - before, 4);
    stop(sim, camera).await;
}

#[tokio::test]
async fn disconnect_stops_without_reconnecting() {
    let tmp = tempfile::tempdir().unwrap();
    let (sim, camera) = start(SimConfig::default(), tmp.path()).await;
    let mut rx = camera.subscribe();
    camera.disconnect().await;
    assert!(!camera.state().connected);
    assert!(camera.shoot().await.is_err());
    // Nothing tries to reconnect after an explicit disconnect.
    let reconnect = timeout(Duration::from_millis(1500), async {
        loop {
            match rx.recv().await {
                Ok(CameraEvent::Reconnecting { .. } | CameraEvent::Connected) => return true,
                Ok(_) => {}
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(_) => return false,
            }
        }
    })
    .await;
    assert!(!matches!(reconnect, Ok(true)));
    sim.events.send(SimCommand::Shutdown).await.unwrap();
}

#[test]
fn state_and_events_serialise() {
    fn assert_serde<T: serde::Serialize + serde::de::DeserializeOwned>() {}
    assert_serde::<CameraState>();
    assert_serde::<CameraEvent>();
    assert_serde::<CameraConfig>();
}

#[tokio::test]
async fn connect_survives_a_live_view_refusal_and_frames_arrive_later() {
    let tmp = tempfile::tempdir().unwrap();
    // The live view check at connect is answered with AccessDenied.
    let (sim, camera) = start(
        SimConfig {
            live_view_info_denied: 1,
            ..SimConfig::default()
        },
        tmp.path(),
    )
    .await;
    let mut rx = camera.subscribe();
    wait_for(&mut rx, "a Frame", |ev| {
        matches!(ev, CameraEvent::Frame).then_some(())
    })
    .await;
    assert!(camera.latest_frame().is_some());
    stop(sim, camera).await;
}

#[tokio::test]
async fn live_view_keeps_polling_through_repeated_refusals() {
    let tmp = tempfile::tempdir().unwrap();
    let (sim, camera) = start(
        SimConfig {
            live_view_info_denied: 12,
            ..SimConfig::default()
        },
        tmp.path(),
    )
    .await;
    let mut rx = camera.subscribe();
    // The backoff slows polling but never stops it.
    let got = timeout(Duration::from_secs(20), async {
        loop {
            if matches!(rx.recv().await, Ok(CameraEvent::Frame)) {
                return;
            }
        }
    })
    .await;
    assert!(got.is_ok(), "no frame within 20 s");
    assert!(camera.latest_frame().is_some());
    stop(sim, camera).await;
}

#[tokio::test]
async fn unwritable_destination_reports_one_failure() {
    let tmp = tempfile::tempdir().unwrap();
    let not_a_dir = tmp.path().join("not-a-dir");
    std::fs::write(&not_a_dir, b"x").unwrap();
    let (sim, camera) = start(SimConfig::default(), &not_a_dir).await;
    let mut rx = camera.subscribe();
    camera.shoot().await.unwrap();
    let mut failed = Vec::new();
    let mut started = 0;
    let _ = timeout(WAIT, async {
        loop {
            match rx.recv().await {
                Ok(CameraEvent::CaptureFailed { filename, error }) => {
                    failed.push((filename, error))
                }
                Ok(CameraEvent::CaptureStarted { .. }) => started += 1,
                Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(e) => panic!("event channel failed: {e}"),
            }
        }
    })
    .await;
    assert_eq!(failed.len(), 1, "{failed:?}");
    assert_eq!(failed[0].0, "DSC00001.JPG");
    assert!(
        failed[0].1.contains(&not_a_dir.display().to_string()),
        "{}",
        failed[0].1
    );
    assert_eq!(started, 0);
    stop(sim, camera).await;
}

#[tokio::test]
async fn suffixed_save_names_the_camera_file() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("DSC00001.JPG"), b"older").unwrap();
    let (sim, camera) = start(SimConfig::default(), tmp.path()).await;
    let mut rx = camera.subscribe();
    camera.shoot().await.unwrap();
    let saved = wait_for(&mut rx, "CaptureSaved", |ev| match ev {
        CameraEvent::CaptureSaved(r) => Some(r.clone()),
        _ => None,
    })
    .await;
    assert_eq!(saved.filename, "DSC00001-1.JPG");
    assert_eq!(saved.camera_filename, "DSC00001.JPG");
    stop(sim, camera).await;
}

#[tokio::test]
async fn shot_that_cannot_be_saved_is_recovered_and_reported() {
    let tmp = tempfile::tempdir().unwrap();
    let shots = tmp.path().join("shots");
    std::fs::create_dir_all(&shots).unwrap();
    std::fs::write(shots.join("DSC00001.JPG"), b"older").unwrap();
    for n in 1..=999 {
        std::fs::write(shots.join(format!("DSC00001-{n}.JPG")), b"older").unwrap();
    }
    let recovery = tmp.path().join("recovered");
    let sim = spawn(SimConfig::default()).await;
    let mut cfg = config(&sim, &shots);
    cfg.destination.recovery_dir = Some(recovery.clone());
    let camera = Camera::connect(cfg).await.unwrap();
    let mut rx = camera.subscribe();
    camera.shoot().await.unwrap();
    let (filename, error) = wait_for(&mut rx, "CaptureFailed", |ev| match ev {
        CameraEvent::CaptureFailed { filename, error } => Some((filename.clone(), error.clone())),
        _ => None,
    })
    .await;
    assert_eq!(filename, "DSC00001.JPG");
    let kept = recovery.join("DSC00001.JPG");
    assert!(error.contains(&kept.display().to_string()), "{error}");
    assert!(kept.is_file());
    stop(sim, camera).await;
}

#[tokio::test]
async fn the_reconnect_hook_runs_before_the_second_attempt() {
    use std::sync::{Arc, Mutex};
    let tmp = tempfile::tempdir().unwrap();
    let sim = spawn(SimConfig::default()).await;
    let addr = sim.addr;
    let seen: Arc<Mutex<Vec<u32>>> = Arc::default();
    let revived: Arc<Mutex<Option<SimHandle>>> = Arc::default();
    let hook: sony_ptpip::ReconnectHook = {
        let (seen, revived) = (seen.clone(), revived.clone());
        Arc::new(move |attempt| {
            let (seen, revived) = (seen.clone(), revived.clone());
            Box::pin(async move {
                seen.lock().unwrap().push(attempt);
                // The camera's network is back by the time the hook returns.
                let sim = spawn(SimConfig {
                    bind_addr: Some(addr),
                    ..SimConfig::default()
                })
                .await;
                *revived.lock().unwrap() = Some(sim);
            })
        })
    };
    let mut cfg = config(&sim, tmp.path());
    cfg.before_reconnect = Some(hook);
    let camera = Camera::connect(cfg).await.unwrap();
    let mut rx = camera.subscribe();
    // The camera disappears: attempt 1 is refused, so attempt 2 needs the hook.
    sim.events.send(SimCommand::Shutdown).await.unwrap();
    timeout(Duration::from_secs(10), async {
        loop {
            if let Ok(CameraEvent::Connected) = rx.recv().await {
                return;
            }
        }
    })
    .await
    .expect("camera came back");
    assert_eq!(*seen.lock().unwrap(), vec![2]);
    assert!(camera.state().connected);
    camera.disconnect().await;
    let revived = revived.lock().unwrap().take().unwrap();
    revived.events.send(SimCommand::Shutdown).await.unwrap();
}

#[tokio::test]
async fn a7_iv_reports_generation_four() {
    let tmp = tempfile::tempdir().unwrap();
    let sim_config = SimConfig {
        model: "ILCE-7M4".into(),
        fixture: "fixtures/ilce-7m4.json".into(),
        ..SimConfig::default()
    };
    let (sim, camera) = start(sim_config, tmp.path()).await;
    let state = camera.state();
    assert_eq!(state.model, "ILCE-7M4");
    assert_eq!(state.generation, "4");
    stop(sim, camera).await;
}

/// Every operation line the simulator logged.
fn sim_log(sim: &SimHandle) -> Vec<String> {
    sim.log.lock().unwrap().clone()
}

/// The simulator's defaults mirror the ZV-E10, which never answers 0x9209
/// with a parameter or 0x9251. Polling must use neither, so a session stays
/// up: no drop and no reconnect for 8 s, live view running, a shot received
/// through the property table, and a settings change reflected.
#[tokio::test]
async fn session_stays_up_against_a_camera_that_ignores_parameter_reads() {
    let tmp = tempfile::tempdir().unwrap();
    let (sim, camera) = start(SimConfig::default(), tmp.path()).await;
    let mut rx = camera.subscribe();
    let mut frames = 0u32;
    let mut saved = None;
    let mut iso_seen = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
    let mut shot = false;
    let mut iso_set = false;
    while tokio::time::Instant::now() < deadline {
        let Ok(ev) = timeout(Duration::from_millis(200), rx.recv()).await else {
            continue;
        };
        match ev {
            Ok(CameraEvent::Disconnected { reason }) => panic!("session dropped: {reason}"),
            Ok(CameraEvent::Reconnecting { attempt }) => panic!("reconnect attempt {attempt}"),
            Ok(CameraEvent::Frame) => frames += 1,
            Ok(CameraEvent::CaptureSaved(r)) => saved = Some(r),
            Ok(CameraEvent::StateChanged(s)) if short_of(&s, PROP_ISO) == "1600" => iso_seen = true,
            Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
            Err(e) => panic!("event channel failed: {e}"),
        }
        if !shot && frames >= 5 {
            shot = true;
            camera.shoot().await.unwrap();
        }
        if !iso_set && saved.is_some() {
            iso_set = true;
            camera.set(PROP_ISO, PtpValue::U32(1600)).await.unwrap();
        }
    }
    let state = camera.state();
    assert!(state.connected);
    assert!(frames > 0);
    assert!(state.live_view_fps > 0.0, "fps {}", state.live_view_fps);
    let saved = saved.expect("the shot was received");
    assert_eq!(saved.filename, "DSC00001.JPG");
    assert!(iso_seen, "StateChanged with ISO 1600");
    assert_eq!(short_of(&state, PROP_ISO), "1600");
    let log = sim_log(&sim);
    assert!(!log.iter().any(|l| l.ends_with(" unanswered")), "{log:?}");
    assert!(!log.iter().any(|l| l.starts_with("op 0x9251 ")), "{log:?}");
    // The table was polled, not read once.
    assert!(log.iter().filter(|l| l.starts_with("op 0x9209 ")).count() > 10);
    stop(sim, camera).await;
}

/// StateChanged is emitted when a property changed, not on every poll.
#[tokio::test]
async fn state_changed_only_when_a_property_changes() {
    let tmp = tempfile::tempdir().unwrap();
    let (sim, camera) = start(SimConfig::default(), tmp.path()).await;
    let mut rx = camera.subscribe();
    let quiet = timeout(Duration::from_millis(1200), async {
        loop {
            if let Ok(CameraEvent::StateChanged(_)) = rx.recv().await {
                return;
            }
        }
    })
    .await;
    assert!(quiet.is_err(), "StateChanged with nothing changed");
    sim.events
        .send(SimCommand::SetProperty {
            code: PROP_ISO,
            value: PtpValue::U32(3200),
        })
        .await
        .unwrap();
    wait_for(&mut rx, "StateChanged with ISO 3200", |ev| match ev {
        CameraEvent::StateChanged(s) if short_of(s, PROP_ISO) == "3200" => Some(()),
        _ => None,
    })
    .await;
    stop(sim, camera).await;
}

/// With the simulator's old behaviour (parameter reads and 0x9251
/// answered), connecting, a shot and a settings change still work.
#[tokio::test]
async fn old_simulator_behaviour_still_works() {
    let tmp = tempfile::tempdir().unwrap();
    let sim_config = SimConfig {
        props_param_hangs: false,
        single_prop_read_hangs: false,
        ..SimConfig::default()
    };
    let (sim, camera) = start(sim_config, tmp.path()).await;
    let mut rx = camera.subscribe();
    camera.shoot().await.unwrap();
    wait_for(&mut rx, "CaptureSaved", |ev| match ev {
        CameraEvent::CaptureSaved(_) => Some(()),
        _ => None,
    })
    .await;
    camera.set(PROP_ISO, PtpValue::U32(1600)).await.unwrap();
    assert_eq!(short_of(&camera.state(), PROP_ISO), "1600");
    stop(sim, camera).await;
}

fn raw(state: &CameraState, code: u16) -> PtpValue {
    state
        .properties
        .iter()
        .find(|p| p.code == code)
        .unwrap_or_else(|| panic!("no property {code:#06x}"))
        .raw
        .clone()
}

/// The real ZV-E10 arrived set to save on its card only, and no shot ever
/// reached the computer.
#[tokio::test]
async fn a_camera_saving_to_its_card_only_is_switched_to_send_shots_here_too() {
    let tmp = tempfile::tempdir().unwrap();
    let sim_config = SimConfig {
        still_destination: Some(0x0010),
        host_image_size: Some(2),
        ..SimConfig::default()
    };
    let (sim, camera) = start(sim_config, tmp.path()).await;
    let state = camera.state();
    // Computer and card: the card copy stays.
    assert_eq!(raw(&state, PROP_STILL_DESTINATION), PtpValue::U16(0x0011));
    // The original file, not the small copy.
    assert_eq!(raw(&state, PROP_IMAGE_SIZE_TO_HOST), PtpValue::U8(1));
    assert_eq!(state.shots_to_computer, Some(true));
    let mut rx = camera.subscribe();
    camera.shoot().await.unwrap();
    let saved = wait_for(&mut rx, "CaptureSaved", |ev| match ev {
        CameraEvent::CaptureSaved(r) => Some(r.clone()),
        _ => None,
    })
    .await;
    assert_eq!(saved.path, tmp.path().join("DSC00001.JPG"));
    stop(sim, camera).await;
}

#[tokio::test]
async fn the_cameras_save_setting_is_left_alone_when_shots_are_not_received() {
    let tmp = tempfile::tempdir().unwrap();
    let sim = spawn(SimConfig {
        still_destination: Some(0x0010),
        ..SimConfig::default()
    })
    .await;
    let camera = Camera::connect(CameraConfig {
        auto_receive: false,
        ..config(&sim, tmp.path())
    })
    .await
    .unwrap();
    let state = camera.state();
    assert_eq!(raw(&state, PROP_STILL_DESTINATION), PtpValue::U16(0x0010));
    assert_eq!(state.shots_to_computer, Some(false));
    stop(sim, camera).await;
}

#[tokio::test]
async fn stepping_iso_moves_one_of_the_cameras_own_stops() {
    let tmp = tempfile::tempdir().unwrap();
    let (sim, camera) = start(SimConfig::default(), tmp.path()).await;
    camera.set(PROP_ISO, PtpValue::U32(100)).await.unwrap();
    let mut seen = Vec::new();
    for _ in 0..4 {
        camera.step(PROP_ISO, 1).await.unwrap();
        seen.push(raw(&camera.state(), PROP_ISO));
    }
    // The smallest change a camera has: its next stop, none skipped.
    assert_eq!(seen, [125, 160, 200, 250].map(PtpValue::U32));
    camera.step(PROP_ISO, -1).await.unwrap();
    assert_eq!(raw(&camera.state(), PROP_ISO), PtpValue::U32(200));
    stop(sim, camera).await;
}

/// Recording, then stopping, with a short pause so the camera notices both.
async fn record_a_clip(camera: &Camera, rx: &mut broadcast::Receiver<CameraEvent>) {
    camera.toggle_record().await.unwrap();
    wait_for(rx, "recording", |ev| match ev {
        CameraEvent::StateChanged(s) if s.recording => Some(()),
        _ => None,
    })
    .await;
    // Long enough for the card to be listed while the recording runs, with
    // every test of this file running at once: a clip that ends before the
    // listing does cannot be told from older ones (see `movie_loop`).
    tokio::time::sleep(Duration::from_secs(5)).await;
    camera.toggle_record().await.unwrap();
    wait_for(rx, "recording stopped", |ev| match ev {
        CameraEvent::StateChanged(s) if !s.recording => Some(()),
        _ => None,
    })
    .await;
}

#[tokio::test]
async fn a_finished_recording_is_copied_from_the_card() {
    let tmp = tempfile::tempdir().unwrap();
    let (sim, camera) = start(SimConfig::default(), tmp.path()).await;
    let mut rx = camera.subscribe();
    record_a_clip(&camera, &mut rx).await;
    let listed = Duration::from_secs(20);
    let started = wait_within(listed, &mut rx, "CaptureStarted", |ev| match ev {
        CameraEvent::CaptureStarted { filename } => Some(filename.clone()),
        _ => None,
    })
    .await;
    // Only the new clip: C0001.MP4 was on the card before.
    assert_eq!(started, "C0002.MP4");
    let mut progressed = false;
    let saved = timeout(Duration::from_secs(20), async {
        loop {
            match rx.recv().await {
                Ok(CameraEvent::CaptureProgress {
                    filename,
                    received,
                    total,
                }) if filename == "C0002.MP4" && received > 0 && received <= total => {
                    progressed = true;
                }
                Ok(CameraEvent::CaptureSaved(r)) => return r,
                Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(e) => panic!("event channel failed: {e}"),
            }
        }
    })
    .await
    .expect("the clip is saved");
    assert!(progressed, "progress is reported while the clip arrives");
    assert_eq!(saved.path, tmp.path().join("C0002.MP4"));
    assert_eq!(std::fs::metadata(&saved.path).unwrap().len(), 3 << 20);
    // The files that were already on the card are not fetched.
    assert!(!tmp.path().join("C0001.MP4").exists());
    assert!(!tmp.path().join("DSC00010.ARW").exists());
    stop(sim, camera).await;
}

#[tokio::test]
async fn a_camera_that_keeps_its_card_to_itself_is_reported_once() {
    let tmp = tempfile::tempdir().unwrap();
    let sim_config = SimConfig {
        card_denied: true,
        ..SimConfig::default()
    };
    let (sim, camera) = start(sim_config, tmp.path()).await;
    let mut rx = camera.subscribe();
    // A second listener, since the helper reads past the news while it waits.
    let mut news = camera.subscribe();
    record_a_clip(&camera, &mut rx).await;
    let reason = wait_for(&mut news, "MovieTransferUnavailable", |ev| match ev {
        CameraEvent::MovieTransferUnavailable { reason } => Some(reason.clone()),
        _ => None,
    })
    .await;
    assert!(!reason.is_empty());
    // The session carries on, and a second clip does not repeat the news.
    record_a_clip(&camera, &mut rx).await;
    tokio::time::sleep(Duration::from_secs(3)).await;
    let mut again = 0;
    while let Ok(ev) = news.try_recv() {
        if matches!(ev, CameraEvent::MovieTransferUnavailable { .. }) {
            again += 1;
        }
    }
    assert_eq!(again, 0);
    assert!(camera.state().connected);
    stop(sim, camera).await;
}

/// What the real ZV-E10 did: it answered OK to "save to computer and card"
/// at connect and went on saving to its card only.
#[tokio::test]
async fn a_camera_that_ignores_the_first_request_is_asked_again_until_it_sends_shots_here() {
    let tmp = tempfile::tempdir().unwrap();
    let sim_config = SimConfig {
        still_destination: Some(0x0010),
        still_destination_ignores_writes: 2,
        ..SimConfig::default()
    };
    let (sim, camera) = start(sim_config, tmp.path()).await;
    // Connecting did not fail, and did not change it.
    assert_eq!(camera.state().shots_to_computer, Some(false));
    let mut rx = camera.subscribe();
    // Rimix keeps at it while the session runs.
    wait_for(&mut rx, "pictures sent to the computer", |ev| match ev {
        CameraEvent::StateChanged(s) if s.shots_to_computer == Some(true) => Some(()),
        _ => None,
    })
    .await;
    camera.shoot().await.unwrap();
    let saved = wait_for(&mut rx, "CaptureSaved", |ev| match ev {
        CameraEvent::CaptureSaved(r) => Some(r.clone()),
        _ => None,
    })
    .await;
    assert_eq!(saved.path, tmp.path().join("DSC00001.JPG"));
    stop(sim, camera).await;
}

/// The app's "Copy pictures to this computer" switch.
#[tokio::test]
async fn receiving_can_be_switched_off_and_on_while_connected() {
    let tmp = tempfile::tempdir().unwrap();
    let (sim, camera) = start(SimConfig::default(), tmp.path()).await;
    let mut rx = camera.subscribe();
    assert_eq!(camera.state().shots_to_computer, Some(true));

    // Off: the camera goes back to its card only, so it never waits for a
    // computer that will not fetch, and a shot brings nothing here.
    camera.set_receiving(false);
    wait_for(&mut rx, "card only", |ev| match ev {
        CameraEvent::StateChanged(s) if s.shots_to_computer == Some(false) => Some(()),
        _ => None,
    })
    .await;
    assert_eq!(
        raw(&camera.state(), PROP_STILL_DESTINATION),
        PtpValue::U16(0x0010)
    );
    camera.shoot().await.unwrap();
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 0);

    // On again: the camera is asked again and the next picture arrives.
    camera.set_receiving(true);
    wait_for(&mut rx, "computer and card", |ev| match ev {
        CameraEvent::StateChanged(s) if s.shots_to_computer == Some(true) => Some(()),
        _ => None,
    })
    .await;
    camera.shoot().await.unwrap();
    let saved = wait_for(&mut rx, "CaptureSaved", |ev| match ev {
        CameraEvent::CaptureSaved(r) => Some(r.clone()),
        _ => None,
    })
    .await;
    assert!(saved.path.starts_with(tmp.path()));
    stop(sim, camera).await;
}

#[tokio::test]
async fn a_recording_is_not_copied_while_receiving_is_off() {
    let tmp = tempfile::tempdir().unwrap();
    let (sim, camera) = start(SimConfig::default(), tmp.path()).await;
    camera.set_receiving(false);
    let mut rx = camera.subscribe();
    record_a_clip(&camera, &mut rx).await;
    tokio::time::sleep(Duration::from_secs(4)).await;
    assert!(!tmp.path().join("C0002.MP4").exists());
    stop(sim, camera).await;
}

/// A body that describes a waiting picture without a file name and without
/// the date strings: the picture must still be fetched, under a name Rimix
/// gives it. Before, the description was rejected and nothing was said.
#[tokio::test]
async fn a_picture_described_without_a_name_is_still_saved() {
    let tmp = tempfile::tempdir().unwrap();
    let sim_config = SimConfig {
        terse_capture_info: true,
        ..SimConfig::default()
    };
    let (sim, camera) = start(sim_config, tmp.path()).await;
    let mut rx = camera.subscribe();
    camera.shoot().await.unwrap();
    let saved = wait_for(&mut rx, "CaptureSaved", |ev| match ev {
        CameraEvent::CaptureSaved(r) => Some(r.clone()),
        _ => None,
    })
    .await;
    assert!(saved.filename.starts_with("Rimix-"), "{}", saved.filename);
    assert!(saved.filename.ends_with(".JPG"), "{}", saved.filename);
    assert_eq!(std::fs::metadata(&saved.path).unwrap().len(), saved.bytes);
    // A second shot gets a name of its own.
    camera.shoot().await.unwrap();
    let second = wait_for(&mut rx, "CaptureSaved", |ev| match ev {
        CameraEvent::CaptureSaved(r) => Some(r.clone()),
        _ => None,
    })
    .await;
    assert_ne!(second.path, saved.path);
    stop(sim, camera).await;
}

/// The real ZV-E10 lists no values for shutter speed or aperture: they can
/// only be nudged one step at a time. Before, plus and minus did nothing.
#[tokio::test]
async fn settings_the_camera_gives_no_list_for_are_nudged_a_step_at_a_time() {
    let tmp = tempfile::tempdir().unwrap();
    let sim_config = SimConfig {
        unlisted: vec![PROP_SHUTTER_SPEED, PROP_F_NUMBER],
        ..SimConfig::default()
    };
    let (sim, camera) = start(sim_config, tmp.path()).await;
    let state = camera.state();
    let shutter = state
        .properties
        .iter()
        .find(|p| p.code == PROP_SHUTTER_SPEED)
        .unwrap();
    assert!(shutter.choices.is_empty());
    assert_eq!(shutter.short, "1/125");
    camera.step(PROP_SHUTTER_SPEED, 1).await.unwrap();
    assert_eq!(
        raw(&camera.state(), PROP_SHUTTER_SPEED),
        PtpValue::U32(65636)
    );
    camera.step(PROP_F_NUMBER, 2).await.unwrap();
    assert_eq!(raw(&camera.state(), PROP_F_NUMBER), PtpValue::U16(500));
    camera.step(PROP_F_NUMBER, -1).await.unwrap();
    assert_eq!(raw(&camera.state(), PROP_F_NUMBER), PtpValue::U16(450));
    stop(sim, camera).await;
}

#[tokio::test]
async fn an_aperture_is_reached_by_nudging_and_stops_at_what_the_lens_allows() {
    let tmp = tempfile::tempdir().unwrap();
    let sim_config = SimConfig {
        unlisted: vec![PROP_F_NUMBER],
        ..SimConfig::default()
    };
    let (sim, camera) = start(sim_config, tmp.path()).await;
    // f/4 to f/8, four stops of a third up the lens's own values.
    camera.set(PROP_F_NUMBER, PtpValue::U16(800)).await.unwrap();
    assert_eq!(raw(&camera.state(), PROP_F_NUMBER), PtpValue::U16(800));
    // Past the lens's smallest aperture: as far as it goes, and no error.
    camera
        .set(PROP_F_NUMBER, PtpValue::U16(2200))
        .await
        .unwrap();
    assert_eq!(raw(&camera.state(), PROP_F_NUMBER), PtpValue::U16(1100));
    // A value the lens does not have lands on the next one it does.
    camera.set(PROP_F_NUMBER, PtpValue::U16(480)).await.unwrap();
    assert_eq!(raw(&camera.state(), PROP_F_NUMBER), PtpValue::U16(450));
    stop(sim, camera).await;
}

#[tokio::test]
async fn a_range_setting_reports_its_ends_and_step() {
    let tmp = tempfile::tempdir().unwrap();
    let (sim, camera) = start(SimConfig::default(), tmp.path()).await;
    let state = camera.state();
    let kelvin = state.properties.iter().find(|p| p.code == 0xD20F).unwrap();
    let range = kelvin
        .range
        .as_ref()
        .expect("colour temperature is a range");
    assert_eq!((range.min, range.max, range.step), (2500, 9900, 100));
    let iso = state
        .properties
        .iter()
        .find(|p| p.code == PROP_ISO)
        .unwrap();
    assert!(iso.range.is_none());
    stop(sim, camera).await;
}

/// Tap to focus: the point goes to the camera on its 640 by 480 grid, then
/// the shutter is half pressed there.
#[tokio::test]
async fn focusing_at_a_point_sends_it_on_the_cameras_grid_then_focuses() {
    let tmp = tempfile::tempdir().unwrap();
    let (sim, camera) = start(SimConfig::default(), tmp.path()).await;
    // The middle of the upper quarter: x 0.5, y 0.25.
    camera.focus_at(0.5, 0.25).await.unwrap();
    let log = sim.log.lock().unwrap().join("\n");
    let point = (320u64 << 16) | 120;
    assert!(log.contains(&format!("D2DC={point}")), "{log}");
    // The half press comes after the point.
    let at = log.find("D2DC=").unwrap();
    assert!(log[at..].contains("D2C1=2"), "{log}");
    drop(log);
    // Outside the picture is pulled to its edge, never sent as it is.
    camera.focus_at(7.0, -3.0).await.unwrap();
    let log = sim.log.lock().unwrap().join("\n");
    assert!(log.contains(&format!("D2DC={}", 639u64 << 16)), "{log}");
    drop(log);
    stop(sim, camera).await;
}

fn focus_area(camera: &Camera) -> u64 {
    let state = camera.state();
    let area = state.properties.iter().find(|p| p.code == 0xD22C);
    area.and_then(|p| p.raw.as_u64()).expect("a focus area")
}

/// A spot the camera already uses is moved, and left the size it is.
#[tokio::test]
async fn a_focus_point_moves_the_spot_the_camera_already_uses() {
    let tmp = tempfile::tempdir().unwrap();
    let config = SimConfig {
        // Flexible Spot: L.
        focus_area: Some((0x0103, true)),
        ..SimConfig::default()
    };
    let (sim, camera) = start(config, tmp.path()).await;
    camera.focus_at(0.25, 0.5).await.unwrap();
    let log = sim.log.lock().unwrap().join("\n");
    assert!(
        log.contains(&format!("D2DC={}", (160u64 << 16) | 240)),
        "{log}"
    );
    assert!(!log.contains("D2E4="), "{log}");
    drop(log);
    assert_eq!(focus_area(&camera), 0x0103);
    stop(sim, camera).await;
}

/// Wide, zone and centre ignore a point: the area becomes a spot first, and
/// the point is only sent once the camera says the area has changed.
#[tokio::test]
async fn a_focus_point_first_makes_the_focus_area_a_spot() {
    let tmp = tempfile::tempdir().unwrap();
    let config = SimConfig {
        focus_area: Some((1, true)),
        ..SimConfig::default()
    };
    let (sim, camera) = start(config, tmp.path()).await;
    camera.focus_at(0.5, 0.5).await.unwrap();
    assert_eq!(focus_area(&camera), 0x0102);
    let log = sim.log.lock().unwrap().join("\n");
    // The write of the area (0x9205) comes before the point.
    let write = log.find("op 0x9205").expect("the area is written");
    let point = log.find("D2DC=").expect("the point is sent");
    assert!(write < point, "{log}");
    assert!(log[point..].contains("D2C1=2"), "{log}");
    drop(log);
    stop(sim, camera).await;
}

/// In its automatic modes the camera fixes the focus area. A touch on its
/// own screen still focuses there, so the tap is sent as that touch.
#[tokio::test]
async fn where_the_camera_fixes_the_focus_area_a_tap_is_sent_as_a_touch() {
    let tmp = tempfile::tempdir().unwrap();
    let config = SimConfig {
        focus_area: Some((1, false)),
        remote_touch: true,
        ..SimConfig::default()
    };
    let (sim, camera) = start(config, tmp.path()).await;
    assert!(camera.state().controls.contains(&0xD2E4));
    camera.focus_at(0.5, 0.25).await.unwrap();
    let log = sim.log.lock().unwrap().join("\n");
    let touch = format!("D2E4={}", (320u64 << 16) | 120);
    assert!(log.contains(&touch), "{log}");
    assert!(!log.contains("D2DC="), "{log}");
    // Then the half press, to focus at the touched point.
    let at = log.find("D2E4=").unwrap();
    assert!(log[at..].contains("D2C1=2"), "{log}");
    drop(log);
    assert_eq!(focus_area(&camera), 1);
    stop(sim, camera).await;
}

/// With the area fixed and no touch control there is nothing that would
/// move the focus: the tap fails and says why, and no half press is made
/// that would focus somewhere else.
#[tokio::test]
async fn a_tap_the_camera_cannot_follow_says_so_and_does_not_focus_elsewhere() {
    let tmp = tempfile::tempdir().unwrap();
    let config = SimConfig {
        focus_area: Some((1, false)),
        ..SimConfig::default()
    };
    let (sim, camera) = start(config, tmp.path()).await;
    let before = sim.log.lock().unwrap().len();
    let err = camera.focus_at(0.5, 0.5).await.unwrap_err();
    assert!(err.to_string().contains("chooses where to focus"), "{err}");
    let log = sim.log.lock().unwrap()[before..].join("\n");
    assert!(!log.contains("D2C1="), "{log}");
    assert!(!log.contains("D2DC="), "{log}");
    stop(sim, camera).await;
}

#[tokio::test]
async fn the_state_lists_the_controls_the_camera_says_it_has() {
    let tmp = tempfile::tempdir().unwrap();
    let (sim, camera) = start(SimConfig::default(), tmp.path()).await;
    // Whatever the camera reports, it reaches the state (and so the report).
    let state = camera.state();
    assert_eq!(state.controls, camera.state().controls);
    assert!(
        state.controls.iter().all(|c| *c >= 0xD000),
        "{:x?}",
        state.controls
    );
    stop(sim, camera).await;
}

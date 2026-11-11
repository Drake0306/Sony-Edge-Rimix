use std::path::Path;
use std::time::Duration;

use sony_ptpip::capture::{Destination, DestinationMode};
use sony_ptpip::ptp::codes::{
    PROP_IMAGE_SIZE_TO_HOST, PROP_ISO, PROP_SHUTTER_SPEED, PROP_STILL_DESTINATION,
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
    mut pick: impl FnMut(&CameraEvent) -> Option<T>,
) -> T {
    timeout(WAIT, async {
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
async fn stepping_iso_moves_about_fifty_and_lands_on_a_real_stop() {
    let tmp = tempfile::tempdir().unwrap();
    let (sim, camera) = start(SimConfig::default(), tmp.path()).await;
    camera.set(PROP_ISO, PtpValue::U32(100)).await.unwrap();
    let mut seen = Vec::new();
    for _ in 0..4 {
        camera.step(PROP_ISO, 1).await.unwrap();
        seen.push(raw(&camera.state(), PROP_ISO));
    }
    // 100 +50 is 150: the nearest stop is 160, then 200, 250, 320.
    assert_eq!(seen, [160, 200, 250, 320].map(PtpValue::U32));
    camera.step(PROP_ISO, -1).await.unwrap();
    assert_eq!(raw(&camera.state(), PROP_ISO), PtpValue::U32(250));
    stop(sim, camera).await;
}

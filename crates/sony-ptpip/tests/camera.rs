use std::path::Path;
use std::time::Duration;

use sony_ptpip::capture::{Destination, DestinationMode};
use sony_ptpip::ptp::codes::{PROP_ISO, PROP_SHUTTER_SPEED};
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
        },
        auto_receive: true,
        reconnect: true,
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

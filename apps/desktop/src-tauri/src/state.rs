//! What the commands do, as plain async functions over [`AppState`] so they
//! can be tested without a window. `commands.rs` wraps each one.

use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sony_ptpip::discovery::PTP_PORT;
use sony_ptpip::ptp::PtpValue;
use sony_ptpip::{Camera, CameraConfig, CameraEvent, CameraState, DiscoveredCamera};
use tokio::sync::broadcast::{self, error::RecvError};

use crate::settings::{self, AppSettings};

/// The error every camera command gives while no camera is connected.
pub const NOT_CONNECTED: &str = "Not connected to a camera";

/// Managed Tauri state: the camera (when connected) and the settings.
pub struct AppState {
    /// Held in an `Arc` so a long command (a shot) does not keep the lock
    /// and stall live view polling.
    pub camera: tokio::sync::Mutex<Option<Arc<Camera>>>,
    pub settings: StdMutex<AppSettings>,
    pub guid: [u8; 16],
    pub settings_path: PathBuf,
}

/// Locks a std mutex, carrying on if a panicking holder poisoned it.
fn lock<T>(m: &StdMutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl AppState {
    /// Starts with no camera and the settings stored at `settings_path`.
    pub fn new(guid: [u8; 16], settings_path: PathBuf) -> Self {
        let settings = settings::load(&settings_path);
        AppState {
            camera: tokio::sync::Mutex::new(None),
            settings: StdMutex::new(settings),
            guid,
            settings_path,
        }
    }

    async fn camera(&self) -> Result<Arc<Camera>, String> {
        self.camera
            .lock()
            .await
            .clone()
            .ok_or_else(|| NOT_CONNECTED.to_string())
    }
}

/// The camera's PTP/IP address from what the user typed: an IP address
/// (port 15740) or `ip:port`.
pub fn parse_addr(ip: &str) -> Result<SocketAddr, String> {
    let ip = ip.trim();
    if let Ok(addr) = ip.parse::<SocketAddr>() {
        return Ok(addr);
    }
    ip.parse::<IpAddr>()
        .map(|ip| SocketAddr::new(ip, PTP_PORT))
        .map_err(|_| format!("Not an IP address: {ip}"))
}

/// "Rimix on <hostname>", shown on the camera's pairing prompt.
fn host_name() -> String {
    match hostname::get() {
        Ok(name) if !name.is_empty() => format!("Rimix on {}", name.to_string_lossy()),
        _ => "Rimix".into(),
    }
}

pub async fn discover(timeout_ms: u64) -> Result<Vec<DiscoveredCamera>, String> {
    sony_ptpip::discover(Duration::from_millis(timeout_ms))
        .await
        .map_err(|e| e.to_string())
}

/// Connects to the camera at `ip`, replacing any camera already connected,
/// and remembers it in the settings. Returns its state and a subscription
/// to its events for [`forward_events`].
pub async fn connect(
    state: &AppState,
    ip: &str,
) -> Result<(CameraState, broadcast::Receiver<CameraEvent>), String> {
    let addr = parse_addr(ip)?;
    let previous = state.camera.lock().await.take();
    if let Some(old) = previous {
        old.disconnect().await;
    }
    let destination = lock(&state.settings).destination();
    let camera = Camera::connect(CameraConfig {
        addr,
        guid: state.guid,
        host_name: host_name(),
        destination,
        auto_receive: true,
        reconnect: true,
    })
    .await
    .map_err(|e| format!("Cannot connect to {addr}: {e}"))?;
    let events = camera.subscribe();
    let camera_state = camera.state();
    // Another connect may have finished while this one waited.
    let replaced = state.camera.lock().await.replace(Arc::new(camera));
    if let Some(old) = replaced {
        old.disconnect().await;
    }
    let snapshot = {
        let mut s = lock(&state.settings);
        s.remember_camera(ip.trim(), &camera_state.model, now_millis());
        s.clone()
    };
    if let Err(e) = settings::save(&state.settings_path, &snapshot) {
        tracing::warn!(error = %e, "cannot save settings after connecting");
    }
    Ok((camera_state, events))
}

/// Hands every event except `Frame` to `emit` (the UI polls frames instead)
/// until the camera is disconnected or replaced and its channel closes.
pub async fn forward_events(
    mut rx: broadcast::Receiver<CameraEvent>,
    mut emit: impl FnMut(&CameraEvent),
) {
    loop {
        match rx.recv().await {
            Ok(CameraEvent::Frame) => {}
            Ok(event) => emit(&event),
            Err(RecvError::Lagged(missed)) => {
                tracing::warn!(missed, "camera events fell behind");
            }
            Err(RecvError::Closed) => break,
        }
    }
}

pub async fn disconnect(state: &AppState) -> Result<(), String> {
    let camera = state.camera.lock().await.take();
    if let Some(camera) = camera {
        camera.disconnect().await;
    }
    Ok(())
}

pub async fn get_state(state: &AppState) -> Option<CameraState> {
    state.camera.lock().await.as_ref().map(|c| c.state())
}

/// Sets property `code` to `raw`, a PTP value as serde writes it, e.g.
/// `{"U32": 1600}`.
pub async fn set_property(
    state: &AppState,
    code: u16,
    raw: serde_json::Value,
) -> Result<(), String> {
    let value: PtpValue = serde_json::from_value(raw).map_err(|e| format!("Invalid value: {e}"))?;
    let camera = state.camera().await?;
    camera.set(code, value).await.map_err(|e| e.to_string())
}

pub async fn step_property(state: &AppState, code: u16, delta: i32) -> Result<(), String> {
    let camera = state.camera().await?;
    camera.step(code, delta).await.map_err(|e| e.to_string())
}

pub async fn shoot(state: &AppState) -> Result<(), String> {
    let camera = state.camera().await?;
    camera.shoot().await.map_err(|e| e.to_string())
}

pub async fn focus(state: &AppState) -> Result<(), String> {
    let camera = state.camera().await?;
    camera.focus().await.map_err(|e| e.to_string())
}

pub async fn toggle_record(state: &AppState) -> Result<(), String> {
    let camera = state.camera().await?;
    camera.toggle_record().await.map_err(|e| e.to_string())
}

/// The latest live view JPEG; empty when there is none or no camera.
pub async fn live_view_frame(state: &AppState) -> Result<Vec<u8>, String> {
    let camera = state.camera.lock().await.clone();
    Ok(camera
        .and_then(|c| c.latest_frame())
        .map(|f| f.jpeg.clone())
        .unwrap_or_default())
}

/// Focus frames and level for the latest frame, and the frame rate:
/// `{ "focus_frames": [...], "level_deg": number|null, "fps": number }`.
pub async fn live_view_meta(state: &AppState) -> Result<serde_json::Value, String> {
    let camera = state.camera.lock().await.clone();
    let frame = camera.as_ref().and_then(|c| c.latest_frame());
    let fps = camera.map_or(0.0, |c| c.state().live_view_fps);
    Ok(serde_json::json!({
        "focus_frames": frame.as_ref().map(|f| f.focus_frames.clone()).unwrap_or_default(),
        "level_deg": frame.and_then(|f| f.level_deg),
        "fps": fps,
    }))
}

pub fn get_settings(state: &AppState) -> AppSettings {
    lock(&state.settings).clone()
}

/// Stores the settings and points a connected camera at the new destination.
pub async fn save_settings(state: &AppState, s: AppSettings) -> Result<(), String> {
    settings::save(&state.settings_path, &s).map_err(|e| format!("Cannot save settings: {e}"))?;
    let destination = s.destination();
    *lock(&state.settings) = s;
    if let Some(camera) = state.camera.lock().await.as_ref() {
        camera.set_destination(destination);
    }
    Ok(())
}

/// Thumbnails larger than this are not sent to the webview.
pub const MAX_THUMBNAIL_BYTES: u64 = 25 * 1024 * 1024;
pub const NOT_A_JPEG: &str = "Only JPEG files have a thumbnail";
pub const TOO_LARGE: &str = "The file is too large for a thumbnail";

/// The bytes of a saved JPEG for the filmstrip. RAW and other files, and
/// files over [`MAX_THUMBNAIL_BYTES`], are refused (the UI shows a tile).
pub fn read_thumbnail(path: &Path) -> Result<Vec<u8>, String> {
    let is_jpeg = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("jpg") || e.eq_ignore_ascii_case("jpeg"));
    if !is_jpeg {
        return Err(NOT_A_JPEG.into());
    }
    let cannot = |e: std::io::Error| format!("Cannot read {}: {e}", path.display());
    let len = std::fs::metadata(path).map_err(cannot)?.len();
    if len > MAX_THUMBNAIL_BYTES {
        return Err(TOO_LARGE.into());
    }
    std::fs::read(path).map_err(cannot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sony_ptpip::capture::DestinationMode;
    use sony_ptpip::ptp::codes::{PROP_ISO, PROP_SHUTTER_SPEED};
    use sony_ptpip::{Choice, PropertySummary};
    use sony_ptpip_sim::{spawn, SimCommand, SimConfig, SimHandle};
    use std::time::Duration;
    use tokio::time::timeout;

    /// Longest wait for anything the simulator should do.
    const WAIT: Duration = Duration::from_secs(5);

    /// An app state whose settings live in `dir` and whose shots go there.
    fn app_state(dir: &Path) -> AppState {
        let path = dir.join("config").join("settings.json");
        let settings = AppSettings {
            destination_base: dir.join("shots"),
            destination_mode: DestinationMode::CustomFolder,
            ..AppSettings::default()
        };
        settings::save(&path, &settings).unwrap();
        AppState::new([0x42; 16], path)
    }

    async fn connected(dir: &Path) -> (SimHandle, AppState, CameraState) {
        let sim = spawn(SimConfig::default()).await;
        let state = app_state(dir);
        let (camera_state, _rx) = connect(&state, &sim.addr.to_string()).await.unwrap();
        (sim, state, camera_state)
    }

    async fn stop(sim: SimHandle, state: &AppState) {
        disconnect(state).await.unwrap();
        sim.events.send(SimCommand::Shutdown).await.unwrap();
    }

    fn short_of(state: &CameraState, code: u16) -> String {
        let prop = state.properties.iter().find(|p| p.code == code).unwrap();
        prop.short.clone()
    }

    #[test]
    fn addresses_take_the_ptp_port_unless_given() {
        assert_eq!(
            parse_addr("192.168.122.1").unwrap(),
            "192.168.122.1:15740".parse().unwrap()
        );
        assert_eq!(
            parse_addr(" 127.0.0.1:9000 ").unwrap(),
            "127.0.0.1:9000".parse().unwrap()
        );
        assert!(parse_addr("camera.local").is_err());
    }

    #[tokio::test]
    async fn connect_returns_state_and_remembers_the_camera() {
        let tmp = tempfile::tempdir().unwrap();
        let (sim, state, camera_state) = connected(tmp.path()).await;
        assert!(camera_state.connected);
        assert_eq!(camera_state.model, "ZV-E10");
        let ip = sim.addr.to_string();
        let saved = settings::load(&state.settings_path);
        assert_eq!(saved.last_ip.as_deref(), Some(ip.as_str()));
        assert_eq!(saved.recent_cameras[0].ip, ip);
        assert_eq!(saved.recent_cameras[0].model, "ZV-E10");
        assert_eq!(get_settings(&state), saved);
        assert!(get_state(&state).await.unwrap().connected);
        stop(sim, &state).await;
        assert_eq!(get_state(&state).await, None);
    }

    #[tokio::test]
    async fn commands_fail_plainly_when_not_connected() {
        let tmp = tempfile::tempdir().unwrap();
        let state = app_state(tmp.path());
        assert_eq!(shoot(&state).await.unwrap_err(), NOT_CONNECTED);
        assert!(live_view_frame(&state).await.unwrap().is_empty());
        let meta = live_view_meta(&state).await.unwrap();
        assert_eq!(
            meta,
            serde_json::json!({ "focus_frames": [], "level_deg": null, "fps": 0.0 })
        );
        disconnect(&state).await.unwrap();
    }

    #[tokio::test]
    async fn set_and_step_property_change_the_camera() {
        let tmp = tempfile::tempdir().unwrap();
        let (sim, state, before) = connected(tmp.path()).await;
        set_property(&state, PROP_ISO, serde_json::json!({ "U32": 1600 }))
            .await
            .unwrap();
        let after = get_state(&state).await.unwrap();
        assert_eq!(short_of(&after, PROP_ISO), "1600");
        // A value that is not a PTP value is refused before the camera.
        let err = set_property(&state, PROP_ISO, serde_json::json!(1600))
            .await
            .unwrap_err();
        assert!(err.starts_with("Invalid value"), "{err}");

        assert_eq!(short_of(&before, PROP_SHUTTER_SPEED), "1/125");
        step_property(&state, PROP_SHUTTER_SPEED, 1).await.unwrap();
        let stepped = get_state(&state).await.unwrap();
        assert_ne!(short_of(&stepped, PROP_SHUTTER_SPEED), "1/125");
        stop(sim, &state).await;
    }

    #[tokio::test]
    async fn shoot_saves_into_the_destination_from_settings() {
        let tmp = tempfile::tempdir().unwrap();
        let (sim, state, _) = connected(tmp.path()).await;
        shoot(&state).await.unwrap();
        assert!(get_state(&state).await.unwrap().last_shot_at.is_some());
        let shots = tmp.path().join("shots");
        timeout(WAIT, async {
            while std::fs::read_dir(&shots).map_or(0, |d| d.count()) == 0 {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("no shot saved");
        focus(&state).await.unwrap();
        stop(sim, &state).await;
    }

    #[tokio::test]
    async fn live_view_frame_is_a_jpeg_with_meta() {
        let tmp = tempfile::tempdir().unwrap();
        let (sim, state, _) = connected(tmp.path()).await;
        let jpeg = timeout(WAIT, async {
            loop {
                let jpeg = live_view_frame(&state).await.unwrap();
                if !jpeg.is_empty() {
                    return jpeg;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("no live view frame");
        assert_eq!(&jpeg[..2], &[0xFF, 0xD8]);
        let meta = live_view_meta(&state).await.unwrap();
        assert!(meta["focus_frames"].is_array(), "{meta}");
        assert!(meta["fps"].is_number(), "{meta}");
        assert!(meta.get("level_deg").is_some(), "{meta}");
        stop(sim, &state).await;
    }

    #[tokio::test]
    async fn forwarding_skips_frames_and_ends_on_disconnect() {
        let tmp = tempfile::tempdir().unwrap();
        let sim = spawn(SimConfig::default()).await;
        let state = app_state(tmp.path());
        let (_, rx) = connect(&state, &sim.addr.to_string()).await.unwrap();
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = seen.clone();
        let forwarder = tokio::spawn(forward_events(rx, move |ev| {
            sink.lock().unwrap().push(ev.clone())
        }));
        // Let some live view frames go by, then change something.
        tokio::time::sleep(Duration::from_millis(300)).await;
        set_property(&state, PROP_ISO, serde_json::json!({ "U32": 1600 }))
            .await
            .unwrap();
        stop(sim, &state).await;
        timeout(WAIT, forwarder)
            .await
            .expect("forwarding did not end after disconnect")
            .unwrap();
        let seen = seen.lock().unwrap();
        assert!(!seen.contains(&CameraEvent::Frame));
        assert!(seen
            .iter()
            .any(|ev| matches!(ev, CameraEvent::StateChanged(s)
            if s.properties.iter().any(|p| p.code == PROP_ISO && p.short == "1600"))));
        assert!(seen
            .iter()
            .any(|ev| matches!(ev, CameraEvent::Disconnected { .. })));
    }

    #[tokio::test]
    async fn save_settings_stores_them_and_moves_the_destination() {
        let tmp = tempfile::tempdir().unwrap();
        let (sim, state, _) = connected(tmp.path()).await;
        let mut s = get_settings(&state);
        s.destination_base = tmp.path().join("elsewhere");
        save_settings(&state, s.clone()).await.unwrap();
        assert_eq!(settings::load(&state.settings_path), s);
        assert_eq!(get_settings(&state), s);
        shoot(&state).await.unwrap();
        let moved = tmp.path().join("elsewhere");
        timeout(WAIT, async {
            while std::fs::read_dir(&moved).map_or(0, |d| d.count()) == 0 {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("shot not saved in the new destination");
        stop(sim, &state).await;
    }

    #[test]
    fn read_thumbnail_returns_small_jpegs_only() {
        let tmp = tempfile::tempdir().unwrap();
        let jpg = tmp.path().join("DSC00001.JPG");
        std::fs::write(&jpg, [0xFF, 0xD8, 0xFF, 0xD9]).unwrap();
        assert_eq!(read_thumbnail(&jpg).unwrap(), vec![0xFF, 0xD8, 0xFF, 0xD9]);
        let lower = tmp.path().join("b.jpeg");
        std::fs::write(&lower, [1, 2]).unwrap();
        assert_eq!(read_thumbnail(&lower).unwrap(), vec![1, 2]);

        let raw = tmp.path().join("DSC00002.ARW");
        std::fs::write(&raw, [1, 2, 3]).unwrap();
        assert_eq!(read_thumbnail(&raw).unwrap_err(), NOT_A_JPEG);

        let big = tmp.path().join("big.jpg");
        std::fs::File::create(&big)
            .unwrap()
            .set_len(MAX_THUMBNAIL_BYTES + 1)
            .unwrap();
        assert_eq!(read_thumbnail(&big).unwrap_err(), TOO_LARGE);

        let missing = read_thumbnail(&tmp.path().join("gone.jpg")).unwrap_err();
        assert!(missing.starts_with("Cannot read"), "{missing}");
    }

    /// Pins the JSON the webview receives for a state change. The same
    /// string is the fixture in `src/fixtures/state-changed.json`, which
    /// `src/ipc.test.ts` checks against the TypeScript type guards.
    #[test]
    fn state_changed_event_json_is_stable() {
        let event = CameraEvent::StateChanged(CameraState {
            connected: true,
            model: "ZV-E10".into(),
            firmware: "2.00".into(),
            battery: Some(72),
            recording: false,
            focus_found: false,
            properties: vec![PropertySummary {
                code: 0x500D,
                label: "1/125".into(),
                short: "1/125".into(),
                raw: PtpValue::U32(0x0001_007D),
                writable: true,
                enabled: sony_ptpip::sony::property::Enabled::On,
                choices: vec![Choice {
                    raw: PtpValue::U32(0x0001_007D),
                    label: "1/125".into(),
                }],
            }],
            live_view_fps: 29.5,
            last_shot_at: None,
        });
        assert_eq!(
            serde_json::to_string(&event).unwrap(),
            STATE_CHANGED_FIXTURE.trim()
        );
        assert_eq!(
            serde_json::to_string(&CameraEvent::Connected).unwrap(),
            r#""Connected""#
        );
    }

    const STATE_CHANGED_FIXTURE: &str = include_str!("../../src/fixtures/state-changed.json");
}

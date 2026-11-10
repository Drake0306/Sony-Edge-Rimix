//! What the commands do, as plain async functions over [`AppState`] so they
//! can be tested without a window. `commands.rs` wraps each one.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sony_ptpip::discovery::{DiscoverySource, PTP_PORT};
use sony_ptpip::error::Error;
use sony_ptpip::ptp::PtpValue;
use sony_ptpip::{
    Camera, CameraConfig, CameraEvent, CameraState, DiscoveredCamera, ReconnectHook, SshConfig,
};
use tokio::sync::broadcast::{self, error::RecvError};

use crate::settings::{self, AppSettings, PresetValue};
use rimix_wifi::{SystemRunner, Wifi};

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
    pub wifi: Wifi,
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
            wifi: Wifi::from_env(Arc::new(SystemRunner)),
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

pub async fn discover(timeout_ms: u64) -> Result<Vec<DiscoveredCamera>, String> {
    sony_ptpip::discover(Duration::from_millis(timeout_ms))
        .await
        .map_err(|e| e.to_string())
}

/// The error [`wait_for_camera`] gives when nothing answered in time.
pub const NO_CAMERA: &str = "No camera answered on this network";

/// Joins the Wi-Fi network `ssid`, waiting until it is up.
pub async fn join_network(
    state: &AppState,
    ssid: &str,
    password: Option<&str>,
) -> Result<(), String> {
    let ssid = ssid.to_string();
    let password = password.map(str::to_string);
    let wifi = state.wifi.clone();
    tokio::task::spawn_blocking(move || wifi.join(&ssid, password.as_deref()))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// The Wi-Fi network this computer is on, if any.
pub async fn current_network(state: &AppState) -> Result<Option<String>, String> {
    let wifi = state.wifi.clone();
    tokio::task::spawn_blocking(move || wifi.current_network())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Waits up to `timeout` for a camera to answer. `RIMIX_CAMERA_ADDR`
/// (`host:port`, port 15740 by default) stands in for one during development.
pub async fn wait_for_camera(timeout: Duration) -> Result<DiscoveredCamera, String> {
    wait_for_camera_with(sony_ptpip::discover, timeout).await
}

/// How long one discovery round listens, and so how often it repeats.
const DISCOVER_ROUND: Duration = Duration::from_millis(1500);

/// [`wait_for_camera`] over any discovery function, so tests need no network.
pub(crate) async fn wait_for_camera_with<F, Fut>(
    discover: F,
    timeout: Duration,
) -> Result<DiscoveredCamera, String>
where
    F: Fn(Duration) -> Fut,
    Fut: std::future::Future<Output = Result<Vec<DiscoveredCamera>, sony_ptpip::error::Error>>,
{
    if let Ok(addr) = std::env::var("RIMIX_CAMERA_ADDR") {
        let addr = parse_addr(&addr)?;
        return Ok(DiscoveredCamera {
            model: "Simulator".to_string(),
            friendly_name: "Simulator".to_string(),
            ip: addr.ip(),
            ptp_port: addr.port(),
            ptp_version: None,
            pairing_required: None,
            ssh_supported: None,
            source: DiscoverySource::Manual,
        });
    }
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let round_start = tokio::time::Instant::now();
        let found = discover(DISCOVER_ROUND).await.unwrap_or_default();
        let pick = found
            .iter()
            .position(|c| c.source == DiscoverySource::Gateway)
            .unwrap_or(0);
        if let Some(camera) = found.into_iter().nth(pick) {
            return Ok(camera);
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(NO_CAMERA.to_string());
        }
        // A round that failed at once (no address yet) must not spin.
        tokio::time::sleep_until(round_start + DISCOVER_ROUND).await;
    }
}

/// Whether two IPv4 addresses share their first three octets, as the
/// camera and a computer on its Wi-Fi Direct network do.
fn same_slash_24(a: Ipv4Addr, b: Ipv4Addr) -> bool {
    a.octets()[..3] == b.octets()[..3]
}

/// What one rejoin before a reconnect attempt did.
#[derive(Debug, PartialEq, Eq)]
enum Rejoin {
    /// The Wi-Fi already had an address on the camera's network.
    Skipped,
    Joined,
    Failed(String),
}

/// Rejoins `ssid` unless the Wi-Fi already has an address in the camera's
/// /24, stopping early once `cancel` is set. Blocks: run it off the async
/// runtime.
fn rejoin_blocking(
    wifi: &Wifi,
    ssid: &str,
    camera: Option<Ipv4Addr>,
    cancel: &AtomicBool,
) -> Rejoin {
    if let Some(camera) = camera {
        match wifi.current_address() {
            Ok(Some(ours)) if same_slash_24(ours, camera) => {
                tracing::info!(%ours, %camera, "wifi link looks fine, skipping rejoin");
                return Rejoin::Skipped;
            }
            Ok(_) => {}
            Err(e) => tracing::debug!(error = %e, "cannot read the Wi-Fi address"),
        }
    }
    match wifi.join_with_cancel(ssid, None, cancel) {
        Ok(_) => Rejoin::Joined,
        Err(e) => Rejoin::Failed(e.to_string()),
    }
}

/// Sets its flag when dropped, so an aborted future cancels the blocking
/// work it started.
struct CancelOnDrop(Arc<AtomicBool>);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// A hook that rejoins the camera's Wi-Fi network `ssid` (already known to
/// the system, so without a password) before a reconnect attempt, unless
/// the Wi-Fi still has an address on the camera's network. Aborting the
/// reconnect (as disconnecting does) cancels a join in progress. Failures
/// are only logged; the reconnect reports its own.
fn rejoin_hook(wifi: Wifi, ssid: String, camera: SocketAddr) -> ReconnectHook {
    let camera = match camera.ip() {
        IpAddr::V4(ip) => Some(ip),
        IpAddr::V6(_) => None,
    };
    Arc::new(move |attempt| {
        let wifi = wifi.clone();
        let ssid = ssid.clone();
        Box::pin(async move {
            tracing::info!(attempt, %ssid, "rejoining the camera's Wi-Fi before reconnecting");
            let cancel = Arc::new(AtomicBool::new(false));
            let _guard = CancelOnDrop(cancel.clone());
            let name = ssid.clone();
            let outcome =
                tokio::task::spawn_blocking(move || rejoin_blocking(&wifi, &name, camera, &cancel))
                    .await;
            match outcome {
                Ok(Rejoin::Skipped) => {}
                Ok(Rejoin::Joined) => tracing::info!(%ssid, "rejoined the camera's Wi-Fi"),
                Ok(Rejoin::Failed(e)) => {
                    tracing::warn!(%ssid, error = %e, "cannot rejoin the camera's Wi-Fi")
                }
                Err(e) => tracing::warn!(%ssid, error = %e, "Wi-Fi rejoin task failed"),
            }
        })
    })
}

/// The Access Authentication login the user typed. The password only
/// travels into [`SshConfig`]; it is never stored, logged or shown, so this
/// type has no `Debug`.
#[derive(serde::Deserialize)]
pub struct SshLogin {
    pub user: String,
    pub password: String,
    /// The fingerprint the user confirmed; `None` on a first connect.
    pub fingerprint: Option<String>,
}

/// How the `connect` error starts when the camera's fingerprint needs
/// confirming: `HOSTKEY <sha256> <md5>`, parsed by the front end.
pub const HOSTKEY_PREFIX: &str = "HOSTKEY ";

/// How the `connect` error starts when the camera's fingerprint differs
/// from the confirmed one: `HOSTKEY_CHANGED <expected> <actual_sha256>`.
pub const HOSTKEY_CHANGED_PREFIX: &str = "HOSTKEY_CHANGED ";

/// The camera's SSH port: 22, or `RIMIX_SSH_PORT` (a development override
/// for the simulator) when it holds a port number.
fn ssh_port_from(var: Option<String>) -> u16 {
    var.and_then(|v| v.trim().parse().ok()).unwrap_or(22)
}

/// Connects to the camera at `ip`, replacing any camera already connected,
/// and remembers it in the settings with its Wi-Fi name (`ssid`, never a
/// password) when known. With `ssh`, the session runs through the camera's
/// Access Authentication tunnel and the username and confirmed fingerprint
/// are remembered too. Returns its state and a subscription to its events
/// for [`forward_events`].
pub async fn connect(
    state: &AppState,
    ip: &str,
    ssid: Option<&str>,
    ssh: Option<SshLogin>,
) -> Result<(CameraState, broadcast::Receiver<CameraEvent>), String> {
    let port = ssh_port_from(std::env::var("RIMIX_SSH_PORT").ok());
    connect_via(state, ip, ssid, ssh, port).await
}

/// [`connect`] with the camera's SSH port given.
pub(crate) async fn connect_via(
    state: &AppState,
    ip: &str,
    ssid: Option<&str>,
    ssh: Option<SshLogin>,
    ssh_port: u16,
) -> Result<(CameraState, broadcast::Receiver<CameraEvent>), String> {
    let addr = parse_addr(ip)?;
    let remembered_login = ssh
        .as_ref()
        .and_then(|l| Some((l.user.clone(), l.fingerprint.clone()?)));
    let ssh = ssh.map(|login| SshConfig {
        user: login.user,
        password: login.password,
        expected_fingerprint: login.fingerprint,
        port: ssh_port,
    });
    let previous = state.camera.lock().await.take();
    if let Some(old) = previous {
        old.disconnect().await;
    }
    let destination = lock(&state.settings).destination();
    let before_reconnect = ssid.map(|name| rejoin_hook(state.wifi.clone(), name.to_string(), addr));
    let camera = Camera::connect(CameraConfig {
        addr,
        guid: state.guid,
        host_name: sony_ptpip::default_host_name(),
        destination,
        auto_receive: true,
        reconnect: true,
        before_reconnect,
        ssh,
    })
    .await
    .map_err(|e| match e {
        Error::HostKeyUnknown { sha256, md5 } => format!("{HOSTKEY_PREFIX}{sha256} {md5}"),
        Error::HostKeyMismatch {
            expected,
            actual_sha256,
        } => format!("{HOSTKEY_CHANGED_PREFIX}{expected} {actual_sha256}"),
        e => format!("Cannot connect to {addr}: {e}"),
    })?;
    let events = camera.subscribe();
    let camera_state = camera.state();
    // Another connect may have finished while this one waited.
    let replaced = state.camera.lock().await.replace(Arc::new(camera));
    if let Some(old) = replaced {
        old.disconnect().await;
    }
    let snapshot = {
        let mut s = lock(&state.settings);
        let login = remembered_login
            .as_ref()
            .map(|(user, fp)| (user.as_str(), fp.as_str()));
        s.remember_camera(ip.trim(), &camera_state.model, ssid, login, now_millis());
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

/// What happened to one value of [`apply_values`]: `error` is `None` when
/// the camera took it.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ApplyOutcome {
    pub code: u16,
    pub error: Option<String>,
}

/// How long a preset waits before trying refused values again: a Mode
/// written earlier in the same preset may not have reached the camera's
/// other properties yet.
pub const APPLY_RETRY_WAIT: Duration = Duration::from_millis(300);

/// Writes each value to the camera in order (a preset), carrying on past
/// any the camera refuses. Refused values are tried once more after
/// [`APPLY_RETRY_WAIT`], so only values refused twice are reported.
pub async fn apply_values(
    state: &AppState,
    values: Vec<PresetValue>,
) -> Result<Vec<ApplyOutcome>, String> {
    let camera = state.camera().await?;
    let set = |code: u16, raw: PtpValue| {
        let camera = camera.clone();
        async move { camera.set(code, raw).await.map_err(|e| e.to_string()) }
    };
    Ok(apply_with_retry(values, set, || tokio::time::sleep(APPLY_RETRY_WAIT)).await)
}

/// [`apply_values`] over any `set`; `settle` runs once, before the retry.
pub(crate) async fn apply_with_retry<F, Fut, S, SFut>(
    values: Vec<PresetValue>,
    mut set: F,
    settle: S,
) -> Vec<ApplyOutcome>
where
    F: FnMut(u16, PtpValue) -> Fut,
    Fut: std::future::Future<Output = Result<(), String>>,
    S: FnOnce() -> SFut,
    SFut: std::future::Future<Output = ()>,
{
    let mut out = Vec::with_capacity(values.len());
    for v in &values {
        let error = set(v.code, v.raw.clone()).await.err();
        out.push(ApplyOutcome {
            code: v.code,
            error,
        });
    }
    if out.iter().any(|o| o.error.is_some()) {
        settle().await;
        for (v, o) in values.iter().zip(out.iter_mut()) {
            if o.error.is_some() {
                o.error = set(v.code, v.raw.clone()).await.err();
            }
        }
    }
    out
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

pub const NOT_A_CAPTURE: &str = "Only files Rimix saved can be shown";

/// Removes `.` components; `..` is kept, never resolved through the disk.
fn normalise(path: &Path) -> PathBuf {
    path.components()
        .filter(|c| !matches!(c, std::path::Component::CurDir))
        .collect()
}

/// True when `path` sits inside (or is) one of the `allowed` folders. The
/// file's parent is resolved through the disk; a folder that does not exist
/// yet is compared by its normalised path.
fn in_allowed_folder(path: &Path, allowed: &[PathBuf]) -> bool {
    let Some(parent) = path.parent() else {
        return false;
    };
    let Ok(parent) = std::fs::canonicalize(parent) else {
        return false;
    };
    allowed.iter().any(|folder| {
        let folder = std::fs::canonicalize(folder).unwrap_or_else(|_| normalise(folder));
        parent.starts_with(folder)
    })
}

/// The bytes of a saved JPEG for the filmstrip. Only files inside one of the
/// `allowed` capture folders are read; RAW and other files, and files over
/// [`MAX_THUMBNAIL_BYTES`], are refused (the UI shows a tile).
pub fn read_thumbnail(path: &Path, allowed: &[PathBuf]) -> Result<Vec<u8>, String> {
    let is_jpeg = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("jpg") || e.eq_ignore_ascii_case("jpeg"));
    if !is_jpeg {
        return Err(NOT_A_JPEG.into());
    }
    let cannot = |e: std::io::Error| format!("Cannot read {}: {e}", path.display());
    if !in_allowed_folder(path, allowed) {
        // A missing file in a missing folder reads as "cannot read".
        return match std::fs::metadata(path) {
            Err(e) => Err(cannot(e)),
            Ok(_) => Err(NOT_A_CAPTURE.into()),
        };
    }
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
    use sony_ptpip::ptp::codes::{PROP_F_NUMBER, PROP_ISO, PROP_SHUTTER_SPEED};
    use sony_ptpip::{Choice, PropertySummary};
    use sony_ptpip_sim::{spawn, SimCommand, SimConfig, SimHandle, SshSimConfig};
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

    #[tokio::test]
    async fn the_reconnect_hook_is_set_only_when_an_ssid_is_known() {
        let dir = tempfile::tempdir().unwrap();
        let sim = spawn(SimConfig::default()).await;
        let state = app_state(dir.path());
        let ip = sim.addr.to_string();
        connect(&state, &ip, Some("DIRECT-x"), None).await.unwrap();
        let camera = state.camera.lock().await.clone().unwrap();
        assert!(camera.has_reconnect_hook());
        drop(camera);
        connect(&state, &ip, None, None).await.unwrap();
        let camera = state.camera.lock().await.clone().unwrap();
        assert!(!camera.has_reconnect_hook());
        drop(camera);
        sim.events.send(SimCommand::Shutdown).await.unwrap();
    }

    async fn connected(dir: &Path) -> (SimHandle, AppState, CameraState) {
        let sim = spawn(SimConfig::default()).await;
        let state = app_state(dir);
        let (camera_state, _rx) = connect(&state, &sim.addr.to_string(), None, None)
            .await
            .unwrap();
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
        let sim = spawn(SimConfig::default()).await;
        let state = app_state(tmp.path());
        let ip = sim.addr.to_string();
        let (camera_state, _rx) = connect(&state, &ip, Some("DIRECT-abc1:ZV-E10"), None)
            .await
            .unwrap();
        assert!(camera_state.connected);
        assert_eq!(camera_state.model, "ZV-E10");
        let saved = settings::load(&state.settings_path);
        assert_eq!(saved.last_ip.as_deref(), Some(ip.as_str()));
        assert_eq!(saved.recent_cameras[0].ip, ip);
        assert_eq!(saved.recent_cameras[0].model, "ZV-E10");
        assert_eq!(
            saved.recent_cameras[0].ssid.as_deref(),
            Some("DIRECT-abc1:ZV-E10")
        );
        assert_eq!(get_settings(&state), saved);
        assert!(get_state(&state).await.unwrap().connected);
        stop(sim, &state).await;
        assert_eq!(get_state(&state).await, None);
    }

    const SSH_USER: &str = "cam";
    const SSH_PASSWORD: &str = "12345678";

    async fn ssh_sim() -> SimHandle {
        spawn(SimConfig {
            ssh: Some(SshSimConfig {
                user: SSH_USER.into(),
                password: SSH_PASSWORD.into(),
            }),
            ..SimConfig::default()
        })
        .await
    }

    fn login(password: &str, fingerprint: Option<String>) -> SshLogin {
        SshLogin {
            user: SSH_USER.into(),
            password: password.into(),
            fingerprint,
        }
    }

    #[tokio::test]
    async fn connect_through_ssh_asks_for_the_fingerprint_then_is_secured() {
        let tmp = tempfile::tempdir().unwrap();
        let sim = ssh_sim().await;
        let state = app_state(tmp.path());
        let port = sim.ssh_addr.unwrap().port();
        // Through SSH the camera's own PTP/IP port is the default one.
        let ip = sim.addr.ip().to_string();
        let sha256 = sim.fingerprint_sha256.clone().unwrap();
        let md5 = sim.fingerprint_md5.clone().unwrap();

        // No fingerprint confirmed yet: the exact line the front end parses.
        let err = connect_via(&state, &ip, None, Some(login(SSH_PASSWORD, None)), port)
            .await
            .unwrap_err();
        assert_eq!(err, format!("HOSTKEY {sha256} {md5}"));
        assert!(settings::load(&state.settings_path)
            .recent_cameras
            .is_empty());

        let (camera_state, _rx) = connect_via(
            &state,
            &ip,
            None,
            Some(login(SSH_PASSWORD, Some(sha256.clone()))),
            port,
        )
        .await
        .unwrap();
        assert!(
            camera_state.connected && camera_state.secured,
            "{camera_state:?}"
        );
        let saved = settings::load(&state.settings_path);
        assert_eq!(saved.recent_cameras[0].ssh_user.as_deref(), Some(SSH_USER));
        assert_eq!(
            saved.recent_cameras[0].ssh_fingerprint.as_deref(),
            Some(sha256.as_str())
        );
        // The password is never written down.
        let text = std::fs::read_to_string(&state.settings_path).unwrap();
        assert!(!text.contains(SSH_PASSWORD), "{text}");
        stop(sim, &state).await;
    }

    #[tokio::test]
    async fn a_changed_fingerprint_names_both() {
        let tmp = tempfile::tempdir().unwrap();
        let sim = ssh_sim().await;
        let state = app_state(tmp.path());
        let port = sim.ssh_addr.unwrap().port();
        let ip = sim.addr.ip().to_string();
        let stored = "SHA256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
        let actual = sim.fingerprint_sha256.clone().unwrap();
        let err = connect_via(
            &state,
            &ip,
            None,
            Some(login(SSH_PASSWORD, Some(stored.into()))),
            port,
        )
        .await
        .unwrap_err();
        assert_eq!(err, format!("HOSTKEY_CHANGED {stored} {actual}"));
        assert!(state.camera.lock().await.is_none());
        sim.events.send(SimCommand::Shutdown).await.unwrap();
    }

    #[test]
    fn slash_24_compares_the_first_three_octets() {
        let cam = Ipv4Addr::new(192, 168, 122, 1);
        assert!(same_slash_24(Ipv4Addr::new(192, 168, 122, 10), cam));
        assert!(!same_slash_24(Ipv4Addr::new(192, 168, 1, 10), cam));
    }

    /// Answers `networksetup` and `ipconfig` as a Mac on `address` would,
    /// and records every command.
    #[cfg(target_os = "macos")]
    struct MacOnAddress {
        address: &'static str,
        calls: StdMutex<Vec<String>>,
    }

    #[cfg(target_os = "macos")]
    impl rimix_wifi::Runner for MacOnAddress {
        fn run(&self, program: &str, args: &[&str]) -> std::io::Result<rimix_wifi::Output> {
            let line = format!("{program} {}", args.join(" "));
            lock(&self.calls).push(line.clone());
            let stdout = if line.contains("-listallhardwareports") {
                "Hardware Port: Wi-Fi\nDevice: en0\n".to_string()
            } else if line.starts_with("ipconfig getifaddr") {
                format!("{}\n", self.address)
            } else if line.contains("-getairportnetwork") {
                "You are not associated with an AirPort network.\n".to_string()
            } else {
                String::new()
            };
            Ok(rimix_wifi::Output {
                success: true,
                stdout,
                stderr: String::new(),
            })
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn rejoin_is_skipped_while_the_wifi_is_on_the_cameras_network() {
        let runner = Arc::new(MacOnAddress {
            address: "192.168.122.10",
            calls: StdMutex::new(Vec::new()),
        });
        let wifi = Wifi::new(runner.clone());
        let outcome = rejoin_blocking(
            &wifi,
            "DIRECT-x",
            Some(Ipv4Addr::new(192, 168, 122, 1)),
            &AtomicBool::new(false),
        );
        assert_eq!(outcome, Rejoin::Skipped);
        assert!(lock(&runner.calls)
            .iter()
            .all(|c| !c.contains("-setairportnetwork")));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn rejoin_joins_when_the_wifi_is_elsewhere_and_stops_when_cancelled() {
        let camera = Some(Ipv4Addr::new(192, 168, 122, 1));
        let elsewhere = || {
            Arc::new(MacOnAddress {
                address: "10.0.0.5",
                calls: StdMutex::new(Vec::new()),
            })
        };
        let runner = elsewhere();
        let wifi = Wifi::new(runner.clone()).with_clock(std::time::Instant::now, |_| {});
        let outcome = rejoin_blocking(&wifi, "DIRECT-x", camera, &AtomicBool::new(false));
        assert_eq!(outcome, Rejoin::Joined);
        assert!(lock(&runner.calls)
            .iter()
            .any(|c| c.contains("-setairportnetwork en0 DIRECT-x")));

        let runner = elsewhere();
        let wifi = Wifi::new(runner.clone()).with_clock(std::time::Instant::now, |_| {});
        let outcome = rejoin_blocking(&wifi, "DIRECT-x", camera, &AtomicBool::new(true));
        assert_eq!(outcome, Rejoin::Failed("cancelled".into()));
        assert!(lock(&runner.calls)
            .iter()
            .all(|c| !c.contains("-setairportnetwork")));
    }

    #[test]
    fn rejoin_without_an_address_joins() {
        let wifi = Wifi::fake();
        let outcome = rejoin_blocking(
            &wifi,
            "DIRECT-x",
            Some(Ipv4Addr::new(192, 168, 122, 1)),
            &AtomicBool::new(false),
        );
        assert_eq!(outcome, Rejoin::Joined);
        assert_eq!(wifi.current_network().unwrap().as_deref(), Some("DIRECT-x"));
    }

    #[tokio::test]
    async fn dropping_the_hook_future_cancels_the_join() {
        let flag = Arc::new(AtomicBool::new(false));
        let guard = CancelOnDrop(flag.clone());
        let task = tokio::spawn(async move {
            let _guard = guard;
            std::future::pending::<()>().await;
        });
        tokio::task::yield_now().await;
        assert!(!flag.load(Ordering::SeqCst));
        task.abort();
        let _ = task.await;
        assert!(flag.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn a_refused_ssh_login_keeps_the_plain_error_text() {
        let tmp = tempfile::tempdir().unwrap();
        let sim = ssh_sim().await;
        let state = app_state(tmp.path());
        let port = sim.ssh_addr.unwrap().port();
        let ip = sim.addr.ip().to_string();
        let fp = sim.fingerprint_sha256.clone();
        let err = connect_via(&state, &ip, None, Some(login("wrong", fp)), port)
            .await
            .unwrap_err();
        assert_eq!(
            err,
            format!("Cannot connect to {ip}:15740: the camera refused the username or password")
        );
        assert!(!err.contains("wrong"));
        sim.events.send(SimCommand::Shutdown).await.unwrap();
    }

    #[test]
    fn the_ssh_port_is_22_unless_the_dev_override_names_one() {
        assert_eq!(ssh_port_from(None), 22);
        assert_eq!(ssh_port_from(Some("2222".into())), 2222);
        assert_eq!(ssh_port_from(Some("nope".into())), 22);
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
    async fn apply_values_writes_in_order_and_reports_each_failure() {
        let tmp = tempfile::tempdir().unwrap();
        let (sim, state, _) = connected(tmp.path()).await;
        let values = vec![
            PresetValue {
                code: PROP_ISO,
                raw: PtpValue::U32(1600),
            },
            // Not in the aperture's settable list.
            PresetValue {
                code: PROP_F_NUMBER,
                raw: PtpValue::U16(9999),
            },
            PresetValue {
                code: PROP_SHUTTER_SPEED,
                raw: PtpValue::U32(66536),
            },
        ];
        let out = apply_values(&state, values).await.unwrap();
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].code, PROP_ISO);
        assert!(out[0].error.is_none(), "{:?}", out[0].error);
        assert_eq!(out[1].code, PROP_F_NUMBER);
        assert!(out[1].error.is_some());
        assert_eq!(out[2].code, PROP_SHUTTER_SPEED);
        assert!(out[2].error.is_none(), "{:?}", out[2].error);
        let after = get_state(&state).await.unwrap();
        assert_eq!(short_of(&after, PROP_ISO), "1600");
        assert_ne!(short_of(&after, PROP_SHUTTER_SPEED), "1/125");
        stop(sim, &state).await;
    }

    #[tokio::test]
    async fn apply_values_tries_a_refused_value_again_before_reporting_it() {
        let tmp = tempfile::tempdir().unwrap();
        let (sim, state, _) = connected(tmp.path()).await;
        let camera = state.camera().await.unwrap();
        let attempts = Arc::new(StdMutex::new(Vec::new()));
        let log = attempts.clone();
        let set = |code: u16, raw: PtpValue| {
            let camera = camera.clone();
            let log = log.clone();
            async move {
                log.lock().unwrap().push(code);
                camera.set(code, raw).await.map_err(|e| e.to_string())
            }
        };
        let settled = Arc::new(StdMutex::new(0));
        let settles = settled.clone();
        let values = vec![
            PresetValue {
                code: PROP_ISO,
                raw: PtpValue::U32(1600),
            },
            // Not in the aperture's settable list, so refused both times.
            PresetValue {
                code: PROP_F_NUMBER,
                raw: PtpValue::U16(9999),
            },
        ];
        let out = apply_with_retry(values, set, || async move {
            *settles.lock().unwrap() += 1;
        })
        .await;
        assert_eq!(
            *attempts.lock().unwrap(),
            vec![PROP_ISO, PROP_F_NUMBER, PROP_F_NUMBER]
        );
        assert_eq!(*settled.lock().unwrap(), 1);
        assert!(out[0].error.is_none(), "{:?}", out[0].error);
        assert!(out[1].error.is_some());
        stop(sim, &state).await;
    }

    #[tokio::test]
    async fn apply_values_reports_a_value_taken_on_the_retry_as_set() {
        let tries = Arc::new(StdMutex::new(0));
        let count = tries.clone();
        let set = |_code: u16, _raw: PtpValue| {
            let count = count.clone();
            async move {
                let mut n = count.lock().unwrap();
                *n += 1;
                if *n == 1 {
                    Err("property cannot be set now".to_string())
                } else {
                    Ok(())
                }
            }
        };
        let values = vec![PresetValue {
            code: PROP_F_NUMBER,
            raw: PtpValue::U16(280),
        }];
        let out = apply_with_retry(values, set, || async {}).await;
        assert_eq!(*tries.lock().unwrap(), 2);
        assert_eq!(out[0].error, None);

        // Nothing refused: no wait and no second pass.
        let out = apply_with_retry(
            vec![],
            |_: u16, _: PtpValue| async { Ok(()) },
            || async { panic!("no wait when nothing was refused") },
        )
        .await;
        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn apply_values_needs_a_camera() {
        let tmp = tempfile::tempdir().unwrap();
        let state = app_state(tmp.path());
        assert_eq!(
            apply_values(&state, vec![]).await.unwrap_err(),
            NOT_CONNECTED
        );
    }

    #[test]
    fn apply_outcome_json_is_stable() {
        let out = ApplyOutcome {
            code: 0x5007,
            error: Some("nope".into()),
        };
        assert_eq!(
            serde_json::to_value(&out).unwrap(),
            serde_json::json!({ "code": 20487, "error": "nope" })
        );
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
        let (_, rx) = connect(&state, &sim.addr.to_string(), None, None)
            .await
            .unwrap();
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
        let allowed = vec![tmp.path().to_path_buf()];
        let jpg = tmp.path().join("DSC00001.JPG");
        std::fs::write(&jpg, [0xFF, 0xD8, 0xFF, 0xD9]).unwrap();
        assert_eq!(
            read_thumbnail(&jpg, &allowed).unwrap(),
            vec![0xFF, 0xD8, 0xFF, 0xD9]
        );
        let lower = tmp.path().join("b.jpeg");
        std::fs::write(&lower, [1, 2]).unwrap();
        assert_eq!(read_thumbnail(&lower, &allowed).unwrap(), vec![1, 2]);

        let raw = tmp.path().join("DSC00002.ARW");
        std::fs::write(&raw, [1, 2, 3]).unwrap();
        assert_eq!(read_thumbnail(&raw, &allowed).unwrap_err(), NOT_A_JPEG);

        let big = tmp.path().join("big.jpg");
        std::fs::File::create(&big)
            .unwrap()
            .set_len(MAX_THUMBNAIL_BYTES + 1)
            .unwrap();
        assert_eq!(read_thumbnail(&big, &allowed).unwrap_err(), TOO_LARGE);

        let missing = read_thumbnail(&tmp.path().join("gone.jpg"), &allowed).unwrap_err();
        assert!(missing.starts_with("Cannot read"), "{missing}");
    }

    #[test]
    fn read_thumbnail_refuses_files_outside_the_capture_folders() {
        let tmp = tempfile::tempdir().unwrap();
        let shots = tmp.path().join("shots");
        let elsewhere = tmp.path().join("elsewhere");
        std::fs::create_dir_all(&shots).unwrap();
        std::fs::create_dir_all(&elsewhere).unwrap();
        let allowed = vec![shots.clone(), tmp.path().join("not-created-yet")];
        let inside = shots.join("a.jpg");
        let outside = elsewhere.join("a.jpg");
        std::fs::write(&inside, [0xFF, 0xD8]).unwrap();
        std::fs::write(&outside, [0xFF, 0xD8]).unwrap();
        assert_eq!(read_thumbnail(&inside, &allowed).unwrap(), vec![0xFF, 0xD8]);
        assert_eq!(
            read_thumbnail(&outside, &allowed).unwrap_err(),
            NOT_A_CAPTURE
        );
        // A `..` path that lands outside is refused too.
        let sneaky = shots.join("..").join("elsewhere").join("a.jpg");
        assert_eq!(
            read_thumbnail(&sneaky, &allowed).unwrap_err(),
            NOT_A_CAPTURE
        );
    }

    /// Pins the JSON the webview receives for a state change. The same
    /// string is the fixture in `src/fixtures/state-changed.json`, which
    /// `src/ipc.test.ts` checks against the TypeScript type guards.
    #[test]
    fn state_changed_event_json_is_stable() {
        let event = CameraEvent::StateChanged(CameraState {
            connected: true,
            secured: true,
            model: "ZV-E10".into(),
            firmware: "2.00".into(),
            generation: "3".into(),
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

    /// Pins the JSON of a saved shot whose name took a `-N` suffix; the same
    /// string is `src/fixtures/capture-saved.json`, used by `src/ipc.test.ts`
    /// and `src/state.test.ts`.
    #[test]
    fn capture_saved_event_json_is_stable() {
        let event = CameraEvent::CaptureSaved(sony_ptpip::capture::Received {
            path: "/p/DSC00001-1.JPG".into(),
            bytes: 3,
            filename: "DSC00001-1.JPG".into(),
            camera_filename: "DSC00001.JPG".into(),
        });
        assert_eq!(
            serde_json::to_string(&event).unwrap(),
            CAPTURE_SAVED_FIXTURE.trim()
        );
    }

    const CAPTURE_SAVED_FIXTURE: &str = include_str!("../../src/fixtures/capture-saved.json");

    /// Serialises the tests that set `RIMIX_CAMERA_ADDR`.
    static CAMERA_ADDR: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    fn camera(name: &str, source: DiscoverySource) -> DiscoveredCamera {
        DiscoveredCamera {
            model: name.to_string(),
            friendly_name: name.to_string(),
            ip: "192.168.122.1".parse().unwrap(),
            ptp_port: PTP_PORT,
            ptp_version: None,
            pairing_required: None,
            ssh_supported: None,
            source,
        }
    }

    #[tokio::test]
    async fn wait_for_camera_honours_the_dev_override() {
        let _guard = CAMERA_ADDR.lock().await;
        std::env::set_var("RIMIX_CAMERA_ADDR", "127.0.0.1:15740");
        let cam = wait_for_camera(Duration::from_millis(10)).await.unwrap();
        std::env::remove_var("RIMIX_CAMERA_ADDR");
        assert_eq!(cam.ip, "127.0.0.1".parse::<IpAddr>().unwrap());
        assert_eq!(cam.ptp_port, 15740);
    }

    #[tokio::test]
    async fn wait_for_camera_times_out_without_spinning() {
        let _guard = CAMERA_ADDR.lock().await;
        std::env::remove_var("RIMIX_CAMERA_ADDR");
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let err = wait_for_camera_with(
            |_| {
                calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                async { Ok(Vec::new()) }
            },
            Duration::from_millis(10),
        )
        .await
        .unwrap_err();
        assert_eq!(err, NO_CAMERA);
        assert!(calls.load(std::sync::atomic::Ordering::SeqCst) < 5);
    }

    #[tokio::test]
    async fn wait_for_camera_prefers_the_gateway_camera() {
        let _guard = CAMERA_ADDR.lock().await;
        std::env::remove_var("RIMIX_CAMERA_ADDR");
        let cam = wait_for_camera_with(
            |_| async {
                Ok(vec![
                    camera("ssdp", DiscoverySource::Ssdp),
                    camera("gateway", DiscoverySource::Gateway),
                ])
            },
            Duration::from_millis(10),
        )
        .await
        .unwrap();
        assert_eq!(cam.model, "gateway");
    }

    #[tokio::test]
    async fn join_network_goes_through_the_wifi_and_current_network_reports_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = app_state(dir.path());
        state.wifi = rimix_wifi::Wifi::fake();
        join_network(&state, "DIRECT-abc1:ZV-E10", Some("12345678"))
            .await
            .unwrap();
        assert_eq!(
            current_network(&state).await.unwrap().as_deref(),
            Some("DIRECT-abc1:ZV-E10")
        );
    }
}

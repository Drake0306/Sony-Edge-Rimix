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
    Camera, CameraConfig, CameraEvent, CameraState, CardBrowse, CardObject, DiscoveredCamera,
    ReconnectHook, SshConfig,
};
use tokio::sync::broadcast::{self, error::RecvError};

use crate::settings::{self, AppSettings, PresetValue};
use rimix_wifi::{SystemRunner, Wifi};

/// The error every camera command gives while no camera is connected.
pub const NOT_CONNECTED: &str = "Not connected to a camera";

/// Which camera a command is for: the camera's Wi-Fi name when it was
/// connected through its network, else its `ip:port`. Serialised as a plain
/// string.
#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct CameraId(pub String);

impl std::fmt::Display for CameraId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The id for a camera at `addr`, reached through the network `ssid` if known.
pub fn camera_id(addr: SocketAddr, ssid: Option<&str>) -> CameraId {
    CameraId(ssid.map_or_else(|| addr.to_string(), str::to_string))
}

/// One connected camera.
pub struct Slot {
    pub id: CameraId,
    /// Where the camera answers PTP/IP. Two cameras cannot answer at one
    /// address at once, so a connect there replaces this slot.
    pub addr: SocketAddr,
    /// The camera's model, as the UI names it.
    pub label: String,
    /// Held in an `Arc` so a long command (a shot) does not keep the lock
    /// and stall live view polling.
    pub camera: Arc<Camera>,
}

/// A camera event as the webview receives it, naming the camera it came
/// from: `{"camera_id": "...", "event": {...}}`.
#[derive(Clone, Copy, serde::Serialize)]
pub struct TaggedEvent<'a> {
    pub camera_id: &'a CameraId,
    pub event: &'a CameraEvent,
}

/// One row of [`list_cameras`].
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct CameraSummary {
    pub id: CameraId,
    pub label: String,
    pub connected: bool,
    pub secured: bool,
    pub active: bool,
}

/// Managed Tauri state: the connected cameras, which one the UI drives, and
/// the settings.
pub struct AppState {
    /// In the order they were connected.
    pub cameras: tokio::sync::Mutex<Vec<Slot>>,
    /// The camera commands act on when they name none.
    pub active: StdMutex<Option<CameraId>>,
    pub settings: StdMutex<AppSettings>,
    pub guid: [u8; 16],
    pub settings_path: PathBuf,
    pub wifi: Wifi,
    /// Receive from camera: the SFTP server the camera pushes files to.
    pub sftp: crate::sftp::SftpService,
    /// Bluetooth: cameras heard nearby and their Wi-Fi details.
    pub ble: crate::ble::BleService,
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
            cameras: tokio::sync::Mutex::new(Vec::new()),
            active: StdMutex::new(None),
            settings: StdMutex::new(settings),
            guid,
            settings_path,
            wifi: Wifi::from_env(Arc::new(SystemRunner)),
            sftp: crate::sftp::SftpService::default(),
            ble: crate::ble::BleService::default(),
        }
    }

    /// The camera `id` names, or the active one for `None`.
    async fn find(&self, id: Option<&CameraId>) -> Option<Arc<Camera>> {
        let id = match id {
            Some(id) => id.clone(),
            None => lock(&self.active).clone()?,
        };
        let slots = self.cameras.lock().await;
        slots.iter().find(|s| s.id == id).map(|s| s.camera.clone())
    }

    /// Like [`AppState::find`], failing with [`NOT_CONNECTED`].
    async fn camera(&self, id: Option<&CameraId>) -> Result<Arc<Camera>, String> {
        self.find(id).await.ok_or_else(|| NOT_CONNECTED.to_string())
    }

    /// Takes the slot `id` out; if it was active, the slot now at its place
    /// (or the last one) becomes active. Call with the slots locked.
    fn remove_slot(&self, slots: &mut Vec<Slot>, id: &CameraId) -> Option<Slot> {
        let at = slots.iter().position(|s| &s.id == id)?;
        let slot = slots.remove(at);
        let mut active = lock(&self.active);
        if active.as_ref() == Some(id) {
            *active = slots.get(at).or_else(|| slots.last()).map(|s| s.id.clone());
        }
        Some(slot)
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

/// Connects to the camera at `ip` and makes it the active camera. A camera
/// already connected under the same [`CameraId`] or at the same address is
/// replaced; the others stay connected unless `keep_others` is false.
/// Remembers the camera in the settings with its Wi-Fi name (`ssid`, never a
/// password) when known. With `ssh`, the session runs through the camera's
/// Access Authentication tunnel and the username and confirmed fingerprint
/// are remembered too. Returns its id, its state and a subscription to its
/// events for [`forward_events`].
pub async fn connect(
    state: &AppState,
    ip: &str,
    ssid: Option<&str>,
    ssh: Option<SshLogin>,
    keep_others: bool,
) -> Result<Connected, String> {
    let port = ssh_port_from(std::env::var("RIMIX_SSH_PORT").ok());
    connect_via(state, ip, ssid, ssh, port, keep_others).await
}

/// What [`connect`] gives: the camera's id, its state and its events.
pub type Connected = (CameraId, CameraState, broadcast::Receiver<CameraEvent>);

/// [`connect`] with the camera's SSH port given.
pub(crate) async fn connect_via(
    state: &AppState,
    ip: &str,
    ssid: Option<&str>,
    ssh: Option<SshLogin>,
    ssh_port: u16,
    keep_others: bool,
) -> Result<Connected, String> {
    let addr = parse_addr(ip)?;
    let id = camera_id(addr, ssid);
    let remembered_login = ssh
        .as_ref()
        .and_then(|l| Some((l.user.clone(), l.fingerprint.clone()?)));
    let ssh = ssh.map(|login| SshConfig {
        user: login.user,
        password: login.password,
        expected_fingerprint: login.fingerprint,
        port: ssh_port,
    });
    // The camera takes one session at a time: an earlier connection to it,
    // under this id or at this address, goes first.
    let previous: Vec<Slot> = {
        let mut slots = state.cameras.lock().await;
        if keep_others {
            let same: Vec<CameraId> = slots
                .iter()
                .filter(|s| s.id == id || s.addr == addr)
                .map(|s| s.id.clone())
                .collect();
            same.iter()
                .filter_map(|old| state.remove_slot(&mut slots, old))
                .collect()
        } else {
            *lock(&state.active) = None;
            std::mem::take(&mut *slots)
        }
    };
    for old in previous {
        old.camera.disconnect().await;
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
    // Another connect to this camera may have finished while this one waited.
    let replaced = {
        let mut slots = state.cameras.lock().await;
        let replaced = state.remove_slot(&mut slots, &id);
        slots.push(Slot {
            id: id.clone(),
            addr,
            label: camera_state.model.clone(),
            camera: Arc::new(camera),
        });
        *lock(&state.active) = Some(id.clone());
        replaced
    };
    if let Some(old) = replaced {
        old.camera.disconnect().await;
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
    Ok((id, camera_state, events))
}

/// Hands every event except `Frame` to `emit`, tagged with `camera_id`
/// (the UI polls frames instead), until the camera is disconnected or
/// replaced and its channel closes.
pub async fn forward_events(
    camera_id: CameraId,
    mut rx: broadcast::Receiver<CameraEvent>,
    mut emit: impl FnMut(TaggedEvent<'_>),
) {
    loop {
        match rx.recv().await {
            Ok(CameraEvent::Frame) => {}
            Ok(event) => emit(TaggedEvent {
                camera_id: &camera_id,
                event: &event,
            }),
            Err(RecvError::Lagged(missed)) => {
                tracing::warn!(%camera_id, missed, "camera events fell behind");
            }
            Err(RecvError::Closed) => break,
        }
    }
}

/// Disconnects the camera `id` names (the active one for `None`); when it
/// was active, the next remaining camera becomes active.
pub async fn disconnect(state: &AppState, id: Option<CameraId>) -> Result<(), String> {
    let slot = {
        let mut slots = state.cameras.lock().await;
        let id = id.or_else(|| lock(&state.active).clone());
        id.and_then(|id| state.remove_slot(&mut slots, &id))
    };
    if let Some(slot) = slot {
        slot.camera.disconnect().await;
    }
    Ok(())
}

/// Disconnects every camera.
pub async fn disconnect_all(state: &AppState) {
    let slots = {
        let mut slots = state.cameras.lock().await;
        *lock(&state.active) = None;
        std::mem::take(&mut *slots)
    };
    for slot in slots {
        slot.camera.disconnect().await;
    }
}

pub async fn get_state(state: &AppState, id: Option<CameraId>) -> Option<CameraState> {
    state.find(id.as_ref()).await.map(|c| c.state())
}

/// The connected cameras in the order they were connected.
pub async fn list_cameras(state: &AppState) -> Vec<CameraSummary> {
    let active = lock(&state.active).clone();
    let slots = state.cameras.lock().await;
    slots
        .iter()
        .map(|slot| {
            let s = slot.camera.state();
            CameraSummary {
                id: slot.id.clone(),
                label: slot.label.clone(),
                connected: s.connected,
                secured: s.secured,
                active: active.as_ref() == Some(&slot.id),
            }
        })
        .collect()
}

/// Makes `id` the camera that commands without an id act on.
pub async fn set_active_camera(state: &AppState, id: CameraId) -> Result<(), String> {
    let slots = state.cameras.lock().await;
    if !slots.iter().any(|s| s.id == id) {
        return Err(NOT_CONNECTED.to_string());
    }
    *lock(&state.active) = Some(id);
    Ok(())
}

/// Shoots every connected camera at once; one entry per camera, with the
/// error when its shot failed.
pub async fn shoot_all(state: &AppState) -> Vec<(CameraId, Option<String>)> {
    let cameras: Vec<(CameraId, Arc<Camera>)> = state
        .cameras
        .lock()
        .await
        .iter()
        .map(|s| (s.id.clone(), s.camera.clone()))
        .collect();
    // Spawned so the shots run concurrently (no `join_all` without the
    // futures crate); awaited in order afterwards.
    let shots: Vec<_> = cameras
        .into_iter()
        .map(|(id, camera)| {
            let shot = tokio::spawn(async move { camera.shoot().await.map_err(|e| e.to_string()) });
            (id, shot)
        })
        .collect();
    let mut out = Vec::with_capacity(shots.len());
    for (id, shot) in shots {
        let error = match shot.await {
            Ok(result) => result.err(),
            Err(e) => Some(e.to_string()),
        };
        out.push((id, error));
    }
    out
}

/// Sets property `code` to `raw`, a PTP value as serde writes it, e.g.
/// `{"U32": 1600}`.
pub async fn set_property(
    state: &AppState,
    id: Option<CameraId>,
    code: u16,
    raw: serde_json::Value,
) -> Result<(), String> {
    let value: PtpValue = serde_json::from_value(raw).map_err(|e| format!("Invalid value: {e}"))?;
    let camera = state.camera(id.as_ref()).await?;
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
    id: Option<CameraId>,
    values: Vec<PresetValue>,
) -> Result<Vec<ApplyOutcome>, String> {
    let camera = state.camera(id.as_ref()).await?;
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

pub async fn step_property(
    state: &AppState,
    id: Option<CameraId>,
    code: u16,
    delta: i32,
) -> Result<(), String> {
    let camera = state.camera(id.as_ref()).await?;
    camera.step(code, delta).await.map_err(|e| e.to_string())
}

pub async fn shoot(state: &AppState, id: Option<CameraId>) -> Result<(), String> {
    let camera = state.camera(id.as_ref()).await?;
    camera.shoot().await.map_err(|e| e.to_string())
}

pub async fn focus(state: &AppState, id: Option<CameraId>) -> Result<(), String> {
    let camera = state.camera(id.as_ref()).await?;
    camera.focus().await.map_err(|e| e.to_string())
}

pub async fn toggle_record(state: &AppState, id: Option<CameraId>) -> Result<(), String> {
    let camera = state.camera(id.as_ref()).await?;
    camera.toggle_record().await.map_err(|e| e.to_string())
}

/// Every file on the camera's cards.
pub async fn card_list(state: &AppState, id: Option<CameraId>) -> Result<Vec<CardObject>, String> {
    let camera = state.camera(id.as_ref()).await?;
    camera.card_list().await.map_err(|e| e.to_string())
}

/// Downloads the card file `handle` into the destination folder and returns
/// where it was saved. Runs until the file is complete; progress arrives as
/// `CardDownloadProgress` events. Only the camera's own connection lock is
/// taken, one chunk at a time, so other commands keep working meanwhile.
pub async fn card_download(
    state: &AppState,
    id: Option<CameraId>,
    handle: u32,
) -> Result<String, String> {
    let camera = state.camera(id.as_ref()).await?;
    let path = camera
        .card_download(handle)
        .await
        .map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
}

/// How the UI names a [`CardBrowse`].
pub fn card_mode_name(mode: CardBrowse) -> &'static str {
    match mode {
        CardBrowse::SameSession => "same-session",
        CardBrowse::NeedsTransferMode => "needs-transfer-mode",
    }
}

/// How the camera lets its card be browsed: `same-session` or
/// `needs-transfer-mode`.
pub async fn card_mode(state: &AppState, id: Option<CameraId>) -> Result<String, String> {
    let camera = state.camera(id.as_ref()).await?;
    let model = camera.state().model;
    Ok(card_mode_name(sony_ptpip::quirks_for(&model).card_browse).to_string())
}

/// The camera's latest live view JPEG; empty when there is none or no camera.
pub async fn live_view_frame(state: &AppState, id: Option<CameraId>) -> Result<Vec<u8>, String> {
    let camera = state.find(id.as_ref()).await;
    Ok(camera
        .and_then(|c| c.latest_frame())
        .map(|f| f.jpeg.clone())
        .unwrap_or_default())
}

/// Focus frames and level for the latest frame, and the frame rate:
/// `{ "focus_frames": [...], "level_deg": number|null, "fps": number }`.
pub async fn live_view_meta(
    state: &AppState,
    id: Option<CameraId>,
) -> Result<serde_json::Value, String> {
    let camera = state.find(id.as_ref()).await;
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

/// Stores the settings and points every connected camera, and the SFTP
/// receiver, at the new destination. The receiver's own settings are kept:
/// only the `sftp_*` commands change them.
pub async fn save_settings(state: &AppState, mut s: AppSettings) -> Result<(), String> {
    // The settings lock is held across merge, save and assign: a concurrent
    // `sftp_set_enabled` (which also saves under this lock) lands either
    // wholly before, and its `sftp` is copied in here, or wholly after, and
    // it changes what is assigned here. Either way the switch is not lost.
    let (moved, destination) = {
        let mut current = lock(&state.settings);
        s.sftp = current.sftp.clone();
        let moved = current.destination() != s.destination();
        settings::save(&state.settings_path, &s)
            .map_err(|e| format!("Cannot save settings: {e}"))?;
        let destination = s.destination();
        *current = s;
        (moved, destination)
    };
    for slot in state.cameras.lock().await.iter() {
        slot.camera.set_destination(destination.clone());
    }
    if moved {
        crate::sftp::restart_if_enabled(state).await;
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
        connect(&state, &ip, Some("DIRECT-x"), None, true)
            .await
            .unwrap();
        let camera = state.camera(None).await.unwrap();
        assert!(camera.has_reconnect_hook());
        drop(camera);
        // Another id (the address), so the first slot is dropped explicitly.
        connect(&state, &ip, None, None, false).await.unwrap();
        let camera = state.camera(None).await.unwrap();
        assert!(!camera.has_reconnect_hook());
        assert_eq!(state.cameras.lock().await.len(), 1);
        drop(camera);
        sim.events.send(SimCommand::Shutdown).await.unwrap();
    }

    async fn connected(dir: &Path) -> (SimHandle, AppState, CameraState) {
        let sim = spawn(SimConfig::default()).await;
        let state = app_state(dir);
        let (_, camera_state, _rx) = connect(&state, &sim.addr.to_string(), None, None, true)
            .await
            .unwrap();
        (sim, state, camera_state)
    }

    async fn stop(sim: SimHandle, state: &AppState) {
        disconnect(state, None).await.unwrap();
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
        let (_, camera_state, _rx) = connect(&state, &ip, Some("DIRECT-abc1:ZV-E10"), None, true)
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
        assert!(get_state(&state, None).await.unwrap().connected);
        stop(sim, &state).await;
        assert_eq!(get_state(&state, None).await, None);
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
        let err = connect_via(
            &state,
            &ip,
            None,
            Some(login(SSH_PASSWORD, None)),
            port,
            true,
        )
        .await
        .unwrap_err();
        assert_eq!(err, format!("HOSTKEY {sha256} {md5}"));
        assert!(settings::load(&state.settings_path)
            .recent_cameras
            .is_empty());

        let (_, camera_state, _rx) = connect_via(
            &state,
            &ip,
            None,
            Some(login(SSH_PASSWORD, Some(sha256.clone()))),
            port,
            true,
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
            true,
        )
        .await
        .unwrap_err();
        assert_eq!(err, format!("HOSTKEY_CHANGED {stored} {actual}"));
        assert!(state.cameras.lock().await.is_empty());
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
        let err = connect_via(&state, &ip, None, Some(login("wrong", fp)), port, true)
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
        assert_eq!(shoot(&state, None).await.unwrap_err(), NOT_CONNECTED);
        assert!(live_view_frame(&state, None).await.unwrap().is_empty());
        let meta = live_view_meta(&state, None).await.unwrap();
        assert_eq!(
            meta,
            serde_json::json!({ "focus_frames": [], "level_deg": null, "fps": 0.0 })
        );
        disconnect(&state, None).await.unwrap();
    }

    #[tokio::test]
    async fn set_and_step_property_change_the_camera() {
        let tmp = tempfile::tempdir().unwrap();
        let (sim, state, before) = connected(tmp.path()).await;
        set_property(&state, None, PROP_ISO, serde_json::json!({ "U32": 1600 }))
            .await
            .unwrap();
        let after = get_state(&state, None).await.unwrap();
        assert_eq!(short_of(&after, PROP_ISO), "1600");
        // A value that is not a PTP value is refused before the camera.
        let err = set_property(&state, None, PROP_ISO, serde_json::json!(1600))
            .await
            .unwrap_err();
        assert!(err.starts_with("Invalid value"), "{err}");

        assert_eq!(short_of(&before, PROP_SHUTTER_SPEED), "1/125");
        step_property(&state, None, PROP_SHUTTER_SPEED, 1)
            .await
            .unwrap();
        let stepped = get_state(&state, None).await.unwrap();
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
        let out = apply_values(&state, None, values).await.unwrap();
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].code, PROP_ISO);
        assert!(out[0].error.is_none(), "{:?}", out[0].error);
        assert_eq!(out[1].code, PROP_F_NUMBER);
        assert!(out[1].error.is_some());
        assert_eq!(out[2].code, PROP_SHUTTER_SPEED);
        assert!(out[2].error.is_none(), "{:?}", out[2].error);
        let after = get_state(&state, None).await.unwrap();
        assert_eq!(short_of(&after, PROP_ISO), "1600");
        assert_ne!(short_of(&after, PROP_SHUTTER_SPEED), "1/125");
        stop(sim, &state).await;
    }

    #[tokio::test]
    async fn apply_values_tries_a_refused_value_again_before_reporting_it() {
        let tmp = tempfile::tempdir().unwrap();
        let (sim, state, _) = connected(tmp.path()).await;
        let camera = state.camera(None).await.unwrap();
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
            apply_values(&state, None, vec![]).await.unwrap_err(),
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
        shoot(&state, None).await.unwrap();
        assert!(get_state(&state, None)
            .await
            .unwrap()
            .last_shot_at
            .is_some());
        let shots = tmp.path().join("shots");
        timeout(WAIT, async {
            while std::fs::read_dir(&shots).map_or(0, |d| d.count()) == 0 {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("no shot saved");
        focus(&state, None).await.unwrap();
        stop(sim, &state).await;
    }

    #[tokio::test]
    async fn live_view_frame_is_a_jpeg_with_meta() {
        let tmp = tempfile::tempdir().unwrap();
        let (sim, state, _) = connected(tmp.path()).await;
        let jpeg = timeout(WAIT, async {
            loop {
                let jpeg = live_view_frame(&state, None).await.unwrap();
                if !jpeg.is_empty() {
                    return jpeg;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("no live view frame");
        assert_eq!(&jpeg[..2], &[0xFF, 0xD8]);
        let meta = live_view_meta(&state, None).await.unwrap();
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
        let (id, _, rx) = connect(&state, &sim.addr.to_string(), None, None, true)
            .await
            .unwrap();
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = seen.clone();
        let expected = id.clone();
        let forwarder = tokio::spawn(forward_events(id, rx, move |ev| {
            assert_eq!(ev.camera_id, &expected);
            sink.lock().unwrap().push(ev.event.clone())
        }));
        // Let some live view frames go by, then change something.
        tokio::time::sleep(Duration::from_millis(300)).await;
        set_property(&state, None, PROP_ISO, serde_json::json!({ "U32": 1600 }))
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
        shoot(&state, None).await.unwrap();
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
        let id = CameraId("192.168.122.1:15740".into());
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
        let tagged = TaggedEvent {
            camera_id: &id,
            event: &event,
        };
        assert_eq!(
            serde_json::to_string(&tagged).unwrap(),
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
        let id = CameraId("192.168.122.1:15740".into());
        let tagged = TaggedEvent {
            camera_id: &id,
            event: &event,
        };
        assert_eq!(
            serde_json::to_string(&tagged).unwrap(),
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
    async fn the_app_finds_and_reads_the_fake_bluetooth_camera() {
        let _guard = crate::ble::tests::BLE_ENV.lock().await;
        std::env::set_var(rimix_ble::ENV, "fake");
        let dir = tempfile::tempdir().unwrap();
        let state = app_state(dir.path());
        assert!(state.settings.lock().unwrap().bluetooth);
        let found = state.ble.scan(3).await.unwrap();
        assert_eq!(found.len(), 1);
        let creds = state.ble.handoff(&found[0].address).await.unwrap();
        assert_eq!(creds.ssid, "DIRECT-abc1:ZV-E10");
        std::env::set_var(rimix_ble::ENV, "off");
        let off = AppState::new([0x42; 16], dir.path().join("other.json"));
        assert_eq!(
            off.ble.scan(3).await.unwrap_err(),
            "Bluetooth unavailable: Bluetooth is off for this run"
        );
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

    async fn two_sims() -> (SimHandle, SimHandle) {
        (
            spawn(SimConfig::default()).await,
            spawn(SimConfig {
                model: "ILCE-7M4".into(),
                ..SimConfig::default()
            })
            .await,
        )
    }

    async fn shutdown(sims: [SimHandle; 2], state: &AppState) {
        disconnect_all(state).await;
        for sim in sims {
            sim.events.send(SimCommand::Shutdown).await.unwrap();
        }
    }

    #[test]
    fn the_id_is_the_network_name_else_the_address() {
        let addr: SocketAddr = "192.168.122.1:15740".parse().unwrap();
        assert_eq!(
            camera_id(addr, Some("DIRECT-ab:ZV-E10")).0,
            "DIRECT-ab:ZV-E10"
        );
        assert_eq!(camera_id(addr, None).0, "192.168.122.1:15740");
        assert_eq!(
            serde_json::to_string(&CameraId("x".into())).unwrap(),
            r#""x""#
        );
    }

    #[tokio::test]
    async fn two_cameras_make_two_slots_and_the_newest_is_active() {
        let tmp = tempfile::tempdir().unwrap();
        let (a, b) = two_sims().await;
        let state = app_state(tmp.path());
        let (id_a, state_a, _ra) = connect(&state, &a.addr.to_string(), None, None, true)
            .await
            .unwrap();
        let (id_b, state_b, _rb) = connect(&state, &b.addr.to_string(), None, None, true)
            .await
            .unwrap();
        assert_eq!(id_a.0, a.addr.to_string());
        assert_eq!(id_b.0, b.addr.to_string());
        assert_eq!(
            (state_a.model.as_str(), state_b.model.as_str()),
            ("ZV-E10", "ILCE-7M4")
        );
        let list = list_cameras(&state).await;
        assert_eq!(
            list,
            vec![
                CameraSummary {
                    id: id_a.clone(),
                    label: "ZV-E10".into(),
                    connected: true,
                    secured: false,
                    active: false,
                },
                CameraSummary {
                    id: id_b.clone(),
                    label: "ILCE-7M4".into(),
                    connected: true,
                    secured: false,
                    active: true,
                },
            ]
        );
        // None is the active camera; an id picks that one.
        assert_eq!(get_state(&state, None).await.unwrap().model, "ILCE-7M4");
        assert_eq!(
            get_state(&state, Some(id_a.clone())).await.unwrap().model,
            "ZV-E10"
        );
        set_property(
            &state,
            Some(id_a.clone()),
            PROP_ISO,
            serde_json::json!({ "U32": 1600 }),
        )
        .await
        .unwrap();
        assert_eq!(
            short_of(
                &get_state(&state, Some(id_a.clone())).await.unwrap(),
                PROP_ISO
            ),
            "1600"
        );
        assert_ne!(
            short_of(&get_state(&state, None).await.unwrap(), PROP_ISO),
            "1600"
        );

        set_active_camera(&state, id_a.clone()).await.unwrap();
        assert_eq!(get_state(&state, None).await.unwrap().model, "ZV-E10");
        let unknown = CameraId("nope".into());
        assert_eq!(
            set_active_camera(&state, unknown.clone())
                .await
                .unwrap_err(),
            NOT_CONNECTED
        );
        assert_eq!(
            shoot(&state, Some(unknown)).await.unwrap_err(),
            NOT_CONNECTED
        );
        shutdown([a, b], &state).await;
        assert!(list_cameras(&state).await.is_empty());
        assert_eq!(get_state(&state, None).await, None);
    }

    #[tokio::test]
    async fn reconnecting_the_same_id_replaces_only_that_slot() {
        let tmp = tempfile::tempdir().unwrap();
        let (a, b) = two_sims().await;
        let state = app_state(tmp.path());
        let (id_a, _, _ra) = connect(&state, &a.addr.to_string(), None, None, true)
            .await
            .unwrap();
        connect(&state, &b.addr.to_string(), None, None, true)
            .await
            .unwrap();
        let (again, _, _rc) = connect(&state, &a.addr.to_string(), None, None, true)
            .await
            .unwrap();
        assert_eq!(again, id_a);
        let list = list_cameras(&state).await;
        assert_eq!(list.len(), 2, "{list:?}");
        assert!(list.iter().all(|c| c.connected));
        assert_eq!(list.iter().find(|c| c.active).unwrap().id, id_a);
        shutdown([a, b], &state).await;
    }

    #[tokio::test]
    async fn the_same_camera_under_another_id_replaces_its_slot() {
        let tmp = tempfile::tempdir().unwrap();
        let sim = spawn(SimConfig::default()).await;
        let state = app_state(tmp.path());
        let ip = sim.addr.to_string();
        connect(&state, &ip, Some("DIRECT-x"), None, true)
            .await
            .unwrap();
        let (id, _, _rx) = connect(&state, &ip, None, None, true).await.unwrap();
        let list = list_cameras(&state).await;
        assert_eq!(list.len(), 1, "{list:?}");
        assert_eq!(list[0].id, id);
        assert!(list[0].active);
        disconnect_all(&state).await;
        sim.events.send(SimCommand::Shutdown).await.unwrap();
    }

    #[tokio::test]
    async fn disconnecting_the_active_camera_activates_the_other() {
        let tmp = tempfile::tempdir().unwrap();
        let (a, b) = two_sims().await;
        let state = app_state(tmp.path());
        let (id_a, _, _ra) = connect(&state, &a.addr.to_string(), None, None, true)
            .await
            .unwrap();
        let (id_b, _, _rb) = connect(&state, &b.addr.to_string(), None, None, true)
            .await
            .unwrap();
        disconnect(&state, None).await.unwrap();
        let list = list_cameras(&state).await;
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, id_a);
        assert!(list[0].active);
        assert!(get_state(&state, Some(id_b)).await.is_none());
        // Disconnecting one that is not active keeps the active one.
        let (id_b, _, _rb) = connect(&state, &b.addr.to_string(), None, None, true)
            .await
            .unwrap();
        disconnect(&state, Some(id_a)).await.unwrap();
        assert_eq!(list_cameras(&state).await[0].id, id_b);
        assert!(list_cameras(&state).await[0].active);
        shutdown([a, b], &state).await;
    }

    #[tokio::test]
    async fn shoot_all_keeps_both_files_with_the_same_camera_name() {
        let tmp = tempfile::tempdir().unwrap();
        let (a, b) = two_sims().await;
        let state = app_state(tmp.path());
        let (id_a, _, ra) = connect(&state, &a.addr.to_string(), None, None, true)
            .await
            .unwrap();
        let (id_b, _, rb) = connect(&state, &b.addr.to_string(), None, None, true)
            .await
            .unwrap();
        let saved = Arc::new(StdMutex::new(Vec::new()));
        for (id, rx) in [(id_a.clone(), ra), (id_b.clone(), rb)] {
            let sink = saved.clone();
            tokio::spawn(forward_events(id, rx, move |ev| {
                if let CameraEvent::CaptureSaved(r) = ev.event {
                    sink.lock().unwrap().push((
                        ev.camera_id.clone(),
                        r.camera_filename.clone(),
                        r.filename.clone(),
                    ));
                }
            }));
        }
        let mut out = shoot_all(&state).await;
        out.sort_by(|x, y| x.0 .0.cmp(&y.0 .0));
        let mut ids = vec![(id_a.clone(), None), (id_b.clone(), None)];
        ids.sort_by(|x, y| x.0 .0.cmp(&y.0 .0));
        assert_eq!(out, ids);
        timeout(WAIT, async {
            while saved.lock().unwrap().len() < 2 {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("both shots saved");
        let mut saved = saved.lock().unwrap().clone();
        saved.sort_by(|x, y| x.2.cmp(&y.2));
        // Both cameras call their first shot DSC00001.JPG; both are kept.
        assert!(saved.iter().all(|s| s.1 == "DSC00001.JPG"), "{saved:?}");
        assert_eq!(saved[0].2, "DSC00001-1.JPG");
        assert_eq!(saved[1].2, "DSC00001.JPG");
        assert_ne!(saved[0].0, saved[1].0);
        let mut files: Vec<String> = std::fs::read_dir(tmp.path().join("shots"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        files.sort();
        assert_eq!(files, vec!["DSC00001-1.JPG", "DSC00001.JPG"]);
        shutdown([a, b], &state).await;
    }

    #[tokio::test]
    async fn shoot_all_reports_each_camera_and_needs_none() {
        let tmp = tempfile::tempdir().unwrap();
        let state = app_state(tmp.path());
        assert!(shoot_all(&state).await.is_empty());
    }

    #[tokio::test]
    async fn keep_others_false_drops_the_other_cameras() {
        let tmp = tempfile::tempdir().unwrap();
        let (a, b) = two_sims().await;
        let state = app_state(tmp.path());
        connect(&state, &a.addr.to_string(), None, None, true)
            .await
            .unwrap();
        let (id_b, _, _rb) = connect(&state, &b.addr.to_string(), None, None, false)
            .await
            .unwrap();
        let list = list_cameras(&state).await;
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, id_b);
        shutdown([a, b], &state).await;
    }

    /// The simulator's byte `i` of the card file `handle`.
    fn sim_byte(handle: u32, i: u64) -> u8 {
        i.wrapping_mul(31).wrapping_add(u64::from(handle)) as u8
    }

    #[tokio::test]
    async fn card_commands_need_a_camera() {
        let tmp = tempfile::tempdir().unwrap();
        let state = app_state(tmp.path());
        assert_eq!(card_list(&state, None).await.unwrap_err(), NOT_CONNECTED);
        assert_eq!(
            card_download(&state, None, 0x101).await.unwrap_err(),
            NOT_CONNECTED
        );
        assert_eq!(card_mode(&state, None).await.unwrap_err(), NOT_CONNECTED);
    }

    #[tokio::test]
    async fn card_list_returns_the_cards_files() {
        let tmp = tempfile::tempdir().unwrap();
        let (sim, state, _) = connected(tmp.path()).await;
        let card = card_list(&state, None).await.unwrap();
        let names: Vec<&str> = card.iter().map(|o| o.filename.as_str()).collect();
        assert_eq!(names, ["DSC00010.JPG", "DSC00010.ARW", "C0001.MP4"]);
        assert_eq!(card[0].kind, sony_ptpip::CardKind::Still);
        assert!(card.iter().all(|o| o.size_known));
        // Named by id too.
        let id = list_cameras(&state).await[0].id.clone();
        assert_eq!(card_list(&state, Some(id)).await.unwrap().len(), 3);
        stop(sim, &state).await;
    }

    #[tokio::test]
    async fn card_mode_is_same_session_for_the_zv_e10() {
        let tmp = tempfile::tempdir().unwrap();
        let (sim, state, _) = connected(tmp.path()).await;
        assert_eq!(card_mode(&state, None).await.unwrap(), "same-session");
        stop(sim, &state).await;
    }

    #[test]
    fn card_browse_names_are_kebab_case() {
        assert_eq!(
            card_mode_name(sony_ptpip::CardBrowse::SameSession),
            "same-session"
        );
        assert_eq!(
            card_mode_name(sony_ptpip::CardBrowse::NeedsTransferMode),
            "needs-transfer-mode"
        );
    }

    #[tokio::test]
    async fn card_download_saves_the_file_with_tagged_progress() {
        let tmp = tempfile::tempdir().unwrap();
        let sim = spawn(SimConfig::default()).await;
        let state = app_state(tmp.path());
        let (id, _, rx) = connect(&state, &sim.addr.to_string(), None, None, true)
            .await
            .unwrap();
        let card = card_list(&state, None).await.unwrap();
        let jpeg = card[0].clone();
        let tagged = Arc::new(StdMutex::new(Vec::<String>::new()));
        let seen = tagged.clone();
        let forward = tokio::spawn(forward_events(id.clone(), rx, move |e| {
            lock(&seen).push(serde_json::to_string(&e).unwrap());
        }));
        let path = card_download(&state, Some(id.clone()), jpeg.handle)
            .await
            .unwrap();
        let path = PathBuf::from(path);
        assert!(path.starts_with(tmp.path().join("shots")));
        let data = std::fs::read(&path).unwrap();
        assert_eq!(data.len() as u64, jpeg.bytes);
        assert!(data
            .iter()
            .enumerate()
            .all(|(i, b)| *b == sim_byte(jpeg.handle, i as u64)));
        timeout(WAIT, async {
            while !lock(&tagged).iter().any(|e| e.contains("CaptureSaved")) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("no CaptureSaved forwarded");
        let events = lock(&tagged).clone();
        let progress: Vec<&String> = events
            .iter()
            .filter(|e| e.contains("CardDownloadProgress"))
            .collect();
        assert_eq!(progress.len(), 2, "{events:?}");
        assert!(progress[1].starts_with(&format!(r#"{{"camera_id":"{id}","event""#)));
        assert!(progress[1].contains(r#""received":2097152,"total":2097152"#));
        stop(sim, &state).await;
        forward.await.unwrap();
    }

    #[tokio::test]
    async fn card_download_leaves_other_commands_free_while_it_runs() {
        let tmp = tempfile::tempdir().unwrap();
        let sim = spawn(SimConfig::default()).await;
        let state = app_state(tmp.path());
        let (_, _, mut rx) = connect(&state, &sim.addr.to_string(), None, None, true)
            .await
            .unwrap();
        let clip = card_list(&state, None)
            .await
            .unwrap()
            .into_iter()
            .find(|o| o.kind == sony_ptpip::CardKind::Video)
            .unwrap();
        let download = card_download(&state, None, clip.handle);
        let probe = async {
            loop {
                match rx.recv().await {
                    Ok(CameraEvent::CardDownloadProgress { .. }) => break,
                    Ok(_) => {}
                    Err(e) => panic!("{e}"),
                }
            }
            // Mid-download: the slots are free and the camera answers.
            assert!(state.cameras.try_lock().is_ok());
            assert_eq!(list_cameras(&state).await.len(), 1);
            assert!(get_state(&state, None).await.unwrap().connected);
        };
        let (path, ()) = timeout(Duration::from_secs(60), async {
            tokio::join!(download, probe)
        })
        .await
        .unwrap();
        assert_eq!(std::fs::metadata(path.unwrap()).unwrap().len(), clip.bytes);
        stop(sim, &state).await;
    }

    #[tokio::test]
    async fn a_refused_card_download_says_why() {
        let tmp = tempfile::tempdir().unwrap();
        let (sim, state, _) = connected(tmp.path()).await;
        let err = card_download(&state, None, 0xDEAD).await.unwrap_err();
        assert!(!err.is_empty());
        stop(sim, &state).await;
    }

    /// Pins the JSON of one card file; the same string is
    /// `src/fixtures/card-object.json`, used by `src/ipc.test.ts`.
    #[test]
    fn card_object_json_is_stable() {
        let obj = sony_ptpip::CardObject {
            handle: 0x101,
            storage: 0x0001_0001,
            filename: "DSC00010.JPG".into(),
            bytes: 2 * 1024 * 1024,
            size_known: true,
            format: sony_ptpip::ptp::codes::FMT_JPEG,
            kind: sony_ptpip::CardKind::Still,
            captured: sony_ptpip::card::parse_ptp_date("20261111T140300"),
        };
        assert_eq!(
            serde_json::to_string(&obj).unwrap(),
            CARD_OBJECT_FIXTURE.trim()
        );
    }

    const CARD_OBJECT_FIXTURE: &str = include_str!("../../src/fixtures/card-object.json");

    // ---- Receive from camera (SFTP) ----

    struct AnyHost;

    impl russh::client::Handler for AnyHost {
        type Error = russh::Error;

        async fn check_server_key(
            &mut self,
            _key: &russh::keys::PublicKeyOrCertificate,
        ) -> Result<bool, Self::Error> {
            Ok(true)
        }
    }

    /// Logs in to the receiver on `port` the way the camera would; `None`
    /// when the login is refused.
    async fn sftp_login(
        port: u16,
        user: &str,
        password: &str,
    ) -> Option<(
        russh::client::Handle<AnyHost>,
        russh_sftp::client::SftpSession,
    )> {
        let addr: SocketAddr = ([127, 0, 0, 1], port).into();
        let config = Arc::new(russh::client::Config::default());
        let mut handle = russh::client::connect(config, addr, AnyHost)
            .await
            .expect("ssh connect");
        let auth = handle.authenticate_password(user, password).await.unwrap();
        if !auth.success() {
            return None;
        }
        let channel = handle.channel_open_session().await.unwrap();
        channel.request_subsystem(true, "sftp").await.unwrap();
        let session = russh_sftp::client::SftpSession::new(channel.into_stream())
            .await
            .unwrap();
        Some((handle, session))
    }

    async fn upload(port: u16, user: &str, password: &str, name: &str, data: &[u8]) {
        use tokio::io::AsyncWriteExt;
        let (_ssh, sftp) = sftp_login(port, user, password)
            .await
            .expect("login refused");
        let mut file = sftp.create(name).await.unwrap();
        file.write_all(data).await.unwrap();
        file.shutdown().await.unwrap();
    }

    /// An app state receiving on a free port, with a fake Wi-Fi, whose
    /// emitted events are collected as JSON.
    fn sftp_state(dir: &Path) -> (AppState, Arc<StdMutex<Vec<String>>>) {
        let path = dir.join("config").join("settings.json");
        let mut settings = AppSettings {
            destination_base: dir.join("shots"),
            destination_mode: DestinationMode::CustomFolder,
            ..AppSettings::default()
        };
        settings.sftp.port = 0;
        settings::save(&path, &settings).unwrap();
        let mut state = AppState::new([0x42; 16], path);
        state.wifi = rimix_wifi::Wifi::fake();
        let seen: Arc<StdMutex<Vec<String>>> = Arc::default();
        let sink = seen.clone();
        state.sftp.set_emit(Arc::new(move |ev: TaggedEvent<'_>| {
            lock(&sink).push(serde_json::to_string(&ev).unwrap());
        }));
        (state, seen)
    }

    async fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
        timeout(WAIT, async {
            while !done() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("{what}"));
    }

    #[tokio::test]
    async fn enabling_sftp_receives_an_upload_into_the_destination_tagged_sftp() {
        let tmp = tempfile::tempdir().unwrap();
        let (state, seen) = sftp_state(tmp.path());
        let off = crate::sftp::status(&state).await;
        assert!(!off.running);
        assert_eq!(off.password.len(), 12, "made on first use");
        assert_eq!(get_settings(&state).sftp.password, off.password);

        crate::sftp::set_enabled(&state, true).await.unwrap();
        let st = crate::sftp::status(&state).await;
        assert!(!off.enabled);
        assert!(st.enabled);
        assert!(st.running, "{:?}", st.error);
        assert_ne!(st.port, 0);
        assert_eq!(st.user, "rimix");
        assert_eq!(st.password, off.password);
        assert!(st.fingerprint.as_deref().unwrap().starts_with("SHA256:"));
        assert_eq!(st.error, None);
        assert_eq!(st.address, None, "the fake Wi-Fi has no address");
        assert!(tmp.path().join("config").join("sftp_host_key").exists());
        assert!(settings::load(&state.settings_path).sftp.enabled);

        upload(st.port, &st.user, &st.password, "/DSC00001.JPG", b"jpeg").await;
        let saved = tmp.path().join("shots").join("DSC00001.JPG");
        wait_for("no event forwarded", || {
            lock(&seen).iter().any(|e| e.contains("CaptureSaved"))
        })
        .await;
        assert_eq!(std::fs::read(&saved).unwrap(), b"jpeg");
        let event = lock(&seen)[0].clone();
        let json: serde_json::Value = serde_json::from_str(&event).unwrap();
        assert_eq!(json["camera_id"], "sftp");
        let r = &json["event"]["CaptureSaved"];
        assert_eq!(r["filename"], "DSC00001.JPG");
        assert_eq!(r["camera_filename"], "/DSC00001.JPG");
        assert_eq!(r["bytes"], 4);
        assert_eq!(r["path"], saved.display().to_string());

        crate::sftp::set_enabled(&state, false).await.unwrap();
        assert!(!crate::sftp::status(&state).await.running);
        assert!(!settings::load(&state.settings_path).sftp.enabled);
        assert!(tokio::net::TcpStream::connect(("127.0.0.1", st.port))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn a_port_in_use_is_shown_as_the_error() {
        let tmp = tempfile::tempdir().unwrap();
        let (state, _) = sftp_state(tmp.path());
        let taken = std::net::TcpListener::bind("0.0.0.0:0").unwrap();
        let port = taken.local_addr().unwrap().port();
        lock(&state.settings).sftp.port = port;
        crate::sftp::set_enabled(&state, true).await.unwrap();
        let st = crate::sftp::status(&state).await;
        assert!(!st.running);
        assert_eq!(st.error, Some(format!("Port {port} is in use")));
        assert_eq!(st.port, port);
        crate::sftp::set_enabled(&state, false).await.unwrap();
        assert_eq!(crate::sftp::status(&state).await.error, None);
    }

    #[tokio::test]
    async fn a_new_destination_restarts_the_receiver_there_and_keeps_its_settings() {
        let tmp = tempfile::tempdir().unwrap();
        let (state, _) = sftp_state(tmp.path());
        crate::sftp::set_enabled(&state, true).await.unwrap();
        let before = crate::sftp::status(&state).await;
        // The webview saves settings without the sftp fields.
        let mut s = get_settings(&state);
        s.destination_base = tmp.path().join("elsewhere");
        s.sftp = settings::SftpSettings::default();
        save_settings(&state, s).await.unwrap();
        let after = crate::sftp::status(&state).await;
        assert!(after.running);
        assert_eq!(after.password, before.password);
        assert_eq!(after.fingerprint, before.fingerprint);
        assert!(settings::load(&state.settings_path).sftp.enabled);
        upload(after.port, &after.user, &after.password, "/A.JPG", b"a").await;
        let moved = tmp.path().join("elsewhere").join("A.JPG");
        wait_for("not saved in the new destination", || moved.exists()).await;
        crate::sftp::stop(&state).await;
    }

    #[tokio::test]
    async fn a_new_password_is_saved_and_the_old_one_stops_working() {
        let tmp = tempfile::tempdir().unwrap();
        let (state, _) = sftp_state(tmp.path());
        crate::sftp::set_enabled(&state, true).await.unwrap();
        let old = crate::sftp::status(&state).await;
        crate::sftp::regenerate_password(&state).await.unwrap();
        let new = crate::sftp::status(&state).await;
        assert!(new.running);
        assert_ne!(new.password, old.password);
        assert_eq!(new.password.len(), 12);
        assert_eq!(
            settings::load(&state.settings_path).sftp.password,
            new.password
        );
        assert!(sftp_login(new.port, "rimix", &old.password).await.is_none());
        assert!(sftp_login(new.port, "rimix", &new.password).await.is_some());
        crate::sftp::stop(&state).await;
    }

    fn day(y: i32, m: u32, d: u32) -> chrono::NaiveDate {
        chrono::NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[tokio::test]
    async fn with_a_folder_per_date_uploads_land_in_todays_folder_and_follow_the_day() {
        let tmp = tempfile::tempdir().unwrap();
        let (state, seen) = sftp_state(tmp.path());
        lock(&state.settings).destination_mode = DestinationMode::FolderPerDate;
        crate::sftp::set_enabled(&state, true).await.unwrap();
        let st = crate::sftp::status(&state).await;
        assert!(st.running, "{:?}", st.error);
        upload(st.port, &st.user, &st.password, "/A.JPG", b"a").await;
        let today = chrono::Local::now()
            .date_naive()
            .format("%Y-%m-%d")
            .to_string();
        let saved = tmp.path().join("shots").join(&today).join("A.JPG");
        wait_for("not saved in today's folder", || saved.exists()).await;
        wait_for("no event", || !lock(&seen).is_empty()).await;
        assert!(lock(&seen)[0].contains(&today), "{:?}", lock(&seen));

        // The same day changes nothing; a new day moves the receiver.
        assert!(
            !crate::sftp::restart_if_day_changed(&state, chrono::Local::now().date_naive()).await
        );
        let next = day(2031, 1, 2);
        assert!(crate::sftp::restart_if_day_changed(&state, next).await);
        let st = crate::sftp::status(&state).await;
        assert!(st.running);
        upload(st.port, &st.user, &st.password, "/B.JPG", b"b").await;
        let moved = tmp.path().join("shots").join("2031-01-02").join("B.JPG");
        wait_for("not saved in the new day's folder", || moved.exists()).await;
        crate::sftp::stop(&state).await;
        // Stopped: a new day starts nothing.
        assert!(!crate::sftp::restart_if_day_changed(&state, day(2031, 1, 3)).await);
        assert!(!crate::sftp::status(&state).await.running);
    }

    #[tokio::test]
    async fn a_custom_folder_receiver_is_not_restarted_at_midnight() {
        let tmp = tempfile::tempdir().unwrap();
        let (state, _) = sftp_state(tmp.path());
        crate::sftp::set_enabled(&state, true).await.unwrap();
        let before = crate::sftp::status(&state).await;
        assert!(before.running, "{:?}", before.error);
        // Same folder on the new day: nothing to move.
        assert!(!crate::sftp::restart_if_day_changed(&state, day(2031, 1, 2)).await);
        let after = crate::sftp::status(&state).await;
        assert!(after.running);
        assert_eq!(after.port, before.port, "the receiver was restarted");
        crate::sftp::stop(&state).await;
    }

    #[tokio::test]
    async fn a_day_change_restart_does_not_start_a_switched_off_receiver() {
        let tmp = tempfile::tempdir().unwrap();
        let (state, _) = sftp_state(tmp.path());
        lock(&state.settings).destination_mode = DestinationMode::FolderPerDate;
        crate::sftp::set_enabled(&state, true).await.unwrap();
        assert!(crate::sftp::status(&state).await.running);
        // A disable lands after the day-change check but before the start.
        lock(&state.settings).sftp.enabled = false;
        crate::sftp::restart_if_day_changed(&state, day(2031, 1, 2)).await;
        assert!(!crate::sftp::status(&state).await.running);
        crate::sftp::stop(&state).await;
    }

    #[tokio::test]
    async fn changing_the_save_mode_restarts_the_receiver_into_the_new_layout() {
        let tmp = tempfile::tempdir().unwrap();
        let (state, _) = sftp_state(tmp.path());
        crate::sftp::set_enabled(&state, true).await.unwrap();
        let mut s = get_settings(&state);
        s.destination_mode = DestinationMode::FolderPerDate;
        save_settings(&state, s).await.unwrap();
        let st = crate::sftp::status(&state).await;
        upload(st.port, &st.user, &st.password, "/C.JPG", b"c").await;
        let today = chrono::Local::now()
            .date_naive()
            .format("%Y-%m-%d")
            .to_string();
        let saved = tmp.path().join("shots").join(today).join("C.JPG");
        wait_for("not saved in today's folder", || saved.exists()).await;
        crate::sftp::stop(&state).await;
    }

    #[tokio::test]
    async fn the_fingerprint_shows_even_when_the_port_is_in_use() {
        let tmp = tempfile::tempdir().unwrap();
        let (state, _) = sftp_state(tmp.path());
        assert_eq!(
            crate::sftp::status(&state).await.fingerprint,
            None,
            "no key yet"
        );
        crate::sftp::set_enabled(&state, true).await.unwrap();
        let running = crate::sftp::status(&state).await.fingerprint.unwrap();
        crate::sftp::stop(&state).await;
        let taken = std::net::TcpListener::bind("0.0.0.0:0").unwrap();
        lock(&state.settings).sftp.port = taken.local_addr().unwrap().port();
        crate::sftp::set_enabled(&state, true).await.unwrap();
        let st = crate::sftp::status(&state).await;
        assert!(!st.running);
        assert!(st.error.is_some());
        assert_eq!(st.fingerprint.as_deref(), Some(running.as_str()));
        crate::sftp::set_enabled(&state, false).await.unwrap();
    }

    #[test]
    fn sftp_status_json_is_stable() {
        let st = crate::sftp::SftpStatus {
            enabled: true,
            running: true,
            address: Some("192.168.1.20:2222".into()),
            port: 2222,
            user: "rimix".into(),
            password: "abc".into(),
            fingerprint: Some("SHA256:x".into()),
            error: None,
        };
        assert_eq!(
            serde_json::to_string(&st).unwrap(),
            r#"{"enabled":true,"running":true,"address":"192.168.1.20:2222","port":2222,"user":"rimix","password":"abc","fingerprint":"SHA256:x","error":null}"#
        );
    }
}

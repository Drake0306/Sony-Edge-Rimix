//! Receive from camera: the SFTP server the camera's FTP Transfer function
//! pushes files to. Each file it finishes is forwarded to the webview as a
//! `CaptureSaved` tagged with the camera id [`SFTP_CAMERA_ID`], so it lands
//! in the filmstrip like a shot.

use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex, MutexGuard};

use chrono::NaiveDate;

use rimix_sftp::{ReceivedFile, Receiver, ReceiverConfig};
use sony_ptpip::capture::Received;
use sony_ptpip::CameraEvent;

use crate::settings::{self, SftpSettings};
use crate::state::{AppState, CameraId, TaggedEvent};

/// The camera id received files are tagged with.
pub const SFTP_CAMERA_ID: &str = "sftp";

/// Where tagged events go: the webview in the app, a list in tests.
pub type Emit = Arc<dyn Fn(TaggedEvent<'_>) + Send + Sync>;

/// The running receiver, if any, and why it last failed to start.
#[derive(Default)]
pub struct SftpService {
    receiver: tokio::sync::Mutex<Option<Receiver>>,
    /// The day the running receiver's folder was chosen for.
    started_on: StdMutex<Option<NaiveDate>>,
    last_error: StdMutex<Option<String>>,
    emit: StdMutex<Option<Emit>>,
}

fn lock<T>(m: &StdMutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl SftpService {
    /// Sets where received files are announced.
    pub fn set_emit(&self, emit: Emit) {
        *lock(&self.emit) = Some(emit);
    }
}

/// What `sftp_status` gives the webview.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct SftpStatus {
    /// Switched on by the user; it may still have failed to start.
    pub enabled: bool,
    pub running: bool,
    /// `<lan-ip>:<port>`, or `None` when this computer's address is unknown.
    pub address: Option<String>,
    /// The port listened on (or to be listened on).
    pub port: u16,
    pub user: String,
    pub password: String,
    pub fingerprint: Option<String>,
    pub error: Option<String>,
}

/// The receiver's settings, making and saving the password on first use.
/// The file is written under the settings lock, like every settings save,
/// so an older snapshot can never land on disk after a newer one.
fn sftp_settings(state: &AppState) -> SftpSettings {
    let mut s = lock(&state.settings);
    if s.sftp.ensure_password() {
        if let Err(e) = settings::save(&state.settings_path, &s) {
            tracing::warn!(error = %e, "cannot save settings after making the SFTP password");
        }
    }
    s.sftp.clone()
}

/// Changes the stored receiver settings and saves them (under the settings
/// lock, see [`sftp_settings`]).
fn update(state: &AppState, change: impl FnOnce(&mut SftpSettings)) -> Result<(), String> {
    let mut s = lock(&state.settings);
    change(&mut s.sftp);
    settings::save(&state.settings_path, &s).map_err(|e| format!("Cannot save settings: {e}"))
}

/// `<config>/sftp_host_key`, next to the settings file.
fn host_key_path(state: &AppState) -> PathBuf {
    state.settings_path.with_file_name("sftp_host_key")
}

/// The fingerprint of the host key on disk, once one has been made.
fn saved_fingerprint(state: &AppState) -> Option<String> {
    let path = host_key_path(state);
    if !path.exists() {
        return None;
    }
    rimix_sftp::load_or_create_host_key(&path)
        .ok()
        .map(|key| rimix_sftp::host_key_fingerprint(&key))
}

/// The event a received file becomes.
fn captured(file: ReceivedFile) -> CameraEvent {
    let filename = file
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    CameraEvent::CaptureSaved(Received {
        path: file.path,
        bytes: file.bytes,
        filename,
        camera_filename: file.remote_name,
    })
}

fn today() -> NaiveDate {
    chrono::Local::now().date_naive()
}

/// (Re)starts the receiver with the current settings, writing where shots
/// go today, and records a failure for [`status`].
async fn start(state: &AppState) {
    start_on(state, today()).await;
}

/// [`start`] for the day `day`: with a folder per date, files go into that
/// day's folder. The switch is read again under the receiver lock, so a
/// turn-off that lands between a caller's check and this start wins: the
/// old receiver stops and no new one starts.
async fn start_on(state: &AppState, day: NaiveDate) {
    let mut slot = state.sftp.receiver.lock().await;
    if let Some(old) = slot.take() {
        old.stop().await;
    }
    *lock(&state.sftp.started_on) = None;
    if !lock(&state.settings).sftp.enabled {
        *lock(&state.sftp.last_error) = None;
        return;
    }
    let cfg = sftp_settings(state);
    let target = lock(&state.settings).destination();
    let emit = lock(&state.sftp.emit).clone();
    let on_file = move |file: ReceivedFile| {
        tracing::info!(path = %file.path.display(), bytes = file.bytes, "received a file over SFTP");
        if let Some(emit) = &emit {
            let id = CameraId(SFTP_CAMERA_ID.to_string());
            emit(TaggedEvent {
                camera_id: &id,
                event: &captured(file),
            });
        }
    };
    let result = async {
        let destination = target
            .ensure_writable(day)
            .map_err(|e| format!("Cannot save to {}: {e}", target.target_dir(day).display()))?;
        let host_key = rimix_sftp::load_or_create_host_key(&host_key_path(state))
            .map_err(|e| format!("Cannot read the host key: {e}"))?;
        let bind = SocketAddr::from((Ipv4Addr::UNSPECIFIED, cfg.port));
        Receiver::start(
            ReceiverConfig {
                bind,
                user: cfg.user,
                password: cfg.password,
                host_key,
                destination,
            },
            on_file,
        )
        .await
        .map_err(|e| match e {
            rimix_sftp::Error::Io(io) if io.kind() == std::io::ErrorKind::AddrInUse => {
                format!("Port {} is in use", cfg.port)
            }
            e => format!("Cannot start receiving: {e}"),
        })
    }
    .await;
    match result {
        Ok(receiver) => {
            *slot = Some(receiver);
            *lock(&state.sftp.started_on) = Some(day);
            *lock(&state.sftp.last_error) = None;
        }
        Err(e) => {
            tracing::warn!(error = %e, "SFTP receiver did not start");
            *lock(&state.sftp.last_error) = Some(e);
        }
    }
}

/// Stops the receiver, if running, and clears its error.
pub async fn stop(state: &AppState) {
    if let Some(receiver) = state.sftp.receiver.lock().await.take() {
        receiver.stop().await;
    }
    *lock(&state.sftp.started_on) = None;
    *lock(&state.sftp.last_error) = None;
}

/// Starts the receiver when the settings say it is on (at app start).
pub async fn start_if_enabled(state: &AppState) {
    if lock(&state.settings).sftp.enabled {
        start(state).await;
    }
}

/// After a settings change: a receiver that is on restarts so it writes
/// into the new destination.
pub async fn restart_if_enabled(state: &AppState) {
    start_if_enabled(state).await;
}

/// When `day`'s folder differs from the one the running receiver writes
/// into (with a folder per date, after midnight), restarts it so files go
/// into `day`'s folder. A custom folder is the same every day and is left
/// alone. True when it restarted. The app calls this every minute, which
/// also catches a computer waking from sleep after midnight.
pub async fn restart_if_day_changed(state: &AppState, day: NaiveDate) -> bool {
    let started = *lock(&state.sftp.started_on);
    let Some(on) = started.filter(|on| *on != day) else {
        return false;
    };
    let target = lock(&state.settings).destination();
    if target.target_dir(day) == target.target_dir(on) {
        return false;
    }
    start_on(state, day).await;
    true
}

/// Every minute, moves a running receiver to the new day's folder after
/// midnight. Runs until the app quits.
pub async fn follow_the_day(state: &AppState) {
    let mut tick = tokio::time::interval(std::time::Duration::from_secs(60));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tick.tick().await;
        restart_if_day_changed(state, today()).await;
    }
}

/// Turns receiving on or off and remembers it. A receiver that cannot
/// start (its port is taken, say) is still on; [`status`] says why.
pub async fn set_enabled(state: &AppState, enabled: bool) -> Result<(), String> {
    update(state, |s| s.enabled = enabled)?;
    if enabled {
        start(state).await;
    } else {
        stop(state).await;
    }
    Ok(())
}

/// Makes and saves a new password; a receiver that is on restarts with it.
pub async fn regenerate_password(state: &AppState) -> Result<(), String> {
    update(state, |s| s.password = settings::new_sftp_password())?;
    restart_if_enabled(state).await;
    Ok(())
}

/// Whether the receiver runs, where the camera should send to and how it
/// logs in.
pub async fn status(state: &AppState) -> SftpStatus {
    let cfg = sftp_settings(state);
    let (running, port, fingerprint) = match &*state.sftp.receiver.lock().await {
        Some(r) => (true, r.local_addr().port(), Some(r.fingerprint_sha256())),
        None => (false, cfg.port, saved_fingerprint(state)),
    };
    let wifi = state.wifi.clone();
    let ip = tokio::task::spawn_blocking(move || wifi.current_address())
        .await
        .ok()
        .and_then(Result::ok)
        .flatten();
    SftpStatus {
        enabled: cfg.enabled,
        running,
        address: ip.map(|ip| format!("{ip}:{port}")),
        port,
        user: cfg.user,
        password: cfg.password,
        fingerprint,
        error: lock(&state.sftp.last_error).clone(),
    }
}

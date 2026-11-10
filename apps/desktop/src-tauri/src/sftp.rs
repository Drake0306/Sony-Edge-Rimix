//! Receive from camera: the SFTP server the camera's FTP Transfer function
//! pushes files to. Each file it finishes is forwarded to the webview as a
//! `CaptureSaved` tagged with the camera id [`SFTP_CAMERA_ID`], so it lands
//! in the filmstrip like a shot.

use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex, MutexGuard};

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
fn sftp_settings(state: &AppState) -> SftpSettings {
    let snapshot = {
        let mut s = lock(&state.settings);
        if !s.sftp.ensure_password() {
            return s.sftp.clone();
        }
        s.clone()
    };
    if let Err(e) = settings::save(&state.settings_path, &snapshot) {
        tracing::warn!(error = %e, "cannot save settings after making the SFTP password");
    }
    snapshot.sftp
}

/// Changes the stored receiver settings and saves them.
fn update(state: &AppState, change: impl FnOnce(&mut SftpSettings)) -> Result<(), String> {
    let snapshot = {
        let mut s = lock(&state.settings);
        change(&mut s.sftp);
        s.clone()
    };
    settings::save(&state.settings_path, &snapshot)
        .map_err(|e| format!("Cannot save settings: {e}"))
}

/// `<config>/sftp_host_key`, next to the settings file.
fn host_key_path(state: &AppState) -> PathBuf {
    state.settings_path.with_file_name("sftp_host_key")
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

/// (Re)starts the receiver with the current settings and destination,
/// recording a failure for [`status`].
async fn start(state: &AppState) {
    let mut slot = state.sftp.receiver.lock().await;
    if let Some(old) = slot.take() {
        old.stop().await;
    }
    let cfg = sftp_settings(state);
    let destination = lock(&state.settings).destination_base.clone();
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
        None => (false, cfg.port, None),
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

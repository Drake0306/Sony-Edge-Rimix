//! Tauri commands: thin wrappers over the functions in `state.rs`.

use sony_ptpip::{CameraState, CardObject, DiscoveredCamera};
use tauri::ipc::Response;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

use crate::settings::{AppSettings, PresetValue};
use crate::state::{self, AppState, ApplyOutcome, CameraId, CameraSummary, SshLogin};

/// The Tauri event that carries each `CameraEvent` (except `Frame`) as JSON,
/// tagged with its camera: `{"camera_id": "...", "event": ...}`.
pub const CAMERA_EVENT: &str = "camera-event";

#[tauri::command]
pub async fn discover(timeout_ms: u64) -> Result<Vec<DiscoveredCamera>, String> {
    state::discover(timeout_ms).await
}

/// What `connect` gives the webview: the new camera's id and state.
#[derive(serde::Serialize)]
pub struct ConnectResult {
    pub id: CameraId,
    pub state: CameraState,
}

/// Connects (through the camera's SSH tunnel when `ssh` is given), makes it
/// the active camera and forwards its events, tagged with its id, to the
/// webview until it is disconnected or replaced. Other cameras stay
/// connected unless `keep_others` is false.
#[tauri::command]
pub async fn connect(
    app: AppHandle,
    state: State<'_, AppState>,
    ip: String,
    ssid: Option<String>,
    ssh: Option<SshLogin>,
    keep_others: Option<bool>,
) -> Result<ConnectResult, String> {
    let (id, camera_state, events) = state::connect(
        &state,
        &ip,
        ssid.as_deref(),
        ssh,
        keep_others.unwrap_or(true),
    )
    .await?;
    tauri::async_runtime::spawn(state::forward_events(id.clone(), events, move |event| {
        if let Err(e) = app.emit(CAMERA_EVENT, event) {
            tracing::warn!(error = %e, "cannot send a camera event to the window");
        }
    }));
    Ok(ConnectResult {
        id,
        state: camera_state,
    })
}

/// Disconnects the camera `camera_id` names, or the active one.
#[tauri::command]
pub async fn disconnect(
    state: State<'_, AppState>,
    camera_id: Option<CameraId>,
) -> Result<(), String> {
    state::disconnect(&state, camera_id).await
}

#[tauri::command]
pub async fn disconnect_all(state: State<'_, AppState>) -> Result<(), String> {
    state::disconnect_all(&state).await;
    Ok(())
}

#[tauri::command]
pub async fn list_cameras(state: State<'_, AppState>) -> Result<Vec<CameraSummary>, String> {
    Ok(state::list_cameras(&state).await)
}

#[tauri::command]
pub async fn set_active_camera(state: State<'_, AppState>, id: CameraId) -> Result<(), String> {
    state::set_active_camera(&state, id).await
}

/// Shoots every connected camera at once: `[id, error or null]` per camera.
#[tauri::command]
pub async fn shoot_all(
    state: State<'_, AppState>,
) -> Result<Vec<(CameraId, Option<String>)>, String> {
    Ok(state::shoot_all(&state).await)
}

#[tauri::command]
pub async fn get_state(
    state: State<'_, AppState>,
    camera_id: Option<CameraId>,
) -> Result<Option<CameraState>, String> {
    Ok(state::get_state(&state, camera_id).await)
}

#[tauri::command]
pub async fn set_property(
    state: State<'_, AppState>,
    camera_id: Option<CameraId>,
    code: u16,
    raw: serde_json::Value,
) -> Result<(), String> {
    state::set_property(&state, camera_id, code, raw).await
}

#[tauri::command]
pub async fn apply_values(
    state: State<'_, AppState>,
    camera_id: Option<CameraId>,
    values: Vec<PresetValue>,
) -> Result<Vec<ApplyOutcome>, String> {
    state::apply_values(&state, camera_id, values).await
}

#[tauri::command]
pub async fn step_property(
    state: State<'_, AppState>,
    camera_id: Option<CameraId>,
    code: u16,
    delta: i32,
) -> Result<(), String> {
    state::step_property(&state, camera_id, code, delta).await
}

#[tauri::command]
pub async fn shoot(state: State<'_, AppState>, camera_id: Option<CameraId>) -> Result<(), String> {
    state::shoot(&state, camera_id).await
}

#[tauri::command]
pub async fn focus(state: State<'_, AppState>, camera_id: Option<CameraId>) -> Result<(), String> {
    state::focus(&state, camera_id).await
}

#[tauri::command]
pub async fn toggle_record(
    state: State<'_, AppState>,
    camera_id: Option<CameraId>,
) -> Result<(), String> {
    state::toggle_record(&state, camera_id).await
}

/// Every file on the camera's cards.
#[tauri::command]
pub async fn card_list(
    state: State<'_, AppState>,
    camera_id: Option<CameraId>,
) -> Result<Vec<CardObject>, String> {
    state::card_list(&state, camera_id).await
}

/// Downloads a card file and resolves with where it was saved. It may take
/// minutes; the webview follows its `CardDownloadProgress` events.
#[tauri::command]
pub async fn card_download(
    state: State<'_, AppState>,
    camera_id: Option<CameraId>,
    handle: u32,
) -> Result<String, String> {
    state::card_download(&state, camera_id, handle).await
}

/// `same-session` or `needs-transfer-mode`.
#[tauri::command]
pub async fn card_mode(
    state: State<'_, AppState>,
    camera_id: Option<CameraId>,
) -> Result<String, String> {
    state::card_mode(&state, camera_id).await
}

/// The camera's latest JPEG as a raw body (an ArrayBuffer in the webview).
#[tauri::command]
pub async fn live_view_frame(
    state: State<'_, AppState>,
    camera_id: Option<CameraId>,
) -> Result<Response, String> {
    Ok(Response::new(
        state::live_view_frame(&state, camera_id).await?,
    ))
}

#[tauri::command]
pub async fn live_view_meta(
    state: State<'_, AppState>,
    camera_id: Option<CameraId>,
) -> Result<serde_json::Value, String> {
    state::live_view_meta(&state, camera_id).await
}

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> Result<AppSettings, String> {
    Ok(state::get_settings(&state))
}

#[tauri::command]
pub async fn save_settings(
    state: State<'_, AppState>,
    settings: AppSettings,
) -> Result<(), String> {
    state::save_settings(&state, settings).await
}

/// A saved JPEG's bytes for the filmstrip (an ArrayBuffer in the webview).
#[tauri::command]
pub async fn read_thumbnail(state: State<'_, AppState>, path: String) -> Result<Response, String> {
    let path = std::path::PathBuf::from(path);
    let allowed = vec![
        state
            .settings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .destination_base
            .clone(),
        sony_ptpip::capture::default_recovery_dir(),
    ];
    tauri::async_runtime::spawn_blocking(move || state::read_thumbnail(&path, &allowed))
        .await
        .map_err(|e| e.to_string())?
        .map(Response::new)
}

/// Asks for a folder; `None` when the dialog is cancelled.
#[tauri::command]
pub async fn pick_folder(app: AppHandle) -> Result<Option<String>, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog().file().pick_folder(move |folder| {
        let _ = tx.send(folder);
    });
    let folder = rx.await.map_err(|e| e.to_string())?;
    folder
        .map(|f| f.into_path().map(|p| p.display().to_string()))
        .transpose()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn join_network(
    state: State<'_, AppState>,
    ssid: String,
    password: Option<String>,
) -> Result<(), String> {
    state::join_network(&state, &ssid, password.as_deref()).await
}

#[tauri::command]
pub async fn current_network(state: State<'_, AppState>) -> Result<Option<String>, String> {
    state::current_network(&state).await
}

#[tauri::command]
pub async fn wait_for_camera(timeout_ms: u64) -> Result<DiscoveredCamera, String> {
    state::wait_for_camera(std::time::Duration::from_millis(timeout_ms)).await
}

/// Opens the logs folder in Finder (or the system's file manager).
#[tauri::command]
pub async fn open_logs(app: AppHandle) -> Result<(), String> {
    let config = crate::settings::config_dir().ok_or("Cannot find the config folder")?;
    let dir = crate::logging::log_dir(&config);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    app.opener()
        .open_path(dir.display().to_string(), None::<&str>)
        .map_err(|e| e.to_string())
}

/// Whether Receive from camera runs, its address, login and fingerprint.
#[tauri::command]
pub async fn sftp_status(state: State<'_, AppState>) -> Result<crate::sftp::SftpStatus, String> {
    Ok(crate::sftp::status(&state).await)
}

/// Turns Receive from camera on or off; a failure to start shows in
/// `sftp_status`.
#[tauri::command]
pub async fn sftp_set_enabled(state: State<'_, AppState>, enabled: bool) -> Result<(), String> {
    crate::sftp::set_enabled(&state, enabled).await
}

/// Makes a new SFTP password; a running receiver restarts with it.
#[tauri::command]
pub async fn sftp_regenerate_password(state: State<'_, AppState>) -> Result<(), String> {
    crate::sftp::regenerate_password(&state).await
}

/// The Sony cameras heard over Bluetooth in `seconds` (1 to 15). When
/// Bluetooth is unavailable the error starts `Bluetooth unavailable:`, and
/// later calls fail the same way at once until `ble_retry`.
#[tauri::command]
pub async fn ble_scan(
    state: State<'_, AppState>,
    seconds: u64,
) -> Result<Vec<crate::ble::BleFound>, String> {
    state.ble.scan(seconds).await
}

/// The camera's Wi-Fi network and password, read over Bluetooth. The
/// webview joins it at once; neither side stores or logs the password.
#[tauri::command]
pub async fn ble_handoff(
    state: State<'_, AppState>,
    address: String,
) -> Result<crate::ble::BleCredentials, String> {
    state.ble.handoff(&address).await
}

/// One photo through the camera's Bluetooth remote shutter.
#[tauri::command]
pub async fn ble_shoot(state: State<'_, AppState>, address: String) -> Result<(), String> {
    state.ble.shoot(&address).await
}

/// Tries Bluetooth again after it was unavailable.
#[tauri::command]
pub async fn ble_retry(state: State<'_, AppState>) -> Result<(), String> {
    state.ble.retry().await;
    Ok(())
}

/// Sends received files to the webview as `CaptureSaved` events tagged
/// `sftp`, starts the receiver if it is on and moves it to each new
/// day's folder after midnight.
pub fn start_sftp(app: &AppHandle) {
    let handle = app.clone();
    let state = app.state::<AppState>();
    state.sftp.set_emit(std::sync::Arc::new(move |event| {
        if let Err(e) = handle.emit(CAMERA_EVENT, event) {
            tracing::warn!(error = %e, "cannot send a received file to the window");
        }
    }));
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        crate::sftp::start_if_enabled(&state).await;
        crate::sftp::follow_the_day(&state).await;
    });
}

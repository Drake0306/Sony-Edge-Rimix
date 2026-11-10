//! Tauri commands: thin wrappers over the functions in `state.rs`.

use sony_ptpip::{CameraState, DiscoveredCamera};
use tauri::ipc::Response;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_dialog::DialogExt;

use crate::settings::{AppSettings, PresetValue};
use crate::state::{self, AppState, ApplyOutcome};

/// The Tauri event that carries each `CameraEvent` (except `Frame`) as JSON.
pub const CAMERA_EVENT: &str = "camera-event";

#[tauri::command]
pub async fn discover(timeout_ms: u64) -> Result<Vec<DiscoveredCamera>, String> {
    state::discover(timeout_ms).await
}

/// Connects and forwards the camera's events to the webview until it is
/// disconnected or replaced.
#[tauri::command]
pub async fn connect(
    app: AppHandle,
    state: State<'_, AppState>,
    ip: String,
    ssid: Option<String>,
) -> Result<CameraState, String> {
    let (camera_state, events) = state::connect(&state, &ip, ssid.as_deref()).await?;
    tauri::async_runtime::spawn(state::forward_events(events, move |event| {
        if let Err(e) = app.emit(CAMERA_EVENT, event) {
            tracing::warn!(error = %e, "cannot send a camera event to the window");
        }
    }));
    Ok(camera_state)
}

#[tauri::command]
pub async fn disconnect(state: State<'_, AppState>) -> Result<(), String> {
    state::disconnect(&state).await
}

#[tauri::command]
pub async fn get_state(state: State<'_, AppState>) -> Result<Option<CameraState>, String> {
    Ok(state::get_state(&state).await)
}

#[tauri::command]
pub async fn set_property(
    state: State<'_, AppState>,
    code: u16,
    raw: serde_json::Value,
) -> Result<(), String> {
    state::set_property(&state, code, raw).await
}

#[tauri::command]
pub async fn apply_values(
    state: State<'_, AppState>,
    values: Vec<PresetValue>,
) -> Result<Vec<ApplyOutcome>, String> {
    state::apply_values(&state, values).await
}

#[tauri::command]
pub async fn step_property(
    state: State<'_, AppState>,
    code: u16,
    delta: i32,
) -> Result<(), String> {
    state::step_property(&state, code, delta).await
}

#[tauri::command]
pub async fn shoot(state: State<'_, AppState>) -> Result<(), String> {
    state::shoot(&state).await
}

#[tauri::command]
pub async fn focus(state: State<'_, AppState>) -> Result<(), String> {
    state::focus(&state).await
}

#[tauri::command]
pub async fn toggle_record(state: State<'_, AppState>) -> Result<(), String> {
    state::toggle_record(&state).await
}

/// The latest JPEG as a raw body (an ArrayBuffer in the webview).
#[tauri::command]
pub async fn live_view_frame(state: State<'_, AppState>) -> Result<Response, String> {
    Ok(Response::new(state::live_view_frame(&state).await?))
}

#[tauri::command]
pub async fn live_view_meta(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    state::live_view_meta(&state).await
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

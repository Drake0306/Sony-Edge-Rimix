//! Tauri commands: thin wrappers over the functions in `state.rs`.

use sony_ptpip::{CameraState, DiscoveredCamera};
use tauri::ipc::Response;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_dialog::DialogExt;

use crate::settings::AppSettings;
use crate::state::{self, AppState};

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
) -> Result<CameraState, String> {
    let (camera_state, events) = state::connect(&state, &ip).await?;
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

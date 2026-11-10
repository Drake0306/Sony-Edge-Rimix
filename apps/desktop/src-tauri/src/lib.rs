//! The Rimix desktop app: Tauri commands over the `sony-ptpip` camera facade.

pub mod commands;
pub mod logging;
pub mod settings;
pub mod sftp;
pub mod state;

use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let dir = settings::config_dir().ok_or("cannot find a config folder")?;
            // Never fail startup over logging: fall back to stderr.
            let guard = match logging::init(&logging::log_dir(&dir)) {
                Ok(guard) => Some(guard),
                Err(e) => {
                    logging::init_stderr();
                    tracing::warn!(error = %e, "cannot open the log folder; logging to stderr");
                    None
                }
            };
            app.manage(logging::LogGuard(guard));
            tracing::info!(version = env!("CARGO_PKG_VERSION"), "Rimix started");
            let guid = sony_ptpip::load_or_create_guid(&dir.join("guid"))?;
            app.manage(state::AppState::new(guid, dir.join("settings.json")));
            commands::start_sftp(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::discover,
            commands::connect,
            commands::disconnect,
            commands::disconnect_all,
            commands::list_cameras,
            commands::set_active_camera,
            commands::shoot_all,
            commands::get_state,
            commands::set_property,
            commands::step_property,
            commands::apply_values,
            commands::shoot,
            commands::focus,
            commands::toggle_record,
            commands::card_list,
            commands::card_download,
            commands::card_mode,
            commands::live_view_frame,
            commands::live_view_meta,
            commands::get_settings,
            commands::save_settings,
            commands::pick_folder,
            commands::read_thumbnail,
            commands::join_network,
            commands::current_network,
            commands::wait_for_camera,
            commands::open_logs,
            commands::sftp_status,
            commands::sftp_set_enabled,
            commands::sftp_regenerate_password,
        ])
        .build(tauri::generate_context!())
        .expect("error while running Rimix")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                if let Some(state) = app.try_state::<state::AppState>() {
                    tauri::async_runtime::block_on(sftp::stop(&state));
                }
            }
        });
}

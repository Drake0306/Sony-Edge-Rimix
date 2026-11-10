//! The Rimix desktop app: Tauri commands over the `sony-ptpip` camera facade.

pub mod commands;
pub mod settings;
pub mod state;

use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let dir = settings::config_dir().ok_or("cannot find a config folder")?;
            let guid = sony_ptpip::load_or_create_guid(&dir.join("guid"))?;
            app.manage(state::AppState::new(guid, dir.join("settings.json")));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::discover,
            commands::connect,
            commands::disconnect,
            commands::get_state,
            commands::set_property,
            commands::step_property,
            commands::shoot,
            commands::focus,
            commands::toggle_record,
            commands::live_view_frame,
            commands::live_view_meta,
            commands::get_settings,
            commands::save_settings,
            commands::pick_folder,
            commands::read_thumbnail,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Rimix");
}

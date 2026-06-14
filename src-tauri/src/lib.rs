mod commands;
mod config;
mod session;
mod transport;

use session::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_drag::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::list_adb_devices,
            commands::connect_adb,
            commands::connect_ssh,
            commands::connect_local,
            commands::list_ssh_history,
            commands::delete_ssh_history,
            commands::disconnect_session,
            commands::list_dir,
            commands::read_chunk,
            commands::stage_for_drag,
            commands::drag_icon,
            commands::upload,
            commands::elevate,
            commands::unelevate,
            commands::remove_path,
            commands::rename_path,
            commands::copy_path,
            commands::exec_command,
            commands::shell_open,
            commands::shell_write,
            commands::shell_resize,
            commands::shell_close,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

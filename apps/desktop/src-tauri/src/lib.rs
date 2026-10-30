pub mod commands;
pub mod runtime;
pub mod settings;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            runtime::start(app.handle())?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_agents,
            commands::get_transcript,
            commands::get_settings,
            commands::set_settings,
            commands::set_selected_agent,
            commands::watcher_mode,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

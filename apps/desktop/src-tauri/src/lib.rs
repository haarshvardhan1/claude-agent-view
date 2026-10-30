pub mod commands;
pub mod lifecycle;
pub mod runtime;
pub mod settings;
pub mod tray;

use tauri::plugin::PermissionState;
use tauri::{Manager, RunEvent, WindowEvent};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_window_state::StateFlags;

/// Asks for notification permission if it hasn't been decided yet.
fn request_notification_permission(app: &tauri::AppHandle) {
    let n = app.notification();
    if matches!(
        n.permission_state(),
        Ok(PermissionState::Prompt | PermissionState::PromptWithRationale)
    ) {
        let _ = n.request_permission();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(
            tauri_plugin_window_state::Builder::new()
                .with_denylist(&[lifecycle::SETTINGS_LABEL])
                // Restoring "hidden" would start the app with no window.
                .with_state_flags(StateFlags::all() - StateFlags::VISIBLE)
                .build(),
        )
        .menu(tray::app_menu)
        .on_menu_event(|app, event| tray::on_menu_event(app, event.id().as_ref()))
        .on_window_event(|window, event| {
            if window.label() != lifecycle::MAIN_LABEL {
                return;
            }
            let app = window.app_handle();
            match event {
                WindowEvent::CloseRequested { api, .. } => {
                    api.prevent_close();
                    lifecycle::on_main_close_requested(app);
                }
                WindowEvent::Focused(focused) => lifecycle::on_main_focus(app, *focused),
                _ => {}
            }
        })
        .setup(|app| {
            tray::build_tray(app.handle())?;
            request_notification_permission(app.handle());
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
            commands::notification_permission,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app, event| {
        if let RunEvent::Reopen { .. } = event {
            // Dock click: bring the main window back.
            lifecycle::show_main(app);
            lifecycle::on_activated(app);
        }
    });
}

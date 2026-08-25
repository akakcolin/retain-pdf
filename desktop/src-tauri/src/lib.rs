mod backend_env;
mod backend_http;
mod backend_startup;
mod commands;
mod constants;
mod desktop_config;
mod events;
mod logging;
mod python_runtime;

use tauri::Manager;

use backend_startup::BackendState;

pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            } else if let Some(splash) = app.get_webview_window("splash") {
                let _ = splash.show();
                let _ = splash.set_focus();
            }
        }))
        .manage(BackendState::default())
        .invoke_handler(tauri::generate_handler![
            commands::load_desktop_config,
            commands::save_desktop_config,
            commands::open_output_directory,
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                backend_startup::run_startup(handle);
            });
            Ok(())
        });

    builder
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| events::on_event(app_handle, event));
}

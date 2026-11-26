#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod orcad;
mod trash;

use tauri::{Manager, RunEvent};

fn main() {
    tauri::Builder::default()
        // Must be first: a second launch would otherwise start a second orcad on the same profile.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(tauri::generate_handler![trash::move_to_trash])
        .setup(|app| {
            let window = app
                .get_webview_window("main")
                .ok_or("main window missing")?;
            // Agent and test launches stay hidden so they never steal the user's focus.
            if std::env::var("ORCA_BACKGROUND_LAUNCH").as_deref() != Ok("1") {
                window.show()?;
            }
            let sidecar = orcad::spawn(app.handle(), window)?;
            app.manage(sidecar);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build Orca Tauri shell")
        .run(|app, event| {
            if let RunEvent::Exit = event {
                app.state::<orcad::Sidecar>().stop();
            }
        });
}

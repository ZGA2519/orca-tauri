#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod orcad;

use tauri::{Manager, RunEvent};

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
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

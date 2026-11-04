#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::Manager;

fn main() {
    let smoke_test = std::env::args().any(|arg| arg == "--smoke-test");

    tauri::Builder::default()
        .on_page_load(move |webview, payload| {
            // CI runs the packaged application and waits for its webview to load.
            if smoke_test && matches!(payload.event(), tauri::webview::PageLoadEvent::Finished) {
                webview.app_handle().exit(0);
            }
        })
        .run(tauri::generate_context!())
        .expect("failed to start SoDeck");
}

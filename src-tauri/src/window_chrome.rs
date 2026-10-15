//! Keep dynamically created windows consistent with the platform main-window config.
use tauri::{Manager, Runtime, WebviewWindowBuilder};

pub fn configure<R: Runtime, M: Manager<R>>(
    builder: WebviewWindowBuilder<'_, R, M>,
) -> WebviewWindowBuilder<'_, R, M> {
    #[cfg(target_os = "windows")]
    let builder = builder.decorations(false).shadow(true);
    #[cfg(target_os = "macos")]
    let builder = builder
        .decorations(true)
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true)
        .traffic_light_position(tauri::LogicalPosition::new(14.0, 24.0));
    builder
}

#[tauri::command]
pub fn window_chrome() -> &'static str {
    if cfg!(target_os = "windows") {
        "custom"
    } else if cfg!(target_os = "macos") {
        "overlay"
    } else {
        "native"
    }
}

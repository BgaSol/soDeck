/// Use the bundle's product name for every secondary native window.
pub(crate) fn window_title(app: &tauri::AppHandle, context: &str) -> String {
    let name = app
        .config()
        .product_name
        .as_deref()
        .unwrap_or(&app.package_info().name);
    format!("{context} · {name}")
}

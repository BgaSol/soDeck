// Export the actual plugin scripts for the Chromium/WebKit interaction suite.
#[path = "../src/browser_behavior.rs"]
mod browser_behavior;

use tauri::plugin::Plugin;

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("provide the fixture output path");
    let scripts = serde_json::json!({
        "disabled": scripts(false),
        "enabled": scripts(true),
    });
    let path = std::path::Path::new(&path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, serde_json::to_vec(&scripts).unwrap()).unwrap();
}

fn scripts(enabled: bool) -> String {
    [
        browser_behavior::plugin::<tauri::Wry>(enabled),
        browser_behavior::shortcut_guard(enabled),
    ]
    .into_iter()
    .map(|plugin| {
        format!(
            ";(function() {{ {} }})();",
            plugin.initialization_script().unwrap()
        )
    })
    .collect::<String>()
}

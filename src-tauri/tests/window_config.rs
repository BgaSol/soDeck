use std::path::Path;
use tauri::utils::{
    config::{parse::read_from, Config},
    platform::Target,
};

#[test]
fn macos_main_window_starts_with_an_overlay_title_bar() {
    let (value, _) = read_from(Target::MacOS, Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
    let config: Config = serde_json::from_value(value).unwrap();
    let window = &config.app.windows[0];
    assert_eq!(window.label, "main");
    assert!(window.decorations);
    assert!(window.hidden_title);
    assert_eq!(window.title_bar_style, tauri::TitleBarStyle::Overlay);
}

#[test]
fn platform_title_bars_preserve_shared_window_settings_and_app_version() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let (base, _) = read_from(Target::Windows, root).unwrap();
    for target in [Target::Windows, Target::Linux, Target::MacOS] {
        let (mut value, _) = read_from(target, root).unwrap();
        let _: Config = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(value["version"], sodeck::app_version());
        assert_eq!(value["app"]["windows"].as_array().unwrap().len(), 1);
        let window = &mut value["app"]["windows"][0];
        if target == Target::MacOS {
            // JSON Merge Patch replaces the window array, so its shared settings must be retained.
            window["decorations"] = false.into();
            window.as_object_mut().unwrap().remove("titleBarStyle");
        } else {
            assert_eq!(window["decorations"], false);
        }
        assert_eq!(value, base);
    }
}

//! One browser-default policy for every native window and every plugin frame.
use tauri::{plugin::Plugin, Runtime};
use tauri_plugin_prevent_default::{Flags, KeyboardShortcut, ModifierKey};

pub fn plugin<R: Runtime>() -> tauri::plugin::TauriPlugin<R> {
    // Shift+Tab is ordinary focus navigation, including inside application forms.
    let mut builder = tauri_plugin_prevent_default::Builder::new()
        .with_flags(Flags::all().difference(Flags::FOCUS_MOVE))
        .shortcut(KeyboardShortcut::new("F12"))
        .shortcut(KeyboardShortcut::with_modifiers(
            "F3",
            &[ModifierKey::ShiftKey],
        ))
        .shortcut(KeyboardShortcut::with_modifiers(
            "c",
            &[ModifierKey::CtrlKey, ModifierKey::ShiftKey],
        ));
    for key in ["i", "j"] {
        builder = builder.shortcut(KeyboardShortcut::with_modifiers(
            key,
            &[ModifierKey::CtrlKey, ModifierKey::ShiftKey],
        ));
    }
    for modifier in [ModifierKey::CtrlKey, ModifierKey::MetaKey] {
        builder = builder.shortcut(KeyboardShortcut::with_modifiers("s", &[modifier]));
        builder = builder.shortcut(KeyboardShortcut::with_modifiers(
            "s",
            &[modifier, ModifierKey::ShiftKey],
        ));
    }
    for modifier in [ModifierKey::CtrlKey, ModifierKey::MetaKey] {
        for key in ["+", "=", "-", "0"] {
            builder = builder.shortcut(KeyboardShortcut::with_modifiers(key, &[modifier]));
            builder = builder.shortcut(KeyboardShortcut::with_modifiers(
                key,
                &[modifier, ModifierKey::ShiftKey],
            ));
        }
    }
    // The upstream flags use Control; Command shortcuts need their own entries.
    for key in ["f", "g", "j", "r", "u", "o", "p"] {
        builder = builder.shortcut(KeyboardShortcut::with_modifiers(
            key,
            &[ModifierKey::MetaKey],
        ));
    }
    for key in ["r", "g", "p", "i", "c", "j"] {
        builder = builder.shortcut(KeyboardShortcut::with_modifiers(
            key,
            &[ModifierKey::MetaKey, ModifierKey::ShiftKey],
        ));
    }
    for key in ["i", "c", "j"] {
        builder = builder.shortcut(KeyboardShortcut::with_modifiers(
            key,
            &[ModifierKey::MetaKey, ModifierKey::AltKey],
        ));
    }
    for key in ["ArrowLeft", "ArrowRight"] {
        builder = builder.shortcut(KeyboardShortcut::with_modifiers(
            key,
            &[ModifierKey::AltKey],
        ));
    }
    for key in ["[", "]"] {
        builder = builder.shortcut(KeyboardShortcut::with_modifiers(
            key,
            &[ModifierKey::MetaKey],
        ));
    }
    #[cfg(target_os = "windows")]
    let builder = builder.platform(
        tauri_plugin_prevent_default::PlatformOptions::new()
            .browser_accelerator_keys(false)
            .default_context_menus(false)
            .dev_tools(false)
            .zoom_control(false)
            .pinch_zoom(false)
            .swipe_navigation(false),
    );

    let mut upstream: tauri::plugin::TauriPlugin<R> = builder.build();
    let script = upstream
        .initialization_script()
        .expect("prevent-default script");
    // Upstream's default injection is main-frame-only on WebKit. Reuse its
    // generated script and native callback through Tauri's public plugin API.
    tauri::plugin::Builder::new("desktop-behavior")
        .js_init_script_on_all_frames(format!("{script}\nwindow.addEventListener('auxclick', e => {{ if (e.button === 3 || e.button === 4) e.preventDefault(); }});"))
        .on_webview_ready(move |webview| upstream.webview_created(webview))
        .build()
}

/// Called from the isolated package smoke worker, never the UI event loop.
#[cfg(target_os = "windows")]
pub fn verify_native_settings(window: &tauri::WebviewWindow) -> Result<(), String> {
    let (send, receive) = std::sync::mpsc::channel();
    window
        .with_webview(move |webview| {
            let result = (|| unsafe {
                let settings = webview
                    .controller()
                    .CoreWebView2()
                    .and_then(|view| view.Settings())
                    .map_err(|e| e.to_string())?;
                let mut enabled = Default::default();
                settings
                    .AreDefaultContextMenusEnabled(&mut enabled)
                    .map_err(|e| e.to_string())?;
                if enabled.as_bool() {
                    return Err("浏览器默认右键菜单仍然开启".into());
                }
                settings
                    .AreDevToolsEnabled(&mut enabled)
                    .map_err(|e| e.to_string())?;
                if enabled.as_bool() {
                    return Err("浏览器开发者工具仍然开启".into());
                }
                settings
                    .IsZoomControlEnabled(&mut enabled)
                    .map_err(|e| e.to_string())?;
                if enabled.as_bool() {
                    return Err("浏览器默认缩放仍然开启".into());
                }
                Ok(())
            })();
            let _ = send.send(result);
        })
        .map_err(|e| e.to_string())?;
    receive
        .recv_timeout(std::time::Duration::from_secs(5))
        .map_err(|e| format!("无法验证 WebView 默认行为：{e}"))?
}

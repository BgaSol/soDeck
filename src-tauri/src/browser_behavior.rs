use tauri_plugin_prevent_default::{Builder, Flags, KeyboardShortcut, ModifierKey};

pub fn plugin<R: tauri::Runtime>(developer_mode: bool) -> tauri::plugin::TauriPlugin<R> {
    let mut builder = Builder::new().with_flags(if developer_mode {
        Flags::empty()
    } else {
        Flags::CONTEXT_MENU | Flags::RELOAD | Flags::DEV_TOOLS
    });
    if !developer_mode {
        // The plugin prevents the browser default without stopping propagation.
        // Terminal/editor handlers keep receiving Ctrl+R and function keys.
        builder = builder
            .shortcut(KeyboardShortcut::new("F12"))
            .shortcut(KeyboardShortcut::with_modifiers(
                "r",
                &[ModifierKey::MetaKey],
            ))
            .shortcut(KeyboardShortcut::with_modifiers(
                "r",
                &[ModifierKey::MetaKey, ModifierKey::ShiftKey],
            ))
            .shortcut(KeyboardShortcut::with_modifiers(
                "i",
                &[ModifierKey::MetaKey, ModifierKey::AltKey],
            ));
    }
    #[cfg(target_os = "windows")]
    let builder = builder.platform(
        tauri_plugin_prevent_default::PlatformOptions::new()
            .default_context_menus(developer_mode)
            .dev_tools(developer_mode),
    );
    builder.build()
}

pub fn shortcut_guard<R: tauri::Runtime>(developer_mode: bool) -> tauri::plugin::TauriPlugin<R> {
    tauri::plugin::Builder::new("developer-shortcuts")
        .js_init_script(include_str!("developer_shortcuts.js").replace(
            "__DEVELOPER_MODE__",
            if developer_mode { "true" } else { "false" },
        ))
        .build()
}

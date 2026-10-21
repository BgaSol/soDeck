use crate::model::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tauri::{Emitter, State, WebviewWindow};

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ShortcutMode {
    #[default]
    Platform,
    Convenient,
    Traditional,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TerminalClipboardPreferences {
    shortcut_mode: ShortcutMode,
    copy_on_select: bool,
    right_click_paste: bool,
}

const SETTINGS_CHANGED: &str = "system-settings-changed";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SettingsFile {
    version: u32,
    #[serde(default)]
    developer_mode: bool,
    #[serde(default)]
    terminal_clipboard: TerminalClipboardPreferences,
}

impl Default for SettingsFile {
    fn default() -> Self {
        Self {
            version: 1,
            developer_mode: false,
            terminal_clipboard: TerminalClipboardPreferences::default(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsSnapshot {
    developer_mode: bool,
    active_developer_mode: bool,
    terminal_clipboard: TerminalClipboardPreferences,
    platform: &'static str,
    revision: u64,
    load_error: Option<String>,
}

struct SavedSettings {
    value: SettingsFile,
    revision: u64,
    load_error: Option<String>,
}

pub struct SystemSettings {
    path: PathBuf,
    // Immutable for the whole process, including windows opened after a save.
    active_developer_mode: bool,
    saved: Mutex<SavedSettings>,
}

impl SystemSettings {
    pub fn load(path: PathBuf) -> Self {
        let result = (|| -> Result<SettingsFile, String> {
            let bytes = match std::fs::read(&path) {
                Ok(bytes) => bytes,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(SettingsFile::default())
                }
                Err(e) => return Err(e.to_string()),
            };
            let value: SettingsFile = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            if value.version != 1 {
                return Err(format!("不支持的设置版本 {}", value.version));
            }
            Ok(value)
        })();
        let (value, load_error) = match result {
            Ok(value) => (value, None),
            Err(error) => (
                SettingsFile::default(),
                Some(format!(
                    "无法读取系统设置，本次使用默认设置且开发者模式已关闭。请检查 {} 后重新启动：{}",
                    path.display(),
                    error
                )),
            ),
        };
        Self {
            path,
            active_developer_mode: value.developer_mode,
            saved: Mutex::new(SavedSettings {
                value,
                revision: 0,
                load_error,
            }),
        }
    }

    pub fn developer_mode(&self) -> bool {
        self.active_developer_mode
    }

    fn snapshot_of(&self, saved: &SavedSettings) -> SettingsSnapshot {
        SettingsSnapshot {
            developer_mode: saved.value.developer_mode,
            active_developer_mode: self.active_developer_mode,
            terminal_clipboard: saved.value.terminal_clipboard.clone(),
            platform: if cfg!(target_os = "windows") {
                "windows"
            } else if cfg!(target_os = "macos") {
                "macos"
            } else {
                "linux"
            },
            revision: saved.revision,
            load_error: saved.load_error.clone(),
        }
    }

    pub fn snapshot(&self) -> SettingsSnapshot {
        self.snapshot_of(&self.saved.lock().unwrap())
    }

    fn save(&self, developer_mode: bool, expected_revision: u64) -> AppResult<SettingsSnapshot> {
        self.update(expected_revision, |next| {
            next.developer_mode = developer_mode
        })
    }

    fn save_clipboard(
        &self,
        preferences: TerminalClipboardPreferences,
        expected_revision: u64,
    ) -> AppResult<SettingsSnapshot> {
        self.update(expected_revision, |next| {
            next.terminal_clipboard = preferences
        })
    }

    fn update(
        &self,
        expected_revision: u64,
        update: impl FnOnce(&mut SettingsFile),
    ) -> AppResult<SettingsSnapshot> {
        let mut saved = self.saved.lock().unwrap();
        if let Some(error) = &saved.load_error {
            // Do not overwrite unreadable or newer settings with defaults.
            return Err(AppError::new("settings", error));
        }
        if saved.revision != expected_revision {
            return Err(AppError::new(
                "settingsConflict",
                "设置已在其他窗口更新，请核对后重试。",
            ));
        }
        let mut next = saved.value.clone();
        update(&mut next);
        if saved.value.developer_mode == next.developer_mode
            && saved.value.terminal_clipboard == next.terminal_clipboard
        {
            return Ok(self.snapshot_of(&saved));
        }
        let persist = || -> Result<(), Box<dyn std::error::Error>> {
            let parent = self.path.parent().ok_or("无法确定设置目录")?;
            std::fs::create_dir_all(parent)?;
            let mut temp = tempfile::NamedTempFile::new_in(parent)?;
            temp.write_all(&serde_json::to_vec_pretty(&next)?)?;
            temp.as_file().sync_all()?;
            temp.persist(&self.path)?;
            Ok(())
        };
        persist().map_err(|e| AppError::new("settings", format!("保存系统设置失败：{e}")))?;
        crate::diagnostics::event::Context::new().event(
            "INFO",
            "settings",
            "settings.saved",
            serde_json::json!({"developerMode":next.developer_mode}),
        );
        saved.value = next;
        saved.revision += 1;
        Ok(self.snapshot_of(&saved))
    }

    fn require_developer_mode(&self) -> AppResult<()> {
        if self.developer_mode() {
            Ok(())
        } else {
            Err(AppError::new(
                "permission",
                "请先开启开发者模式，并重新启动应用。",
            ))
        }
    }
}

#[tauri::command]
pub fn get_system_settings(settings: State<'_, Arc<SystemSettings>>) -> SettingsSnapshot {
    settings.snapshot()
}

#[tauri::command]
pub async fn set_developer_mode(
    app: tauri::AppHandle,
    settings: State<'_, Arc<SystemSettings>>,
    developer_mode: bool,
    expected_revision: u64,
) -> AppResult<SettingsSnapshot> {
    let settings = settings.inner().clone();
    let snapshot =
        crate::blocking(move || settings.save(developer_mode, expected_revision)).await?;
    // A delivery failure must not report a failed save after the file was committed.
    if let Err(error) = app.emit(SETTINGS_CHANGED, &snapshot) {
        eprintln!("无法广播系统设置变更：{error}");
    }
    Ok(snapshot)
}

#[tauri::command]
pub async fn set_terminal_clipboard_settings(
    app: tauri::AppHandle,
    settings: State<'_, Arc<SystemSettings>>,
    preferences: TerminalClipboardPreferences,
    expected_revision: u64,
) -> AppResult<SettingsSnapshot> {
    let settings = settings.inner().clone();
    let snapshot =
        crate::blocking(move || settings.save_clipboard(preferences, expected_revision)).await?;
    if let Err(error) = app.emit(SETTINGS_CHANGED, &snapshot) {
        eprintln!("无法广播系统设置变更：{error}");
    }
    Ok(snapshot)
}

#[tauri::command]
pub fn open_window_devtools(
    window: WebviewWindow,
    settings: State<'_, Arc<SystemSettings>>,
) -> AppResult<()> {
    settings.require_developer_mode()?;
    window.open_devtools();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipboard_preferences_migrate_and_preserve_developer_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"version":1,"developerMode":true}"#).unwrap();
        let settings = SystemSettings::load(path.clone());
        assert_eq!(
            settings.snapshot().terminal_clipboard,
            TerminalClipboardPreferences::default()
        );
        let preferences = TerminalClipboardPreferences {
            shortcut_mode: ShortcutMode::Traditional,
            copy_on_select: true,
            right_click_paste: true,
        };
        let saved = settings.save_clipboard(preferences.clone(), 0).unwrap();
        assert!(saved.developer_mode);
        assert!(saved.active_developer_mode);
        assert_eq!(saved.revision, 1);
        assert_eq!(
            settings
                .save_clipboard(preferences.clone(), 1)
                .unwrap()
                .revision,
            1
        );
        assert_eq!(
            settings.save(false, 0).unwrap_err().kind,
            "settingsConflict"
        );
        settings.save(false, 1).unwrap();
        let reloaded = SystemSettings::load(path);
        assert!(!reloaded.developer_mode());
        assert_eq!(reloaded.snapshot().terminal_clipboard, preferences);
    }

    #[test]
    fn failed_clipboard_write_does_not_change_memory_or_revision() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let settings = SystemSettings::load(path.clone());
        std::fs::create_dir(&path).unwrap();
        let preferences = TerminalClipboardPreferences {
            copy_on_select: true,
            ..Default::default()
        };
        assert!(settings.save_clipboard(preferences, 0).is_err());
        assert_eq!(
            settings.snapshot().terminal_clipboard,
            TerminalClipboardPreferences::default()
        );
        assert_eq!(settings.snapshot().revision, 0);
    }

    #[test]
    fn preference_only_becomes_active_after_a_new_start() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let first = SystemSettings::load(path.clone());
        assert!(!first.developer_mode());
        let pending = first.save(true, 0).unwrap();
        assert!(pending.developer_mode);
        assert!(!pending.active_developer_mode);
        assert!(first.require_developer_mode().is_err());
        let second = SystemSettings::load(path.clone());
        assert!(second.developer_mode());
        assert!(second.require_developer_mode().is_ok());
        let pending = second.save(false, 0).unwrap();
        assert!(!pending.developer_mode);
        assert!(pending.active_developer_mode);
        assert!(second.require_developer_mode().is_ok());
        assert!(!SystemSettings::load(path).developer_mode());
    }

    #[test]
    fn stale_window_cannot_overwrite_newer_settings_and_pending_can_be_cancelled() {
        let dir = tempfile::tempdir().unwrap();
        let settings = SystemSettings::load(dir.path().join("settings.json"));
        settings.save(true, 0).unwrap();
        assert_eq!(
            settings.save(false, 0).unwrap_err().kind,
            "settingsConflict"
        );
        assert!(settings.snapshot().developer_mode);
        let cancelled = settings.save(false, 1).unwrap();
        assert_eq!(cancelled.developer_mode, cancelled.active_developer_mode);
        assert_eq!(cancelled.revision, 2);
    }

    #[test]
    fn failed_write_keeps_the_saved_and_active_state() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let settings = SystemSettings::load(path.clone());
        // Deterministic failure even when tests run as an administrator.
        std::fs::create_dir(&path).unwrap();
        assert!(settings.save(true, 0).is_err());
        assert!(!settings.snapshot().developer_mode);
        assert!(!settings.developer_mode());
        assert_eq!(settings.snapshot().revision, 0);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn unreadable_and_future_files_are_preserved_and_start_with_devtools_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        for bytes in ["{broken", r#"{"version":2,"developerMode":true}"#] {
            std::fs::write(&path, bytes).unwrap();
            let settings = SystemSettings::load(path.clone());
            assert!(!settings.developer_mode());
            assert!(settings.snapshot().load_error.is_some());
            assert!(settings.save(true, 0).is_err());
            assert_eq!(std::fs::read_to_string(&path).unwrap(), bytes);
        }
    }
}

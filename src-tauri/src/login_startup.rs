//! Current-user login startup registration. The operating system is the source of truth.

use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub enabled: bool,
    pub state: String,
    pub reason: Option<String>,
}

impl Status {
    fn disabled() -> Self {
        Self {
            enabled: false,
            state: "disabled".into(),
            reason: None,
        }
    }
}

pub struct Service {
    mutation: Mutex<()>,
    available: bool,
    stopped: AtomicBool,
}

impl crate::AppCore {
    pub fn record_login_startup(&self, enabled: bool, result: &Result<Status, String>) {
        self.log(None,if result.is_ok(){"info"}else{"error"},"修改登录启动设置",Some(serde_json::json!({"enabled":enabled,"result":result.as_ref().map(|s|s.state.clone()).map_err(|e|e.clone())})));
    }
}

impl Service {
    pub fn new(isolated: bool) -> Self {
        Self {
            mutation: Mutex::new(()),
            available: !isolated && !cfg!(debug_assertions),
            stopped: AtomicBool::new(false),
        }
    }

    pub fn refresh(&self) -> Status {
        let _mutation = self.mutation.lock().unwrap();
        self.read()
    }

    fn read(&self) -> Status {
        if !self.available {
            return Status {
                enabled: false,
                state: "unavailable".into(),
                reason: Some(
                    "开发和隔离验收模式不修改系统登录项；请在正式安装的应用中设置。".into(),
                ),
            };
        }
        platform::read().unwrap_or_else(|error| Status {
            enabled: false,
            state: "unknown".into(),
            reason: Some(error),
        })
    }

    pub fn set(&self, enabled: bool) -> Result<Status, String> {
        if !self.available {
            return Err("开发和隔离验收模式不能修改系统登录项".into());
        }
        let _mutation = self.mutation.lock().unwrap();
        if self.stopped.load(Ordering::Acquire) {
            return Err("应用正在退出或重启".into());
        }
        let result = if enabled {
            platform::installed(&command_path()?).and_then(|_| platform::set(true))
        } else {
            platform::set(false)
        };
        let state = self.read();
        result.map(|_| state)
    }

    pub fn settle(&self) {
        let _mutation = self.mutation.lock().unwrap();
        self.stopped.store(true, Ordering::Release);
    }
}

fn command_path() -> Result<std::path::PathBuf, String> {
    #[cfg(target_os = "linux")]
    if let Some(path) = std::env::var_os("APPIMAGE") {
        return Ok(path.into());
    }
    std::env::current_exe().map_err(|error| format!("无法确定 SoDeck 启动位置：{error}"))
}

#[cfg(target_os = "linux")]
fn escaped_argument(path: &std::path::Path) -> String {
    let value = path.to_string_lossy();
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\\\\\")
            .replace('"', "\\\\\"")
            .replace('$', "\\\\$")
            .replace('`', "\\\\`")
            .replace('%', "%%")
    )
}

#[cfg(target_os = "windows")]
#[path = "login_startup/windows.rs"]
mod platform;

#[cfg(target_os = "linux")]
mod platform {
    use super::{command_path, escaped_argument, Status};
    use std::{fs, path::PathBuf};

    fn path() -> Result<PathBuf, String> {
        let root = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
            .ok_or("无法确定当前用户的配置目录".to_owned())?;
        Ok(root.join("autostart/com.bgasol.sodeck.desktop"))
    }

    pub fn read() -> Result<Status, String> {
        let path = path()?;
        if !path.exists() {
            return Ok(Status::disabled());
        }
        let text = fs::read_to_string(&path)
            .map_err(|error| format!("无法读取 Linux 登录启动项：{error}"))?;
        let enabled = !text.lines().any(|line| {
            matches!(
                line.trim(),
                "Hidden=true" | "X-GNOME-Autostart-enabled=false"
            )
        });
        if !enabled {
            return Ok(Status {
                enabled: false,
                state: "disabled".into(),
                reason: Some("已被系统启动项标记为禁用".into()),
            });
        }
        if !text.lines().any(|line| {
            line == format!(
                "Exec={} --login-start",
                escaped_argument(&command_path().unwrap_or_default())
            )
        }) {
            return Ok(Status {
                enabled: true,
                state: "stale".into(),
                reason: Some("启动项指向旧程序位置。关闭后重新开启可登记当前位置。".into()),
            });
        }
        Ok(Status {
            enabled: true,
            state: "enabled".into(),
            reason: None,
        })
    }

    pub fn set(enabled: bool) -> Result<(), String> {
        let path = path()?;
        if !enabled {
            return fs::remove_file(&path)
                .or_else(|error| {
                    if error.kind() == std::io::ErrorKind::NotFound {
                        Ok(())
                    } else {
                        Err(error)
                    }
                })
                .map_err(|error| format!("无法关闭 Linux 登录启动项：{error}"));
        }
        let executable = command_path()?;
        let parent = path.parent().ok_or("Linux 登录启动项路径无效")?;
        fs::create_dir_all(parent)
            .map_err(|error| format!("无法创建 Linux 登录启动项目录：{error}"))?;
        let content = content(&executable)?;
        crate::storage::atomic_write(&path, |file| {
            use std::io::Write;
            file.write_all(content.as_bytes())
        })
        .map_err(|error| format!("无法提交 Linux 登录启动项：{error}"))
    }

    fn content(executable: &std::path::Path) -> Result<String, String> {
        if executable
            .to_str()
            .is_none_or(|s| s.contains(['\n', '\r', '\0']))
        {
            return Err("启动路径包含无法登记的字符".into());
        }
        Ok(format!("[Desktop Entry]\nType=Application\nName=SoDeck\nExec={} --login-start\nX-GNOME-Autostart-enabled=true\n",escaped_argument(executable)))
    }

    pub(super) fn verify(directory: &std::path::Path) -> Result<(), String> {
        let text = content(&directory.join("中文 space $ `%/SoDeck.AppImage"))?;
        let path = directory.join("login-test.desktop");
        fs::write(&path, &text).map_err(|e| e.to_string())?;
        if fs::read_to_string(&path).map_err(|e| e.to_string())? != text {
            return Err("登录登记文件读回不一致".into());
        }
        fs::remove_file(path).map_err(|e| e.to_string())
    }

    pub fn installed(executable: &std::path::Path) -> Result<(), String> {
        if std::env::var_os("APPIMAGE").is_some()
            && executable.is_file()
            && !executable.starts_with(std::env::temp_dir())
        {
            return Ok(());
        }
        if executable == std::path::Path::new("/usr/bin/sodeck")
            || executable == std::path::Path::new("/usr/local/bin/sodeck")
        {
            return Ok(());
        }
        Err("请先安装 DEB，或把 AppImage 放在固定目录，再开启登录启动。".into())
    }
}

#[cfg(target_os = "macos")]
#[path = "login_startup/macos.rs"]
mod platform;

/// Exercises isolated platform IO; macOS registration additionally requires an explicit clean-CI probe.
pub fn verify_native(directory: &std::path::Path) -> Result<(), String> {
    platform::verify(directory)
}

/// One-shot native event, read on the UI thread during application startup.
pub fn native_login_launch() -> bool {
    #[cfg(target_os = "macos")]
    {
        platform::launched_at_login()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
mod platform {
    use super::Status;
    pub fn read() -> Result<Status, String> {
        Ok(Status {
            enabled: false,
            state: "unsupported".into(),
            reason: Some("当前平台不支持登录后自动启动".into()),
        })
    }
    pub fn set(_: bool) -> Result<(), String> {
        Err("当前平台不支持登录后自动启动".into())
    }
}

#[cfg(test)]
mod tests {
    use super::Service;
    #[test]
    fn isolated_app_never_registers_the_real_user_login_item() {
        let service = Service::new(true);
        assert_eq!(service.refresh().state, "unavailable");
        assert!(service.set(true).is_err());
        assert!(service.set(false).is_err());
    }
}

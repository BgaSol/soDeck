use super::{command_path, Status};
use std::path::Path;
#[path = "macos/legacy.rs"]
mod legacy;
#[path = "macos/native.rs"]
mod native;
use native::State;

fn status(enabled: bool, state: &str, reason: &str) -> Status {
    Status {
        enabled,
        state: state.into(),
        reason: (!reason.is_empty()).then(|| reason.into()),
    }
}

fn combined(current: State, old_enabled: Option<bool>) -> Status {
    match (current, old_enabled) {
        (State::Enabled, Some(_)) => status(true, "stale", "主应用登录项已开启，旧登记尚待清理，请更新登录项。"),
        (State::Enabled, None) => status(true, "enabled", ""),
        (_, Some(true)) => status(true, "stale", "旧版登录项仍开启。请更新登录项，使系统使用 SoDeck 应用名称和图标；如需系统允许，请在登录项中确认。"),
        (State::RequiresApproval, _) | (_, Some(false)) => status(false, "blocked", "登录项已被系统限制或旧项已关闭。请在系统设置 → 通用 → 登录项与扩展中允许 SoDeck；应用不会自动重新开启。"),
        (State::NotRegistered, None) => Status::disabled(),
        (State::NotFound, None) => status(false, "unknown", "系统未找到 SoDeck 主应用登录项，请确认应用安装位置并重新读取。"),
    }
}

fn old_enabled(entry: &legacy::Entry) -> Result<bool, String> {
    if !entry.enabled {
        return Ok(false);
    }
    if native::available() {
        match native::legacy_status(&entry.path)? {
            State::Enabled => Ok(true),
            State::RequiresApproval => Ok(false),
            _ => Err("系统无法确认旧登录项授权状态，原文件已保留；请检查系统登录项。".into()),
        }
    } else {
        Ok(true)
    }
}

pub fn read() -> Result<Status, String> {
    let bundle = native::bundle(&command_path()?, false)?;
    let old = legacy::inspect(&bundle.executable)?;
    let old = old.as_ref().map(old_enabled).transpose()?;
    if native::available() {
        Ok(combined(native::status()?, old))
    } else {
        Ok(match old {
            Some(true) => status(true, "enabled", ""),
            Some(false) => status(false, "blocked", "登录项已被关闭。"),
            None => Status::disabled(),
        })
    }
}

pub fn set(enabled: bool) -> Result<(), String> {
    let bundle = native::bundle(&command_path()?, enabled)?;
    // Validate ownership before any registration or removal, including disabling.
    let old = legacy::inspect(&bundle.executable)?;
    if !native::available() {
        return if enabled {
            native::register_bundle(&bundle)?;
            legacy::write(&bundle.executable)
        } else if let Some(entry) = old {
            legacy::remove(&entry)
        } else {
            Ok(())
        };
    }
    let before = native::status()?;
    if !enabled {
        if before != State::NotRegistered {
            native::set(false)?;
        }
        if let Some(entry) = old {
            legacy::remove(&entry)?;
        }
        return Ok(());
    }
    native::register_bundle(&bundle)?;
    if before != State::Enabled {
        native::set(true)?;
    }
    if native::status()? != State::Enabled {
        return Err(
            "主应用登录项尚未获得系统允许，旧项已保留；请在系统登录项中确认后重试。".into(),
        );
    }
    if let Some(entry) = old {
        if let Err(error) = legacy::remove(&entry) {
            // Never bootout: the legacy job may own this application's current process.
            let rollback = if before != State::Enabled {
                native::set(false)
            } else {
                Ok(())
            };
            return Err(match rollback { Ok(()) => error, Err(rollback) => format!("{error}；新登记回退也未完成：{rollback}。请检查系统登录项，当前可能存在两份登记。") });
        }
    }
    Ok(())
}

pub fn installed(executable: &Path) -> Result<(), String> {
    native::bundle(executable, true).map(|_| ())
}

pub(super) fn verify(directory: &Path) -> Result<(), String> {
    native::bundle(&command_path()?, false)?;
    if native::available() {
        let _ = native::status()?;
    }
    legacy::verify(directory)?;
    if std::env::var("SODECK_TEST_MACOS_LOGIN").as_deref() == Ok("1") {
        verify_registration(directory)?;
    }
    Ok(())
}

fn verify_registration(directory: &Path) -> Result<(), String> {
    let bundle = native::bundle(&command_path()?, true)?;
    let home = std::path::PathBuf::from(std::env::var_os("HOME").ok_or("无法确定测试用户")?);
    let old = legacy::path()?;
    if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
        || !bundle.path.starts_with(home.join("Applications"))
        || old.try_exists().map_err(|e| e.to_string())?
        || native::status()? != State::NotRegistered
    {
        return Err("真实登录项验收仅允许在干净 CI 用户的 Applications 安装中运行".into());
    }
    let result = (|| {
        // Reproduce the exact 2.5.0 disk registration, without bootstrapping the running app.
        legacy::write(&bundle.executable)?;
        set(true)?;
        if old.exists() || read()?.state != "enabled" {
            return Err("旧登录项未完成迁移".into());
        }
        set(true)?;
        std::fs::write(directory.join("macos-login-enabled.json"), serde_json::json!({"pid":std::process::id(),"bundle":bundle.path,"state":"enabled","migration":true}).to_string()).map_err(|e|e.to_string())?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while !directory.join("macos-login-inspected.json").exists() {
            if std::time::Instant::now() >= deadline {
                return Err("登录项系统证据采集未完成".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        set(false)?;
        set(false)?;
        if read()?.state != "disabled" {
            return Err("取消主应用登录项未生效".into());
        }
        std::fs::write(directory.join("macos-login-service.json"), serde_json::json!({"enabled":true,"migrated":true,"disabled":true,"processContinued":true,"bundle":bundle.path}).to_string()).map_err(|e|e.to_string())?;
        Ok(())
    })();
    let cleanup = if native::status()? != State::NotRegistered {
        native::set(false)
    } else {
        Ok(())
    };
    if let Some(entry) = legacy::inspect(&bundle.executable)? {
        legacy::remove(&entry)?;
    }
    result.and(cleanup)
}

pub fn launched_at_login() -> bool {
    native::launched_at_login()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_and_new_registration_facts_are_not_conflated() {
        for (current, old, enabled, state) in [
            (State::NotRegistered, None, false, "disabled"),
            (State::Enabled, None, true, "enabled"),
            (State::RequiresApproval, None, false, "blocked"),
            (State::NotFound, None, false, "unknown"),
            (State::RequiresApproval, Some(true), true, "stale"),
            (State::NotRegistered, Some(false), false, "blocked"),
            (State::Enabled, Some(false), true, "stale"),
            (State::Enabled, Some(true), true, "stale"),
        ] {
            let result = combined(current, old);
            assert_eq!((result.enabled, result.state.as_str()), (enabled, state));
        }
    }
}

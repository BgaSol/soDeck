use super::{command_path, Status};
use std::{path::Path, ptr};
use windows_sys::Win32::System::Registry::*;

const RUN: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const APPROVED: &str =
    "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run";
const VALUE: &str = "com.bgasol.sodeck";
struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            RegCloseKey(self.0);
        }
    }
}
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn query(hive: HKEY, path: &str, name: &str) -> Result<Option<(u32, Vec<u8>)>, String> {
    let mut key = ptr::null_mut();
    let result = unsafe { RegOpenKeyExW(hive, wide(path).as_ptr(), 0, KEY_READ, &mut key) };
    if result == 2 {
        return Ok(None);
    }
    if result != 0 {
        return Err(format!("无法读取 Windows 启动设置：错误码 {result}"));
    }
    let key = Key(key);
    let mut kind = 0;
    let mut bytes = 0;
    let result = unsafe {
        RegQueryValueExW(
            key.0,
            wide(name).as_ptr(),
            ptr::null_mut(),
            &mut kind,
            ptr::null_mut(),
            &mut bytes,
        )
    };
    if result == 2 {
        return Ok(None);
    }
    if result != 0 || bytes > 65536 {
        return Err("Windows 启动设置无法读取或超过长度上限".into());
    }
    let mut data = vec![0u8; bytes as usize];
    let result = unsafe {
        RegQueryValueExW(
            key.0,
            wide(name).as_ptr(),
            ptr::null_mut(),
            &mut kind,
            data.as_mut_ptr(),
            &mut bytes,
        )
    };
    if result != 0 {
        return Err(format!("无法确认 Windows 启动设置：错误码 {result}"));
    }
    data.truncate(bytes as usize);
    Ok(Some((kind, data)))
}

fn string(kind: u32, bytes: &[u8]) -> Result<String, String> {
    if kind != REG_SZ || !bytes.len().is_multiple_of(2) {
        return Err("Windows 启动项格式无法识别".into());
    }
    let value: Vec<_> = bytes
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .take_while(|c| *c != 0)
        .collect();
    String::from_utf16(&value).map_err(|_| "Windows 启动项文字无法解析".into())
}

fn command(path: &Path) -> Result<String, String> {
    let path = path.to_str().ok_or("启动路径不是有效 Unicode")?;
    if path.contains(['"', '\0', '\n', '\r']) {
        return Err("启动路径包含无法登记的字符".into());
    }
    Ok(format!("\"{path}\" --login-start"))
}

fn classify(value: &str, expected: &str, approval: Option<&[u8]>) -> Status {
    if approval.is_some_and(|bytes| matches!(bytes.first(), Some(3 | 7))) {
        return Status {
            enabled: false,
            state: "blocked".into(),
            reason: Some("已被 Windows 禁用。请在系统设置 → 应用 → 启动中允许 SoDeck。".into()),
        };
    }
    if value != expected {
        return Status {
            enabled: true,
            state: "stale".into(),
            reason: Some("启动项指向旧程序位置。关闭后重新开启可登记当前位置。".into()),
        };
    }
    Status {
        enabled: true,
        state: "enabled".into(),
        reason: None,
    }
}

pub fn read() -> Result<Status, String> {
    let Some((kind, bytes)) = query(HKEY_CURRENT_USER, RUN, VALUE)? else {
        return Ok(Status::disabled());
    };
    let value = string(kind, &bytes)?;
    let approval = query(HKEY_CURRENT_USER, APPROVED, VALUE)?;
    Ok(classify(
        &value,
        &command(&command_path()?)?,
        approval
            .as_ref()
            .filter(|(kind, _)| *kind == REG_BINARY)
            .map(|(_, v)| v.as_slice()),
    ))
}

fn write(path: &str, name: &str, value: Option<&str>) -> Result<(), String> {
    let mut key = ptr::null_mut();
    let result = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            wide(path).as_ptr(),
            0,
            ptr::null_mut(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            ptr::null(),
            &mut key,
            ptr::null_mut(),
        )
    };
    if result != 0 {
        return Err(format!("无法打开 Windows 启动项：错误码 {result}"));
    }
    let key = Key(key);
    let result = if let Some(value) = value {
        let value = wide(value);
        unsafe {
            RegSetValueExW(
                key.0,
                wide(name).as_ptr(),
                0,
                REG_SZ,
                value.as_ptr().cast(),
                (value.len() * 2) as u32,
            )
        }
    } else {
        unsafe { RegDeleteValueW(key.0, wide(name).as_ptr()) }
    };
    if result != 0 && result != 2 {
        Err(format!("无法修改 Windows 启动项：错误码 {result}"))
    } else {
        Ok(())
    }
}

pub fn set(enabled: bool) -> Result<(), String> {
    let value = if enabled {
        Some(command(&command_path()?)?)
    } else {
        None
    };
    write(RUN, VALUE, value.as_deref())
}

pub fn installed(executable: &Path) -> Result<(), String> {
    let current = executable
        .parent()
        .and_then(|p| std::fs::canonicalize(p).ok())
        .ok_or("无法确认正式安装位置")?;
    for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        if let Some((kind, bytes)) = query(
            hive,
            "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\SoDeck",
            "InstallLocation",
        )? {
            let value = string(kind, &bytes)?;
            if std::fs::canonicalize(value.trim_matches('"')).is_ok_and(|p| p == current) {
                return Ok(());
            }
        }
    }
    Err("请先使用正式安装包安装 SoDeck，再开启登录启动。".into())
}

pub(super) fn verify(directory: &Path) -> Result<(), String> {
    let key = format!("Software\\SoDeck-Test\\Login-{}", std::process::id());
    let command = command(&directory.join("中文 space/sodeck.exe"))?;
    let result = (|| {
        write(&key, VALUE, Some(&command))?;
        let (kind, bytes) = query(HKEY_CURRENT_USER, &key, VALUE)?.ok_or("测试登记缺失")?;
        if string(kind, &bytes)? != command {
            return Err("测试登记路径不一致".into());
        }
        write(&key, VALUE, None)?;
        if query(HKEY_CURRENT_USER, &key, VALUE)?.is_some() {
            return Err("测试登记未移除".into());
        }
        Ok(())
    })();
    unsafe {
        RegDeleteKeyW(HKEY_CURRENT_USER, wide(&key).as_ptr());
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn system_disabled_is_never_reported_as_enabled() {
        let value = "\"C:\\中文 space\\sodeck.exe\" --login-start";
        assert_eq!(
            command(Path::new("C:\\中文 space\\sodeck.exe")).unwrap(),
            value
        );
        assert_eq!(classify(value, value, None).state, "enabled");
        assert_eq!(classify("old", value, None).state, "stale");
        let blocked = classify(value, value, Some(&[3, 0, 0, 0]));
        assert!(!blocked.enabled);
        assert_eq!(blocked.state, "blocked");
    }
    #[test]
    fn registry_roundtrip_uses_a_non_startup_test_key() {
        verify(&std::env::temp_dir()).unwrap();
    }
}

//! System default applications; callers validate the business target before dispatch.
use std::path::{Path, PathBuf};

pub(crate) async fn directory(_handle: tauri::AppHandle, path: PathBuf) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        validate_directory(&path)?;
        #[cfg(windows)]
        return windows_open(path.as_os_str()).map_err(|e| format!("系统无法打开目录：{e}"));
        #[cfg(not(windows))]
        tauri_plugin_opener::open_path(&path, None::<&str>)
            .map_err(|e| format!("系统无法打开目录：{e}"))
    })
    .await
    .map_err(|e| e.to_string())?
}

pub(crate) async fn web_page(_handle: tauri::AppHandle, url: reqwest::Url) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        #[cfg(windows)]
        return windows_open(std::ffi::OsStr::new(url.as_str()))
            .map_err(|e| format!("系统无法打开网页：{e}"));
        #[cfg(not(windows))]
        tauri_plugin_opener::open_url(url.as_str(), None::<&str>)
            .map_err(|e| format!("系统无法打开网页：{e}"))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(windows)]
fn windows_open(target: &std::ffi::OsStr) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::{
        System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED},
        UI::{
            Shell::{ShellExecuteExW, SEE_MASK_FLAG_NO_UI, SHELLEXECUTEINFOW},
            WindowsAndMessaging::SW_SHOWNORMAL,
        },
    };
    let target: Vec<u16> = target.encode_wide().chain([0]).collect();
    let mut request = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        // A broken association must return an error, not wait in a hidden Shell dialog.
        fMask: SEE_MASK_FLAG_NO_UI,
        lpFile: target.as_ptr(),
        nShow: SW_SHOWNORMAL,
        ..unsafe { std::mem::zeroed() }
    };
    unsafe {
        let initialized = CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32);
        let result = if ShellExecuteExW(&mut request) != 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        };
        if initialized >= 0 {
            CoUninitialize();
        }
        result
    }
}

fn validate_directory(path: &Path) -> Result<(), String> {
    if !path.is_absolute() || path.as_os_str().as_encoded_bytes().contains(&0) {
        return Err("本地目录路径无效".into());
    }
    if !std::fs::metadata(path)
        .map_err(|e| format!("无法读取本地目录：{e}"))?
        .is_dir()
    {
        return Err("本地目标不是目录".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    #[test]
    fn a_missing_shell_target_returns_an_error_without_interactive_repair() {
        let missing = std::env::temp_dir()
            .join(format!("sodeck-missing-launcher-{}", std::process::id()))
            .join("missing.exe");
        assert!(!missing.exists());
        assert!(windows_open(missing.as_os_str()).is_err());
    }
    #[test]
    fn directory_validation_precedes_system_dispatch() {
        assert!(validate_directory(Path::new("relative")).is_err());
        assert!(validate_directory(&std::env::temp_dir().join("bad\0path")).is_err());
        assert!(validate_directory(
            &std::env::temp_dir().join(format!("sodeck-missing-{}", std::process::id()))
        )
        .is_err());
        assert_eq!(
            validate_directory(&std::env::current_exe().unwrap()).unwrap_err(),
            "本地目标不是目录"
        );
        assert!(validate_directory(&std::env::temp_dir()).is_ok());
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn validation_preserves_non_utf8_paths() {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt};
        let path = std::env::temp_dir().join(OsString::from_vec(
            format!("sodeck-native-{}-", std::process::id())
                .into_bytes()
                .into_iter()
                .chain([0xff])
                .collect(),
        ));
        std::fs::create_dir(&path).unwrap();
        let result = validate_directory(&path);
        std::fs::remove_dir(&path).unwrap();
        assert!(result.is_ok());
    }
}

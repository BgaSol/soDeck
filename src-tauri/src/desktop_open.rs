//! System default applications; callers validate the business target before dispatch.
use std::path::{Path, PathBuf};

pub(crate) async fn directory(_handle: tauri::AppHandle, path: PathBuf) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        validate_directory(&path)?;
        tauri_plugin_opener::open_path(&path, None::<&str>)
            .map_err(|e| format!("系统无法打开目录：{e}"))
    })
    .await
    .map_err(|e| e.to_string())?
}

pub(crate) async fn web_page(_handle: tauri::AppHandle, url: reqwest::Url) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        tauri_plugin_opener::open_url(url.as_str(), None::<&str>)
            .map_err(|e| format!("系统无法打开网页：{e}"))
    })
    .await
    .map_err(|e| e.to_string())?
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

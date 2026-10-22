use super::*;
use tauri::utils::{config::BundleType, platform::bundle_type};
pub(super) fn capability() -> Capability {
    profile(
        std::env::consts::OS,
        std::env::consts::ARCH,
        bundle_type(),
        cfg!(debug_assertions),
    )
}
pub(super) fn profile(os: &str, arch: &str, bundle: Option<BundleType>, debug: bool) -> Capability {
    let (target, format, supported) = match (os, arch, bundle) {
        ("windows", "x86_64", Some(BundleType::Nsis)) => {
            (Some("windows-x86_64-nsis"), Some("exe"), true)
        }
        ("windows", "x86_64", Some(BundleType::Msi)) => (None, Some("msi"), false),
        ("linux", "x86_64", Some(BundleType::AppImage)) => {
            (Some("linux-x86_64"), Some("appimage"), true)
        }
        ("linux", "x86_64", Some(BundleType::Deb)) => (None, Some("deb"), false),
        // Developer ID/notarization and real-machine qualification are required before enabling macOS in-app installation.
        ("macos", "aarch64", _) => (None, Some("dmg"), false),
        _ => (None, None, false),
    };
    Capability {
        mode: if supported && !debug {
            "inApp"
        } else if format.is_some() {
            "manual"
        } else {
            "unavailable"
        }
        .into(),
        reason: if debug {
            Some("开发构建仅检查版本；请使用正式安装包升级。".into())
        } else if supported {
            None
        } else if format.is_some() {
            Some("当前安装方式请下载对应安装包后手动安装。".into())
        } else {
            Some("当前平台或安装来源未识别，请查看官方发布页。".into())
        },
        target: target.map(str::to_owned),
        manual_format: format.map(str::to_owned),
    }
}
pub(super) fn manual_asset(version: &str, format: &str) -> Option<(String, String)> {
    let (key, suffix) = match format {
        "exe" => ("windows-x86_64-nsis", "windows_x64-setup.exe"),
        "msi" => ("windows-x86_64-msi", "windows_x64.msi"),
        "dmg" => ("darwin-aarch64-dmg", "macos_arm64.dmg"),
        "deb" => ("linux-x86_64-deb", "linux_x64.deb"),
        "appimage" => ("linux-x86_64-appimage", "linux_x64.AppImage"),
        _ => return None,
    };
    Some((key.into(), format!("SoDeck_{version}_{suffix}")))
}
pub(super) fn package_name(version: &str, target: &str) -> Option<String> {
    let suffix = match target {
        "windows-x86_64-nsis" => "windows_x64-setup.exe",
        "linux-x86_64" => "linux_x64.AppImage",
        "darwin-aarch64" => "macos_arm64.app.tar.gz",
        _ => return None,
    };
    Some(format!("SoDeck_{version}_{suffix}"))
}
pub(super) fn preflight_path() -> AppResult<()> {
    if capability().mode != "inApp" {
        return Err(error("此安装方式不支持应用内安装。"));
    }
    let executable = std::env::current_exe().map_err(|_| error("无法读取应用安装位置。"))?;
    #[cfg(target_os = "linux")]
    {
        let appimage = std::env::var_os("APPIMAGE")
            .map(PathBuf::from)
            .ok_or_else(|| error("无法定位原 AppImage 文件。"))?;
        let parent = appimage
            .parent()
            .ok_or_else(|| error("AppImage 安装位置无效。"))?;
        tempfile::NamedTempFile::new_in(parent)
            .map_err(|_| error("AppImage 所在目录不可写，请手动更新。"))?;
        std::fs::OpenOptions::new()
            .write(true)
            .open(&appimage)
            .map_err(|_| error("AppImage 不可替换，请手动更新。"))?;
    }
    if !executable.is_file() {
        return Err(error("应用安装位置不可用。"));
    }
    Ok(())
}

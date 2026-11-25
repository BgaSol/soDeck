use objc2::{msg_send, rc::autoreleasepool, rc::Retained, runtime::AnyClass, runtime::AnyObject};
use objc2_foundation::{NSAppleEventManager, NSBundle, NSError, NSString, NSURL};
use std::path::{Path, PathBuf};

#[link(name = "ServiceManagement", kind = "framework")]
extern "C" {}
#[link(name = "CoreServices", kind = "framework")]
extern "C" {
    fn LSRegisterURL(url: *const std::ffi::c_void, update: u8) -> i32;
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum State {
    NotRegistered,
    Enabled,
    RequiresApproval,
    NotFound,
}

fn state(value: usize) -> Result<State, String> {
    match value {
        0 => Ok(State::NotRegistered),
        1 => Ok(State::Enabled),
        2 => Ok(State::RequiresApproval),
        3 => Ok(State::NotFound),
        _ => Err(format!("macOS 返回未知登录项状态：{value}")),
    }
}

pub fn available() -> bool {
    AnyClass::get(c"SMAppService").is_some()
}

fn service() -> Result<Retained<AnyObject>, String> {
    let class = AnyClass::get(c"SMAppService").ok_or("当前系统不支持主应用登录项")?;
    // Class lookup keeps macOS 11/12 from linking a symbol introduced in macOS 13.
    let object: Retained<AnyObject> = unsafe { msg_send![class, mainAppService] };
    Ok(object)
}

pub fn status() -> Result<State, String> {
    autoreleasepool(|_| {
        let service = service()?;
        state(unsafe { msg_send![&*service, status] })
    })
}

pub fn legacy_status(path: &Path) -> Result<State, String> {
    autoreleasepool(|_| {
        let class = AnyClass::get(c"SMAppService").ok_or("当前系统不支持登录项授权查询")?;
        let path = NSString::from_str(path.to_str().ok_or("登录项路径不是有效 Unicode")?);
        let url = NSURL::fileURLWithPath(&path);
        state(unsafe { msg_send![class, statusForLegacyPlistAtURL: &*url] })
    })
}

pub fn set(enabled: bool) -> Result<(), String> {
    autoreleasepool(|_| {
        let service = service()?;
        let mut error: *mut NSError = std::ptr::null_mut();
        let success: bool = unsafe {
            if enabled {
                msg_send![&*service, registerAndReturnError: &mut error]
            } else {
                msg_send![&*service, unregisterAndReturnError: &mut error]
            }
        };
        if success {
            return Ok(());
        }
        let reason = unsafe { error.as_ref() }
            .map(|e| format!("{}（{}）", e.localizedDescription(), e.code()))
            .unwrap_or_else(|| "系统未提供具体原因".into());
        Err(format!("macOS 登录项修改未完成：{reason}"))
    })
}

pub struct Bundle {
    pub path: PathBuf,
    pub executable: PathBuf,
}

pub fn bundle(executable: &Path, installed: bool) -> Result<Bundle, String> {
    autoreleasepool(|_| {
        let main = NSBundle::mainBundle();
        let path = PathBuf::from(main.bundlePath().to_string());
        let binary = PathBuf::from(
            main.executablePath()
                .ok_or("应用 bundle 缺少可执行文件")?
                .to_string(),
        );
        if main
            .bundleIdentifier()
            .as_deref()
            .map(ToString::to_string)
            .as_deref()
            != Some("com.bgasol.sodeck")
            || path.extension().is_none_or(|x| x != "app")
            || binary != path.join("Contents/MacOS/sodeck")
            || std::fs::canonicalize(&binary).map_err(|e| e.to_string())?
                != std::fs::canonicalize(executable).map_err(|e| e.to_string())?
        {
            return Err("当前程序不是完整的 SoDeck.app，无法修改登录项".into());
        }
        let icon = main
            .objectForInfoDictionaryKey(&NSString::from_str("CFBundleIconFile"))
            .and_then(|value| value.downcast::<NSString>().ok())
            .ok_or("应用 bundle 缺少图标声明")?
            .to_string();
        if icon != "icon.icns" || !path.join("Contents/Resources").join(&icon).is_file() {
            return Err("应用 bundle 图标资源不完整，请重新安装 SoDeck".into());
        }
        if installed {
            let real = std::fs::canonicalize(&path).map_err(|e| e.to_string())?;
            if real.starts_with("/Volumes")
                || real.starts_with("/private/var/folders")
                || real.starts_with("/private/tmp")
                || real.starts_with(std::env::temp_dir())
                || real
                    .components()
                    .any(|x| x.as_os_str() == "AppTranslocation")
            {
                return Err("请先将 SoDeck.app 移出磁盘映像或临时目录，再开启登录启动。".into());
            }
        }
        Ok(Bundle {
            path,
            executable: binary,
        })
    })
}

pub fn register_bundle(bundle: &Bundle) -> Result<(), String> {
    autoreleasepool(|_| {
        let path = NSString::from_str(bundle.path.to_str().ok_or("应用路径不是有效 Unicode")?);
        let url = NSURL::fileURLWithPath(&path);
        // NSURL and CFURL are toll-free bridged. Only update this application's registration.
        let result = unsafe { LSRegisterURL(Retained::as_ptr(&url).cast(), 1) };
        if result == 0 {
            Ok(())
        } else {
            Err(format!("无法登记 SoDeck 应用身份：{result}"))
        }
    })
}

/// Read synchronously during Tauri setup, inside applicationDidFinishLaunching.
pub fn launched_at_login() -> bool {
    autoreleasepool(|_| {
        NSAppleEventManager::sharedAppleEventManager()
            .currentAppleEvent()
            .is_some_and(|event| {
                let class: u32 = unsafe { msg_send![&*event, eventClass] };
                let id: u32 = unsafe { msg_send![&*event, eventID] };
                let property: Option<Retained<AnyObject>> = unsafe {
                    msg_send![&*event, paramDescriptorForKeyword: u32::from_be_bytes(*b"prdt")]
                };
                class == u32::from_be_bytes(*b"aevt")
                    && id == u32::from_be_bytes(*b"oapp")
                    && property.is_some_and(|value| {
                        let code: u32 = unsafe { msg_send![&*value, enumCodeValue] };
                        code == u32::from_be_bytes(*b"lgit")
                    })
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_status_reads_native_api_without_registering_a_login_item() {
        if !available() {
            return; // SMAppService is only available on macOS 13+.
        }
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("legacy login item.plist");
        std::fs::write(
            &path,
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>Label</key><string>com.bgasol.sodeck.login-regression</string>
<key>ProgramArguments</key><array><string>/usr/bin/true</string></array>
<key>RunAtLoad</key><true/>
</dict></plist>"#,
        )
        .unwrap();
        assert!(legacy_status(&path).is_ok());
    }
}

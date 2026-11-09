//! System default applications; callers validate the business target before dispatch.
use std::path::PathBuf;

enum Target {
    Directory(PathBuf),
    Web(reqwest::Url),
}

pub(crate) async fn directory(handle: tauri::AppHandle, path: PathBuf) -> Result<(), String> {
    open(handle, Target::Directory(path)).await
}

pub(crate) async fn web_page(handle: tauri::AppHandle, url: reqwest::Url) -> Result<(), String> {
    open(handle, Target::Web(url)).await
}

async fn open(handle: tauri::AppHandle, target: Target) -> Result<(), String> {
    #[cfg(windows)]
    {
        let _ = handle;
        tauri::async_runtime::spawn_blocking(move || native(target))
            .await
            .map_err(|e| e.to_string())?
    }
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        handle
            .run_on_main_thread(move || {
                if !sender.is_closed() {
                    let _ = sender.send(native(target));
                }
            })
            .map_err(|e| e.to_string())?;
        receiver.await.map_err(|e| e.to_string())?
    }
}

#[cfg(windows)]
fn native(target: Target) -> Result<(), String> {
    use std::{ffi::OsString, os::windows::ffi::OsStrExt};
    use windows_sys::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};
    let value = match target {
        Target::Directory(path) => path.into_os_string(),
        Target::Web(url) => OsString::from(url.as_str()),
    };
    let mut value: Vec<u16> = value.encode_wide().collect();
    if value.contains(&0) {
        return Err("系统打开目标含 NUL".into());
    }
    value.push(0);
    // The path/URL buffer is terminated and lives through the synchronous Shell call.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            windows_sys::w!("open"),
            value.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    if result as isize <= 32 {
        Err(format!("系统无法打开目标（错误码 {}）", result as isize))
    } else {
        Ok(())
    }
}

#[cfg(target_os = "macos")]
fn native(target: Target) -> Result<(), String> {
    use objc2::rc::autoreleasepool;
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{NSString, NSURL};
    use std::{ffi::CString, os::unix::ffi::OsStrExt, ptr::NonNull};
    autoreleasepool(|_| {
        let url = match target {
            Target::Directory(path) => {
                let path = CString::new(path.as_os_str().as_bytes()).map_err(|_| "本地路径无效")?;
                // NSURL copies this NUL-terminated filesystem representation during construction.
                unsafe {
                    NSURL::fileURLWithFileSystemRepresentation_isDirectory_relativeToURL(
                        NonNull::new(path.as_ptr().cast_mut()).unwrap(),
                        true,
                        None,
                    )
                }
            }
            Target::Web(url) => {
                NSURL::URLWithString(&NSString::from_str(url.as_str())).ok_or("网页 URL 无效")?
            }
        };
        if NSWorkspace::sharedWorkspace().openURL(&url) {
            Ok(())
        } else {
            Err("系统无法打开目标".into())
        }
    })
}

#[cfg(target_os = "linux")]
fn native(target: Target) -> Result<(), String> {
    let url = match target {
        Target::Directory(path) => {
            reqwest::Url::from_file_path(path).map_err(|_| "本地路径无效")?
        }
        Target::Web(url) => url,
    };
    gtk::show_uri_on_window(
        None::<&gtk::Window>,
        url.as_str(),
        gdk::ffi::GDK_CURRENT_TIME as u32,
    )
    .map_err(|e| format!("系统无法打开目标：{e}"))
}

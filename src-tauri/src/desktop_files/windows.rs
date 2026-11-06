use super::*;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use windows_sys::Win32::{
    System::DataExchange::{
        CloseClipboard, GetClipboardData, GetClipboardSequenceNumber, IsClipboardFormatAvailable,
        OpenClipboard,
    },
    UI::Shell::{DragQueryFileW, ShellExecuteW},
};
pub(super) fn reveal(path: PathBuf) -> Result<(), String> {
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let open: Vec<u16> = "open".encode_utf16().chain(Some(0)).collect();
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            open.as_ptr(),
            path.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
        )
    };
    if result as isize <= 32 {
        Err("无法打开本地目标目录".into())
    } else {
        Ok(())
    }
}
pub(super) fn read(files: bool) -> Result<Clipboard, String> {
    let revision = unsafe { GetClipboardSequenceNumber() }.to_string();
    let has_files = unsafe { IsClipboardFormatAvailable(15) } != 0;
    if revision != unsafe { GetClipboardSequenceNumber() }.to_string() {
        return Err("系统文件剪贴板在读取期间变化，请重试".into());
    }
    if !files {
        return Ok(Clipboard {
            paths: vec![],
            revision,
            has_files,
        });
    }
    if unsafe { IsClipboardFormatAvailable(15) } == 0 {
        return Ok(Clipboard {
            paths: vec![],
            revision,
            has_files: false,
        });
    }
    let mut opened = false;
    for _ in 0..5 {
        if unsafe { OpenClipboard(std::ptr::null_mut()) } != 0 {
            opened = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    if !opened {
        return Err("系统剪贴板正忙，请重试".into());
    }
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            unsafe {
                CloseClipboard();
            }
        }
    }
    let _guard = Guard;
    let start = unsafe { GetClipboardSequenceNumber() };
    let handle = unsafe { GetClipboardData(15) };
    if handle.is_null() {
        return Err("无法读取系统文件剪贴板".into());
    }
    let count = unsafe { DragQueryFileW(handle, u32::MAX, std::ptr::null_mut(), 0) };
    if count > 4096 {
        return Err("复制条目超过 4096 项".into());
    }
    let mut paths = Vec::new();
    for index in 0..count {
        let length = unsafe { DragQueryFileW(handle, index, std::ptr::null_mut(), 0) };
        if length == 0 || length > 32768 {
            return Err("系统文件剪贴板路径长度无效".into());
        }
        let mut buffer = vec![0u16; length as usize + 1];
        let actual =
            unsafe { DragQueryFileW(handle, index, buffer.as_mut_ptr(), buffer.len() as u32) };
        if actual != length {
            return Err("系统文件剪贴板在读取期间变化".into());
        }
        paths.push(PathBuf::from(std::ffi::OsString::from_wide(
            &buffer[..length as usize],
        )));
    }
    let end = unsafe { GetClipboardSequenceNumber() };
    if start != end {
        return Err("系统文件剪贴板在读取期间变化，请重试".into());
    }
    Ok(Clipboard {
        has_files: !paths.is_empty(),
        paths,
        revision: end.to_string(),
    })
}

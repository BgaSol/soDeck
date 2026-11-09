use super::*;
use objc2::rc::autoreleasepool;
use objc2_app_kit::{NSPasteboard, NSPasteboardTypeFileURL};
use objc2_foundation::NSURL;
use std::{ffi::CStr, os::unix::ffi::OsStrExt};

pub(super) fn read(files: bool) -> Result<Clipboard, String> {
    autoreleasepool(|_| {
        let board = NSPasteboard::generalPasteboard();
        let revision = board.changeCount().to_string();
        // This framework constant has static lifetime and is read only.
        let file_type = unsafe { NSPasteboardTypeFileURL };
        let has_files = board
            .types()
            .is_some_and(|types| types.containsObject(file_type));
        let mut paths = Vec::new();
        if files {
            if let Some(items) = board.pasteboardItems() {
                if items.count() > 4096 {
                    return Err("复制条目超过 4096 项".into());
                }
                for index in 0..items.count() {
                    let Some(text) = items.objectAtIndex(index).stringForType(file_type) else {
                        continue;
                    };
                    let url = NSURL::URLWithString(&text).ok_or("无法读取本地文件 URL")?;
                    if !url.isFileURL() {
                        continue;
                    }
                    // Keep strict source/encoding checks before NSURL normalizes filesystem bytes.
                    super::uri_paths(text.to_string().as_bytes())?;
                    // Copy the filesystem bytes while the retained NSURL is alive.
                    let bytes = unsafe { CStr::from_ptr(url.fileSystemRepresentation().as_ptr()) }
                        .to_bytes();
                    paths.push(PathBuf::from(std::ffi::OsStr::from_bytes(bytes)));
                }
            }
        }
        if revision != board.changeCount().to_string() {
            return Err("系统文件剪贴板在读取期间变化".into());
        }
        Ok(Clipboard {
            has_files: if files { !paths.is_empty() } else { has_files },
            paths,
            revision,
        })
    })
}

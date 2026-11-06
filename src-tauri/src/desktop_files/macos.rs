use super::*;
use std::{
    ffi::{c_char, c_void, CStr},
    os::unix::ffi::OsStrExt,
};
type Obj = *mut c_void;
#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {}
#[link(name = "objc")]
unsafe extern "C" {
    fn objc_getClass(name: *const c_char) -> Obj;
    fn sel_registerName(name: *const c_char) -> Obj;
    fn objc_msgSend();
}
unsafe fn msg(object: Obj, selector: &CStr) -> Obj {
    let function: unsafe extern "C" fn(Obj, Obj) -> Obj =
        unsafe { std::mem::transmute(objc_msgSend as unsafe extern "C" fn()) };
    unsafe { function(object, sel_registerName(selector.as_ptr())) }
}
unsafe fn arg(object: Obj, selector: &CStr, value: Obj) -> Obj {
    let function: unsafe extern "C" fn(Obj, Obj, Obj) -> Obj =
        unsafe { std::mem::transmute(objc_msgSend as unsafe extern "C" fn()) };
    unsafe { function(object, sel_registerName(selector.as_ptr()), value) }
}
unsafe fn number(object: Obj, selector: &CStr) -> usize {
    let function: unsafe extern "C" fn(Obj, Obj) -> usize =
        unsafe { std::mem::transmute(objc_msgSend as unsafe extern "C" fn()) };
    unsafe { function(object, sel_registerName(selector.as_ptr())) }
}
pub(super) fn reveal(path: PathBuf) -> Result<(), String> {
    unsafe {
        let pool = msg(objc_getClass(c"NSAutoreleasePool".as_ptr()), c"new");
        struct Pool(Obj);
        impl Drop for Pool {
            fn drop(&mut self) {
                unsafe {
                    msg(self.0, c"drain");
                }
            }
        }
        let _pool = Pool(pool);
        let bytes =
            std::ffi::CString::new(path.as_os_str().as_bytes()).map_err(|_| "本地路径无效")?;
        let url_fn: unsafe extern "C" fn(Obj, Obj, *const c_char, bool, Obj) -> Obj =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let url = url_fn(
            objc_getClass(c"NSURL".as_ptr()),
            sel_registerName(
                c"fileURLWithFileSystemRepresentation:isDirectory:relativeToURL:".as_ptr(),
            ),
            bytes.as_ptr(),
            true,
            std::ptr::null_mut(),
        );
        let workspace = msg(objc_getClass(c"NSWorkspace".as_ptr()), c"sharedWorkspace");
        let open: unsafe extern "C" fn(Obj, Obj, Obj) -> bool =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        if open(workspace, sel_registerName(c"openURL:".as_ptr()), url) {
            Ok(())
        } else {
            Err("无法打开本地目标目录".into())
        }
    }
}
pub(super) fn read(files: bool) -> Result<Clipboard, String> {
    unsafe {
        let pool = msg(objc_getClass(c"NSAutoreleasePool".as_ptr()), c"new");
        struct Pool(Obj);
        impl Drop for Pool {
            fn drop(&mut self) {
                unsafe {
                    msg(self.0, c"drain");
                }
            }
        }
        let _pool = Pool(pool);
        let board = msg(
            objc_getClass(c"NSPasteboard".as_ptr()),
            c"generalPasteboard",
        );
        let revision = number(board, c"changeCount").to_string();
        let mut paths = Vec::new();
        if files {
            let list = msg(board, c"pasteboardItems");
            let count = number(list, c"count");
            if count > 4096 {
                return Err("复制条目超过 4096 项".into());
            }
            let file_type = arg(
                objc_getClass(c"NSString".as_ptr()),
                c"stringWithUTF8String:",
                c"public.file-url".as_ptr() as Obj,
            );
            let at: unsafe extern "C" fn(Obj, Obj, usize) -> Obj =
                std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
            for index in 0..count {
                let item = at(list, sel_registerName(c"objectAtIndex:".as_ptr()), index);
                let text = arg(item, c"stringForType:", file_type);
                if text.is_null() {
                    continue;
                }
                let url = arg(objc_getClass(c"NSURL".as_ptr()), c"URLWithString:", text);
                if number(url, c"isFileURL") == 0 {
                    continue;
                }
                let representation = msg(url, c"fileSystemRepresentation") as *const c_char;
                if representation.is_null() {
                    return Err("无法读取本地文件 URL".into());
                }
                paths.push(PathBuf::from(std::ffi::OsStr::from_bytes(
                    CStr::from_ptr(representation).to_bytes(),
                )));
            }
        }
        if revision != number(board, c"changeCount").to_string() {
            return Err("系统文件剪贴板在读取期间变化".into());
        }
        Ok(Clipboard { paths, revision })
    }
}

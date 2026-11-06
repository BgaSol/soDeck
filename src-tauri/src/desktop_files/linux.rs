use super::*;
use std::{
    ffi::{c_char, c_int, c_void},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex, Once, OnceLock,
    },
};
type Ptr = *mut c_void;
static REVISION: AtomicU64 = AtomicU64::new(1);
static WATCH: Once = Once::new();
static REQUEST_ID: AtomicU64 = AtomicU64::new(1);
static REQUESTS: OnceLock<Mutex<std::collections::HashMap<usize, Request>>> = OnceLock::new();
fn requests() -> &'static Mutex<std::collections::HashMap<usize, Request>> {
    REQUESTS.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}
#[link(name = "gtk-3")]
unsafe extern "C" {
    fn gtk_show_uri_on_window(
        window: Ptr,
        uri: *const c_char,
        timestamp: u32,
        error: *mut Ptr,
    ) -> c_int;
    fn gtk_clipboard_get(selection: Ptr) -> Ptr;
    fn gtk_clipboard_request_targets(
        clipboard: Ptr,
        callback: unsafe extern "C" fn(Ptr, *mut Ptr, c_int, Ptr),
        data: Ptr,
    );
    fn gtk_clipboard_request_contents(
        clipboard: Ptr,
        target: Ptr,
        callback: unsafe extern "C" fn(Ptr, Ptr, Ptr),
        data: Ptr,
    );
    fn gtk_selection_data_get_length(data: Ptr) -> c_int;
    fn gtk_selection_data_get_data(data: Ptr) -> *const u8;
}
#[link(name = "glib-2.0")]
unsafe extern "C" {
    fn g_error_free(error: Ptr);
}
pub(super) fn reveal(path: PathBuf) -> Result<(), String> {
    use std::os::unix::ffi::OsStrExt;
    let mut uri = String::from("file://");
    for byte in path.as_os_str().as_bytes() {
        if byte.is_ascii_alphanumeric() || b"/-._~".contains(byte) {
            uri.push(*byte as char);
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    let uri = std::ffi::CString::new(uri).map_err(|_| "本地路径无效")?;
    let mut error = std::ptr::null_mut();
    if unsafe { gtk_show_uri_on_window(std::ptr::null_mut(), uri.as_ptr(), 0, &mut error) } != 0 {
        Ok(())
    } else {
        if !error.is_null() {
            unsafe {
                g_error_free(error);
            }
        }
        Err("无法打开本地目标目录".into())
    }
}
#[link(name = "gdk-3")]
unsafe extern "C" {
    fn gdk_atom_intern_static_string(name: *const c_char) -> Ptr;
}
#[link(name = "gobject-2.0")]
unsafe extern "C" {
    fn g_signal_connect_data(
        object: Ptr,
        name: *const c_char,
        callback: unsafe extern "C" fn(Ptr, Ptr, Ptr),
        data: Ptr,
        destroy: Option<unsafe extern "C" fn(Ptr, Ptr)>,
        flags: c_int,
    ) -> u64;
}
struct Request {
    sender: Option<tokio::sync::oneshot::Sender<Result<Clipboard, String>>>,
    revision: String,
    fallback: bool,
}
unsafe extern "C" fn changed(_: Ptr, _: Ptr, _: Ptr) {
    REVISION.fetch_add(1, Ordering::SeqCst);
}
unsafe extern "C" fn targets_received(_: Ptr, targets: *mut Ptr, count: c_int, userdata: Ptr) {
    unsafe {
        let Some(mut request) = requests().lock().unwrap().remove(&(userdata as usize)) else {
            return;
        };
        let supported = [
            gdk_atom_intern_static_string(c"text/uri-list".as_ptr()),
            gdk_atom_intern_static_string(c"x-special/gnome-copied-files".as_ptr()),
        ];
        let has_files = count > 0
            && !targets.is_null()
            && std::slice::from_raw_parts(targets, count as usize)
                .iter()
                .any(|atom| supported.contains(atom));
        let result = if request.revision != REVISION.load(Ordering::SeqCst).to_string() {
            Err("系统文件剪贴板在读取期间变化".into())
        } else {
            Ok(Clipboard {
                paths: vec![],
                revision: request.revision,
                has_files,
            })
        };
        if let Some(sender) = request.sender.take() {
            let _ = sender.send(result);
        }
    }
}
unsafe extern "C" fn received(clipboard: Ptr, data: Ptr, userdata: Ptr) {
    unsafe {
        let Some(mut request) = requests().lock().unwrap().remove(&(userdata as usize)) else {
            return;
        };
        let length = gtk_selection_data_get_length(data);
        if length < 0 && !request.fallback {
            request.fallback = true;
            requests()
                .lock()
                .unwrap()
                .insert(userdata as usize, request);
            gtk_clipboard_request_contents(
                clipboard,
                gdk_atom_intern_static_string(c"x-special/gnome-copied-files".as_ptr()),
                received,
                userdata,
            );
            return;
        }
        let result = if length < 0 {
            Ok(Clipboard {
                paths: vec![],
                revision: request.revision.clone(),
                has_files: false,
            })
        } else if length > 4 * 1024 * 1024 {
            Err("文件剪贴板内容过大".into())
        } else {
            let bytes = if length == 0 {
                &[][..]
            } else {
                let pointer = gtk_selection_data_get_data(data);
                if pointer.is_null() {
                    if let Some(sender) = request.sender.take() {
                        let _ = sender.send(Err("无法读取文件剪贴板".into()));
                    }
                    return;
                }
                std::slice::from_raw_parts(pointer, length as usize)
            };
            super::uri_paths(bytes).map(|paths| Clipboard {
                has_files: !paths.is_empty(),
                paths,
                revision: request.revision.clone(),
            })
        };
        let result = if request.revision != REVISION.load(Ordering::SeqCst).to_string() {
            Err("系统文件剪贴板在读取期间变化".into())
        } else {
            result
        };
        if let Some(sender) = request.sender.take() {
            let _ = sender.send(result);
        }
    }
}
pub(super) fn read(files: bool, sender: tokio::sync::oneshot::Sender<Result<Clipboard, String>>) {
    unsafe {
        let clipboard = gtk_clipboard_get(gdk_atom_intern_static_string(c"CLIPBOARD".as_ptr()));
        if clipboard.is_null() {
            let _ = sender.send(Err("当前桌面无法访问文件剪贴板".into()));
            return;
        }
        WATCH.call_once(|| {
            g_signal_connect_data(
                clipboard,
                c"owner-change".as_ptr(),
                changed,
                std::ptr::null_mut(),
                None,
                0,
            );
        });
        let revision = REVISION.load(Ordering::SeqCst).to_string();
        let request = Request {
            sender: Some(sender),
            revision,
            fallback: false,
        };
        let mut pending = requests().lock().unwrap();
        pending.retain(|_, r| r.sender.as_ref().is_some_and(|s| !s.is_closed()));
        if pending.len() >= 4 {
            if let Some(sender) = request.sender {
                let _ = sender.send(Err("文件剪贴板正忙，请稍后重试".into()));
            }
            return;
        }
        let id = REQUEST_ID.fetch_add(1, Ordering::Relaxed) as usize;
        pending.insert(id, request);
        drop(pending);
        if !files {
            gtk_clipboard_request_targets(clipboard, targets_received, id as Ptr);
            return;
        }
        gtk_clipboard_request_contents(
            clipboard,
            gdk_atom_intern_static_string(c"text/uri-list".as_ptr()),
            received,
            id as Ptr,
        );
    }
}

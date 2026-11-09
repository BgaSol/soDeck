use super::{
    requests::{Pending, Request},
    *,
};
use glib::{prelude::*, translate::ToGlibPtr};
use std::{
    cell::RefCell,
    sync::atomic::{AtomicU64, Ordering},
};

static REVISION: AtomicU64 = AtomicU64::new(1);
thread_local! { static BACKEND: RefCell<Backend> = RefCell::new(Backend::default()); }
#[derive(Default)]
struct Backend {
    clipboard: Option<gtk::Clipboard>,
    subscription: Option<glib::SignalHandlerId>,
    pending: Pending,
    closed: bool,
}
fn revision() -> String {
    REVISION.load(Ordering::SeqCst).to_string()
}

fn clipboard() -> Result<gtk::Clipboard, String> {
    let existing = BACKEND.with(|state| {
        let state = state.borrow();
        if state.closed {
            return Err("文件剪贴板读取已结束".to_string());
        }
        Ok(state.clipboard.clone())
    })?;
    if let Some(clipboard) = existing {
        return Ok(clipboard);
    }
    let display = gdk::Display::default().ok_or("当前桌面无法访问文件剪贴板")?;
    let clipboard = gtk::Clipboard::for_display(&display, &gdk::SELECTION_CLIPBOARD);
    // gtk 0.18 has no typed owner-change convenience method; GLib owns the closure.
    let subscription = clipboard.connect_local("owner-change", false, |_| {
        REVISION.fetch_add(1, Ordering::SeqCst);
        None
    });
    BACKEND.with(|state| {
        let mut state = state.borrow_mut();
        state.clipboard = Some(clipboard.clone());
        state.subscription = Some(subscription);
    });
    Ok(clipboard)
}

pub(super) fn read(
    id: u64,
    files: bool,
    sender: tokio::sync::oneshot::Sender<Result<Clipboard, String>>,
) {
    if sender.is_closed() {
        return;
    }
    let clipboard = match clipboard() {
        Ok(clipboard) => clipboard,
        Err(error) => {
            let _ = sender.send(Err(error));
            return;
        }
    };
    let request = Request {
        revision: revision(),
        sender: Some(sender),
    };
    if let Err(request) = BACKEND.with(|state| state.borrow_mut().pending.insert(id, request)) {
        request.finish(Err("文件剪贴板正忙，请稍后重试".into()));
        return;
    }
    issue(&clipboard, id, files, false);
}

fn issue(clipboard: &gtk::Clipboard, id: u64, files: bool, fallback: bool) {
    let target = gdk::Atom::intern(if !files {
        "TARGETS"
    } else if fallback {
        "x-special/gnome-copied-files"
    } else {
        "text/uri-list"
    });
    clipboard.request_contents(&target, move |clipboard, data| {
        // An internal failure must not unwind through GTK's C callback trampoline.
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            received(clipboard, data, id, files, fallback)
        }))
        .is_err()
        {
            if let Some(request) = BACKEND.with(|state| state.borrow_mut().pending.take(id)) {
                request.finish(Err("文件剪贴板回调异常".into()));
            }
            eprintln!("文件剪贴板回调异常");
        }
    });
}

fn received(
    clipboard: &gtk::Clipboard,
    data: &gtk::SelectionData,
    id: u64,
    files: bool,
    fallback: bool,
) {
    let Some(request) = BACKEND.with(|state| state.borrow_mut().pending.take(id)) else {
        return;
    };
    if !request.active() {
        return;
    }
    if request.revision != revision() {
        request.finish(Err("系统文件剪贴板在读取期间变化".into()));
        return;
    }
    if files && data.length() < 0 && !fallback {
        if let Err(request) = BACKEND.with(|state| state.borrow_mut().pending.insert(id, request)) {
            request.finish(Err("文件剪贴板正忙，请稍后重试".into()));
            return;
        }
        issue(clipboard, id, files, true);
        return;
    }
    let result = read_selection(data, files, &request.revision);
    let result = if request.revision != revision() {
        Err("系统文件剪贴板在读取期间变化".into())
    } else {
        result
    };
    request.finish(result);
}

fn read_selection(
    data: &gtk::SelectionData,
    files: bool,
    revision: &str,
) -> Result<Clipboard, String> {
    let length = data.length();
    if length > 4 * 1024 * 1024 {
        return Err("文件剪贴板内容过大".into());
    }
    let mut paths = Vec::new();
    let has_files = if length < 0 {
        false
    } else if !files {
        let supported = [
            gdk::Atom::intern("text/uri-list"),
            gdk::Atom::intern("x-special/gnome-copied-files"),
        ];
        data.targets()
            .is_some_and(|targets| targets.iter().any(|atom| supported.contains(atom)))
    } else {
        let bytes = if length == 0 {
            &[][..]
        } else {
            // Borrow only after checking length; SelectionData remains alive for this callback.
            let pointer = unsafe { gtk::ffi::gtk_selection_data_get_data(data.to_glib_none().0) };
            if pointer.is_null() {
                return Err("无法读取文件剪贴板".into());
            }
            unsafe { std::slice::from_raw_parts(pointer, length as usize) }
        };
        paths = super::uri_paths(bytes)?;
        !paths.is_empty()
    };
    Ok(Clipboard {
        paths,
        revision: revision.into(),
        has_files,
    })
}

pub(super) fn cancel(id: u64) {
    BACKEND.with(|state| state.borrow_mut().pending.cancel(id));
}
pub(super) fn shutdown() {
    let (clipboard, subscription) = BACKEND.with(|state| {
        let mut state = state.borrow_mut();
        state.closed = true;
        state.pending.shutdown();
        (state.clipboard.take(), state.subscription.take())
    });
    if let (Some(clipboard), Some(subscription)) = (clipboard, subscription) {
        clipboard.disconnect(subscription);
    }
}

//! Read AppKit button frames only during the explicit packaged-application smoke test.
use std::{ffi::c_void, sync::mpsc, time::Duration};

#[derive(Clone, Copy)]
#[repr(C)]
struct Rect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

#[link(name = "objc")]
extern "C" {
    fn sel_registerName(name: *const std::ffi::c_char) -> *mut c_void;
    fn objc_msgSend();
    #[cfg(target_arch = "x86_64")]
    fn objc_msgSend_stret();
}

unsafe fn object(receiver: *mut c_void, selector: &std::ffi::CStr) -> *mut c_void {
    let send: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
        std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
    send(receiver, sel_registerName(selector.as_ptr()))
}

unsafe fn bounds(receiver: *mut c_void) -> Rect {
    let selector = sel_registerName(c"bounds".as_ptr());
    #[cfg(target_arch = "aarch64")]
    {
        let send: unsafe extern "C" fn(*mut c_void, *mut c_void) -> Rect =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        send(receiver, selector)
    }
    #[cfg(target_arch = "x86_64")]
    {
        let send: unsafe extern "C" fn(*mut Rect, *mut c_void, *mut c_void) =
            std::mem::transmute(objc_msgSend_stret as unsafe extern "C" fn());
        let mut result = std::mem::MaybeUninit::uninit();
        send(result.as_mut_ptr(), receiver, selector);
        result.assume_init()
    }
}

unsafe fn window_rect(button: *mut c_void) -> Rect {
    let selector = sel_registerName(c"convertRect:toView:".as_ptr());
    let rect = bounds(button);
    #[cfg(target_arch = "aarch64")]
    {
        let send: unsafe extern "C" fn(*mut c_void, *mut c_void, Rect, *mut c_void) -> Rect =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        send(button, selector, rect, std::ptr::null_mut())
    }
    #[cfg(target_arch = "x86_64")]
    {
        let send: unsafe extern "C" fn(*mut Rect, *mut c_void, *mut c_void, Rect, *mut c_void) =
            std::mem::transmute(objc_msgSend_stret as unsafe extern "C" fn());
        let mut result = std::mem::MaybeUninit::uninit();
        send(
            result.as_mut_ptr(),
            button,
            selector,
            rect,
            std::ptr::null_mut(),
        );
        result.assume_init()
    }
}

pub(super) fn verify(window: &tauri::WebviewWindow) -> Result<(), String> {
    let label = window.label().to_owned();
    let (send, receive) = mpsc::channel();
    window
        .with_webview(move |webview| {
            // AppKit must be read on the UI thread; with_webview dispatches there.
            let result = (|| unsafe {
                let native = webview.ns_window();
                let height = bounds(object(native, c"contentView")).height;
                let button: unsafe extern "C" fn(*mut c_void, *mut c_void, isize) -> *mut c_void =
                    std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
                for kind in 0..3 {
                    let control = button(
                        native,
                        sel_registerName(c"standardWindowButton:".as_ptr()),
                        kind,
                    );
                    if control.is_null() {
                        return Err(format!("{label}: macOS 原生窗口按钮 {kind} 不存在"));
                    }
                    let rect = window_rect(control);
                    let center = height - rect.y - rect.height / 2.;
                    if !center.is_finite() || (center - 22.).abs() > 1. {
                        return Err(format!(
                            "{label}: macOS 按钮 {kind} 中心距顶部 {center:.2}pt，应为 22pt"
                        ));
                    }
                    if rect.x < 0. || rect.x + rect.width > 86. {
                        return Err(format!("{label}: macOS 按钮 {kind} 超出标题栏左侧留位"));
                    }
                    println!(
                        "{label}: macOS button {kind} center={center:.2}pt, x={:.2}pt",
                        rect.x
                    );
                }
                Ok(())
            })();
            let _ = send.send(result);
        })
        .map_err(|e| e.to_string())?;
    receive
        .recv_timeout(Duration::from_secs(5))
        .map_err(|e| format!("无法验证 macOS 原生窗口按钮：{e}"))?
}

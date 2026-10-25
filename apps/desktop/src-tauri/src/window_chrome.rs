//! Windows owns resizing, caption dragging, the system menu and Snap hit tests.
//! Shell reports are in client logical coordinates; hit tests use the HWND DPI.
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};
use tauri::WebviewWindow;

#[derive(Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
impl Rect {
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.width && y < self.y + self.height
    }
    fn valid(&self) -> bool {
        [self.x, self.y, self.width, self.height]
            .iter()
            .all(|v| v.is_finite() && v.abs() < 100000.)
            && self.width >= 0.
            && self.height >= 0.
    }
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Tab {
    pub workspace_id: String,
    #[serde(flatten)]
    pub rect: Rect,
}
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Report {
    pub titlebar: Rect,
    pub drag: Rect,
    pub maximize: Rect,
    pub tab_strip: Option<Rect>,
    pub tabs: Vec<Tab>,
}
impl Report {
    pub fn valid(&self) -> bool {
        self.tabs.len() <= 100
            && self.titlebar.valid()
            && self.drag.valid()
            && self.maximize.valid()
            && self.tab_strip.is_none_or(|r| r.valid())
            && self
                .tabs
                .iter()
                .all(|t| !t.workspace_id.is_empty() && t.rect.valid())
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use windows::Win32::{
        Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        UI::{
            HiDpi::GetDpiForWindow,
            Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::*,
        },
    };
    fn reports() -> &'static Mutex<HashMap<usize, Report>> {
        static REPORTS: OnceLock<Mutex<HashMap<usize, Report>>> = OnceLock::new();
        REPORTS.get_or_init(|| Mutex::new(HashMap::new()))
    }
    fn children() -> &'static Mutex<HashMap<usize, (isize, usize)>> {
        static CHILDREN: OnceLock<Mutex<HashMap<usize, (isize, usize)>>> = OnceLock::new();
        CHILDREN.get_or_init(|| Mutex::new(HashMap::new()))
    }
    fn pressed() -> &'static Mutex<std::collections::HashSet<usize>> {
        static PRESSED: OnceLock<Mutex<std::collections::HashSet<usize>>> = OnceLock::new();
        PRESSED.get_or_init(|| Mutex::new(Default::default()))
    }
    const HOOK: usize = 0x534f4445;
    fn point(lparam: LPARAM) -> POINT {
        POINT {
            x: (lparam.0 as u16 as i16) as i32,
            y: ((lparam.0 >> 16) as u16 as i16) as i32,
        }
    }
    unsafe extern "system" fn procedure(
        hwnd: HWND,
        message: u32,
        wp: WPARAM,
        lp: LPARAM,
        _: usize,
        root: usize,
    ) -> LRESULT {
        let parent = HWND(root as *mut _);
        if message == WM_NCLBUTTONDOWN && wp.0 == HTMAXBUTTON as usize {
            pressed().lock().unwrap().insert(root);
            windows::Win32::UI::Input::KeyboardAndMouse::SetCapture(parent);
            return LRESULT(0);
        }
        if matches!(message, WM_LBUTTONUP | WM_NCLBUTTONUP) {
            let was_pressed = pressed().lock().unwrap().remove(&root);
            if was_pressed {
                let _ = windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture();
                let mut p = POINT::default();
                let _ = GetCursorPos(&mut p);
                let _ = windows::Win32::Graphics::Gdi::ScreenToClient(parent, &mut p);
                let scale = GetDpiForWindow(parent).max(96) as f64 / 96.;
                let inside =
                    reports().lock().unwrap().get(&root).is_some_and(|r| {
                        r.maximize.contains(p.x as f64 / scale, p.y as f64 / scale)
                    });
                if inside {
                    let _ = ShowWindow(
                        parent,
                        if IsZoomed(parent).as_bool() {
                            SW_RESTORE
                        } else {
                            SW_MAXIMIZE
                        },
                    );
                }
                return LRESULT(0);
            }
        }
        if matches!(message, WM_CAPTURECHANGED | WM_CANCELMODE | WM_NCDESTROY) {
            pressed().lock().unwrap().remove(&root);
        }
        if message == WM_NCDESTROY {
            let _ = RemoveWindowSubclass(hwnd, Some(procedure), HOOK);
            if hwnd == parent {
                reports().lock().unwrap().remove(&root);
            }
            return DefSubclassProc(hwnd, message, wp, lp);
        }
        if message == WM_NCHITTEST {
            let report = reports().lock().unwrap().get(&root).cloned();
            if let Some(report) = report {
                let mut client = point(lp);
                let _ = windows::Win32::Graphics::Gdi::ScreenToClient(parent, &mut client);
                let scale = GetDpiForWindow(parent).max(96) as f64 / 96.;
                let x = client.x as f64 / scale;
                let y = client.y as f64 / scale;
                let mut size = RECT::default();
                let _ = GetClientRect(parent, &mut size);
                let edge = 6.;
                let width = size.right as f64 / scale;
                let height = size.bottom as f64 / scale;
                let hit = if !IsZoomed(parent).as_bool()
                    && (x < edge || y < edge || x >= width - edge || y >= height - edge)
                {
                    if y < edge {
                        if x < edge {
                            HTTOPLEFT
                        } else if x >= width - edge {
                            HTTOPRIGHT
                        } else {
                            HTTOP
                        }
                    } else if y >= height - edge {
                        if x < edge {
                            HTBOTTOMLEFT
                        } else if x >= width - edge {
                            HTBOTTOMRIGHT
                        } else {
                            HTBOTTOM
                        }
                    } else if x < edge {
                        HTLEFT
                    } else {
                        HTRIGHT
                    }
                } else if report.maximize.contains(x, y) {
                    HTMAXBUTTON
                } else if report.drag.contains(x, y) {
                    HTCAPTION
                } else {
                    HTCLIENT
                };
                if hit != HTCLIENT {
                    return LRESULT(if hwnd == parent {
                        hit as isize
                    } else {
                        HTTRANSPARENT as i32 as isize
                    });
                }
            }
        }
        DefSubclassProc(hwnd, message, wp, lp)
    }
    unsafe extern "system" fn child_procedure(
        hwnd: HWND,
        message: u32,
        wp: WPARAM,
        lp: LPARAM,
    ) -> LRESULT {
        let entry = children().lock().unwrap().get(&(hwnd.0 as usize)).copied();
        let Some((previous, root)) = entry else {
            return DefWindowProcW(hwnd, message, wp, lp);
        };
        let parent = HWND(root as *mut _);
        if message == WM_NCHITTEST {
            let hit = SendMessageW(parent, message, Some(wp), Some(lp));
            if hit.0 != HTCLIENT as isize {
                return hit;
            }
        }
        // WebView2 input HWNDs can belong to another thread. HTTRANSPARENT
        // only walks windows on the same thread, so forward native chrome
        // messages to the owning top-level HWND instead.
        if matches!(
            message,
            WM_NCMOUSEMOVE
                | WM_NCMOUSELEAVE
                | WM_NCLBUTTONDOWN
                | WM_NCLBUTTONUP
                | WM_NCLBUTTONDBLCLK
                | WM_NCRBUTTONDOWN
                | WM_NCRBUTTONUP
        ) {
            return SendMessageW(parent, message, Some(wp), Some(lp));
        }
        if message == WM_NCDESTROY {
            // Only remove our procedure if it is still the head of the chain.
            // A later subclass owns its own teardown and must not be unlinked.
            if GetWindowLongPtrW(hwnd, GWLP_WNDPROC) == child_procedure as *const () as isize {
                SetWindowLongPtrW(hwnd, GWLP_WNDPROC, previous);
            }
            let result = CallWindowProcW(
                Some(std::mem::transmute::<
                    isize,
                    unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT,
                >(previous)),
                hwnd,
                message,
                wp,
                lp,
            );
            children().lock().unwrap().remove(&(hwnd.0 as usize));
            return result;
        }
        CallWindowProcW(
            Some(std::mem::transmute::<
                isize,
                unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT,
            >(previous)),
            hwnd,
            message,
            wp,
            lp,
        )
    }
    unsafe extern "system" fn child_subclass(
        hwnd: HWND,
        message: u32,
        wp: WPARAM,
        lp: LPARAM,
        _: usize,
        root: usize,
    ) -> LRESULT {
        let parent = HWND(root as *mut _);
        if message == WM_NCHITTEST {
            let hit = SendMessageW(parent, message, Some(wp), Some(lp));
            if hit.0 != HTCLIENT as isize {
                return hit;
            }
        }
        if matches!(
            message,
            WM_NCMOUSEMOVE
                | WM_NCMOUSELEAVE
                | WM_NCLBUTTONDOWN
                | WM_NCLBUTTONUP
                | WM_NCLBUTTONDBLCLK
                | WM_NCRBUTTONDOWN
                | WM_NCRBUTTONUP
        ) {
            return SendMessageW(parent, message, Some(wp), Some(lp));
        }
        if message == WM_NCDESTROY {
            let _ = RemoveWindowSubclass(hwnd, Some(child_subclass), HOOK);
        }
        DefSubclassProc(hwnd, message, wp, lp)
    }
    unsafe extern "system" fn child(hwnd: HWND, param: LPARAM) -> windows::core::BOOL {
        let mut process = 0;
        let thread = GetWindowThreadProcessId(hwnd, Some(&mut process));
        let mut root_process = 0;
        let root_thread =
            GetWindowThreadProcessId(HWND(param.0 as *mut _), Some(&mut root_process));
        if process != root_process {
            return windows::core::BOOL(1);
        }
        // Wry uses comctl32 subclasses with owned COM state. Keep that chain
        // intact on its UI thread, including when Wry removes its last hook.
        if thread == root_thread {
            let _ = SetWindowSubclass(hwnd, Some(child_subclass), HOOK, param.0 as usize);
            return windows::core::BOOL(1);
        }
        let mut entries = children().lock().unwrap();
        if !entries.contains_key(&(hwnd.0 as usize)) {
            let previous =
                SetWindowLongPtrW(hwnd, GWLP_WNDPROC, child_procedure as *const () as isize);
            if previous != 0 {
                entries.insert(hwnd.0 as usize, (previous, param.0 as usize));
            }
        }
        windows::core::BOOL(1)
    }
    pub fn update(window: &WebviewWindow, report: Report) -> Result<(), String> {
        let hwnd = window.hwnd().map_err(|e| e.to_string())?.0 as usize;
        reports().lock().unwrap().insert(hwnd, report);
        window
            .run_on_main_thread(move || unsafe {
                let root = HWND(hwnd as *mut _);
                let _ = SetWindowSubclass(root, Some(procedure), HOOK, hwnd);
                let _ = EnumChildWindows(Some(root), Some(child), LPARAM(hwnd as isize));
            })
            .map_err(|e| e.to_string())
    }
    pub fn system_menu(window: &WebviewWindow) -> Result<(), String> {
        let hwnd = window.hwnd().map_err(|e| e.to_string())?.0 as usize;
        window
            .run_on_main_thread(move || unsafe {
                let hwnd = HWND(hwnd as *mut _);
                let mut p = POINT::default();
                let _ = GetCursorPos(&mut p);
                let menu = GetSystemMenu(hwnd, false);
                let result = TrackPopupMenu(
                    menu,
                    TPM_RETURNCMD | TPM_RIGHTBUTTON,
                    p.x,
                    p.y,
                    None,
                    hwnd,
                    None,
                );
                if result.0 != 0 {
                    let _ = PostMessageW(
                        Some(hwnd),
                        WM_SYSCOMMAND,
                        WPARAM(result.0 as usize),
                        LPARAM(0),
                    );
                }
            })
            .map_err(|e| e.to_string())
    }
}
#[cfg(windows)]
pub use platform::{system_menu, update};
#[cfg(not(windows))]
pub fn update(_: &WebviewWindow, _: Report) -> Result<(), String> {
    Ok(())
}
#[cfg(not(windows))]
pub fn system_menu(_: &WebviewWindow) -> Result<(), String> {
    Err("此平台未实现系统菜单".into())
}

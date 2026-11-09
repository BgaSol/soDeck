//! Windows native pointer driver for explicit tests; never shipped in a product package.
#[cfg(windows)]
mod native {
    use serde::Deserialize;
    use serde_json::json;
    use std::{thread, time::Duration};
    use windows_sys::Win32::{
        Foundation::{HWND, LPARAM, POINT, RECT},
        Graphics::Gdi::{
            ClientToScreen, GetMonitorInfoW, MonitorFromWindow, MONITORINFO,
            MONITOR_DEFAULTTONEAREST,
        },
        System::Threading::{AttachThreadInput, GetCurrentThreadId},
        UI::{
            HiDpi::{
                GetDpiForWindow, SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT,
                DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
            },
            Input::KeyboardAndMouse::{mouse_event, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP},
            WindowsAndMessaging::{
                BringWindowToTop, EnumWindows, GetClientRect, GetCursorPos, GetForegroundWindow,
                GetWindow, GetWindowRect, GetWindowThreadProcessId, IsWindow, IsWindowVisible,
                MoveWindow, SetCursorPos, SetForegroundWindow, ShowWindow, GW_OWNER, SW_RESTORE,
            },
        },
    };

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Point {
        x: f64,
        y: f64,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Size {
        width: f64,
        height: f64,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Drag {
        from: Point,
        to: Point,
    }
    fn coordinate(value: f64) -> Result<i32, String> {
        if !value.is_finite() || value < i32::MIN as f64 || value > i32::MAX as f64 {
            return Err("Pointer coordinate is invalid".into());
        }
        Ok(value.round() as i32)
    }
    fn checked(result: i32, message: &str) -> Result<(), String> {
        if result == 0 {
            Err(format!("{message}: {}", std::io::Error::last_os_error()))
        } else {
            Ok(())
        }
    }
    fn pause(milliseconds: u64) {
        thread::sleep(Duration::from_millis(milliseconds));
    }

    struct Search {
        pid: u32,
        window: HWND,
    }
    unsafe extern "system" fn find(window: HWND, data: LPARAM) -> i32 {
        // EnumWindows borrows this Search for the duration of the synchronous call.
        let search = unsafe { &mut *(data as *mut Search) };
        let mut pid = 0;
        unsafe {
            GetWindowThreadProcessId(window, &mut pid);
        }
        if pid == search.pid
            && unsafe { IsWindowVisible(window) } != 0
            && unsafe { GetWindow(window, GW_OWNER) }.is_null()
        {
            search.window = window;
            return 0;
        }
        1
    }
    fn owned(window: HWND, pid: u32) -> Result<(), String> {
        let mut owner = 0;
        unsafe {
            GetWindowThreadProcessId(window, &mut owner);
        }
        if window.is_null() || owner != pid || unsafe { IsWindow(window) } == 0 {
            Err("Test process has no owned main window".into())
        } else {
            Ok(())
        }
    }
    struct Dpi(DPI_AWARENESS_CONTEXT);
    impl Drop for Dpi {
        fn drop(&mut self) {
            unsafe {
                SetThreadDpiAwarenessContext(self.0);
            }
        }
    }
    struct Input {
        from: u32,
        to: u32,
        attached: bool,
    }
    impl Drop for Input {
        fn drop(&mut self) {
            if self.attached {
                unsafe {
                    AttachThreadInput(self.from, self.to, 0);
                }
            }
        }
    }
    struct Pointer {
        saved: POINT,
        pressed: bool,
    }
    impl Drop for Pointer {
        fn drop(&mut self) {
            unsafe {
                if self.pressed {
                    mouse_event(MOUSEEVENTF_LEFTUP, 0, 0, 0, 0);
                }
                SetCursorPos(self.saved.x, self.saved.y);
            }
        }
    }

    pub fn run() -> Result<(), String> {
        let args: Vec<String> = std::env::args().skip(1).collect();
        if args.len() != 3 {
            return Err("Provide process ID, arrange/drag, and JSON plan".into());
        }
        let pid: u32 = args[0].parse().map_err(|_| "Invalid process ID")?;
        if pid == 0 {
            return Err("Invalid process ID".into());
        }
        // Validate the entire plan before touching the desktop.
        let size = if args[1] == "arrange" {
            let size: Size = serde_json::from_str(&args[2]).map_err(|e| e.to_string())?;
            if !size.width.is_finite()
                || !size.height.is_finite()
                || size.width <= 0.
                || size.height <= 0.
            {
                return Err("Invalid window dimensions".into());
            }
            Some(size)
        } else {
            None
        };
        let drag = if args[1] == "drag" {
            let drag: Drag = serde_json::from_str(&args[2]).map_err(|e| e.to_string())?;
            for point in [&drag.from, &drag.to] {
                coordinate(point.x)?;
                coordinate(point.y)?;
            }
            Some(drag)
        } else {
            None
        };
        if size.is_none() && drag.is_none() {
            return Err("Action must be arrange or drag".into());
        }

        let old_dpi =
            unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
        if old_dpi.is_null() {
            return Err("Cannot set test DPI context".into());
        }
        let _dpi = Dpi(old_dpi);
        let mut search = Search {
            pid,
            window: std::ptr::null_mut(),
        };
        unsafe {
            EnumWindows(Some(find), &mut search as *mut Search as LPARAM);
        }
        let window = search.window;
        owned(window, pid)?;
        if let Some(size) = size {
            let mut monitor: MONITORINFO = unsafe { std::mem::zeroed() };
            monitor.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
            checked(
                unsafe {
                    GetMonitorInfoW(
                        MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST),
                        &mut monitor,
                    )
                },
                "Cannot read test monitor",
            )?;
            unsafe {
                ShowWindow(window, SW_RESTORE);
            }
            let dpi = unsafe { GetDpiForWindow(window) };
            if dpi == 0 {
                return Err("Cannot read test window DPI".into());
            }
            let scale = dpi as f64 / 96.;
            let width = coordinate(
                (size.width * scale).min((monitor.rcWork.right - monitor.rcWork.left - 180) as f64),
            )?;
            let height = coordinate(
                (size.height * scale).min((monitor.rcWork.bottom - monitor.rcWork.top - 80) as f64),
            )?;
            if width <= 0 || height <= 0 {
                return Err("Monitor work area is too small".into());
            }
            checked(
                unsafe {
                    MoveWindow(
                        window,
                        monitor.rcWork.left + 40,
                        monitor.rcWork.top + 40,
                        width,
                        height,
                        1,
                    )
                },
                "Cannot position test window",
            )?;
            unsafe {
                SetForegroundWindow(window);
            }
            pause(250);
            let mut client: RECT = unsafe { std::mem::zeroed() };
            let mut outer: RECT = unsafe { std::mem::zeroed() };
            let mut origin = POINT { x: 0, y: 0 };
            checked(
                unsafe { GetClientRect(window, &mut client) },
                "Cannot read client rectangle",
            )?;
            checked(
                unsafe { ClientToScreen(window, &mut origin) },
                "Cannot read client origin",
            )?;
            checked(
                unsafe { GetWindowRect(window, &mut outer) },
                "Cannot read window rectangle",
            )?;
            println!(
                "{}",
                json!({"x":origin.x,"y":origin.y,"width":client.right,"height":client.bottom,"right":outer.right,"workRight":monitor.rcWork.right,"scale":scale})
            );
        } else if let Some(drag) = drag {
            let thread =
                unsafe { GetWindowThreadProcessId(GetForegroundWindow(), std::ptr::null_mut()) };
            let helper = unsafe { GetCurrentThreadId() };
            {
                let _input = Input {
                    from: helper,
                    to: thread,
                    attached: thread != helper
                        && unsafe { AttachThreadInput(helper, thread, 1) } != 0,
                };
                unsafe {
                    BringWindowToTop(window);
                    SetForegroundWindow(window);
                }
            }
            let mut owner = 0;
            unsafe {
                GetWindowThreadProcessId(GetForegroundWindow(), &mut owner);
            }
            if owner != pid {
                return Err("Test window could not receive native pointer input".into());
            }
            let mut saved = POINT { x: 0, y: 0 };
            checked(unsafe { GetCursorPos(&mut saved) }, "Cannot save pointer")?;
            let mut pointer = Pointer {
                saved,
                pressed: false,
            };
            checked(
                unsafe { SetCursorPos(coordinate(drag.from.x)?, coordinate(drag.from.y)?) },
                "Cannot position pointer",
            )?;
            pause(100);
            unsafe {
                mouse_event(MOUSEEVENTF_LEFTDOWN, 0, 0, 0, 0);
            }
            pointer.pressed = true;
            pause(150);
            for step in 1..=20 {
                owned(window, pid)?;
                let x = coordinate(drag.from.x + (drag.to.x - drag.from.x) * step as f64 / 20.)?;
                let y = coordinate(drag.from.y + (drag.to.y - drag.from.y) * step as f64 / 20.)?;
                checked(unsafe { SetCursorPos(x, y) }, "Cannot move pointer")?;
                pause(40);
            }
            pause(300);
        }
        Ok(())
    }
}

fn main() {
    #[cfg(windows)]
    let result = native::run();
    #[cfg(not(windows))]
    let result: Result<(), String> = Err("Native pointer fixture requires Windows".into());
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

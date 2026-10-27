use super::*;

impl State {
    pub(super) fn window_service(
        self: &Arc<Self>,
        source: Option<&str>,
        method: &str,
        params: &Value,
    ) -> Option<Result<Value>> {
        if !matches!(method, "windows.chrome" | "workspaces.drag") {
            return None;
        }
        Some((|| {
            let window = source
                .and_then(|s| s.strip_prefix("@window:"))
                .ok_or_else(|| err("SOURCE_INVALID", "窗口布局须来自原生外壳"))?;
            if method == "windows.chrome" {
                let report: crate::window_chrome::Report =
                    serde_json::from_value(params.clone())
                        .map_err(|e| err("INVALID_PARAMS", format!("窗口命中区域无效：{e}")))?;
                if !report.valid() {
                    return Err(err("INVALID_PARAMS", "窗口命中区域超出范围"));
                }
                let win = self
                    .app
                    .get_webview_window(window)
                    .ok_or_else(|| err("WINDOW_CLOSED", "窗口已关闭"))?;
                crate::window_chrome::update(&win, report.clone()).map_err(native_error)?;
                self.chrome_reports
                    .lock()
                    .unwrap()
                    .insert(window.into(), report);
                Ok(Value::Null)
            } else {
                self.drag_workspace(
                    window,
                    required(params, "workspaceId")?,
                    params["operationId"].as_str(),
                )
            }
        })())
    }
    #[cfg(windows)]
    fn drag_workspace(
        self: &Arc<Self>,
        source: &str,
        id: &str,
        operation: Option<&str>,
    ) -> Result<Value> {
        use windows::Win32::{
            Foundation::POINT,
            UI::{
                Input::KeyboardAndMouse::{GetAsyncKeyState, VK_ESCAPE, VK_LBUTTON},
                WindowsAndMessaging::{
                    GetAncestor, GetCursorPos, GetForegroundWindow, WindowFromPoint, GA_ROOT,
                },
            },
        };
        if self
            .workbench
            .lock()
            .unwrap()
            .owner(id)
            .map_err(native_error)?
            != source
        {
            return Err(err("SOURCE_INVALID", "工作区不属于拖动窗口"));
        }
        if self.dragging.swap(true, Ordering::SeqCst) {
            return Err(err("MOVE_IN_PROGRESS", "另一个拖动仍在进行"));
        }
        struct Reset<'a>(&'a AtomicBool);
        impl Drop for Reset<'_> {
            fn drop(&mut self) {
                self.0.store(false, Ordering::SeqCst);
            }
        }
        let _reset = Reset(&self.dragging);
        let end = Instant::now() + Duration::from_secs(30);
        let mut previous = Value::Null;
        let result = (|| loop {
            if self.stopping.load(Ordering::SeqCst) || self.app.get_webview_window(source).is_none()
            {
                return Err(err("CANCELED", "源窗口已经关闭"));
            }
            let source_hwnd = self
                .app
                .get_webview_window(source)
                .and_then(|w| w.hwnd().ok());
            if source_hwnd != Some(unsafe { GetForegroundWindow() })
                || operation.is_some_and(|id| {
                    self.pending_opens
                        .lock()
                        .unwrap()
                        .get(id)
                        .is_some_and(|op| op.canceled)
                })
            {
                return Ok(json!({"canceled":true}));
            }
            if unsafe { GetAsyncKeyState(VK_ESCAPE.0 as i32) } < 0 || Instant::now() >= end {
                return Ok(json!({"canceled":true}));
            }
            let mut p = POINT::default();
            unsafe {
                GetCursorPos(&mut p).map_err(|e| native_error(e.to_string()))?;
            }
            let root = unsafe { GetAncestor(WindowFromPoint(p), GA_ROOT) };
            let mut target = None;
            let mut inside = false;
            for (label, window) in self.app.webview_windows() {
                if !window.is_visible().unwrap_or(false) || window.is_minimized().unwrap_or(true) {
                    continue;
                }
                let Ok(hwnd) = window.hwnd() else {
                    continue;
                };
                if hwnd != root {
                    continue;
                }
                inside = true;
                let Ok(position) = window.inner_position() else {
                    continue;
                };
                let scale = window.scale_factor().unwrap_or(1.);
                let x = (p.x - position.x) as f64 / scale;
                let y = (p.y - position.y) as f64 / scale;
                if let Some(report) = self.chrome_reports.lock().unwrap().get(&label) {
                    if report.tab_strip.is_some_and(|r| r.contains(x, y)) {
                        let index = report
                            .tabs
                            .iter()
                            .position(|t| x < t.rect.x + t.rect.width / 2.)
                            .unwrap_or(report.tabs.len());
                        target = Some((label, index));
                    }
                }
                break;
            }
            let preview = match &target {
                Some((window, index)) => json!({"windowId":window,"index":index}),
                None => json!({"newWindow":!inside}),
            };
            if preview != previous {
                self.drag_preview(preview.clone());
                previous = preview;
            }
            if unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } >= 0 {
                if let Some((window, index)) = target {
                    if window == source {
                        let mut w = self.workbench.lock().unwrap();
                        let mut ids = w
                            .windows
                            .get(source)
                            .ok_or_else(|| err("WINDOW_CLOSED", "窗口已关闭"))?
                            .workspace_ids
                            .clone();
                        let old = ids
                            .iter()
                            .position(|value| value == id)
                            .ok_or_else(|| err("WORKSPACE_CLOSED", "工作区已关闭"))?;
                        ids.remove(old);
                        let at = index
                            .saturating_sub(usize::from(index > old))
                            .min(ids.len());
                        ids.insert(at, id.into());
                        w.reorder(source, ids).map_err(native_error)?;
                        drop(w);
                        return Ok(self.commit_runtime(
                            json!({"reordered":true})));
                    }
                    return self.move_workspace_batch(
                        vec![id.into()],
                        &window,
                        index,
                        Some(id),
                        operation,
                        json!({"drag":id,"target":window,"index":index}),
                    );
                }
                if inside {
                    return Ok(json!({"canceled":true,"reason":"invalidDropTarget"}));
                }
                let target = format!("workspace-{}", Uuid::new_v4());
                let result = self.move_workspace_batch(
                    vec![id.into()],
                    &target,
                    0,
                    Some(id),
                    operation,
                    json!({"drag":id,"target":target}),
                )?;
                if let Some(window) = self.app.get_webview_window(&target) {
                    let _ = window.set_position(tauri::PhysicalPosition::new(p.x - 80, p.y - 20));
                }
                return Ok(result);
            }
            std::thread::sleep(Duration::from_millis(16));
        })();
        self.drag_preview(Value::Null);
        result
    }
    #[cfg(not(windows))]
    fn drag_workspace(self: &Arc<Self>, _: &str, _: &str, _: Option<&str>) -> Result<Value> {
        Err(err("UNSUPPORTED_PLATFORM", "本批仅验证 Windows 拖放"))
    }
    fn drag_preview(&self, payload: Value) {
        let event = json!({"type":"workbench.dragPreview","payload":payload});
        for window in self.app.webview_windows().values() {
            let _ = window.eval(&format!("window.__sodeckNotify?.({event});"));
        }
    }
    pub(super) fn remember_window(&self, id: &str) {
        let Some(window) = self.app.get_webview_window(id) else {
            return;
        };
        if window.is_minimized().unwrap_or(true) {
            return;
        }
        let maximized = window.is_maximized().unwrap_or(false);
        let (Ok(position), Ok(size)) = (window.outer_position(), window.inner_size()) else {
            return;
        };
        let mut w = self.workbench.lock().unwrap();
        if let Some(record) = w.windows.get_mut(id) {
            if maximized {
                if let Some(bounds) = &mut record.bounds {
                    bounds.maximized = true;
                }
            } else {
                record.bounds = Some(crate::window_registry::WindowBounds {
                    x: position.x,
                    y: position.y,
                    width: size.width,
                    height: size.height,
                    maximized,
                });
            }
        }
        if id == "main" {
            if let Err(error) = crate::window_preferences::save(&self.data, w.windows.get("main").and_then(|w| w.bounds.clone())) {
                let message = format!("窗口偏好未保存：{error}");
                if !w.recovery_warnings.contains(&message) { w.recovery_warnings.push(message); }
            }
        }
    }
    pub(super) fn restore_window(&self, id: &str) {
        let bounds = self
            .workbench
            .lock()
            .unwrap()
            .windows
            .get(id)
            .and_then(|w| w.bounds.clone());
        let Some(bounds) = bounds else {
            return;
        };
        let Some(window) = self.app.get_webview_window(id) else {
            return;
        };
        let _ = window.set_size(tauri::PhysicalSize::new(
            bounds.width.clamp(760, 10000),
            bounds.height.clamp(480, 10000),
        ));
        let visible = window
            .available_monitors()
            .unwrap_or_default()
            .iter()
            .any(|m| {
                let p = m.position();
                let s = m.size();
                bounds.x + 80 > p.x
                    && bounds.y + 44 > p.y
                    && bounds.x < p.x + s.width as i32 - 80
                    && bounds.y < p.y + s.height as i32 - 44
            });
        if visible {
            let _ = window.set_position(tauri::PhysicalPosition::new(bounds.x, bounds.y));
        } else {
            let _ = window.center();
        }
        if bounds.maximized {
            let _ = window.maximize();
        }
    }
}

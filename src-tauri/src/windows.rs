//! Native window coordination and physical screen-coordinate tab hit testing.
use crate::{
    workspaces::{Layout, Projection},
    AppCore,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager, State, WebviewWindow};

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TabRect {
    pub id: String,
    pub center: f64,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Surface {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub tabs: Vec<TabRect>,
    pub blocked: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DropTarget {
    #[serde(rename_all = "camelCase")]
    Strip {
        window_id: String,
        index: usize,
    },
    Outside {
        x: f64,
        y: f64,
    },
    Invalid,
}
#[derive(Clone)]
struct Drag {
    token: String,
    source: String,
    workspace: String,
    offset: (f64, f64),
}

pub struct DesktopWindows {
    pub core: Arc<AppCore>,
    surfaces: Mutex<HashMap<String, Surface>>,
    focus: Mutex<Vec<String>>,
    drag: Mutex<Option<Drag>>,
}
impl DesktopWindows {
    pub fn accept_session(
        &self,
        handle: &tauri::AppHandle,
        session: crate::workspaces::ConnectedSession,
    ) -> Result<crate::workspaces::Workspace, String> {
        let workspace = self.core.accept_connected(session)?;
        if let Some(window) = handle.get_webview_window(&workspace.window_id) {
            let _ = window.show();
            let _ = window.set_focus();
        }
        Ok(workspace)
    }
    pub fn new(core: Arc<AppCore>) -> Self {
        Self {
            core,
            surfaces: Mutex::new(HashMap::new()),
            focus: Mutex::new(vec!["main".into()]),
            drag: Mutex::new(None),
        }
    }
    pub fn focused(&self, window: &str) {
        let mut focus = self.focus.lock().unwrap();
        focus.retain(|w| w != window);
        focus.push(window.into());
    }
    pub fn remove_surface(&self, window: &str) {
        self.surfaces.lock().unwrap().remove(window);
        self.focus.lock().unwrap().retain(|w| w != window);
        let mut drag = self.drag.lock().unwrap();
        if drag.as_ref().is_some_and(|d| d.source == window) {
            *drag = None;
        }
    }
    pub fn close_secondary(&self, handle: &tauri::AppHandle, window: &str) -> Result<(), String> {
        self.core.workspaces.lock().unwrap().close_window(window)?;
        self.remove_surface(window);
        let _ = handle.emit("workspace-drop-preview", DropTarget::Invalid);
        if let Some(w) = handle.get_webview_window(window) {
            w.destroy().map_err(|e| e.to_string())?;
        }
        self.core.workspace_changed();
        Ok(())
    }
    fn cleanup_empty(&self, handle: &tauri::AppHandle, window: &str) {
        let empty = self.core.workspaces.lock().unwrap().empty_secondary(window);
        if empty {
            let _ = self.close_secondary(handle, window);
        }
    }
    pub fn move_to(
        &self,
        handle: &tauri::AppHandle,
        source: &str,
        id: &str,
        target: Option<String>,
        index: usize,
        position: Option<(f64, f64)>,
    ) -> Result<(), String> {
        if self.core.is_closing() {
            return Err("应用正在退出".into());
        }
        let new_window = target.is_none();
        let target = target.unwrap_or_else(|| format!("workspace-{}", self.core.next_id()));
        let m = {
            let mut state = self.core.workspaces.lock().unwrap();
            if new_window {
                state.register(&target)?;
            }
            match state.prepare(source, id, &target, index, self.core.next_id()) {
                Ok(m) => m,
                Err(e) => {
                    if new_window {
                        let _ = state.close_window(&target);
                    }
                    return Err(e);
                }
            }
        };
        self.core.workspace_changed();
        let result = (|| {
            if new_window {
                let builder = tauri::WebviewWindowBuilder::new(
                    handle,
                    &target,
                    tauri::WebviewUrl::App("index.html".into()),
                )
                .title("SoDeck")
                .inner_size(1100., 720.)
                .min_inner_size(720., 480.)
                .decorations(false)
                .visible(false);
                #[cfg(target_os = "macos")]
                let builder = builder
                    .decorations(true)
                    .hidden_title(true)
                    .title_bar_style(tauri::TitleBarStyle::Overlay);
                let window = builder.build().map_err(|e| format!("无法建立窗口：{e}"))?;
                if let Some((x, y)) = position {
                    // Set physical coordinates after creation: mixed-DPI monitor origins must not be divided by a scale factor.
                    window
                        .set_position(tauri::PhysicalPosition::new(
                            x.round() as i32,
                            y.round() as i32,
                        ))
                        .map_err(|e| e.to_string())?;
                }
            }
            let started = Instant::now();
            loop {
                let state = self.core.workspaces.lock().unwrap();
                if state.is_ready(&target) {
                    break;
                }
                if !state.windows.contains_key(&target) || self.core.is_closing() {
                    return Err("目标窗口已关闭".into());
                }
                if started.elapsed() > Duration::from_secs(8) {
                    return Err("目标窗口未就绪，工作区保留在原窗口".into());
                }
                drop(state);
                std::thread::sleep(Duration::from_millis(20));
            }
            let window = handle.get_webview_window(&target).ok_or("目标窗口已关闭")?;
            window.show().map_err(|e| e.to_string())?;
            self.core.workspaces.lock().unwrap().commit(&m)?;
            self.core.workspace_changed();
            let _ = window.set_focus();
            Ok(())
        })();
        if result.is_err() {
            self.core.workspaces.lock().unwrap().cancel(&m);
            if new_window {
                self.cleanup_empty(handle, &target);
            }
            self.core.workspace_changed();
        } else if source != target {
            self.cleanup_empty(handle, source);
        }
        result
    }
    fn hit(&self, handle: &tauri::AppHandle, drag: &Drag) -> Result<DropTarget, String> {
        let source = handle
            .get_webview_window(&drag.source)
            .ok_or("源窗口已关闭")?;
        let p = source
            .cursor_position()
            .map_err(|_| "当前桌面无法提供全局指针坐标，请使用 Tab 菜单移窗")?;
        // Wayland cannot reliably supply global window coordinates. Never detach on a guessed position.
        #[cfg(target_os = "linux")]
        if std::env::var_os("WAYLAND_DISPLAY").is_some()
            && std::env::var("GDK_BACKEND").as_deref() != Ok("x11")
        {
            return Err("Wayland 不支持此跨窗口拖动，请使用 Tab 菜单移至新窗口或返回主窗口".into());
        }
        self.hit_point(handle, p.x, p.y, &drag.workspace, drag.offset)
    }
    fn hit_point(
        &self,
        handle: &tauri::AppHandle,
        px: f64,
        py: f64,
        workspace: &str,
        offset: (f64, f64),
    ) -> Result<DropTarget, String> {
        // Native queries may synchronously use the event loop. Never keep a
        // registry lock while querying a window whose callbacks update it.
        let surfaces = self.surfaces.lock().unwrap().clone();
        let focus = self.focus.lock().unwrap().clone();
        for label in focus.iter().rev() {
            let Some(w) = handle.get_webview_window(label) else {
                continue;
            };
            if !w.is_visible().unwrap_or(false) || w.is_minimized().unwrap_or(true) {
                continue;
            }
            let (Ok(origin), Ok(size), Ok(scale)) =
                (w.inner_position(), w.inner_size(), w.scale_factor())
            else {
                continue;
            };
            let x = (px - origin.x as f64) / scale;
            let y = (py - origin.y as f64) / scale;
            if x < 0. || y < 0. || x >= size.width as f64 / scale || y >= size.height as f64 / scale
            {
                continue;
            }
            let Some(surface) = surfaces.get(label) else {
                return Ok(DropTarget::Invalid);
            };
            return Ok(hit_surface(label, surface, x, y, workspace));
        }
        Ok(DropTarget::Outside {
            x: px - offset.0,
            y: py - offset.1,
        })
    }
    /// Package smoke check using real window geometry, without moving the user's pointer.
    pub fn verify_native_geometry(
        &self,
        handle: &tauri::AppHandle,
        label: &str,
        workspace: &str,
    ) -> Result<(), String> {
        let window = handle.get_webview_window(label).ok_or("原生窗口不存在")?;
        let origin = window.inner_position().map_err(|e| e.to_string())?;
        let size = window.inner_size().map_err(|e| e.to_string())?;
        let scale = window.scale_factor().map_err(|e| e.to_string())?;
        let surface = self
            .surfaces
            .lock()
            .unwrap()
            .get(label)
            .cloned()
            .ok_or("Tab 区域尚未上报")?;
        self.focused(label);
        let x = origin.x as f64 + (surface.x + surface.width / 2.) * scale;
        let y = origin.y as f64 + (surface.y + surface.height / 2.) * scale;
        if !matches!(self.hit_point(handle, x, y, workspace, (0.,0.))?, DropTarget::Strip { window_id, .. } if window_id == label)
        {
            return Err("原生屏幕坐标没有命中目标 Tab 条".into());
        }
        for (px, py) in [
            (
                origin.x as f64 + 200. * scale,
                origin.y as f64 + 100. * scale,
            ),
            (origin.x as f64 + size.width as f64 - 5., y),
        ] {
            if self.hit_point(handle, px, py, workspace, (0., 0.))? != DropTarget::Invalid {
                return Err("内容区或窗口按钮错误地接受了 Tab 拖放".into());
            }
        }
        Ok(())
    }
}

pub fn hit_surface(label: &str, surface: &Surface, x: f64, y: f64, workspace: &str) -> DropTarget {
    if surface.blocked
        || !x.is_finite()
        || !y.is_finite()
        || x < surface.x
        || y < surface.y
        || x >= surface.x + surface.width
        || y >= surface.y + surface.height
    {
        return DropTarget::Invalid;
    }
    let index = surface
        .tabs
        .iter()
        .filter(|t| t.id != workspace && t.center < x)
        .count();
    DropTarget::Strip {
        window_id: label.into(),
        index,
    }
}

#[tauri::command]
pub fn workspace_snapshot(
    window: WebviewWindow,
    desktop: State<DesktopWindows>,
) -> Result<Projection, String> {
    desktop.core.workspace_projection(window.label())
}
#[tauri::command]
pub fn workspace_ready(
    window: WebviewWindow,
    desktop: State<DesktopWindows>,
) -> Result<(), String> {
    desktop
        .core
        .workspaces
        .lock()
        .unwrap()
        .ready(window.label())?;
    desktop.focused(window.label());
    Ok(())
}
#[tauri::command]
pub fn workspace_surface(
    window: WebviewWindow,
    desktop: State<DesktopWindows>,
    surface: Surface,
) -> Result<(), String> {
    if ![surface.x, surface.y, surface.width, surface.height]
        .iter()
        .all(|v| v.is_finite())
        || surface.width < 0.
        || surface.height < 0.
    {
        return Err("无效 Tab 区域".into());
    }
    desktop
        .surfaces
        .lock()
        .unwrap()
        .insert(window.label().into(), surface);
    Ok(())
}
#[tauri::command]
pub fn workspace_activate(
    window: WebviewWindow,
    desktop: State<DesktopWindows>,
    id: Option<String>,
) -> Result<(), String> {
    desktop
        .core
        .workspaces
        .lock()
        .unwrap()
        .activate(window.label(), id.as_deref())?;
    desktop.core.workspace_changed();
    Ok(())
}
#[tauri::command]
pub fn workspace_layout(
    window: WebviewWindow,
    desktop: State<DesktopWindows>,
    id: String,
    layout: Layout,
) -> Result<(), String> {
    desktop
        .core
        .workspaces
        .lock()
        .unwrap()
        .layout(window.label(), &id, layout)?;
    desktop.core.workspace_changed();
    Ok(())
}
#[tauri::command]
pub async fn workspace_move(
    handle: tauri::AppHandle,
    window: WebviewWindow,
    desktop: State<'_, DesktopWindows>,
    id: String,
    target: Option<String>,
    index: usize,
) -> Result<(), String> {
    let source = window.label().to_owned();
    let _ = desktop; // Release borrowed command state before blocking native setup.
    let task_handle = handle.clone();
    tauri::async_runtime::spawn_blocking(move || {
        task_handle.state::<DesktopWindows>().move_to(
            &task_handle,
            &source,
            &id,
            target,
            index,
            None,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub fn workspace_cancel_move(window: WebviewWindow, desktop: State<DesktopWindows>, id: String) {
    let mut state = desktop.core.workspaces.lock().unwrap();
    if let Some(m) = state.pending_move(&id, window.label()) {
        state.cancel(&m);
    }
    drop(state);
    desktop.core.workspace_changed();
}
#[tauri::command]
pub fn workspace_close(
    handle: tauri::AppHandle,
    window: WebviewWindow,
    desktop: State<DesktopWindows>,
    id: String,
) -> Result<(), String> {
    desktop
        .core
        .workspaces
        .lock()
        .unwrap()
        .close(window.label(), &id)?;
    desktop.core.workspace_changed();
    desktop.cleanup_empty(&handle, window.label());
    Ok(())
}
#[tauri::command]
pub fn workspace_drag_start(
    window: WebviewWindow,
    desktop: State<DesktopWindows>,
    id: String,
    offset_x: f64,
    offset_y: f64,
) -> Result<String, String> {
    let state = desktop.core.workspace_projection(window.label())?;
    if !state
        .workspaces
        .iter()
        .any(|w| w.context.workspace_id == id)
    {
        return Err("工作区已移动或关闭".into());
    }
    let scale = window.scale_factor().map_err(|e| e.to_string())?;
    let mut drag = desktop.drag.lock().unwrap();
    if drag.is_some() {
        return Err("已有 Tab 正在拖动".into());
    }
    let token = desktop.core.next_id();
    *drag = Some(Drag {
        token: token.clone(),
        source: window.label().into(),
        workspace: id,
        offset: (offset_x * scale, offset_y * scale),
    });
    Ok(token)
}
#[tauri::command]
pub fn workspace_drag_sample(
    handle: tauri::AppHandle,
    window: WebviewWindow,
    desktop: State<DesktopWindows>,
    token: String,
) -> Result<DropTarget, String> {
    let drag = desktop
        .drag
        .lock()
        .unwrap()
        .as_ref()
        .filter(|d| d.token == token && d.source == window.label())
        .cloned()
        .ok_or("拖动已取消")?;
    let target = desktop.hit(&handle, &drag)?;
    if !desktop
        .drag
        .lock()
        .unwrap()
        .as_ref()
        .is_some_and(|d| d.token == token)
    {
        return Ok(DropTarget::Invalid);
    }
    let _ = handle.emit("workspace-drop-preview", &target);
    Ok(target)
}
#[tauri::command]
pub async fn workspace_drag_finish(
    handle: tauri::AppHandle,
    window: WebviewWindow,
    desktop: State<'_, DesktopWindows>,
    token: String,
    cancel: bool,
) -> Result<(), String> {
    let (source, id, target) = {
        let mut lock = desktop.drag.lock().unwrap();
        if !lock
            .as_ref()
            .is_some_and(|d| d.token == token && d.source == window.label())
        {
            return Err("拖动已取消".into());
        }
        let drag = lock.take().unwrap();
        drop(lock);
        let target = if cancel {
            Ok(DropTarget::Invalid)
        } else {
            desktop.hit(&handle, &drag)
        };
        (drag.source, drag.workspace, target)
    };
    let _ = handle.emit("workspace-drop-preview", DropTarget::Invalid);
    let target = target?;
    tauri::async_runtime::spawn_blocking(move || {
        let desktop = handle.state::<DesktopWindows>();
        match target {
            DropTarget::Strip { window_id, index } => {
                desktop.move_to(&handle, &source, &id, Some(window_id), index, None)
            }
            DropTarget::Outside { x, y } => {
                desktop.move_to(&handle, &source, &id, None, 0, Some((x, y)))
            }
            DropTarget::Invalid => Ok(()),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

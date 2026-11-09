//! Native window coordination and physical screen-coordinate tab hit testing.
use crate::{workspaces::Projection, AppCore};
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
    #[serde(default)]
    pub groups: Vec<GroupRect>,
    #[serde(default)]
    pub docks: Vec<DockRect>,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DockRect {
    pub workspace_id: String,
    pub side: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupRect {
    pub workspace_id: String,
    pub group_id: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub tabs: Vec<TabRect>,
    #[serde(default)]
    pub workspace_width: f64,
    #[serde(default)]
    pub workspace_height: f64,
}
#[derive(Clone, Deserialize)]
pub struct LocalPoint {
    pub x: f64,
    pub y: f64,
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
    #[serde(rename_all = "camelCase")]
    Group {
        window_id: String,
        group_id: String,
        edge: String,
        index: usize,
    },
    #[serde(rename_all = "camelCase")]
    Dock {
        window_id: String,
        workspace_id: String,
        side: String,
    },
}
#[derive(Clone)]
struct Drag {
    token: String,
    source: String,
    workspace: String,
    offset: (f64, f64),
    scope: crate::workbench_move::Scope,
}

pub struct DesktopWindows {
    pub core: Arc<AppCore>,
    surfaces: Mutex<HashMap<String, Surface>>,
    focus: Mutex<Vec<String>>,
    drag: Mutex<Option<Drag>>,
    native_moves: Mutex<std::collections::HashSet<String>>,
}
impl DesktopWindows {
    fn estimate_bounds(
        &self,
        handle: &tauri::AppHandle,
        source: &str,
        workspace: &str,
        scope: &crate::workbench_move::Scope,
    ) -> Option<crate::workbench_memory::Bounds> {
        let window = handle.get_webview_window(source)?;
        let scale = window.scale_factor().ok()?;
        let size = window.inner_size().ok()?.to_logical::<f64>(scale);
        let origin = window.outer_position().ok()?;
        let mut width = size.width;
        let mut height = size.height;
        if !matches!(scope, crate::workbench_move::Scope::Host) {
            let ui = self.core.ui_state();
            let host = ui.workspaces.get(workspace)?.host(source).ok()?;
            let group = match scope {
                crate::workbench_move::Scope::Group { id } => Some(id.clone()),
                crate::workbench_move::Scope::View { id } => host.group_of(id),
                _ => None,
            };
            drop(ui);
            let surfaces = self.surfaces.lock().unwrap();
            if let Some(rect) = surfaces
                .get(source)?
                .groups
                .iter()
                .find(|g| g.workspace_id == workspace && Some(&g.group_id) == group.as_ref())
            {
                width = rect.width + 84.;
                height = rect.height + 72.;
            }
        }
        Some(crate::workbench_memory::Bounds {
            x: origin.x + (40. * scale) as i32,
            y: origin.y + (40. * scale) as i32,
            width: width.max(720.),
            height: height.max(480.),
        })
    }
    fn create_display(
        &self,
        handle: &tauri::AppHandle,
        target: &str,
        bounds: Option<&crate::workbench_memory::Bounds>,
        position: Option<(f64, f64)>,
    ) -> Result<WebviewWindow, String> {
        let builder = tauri::WebviewWindowBuilder::new(
            handle,
            target,
            tauri::WebviewUrl::App("index.html".into()),
        )
        .title("SoDeck")
        .inner_size(
            bounds.map_or(1100., |b| b.width),
            bounds.map_or(720., |b| b.height),
        )
        .min_inner_size(720., 480.)
        .decorations(false)
        .devtools(false)
        .visible(false);
        #[cfg(target_os = "macos")]
        let builder = builder
            .decorations(true)
            .hidden_title(true)
            .title_bar_style(tauri::TitleBarStyle::Overlay)
            .traffic_light_position({
                let p = handle.config().app.windows[0]
                    .traffic_light_position
                    .as_ref()
                    .ok_or("macOS 窗口按钮位置尚未配置")?;
                tauri::LogicalPosition::new(p.x, p.y)
            });
        let window = builder.build().map_err(|e| format!("无法建立窗口：{e}"))?;
        if let Some((mut x, mut y)) = position.or_else(|| bounds.map(|b| (b.x as f64, b.y as f64)))
        {
            if bounds.is_some() {
                let monitors = window.available_monitors().map_err(|e| e.to_string())?;
                let monitor = monitors
                    .iter()
                    .find(|m| {
                        x >= m.position().x as f64
                            && x < (m.position().x as f64 + m.size().width as f64)
                            && y >= m.position().y as f64
                            && y < (m.position().y as f64 + m.size().height as f64)
                    })
                    .or_else(|| monitors.first());
                if let Some(m) = monitor {
                    let b = bounds.unwrap();
                    let area = m.work_area();
                    let left = area.position.x as f64;
                    let top = area.position.y as f64;
                    window
                        .set_size(tauri::LogicalSize::new(
                            b.width.min(area.size.width as f64 / m.scale_factor()),
                            b.height.min(area.size.height as f64 / m.scale_factor()),
                        ))
                        .map_err(|e| e.to_string())?;
                    x = x.clamp(
                        left,
                        left + (area.size.width as f64 - b.width * m.scale_factor()).max(0.),
                    );
                    y = y.clamp(
                        top,
                        top + (area.size.height as f64 - b.height * m.scale_factor()).max(0.),
                    );
                }
            }
            window
                .set_position(tauri::PhysicalPosition::new(
                    x.round() as i32,
                    y.round() as i32,
                ))
                .map_err(|e| e.to_string())?;
        }
        Ok(window)
    }
    pub fn record_bounds(&self, window: &WebviewWindow) {
        if window.is_maximized().unwrap_or(true) || window.is_fullscreen().unwrap_or(true) {
            return;
        }
        let (Ok(position), Ok(size), Ok(scale)) = (
            window.outer_position(),
            window.inner_size(),
            window.scale_factor(),
        ) else {
            return;
        };
        let logical = size.to_logical::<f64>(scale);
        if logical.width < 720. || logical.height < 480. {
            return;
        }
        let mut state = self.core.workbench_state();
        if state.ui.restoring_layout {
            return;
        }
        for w in state.ui.workspaces.values_mut() {
            if let Some(h) = w.hosts.get_mut(window.label()) {
                h.bounds = Some(crate::workbench_memory::Bounds {
                    x: position.x,
                    y: position.y,
                    width: logical.width,
                    height: logical.height,
                });
            }
        }
        drop(state);
        self.core.remember_workbench_layout();
    }
    fn restore_layout(
        &self,
        handle: &tauri::AppHandle,
        source: &str,
        context: &crate::workspaces::Context,
    ) -> Result<(), String> {
        let saved = {
            let mut state = self.core.workbench_state();
            state.workspaces.validate_owned(source, context)?;
            if state.ui.restoring_layout
                || state
                    .ui
                    .lifecycle
                    .host_closing(&context.workspace_id, source)
                || !state.ui.transfers.is_empty()
            {
                return Err("工作台正在交接或关闭，请稍后恢复布局".into());
            }
            let hosts = state.ui.memory.hosts(&context.server_id);
            state.ui.restoring_layout = true;
            hosts
        };
        let result = (|| {
            for saved in saved {
                let target = {
                    let mut state = self.core.workbench_state();
                    state.workspaces.validate_owned(source, context)?;
                    let w = state
                        .ui
                        .workspaces
                        .get(&context.workspace_id)
                        .ok_or("工作台已关闭")?;
                    if w.hosts.values().any(|h| h.layout_slot == saved.slot) {
                        continue;
                    }
                    let target = format!("workspace-{}", self.core.next_id());
                    let host = saved.seed(format!("{}@{target}", context.workspace_id))?;
                    state.workspaces.register(&target)?;
                    state
                        .workspaces
                        .add_host(&target, &context.workspace_id, usize::MAX)?;
                    state
                        .ui
                        .workspaces
                        .get_mut(&context.workspace_id)
                        .unwrap()
                        .hosts
                        .insert(target.clone(), host);
                    target
                };
                let display = (|| {
                    let window =
                        self.create_display(handle, &target, saved.bounds.as_ref(), None)?;
                    self.core.workspace_changed();
                    let deadline = Instant::now() + Duration::from_secs(8);
                    while !self.core.workspace_state().is_ready(&target) {
                        if Instant::now() > deadline {
                            return Err("恢复窗口未就绪，已有页面继续留在原位置".into());
                        }
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    window.show().map_err(|e| e.to_string())?;
                    Ok(())
                })();
                if let Err(error) = display {
                    let mut state = self.core.workbench_state();
                    state.workspaces.remove_host(&target, &context.workspace_id);
                    if let Some(w) = state.ui.workspaces.get_mut(&context.workspace_id) {
                        w.hosts.remove(&target);
                    }
                    drop(state);
                    self.cleanup_empty(handle, &target);
                    return Err(error);
                }
                for key in saved.tools.keys() {
                    let view = {
                        let ui = self.core.ui_state();
                        ui.workspaces[&context.workspace_id]
                            .views
                            .iter()
                            .find(|v| {
                                &v.definition_key == key
                                    && v.ui_requested
                                    && ui
                                        .definitions
                                        .get(key)
                                        .is_some_and(|d| d.available && d.ui_api_version == "2")
                            })
                            .map(|v| v.id.clone())
                    };
                    let Some(view) = view else {
                        continue;
                    };
                    let (origin, destination) = {
                        let ui = self.core.ui_state();
                        let w = &ui.workspaces[&context.workspace_id];
                        (
                            w.view_window(&view),
                            saved.destination(w.host(&target)?, key),
                        )
                    };
                    if let (Some(origin), Some(destination)) = (origin, destination) {
                        if origin != target {
                            self.move_scope_to(
                                handle,
                                &origin,
                                &context.workspace_id,
                                Some(target.clone()),
                                usize::MAX,
                                None,
                                crate::workbench_move::Scope::View { id: view.clone() },
                                destination,
                            )?;
                        }
                    }
                    if saved.tools[key].side.is_some() && !saved.tools[key].visible {
                        self.core.ui_action(
                            &target,
                            context,
                            serde_json::json!({"op":"collapse","side":saved.tools[key].side}),
                        )?;
                    }
                }
            }
            Ok(())
        })();
        self.core.ui_state().restoring_layout = false;
        self.core.workspace_changed();
        result
    }
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
            native_moves: Mutex::new(Default::default()),
        }
    }
    pub fn restore_main_display(&self, handle: &tauri::AppHandle) -> Result<(), String> {
        if handle.get_webview_window("main").is_some() || self.core.is_closing() {
            return Ok(());
        }
        let config = handle
            .config()
            .app
            .windows
            .first()
            .ok_or("主窗口配置缺失")?;
        let result = tauri::WebviewWindowBuilder::from_config(handle, config)
            .and_then(|builder| builder.build());
        if let Err(error) = result {
            self.core.log(
                None,
                "error",
                "主窗口显示恢复失败",
                Some(serde_json::json!({"error":error.to_string()})),
            );
            return Err(error.to_string());
        }
        Ok(())
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
        let op = self.core.close_native_prepare(
            window,
            crate::lifecycle::Target::Window { id: window.into() },
        )?;
        self.core.close_unhosted(&op.id);
        if let Some(w) = handle.get_webview_window(window) {
            let _ = w.set_focus();
        }
        Ok(())
    }
    pub fn finish_window(&self, handle: &tauri::AppHandle, window: &str) -> Result<(), String> {
        self.remove_surface(window);
        let _ = handle.emit("workspace-drop-preview", DropTarget::Invalid);
        if let Some(w) = handle.get_webview_window(window) {
            w.destroy().map_err(|e| e.to_string())?;
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        while handle.get_webview_window(window).is_some() {
            if Instant::now() >= deadline {
                return Err("原生窗口尚未确认销毁，请继续完成关闭".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        self.core.workspace_changed();
        Ok(())
    }
    fn cleanup_empty(&self, handle: &tauri::AppHandle, window: &str) {
        let empty = self.core.workspace_state().empty_secondary(window);
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
        self.move_scope_to(
            handle,
            source,
            id,
            target,
            index,
            position,
            crate::workbench_move::Scope::Host,
            Default::default(),
        )
    }
    pub fn move_scope_to(
        &self,
        handle: &tauri::AppHandle,
        source: &str,
        id: &str,
        target: Option<String>,
        index: usize,
        position: Option<(f64, f64)>,
        scope: crate::workbench_move::Scope,
        destination: crate::workbench_move::Destination,
    ) -> Result<(), String> {
        self.move_scope_request(
            handle,
            source,
            id,
            target,
            index,
            position,
            scope,
            destination,
            None,
        )
    }
    fn move_scope_request(
        &self,
        handle: &tauri::AppHandle,
        source: &str,
        id: &str,
        target: Option<String>,
        index: usize,
        position: Option<(f64, f64)>,
        scope: crate::workbench_move::Scope,
        destination: crate::workbench_move::Destination,
        request_id: Option<String>,
    ) -> Result<(), String> {
        let (target, destination) = if destination.return_to_source {
            if let Some(result) = request_id
                .as_ref()
                .and_then(|token| self.core.existing_return_move(token, source, id, &scope))
            {
                return result;
            }
            let (target, destination) = self.core.resolve_workbench_return(source, id, &scope)?;
            (Some(target), destination)
        } else {
            (target, destination)
        };
        if target.as_deref() == Some(source) && !matches!(scope, crate::workbench_move::Scope::Host)
        {
            let context = self
                .core
                .workspace_projection(source)?
                .workspaces
                .iter()
                .find(|w| w.context.workspace_id == id)
                .ok_or("工作台已关闭")?
                .context
                .clone();
            let action = match scope {
                crate::workbench_move::Scope::View { id } => {
                    let edge = destination.edge.as_deref().unwrap_or("center");
                    if let Some(side) = destination.side.as_deref() {
                        serde_json::json!({"op":"dock","viewId":id,"side":side})
                    } else if ["left", "right", "top", "bottom"].contains(&edge) {
                        serde_json::json!({"op":"split","groupId":destination.group_id,"viewId":id,"axis":if ["left","right"].contains(&edge){"x"}else{"y"},"before":(["left","top"].contains(&edge))})
                    } else {
                        serde_json::json!({"op":"moveView","groupId":destination.group_id,"viewId":id,"index":destination.index})
                    }
                }
                crate::workbench_move::Scope::Group { id } => {
                    serde_json::json!({"op":"moveGroup","sourceGroupId":id,"groupId":destination.group_id,"edge":destination.edge})
                }
                _ => unreachable!(),
            };
            self.core.ui_action(source, &context, action)?;
            return Ok(());
        }
        let new_window = target.is_none();
        let token = request_id.unwrap_or_else(|| self.core.next_id());
        let target = target.unwrap_or_else(|| {
            use sha2::{Digest, Sha256};
            let hash = Sha256::digest(token.as_bytes());
            format!("workspace-{:x}", hash)[..30].to_owned()
        });
        if let Some(result) = self.core.existing_workbench_move(
            &token,
            source,
            id,
            &target,
            index,
            &scope,
            &destination,
        ) {
            return result;
        }
        if self.core.is_closing() {
            return Err("应用正在退出".into());
        }
        if self.core.ui_state().frozen(id) {
            return Err("工作区正在确认关闭".into());
        }
        let full = source != target;
        let initial_bounds = new_window
            .then(|| self.estimate_bounds(handle, source, id, &scope))
            .flatten();
        let m = if full {
            self.core.prepare_workbench_move(
                source,
                id,
                &target,
                index,
                token.clone(),
                new_window,
                scope,
                destination,
            )?
        } else {
            self.core.prepare_workspace_move(
                source,
                id,
                &target,
                index,
                self.core.next_id(),
                new_window,
            )?
        };
        let fields = serde_json::json!({"workspaceId":id,"sourceWindowId":source,"targetWindowId":target,"operationId":m.token});
        self.core
            .log(None, "info", "工作台迁移准备", Some(fields.clone()));
        self.core.workspace_changed();
        let result = (|| {
            if new_window {
                self.create_display(handle, &target, initial_bounds.as_ref(), position)?;
            }
            let started = Instant::now();
            loop {
                let state = self.core.workbench_state();
                if state.workspaces.is_ready(&target)
                    && (if full {
                        state.ui.transfers.get(&m.token).is_some_and(|t| {
                            t.views.iter().all(|v| {
                                state
                                    .ui
                                    .surfaces
                                    .values()
                                    .any(|s| s.window == target && s.view == *v && s.ready)
                            })
                        })
                    } else {
                        state.ui.target_ready(id, &target)
                    })
                {
                    break;
                }
                if !state.workspaces.windows.contains_key(&target) || self.core.is_closing() {
                    return Err("目标窗口已关闭".into());
                }
                if started.elapsed() > Duration::from_secs(8) {
                    return Err("目标窗口未就绪，工作区保留在原窗口".into());
                }
                drop(state);
                std::thread::sleep(Duration::from_millis(20));
            }
            if full {
                self.core
                    .log(None, "info", "工作台迁移排空", Some(fields.clone()));
                self.core.begin_transfer_drain(&m)?;
                let started = Instant::now();
                while !self.core.transfer_can_commit(&m) {
                    if started.elapsed() > Duration::from_secs(45)
                        || self
                            .core
                            .workspace_state()
                            .pending_move(id, source)
                            .is_none()
                    {
                        return Err("页面交接尚未确认，保留在原窗口".into());
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
            let window = handle.get_webview_window(&target).ok_or("目标窗口已关闭")?;
            window.unminimize().map_err(|e| e.to_string())?;
            window.show().map_err(|e| e.to_string())?;
            if full {
                self.core.commit_workbench_move(&m)?;
            } else {
                self.core.commit_workspace_move(&m)?;
            }
            self.record_bounds(&window);
            self.core
                .log(None, "info", "工作台迁移提交", Some(fields.clone()));
            self.core.workspace_changed();
            let _ = window.set_focus();
            Ok(())
        })();
        if result.is_err() {
            self.core.log(None, "warn", "工作台迁移取消", Some(fields));
            self.core.cancel_workbench_move(&m);
            if new_window {
                self.cleanup_empty(handle, &target);
            }
            self.core.workspace_changed();
        } else if source != target {
            self.cleanup_empty(handle, source);
        }
        result
    }
    fn return_window(
        &self,
        handle: &tauri::AppHandle,
        source: &str,
        token: String,
    ) -> Result<(), String> {
        if let Some(result) = self.core.existing_window_return(source, &token) {
            return result;
        }
        let movements = self.core.prepare_window_return(source, token)?;
        self.core.workspace_changed();
        let result = (|| {
            let started = Instant::now();
            while !movements.iter().all(|m| self.core.transfer_prepared(m)) {
                if started.elapsed() > Duration::from_secs(8) || self.core.is_closing() {
                    return Err("部分页面未就绪，全部工作台保留在原窗口".into());
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            for m in &movements {
                self.core.begin_transfer_drain(m)?;
            }
            let started = Instant::now();
            while !movements.iter().all(|m| self.core.transfer_can_commit(m)) {
                if started.elapsed() > Duration::from_secs(45) || self.core.is_closing() {
                    return Err("部分页面交接未确认，全部工作台保留在原窗口".into());
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            let main = handle.get_webview_window("main").ok_or("主窗口已关闭")?;
            main.unminimize().map_err(|e| e.to_string())?;
            main.show().map_err(|e| e.to_string())?;
            self.core.commit_workbench_moves(&movements)?;
            self.core.workspace_changed();
            let _ = main.set_focus();
            Ok(())
        })();
        if result.is_err() {
            for m in &movements {
                self.core.cancel_workbench_move(m);
            }
        } else {
            self.cleanup_empty(handle, source);
        }
        self.core.log(None, if result.is_ok(){"info"}else{"warn"}, "窗口全部返回主窗口", Some(serde_json::json!({"sourceWindowId":source,"count":movements.len(),"error":result.as_ref().err()})));
        self.core.workspace_changed();
        result
    }
    fn hit(
        &self,
        handle: &tauri::AppHandle,
        drag: &Drag,
        local: Option<&LocalPoint>,
    ) -> Result<DropTarget, String> {
        let source = handle
            .get_webview_window(&drag.source)
            .ok_or("源窗口已关闭")?;
        #[cfg(target_os = "linux")]
        if std::env::var_os("WAYLAND_DISPLAY").is_some()
            && std::env::var("GDK_BACKEND").as_deref() != Ok("x11")
        {
            if let Some(point) = local.filter(|p| p.x.is_finite() && p.y.is_finite()) {
                let size = source
                    .inner_size()
                    .map_err(|e| e.to_string())?
                    .to_logical::<f64>(source.scale_factor().map_err(|e| e.to_string())?);
                if point.x >= 0. && point.y >= 0. && point.x < size.width && point.y < size.height {
                    let surfaces = self.surfaces.lock().unwrap();
                    let s = surfaces.get(&drag.source).ok_or("工作台落点尚未就绪")?;
                    let strip = hit_surface(&drag.source, s, point.x, point.y, &drag.workspace);
                    return Ok(
                        if strip != DropTarget::Invalid
                            || s.blocked
                            || matches!(drag.scope, crate::workbench_move::Scope::Host)
                        {
                            strip
                        } else {
                            self.hit_content(
                                &drag.source,
                                s,
                                point.x,
                                point.y,
                                &drag.workspace,
                                &drag.scope,
                            )
                        },
                    );
                }
            }
            return Err("Wayland 不支持跨窗口拖动，请使用菜单移窗；同窗拖动仍可用".into());
        }
        #[cfg(not(target_os = "linux"))]
        let _ = local;
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
        self.hit_scoped(handle, p.x, p.y, &drag.workspace, drag.offset, &drag.scope)
    }
    fn hit_point(
        &self,
        handle: &tauri::AppHandle,
        px: f64,
        py: f64,
        workspace: &str,
        offset: (f64, f64),
    ) -> Result<DropTarget, String> {
        self.hit_scoped(
            handle,
            px,
            py,
            workspace,
            offset,
            &crate::workbench_move::Scope::Host,
        )
    }
    fn hit_scoped(
        &self,
        handle: &tauri::AppHandle,
        px: f64,
        py: f64,
        workspace: &str,
        offset: (f64, f64),
        scope: &crate::workbench_move::Scope,
    ) -> Result<DropTarget, String> {
        // Native queries may synchronously use the event loop. Never keep a
        // registry lock while querying a window whose callbacks update it.
        let surfaces = self.surfaces.lock().unwrap().clone();
        let focus = self.focus.lock().unwrap().clone();
        #[cfg(target_os = "windows")]
        let front = {
            use windows_sys::Win32::Foundation::{HWND, POINT};
            #[link(name = "user32")]
            unsafe extern "system" {
                fn WindowFromPoint(point: POINT) -> HWND;
                fn GetAncestor(window: HWND, flags: u32) -> HWND;
            }
            let hwnd = unsafe {
                GetAncestor(
                    WindowFromPoint(POINT {
                        x: px.round() as i32,
                        y: py.round() as i32,
                    }),
                    2,
                )
            };
            if hwnd.is_null() {
                return Ok(DropTarget::Invalid);
            }
            hwnd as isize
        };
        for label in focus.iter().rev() {
            let Some(w) = handle.get_webview_window(label) else {
                continue;
            };
            #[cfg(target_os = "windows")]
            if w.hwnd().map(|h| h.0 as isize).ok() != Some(front) {
                continue;
            }
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
            let strip = hit_surface(label, surface, x, y, workspace);
            if strip != DropTarget::Invalid
                || matches!(scope, crate::workbench_move::Scope::Host)
                || surface.blocked
            {
                return Ok(strip);
            }
            return Ok(self.hit_content(label, surface, x, y, workspace, scope));
        }
        Ok(DropTarget::Outside {
            x: px - offset.0,
            y: py - offset.1,
        })
    }
    fn hit_content(
        &self,
        label: &str,
        surface: &Surface,
        x: f64,
        y: f64,
        workspace: &str,
        scope: &crate::workbench_move::Scope,
    ) -> DropTarget {
        let exclude = match scope {
            crate::workbench_move::Scope::View { id } => Some(id.as_str()),
            _ => None,
        };
        let is_tool = exclude.is_some_and(|id| {
            let ui = self.core.ui_state();
            ui.resource_view(id).is_ok_and(|v| {
                ui.definitions
                    .get(&v.definition_key)
                    .is_some_and(|d| d.definition.container == "sidebar")
            })
        });
        if is_tool {
            let dock = hit_dock(label, &surface.docks, x, y, workspace);
            if dock != DropTarget::Invalid {
                return dock;
            }
        }
        let hit = hit_group(label, &surface.groups, x, y, workspace, exclude);
        if let DropTarget::Group { group_id, edge, .. } = &hit {
            if ["left", "right", "top", "bottom"].contains(&edge.as_str()) {
                let ui = self.core.ui_state();
                let Some(host) = ui
                    .workspaces
                    .get(workspace)
                    .and_then(|w| w.host(label).ok())
                else {
                    return DropTarget::Invalid;
                };
                let mut preview = host.clone();
                match scope {
                    crate::workbench_move::Scope::View { id } if preview.contains(id) => {
                        if preview.group_of(id).as_deref() == Some(group_id)
                            && preview.groups[group_id].view_ids.len() == 1
                        {
                            return DropTarget::Invalid;
                        }
                        preview.remove(id);
                    }
                    crate::workbench_move::Scope::Group { id }
                        if preview.groups.contains_key(id) =>
                    {
                        if id == group_id {
                            return DropTarget::Invalid;
                        }
                        if preview.remove_group(id).is_err() {
                            return DropTarget::Invalid;
                        }
                    }
                    _ => {}
                }
                if preview
                    .split(
                        group_id,
                        if ["left", "right"].contains(&edge.as_str()) {
                            "x"
                        } else {
                            "y"
                        },
                        "native-preview-group".into(),
                        "native-preview-split".into(),
                        false,
                    )
                    .is_err()
                {
                    return DropTarget::Invalid;
                }
                let (width, height) = preview.center.minimum();
                let Some(rect) = surface.groups.iter().find(|g| &g.group_id == group_id) else {
                    return DropTarget::Invalid;
                };
                if width > rect.workspace_width || height > rect.workspace_height {
                    return DropTarget::Invalid;
                }
            }
        }
        hit
    }
    /// Package smoke check using real window geometry, without moving the user's pointer.
    pub fn verify_native_geometry(
        &self,
        handle: &tauri::AppHandle,
        label: &str,
        workspace: &str,
    ) -> Result<(), String> {
        let window = handle.get_webview_window(label).ok_or("原生窗口不存在")?;
        window.unminimize().map_err(|e| e.to_string())?;
        window.show().map_err(|e| e.to_string())?;
        window.set_focus().map_err(|e| e.to_string())?;
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
        // Native focus requests complete asynchronously; check real occlusion after focus settles.
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let hit = self.hit_point(handle, x, y, workspace, (0., 0.))?;
            if matches!(&hit, DropTarget::Strip { window_id, .. } if window_id == label) {
                break;
            }
            if Instant::now() >= deadline {
                return Err(format!(
                    "原生屏幕坐标没有命中目标 Tab 条: window={label}, focused={:?}, origin={origin:?}, size={size:?}, scale={scale}, point=({x},{y}), hit={hit:?}",
                    window.is_focused()
                ));
            }
            std::thread::sleep(Duration::from_millis(20));
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

#[tauri::command]
pub fn workbench_locate_view(
    handle: tauri::AppHandle,
    window: WebviewWindow,
    desktop: State<DesktopWindows>,
    context: crate::workspaces::Context,
    view_id: String,
) -> Result<(), String> {
    let target = {
        let state = desktop.core.workbench_state();
        state.workspaces.validate_owned(window.label(), &context)?;
        let view = state.ui.resource_view(&view_id)?;
        if view.context != context {
            return Err("页面 Context 已改变".into());
        }
        state.ui.workspaces[&context.workspace_id]
            .view_window(&view_id)
            .ok_or("页面没有显示位置")?
    };
    desktop.core.ui_action(
        &target,
        &context,
        serde_json::json!({"op":"focus","viewId":view_id}),
    )?;
    desktop
        .core
        .workspace_state()
        .activate(&target, Some(&context.workspace_id))?;
    desktop.core.workspace_changed();
    let target_window = handle.get_webview_window(&target).ok_or("目标窗口已消失")?;
    target_window.show().map_err(|e| e.to_string())?;
    target_window.set_focus().map_err(|e| e.to_string())?;
    target_window
        .emit(
            "workbench-locate-view",
            serde_json::json!({"workspaceId":context.workspace_id,"viewId":view_id}),
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}
#[tauri::command]
pub async fn workbench_move(
    handle: tauri::AppHandle,
    window: WebviewWindow,
    id: String,
    request_id: String,
    context: Option<crate::workspaces::Context>,
    target: Option<String>,
    scope: crate::workbench_move::Scope,
    destination: Option<crate::workbench_move::Destination>,
) -> Result<(), String> {
    let source = window.label().to_owned();
    let task_handle = handle.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let desktop = task_handle.state::<DesktopWindows>();
        if !desktop
            .native_moves
            .lock()
            .unwrap()
            .insert(request_id.clone())
        {
            return Err("移动正在进行，请查询原 requestId".into());
        }
        let result = (|| {
            if let Some(context) = context {
                if desktop
                    .core
                    .workbench_move_status(&source, &request_id)
                    .is_err()
                {
                    desktop
                        .core
                        .workspace_state()
                        .validate_owned(&source, &context)?;
                }
            }
            desktop.move_scope_request(
                &task_handle,
                &source,
                &id,
                target,
                usize::MAX,
                None,
                scope,
                destination.unwrap_or_default(),
                Some(request_id.clone()),
            )
        })();
        desktop.native_moves.lock().unwrap().remove(&request_id);
        result
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn workbench_return_window(
    handle: tauri::AppHandle,
    window: WebviewWindow,
    request_id: String,
) -> Result<(), String> {
    let source = window.label().to_owned();
    tauri::async_runtime::spawn_blocking(move || {
        let desktop = handle.state::<DesktopWindows>();
        if !desktop
            .native_moves
            .lock()
            .unwrap()
            .insert(request_id.clone())
        {
            return Err("返回正在进行，请查询原 requestId".into());
        }
        let result = desktop.return_window(&handle, &source, request_id.clone());
        desktop.native_moves.lock().unwrap().remove(&request_id);
        result
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn workbench_restore_layout(
    handle: tauri::AppHandle,
    window: WebviewWindow,
    context: crate::workspaces::Context,
) -> Result<(), String> {
    let source = window.label().to_owned();
    tauri::async_runtime::spawn_blocking(move || {
        handle
            .state::<DesktopWindows>()
            .restore_layout(&handle, &source, &context)
    })
    .await
    .map_err(|e| e.to_string())?
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
pub fn hit_group(
    label: &str,
    groups: &[GroupRect],
    x: f64,
    y: f64,
    workspace: &str,
    exclude: Option<&str>,
) -> DropTarget {
    for g in groups {
        if g.workspace_id != workspace
            || !x.is_finite()
            || !y.is_finite()
            || x < g.x
            || y < g.y
            || x >= g.x + g.width
            || y >= g.y + g.height
        {
            continue;
        }
        let edge = if y - g.y < 36. {
            "tab"
        } else if x - g.x < g.width.min(240.) * 0.2 {
            "left"
        } else if g.x + g.width - x < g.width.min(240.) * 0.2 {
            "right"
        } else if y - g.y < g.height.min(180.) * 0.2 {
            "top"
        } else if g.y + g.height - y < g.height.min(180.) * 0.2 {
            "bottom"
        } else {
            "center"
        };
        return DropTarget::Group {
            window_id: label.into(),
            group_id: g.group_id.clone(),
            edge: edge.into(),
            index: g
                .tabs
                .iter()
                .filter(|t| Some(t.id.as_str()) != exclude && t.center < x)
                .count(),
        };
    }
    DropTarget::Invalid
}
pub fn hit_dock(label: &str, docks: &[DockRect], x: f64, y: f64, workspace: &str) -> DropTarget {
    for dock in docks {
        if dock.workspace_id == workspace
            && ["left", "right"].contains(&dock.side.as_str())
            && x.is_finite()
            && y.is_finite()
            && x >= dock.x
            && y >= dock.y
            && x < dock.x + dock.width
            && y < dock.y + dock.height
        {
            return DropTarget::Dock {
                window_id: label.into(),
                workspace_id: workspace.into(),
                side: dock.side.clone(),
            };
        }
    }
    DropTarget::Invalid
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
    desktop.core.workspace_state().ready(window.label())?;
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
        .workspace_state()
        .activate(window.label(), id.as_deref())?;
    desktop.core.workspace_changed();
    Ok(())
}
#[tauri::command]
pub fn workspace_layout(
    window: WebviewWindow,
    desktop: State<DesktopWindows>,
    id: String,
    side: String,
    width: u16,
) -> Result<crate::workspaces::LayoutCommit, String> {
    desktop
        .core
        .workspace_resize(window.label(), &id, &side, width)
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
    let movement = desktop
        .core
        .workspace_state()
        .pending_move(&id, window.label());
    if let Some(m) = movement {
        desktop.core.cancel_workbench_move(&m);
    }
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
        .close_native_prepare(window.label(), crate::lifecycle::Target::Host { id })?;
    let _ = handle;
    Ok(())
}
#[tauri::command]
pub fn workspace_close_window(
    handle: tauri::AppHandle,
    window: WebviewWindow,
    desktop: State<DesktopWindows>,
) -> Result<(), String> {
    if window.label() == "main" {
        return Err("请通过应用关闭入口退出".into());
    }
    desktop.close_secondary(&handle, window.label())
}
#[tauri::command]
pub fn workspace_drag_start(
    window: WebviewWindow,
    desktop: State<DesktopWindows>,
    id: String,
    offset_x: f64,
    offset_y: f64,
    scope: Option<crate::workbench_move::Scope>,
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
        scope: scope.unwrap_or(crate::workbench_move::Scope::Host),
    });
    Ok(token)
}
#[tauri::command]
pub fn workspace_drag_sample(
    handle: tauri::AppHandle,
    window: WebviewWindow,
    desktop: State<DesktopWindows>,
    token: String,
    local_point: Option<LocalPoint>,
) -> Result<DropTarget, String> {
    let drag = desktop
        .drag
        .lock()
        .unwrap()
        .as_ref()
        .filter(|d| d.token == token && d.source == window.label())
        .cloned()
        .ok_or("拖动已取消")?;
    let target = desktop.hit(&handle, &drag, local_point.as_ref())?;
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
    local_point: Option<LocalPoint>,
) -> Result<(), String> {
    let (source, id, target, scope) = {
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
            desktop.hit(&handle, &drag, local_point.as_ref())
        };
        (drag.source, drag.workspace, target, drag.scope)
    };
    let _ = handle.emit("workspace-drop-preview", DropTarget::Invalid);
    let target = target?;
    tauri::async_runtime::spawn_blocking(move || {
        let desktop = handle.state::<DesktopWindows>();
        match target {
            DropTarget::Strip { window_id, index } => desktop.move_scope_request(
                &handle,
                &source,
                &id,
                Some(window_id),
                index,
                None,
                scope,
                Default::default(),
                Some(token),
            ),
            DropTarget::Outside { x, y } => desktop.move_scope_request(
                &handle,
                &source,
                &id,
                None,
                0,
                Some((x, y)),
                scope,
                Default::default(),
                Some(token),
            ),
            DropTarget::Group {
                window_id,
                group_id,
                edge,
                index,
            } => desktop.move_scope_request(
                &handle,
                &source,
                &id,
                Some(window_id),
                usize::MAX,
                None,
                scope,
                crate::workbench_move::Destination {
                    group_id: Some(group_id),
                    edge: Some(edge),
                    index: Some(index),
                    side: None,
                    ..Default::default()
                },
                Some(token),
            ),
            DropTarget::Dock {
                window_id, side, ..
            } => desktop.move_scope_request(
                &handle,
                &source,
                &id,
                Some(window_id),
                usize::MAX,
                None,
                scope,
                crate::workbench_move::Destination {
                    side: Some(side),
                    ..Default::default()
                },
                Some(token),
            ),
            DropTarget::Invalid => Ok(()),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

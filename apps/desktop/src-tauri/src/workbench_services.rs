use super::*;
use crate::window_registry::WindowRecord;

pub(super) struct LayoutOperation {
    pub request: Value,
    pub result: Mutex<Value>,
    pub canceled: AtomicBool,
    pub created: Instant,
}
pub(super) struct WorkspaceGuard<'a> {
    state: &'a State,
    ids: Vec<String>,
}
impl Drop for WorkspaceGuard<'_> {
    fn drop(&mut self) {
        let mut busy = self.state.workspace_busy.lock().unwrap();
        for id in &self.ids {
            busy.remove(id);
        }
    }
}

impl State {
    fn finish_workspace_open(&self, params: &Value, window: &str, mut result: Value) -> Value {
        if let Some(ws) = self.workbench.lock().unwrap().workspaces.get(params["serverId"].as_str().unwrap_or("")) {
            result["generation"] = json!(ws.generation);
            result["initializers"] = json!(ws.initializers);
        }
        let result = self.commit_runtime(result);
        if let Some(id) = params["operationId"].as_str() {
            self.layout_operations.lock().unwrap().insert(id.into(), Arc::new(LayoutOperation {
                request: json!({"method":"workspaces.open","windowId":window,"params":params}),
                result: Mutex::new(result.clone()), canceled: AtomicBool::new(false), created: Instant::now(),
            }));
        }
        result
    }
    fn initialize_workspace(self: &Arc<Self>, id: &str) -> Result<()> {
        let plugins = self.all_plugins();
        let enabled_missing=plugins.is_empty()&&{let m=self.plugins.lock().unwrap();m.status().iter().any(|s|m.boot_enabled(&s.plugin_id))};
        let (generation, window, jobs) = {
            let mut w = self.workbench.lock().unwrap();
            let ws = w.workspaces.get_mut(id).ok_or_else(|| err("WORKSPACE_CLOSED", "工作区已关闭"))?;
            if plugins.is_empty(){ws.initialization_error=enabled_missing.then(||"没有可用插件，请检查插件后重试".into());return Ok(());}
            ws.initialization_error = None;
            let mut jobs = vec![];
            for plugin in &plugins { for init in &plugin.package.manifest.contributes.workspace_initializers {
                let init_key=format!("{}::{}",plugin.package.manifest.plugin_id,init.id);
                if ws.initializers.iter().any(|i| i.id == init_key && (i.state != "failed" || i.view_id.is_some())) { continue; }
                let operation = Uuid::new_v4().to_string();
                ws.initializers.retain(|i| i.id != init_key);
                ws.initializers.push(crate::workbench::Initializer { id:init_key.clone(), command_id:init.command_id.clone(), operation_id:operation.clone(), state:"running".into(), view_id:None, error:None });
                jobs.push((plugin.package.manifest.plugin_id.clone(),init_key, init.command_id.clone(), operation));
            }
            }
            (ws.generation.clone(), ws.owner_window_id.clone(), jobs)
        };
        for (plugin,initializer, command, operation) in jobs {
            let state = self.clone(); let id = id.to_string(); let generation = generation.clone(); let window = window.clone();
            std::thread::spawn(move || {
                let params = json!({"pluginId":plugin,"commandId":command,"workspaceId":id,"workspaceGeneration":generation,"initializerId":initializer,"operationId":operation});
                let result = state.dispatch(&Caller::shell(), Some(&format!("@window:{window}")), "commands.execute", params);
                let view = state.pending_opens.lock().unwrap().get(&operation).and_then(|o| o.view_id.clone());
                {
                    let mut w = state.workbench.lock().unwrap();
                    if let Some(ws) = w.workspaces.get_mut(&id).filter(|ws| ws.generation == generation) {
                        if let Some(init) = ws.initializers.iter_mut().find(|i| i.operation_id == operation) {
                            init.state = if result.is_ok() {"finished"} else {"failed"}.into();
                            init.view_id = view;
                            init.error = result.err().map(|e| e.message);
                        }
                    }
                }
                state.changed();
            });
        }
        Ok(())
    }
    pub(super) fn cancel_view_open(&self, view: &str) -> Result<()> {
        let operations = {
            let mut ops = self.pending_opens.lock().unwrap();
            ops.iter_mut().filter(|(_,o)| o.view_id.as_deref() == Some(view) && !o.canceled && (!o.finished || o.waiting_auth))
                .map(|(id,o)| {o.canceled=true; o.waiting_auth=false; id.clone()}).collect::<Vec<_>>()
        };
        for id in operations { self.worker("lifecycle.cancel", json!({"operationId":id}))?; }
        Ok(())
    }
    pub(super) fn workspace_guard(&self, ids: &[String]) -> Result<WorkspaceGuard<'_>> {
        self.workspace_guard_except(ids, None)
    }
    pub(super) fn workspace_guard_except(
        &self,
        ids: &[String],
        own_operation: Option<&str>,
    ) -> Result<WorkspaceGuard<'_>> {
        let mut busy = self.workspace_busy.lock().unwrap();
        if ids.iter().any(|id| busy.contains(id)) {
            return Err(err("MOVE_IN_PROGRESS", "该工作区正在移动或关闭"));
        }
        {
            let w = self.workbench.lock().unwrap();
            let ops = self.pending_opens.lock().unwrap();
            if ids.iter().filter_map(|id| w.workspaces.get(id)).any(|ws| ws.initializers.iter().any(|i| i.state == "running" && !ops.contains_key(&i.operation_id))) {
                return Err(err("MOVE_IN_PROGRESS", "工作区正在登记初始页面"));
            }
        }
        if self.pending_opens.lock().unwrap().iter().any(|(id, o)| {
            Some(id.as_str()) != own_operation
                && !o.canceled && (!o.finished || o.waiting_auth)
                && o.server_id.as_ref().is_some_and(|s| ids.contains(s))
        }) {
            return Err(err("MOVE_IN_PROGRESS", "请先完成或取消连接操作"));
        }
        busy.extend(ids.iter().cloned());
        Ok(WorkspaceGuard {
            state: self,
            ids: ids.to_vec(),
        })
    }
    pub(super) fn source_window(&self, source: Option<&str>, params: &Value) -> Result<String> {
        if let Some(source) = source {
            return self.endpoint_window(source,false);
        }
        if let Some(op) = params["operationId"].as_str() {
            if let Some(p) = self.pending_opens.lock().unwrap().get(op) {
                return Ok(p.window_id.clone());
            }
        }
        if let Some(id) = params["workspaceId"].as_str() {
            if id == "@app" {
                return Ok("main".into());
            }
            return self
                .workbench
                .lock()
                .unwrap()
                .owner(id)
                .map_err(native_error);
        }
        Ok("main".into())
    }
    pub(super) fn show_window(&self, id: &str) {
        if let Some(w) = self.app.get_webview_window(id) {
            let _ = w.show();
            let _ = w.unminimize();
            let _ = w.set_focus();
        }
    }
    pub(super) fn create_workspace_window(&self, id: &str) -> Result<()> {
        if self.app.get_webview_window(id).is_some() {
            return Ok(());
        }
        let mut builder =
            WebviewWindowBuilder::new(&self.app, id, WebviewUrl::App("index.html".into()))
                .title("SoDeck v2")
                .icon(crate::brand::icon(false)).map_err(|e|native_error(e.to_string()))?
                .background_color(self.theme_background())
                .initialization_script(crate::theme::bootstrap(&self.theme_snapshot(true)))
                .decorations(false)
                .visible(false)
                .inner_size(1100., 740.)
                .min_inner_size(760., 480.)
                .initialization_script(format!(
                    "window.__SODECK_CONTEXT__={{windowId:{}}};",
                    serde_json::to_string(id).unwrap()
                ))
                .data_directory(self.data.join("profiles/shell"));
        if cfg!(debug_assertions) {
            if let Some(port) = self.app.state::<crate::startup_config::DebugConfig>().shell_port {
                builder =
                    builder.additional_browser_args(&format!("--remote-debugging-port={port}"));
            }
        }
        builder.build().map_err(|e| native_error(e.to_string()))?;
        Ok(())
    }
    fn check_revision(&self, params: &Value) -> Result<()> {
        if let Some(rev) = params["expectedRevision"].as_u64() {
            if rev != self.workbench.lock().unwrap().revision {
                return Err(err("LAYOUT_CONFLICT", "布局已改变，请重试"));
            }
        }
        Ok(())
    }
    pub(super) fn workbench_service(
        self: &Arc<Self>,
        caller: &Caller,
        source: Option<&str>,
        method: &str,
        params: &Value,
    ) -> Option<Result<Value>> {
        if !matches!(
            method,
            "workbench.snapshot"
                | "workbench.configure"
                | "workbench.surface"
                | "workbench.modal"
                | "workbench.preferences.retry"
                | "workbench.operation.get"
                | "workbench.operation.cancel"
        ) && !method.starts_with("workspaces.")
            && !method.starts_with("windows.")
            && !method.starts_with("commands.")
            && !matches!(
                method,
                "views.open" | "views.update" | "views.bindResources"
            )
        {
            return None;
        }
        Some((|| {
            let window = self.source_window(source, params)?;
            match method {
                "workbench.preferences.retry" => {
                    self.save_preferences()?;
                    crate::window_preferences::retire(&self.data).map_err(native_error)?;
                    self.workbench.lock().unwrap().recovery_warnings.clear();
                    self.changed();Ok(json!({"persisted":true}))
                }
                "workbench.snapshot" => {
                    let target = params["targetWindowId"].as_str().unwrap_or(&window);
                    let mut snapshot = self.workbench.lock().unwrap().snapshot(target);
                    snapshot["commands"]=json!(self.all_plugins().iter().flat_map(|p|p.package.manifest.contributes.commands.clone()).collect::<Vec<_>>());
                    snapshot["viewDeclarations"]=json!(self.all_plugins().iter().flat_map(|p|p.package.manifest.contributes.views.iter().map(|d|{let locations=["center","left","right","detached"].into_iter().filter(|place|{let host=match *place{"center"=>"tab","detached"=>"window",_=>"dock"};d.supported_hosts.iter().any(|h|h==host)&&(host!="dock"||d.dock_edges.iter().any(|e|e==place))}).collect::<Vec<_>>();json!({"id":d.id,"locations":locations})})).collect::<Vec<_>>());
                    snapshot["pendingOpens"]=json!(self.pending_opens.lock().unwrap().iter().filter(|(_,o)|!o.finished&&o.window_id==target).map(|(id,o)|json!({"operationId":id,"serverId":o.server_id,"commandId":o.command_id,"cancelling":o.canceled})).collect::<Vec<_>>());
                    Ok(snapshot)
                }
                "workbench.configure" => {
                    let mut w = self.workbench.lock().unwrap();
                    let mut next = w.clone();
                    next.configure(&window, params).map_err(native_error)?;
                    w.commit(next)
                        .map_err(native_error)?;
                    let result = w.snapshot(&window);
                    drop(w);
                    self.changed();
                    Ok(result)
                }
                "workbench.surface" => self.present(
                    &window,
                    required(params, "viewId")?,
                    params["mountGeneration"]
                        .as_u64()
                        .ok_or_else(|| err("INVALID_PARAMS", "缺少挂载代次"))?,
                    serde_json::from_value(params["bounds"].clone())
                        .map_err(|_| err("INVALID_PARAMS", "尺寸无效"))?,
                    params["visible"].as_bool().unwrap_or(false),
                ),
                "workbench.modal" => {
                    self.shell_modal(&window, params["open"].as_bool().unwrap_or(false))?;
                    Ok(Value::Null)
                }
                "workspaces.open" => {
                    let _creation = self.workspace_creation.lock().unwrap();
                    if let Some(id) = params["operationId"].as_str() {
                        if let Some(op) = self.layout_operations.lock().unwrap().get(id) {
                            if op.request != json!({"method":"workspaces.open","windowId":window,"params":params}) {
                                return Err(err("REQUEST_CONFLICT", "操作标识已经用于其他请求"));
                            }
                            return Ok(op.result.lock().unwrap().clone());
                        }
                    }
                    let id = required(params, "serverId")?;
                    self.core
                        .call(&Caller::shell(), "servers.get", json!({"serverId":id}))?;
                    let existing = {
                        self.workbench
                            .lock()
                            .unwrap()
                            .workspaces
                            .get(id)
                            .map(|w| w.owner_window_id.clone())
                    };
                    if let Some(owner) = existing {
                        self.workbench
                            .lock()
                            .unwrap()
                            .activate_workspace(id)
                            .map_err(native_error)?;
                        self.show_window(&owner);
                        return Ok(self.finish_workspace_open(params, &window,
                            json!({"workspaceId":id,"ownerWindowId":owner,"created":false}),
                        ));
                    }
                    let target = if params["target"] == "newWindow" {
                        format!("workspace-{}", Uuid::new_v4())
                    } else {
                        if window.starts_with("detached-") {
                            self.workbench
                                .lock()
                                .unwrap()
                                .windows
                                .get("main")
                                .unwrap()
                                .window_id
                                .clone()
                        } else {
                            window.clone()
                        }
                    };
                    let fresh = target != window && target != "main";
                    if fresh {
                        self.create_workspace_window(&target)?;
                        self.workbench
                            .lock()
                            .unwrap()
                            .windows
                            .insert(target.clone(), WindowRecord::new(&target));
                    }
                    self.workbench
                        .lock()
                        .unwrap()
                        .open_workspace(id, &target)
                        .map_err(native_error)?;
                    let mut default_plugins = self.all_plugins();
                    default_plugins.sort_by(|a,b| a.package.manifest.plugin_id.cmp(&b.package.manifest.plugin_id));
                    for plugin in default_plugins {
                        let mut w = self.workbench.lock().unwrap();
                        let mut entries = plugin.package.manifest.contributes.workspace_entries.iter().collect::<Vec<_>>();
                        entries.sort_by(|a,b| a.view_type.cmp(&b.view_type));
                        for entry in entries {
                            if entry.open_by_default {
                                if let Err(e)=w.open_default_view(
                                    &plugin.package.manifest.plugin_id,
                                    &entry.view_type,
                                    id,
                                    "",
                                )
                                {if let Some(ws)=w.workspaces.get_mut(id){ws.initialization_error=Some(e);}}
                            }
                        }
                    }
                    {let w=self.workbench.lock().unwrap();if let Some(ws)=w.workspaces.get(id){self.core.register_workspace(id,&ws.generation);}}
                    self.initialize_workspace(id)?;
                    self.show_window(&target);
                    Ok(self.finish_workspace_open(params, &window,
                        json!({"workspaceId":id,"ownerWindowId":target,"created":true}),
                    ))
                }
                "workspaces.initialize" => {
                    let _creation = self.workspace_creation.lock().unwrap();
                    self.initialize_workspace(required(params, "workspaceId")?)?;
                    Ok(json!({"scheduled":true}))
                }
                "workspaces.activate" => {
                    let id = required(params, "workspaceId")?;
                    let owner = {
                        let mut w = self.workbench.lock().unwrap();
                        w.activate_workspace(id).map_err(native_error)?;
                        w.owner(id).map_err(native_error)?
                    };
                    self.show_window(&owner);
                    Ok(self.commit_runtime(
                        json!({"workspaceId":id,"ownerWindowId":owner})))
                }
                "workspaces.reorder" => {
                    self.check_revision(params)?;
                    let target = params["windowId"].as_str().unwrap_or(&window);
                    let ids: Vec<String> = serde_json::from_value(params["workspaceIds"].clone())
                        .map_err(|_| err("INVALID_PARAMS", "排序列表无效"))?;
                    self.workbench
                        .lock()
                        .unwrap()
                        .reorder(target, ids)
                        .map_err(native_error)?;
                    Ok(self.commit_runtime(json!({"reordered":true})))
                }
                "workspaces.move" | "windows.merge" => {
                    let request = json!({"method":method,"windowId":window,"params":params});
                    if let Some(id) = params["operationId"].as_str() {
                        if let Some(op) = self.layout_operations.lock().unwrap().get(id) {
                            if op.request != request {
                                return Err(err("REQUEST_CONFLICT", "操作标识已经用于其他请求"));
                            }
                            return Ok(op.result.lock().unwrap().clone());
                        }
                    }
                    self.check_revision(params)?;
                    let (ids, active) = if method == "windows.merge" {
                        let source = params["sourceWindowId"].as_str().unwrap_or(&window);
                        if source == "main" {
                            return Err(err("INVALID_PARAMS", "主窗口不能整窗合并"));
                        }
                        let w = self.workbench.lock().unwrap();
                        let win = w
                            .windows
                            .get(source)
                            .ok_or_else(|| err("WINDOW_CLOSED", "窗口已关闭"))?;
                        (win.workspace_ids.clone(), win.selected_workspace().cloned())
                    } else {
                        let id = required(params, "workspaceId")?.to_string();
                        (vec![id.clone()], Some(id))
                    };
                    let target = params["targetWindowId"]
                        .as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| {
                            if method == "windows.merge" {
                                "main".into()
                            } else {
                                format!("workspace-{}", Uuid::new_v4())
                            }
                        });
                    self.move_workspace_batch(
                        ids,
                        &target,
                        params["index"].as_u64().unwrap_or(u64::MAX) as usize,
                        active.as_deref(),
                        params["operationId"].as_str(),
                        request,
                    )
                }
                "workspaces.close" => {
                    let id = required(params, "workspaceId")?;
                    match self.close_workspaces(&[id.to_string()]) {
                        Err(e) if e.code == "MOVE_IN_PROGRESS" => {
                            self.queue_workspaces_close(format!("workspace:{id}"), vec![id.into()]);
                            Ok(json!({"queued":true}))
                        }
                        result => result,
                    }
                }
                "windows.close" => {
                    if params["windowId"].as_str().unwrap_or(&window)==settings_window::LABEL {return self.settings_request_close("close");}
                    if params["windowId"].as_str().unwrap_or(&window)==monitor_window::LABEL {return self.close_monitor(source);}
                    self.close_workspace_window(params["windowId"].as_str().unwrap_or(&window))
                }
                "windows.control" => {
                    let target = params["windowId"].as_str().unwrap_or(&window);
                    let win = self
                        .app
                        .get_webview_window(target)
                        .ok_or_else(|| err("WINDOW_CLOSED", "窗口已关闭"))?;
                    match required(params, "action")? {
                        "minimize" => win.minimize().map_err(|e| native_error(e.to_string()))?,
                        "toggleMaximize" => {
                            if win
                                .is_maximized()
                                .map_err(|e| native_error(e.to_string()))?
                            {
                                win.unmaximize()
                            } else {
                                win.maximize()
                            }
                            .map_err(|e| native_error(e.to_string()))?;
                        }
                        "show" => self.show_window(target),
                        "systemMenu" => {
                            crate::window_chrome::system_menu(&win).map_err(native_error)?
                        }
                        "drag" => win
                            .start_dragging()
                            .map_err(|e| native_error(e.to_string()))?,
                        "recall" => {
                            for (id,w) in self.app.webview_windows() {
                                if id.starts_with("application-menu-"){continue;}
                                self.cancel_menus(&id);
                                let _ = w.center();
                                let _ = w.show();
                            }
                        }
                        _ => return Err(err("INVALID_PARAMS", "不支持的窗口操作")),
                    }
                    Ok(json!({"maximized":win.is_maximized().unwrap_or(false)}))
                }
                "commands.execute" => self.execute_command(caller, &window, params),
                "commands.retry" => {
                    let id = required(params, "operationId")?;
                    let (mut request, origin) = {
                        let mut ops = self.pending_opens.lock().unwrap();
                        let op = ops
                            .get_mut(id)
                            .ok_or_else(|| err("OPERATION_EXPIRED", "认证操作已结束"))?;
                        if op.canceled || !op.waiting_auth {
                            return Err(err("CANCELED", "认证操作已取消或已重试"));
                        }
                        if op.window_id != window {
                            return Err(err("SOURCE_INVALID", "认证须在原窗口完成"));
                        }
                        op.waiting_auth = false;
                        {
                            let mut request = op.request.clone();
                            request["viewId"] = json!(op.view_id);
                            (request, op.window_id.clone())
                        }
                    };
                    request.as_object_mut().unwrap().remove("initializerId");
                    request["operationId"] = json!(Uuid::new_v4().to_string());
                    self.execute_command(caller, &origin, &request)
                }
                "views.open" => {
                    let plugin = self.loaded_for(caller.plugin_id.as_deref().ok_or_else(||err("SOURCE_INVALID","缺少插件来源"))?)?;
                    if caller.plugin_id.as_deref() != Some(&plugin.package.manifest.plugin_id) {
                        return Err(err("SOURCE_INVALID", "视图须由已登记插件创建"));
                    }
                    let kind = required(params, "viewType")?;
                    let declaration = plugin
                        .package
                        .manifest
                        .contributes
                        .views
                        .iter()
                        .find(|v| v.id == kind)
                        .ok_or_else(|| err("INVALID_PARAMS", "插件未声明此视图"))?;
                    let workspace = if declaration.scope == "global" {
                        "@app"
                    } else {
                        required(params, "workspaceId")?
                    };
                    let _worker = self.check_worker_operation(workspace, params)?;
                    let mut w = self.workbench.lock().unwrap();
                    let mut candidate = w.clone();
                    let previous_windows = w.windows.clone();
                    let previous_focus=w.workspaces.get(workspace).filter(|ws|ws.focus_intent>0).and_then(|ws|ws.focus_target.clone());
                    let mut view = candidate
                        .open_view(
                            &plugin.package.manifest.plugin_id,
                            required(params, "viewType")?,
                            workspace,
                            params["instanceKey"].as_str().unwrap_or(""),
                        )
                        .map_err(native_error)?;
                    Self::apply_view_metadata(&mut view, params)?;
                    self.validate_view_actions(&view)?;
                    let existed = w.views.iter().any(|v| v.view_id == view.view_id);
                    view.runtime_data = params["runtimeData"].clone();
                    *candidate
                        .views
                        .iter_mut()
                        .find(|v| v.view_id == view.view_id)
                        .unwrap() = view.clone();
                    // Preserve the user's foreground workspace during asynchronous page creation.
                    if params["operationId"].as_str().is_some_and(|id|self.pending_opens.lock().unwrap().get(id).is_some_and(|op|op.request.get("initializerId").is_some())) {
                        if let Some(focus)=previous_focus {
                            if let Some(ws)=candidate.workspaces.get_mut(workspace){ws.focus_target=Some(focus.clone());}
                            if let Ok(owner)=candidate.owner(workspace){if let Some(win)=candidate.windows.get_mut(&owner){win.focused_view_id=Some(focus);}}
                        }
                    }
                    for (id, previous) in previous_windows {
                        if previous.selected_workspace().map(String::as_str) != Some(workspace) {
                            if let Some(win) = candidate.windows.get_mut(&id) {
                                win.selection = previous.selection;win.last_workspace_id=previous.last_workspace_id;win.navigation_revision=previous.navigation_revision;
                                win.focused_view_id = previous.focused_view_id;
                            }
                        }
                    }
                    *w = candidate;
                    drop(w);
                    if view.placement == "detached" {
                        if let Err(error) = self.open_detached(&view) {
                            if !existed {
                                let _ = self.workbench.lock().unwrap().close(&view.view_id);
                            }
                            return Err(error);
                        }
                    }
                    if let Some(op) = params["operationId"].as_str() {
                        if let Some(p) = self.pending_opens.lock().unwrap().get_mut(op) {
                            p.view_id = Some(view.view_id.clone());
                        }
                    }
                    if !existed{if let Ok(target)=self.core.observation_target("view",&view.view_id,Some(&view.mount_generation.to_string())){let _=self.core.observe_host_event(caller,"core.view.created",json!({"objectRef":target["ref"],"generation":view.mount_generation.to_string()}));}}
                    Ok(self.commit_runtime(serde_json::to_value(view).unwrap()))
                }
                "views.update" => {
                    let id = required(params, "viewId")?;
                    let workspace = self
                        .workbench
                        .lock()
                        .unwrap()
                        .view(id)
                        .map_err(native_error)?
                        .workspace_id;
                    let _worker = self.check_worker_operation(&workspace, params)?;
                    let mut w = self.workbench.lock().unwrap();
                    let view = w
                        .views
                        .iter_mut()
                        .find(|v| v.view_id == id)
                        .ok_or_else(|| err("VIEW_CLOSED", "视图已关闭"))?;
                    if caller.plugin_id.as_deref() != Some(&view.plugin_id) {
                        return Err(err("SOURCE_INVALID", "元数据来源不匹配"));
                    }
                    if params["expectedRevision"]
                        .as_u64()
                        .is_some_and(|r| r != view.revision)
                    {
                        return Err(err("LAYOUT_CONFLICT", "视图已更新"));
                    }
                    let mut updated = view.clone();
                    Self::apply_view_metadata(&mut updated, params)?;
                    self.validate_view_actions(&updated)?;
                    updated.revision += 1;
                    *view = updated.clone();
                    w.revision += 1;
                    drop(w);
                    self.update_window_titles();
                    Ok(self.commit_runtime(
                        serde_json::to_value(updated).unwrap()))
                }
                "views.bindResources" => {
                    let _mount = self.mounting.lock().unwrap();
                    let id = required(params, "viewId")?;
                    let view = self
                        .workbench
                        .lock()
                        .unwrap()
                        .view(id)
                        .map_err(native_error)?;
                    if caller.plugin_id.as_deref() != Some(&view.plugin_id) {
                        return Err(err("SOURCE_INVALID", "资源绑定来源不匹配"));
                    }
                    let _worker = self.check_worker_operation(&view.workspace_id, params)?;
                    let resources = params["resources"]
                        .as_array()
                        .cloned()
                        .ok_or_else(|| err("INVALID_PARAMS", "资源引用无效"))?;
                    let cursors=self.core.bind_view_resources(&caller.instance_id,&[json!({"viewId":id,"workspaceId":view.workspace_id,"mountGeneration":view.mount_generation,"resources":resources})])?;
                    self.workbench
                        .lock()
                        .unwrap()
                        .resources
                        .insert(id.into(), resources);
                    if let Some(binding) = self.view_binding(id) {
                        *binding.resume.lock().unwrap() = cursors[id].clone();
                        let _ = self.native.notify(
                            binding.id.clone(),
                            "view.resourcesChanged",
                            json!({"viewId":id}),
                        );
                    }
                    Ok(json!({"bound":true,"cursor":cursors[id]}))
                }
                "workbench.operation.get" => {
                    let id = required(params, "operationId")?;
                    if let Some(op) = self.layout_operations.lock().unwrap().get(id) {
                        return Ok(op.result.lock().unwrap().clone());
                    }
                    if let Some(op) = self.pending_opens.lock().unwrap().get(id) {
                        return Ok(
                            json!({"operationId":id,"phase":if op.finished{"finished"}else{"running"},"waitingAuth":op.waiting_auth,"canceled":op.canceled,"viewId":op.view_id,"result":op.result.as_ref().and_then(|r|r.as_ref().ok()),"error":op.result.as_ref().and_then(|r|r.as_ref().err())}),
                        );
                    }
                    Err(err("OPERATION_EXPIRED", "操作记录已过期"))
                }
                "workbench.operation.cancel" => {
                    let id = required(params, "operationId")?;
                    if let Some(op) = self.layout_operations.lock().unwrap().get(id) {
                        let result = op.result.lock().unwrap();
                        if result["committed"] == true {
                            return Ok(json!({"canceled":false,"committed":true}));
                        }
                        op.canceled.store(true, Ordering::SeqCst);
                        return Ok(json!({"canceled":true}));
                    }
                    let active = {
                        let mut ops = self.pending_opens.lock().unwrap();
                        ops.retain(|_, o| {
                            !o.finished || o.created.elapsed() < Duration::from_secs(600)
                        });
                        if let Some(op) = ops.get_mut(id) {
                            if op.finished && !op.waiting_auth {
                                return Ok(json!({"canceled":false,"viewId":op.view_id}));
                            }
                            op.canceled = true;
                            op.waiting_auth = false;
                            true
                        } else {
                            if ops.len() >= 10000 {
                                return Err(err("CAPACITY", "操作记录已满"));
                            }
                            ops.insert(
                                id.into(),
                                PendingOpen {
                                    plugin_id: String::new(),
                                    generation: None,
                                    created: Instant::now(),
                                    server_id: None,
                                    window_id: window.clone(),
                                    command_id: String::new(),
                                    canceled: true,
                                    finished: true,
                                    view_id: None,
                                    request: Value::Null,
                                    result: None,
                                    waiting_auth: false,
                                },
                            );
                            false
                        }
                    };
                    if active {
                        self.worker("lifecycle.cancel", json!({"operationId":id}))?;
                    }
                    let notification =
                        json!({"type":"workbench.openCanceled","payload":{"operationId":id}});
                    for w in self.app.webview_windows().values() {
                        let _ = w.eval(&format!("window.__sodeckNotify?.({notification});"));
                    }
                    self.changed();
                    Ok(json!({"canceled":true}))
                }
                _ => Err(err("UNSUPPORTED_API", "未实现的工作台方法")),
            }
        })())
    }
    pub(super) fn apply_view_metadata(view: &mut View, params: &Value) -> Result<()> {
        if let Some(title) = params["title"].as_str() {
            if title.trim().is_empty()
                || title.chars().count() > 256
                || title.chars().any(char::is_control)
            {
                return Err(err("INVALID_PARAMS", "视图标题无效"));
            }
            view.title = title.into();
        }
        if let Some(status) = params["displayStatus"].as_str() {
            if ![
                "loading",
                "ready",
                "running",
                "ended",
                "error",
                "viewFailed",
            ]
            .contains(&status)
            {
                return Err(err("INVALID_PARAMS", "展示状态无效"));
            }
            view.status = status.into();
        }
        if let Some(actions) = params.get("actions") {
            view.actions = serde_json::from_value(actions.clone())
                .map_err(|_| err("INVALID_PARAMS", "视图动作无效"))?;
        }
        if let Some(state) = params.get("pluginState") {
            if serde_json::to_vec(state).unwrap_or_default().len() > 64 * 1024 {
                return Err(err("CAPACITY", "可恢复展示状态超限"));
            }
            view.plugin_state = state.clone();
        }
        Ok(())
    }
    fn validate_view_actions(&self, view: &View) -> Result<()> {
        let plugin = self.loaded_for(&view.plugin_id)?;
        if view.actions.iter().any(|id| {
            !plugin
                .package
                .manifest
                .contributes
                .commands
                .iter()
                .any(|c| &c.id == id)
        }) {
            return Err(err("INVALID_PARAMS", "视图动作引用了未登记命令"));
        }
        Ok(())
    }
    fn check_worker_operation(
        &self,
        workspace: &str,
        params: &Value,
    ) -> Result<std::sync::MutexGuard<'_, std::collections::BTreeSet<String>>> {
        let busy = self.workspace_busy.lock().unwrap();
        if busy.contains(workspace) {
            return Err(err("MOVE_IN_PROGRESS", "工作区正在移动或关闭"));
        }
        if let Some(id) = params["operationId"].as_str() {
            let (canceled, generation) = self.pending_opens.lock().unwrap().get(id)
                .map(|o| (o.canceled, o.generation.clone())).unwrap_or((true, None));
            let current = self.workbench.lock().unwrap().workspaces.get(workspace).map(|w| w.generation.clone());
            if canceled || generation != current {
                return Err(err("CANCELED", "操作已取消"));
            }
        }
        Ok(busy)
    }
    fn execute_command(
        self: &Arc<Self>,
        caller: &Caller,
        window: &str,
        params: &Value,
    ) -> Result<Value> {
        let loaded = self.command_plugin(caller,params)?;
        let command = required(params, "commandId")?;
        let declaration = loaded
            .package
            .manifest
            .contributes
            .commands
            .iter()
            .find(|c| c.id == command)
            .ok_or_else(|| err("UNSUPPORTED_API", "插件没有声明此命令"))?;
        if caller
            .plugin_id
            .as_ref()
            .is_some_and(|id| id != &loaded.package.manifest.plugin_id)
        {
            return Err(err("SOURCE_INVALID", "暂不开放插件互调"));
        }
        let view = params["viewId"]
            .as_str()
            .map(|id| {
                self.workbench
                    .lock()
                    .unwrap()
                    .view(id)
                    .map_err(native_error)
            })
            .transpose()?;
        if declaration.scope == "view" && view.is_none() {
            return Err(err("INVALID_PARAMS", "此命令需要视图"));
        }
        if view.as_ref().is_some_and(|v| {
            v.plugin_id != loaded.package.manifest.plugin_id
                || params["workspaceId"]
                    .as_str()
                    .is_some_and(|id| id != v.workspace_id)
        }) {
            return Err(err("SOURCE_INVALID", "命令视图和工作区不匹配"));
        }
        let workspace = if declaration.scope == "global" {
            "@app".to_string()
        } else {
            params["workspaceId"]
                .as_str()
                .map(str::to_string)
                .or_else(|| view.as_ref().map(|v| v.workspace_id.clone()))
                .or_else(|| (declaration.scope == "global").then(|| "@app".into()))
                .ok_or_else(|| err("INVALID_PARAMS", "缺少工作区"))?
        };
        if workspace == "@app" {
            self.workbench
                .lock()
                .unwrap()
                .workspaces
                .entry(workspace.clone())
                .or_default();
        }
        self.workbench
            .lock()
            .unwrap()
            .owner(&workspace)
            .map_err(native_error)?;
        let operation = params["operationId"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        {
            let busy = self.workspace_busy.lock().unwrap();
            if busy.contains(&workspace) {
                return Err(err("MOVE_IN_PROGRESS", "工作区正在移动或关闭"));
            }
            let generation = {
                let w = self.workbench.lock().unwrap();
                let ws = w.workspaces.get(&workspace).ok_or_else(|| err("WORKSPACE_CLOSED", "工作区已关闭"))?;
                if params["workspaceGeneration"].as_str().is_some_and(|g| g != ws.generation) {
                    return Err(err("CANCELED", "原工作区已关闭"));
                }
                if let Some(initializer) = params["initializerId"].as_str() {
                    if !ws.initializers.iter().any(|i| i.id == initializer && i.command_id == command && i.operation_id == operation) {
                        return Err(err("SOURCE_INVALID", "初始化动作与工作区不匹配"));
                    }
                } else if view.is_none() {
                    if let Some(init) = ws.initializers.iter().find(|i| i.id.starts_with(&format!("{}::",loaded.package.manifest.plugin_id)) && i.command_id == command && i.state == "running") {
                        return Ok(json!({"operationId":init.operation_id,"viewId":init.view_id,"pending":true}));
                    }
                }
                ws.generation.clone()
            };
            let mut ops = self.pending_opens.lock().unwrap();
            ops.retain(|_, o| !o.finished || o.created.elapsed() < Duration::from_secs(600));
            if let Some(op) = ops.get(&operation) {
                if op.canceled {
                    return Err(err("CANCELED", "操作已取消"));
                }
                if op.request != *params {
                    return Err(err("REQUEST_CONFLICT", "操作标识已用于其他请求"));
                }
                return op
                    .result
                    .clone()
                    .unwrap_or_else(|| Err(err("RESULT_UNKNOWN", "操作仍在处理中，请查询状态")));
            }
            if ops.len() >= 10000 {
                return Err(err("CAPACITY", "操作记录已满"));
            }
            ops.insert(
                operation.clone(),
                PendingOpen {
                    plugin_id: loaded.package.manifest.plugin_id.clone(),
                    generation: Some(generation),
                    created: Instant::now(),
                    server_id: Some(workspace.clone()),
                    window_id: window.into(),
                    command_id: command.into(),
                    canceled: false,
                    finished: false,
                    view_id: view.as_ref().map(|v| v.view_id.clone()),
                    request: params.clone(),
                    result: None,
                    waiting_auth: false,
                },
            );
        }
        self.changed();
        let server_id = (workspace != "@app").then_some(&workspace);
        let context = json!({"pluginId":loaded.package.manifest.plugin_id,"workspaceGeneration":self.workbench.lock().unwrap().workspaces.get(&workspace).map(|w|w.generation.clone()),"windowId":window,"workspaceId":workspace,"serverId":server_id,"viewId":view.as_ref().map(|v|&v.view_id),"pluginData":view.as_ref().map(|v|&v.runtime_data),"pluginState":view.as_ref().map(|v|&v.plugin_state),"title":view.as_ref().map(|v|&v.title),"instanceKey":view.as_ref().map(|v|&v.instance_key),"operationId":operation});
        let invoke=||self.worker("command.execute",json!({"commandId":command,"args":params.get("args").cloned().unwrap_or(json!({})),"context":context}));
        let mut result=if caller.plugin_id.is_none(){self.core.host_diagnostic_action(command,params.get("initializerId").is_none(),invoke)}else{invoke()};
        if self.pending_opens.lock().unwrap().get(&operation).is_some_and(|o| o.canceled) {
            result = Err(err("CANCELED", "操作已取消"));
        }
        {
            if let Some(op) = self.pending_opens.lock().unwrap().get_mut(&operation) {
                op.finished = true;
                op.result = Some(result.clone());
                op.waiting_auth = result.as_ref().err().is_some_and(|e| {
                    sodeck_core::is_connection_authentication_error(&e.code)
                }) && !op.canceled;
            }
        }
        if let Err(error) = &result {
            if sodeck_core::is_connection_authentication_error(&error.code)
                && !self
                    .pending_opens
                    .lock()
                    .unwrap()
                    .get(&operation)
                    .is_some_and(|o| o.canceled)
            {
                let server = self.core.call(
                    &Caller::shell(),
                    "servers.get",
                    json!({"serverId":workspace}),
                )?;
                let auth_server=if let Some(challenge)=error.details.as_ref().and_then(|d|d["challengeId"].as_str()) {
                    let resolved=self.core.call(&Caller::shell(),"auth.challenge.get",json!({"challengeId":challenge,"targetServerId":workspace}))?;
                    if resolved["workspaceId"]!=workspace || resolved["operationId"]!=operation {return Err(err("HOST_CHALLENGE_MISMATCH","认证操作与当前工作区不一致"));}
                    if resolved["ownerKind"]=="socks5"{server}else{resolved["server"].clone()}
                }else{server};
                let event = json!({"type":"auth.required","payload":{"server":auth_server,"workspaceId":workspace,"error":error,"operationId":operation,"commandId":command,"viewId":params["viewId"],"args":params["args"]}});
                if self.workbench.lock().unwrap().windows.get(window).is_some_and(|w|w.selected_workspace().map(String::as_str)==Some(workspace.as_str())) { if let Some(w) = self.app.get_webview_window(window) {
                    let _ = w.eval(&format!("window.__sodeckNotify?.({event});")); }
                }
            }
        }
        self.changed();
        result
    }
    pub(super) fn update_window_titles(&self) {
        let w = self.workbench.lock().unwrap();
        for window in self.app.webview_windows().values() {
            let workspace = if let Some(id) = window.label().strip_prefix("detached-") {
                w.views
                    .iter()
                    .find(|v| v.view_id == id)
                    .map(|v| (v.server_id.clone(), Some(v.title.clone())))
            } else {
                w.windows
                    .get(window.label())
                    .and_then(|win| win.selected_workspace().cloned())
                    .map(|id| (id, None))
            };
            if w.windows.get(window.label()).is_some_and(|r|r.selection==crate::window_registry::Selection::ConnectionHome){let _=window.set_title("新建连接 — SoDeck");continue;}
            if let Some((id, title)) = workspace {
                if let Ok(server) =
                    self.core
                        .call(&Caller::shell(), "servers.get", json!({"serverId":id}))
                {
                    let _ = window.set_title(&format!(
                        "{}{} — SoDeck",
                        server["name"].as_str().unwrap_or("服务器"),
                        title.map(|s| format!(" · {s}")).unwrap_or_default()
                    ));
                }
            }
        }
    }
    pub(super) fn close_workspace_window(self: &Arc<Self>, id: &str) -> Result<Value> {
        if id == "main" {
            if self.app.tray_by_id("sodeck").is_some() {
                if let Some(w) = self.app.get_webview_window(id) {
                    w.hide().map_err(|e| native_error(e.to_string()))?;
                }
            }
            return Ok(json!({"hidden":true}));
        }
        if let Some(view) = id.strip_prefix("detached-") {
            self.cancel_window_evidence_export(id);
            if let Err(error) = self.close_view(view) {
                if error.code == "MOVE_IN_PROGRESS" {
                    self.queue_native_close(view.into());
                    return Ok(json!({"queued":true}));
                }
                return Err(error);
            }
            return Ok(json!({"closed":true}));
        }
        let ids = self
            .workbench
            .lock()
            .unwrap()
            .windows
            .get(id)
            .ok_or_else(|| err("WINDOW_CLOSED", "窗口已关闭"))?
            .workspace_ids
            .clone();
        match self.close_workspaces(&ids) {
            Err(e) if e.code == "MOVE_IN_PROGRESS" => {
                self.queue_workspaces_close(id.into(), ids);
                Ok(json!({"queued":true}))
            }
            result => result,
        }
    }
    pub(super) fn queue_workspace_close(self: &Arc<Self>, window: &str) {
        let ids = self
            .workbench
            .lock()
            .unwrap()
            .windows
            .get(window)
            .map(|w| w.workspace_ids.clone())
            .unwrap_or_default();
        self.queue_workspaces_close(window.into(), ids);
    }
    fn queue_workspaces_close(self: &Arc<Self>, window_id: String, ids: Vec<String>) {
        if !self
            .queued_window_closes
            .lock()
            .unwrap()
            .insert(window_id.clone())
        {
            return;
        }
        let state = self.clone();
        std::thread::spawn(move || {
            loop {
                if state.stopping.load(Ordering::SeqCst) {
                    break;
                }
                match state.close_workspaces(&ids) {
                    Err(error) if error.code == "MOVE_IN_PROGRESS" => {
                        std::thread::sleep(Duration::from_millis(50))
                    }
                    Err(error) => {
                        state.warning(&json!({"code":error.code,"message":error.message}));
                        break;
                    }
                    Ok(_) => break,
                }
            }
            state
                .queued_window_closes
                .lock()
                .unwrap()
                .remove(&window_id);
        });
    }
    pub(super) fn close_workspaces(self: &Arc<Self>, ids: &[String]) -> Result<Value> {
        if self.begin_close_review(window_close_review::Action::Close(ids.to_vec()),None)?{return Err(err("CLOSE_REVIEW_REQUIRED","请先处理连接页草稿"));}
        let canceled = {
            let mut ops = self.pending_opens.lock().unwrap();
            ops.iter_mut()
                .filter(|(_, op)| {
                    (!op.finished || op.waiting_auth)
                        && !op.canceled
                        && op.server_id.as_ref().is_some_and(|id| ids.contains(id))
                })
                .map(|(id, op)| {
                    op.canceled = true;
                    op.waiting_auth = false;
                    (id.clone(), op.finished)
                })
                .collect::<Vec<_>>()
        };
        for (id, finished) in canceled {
            if !finished {
                self.worker("lifecycle.cancel", json!({"operationId":id}))?;
            }
            let event = json!({"type":"workbench.openCanceled","payload":{"operationId":id}});
            for window in self.app.webview_windows().values() {
                let _ = window.eval(&format!("window.__sodeckNotify?.({event});"));
            }
        }
        let _guard = self.workspace_guard(ids)?;
        let views = self
            .workbench
            .lock()
            .unwrap()
            .views
            .iter()
            .filter(|v| ids.contains(&v.workspace_id))
            .cloned()
            .collect::<Vec<_>>();
        {
            for v in &views {
                if self.loaded_for(&v.plugin_id).is_err(){continue;}
                let prepare=self.worker("lifecycle.prepareClose",json!({"viewId":v.view_id,"workspaceId":v.workspace_id,"pluginData":v.runtime_data}))?;
                if prepare["allow"] != true {
                    return Err(err("CANCELED", "关闭已取消"));
                }
            }
        }
        // A native save modal must finish before surface teardown needs the UI thread.
        let ending_windows={let w=self.workbench.lock().unwrap();w.windows.iter()
            .filter(|(id,window)|id.as_str()!="main"&&!window.workspace_ids.is_empty()&&window.workspace_ids.iter().all(|workspace|ids.contains(workspace)))
            .map(|(id,_)|id.clone()).collect::<Vec<_>>()};
        for window in ending_windows{self.cancel_window_evidence_export(&window);}
        {let w=self.workbench.lock().unwrap();for id in ids{if let Some(ws)=w.workspaces.get(id){self.core.revoke_workspace(id,&ws.generation);}}}
        let mut results = vec![];
        for v in &views {
            let result = self.close_reserved_view(&v.view_id);
            results.push(match result {
                Ok(()) => json!({"viewId":v.view_id,"closed":true}),
                Err(e) => json!({"viewId":v.view_id,"closed":false,"error":e}),
            });
        }
        {
            let mut w = self.workbench.lock().unwrap();
            for id in ids {
                if !w.views.iter().any(|v| v.workspace_id == *id) {
                    let generation=w.workspaces.get(id).map(|v|v.generation.clone());w.remove_workspace(id).map_err(native_error)?;
                    if let Some(generation)=generation{if let Ok(target)=self.core.observation_target("workspace",id,Some(&generation)){let _=self.core.observe_host_event(&Caller::shell(),"core.workspace.closed",json!({"objectRef":target["ref"],"generation":generation}));}}
                }
            }
        }
        self.reap_empty_windows();
        Ok(self.commit_runtime(json!({"results":results})))
    }
    pub(super) fn reap_empty_windows(&self) {
        let empty = {
            let mut w = self.workbench.lock().unwrap();
            let empty = w
                .windows
                .iter()
                .filter(|(id, win)| id.as_str() != "main" && win.workspace_ids.is_empty())
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>();
            for id in &empty {
                w.windows.remove(id);
            }
            empty
        };
        for id in empty {
            {let mut registry=self.connection_home.lock().unwrap();registry.sessions.remove(&id);registry.receipts.retain(|(window,_),_|window!=&id);}
            if let Some(win) = self.app.get_webview_window(&id) {
                // Let the initiating shell receive its committed result first.
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(300));
                    let _ = win.destroy();
                });
            }
        }
    }
}

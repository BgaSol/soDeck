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
        if self.pending_opens.lock().unwrap().iter().any(|(id, o)| {
            Some(id.as_str()) != own_operation
                && (!o.finished || o.waiting_auth)
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
            if let Some(id) = source.strip_prefix("@window:") {
                return Ok(id.into());
            }
            return Ok(self.binding(source)?.window.clone());
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
            if let Ok(port) = std::env::var("SODECK_SHELL_DEBUG_PORT") {
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
                | "workbench.persistence.retry"
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
                "workbench.persistence.retry" => {
                    self.workbench.lock().unwrap().retry_save(&self.data.join("workbench.json")).map_err(native_error)?;
                    self.changed();Ok(json!({"persisted":true}))
                }
                "workbench.snapshot" => {
                    let target = params["targetWindowId"].as_str().unwrap_or(&window);
                    let mut snapshot = self.workbench.lock().unwrap().snapshot(target);
                    snapshot["commands"] = self
                        .loaded()
                        .map(|l| {
                            serde_json::to_value(&l.package.manifest.contributes.commands).unwrap()
                        })
                        .unwrap_or(json!([]));
                    snapshot["viewDeclarations"] = self
                        .loaded()
                        .map(|l| {
                            json!(l
                                .package
                                .manifest
                                .contributes
                                .views
                                .iter()
                                .map(|d| {
                                    let locations =
                                        ["center", "left", "right", "bottom", "detached"]
                                            .into_iter()
                                            .filter(|p| {
                                                let host = match *p {
                                                    "center" => "tab",
                                                    "detached" => "window",
                                                    _ => "dock",
                                                };
                                                d.supported_hosts.iter().any(|h| h == host)
                                                    && (host != "dock"
                                                        || d.dock_edges.iter().any(|e| e == p))
                                            })
                                            .collect::<Vec<_>>();
                                    json!({"id":d.id,"locations":locations})
                                })
                                .collect::<Vec<_>>())
                        })
                        .unwrap_or(json!([]));
                    snapshot["pendingOpens"]=json!(self.pending_opens.lock().unwrap().iter().filter(|(_,o)|!o.finished&&o.window_id==target).map(|(id,o)|json!({"operationId":id,"serverId":o.server_id,"commandId":o.command_id,"cancelling":o.canceled})).collect::<Vec<_>>());
                    Ok(snapshot)
                }
                "workbench.configure" => {
                    let mut w = self.workbench.lock().unwrap();
                    let mut next = w.clone();
                    next.configure(&window, params).map_err(native_error)?;
                    w.commit(next, &self.data.join("workbench.json"))
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
                    self.native
                        .modal(window, params["open"].as_bool().unwrap_or(false))
                        .map_err(native_error)?;
                    Ok(Value::Null)
                }
                "workspaces.open" => {
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
                        return Ok(self.save_committed(
                            json!({"workspaceId":id,"ownerWindowId":owner,"created":false}),
                            "位置已定位，但布局未保存",
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
                    if let Ok(plugin) = self.loaded() {
                        let mut w = self.workbench.lock().unwrap();
                        for entry in &plugin.package.manifest.contributes.workspace_entries {
                            if entry.open_by_default {
                                w.open_view(
                                    &plugin.package.manifest.plugin_id,
                                    &entry.view_type,
                                    id,
                                    "",
                                )
                                .map_err(native_error)?;
                            }
                        }
                    }
                    self.show_window(&target);
                    Ok(self.save_committed(
                        json!({"workspaceId":id,"ownerWindowId":target,"created":true}),
                        "工作区已打开，但布局未保存",
                    ))
                }
                "workspaces.activate" => {
                    let id = required(params, "workspaceId")?;
                    let owner = {
                        let mut w = self.workbench.lock().unwrap();
                        w.activate_workspace(id).map_err(native_error)?;
                        w.owner(id).map_err(native_error)?
                    };
                    self.show_window(&owner);
                    Ok(self.save_committed(
                        json!({"workspaceId":id,"ownerWindowId":owner}),
                        "切换已完成，布局未保存",
                    ))
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
                    Ok(self.save_committed(json!({"reordered":true}), "排序已完成，布局未保存"))
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
                        (win.workspace_ids.clone(), win.active_workspace_id.clone())
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
                            for w in self.app.webview_windows().values() {
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
                        (op.request.clone(), op.window_id.clone())
                    };
                    request["operationId"] = json!(Uuid::new_v4().to_string());
                    self.execute_command(caller, &origin, &request)
                }
                "views.open" => {
                    let plugin = self.loaded()?;
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
                    Ok(self.save_committed(
                        serde_json::to_value(view).unwrap(),
                        "视图已打开，但布局未保存",
                    ))
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
                    Ok(self.save_committed(
                        serde_json::to_value(updated).unwrap(),
                        "视图已更新，但布局未保存",
                    ))
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
                            if op.finished && op.view_id.is_some() {
                                return Ok(json!({"canceled":false,"viewId":op.view_id}));
                            }
                            op.canceled = true;
                            op.waiting_auth = false;
                            !op.finished
                        } else {
                            if ops.len() >= 10000 {
                                return Err(err("CAPACITY", "操作记录已满"));
                            }
                            ops.insert(
                                id.into(),
                                PendingOpen {
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
        let plugin = self.loaded()?;
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
            if self
                .pending_opens
                .lock()
                .unwrap()
                .get(id)
                .is_some_and(|o| o.canceled)
            {
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
        let loaded = self.loaded()?;
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
                    created: Instant::now(),
                    server_id: Some(workspace.clone()),
                    window_id: window.into(),
                    command_id: command.into(),
                    canceled: false,
                    finished: false,
                    view_id: None,
                    request: params.clone(),
                    result: None,
                    waiting_auth: false,
                },
            );
        }
        self.changed();
        let server_id = (workspace != "@app").then_some(&workspace);
        let context = json!({"windowId":window,"workspaceId":workspace,"serverId":server_id,"viewId":view.as_ref().map(|v|&v.view_id),"pluginData":view.as_ref().map(|v|&v.runtime_data),"pluginState":view.as_ref().map(|v|&v.plugin_state),"title":view.as_ref().map(|v|&v.title),"instanceKey":view.as_ref().map(|v|&v.instance_key),"operationId":operation});
        let mut result=self.worker("command.execute",json!({"commandId":command,"args":params.get("args").cloned().unwrap_or(json!({})),"context":context}));
        let canceled_view = {
            let ops = self.pending_opens.lock().unwrap();
            ops.get(&operation)
                .filter(|op| op.canceled)
                .map(|op| op.view_id.clone())
        };
        if let Some(view) = canceled_view {
            if let Some(id) = view {
                let exists = self.workbench.lock().unwrap().view(&id).is_ok();
                if exists {
                    self.close_view_for_operation(&id, Some(&operation))?;
                }
            }
            result = Err(err("CANCELED", "操作已取消"));
        }
        {
            if let Some(op) = self.pending_opens.lock().unwrap().get_mut(&operation) {
                op.finished = true;
                op.result = Some(result.clone());
                op.waiting_auth = result.as_ref().err().is_some_and(|e| {
                    [
                        "HOST_KEY_REQUIRED",
                        "HOST_KEY_CHANGED",
                        "AUTH_REQUIRED",
                        "AUTH_FAILED",
                    ]
                    .contains(&e.code.as_str())
                }) && !op.canceled;
            }
        }
        if let Err(error) = &result {
            if [
                "HOST_KEY_REQUIRED",
                "HOST_KEY_CHANGED",
                "AUTH_REQUIRED",
                "AUTH_FAILED",
            ]
            .contains(&error.code.as_str())
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
                let event = json!({"type":"auth.required","payload":{"server":server,"error":error,"operationId":operation,"commandId":command,"viewId":params["viewId"],"args":params["args"]}});
                if let Some(w) = self.app.get_webview_window(window) {
                    let _ = w.eval(&format!("window.__sodeckNotify?.({event});"));
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
                    .and_then(|win| win.active_workspace_id.clone())
                    .map(|id| (id, None))
            };
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
    fn close_workspaces(self: &Arc<Self>, ids: &[String]) -> Result<Value> {
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
        if self.loaded().is_ok() {
            for v in &views {
                let prepare=self.worker("lifecycle.prepareClose",json!({"viewId":v.view_id,"workspaceId":v.workspace_id,"pluginData":v.runtime_data}))?;
                if prepare["allow"] != true {
                    return Err(err("CANCELED", "关闭已取消"));
                }
            }
        }
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
                    w.remove_workspace(id).map_err(native_error)?;
                }
            }
        }
        self.reap_empty_windows();
        Ok(self.save_committed(json!({"results":results}), "关闭已执行，但布局未保存"))
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

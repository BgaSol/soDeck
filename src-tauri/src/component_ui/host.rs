//! Native plugin resources, RPC requests and mounted UI surfaces.
use super::*;

impl AppCore {
    pub(crate) fn surface_write_admission(
        &self,
        window: &str,
        token: &str,
    ) -> Result<std::sync::MutexGuard<'_, crate::workbench::State>, String> {
        let state = self.workbench_state();
        let s = state
            .ui
            .surfaces
            .get(token)
            .filter(|s| s.window == window && s.ready && s.phase == SurfacePhase::Active)
            .ok_or("界面输入权已改变")?;
        let v = state.ui.resource_view(&s.view)?;
        state.workspaces.validate_identity(&v.context)?;
        if state.ui.workspaces[&v.context.workspace_id]
            .view_window(&v.id)
            .as_deref()
            != Some(window)
            || !state.workspaces.contains(window, &v.context.workspace_id)
            || state.ui.lifecycle.view_frozen(&v.id)
                && !state.ui.lifecycle.view_input_allowed(&v.id, window)
        {
            return Err("页面已移动或正在关闭".into());
        }
        Ok(state)
    }
    pub(crate) fn surface_call_admission(
        &self,
        window: &str,
        token: &str,
    ) -> Result<(View, crate::resource_tasks::Task), String> {
        let state = self.surface_write_admission(window, token)?;
        let surface = &state.ui.surfaces[token];
        let view = state.ui.resource_view(&surface.view)?.clone();
        let task = view.calls.enter()?;
        Ok((view, task))
    }
    pub fn ui_detach(&self, window: &str, token: &str) {
        let mut ui = self.ui_state();
        if ui.surfaces.get(token).is_some_and(|s| s.window == window) {
            ui.surfaces.remove(token);
            self.shared_state
                .remove_surface(window, token, std::time::Instant::now());
        }
    }
    pub(crate) fn ui_ready_surface(&self, view: &str) -> Option<(String, String)> {
        self.ui_state()
            .surfaces
            .iter()
            .find(|(_, s)| s.view == view && s.ready)
            .map(|(token, s)| (s.window.clone(), token.clone()))
    }
    pub(crate) fn ui_data_surface_ids(&self) -> HashSet<String> {
        self.ui_state().surfaces.keys().cloned().collect()
    }
    pub(crate) fn ui_data_view(&self, window: &str, token: &str) -> Result<View, String> {
        let ui = self.ui_state();
        let s = ui
            .surfaces
            .get(token)
            .filter(|s| s.window == window)
            .ok_or("界面挂载已失效")?;
        Ok(ui.view(&s.view)?.clone())
    }
    pub(crate) fn ui_data_writable(&self, window: &str, token: &str) -> Result<View, String> {
        let state = self.surface_write_admission(window, token)?;
        Ok(state
            .ui
            .resource_view(&state.ui.surfaces[token].view)?
            .clone())
    }
    pub fn ui_process_update(
        &self,
        plugin: &str,
        instance: &str,
        params: &Value,
    ) -> Result<Value, String> {
        let id = params["viewId"].as_str().ok_or("缺少 viewId")?;
        let mut ui = self.ui_state();
        let v = ui.view(id)?;
        if v.plugin_id != plugin || v.plugin_instance_id != instance {
            return Err("组件不属于此插件实例".into());
        }
        if ui.lifecycle.view_frozen(&v.id) {
            return Err("工作区正在确认关闭".into());
        }
        ui.update_metadata(id, &params["metadata"])?;
        drop(ui);
        self.workspace_changed();
        Ok(json!({"accepted":true}))
    }
    pub(crate) fn ui_plugin_call(
        &self,
        view: &View,
        method: &str,
        data: Value,
    ) -> Result<Value, String> {
        self.ui_plugin_call_observed(view, method, data, None, std::time::Duration::from_secs(5))
    }
    pub(crate) fn ui_create_view(&self, view: &View) -> Result<(), String> {
        let version = self
            .ui_state()
            .definitions
            .get(&view.definition_key)
            .ok_or("组件未登记")?
            .ui_api_version
            .clone();
        if version != "2" {
            return Ok(());
        }
        let mut creation = view.creation.lock().unwrap();
        if let Some(result) = &*creation {
            return result.clone();
        }
        let result = self
            .ui_plugin_call(view, "ui.createView", Value::Null)
            .map(|_| ());
        *creation = Some(result.clone());
        result
    }
    pub(crate) fn ui_plugin_call_observed(
        &self,
        view: &View,
        method: &str,
        data: Value,
        receipt: Option<crate::plugins::Receipt>,
        timeout: std::time::Duration,
    ) -> Result<Value, String> {
        let instance = self
            .instances
            .lock()
            .unwrap()
            .get(&view.plugin_id)
            .cloned()
            .ok_or("插件未运行")?;
        if instance.view().instance_id.as_ref() != Some(&view.plugin_instance_id) {
            return Err("插件实例已更换，请重新打开组件".into());
        }
        let task = {
            let ui = self.ui_state();
            let current = ui.resource_view(&view.id)?;
            if current.context != view.context
                || current.plugin_instance_id != view.plugin_instance_id
            {
                return Err("组件上下文已改变".into());
            }
            if method == "ui.event"
                && ui
                    .surfaces
                    .values()
                    .filter(|s| s.view == view.id)
                    .all(|s| s.phase != SurfacePhase::Active)
            {
                return Err("界面输入权已改变".into());
            }
            if matches!(method, "ui.mount" | "ui.event") && ui.lifecycle.view_frozen(&view.id) {
                let input = method == "ui.event"
                    && ui.surfaces.values().any(|s| {
                        s.view == view.id && ui.lifecycle.view_input_allowed(&view.id, &s.window)
                    });
                if !input {
                    return Err("组件正在关闭".into());
                }
            }
            view.calls.enter()?
        };
        instance.request_observed(method, json!({"viewId":view.id,"viewType":view.definition_key.split(':').nth(1),"context":view.context,"resourceKey":view.resource_key,"params":view.params,"data":data}),Some(task),receipt, timeout)
    }
    pub fn ui_attach(&self, window: &str, id: &str) -> Result<Value, String> {
        let (view, candidate, version) = {
            let state = self.workbench_state();
            let ui = &state.ui;
            let view = ui.view(id)?.clone();
            let active = ui.workspaces[&view.context.workspace_id]
                .view_window(id)
                .as_deref()
                == Some(window)
                || ui.definitions[&view.definition_key].definition.container == "dialog"
                    && state
                        .workspaces
                        .contains(window, &view.context.workspace_id);
            let candidate = ui
                .transfers
                .values()
                .any(|t| t.movement.target == window && t.views.iter().any(|v| v == id))
                || state
                    .workspaces
                    .pending_for_workspace(window, &view.context.workspace_id)
                    .is_some_and(|m| {
                        m.workspace_id == view.context.workspace_id
                            && ui.workspaces[&m.workspace_id]
                                .host(&m.source)
                                .is_ok_and(|h| h.contains(id))
                    });
            if !active && !candidate {
                return Err("组件不属于此窗口".into());
            }
            let version = ui.definitions[&view.definition_key].ui_api_version.clone();
            (view, !active, version)
        };
        let token = self.next_id();
        if !candidate {
            self.ui_create_view(&view)?;
        }
        let data = self.ui_plugin_call(
            &view,
            if version == "2" {
                "ui.getView"
            } else {
                "ui.mount"
            },
            Value::Null,
        )?;
        let mut _workbench = self.workbench_state();
        let crate::workbench::State { workspaces, ui, .. } = &mut *_workbench;
        if !workspaces.contains(window, &view.context.workspace_id)
            && !ui
                .transfers
                .values()
                .any(|t| t.movement.target == window && t.views.contains(&view.id))
            && workspaces
                .pending_for_workspace(window, &view.context.workspace_id)
                .is_none_or(|m| m.workspace_id != view.context.workspace_id)
        {
            return Err("挂载期间工作区已移动或关闭".into());
        }

        if self.is_closing()
            || ui.lifecycle.window_closing(window)
            || (!candidate && ui.lifecycle.view_frozen(&view.id))
            || !ui.resource_view(id).is_ok_and(|v| {
                v.context == view.context && v.plugin_instance_id == view.plugin_instance_id
            })
        {
            return Err("组件已关闭或上下文已改变".into());
        }
        if !candidate {
            ui.update_metadata(id, &data["metadata"])?;
        }
        ui.surfaces
            .retain(|_, s| s.window != window || s.view != id);
        ui.surfaces.insert(
            token.clone(),
            Surface {
                window: window.into(),
                view: id.into(),
                ready: false,
                phase: if candidate {
                    SurfacePhase::Candidate
                } else {
                    SurfacePhase::Active
                },
            },
        );
        Ok(
            json!({"uiApiVersion":version,"inputEpoch":token,"surfaceId":token,"view":view,"data":data["data"],"readOnly":candidate,"capabilities":{"workbench":2,"sharedState":1,"fileLocation":1,"fileLocationProof":1,"uiOpenView":1}}),
        )
    }
    pub fn ui_surface_ready(&self, window: &str, token: &str) -> Result<(), String> {
        let mut ui = self.ui_state();
        let s = ui
            .surfaces
            .get_mut(token)
            .filter(|s| s.window == window)
            .ok_or("界面挂载已失效")?;
        s.ready = true;
        Ok(())
    }
    pub fn ui_event(&self, window: &str, token: &str, event: Value) -> Result<Value, String> {
        if serde_json::to_vec(&event).map_err(|e| e.to_string())?.len() > 24 * 1024 {
            return Err("UI 事件超过 24 KiB；该接口不用于文件或终端数据流".into());
        }
        let (view, _call) = self.surface_call_admission(window, token)?;
        let response = self.ui_plugin_call(&view, "ui.event", event)?;
        let changed = self
            .ui_state()
            .update_metadata(&view.id, &response["metadata"])?;
        if changed {
            self.workspace_changed();
        }
        Ok(response["data"].clone())
    }
    pub fn ui_resource(
        &self,
        plugin: &str,
        relative: &str,
    ) -> Result<(Vec<u8>, &'static str), String> {
        let instance = self
            .instances
            .lock()
            .unwrap()
            .get(plugin)
            .cloned()
            .ok_or("插件未运行")?;
        let p = instance.view();
        if p.status != PluginStatus::Running || p.manifest.ui.is_none() {
            return Err("插件界面不可用".into());
        }
        let path = resource(&p.directory, relative)?;
        if fs::metadata(&path).map_err(|e| e.to_string())?.len() > 16 * 1024 * 1024 {
            return Err("单个界面资源超过 16 MiB".into());
        }
        let mime = match path.extension().and_then(|s| s.to_str()).unwrap_or("") {
            "html" => "text/html; charset=utf-8",
            "js" | "mjs" => "text/javascript; charset=utf-8",
            "css" => "text/css; charset=utf-8",
            "svg" => "image/svg+xml",
            "png" => "image/png",
            "woff2" => "font/woff2",
            "json" => "application/json",
            _ => "application/octet-stream",
        };
        Ok((fs::read(path).map_err(|e| e.to_string())?, mime))
    }
}

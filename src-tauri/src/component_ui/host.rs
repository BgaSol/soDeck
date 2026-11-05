//! Native plugin resources, RPC requests and mounted UI surfaces.
use super::*;

impl AppCore {
    pub(crate) fn ui_ready_surface(&self, view: &str) -> Option<(String, String)> {
        self.component_ui
            .lock()
            .unwrap()
            .surfaces
            .iter()
            .find(|(_, s)| s.view == view && s.ready)
            .map(|(token, s)| (s.window.clone(), token.clone()))
    }
    pub(crate) fn ui_data_surface_ids(&self) -> HashSet<String> {
        self.component_ui
            .lock()
            .unwrap()
            .surfaces
            .keys()
            .cloned()
            .collect()
    }
    pub(crate) fn ui_data_view(&self, window: &str, token: &str) -> Result<View, String> {
        let ui = self.component_ui.lock().unwrap();
        let s = ui
            .surfaces
            .get(token)
            .filter(|s| s.window == window)
            .ok_or("界面挂载已失效")?;
        Ok(ui.view(&s.view)?.clone())
    }
    pub(crate) fn ui_data_writable(&self, window: &str, token: &str) -> Result<View, String> {
        let view = {
            let ui = self.component_ui.lock().unwrap();
            let s = ui
                .surfaces
                .get(token)
                .filter(|s| s.window == window && s.ready)
                .ok_or("界面尚未就绪或挂载已失效")?;
            let v = ui.view(&s.view)?;
            if ui.frozen(&v.context.workspace_id) {
                return Err("工作区正在确认关闭".into());
            }
            v.clone()
        };
        self.workspaces
            .lock()
            .unwrap()
            .validate_owned(window, &view.context)?;
        Ok(view)
    }
    pub fn ui_process_update(
        &self,
        plugin: &str,
        instance: &str,
        params: &Value,
    ) -> Result<Value, String> {
        let id = params["viewId"].as_str().ok_or("缺少 viewId")?;
        let mut ui = self.component_ui.lock().unwrap();
        let v = ui.view(id)?;
        if v.plugin_id != plugin || v.plugin_instance_id != instance {
            return Err("组件不属于此插件实例".into());
        }
        if ui.frozen(&v.context.workspace_id) {
            return Err("工作区正在确认关闭".into());
        }
        ui.update_metadata(id, &params["metadata"])?;
        drop(ui);
        self.workspace_changed();
        Ok(json!({"accepted":true}))
    }
    pub(super) fn ui_plugin_call(
        &self,
        view: &View,
        method: &str,
        data: Value,
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
        instance.request(method, json!({"viewId":view.id,"viewType":view.definition_key.split(':').nth(1),"context":view.context,"resourceKey":view.resource_key,"params":view.params,"data":data}))
    }
    pub fn ui_attach(&self, window: &str, id: &str) -> Result<Value, String> {
        let projection = self.workspace_projection(window)?;
        let view = self.component_ui.lock().unwrap().view(id)?.clone();
        if !projection
            .workspaces
            .iter()
            .chain(projection.incoming.iter())
            .any(|w| w.context == view.context)
        {
            return Err("组件不属于此窗口".into());
        }
        let token = self.next_id();
        let data = self.ui_plugin_call(&view, "ui.mount", Value::Null)?;
        let mut ui = self.component_ui.lock().unwrap();
        ui.update_metadata(id, &data["metadata"])?;
        ui.surfaces
            .retain(|_, s| s.window != window || s.view != id);
        ui.surfaces.insert(
            token.clone(),
            Surface {
                window: window.into(),
                view: id.into(),
                ready: false,
            },
        );
        Ok(json!({"surfaceId":token,"view":view,"data":data["data"]}))
    }
    pub fn ui_surface_ready(&self, window: &str, token: &str) -> Result<(), String> {
        let mut ui = self.component_ui.lock().unwrap();
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
        let view = {
            let ui = self.component_ui.lock().unwrap();
            let s = ui
                .surfaces
                .get(token)
                .filter(|s| s.window == window && s.ready)
                .ok_or("界面挂载已失效")?;
            let v = ui.view(&s.view)?;
            if ui.frozen(&v.context.workspace_id) {
                return Err("工作区正在确认关闭".into());
            }
            v.clone()
        };
        self.workspaces
            .lock()
            .unwrap()
            .validate_owned(window, &view.context)?;
        let response = self.ui_plugin_call(&view, "ui.event", event)?;
        self.component_ui
            .lock()
            .unwrap()
            .update_metadata(&view.id, &response["metadata"])?;
        self.workspace_changed();
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

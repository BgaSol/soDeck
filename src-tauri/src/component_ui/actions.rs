//! Declared UI behaviors adapt to commands; own-page opens retain their original contract.
use super::*;
use sha2::{Digest, Sha256};
fn legacy_command(mut result: Value) -> Value {
    if result["status"] == "unknown" {
        result["status"] = json!("invalidated");
    }
    if let Some(message) = result["error"]["message"].as_str() {
        result["error"] = json!(message);
    }
    result
}

pub(super) struct OpenRequest {
    context: Context,
    fingerprint: Vec<u8>,
    pub view_id: String,
    pub closed: bool,
    bytes: usize,
}
fn bounded(value: &Value, limit: usize) -> Result<usize, String> {
    let size = serde_json::to_vec(value).map_err(|e| e.to_string())?.len();
    if size > limit {
        return Err(format!("UI 参数超过 {} KiB", limit / 1024));
    }
    Ok(size)
}
fn request_id(p: &Value) -> Result<&str, String> {
    p["requestId"]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 128)
        .ok_or_else(|| "requestId 必须为 1 至 128 字节的字符串".into())
}
fn fingerprint(value: Value) -> Vec<u8> {
    Sha256::digest(serde_json::to_vec(&value).unwrap()).to_vec()
}
impl Runtime {
    pub(super) fn retain_request_contexts(
        &mut self,
        contexts: &[Context],
        workspace: Option<&str>,
    ) {
        self.open_requests.retain(|_, r| {
            workspace.is_some_and(|id| r.context.workspace_id != id)
                || contexts.contains(&r.context)
        });
    }
    pub(crate) fn open_request_usage(&self, context: &Context) -> (usize, usize, usize) {
        (
            self.open_requests
                .values()
                .filter(|r| r.context == *context)
                .count(),
            self.open_requests.len(),
            self.open_requests.values().map(|r| r.bytes + 128).sum(),
        )
    }
    fn reserve_request(
        &self,
        context: &Context,
        bytes: usize,
        result_budget: usize,
        other: (usize, usize, usize),
    ) -> Result<(), String> {
        let (count, total, used) = self.open_request_usage(context);
        if count + other.0 >= 4096
            || total + other.1 >= 16384
            || used + other.2 + bytes + result_budget > 16 * 1024 * 1024
        {
            return Err("工作区创建请求记录已满，请关闭并重新打开工作区".into());
        }
        Ok(())
    }
}
impl AppCore {
    pub fn ui_invoke_action(&self, window: &str, p: &Value) -> Result<Value, String> {
        let action = self
            .ui_state()
            .actions
            .get(p["actionKey"].as_str().unwrap_or_default())
            .cloned()
            .ok_or("行为未登记")?;
        let mut params = p.clone();
        params["op"] = json!("submit");
        params["name"] = json!(action
            .definition
            .command
            .as_deref()
            .unwrap_or("ui.action.v1"));
        params["payload"] = p.get("data").cloned().unwrap_or(Value::Null);
        self.command_host_request(window, &params)
            .map(legacy_command)
            .map_err(|e| e.to_string())
    }
    pub fn ui_action_status(&self, window: &str, p: &Value) -> Result<Value, String> {
        let mut params = p.clone();
        params["op"] = json!("get");
        self.command_host_request(window, &params)
            .map(legacy_command)
            .map_err(|e| e.to_string())
    }
    pub fn ui_process_open(
        &self,
        plugin: &str,
        instance: &str,
        p: &Value,
    ) -> Result<Value, String> {
        if let Some(input) = self.command_legacy_open(plugin, instance, p) {
            return self
                .command_open(plugin, instance, &input)
                .map_err(|e| e.to_string())
                .and_then(|result| {
                    if result["closed"] == true {
                        Err("viewClosed：原页面已关闭".into())
                    } else {
                        Ok(result)
                    }
                });
        }
        if p.get("commandId").is_some() {
            return self
                .command_open(plugin, instance, p)
                .map_err(|e| e.to_string());
        }
        bounded(p, 24 * 1024)?;
        let context: Context =
            serde_json::from_value(p["expectedContext"].clone()).map_err(|e| e.to_string())?;
        let request = request_id(p)?;
        let view_type = p["viewType"].as_str().ok_or("缺少 viewType")?;
        let resource = p
            .get("resourceKey")
            .map(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .ok_or("resourceKey 必须为字符串")
            })
            .transpose()?;
        let params = p.get("params").cloned().unwrap_or(Value::Null);
        let bytes = bounded(&params, 16 * 1024)?;
        let input = fingerprint(json!([view_type, resource, params, p["sourceViewId"]]));
        let open_key = json!([context, plugin, instance, request]).to_string();
        let mut _workbench = self.workbench_state();
        let crate::workbench::State {
            workspaces,
            ui,
            commands,
        } = &mut *_workbench;
        workspaces.validate_identity(&context)?;

        let actual = self
            .instances
            .lock()
            .unwrap()
            .get(plugin)
            .map(|i| i.view())
            .ok_or("插件未运行")?;
        if actual.status != PluginStatus::Running || actual.instance_id.as_deref() != Some(instance)
        {
            return Err("插件实例已失效".into());
        }
        if let Some(r) = ui.open_requests.get(&open_key) {
            if r.fingerprint != input {
                return Err("requestId 已用于不同的页面参数".into());
            }
            if r.closed {
                return Err("viewClosed：原页面已关闭".into());
            }
            return Ok(json!({"viewId":r.view_id}));
        }
        workspaces.validate_context(&context)?;
        let mut parent = None;
        let (owner, group) = if let Some(source) = p["sourceViewId"].as_str() {
            let v = ui.resource_view(source)?;
            if v.context != context
                || v.plugin_id != plugin
                || v.plugin_instance_id != instance
                || ui.lifecycle.view_frozen(source)
            {
                return Err("创建来源页面已失效".into());
            }
            let w = &ui.workspaces[&context.workspace_id];
            let window = w.view_window(source).ok_or("创建来源页面没有显示位置")?;
            parent = Some(source.to_owned());
            (window.clone(), w.host(&window)?.group_of(source))
        } else {
            (
                workspaces
                    .owner(&context.workspace_id)
                    .ok_or("工作区已关闭")?,
                None,
            )
        };
        if self.is_closing()
            || ui.frozen(&context.workspace_id)
            || ui.lifecycle.host_closing(&context.workspace_id, &owner)
        {
            return Err("发起工作台正在关闭".into());
        }
        ui.reserve_request(&context, bytes, 128, commands.usage(&context))?;
        let definition = format!("{plugin}:{view_type}");
        if ui
            .definitions
            .get(&definition)
            .is_none_or(|d| d.plugin_instance_id != instance)
        {
            return Err("视图不属于当前插件实例".into());
        }
        if ui.definitions[&definition]
            .definition
            .create_action_id
            .is_some()
        {
            return Err("命令视图必须通过命令处理者登记".into());
        }
        let policy = &ui.definitions[&definition].definition.instance_policy;
        if ui.workspaces.get(&context.workspace_id).is_some_and(|w| {
            w.views.iter().any(|v| {
                v.definition_key == definition
                    && v.plugin_instance_id != instance
                    && (policy == "perWorkspace"
                        || (policy == "perResource" && v.resource_key == resource))
            })
        }) {
            return Err("原页面属于已停止的插件实例，请先关闭原页面".into());
        }
        let id = ui.open_view(
            &context,
            &owner,
            group.as_deref(),
            &definition,
            resource,
            params,
            self.next_id(),
            false,
        )?;
        if ui.definitions[&definition].definition.container == "dialog" {
            if let Some(v) = ui
                .workspaces
                .get_mut(&context.workspace_id)
                .unwrap()
                .views
                .iter_mut()
                .find(|v| v.id == id)
            {
                v.parent_view_id = parent;
            }
        }
        ui.open_requests.insert(
            open_key,
            OpenRequest {
                context,
                fingerprint: input,
                view_id: id.clone(),
                closed: false,
                bytes,
            },
        );
        self.sync_shared_ui_locked(&ui);
        drop(_workbench);
        self.workspace_changed();
        Ok(json!({"viewId":id}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn limits_keep_old_dedup_records_and_context_end_reclaims_them() {
        let mut ui = Runtime::open(&std::env::temp_dir().join("sodeck-action-quota-unit")).unwrap();
        let context = Context {
            workspace_id: "w".into(),
            server_id: "s".into(),
            connection_id: "c".into(),
            generation: 1,
        };
        for n in 0..4096 {
            ui.open_requests.insert(
                n.to_string(),
                OpenRequest {
                    context: context.clone(),
                    fingerprint: vec![],
                    view_id: n.to_string(),
                    closed: true,
                    bytes: 2,
                },
            );
        }
        assert!(ui.reserve_request(&context, 2, 128, (0, 0, 0)).is_err());
        assert_eq!(ui.open_requests.len(), 4096);
        ui.retain_request_contexts(&[], None);
        assert!(ui.open_requests.is_empty());
        ui.open_requests.insert(
            "large".into(),
            OpenRequest {
                context: context.clone(),
                fingerprint: vec![],
                view_id: "v".into(),
                closed: false,
                bytes: 16 * 1024 * 1024,
            },
        );
        assert!(ui
            .reserve_request(&context, 2, 64 * 1024, (0, 0, 0))
            .is_err());
        assert!(ui.open_requests.contains_key("large"));
        assert!(bounded(&json!("x".repeat(16 * 1024)), 16 * 1024).is_err());
        assert!(request_id(&json!({"requestId":""})).is_err());
        assert!(request_id(&json!({"requestId":"x".repeat(129)})).is_err());
    }
}

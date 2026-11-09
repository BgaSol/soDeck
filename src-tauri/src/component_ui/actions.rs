//! Generic declared behavior delivery and caller-owned page creation.
use super::*;
use crate::plugins::Receipt;
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};

pub(super) struct ActionRequest {
    pub context: Context,
    request_id: String,
    window: String,
    host_id: String,
    group_id: String,
    action_key: String,
    plugin_id: String,
    instance: String,
    fingerprint: Vec<u8>,
    receipt: Receipt,
    outcome: Option<Result<Value, String>>,
    invalidated: bool,
    view_id: Option<String>,
    bytes: usize,
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
fn key(context: &Context, request: &str) -> String {
    json!([context, request]).to_string()
}
fn fingerprint(value: Value) -> Vec<u8> {
    Sha256::digest(serde_json::to_vec(&value).unwrap()).to_vec()
}
impl ActionRequest {
    fn pending(&self) -> bool {
        self.outcome.is_none() && !self.invalidated
    }
    fn status(&self) -> Value {
        let (status, result, error) = if self.invalidated {
            (
                "invalidated",
                Value::Null,
                self.outcome
                    .as_ref()
                    .and_then(|r| r.as_ref().err())
                    .cloned(),
            )
        } else {
            match &self.outcome {
                None => ("pending", Value::Null, None),
                Some(Ok(v)) => ("completed", v.clone(), None),
                Some(Err(e)) => ("failed", Value::Null, Some(e.clone())),
            }
        };
        json!({"status":status,"requestId":self.request_id,"context":self.context,
            "actionKey":self.action_key,"viewId":self.view_id,"result":result,"error":error})
    }
}
impl Runtime {
    fn refresh_action(&mut self, request: &str) {
        let Some(r) = self.action_requests.get(request) else {
            return;
        };
        if !r.pending() {
            return;
        }
        let Some(mut outcome) = r.receipt.lock().unwrap().take() else {
            return;
        };
        if let Ok(value) = &outcome {
            let validation = (|| {
                if !value.is_object() {
                    return Err("ui.action 结果必须为对象".into());
                }
                bounded(value, 16 * 1024)?;
                if let Some(id) = value.get("viewId") {
                    let view = self.view(id.as_str().ok_or("ui.action viewId 必须为字符串")?)?;
                    if view.context != r.context
                        || view.plugin_id != r.plugin_id
                        || view.plugin_instance_id != r.instance
                    {
                        return Err("ui.action 返回了其他插件实例或工作区的页面".into());
                    }
                }
                Ok::<_, String>(())
            })();
            if let Err(e) = validation {
                outcome = Err(e);
            }
        }
        if let Err(error) = &mut outcome {
            if serde_json::to_vec(error).unwrap().len() > 16 * 1024 {
                *error = "ui.action 错误超过 16 KiB".into();
            }
        }
        let r = self.action_requests.get_mut(request).unwrap();
        if let Ok(value) = &outcome {
            if let Some(id) = value["viewId"].as_str() {
                r.view_id = Some(id.into());
            }
        }
        r.outcome = Some(outcome);
    }
    fn refresh_actions(&mut self) {
        for key in self.action_requests.keys().cloned().collect::<Vec<_>>() {
            self.refresh_action(&key);
        }
    }
    pub(super) fn pending_actions(&self) -> Vec<Value> {
        self.action_requests
            .values()
            .filter(|r| r.pending())
            .map(ActionRequest::status)
            .collect()
    }
    pub(crate) fn invalidate_host_actions(&mut self, window: &str, reason: &str) {
        self.invalidate_actions(|r| r.window == window, reason);
    }
    pub(crate) fn invalidate_workspace_actions(&mut self, ids: &[String], reason: &str) {
        self.invalidate_actions(|r| ids.contains(&r.context.workspace_id), reason);
    }
    pub(crate) fn invalidate_instance_actions(&mut self, plugin: &str, instance: &str) {
        self.invalidate_actions(
            |r| r.plugin_id == plugin && r.instance == instance,
            "行为提供者已停止",
        );
    }
    pub(super) fn invalidate_actions(
        &mut self,
        predicate: impl Fn(&ActionRequest) -> bool,
        reason: &str,
    ) {
        self.refresh_actions();
        for r in self
            .action_requests
            .values_mut()
            .filter(|r| r.pending() && predicate(r))
        {
            r.invalidated = true;
            r.outcome = Some(Err(reason.into()));
            // The supervisor still owns the original receipt until the real RPC ends.
            r.receipt = Arc::new(Mutex::new(None));
        }
    }
    pub(super) fn retain_request_contexts(
        &mut self,
        contexts: &[Context],
        workspace: Option<&str>,
    ) {
        let retain = |context: &Context| {
            workspace.is_some_and(|id| context.workspace_id != id) || contexts.contains(context)
        };
        self.action_requests.retain(|_, r| retain(&r.context));
        self.open_requests.retain(|_, r| retain(&r.context));
    }
    fn reserve_request(
        &self,
        context: &Context,
        bytes: usize,
        result_budget: usize,
    ) -> Result<(), String> {
        let count = self
            .action_requests
            .values()
            .filter(|r| &r.context == context)
            .count()
            + self
                .open_requests
                .values()
                .filter(|r| &r.context == context)
                .count();
        let used: usize = self
            .action_requests
            .values()
            .map(|r| {
                r.bytes
                    + r.outcome
                        .as_ref()
                        .map_or(64 * 1024, |v| serde_json::to_vec(v).unwrap().len())
            })
            .sum::<usize>()
            + self
                .open_requests
                .values()
                .map(|r| r.bytes + 128)
                .sum::<usize>();
        if count >= 4096
            || self.action_requests.len() + self.open_requests.len() >= 16384
            || used + bytes + result_budget > 16 * 1024 * 1024
        {
            return Err("工作区创建请求记录已满，请关闭并重新打开工作区".into());
        }
        Ok(())
    }
}
impl AppCore {
    pub fn ui_invoke_action(&self, window: &str, p: &Value) -> Result<Value, String> {
        bounded(p, 24 * 1024)?;
        let context: Context =
            serde_json::from_value(p["context"].clone()).map_err(|e| e.to_string())?;
        let request = request_id(p)?;
        let action_key = p["actionKey"].as_str().ok_or("缺少 actionKey")?;
        let data = p.get("data").cloned().unwrap_or(Value::Null);
        let bytes = bounded(&data, 16 * 1024)?;
        let input = fingerprint(json!([action_key, data, p["groupId"]]));
        let request_key = key(&context, request);
        let (instance, action, receipt) = {
            let mut _workbench = self.workbench_state();
            let crate::workbench::State { workspaces, ui } = &mut *_workbench;
            workspaces.validate_owned(window, &context)?;

            ui.refresh_action(&request_key);
            if let Some(r) = ui.action_requests.get(&request_key) {
                if r.fingerprint != input {
                    return Err("requestId 已用于不同的行为或参数".into());
                }
                return Ok(r.status());
            }
            workspaces.validate_context(&context)?;
            if self.is_closing()
                || ui.frozen(&context.workspace_id)
                || ui.lifecycle.host_closing(&context.workspace_id, window)
            {
                return Err("工作区正在关闭".into());
            }
            let action = ui
                .actions
                .get(action_key)
                .filter(|a| a.available)
                .cloned()
                .ok_or("行为提供者不可用")?;
            let instance = self
                .instances
                .lock()
                .unwrap()
                .get(&action.plugin_id)
                .cloned()
                .ok_or("行为提供者未运行")?;
            let actual = instance.view();
            if actual.status != PluginStatus::Running
                || actual.instance_id.as_deref() != Some(&action.plugin_instance_id)
            {
                return Err("行为提供者实例已失效".into());
            }
            ui.refresh_actions();
            if ui
                .action_requests
                .values()
                .any(|r| r.context == context && r.pending())
            {
                return Err("工作区已有结果尚未确认的行为".into());
            }
            let host = ui.workspaces[&context.workspace_id].host(window)?;
            ui.layout_admission(
                &context.workspace_id,
                window,
                Some(p["groupId"].as_str().unwrap_or(&host.focused_group_id)),
                None,
                None,
            )?;
            ui.reserve_request(&context, bytes, 64 * 1024)?;
            let receipt = Arc::new(Mutex::new(None));
            ui.action_requests.insert(
                request_key.clone(),
                ActionRequest {
                    context: context.clone(),
                    request_id: request.into(),
                    window: window.into(),
                    host_id: ui.workspaces[&context.workspace_id]
                        .host(window)?
                        .id
                        .clone(),
                    group_id: p["groupId"]
                        .as_str()
                        .unwrap_or(
                            &ui.workspaces[&context.workspace_id]
                                .host(window)?
                                .focused_group_id,
                        )
                        .into(),
                    action_key: action_key.into(),
                    plugin_id: action.plugin_id.clone(),
                    instance: action.plugin_instance_id.clone(),
                    fingerprint: input,
                    receipt: receipt.clone(),
                    outcome: None,
                    invalidated: false,
                    view_id: None,
                    bytes,
                },
            );
            (instance, action, receipt)
        };
        let _ = instance.request_observed("ui.action", json!({"context":context,"requestId":request,"actionId":action.definition.id,"data":data}),
            None, Some(receipt), std::time::Duration::from_secs(5));
        let result = {
            let mut ui = self.ui_state();
            ui.refresh_action(&request_key);
            ui.action_requests
                .get(&request_key)
                .ok_or("行为上下文已结束")?
                .status()
        };
        self.workspace_changed();
        Ok(result)
    }
    pub fn ui_action_status(&self, window: &str, p: &Value) -> Result<Value, String> {
        bounded(p, 24 * 1024)?;
        let context: Context =
            serde_json::from_value(p["context"].clone()).map_err(|e| e.to_string())?;
        let mut _workbench = self.workbench_state();
        let crate::workbench::State { workspaces, ui } = &mut *_workbench;
        workspaces.validate_owned(window, &context)?;
        let key = key(&context, request_id(p)?);

        ui.refresh_action(&key);
        Ok(ui
            .action_requests
            .get(&key)
            .ok_or("行为请求不存在或上下文已结束")?
            .status())
    }
    pub fn ui_process_open(
        &self,
        plugin: &str,
        instance: &str,
        p: &Value,
    ) -> Result<Value, String> {
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
        let action_key = key(&context, request);
        let open_key = json!([context, plugin, instance, request]).to_string();
        let mut _workbench = self.workbench_state();
        let crate::workbench::State { workspaces, ui } = &mut *_workbench;
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
        ui.refresh_action(&action_key);
        let mut parent = None;
        let (owner, group) = if let Some(r) = ui.action_requests.get(&action_key) {
            if r.plugin_id != plugin || r.instance != instance || !r.pending() {
                return Err("关联行为已结束或失效，不能申请迟到的页面".into());
            }
            let w = ui
                .workspaces
                .get(&context.workspace_id)
                .ok_or("工作台已关闭")?;
            let (window, host) = w
                .hosts
                .iter()
                .find(|(_, h)| h.id == r.host_id)
                .ok_or("发起工作台已关闭，创建意图已失效")?;
            (
                window.clone(),
                Some(if host.groups.contains_key(&r.group_id) {
                    r.group_id.clone()
                } else {
                    host.focused_group_id.clone()
                }),
            )
        } else if let Some(source) = p["sourceViewId"].as_str() {
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
        ui.reserve_request(&context, bytes, 128)?;
        let definition = format!("{plugin}:{view_type}");
        if ui
            .definitions
            .get(&definition)
            .is_none_or(|d| d.plugin_instance_id != instance)
        {
            return Err("视图不属于当前插件实例".into());
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
        if let Some(r) = ui.action_requests.get_mut(&action_key) {
            r.view_id = Some(id.clone());
        }
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
        assert!(ui.reserve_request(&context, 2, 128).is_err());
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
        assert!(ui.reserve_request(&context, 2, 64 * 1024).is_err());
        assert!(ui.open_requests.contains_key("large"));
        assert!(bounded(&json!("x".repeat(16 * 1024)), 16 * 1024).is_err());
        assert!(request_id(&json!({"requestId":""})).is_err());
        assert!(request_id(&json!({"requestId":"x".repeat(129)})).is_err());
    }
}

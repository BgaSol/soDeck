//! Context-bound, single-provider commands. The workbench lock owns admission and effects.
use crate::{
    model::{Manifest, PluginStatus},
    resource_tasks::{Task, Tasks},
    workspaces::Context,
    AppCore,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

const MAX_VALUE: usize = 16 * 1024;
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Declaration {
    pub api_version: String,
    pub handlers: Vec<String>,
}
pub fn valid_name(name: &str) -> bool {
    let Some((stem, version)) = name.rsplit_once(".v") else {
        return false;
    };
    name.len() <= 128
        && !stem.is_empty()
        && version
            .parse::<u32>()
            .is_ok_and(|v| v > 0 && v.to_string() == version)
        && stem.split('.').all(|s| {
            !s.is_empty()
                && s.bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
        })
}
impl Declaration {
    pub fn validate(&self) -> Result<(), String> {
        let names: HashSet<_> = self.handlers.iter().collect();
        if self.api_version != "1"
            || names.len() != self.handlers.len()
            || names.len() > 32
            || self.handlers.iter().any(|s| !valid_name(s))
        {
            return Err("命令 API、名称或数量无效".into());
        }
        Ok(())
    }
}
pub(crate) fn confirm(manifest: &Manifest, result: &Value) -> Result<(), String> {
    if let Some(d) = &manifest.commands {
        let actual: Vec<String> = serde_json::from_value(result["commandHandlers"].clone())
            .map_err(|_| "插件未确认命令处理者")?;
        let declared: HashSet<_> = d.handlers.iter().collect();
        if result["commandsApiVersion"] != "1"
            || actual.len() != declared.len()
            || actual.iter().collect::<HashSet<_>>() != declared
        {
            return Err("插件清单与初始化命令声明不一致".into());
        }
    }
    Ok(())
}
#[derive(Clone, Debug, Serialize)]
pub struct Error {
    pub code: String,
    pub message: String,
}
impl Error {
    pub(crate) fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
    fn invalid(message: impl Into<String>) -> Self {
        Self::new("invalidParams", message)
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}：{}", self.code, self.message)
    }
}
fn text<'a>(p: &'a Value, key: &str) -> Result<&'a str, Error> {
    p[key]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 128)
        .ok_or_else(|| Error::invalid(format!("{key} 必须为 1 至 128 字节的字符串")))
}
fn bounded(value: &Value) -> Result<usize, Error> {
    let size = serde_json::to_vec(value)
        .map_err(|e| Error::invalid(e.to_string()))?
        .len();
    if size > MAX_VALUE {
        return Err(Error::invalid("命令参数或结果超过 16 KiB"));
    }
    Ok(size)
}
fn canonical(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut pairs: Vec<_> = map.iter().collect();
            pairs.sort_by_key(|(k, _)| *k);
            Value::Object(
                pairs
                    .into_iter()
                    .map(|(k, v)| (k.clone(), canonical(v)))
                    .collect(),
            )
        }
        Value::Array(items) => Value::Array(items.iter().map(canonical).collect()),
        _ => value.clone(),
    }
}
fn fingerprint(value: &Value) -> Vec<u8> {
    Sha256::digest(serde_json::to_vec(&canonical(value)).unwrap()).to_vec()
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum Origin {
    Host {
        id: String,
    },
    Startup {
        host: String,
        definition: String,
    },
    Plugin {
        plugin: String,
        instance: String,
        view: String,
    },
}
#[derive(Default)]
struct DeliveryState {
    sent: bool,
    finished: bool,
    outcome: Option<Result<Value, Error>>,
}
/// Owned by the existing RPC request table until a real response or confirmed process exit.
pub(crate) struct Delivery(Arc<Mutex<DeliveryState>>, String);
impl Delivery {
    pub(crate) fn allowed(&self, core: &AppCore) -> bool {
        let mut state = core.workbench_state();
        let crate::workbench::State {
            workspaces,
            ui,
            commands,
        } = &mut *state;
        let Some(r) = commands.records.get_mut(&self.1) else {
            return false;
        };
        if workspaces.validate_context(&r.context).is_err() {
            r.end(
                "invalidated",
                Error::new("staleContext", "连接上下文已结束"),
            );
        }
        if let Origin::Plugin {
            plugin,
            instance,
            view,
        } = &r.origin
        {
            if ui.resource_view(view).is_err()
                || ui.lifecycle.view_frozen(view)
                || core.instances.lock().unwrap().get(plugin).is_none_or(|i| {
                    i.view().status != PluginStatus::Running
                        || i.view().instance_id.as_deref() != Some(instance)
                })
            {
                r.end("invalidated", Error::new("sourceStopped", "命令来源已停止"));
            }
        }
        r.status == "pending" && r.eligible
    }
    pub(crate) fn sent(&mut self) {
        self.0.lock().unwrap().sent = true;
    }
    pub(crate) fn log(&self, core: &AppCore) {
        core.command_log(&self.1, "命令处理回执");
    }
    pub(crate) fn complete(&mut self, frame: &Value) {
        let outcome = if frame.get("error").is_some() {
            let e = &frame["error"];
            Err(Error::new(
                e["data"]["code"]
                    .as_str()
                    .filter(|s| !s.is_empty() && s.len() <= 128)
                    .unwrap_or("handlerError"),
                e["message"]
                    .as_str()
                    .unwrap_or("命令处理失败")
                    .chars()
                    .take(2048)
                    .collect::<String>(),
            ))
        } else {
            let result = &frame["result"];
            bounded(result).and_then(|_| {
                if result.is_object() {
                    Ok(result.clone())
                } else {
                    Err(Error::new("protocolError", "命令结果必须为对象"))
                }
            })
        };
        let mut state = self.0.lock().unwrap();
        state.finished = true;
        state.outcome = Some(outcome);
    }
}
impl Drop for Delivery {
    fn drop(&mut self) {
        let mut state = self.0.lock().unwrap();
        if !state.finished && state.outcome.is_none() {
            state.outcome = Some(Err(if state.sent {
                Error::new("resultUnknown", "插件通信已结束，执行结果无法确认")
            } else {
                Error::new("notSent", "命令尚未发送，插件忙碌或已停止")
            }));
        }
    }
}
pub(crate) struct Record {
    pub context: Context,
    origin: Origin,
    request_id: String,
    name: String,
    action_key: Option<String>,
    input: Vec<u8>,
    pub host: String,
    pub group: String,
    provider: String,
    instance: String,
    status: &'static str,
    eligible: bool,
    result: Value,
    error: Option<Error>,
    pub view: Option<String>,
    opened_input: Option<Vec<u8>>,
    delivery: Arc<Mutex<DeliveryState>>,
    pub tasks: Arc<Tasks>,
    bytes: usize,
    retired: bool,
    focus_clock: u64,
    legacy: bool,
}
impl Record {
    fn wire(&self, id: &str) -> Value {
        json!({"commandId":id,"requestId":self.request_id,"name":self.name,"context":self.context,"status":self.status,
            "actionKey":self.action_key,"result":self.result,"error":self.error,"viewId":self.view,"hostId":self.host,"groupId":self.group})
    }
    fn end(&mut self, status: &'static str, error: Error) {
        self.eligible = false;
        if self.status == "pending" {
            self.status = status;
            self.error = Some(error);
        }
    }
}
#[derive(Default)]
pub struct Runtime {
    records: HashMap<String, Record>,
    keys: HashMap<String, String>,
    active: HashSet<String>,
}
impl Runtime {
    pub(crate) fn usage(&self, context: &Context) -> (usize, usize, usize) {
        (
            self.records
                .values()
                .filter(|r| r.context == *context)
                .count(),
            self.records.len(),
            self.records
                .values()
                .map(|r| r.bytes + MAX_VALUE + 1024)
                .sum::<usize>()
                + self.active.len() * 65536,
        )
    }
    pub(crate) fn refresh(&mut self) {
        for id in self.active.clone() {
            let r = self.records.get_mut(&id).unwrap();
            let outcome = r.delivery.lock().unwrap().outcome.take();
            if let Some(outcome) = outcome {
                if r.status == "pending" {
                    r.eligible = false;
                    match outcome {
                        Ok(v)
                            if v.get("viewId").is_none()
                                || v["viewId"].as_str() == r.view.as_deref() =>
                        {
                            r.status = "completed";
                            r.result = v;
                        }
                        Ok(_) => r.end(
                            "failed",
                            Error::new("protocolError", "处理者返回了未登记的页面"),
                        ),
                        Err(e) => r.end(
                            if e.code == "resultUnknown" {
                                "unknown"
                            } else {
                                "failed"
                            },
                            e,
                        ),
                    }
                }
            }
            // A logical terminal state does not release the real RPC's quota.
            if r.tasks.active() == 0 && r.status != "pending" {
                self.active.remove(&id);
                if r.retired {
                    self.records.remove(&id);
                }
            }
        }
    }
    pub(crate) fn retain_contexts(&mut self, contexts: &[Context]) {
        self.refresh();
        for r in self
            .records
            .values_mut()
            .filter(|r| !contexts.contains(&r.context))
        {
            r.retired = true;
            r.end(
                "invalidated",
                Error::new("staleContext", "连接上下文已结束"),
            );
        }
        self.keys
            .retain(|_, id| self.records.get(id).is_some_and(|r| !r.retired));
        self.records
            .retain(|id, r| !r.retired || self.active.contains(id));
    }
    pub(crate) fn invalidate_context(&mut self, context: &Context, reason: &str) {
        self.refresh();
        for r in self.records.values_mut().filter(|r| &r.context == context) {
            r.end("invalidated", Error::new("staleContext", reason));
        }
    }
    pub(crate) fn stop_instance(&mut self, plugin: &str, instance: &str) {
        self.refresh();
        for r in self.records.values_mut() {
            if matches!(&r.origin, Origin::Plugin{plugin:p,instance:i,..} if p==plugin && i==instance)
            {
                r.end("invalidated", Error::new("sourceStopped", "命令来源已停止"));
            } else if r.provider == plugin && r.instance == instance {
                r.eligible = false;
            }
        }
    }
    pub(crate) fn lost_host(&mut self, window: &str, ui: &crate::component_ui::Runtime) {
        self.refresh();
        for r in self.records.values_mut() {
            if ui
                .workspaces
                .get(&r.context.workspace_id)
                .and_then(|w| w.hosts.get(window))
                .is_some_and(|h| h.id == r.host)
            {
                r.end("invalidated", Error::new("sourceStopped", "原生窗口已消失"));
            }
        }
    }
    pub(crate) fn reserve_close(
        &mut self,
        op: &crate::lifecycle::Operation,
        ui: &crate::component_ui::Runtime,
    ) -> Vec<Arc<Tasks>> {
        self.refresh();
        let mut tasks = Vec::new();
        for r in self.records.values_mut() {
            let window = ui
                .workspaces
                .get(&r.context.workspace_id)
                .and_then(|w| w.hosts.iter().find(|(_, h)| h.id == r.host))
                .map(|(w, _)| w.as_str());
            let affected = op.ending_workspaces.contains(&r.context.workspace_id)
                || r.view.as_ref().is_some_and(|id| op.views.contains(id))
                || matches!(&r.origin, Origin::Plugin{view,..} if op.views.contains(view))
                || match &op.target {
                    crate::lifecycle::Target::Application { .. } => true,
                    crate::lifecycle::Target::Window { id } => window == Some(id),
                    crate::lifecycle::Target::Host { id } => {
                        r.context.workspace_id == *id && window == Some(op.window.as_str())
                    }
                    crate::lifecycle::Target::Group { id, workspace_id } => {
                        r.context.workspace_id == *workspace_id
                            && r.group == *id
                            && window == Some(op.window.as_str())
                    }
                    _ => false,
                };
            if affected {
                r.end(
                    "invalidated",
                    Error::new("workspaceClosing", "关闭已撤销此命令的创建资格"),
                );
                tasks.push(r.tasks.clone());
            }
        }
        tasks
    }
    pub(crate) fn pending(&self, window: &str, ui: &crate::component_ui::Runtime) -> Vec<Value> {
        self.active
            .iter()
            .filter_map(|id| {
                let r = &self.records[id];
                (r.status == "pending"
                    && matches!(r.origin, Origin::Host { .. })
                    && ui
                        .workspaces
                        .get(&r.context.workspace_id)
                        .is_some_and(|w| w.hosts.get(window).is_some_and(|h| h.id == r.host)))
                .then(|| r.wire(id))
            })
            .collect()
    }
}

impl AppCore {
    fn command_log(&self, id: &str, event: &str) {
        let mut state = self.workbench_state();
        state.commands.refresh();
        let Some(r) = state.commands.records.get(id) else {
            return;
        };
        let fields = json!({"commandId":id,"name":r.name,"origin":r.origin,"context":r.context,
            "provider":r.provider,"instance":r.instance,"status":r.status,"viewId":r.view,
            "errorCode":r.error.as_ref().map(|e|&e.code)});
        drop(state);
        self.log(None, "info", event, Some(fields));
    }
    pub(crate) fn command_provider(
        &self,
        name: &str,
    ) -> Result<(String, String, Arc<crate::plugins::Instance>), Error> {
        let instances = self.instances.lock().unwrap();
        let mut providers = instances.iter().filter_map(|(id, i)| {
            let v = i.view();
            (v.status == PluginStatus::Running
                && i.commands_api.load(std::sync::atomic::Ordering::Acquire)
                && v.manifest
                    .commands
                    .as_ref()
                    .is_some_and(|d| d.handlers.iter().any(|s| s == name)))
            .then(|| (id.clone(), v.instance_id.unwrap(), i.clone()))
        });
        let first = providers
            .next()
            .ok_or_else(|| Error::new("noHandler", "没有运行中的命令处理者"))?;
        if providers.next().is_some() {
            return Err(Error::new(
                "ambiguousHandler",
                "多个插件处理相同命令，无法确定目标",
            ));
        }
        Ok(first)
    }
    pub fn command_host_request(&self, window: &str, p: &Value) -> Result<Value, Error> {
        self.command_request(None, Some(window), p)
    }
    pub(crate) fn command_process_request(
        &self,
        plugin: &str,
        instance: &str,
        method: &str,
        p: &Value,
    ) -> Result<Value, Error> {
        if !p.is_object() {
            return Err(Error::invalid("命令参数必须为对象"));
        }
        let actual = self
            .instances
            .lock()
            .unwrap()
            .get(plugin)
            .map(|i| i.view())
            .ok_or_else(|| Error::new("sourceStopped", "插件已停止"))?;
        if actual.status != PluginStatus::Running || actual.instance_id.as_deref() != Some(instance)
        {
            return Err(Error::new("sourceStopped", "插件实例已失效"));
        }
        let mut params = p.clone();
        params["op"] = json!(method.trim_start_matches("commands."));
        self.command_request(Some((plugin, instance)), None, &params)
    }
    fn command_request(
        &self,
        process: Option<(&str, &str)>,
        window: Option<&str>,
        p: &Value,
    ) -> Result<Value, Error> {
        if serde_json::to_vec(p)
            .map_err(|e| Error::invalid(e.to_string()))?
            .len()
            > 24 * 1024
        {
            return Err(Error::invalid("命令信封超过 24 KiB"));
        }
        for (key, limit) in [
            ("groupId", 128),
            ("startupDefinition", 256),
            ("actionKey", 256),
        ] {
            if !p[key].is_null()
                && p[key]
                    .as_str()
                    .is_none_or(|s| s.is_empty() || s.len() > limit)
            {
                return Err(Error::invalid(format!("{key} 必须为有界非空字符串")));
            }
        }
        let input = fingerprint(&json!([
            p["name"],
            p.get("payload").cloned().unwrap_or(Value::Null),
            p["groupId"],
            p["startupDefinition"],
            p["actionKey"]
        ]));
        let context: Context = serde_json::from_value(
            p.get("expectedContext")
                .or_else(|| p.get("context"))
                .cloned()
                .unwrap_or(Value::Null),
        )
        .map_err(|_| Error::invalid("缺少完整 Context"))?;
        let op = text(p, "op")?;
        let payload = p.get("payload").cloned().unwrap_or(Value::Null);
        let bytes = bounded(&payload)?;
        let mut state = self.workbench_state();
        state
            .workspaces
            .validate_identity(&context)
            .map_err(|e| Error::new("staleContext", e))?;
        state.commands.refresh();
        let origin = if let Some((plugin, instance)) = process {
            Origin::Plugin {
                plugin: plugin.into(),
                instance: instance.into(),
                view: text(p, "sourceViewId")?.into(),
            }
        } else {
            let window = window.unwrap();
            let host = state
                .ui
                .workspaces
                .get(&context.workspace_id)
                .and_then(|w| w.hosts.get(window))
                .ok_or_else(|| Error::new("staleContext", "工作台已移动或关闭"))?;
            if let Some(definition) = p["startupDefinition"].as_str() {
                Origin::Startup {
                    host: host.id.clone(),
                    definition: definition.into(),
                }
            } else {
                Origin::Host {
                    id: host.id.clone(),
                }
            }
        };
        if op == "get" || op == "submit" {
            let request = text(p, "requestId")?;
            let key = json!([context, origin, request]).to_string();
            let id = p["commandId"]
                .as_str()
                .or_else(|| state.commands.keys.get(&key).map(String::as_str));
            if let Some(id) = id {
                let r = state
                    .commands
                    .records
                    .get(id)
                    .filter(|r| {
                        !r.retired
                            && r.context == context
                            && r.origin == origin
                            && r.request_id == request
                    })
                    .ok_or_else(|| Error::new("notFound", "命令回执不存在"))?;
                if op == "submit" && r.input != input {
                    return Err(Error::new(
                        "requestConflict",
                        "requestId 已用于不同的命令或参数",
                    ));
                }
                return Ok(r.wire(id));
            }
            if op == "get" {
                return Err(Error::new("notFound", "命令尚未被观察到；请保留原请求编号"));
            }
        }
        if !["submit", "resolve"].contains(&op) {
            return Err(Error::invalid("未知命令操作"));
        }
        let name = text(p, "name")?;
        if !valid_name(name) {
            return Err(Error::invalid("命令名称必须带主版本"));
        }
        state
            .workspaces
            .validate_context(&context)
            .map_err(|e| Error::new("staleContext", e))?;
        let (host_id, group) = if let Origin::Plugin {
            plugin,
            instance,
            view,
        } = &origin
        {
            let v = state
                .ui
                .resource_view(view)
                .map_err(|e| Error::new("sourceStopped", e))?;
            if v.context != context || v.plugin_id != *plugin || v.plugin_instance_id != *instance {
                return Err(Error::new("sourceStopped", "来源页面或 Context 不匹配"));
            }
            if state.ui.lifecycle.view_frozen(view) {
                return Err(Error::new("workspaceClosing", "来源页面正在关闭"));
            }
            let w = &state.ui.workspaces[&context.workspace_id];
            let window = w
                .view_window(view)
                .ok_or_else(|| Error::new("sourceStopped", "来源页面没有显示位置"))?;
            let host = w
                .host(&window)
                .map_err(|e| Error::new("sourceStopped", e))?;
            (
                host.id.clone(),
                host.group_of(view)
                    .unwrap_or_else(|| host.focused_group_id.clone()),
            )
        } else {
            let host = state.ui.workspaces[&context.workspace_id]
                .host(window.unwrap())
                .map_err(|e| Error::new("staleContext", e))?;
            (
                host.id.clone(),
                p["groupId"]
                    .as_str()
                    .unwrap_or(&host.focused_group_id)
                    .to_owned(),
            )
        };
        let w = &state.ui.workspaces[&context.workspace_id];
        let (target, host) = w.hosts.iter().find(|(_, h)| h.id == host_id).unwrap();
        if !host.groups.contains_key(&group) {
            return Err(Error::new("targetClosed", "目标标签组已关闭"));
        }
        command_admission(self, &state, &context, target, &group)?;
        let legacy_action = if process.is_none() && name == "ui.action.v1" {
            let action = state
                .ui
                .actions
                .get(text(p, "actionKey")?)
                .filter(|a| a.available && a.definition.command.is_none())
                .cloned()
                .ok_or_else(|| Error::new("noHandler", "行为提供者不可用"))?;
            Some(action)
        } else {
            None
        };
        let (provider, instance, transport) = if let Some(action) = &legacy_action {
            let transport = self
                .instances
                .lock()
                .unwrap()
                .get(&action.plugin_id)
                .cloned()
                .ok_or_else(|| Error::new("noHandler", "行为提供者不可用"))?;
            if transport.view().status != PluginStatus::Running
                || transport.view().instance_id.as_deref() != Some(&action.plugin_instance_id)
            {
                return Err(Error::new("noHandler", "行为提供者已停止"));
            }
            (
                action.plugin_id.clone(),
                action.plugin_instance_id.clone(),
                transport,
            )
        } else {
            self.command_provider(name)?
        };
        if op == "resolve" {
            return Ok(json!({"available":true,"name":name}));
        }
        let request = text(p, "requestId")?;
        let active_source = state
            .commands
            .active
            .iter()
            .filter(|id| state.commands.records[*id].origin == origin)
            .count();
        let active_provider = state
            .commands
            .active
            .iter()
            .filter(|id| state.commands.records[*id].instance == instance)
            .count();
        let (count, total, used) = state.commands.usage(&context);
        let (own_count, own_total, own_bytes) = state.ui.open_request_usage(&context);
        if active_source >= 32
            || active_provider >= 32
            || total + own_total >= 16384
            || count + own_count >= 4096
            || used + own_bytes + bytes + MAX_VALUE + 66560 > 16 * 1024 * 1024
        {
            return Err(Error::new("resourceLimit", "命令回执或在途请求已满"));
        }
        let focus_clock = state.ui.clock;
        let id = self.next_id();
        let tasks = Arc::new(Tasks::default());
        let task: Task = tasks.enter().map_err(Error::invalid)?;
        let delivery = Arc::new(Mutex::new(DeliveryState::default()));
        let key = json!([context, origin, request]).to_string();
        state.commands.records.insert(
            id.clone(),
            Record {
                context: context.clone(),
                origin,
                request_id: request.into(),
                name: name.into(),
                action_key: p["actionKey"].as_str().map(str::to_owned),
                input,
                host: host_id,
                group,
                provider,
                instance,
                status: "pending",
                eligible: true,
                result: Value::Null,
                error: None,
                view: None,
                opened_input: None,
                delivery: delivery.clone(),
                tasks,
                bytes,
                retired: false,
                focus_clock,
                legacy: legacy_action.is_some(),
            },
        );
        state.commands.keys.insert(key, id.clone());
        state.commands.active.insert(id.clone());
        drop(state);
        self.command_log(&id, "命令已接纳");
        let (method, params) = if let Some(action) = legacy_action {
            (
                "ui.action",
                json!({"requestId":id,"context":context,"actionId":action.definition.id,"data":payload}),
            )
        } else {
            (
                "commands.handle",
                json!({"commandId":id,"name":name,"context":context,"payload":payload}),
            )
        };
        let _ = transport.enqueue_command(method, params, task, Delivery(delivery, id.clone()));
        let mut state = self.workbench_state();
        state.commands.refresh();
        Ok(state
            .commands
            .records
            .get(&id)
            .map(|r| r.wire(&id))
            .unwrap_or_else(|| json!({"commandId":id,"status":"invalidated"})))
    }
    pub(crate) fn command_open(
        &self,
        plugin: &str,
        instance: &str,
        p: &Value,
    ) -> Result<Value, Error> {
        if serde_json::to_vec(p)
            .map_err(|e| Error::invalid(e.to_string()))?
            .len()
            > 24 * 1024
        {
            return Err(Error::invalid("页面申请信封超过 24 KiB"));
        }
        let resource = p
            .get("resourceKey")
            .map(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| Error::invalid("resourceKey 必须为字符串"))
            })
            .transpose()?;
        let id = text(p, "commandId")?;
        let request = text(p, "requestId")?;
        if request != format!("open:{id}") {
            return Err(Error::invalid("命令页面必须使用稳定子请求编号"));
        }
        let context: Context = serde_json::from_value(p["expectedContext"].clone())
            .map_err(|_| Error::invalid("缺少完整 Context"))?;
        let input = fingerprint(&json!([p["viewType"], p["resourceKey"], p["params"]]));
        bounded(&p["params"])?;
        let mut state = self.workbench_state();
        let crate::workbench::State {
            workspaces,
            ui,
            commands,
        } = &mut *state;
        workspaces
            .validate_identity(&context)
            .map_err(|e| Error::new("staleContext", e))?;
        commands.refresh();
        let r = commands
            .records
            .get_mut(id)
            .filter(|r| r.context == context && r.provider == plugin && r.instance == instance)
            .ok_or_else(|| Error::new("notFound", "命令不属于当前处理者或 Context"))?;
        if let Some(view) = &r.view {
            if r.opened_input.as_ref() != Some(&input) {
                return Err(Error::new("requestConflict", "原命令已登记不同页面"));
            }
            return Ok(json!({"viewId":view,"closed":ui.resource_view(view).is_err()}));
        }
        let actual = self.instances.lock().unwrap().get(plugin).map(|i| i.view());
        if actual.is_none_or(|v| {
            v.status != PluginStatus::Running || v.instance_id.as_deref() != Some(instance)
        }) {
            r.eligible = false;
        }
        if !r.eligible || r.status != "pending" {
            return Err(Error::new("invalidated", "命令已结束，不能创建迟到的页面"));
        }
        workspaces
            .validate_context(&context)
            .map_err(|e| Error::new("staleContext", e))?;
        let w = ui
            .workspaces
            .get(&context.workspace_id)
            .ok_or_else(|| Error::new("targetClosed", "工作台已关闭"))?;
        let (window, host) = w
            .hosts
            .iter()
            .find(|(_, h)| h.id == r.host)
            .ok_or_else(|| Error::new("targetClosed", "目标工作台已关闭"))?;
        if !host.groups.contains_key(&r.group) {
            return Err(Error::new("targetClosed", "目标标签组已关闭"));
        }
        let window = window.clone();
        if workspaces
            .pending_moves()
            .iter()
            .any(|m| m.workspace_id == context.workspace_id)
        {
            return Err(Error::new("targetBusy", "目标工作台正在交接"));
        }
        if self.is_closing() || ui.lifecycle.host_closing(&context.workspace_id, &window) {
            return Err(Error::new("workspaceClosing", "目标正在关闭"));
        }
        ui.layout_admission(&context.workspace_id, &window, Some(&r.group), None, None)
            .map_err(|e| Error::new("targetBusy", e))?;
        if let Origin::Plugin {
            plugin,
            instance,
            view,
        } = &r.origin
        {
            if self.instances.lock().unwrap().get(plugin).is_none_or(|i| {
                i.view().status != PluginStatus::Running
                    || i.view().instance_id.as_deref() != Some(instance)
            }) {
                return Err(Error::new("sourceStopped", "命令来源进程已停止"));
            }
            if ui.resource_view(view).is_err() || ui.lifecycle.view_frozen(view) {
                return Err(Error::new("sourceStopped", "命令来源已关闭"));
            }
        }
        let definition = format!("{plugin}:{}", text(p, "viewType")?);
        if ui
            .definitions
            .get(&definition)
            .is_none_or(|d| !d.available || d.plugin_instance_id != instance)
        {
            return Err(Error::new("handlerStopped", "视图定义不属于处理者"));
        }
        if matches!(&r.origin,Origin::Startup{definition:d,..} if w.views.iter().any(|v|v.definition_key==*d))
        {
            r.end(
                "invalidated",
                Error::new("startupSuperseded", "已有显式创建的页面，跳过默认页面"),
            );
            return Err(Error::new("startupSuperseded", "默认页面已由显式创建取代"));
        }
        let previous = host.clone();
        let keep_focus = ui.clock != r.focus_clock;
        let view = ui
            .open_view(
                &context,
                &window,
                Some(&r.group),
                &definition,
                resource,
                p["params"].clone(),
                self.next_id(),
                false,
            )
            .map_err(|e| Error::new("pageRejected", e))?;
        if keep_focus {
            let host = ui
                .workspaces
                .get_mut(&context.workspace_id)
                .unwrap()
                .host_mut(&window)
                .unwrap();
            host.preserve_focus(&previous);
        }
        r.view = Some(view.clone());
        r.opened_input = Some(input);
        self.sync_shared_ui_locked(ui);
        drop(state);
        self.command_log(id, "命令页面已登记");
        self.workspace_changed();
        Ok(json!({"viewId":view}))
    }
    pub(crate) fn command_legacy_open(
        &self,
        plugin: &str,
        instance: &str,
        p: &Value,
    ) -> Option<Value> {
        let state = self.workbench_state();
        let id = p["requestId"].as_str()?;
        let r = state.commands.records.get(id)?;
        if !r.legacy || r.provider != plugin || r.instance != instance {
            return None;
        }
        let mut input = p.clone();
        input["commandId"] = json!(id);
        input["requestId"] = json!(format!("open:{id}"));
        Some(input)
    }
}
fn command_admission(
    core: &AppCore,
    state: &crate::workbench::State,
    context: &Context,
    window: &str,
    group: &str,
) -> Result<(), Error> {
    if core.is_closing()
        || state
            .ui
            .lifecycle
            .host_closing(&context.workspace_id, window)
    {
        return Err(Error::new("workspaceClosing", "目标工作台正在关闭"));
    }
    if state
        .workspaces
        .pending_moves()
        .iter()
        .any(|m| m.workspace_id == context.workspace_id)
    {
        return Err(Error::new("targetBusy", "工作台正在交接"));
    }
    state
        .ui
        .layout_admission(&context.workspace_id, window, Some(group), None, None)
        .map_err(|e| Error::new("targetBusy", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_and_fingerprints_preserve_business_values() {
        for name in ["terminal.open.v1", "a-b_c.v12"] {
            assert!(valid_name(name));
        }
        for name in [
            "terminal.open",
            "a.v0",
            "a.v01",
            "a..b.v1",
            "a/v1",
            "a.v4294967296",
        ] {
            assert!(!valid_name(name));
        }
        assert_eq!(
            fingerprint(&json!({"b":2,"a":{"z":1,"y":2}})),
            fingerprint(&json!({"a":{"y":2,"z":1},"b":2}))
        );
        assert_ne!(
            fingerprint(&json!({"path":"/a"})),
            fingerprint(&json!({"path":"/a/"}))
        );
    }
    #[test]
    fn delivery_distinguishes_not_sent_from_unconfirmed_and_bounds_results() {
        let state = Arc::new(Mutex::new(DeliveryState::default()));
        drop(Delivery(state.clone(), String::new()));
        assert_eq!(
            state
                .lock()
                .unwrap()
                .outcome
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap_err()
                .code,
            "notSent"
        );
        let state = Arc::new(Mutex::new(DeliveryState::default()));
        let mut delivery = Delivery(state.clone(), String::new());
        delivery.sent();
        drop(delivery);
        assert_eq!(
            state
                .lock()
                .unwrap()
                .outcome
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap_err()
                .code,
            "resultUnknown"
        );
        let state = Arc::new(Mutex::new(DeliveryState::default()));
        let mut delivery = Delivery(state.clone(), String::new());
        delivery.complete(&json!({"result":{"large":"x".repeat(MAX_VALUE)}}));
        drop(delivery);
        assert!(state.lock().unwrap().outcome.as_ref().unwrap().is_err());
        let state = Arc::new(Mutex::new(DeliveryState::default()));
        let mut delivery = Delivery(state.clone(), String::new());
        delivery
            .complete(&json!({"error":{"data":{"code":"x".repeat(MAX_VALUE)},"message":"failed"}}));
        assert_eq!(
            state
                .lock()
                .unwrap()
                .outcome
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap_err()
                .code,
            "handlerError"
        );
    }
}

use super::*;
use crate::model::PluginStatus;
use serde_json::{json, Value};
use std::collections::HashSet;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Params {
    request_id: String,
    server_id: Option<String>,
    expected_profile_token: Option<String>,
    view_type: Option<String>,
}

pub(crate) fn valid_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 256 && !s.contains('\0')
}

pub(crate) fn bounded(value: Value) -> Result<Value, Error> {
    if serde_json::to_vec(&value)
        .map_err(|e| Error::new("invalidResult", e.to_string()))?
        .len()
        > 48 * 1024
    {
        Err(Error::new(
            "resourceLimit",
            "核心连接结果超过控制帧预算，请缩小查询范围",
        ))
    } else {
        Ok(value)
    }
}

impl AppCore {
    pub(crate) fn connection_request(
        self: &Arc<Self>,
        plugin: &str,
        instance: &str,
        method: &str,
        p: &Value,
    ) -> Result<Value, Error> {
        let caller = self
            .plugin_instance(plugin, instance)
            .ok_or_else(|| Error::new("staleInstance", "插件实例不可用"))?;
        let params: Params = serde_json::from_value(p.clone())
            .map_err(|e| Error::new("invalidParams", e.to_string()))?;
        if !valid_id(&params.request_id)
            || params.request_id.len() > 128
            || p.as_object().is_none_or(|o| o.values().any(Value::is_null))
            || params.view_type.as_ref().is_some_and(|s| !valid_id(s))
        {
            return Err(Error::new("invalidParams", "连接请求参数无效"));
        }
        if let Some(view_type) = &params.view_type {
            self.validate_background_view(plugin, instance, view_type)
                .map_err(|e| Error::new("invalidParams", e))?;
        }
        let key = RequestKey {
            plugin: plugin.into(),
            instance: instance.into(),
            request: params.request_id,
        };
        match method {
            "connections.open" => {
                let server = params
                    .server_id
                    .filter(|s| valid_id(s))
                    .ok_or_else(|| Error::new("invalidParams", "缺少 serverId"))?;
                let profile = params
                    .expected_profile_token
                    .filter(|s| valid_id(s))
                    .ok_or_else(|| Error::new("invalidParams", "缺少 expectedProfileToken"))?;
                let input = OpenInput {
                    server: server.clone(),
                    profile,
                };
                let exists = {
                    let registry = self.connections.registry.lock().unwrap();
                    if let Some(old) = registry.opens.get(&key) {
                        if old.input.as_ref().is_some_and(|i| i != &input) {
                            return Err(Error::new(
                                "requestConflict",
                                "同一 requestId 的连接参数不同",
                            ));
                        }
                        true
                    } else {
                        false
                    }
                };
                if !exists {
                    let window = self
                        .workspace_state()
                        .by_server(&server)
                        .map(|w| w.window_id.clone())
                        .unwrap_or_else(|| "main".into());
                    match self.select_connection(
                        &window,
                        &server,
                        Some((key.clone(), input, caller)),
                    ) {
                        Ok(_) => {}
                        Err(e) if e.code == "requestRecorded" => {}
                        Err(e) => return Err(e),
                    }
                }
                self.open_snapshot(&key, params.view_type.as_deref())
            }
            "connections.get" => {
                if params.server_id.is_some() || params.expected_profile_token.is_some() {
                    return Err(Error::new(
                        "invalidParams",
                        "get 只接受原 requestId 和 viewType",
                    ));
                }
                self.open_snapshot(&key, params.view_type.as_deref())
            }
            "connections.cancel" => {
                if params.server_id.is_some()
                    || params.expected_profile_token.is_some()
                    || params.view_type.is_some()
                {
                    return Err(Error::new("invalidParams", "cancel 只接受原 requestId"));
                }
                let mut registry = self.connections.registry.lock().unwrap();
                if caller.view().status != PluginStatus::Running {
                    return Err(Error::new("staleInstance", "插件正在停止"));
                }
                let attached = registry.opens.get(&key).and_then(|r| match &r.binding {
                    Binding::Attached { attempt_id } => Some(attempt_id.clone()),
                    _ => None,
                });
                match registry.opens.get_mut(&key) {
                    Some(r)
                        if matches!(
                            r.binding,
                            Binding::Accepted { .. } | Binding::Failed { .. }
                        ) => {}
                    Some(r) => r.binding = Binding::Cancelled,
                    None => {
                        // A full instance cannot admit the delayed open either; records are not evicted.
                        if registry.check_capacity(&key).is_err() {
                            return Ok(json!({"receipt":{"state":"cancelled"}}));
                        }
                        registry.opens.insert(
                            key.clone(),
                            OpenRequest {
                                input: None,
                                binding: Binding::Cancelled,
                            },
                        );
                    }
                }
                if let Some(id) = attached {
                    if !registry.has_external(&id) {
                        if let Some(a) = registry
                            .current
                            .values()
                            .find(|a| a.view().attempt_id == id)
                        {
                            let s = a.state.lock().unwrap();
                            if !s.desktop_requested && s.view.context.is_none() {
                                a.cancel.cancel();
                            }
                        }
                    }
                }
                drop(registry);
                self.open_snapshot(&key, None)
            }
            _ => Err(Error::new("invalidParams", "未知连接方法")),
        }
    }

    fn open_snapshot(&self, key: &RequestKey, view_type: Option<&str>) -> Result<Value, Error> {
        let (binding, view) = {
            let registry = self.connections.registry.lock().unwrap();
            let request = registry
                .opens
                .get(key)
                .ok_or_else(|| Error::new("connectionRequestNotFound", "原请求未登记"))?;
            let view = match &request.binding {
                Binding::Attached { attempt_id } => registry
                    .current
                    .values()
                    .find(|a| a.view().attempt_id == *attempt_id)
                    .map(|a| a.view()),
                _ => None,
            };
            (request.binding.clone(), view)
        };
        let context = match &binding {
            Binding::Accepted { context } => Some(context),
            _ => None,
        };
        let current = context.map(|c| {
            self.context_target_facts(c)
                .unwrap_or_else(|e| json!({"error":{"code":e.code,"message":e.message}}))
        });
        let background = match (context, view_type) {
            (Some(c), Some(v)) => Some(
                self.background_creation(&key.plugin, &key.instance, v, c)
                    .map_err(|e| Error::new("invalidParams", e))?,
            ),
            _ => None,
        };
        bounded(json!({"receipt":binding,"view":view,"current":current,"background":background}))
    }

    pub(crate) fn notify_connection_change(&self, server: &str) {
        let sources: HashSet<_> = self
            .connections
            .registry
            .lock()
            .unwrap()
            .opens
            .iter()
            .filter(|(_, r)| r.input.as_ref().is_some_and(|i| i.server == server))
            .map(|(k, _)| (k.plugin.clone(), k.instance.clone()))
            .collect();
        for (plugin, instance) in sources {
            if let Some(caller) = self.plugin_instance(&plugin, &instance) {
                let _ = caller.enqueue_connection_change(json!({"serverId":server}));
            }
        }
    }

    pub(crate) fn stop_connection_requests(&self, plugin: &str, instance: &str) {
        let mut registry = self.connections.registry.lock().unwrap();
        let ids: HashSet<_> = registry
            .opens
            .iter()
            .filter(|(k, _)| k.plugin == plugin && k.instance == instance)
            .filter_map(|(_, r)| match &r.binding {
                Binding::Attached { attempt_id } => Some(attempt_id.clone()),
                _ => None,
            })
            .collect();
        registry
            .opens
            .retain(|k, _| k.plugin != plugin || k.instance != instance);
        for a in registry.current.values() {
            let s = a.state.lock().unwrap();
            if ids.contains(&s.view.attempt_id)
                && !s.desktop_requested
                && s.view.context.is_none()
                && !registry.has_external(&s.view.attempt_id)
            {
                a.cancel.cancel();
            }
        }
    }
}

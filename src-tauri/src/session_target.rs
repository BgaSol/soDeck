//! Read-only target facts captured under the connection/workbench lock order.
use crate::{session_exec::Error, workspaces::Context, AppCore};
use serde_json::{json, Value};
use sodeck_plugin_protocol::requests::ViewContext as Describe;
fn error(code: &'static str, message: impl Into<String>) -> Error {
    Error {
        code,
        message: message.into(),
    }
}
fn identity(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256 && !value.contains('\0')
}
impl AppCore {
    pub(crate) fn describe_target(
        &self,
        plugin: &str,
        instance: &str,
        p: &Value,
    ) -> Result<Value, Error> {
        let request: Describe =
            serde_json::from_value(p.clone()).map_err(|e| error("invalidParams", e.to_string()))?;
        let context = &request.expected_context;
        if !identity(&request.view_id)
            || !identity(&context.workspace_id)
            || !identity(&context.server_id)
            || !identity(&context.connection_id)
            || context.generation == 0
            || p["expectedContext"].as_object().is_some_and(|o| {
                o.keys().any(|k| {
                    !matches!(
                        k.as_str(),
                        "workspaceId" | "serverId" | "connectionId" | "generation"
                    )
                })
            })
        {
            return Err(error("invalidParams", "目标查询身份无效"));
        }
        self.target_facts(plugin, instance, &request.view_id, context)
    }
    pub(crate) fn target_facts(
        &self,
        plugin: &str,
        instance: &str,
        view_id: &str,
        expected: &Context,
    ) -> Result<Value, Error> {
        let registry = self.connections.registry.lock().unwrap();
        let state = self.workbench_state();
        let view = state
            .ui
            .resource_view(view_id)
            .map_err(|e| error("staleContext", e))?;
        if view.plugin_id != plugin || view.plugin_instance_id != instance {
            return Err(error("staleContext", "视图不属于此实例"));
        }
        let context = view.context.clone();
        if expected != &context {
            return Err(error("staleContext", "连接代次已改变"));
        }
        let closing = state.ui.lifecycle.view_frozen(&view.id);
        target_context_facts(&registry, &state, &context, closing)
    }
    pub(crate) fn context_target_facts(&self, context: &Context) -> Result<Value, Error> {
        let registry = self.connections.registry.lock().unwrap();
        let state = self.workbench_state();
        let closing = state.ui.frozen(&context.workspace_id);
        target_context_facts(&registry, &state, context, closing)
    }
}

fn target_context_facts(
    registry: &crate::connections::registry::Registry,
    state: &crate::workbench::State,
    context: &Context,
    closing: bool,
) -> Result<Value, Error> {
    let workspace = state
        .workspaces
        .target_workspace(&context)
        .map_err(|e| error("staleContext", e))?;
    let session = if workspace.connected {
        registry
            .current
            .get(&context.server_id)
            .and_then(|a| a.session_for(&context))
    } else {
        None
    };
    let connected = session.is_some();
    let target = session
        .as_ref()
        .map(|s| &s.target)
        .or(workspace.connection_target.as_ref())
        .cloned();
    let name = workspace.name.clone();

    let target = target.map(|t| {
            let mut value = json!({"name":name,"protocol":t.protocol,"endpoint":t.endpoint(),"username":t.username});
            value["host"] = json!(t.host); value["port"] = json!(t.port);
            value
        });
    let mut value = json!({"context":context,"target":target,"state":if connected {"connected"}else{"disconnected"},
            "closing":closing,"capabilities":if connected {vec!["exec","files"]}else{vec![]}});
    {
        value["targetSource"] = json!(if value["target"].is_null() {
            "unknown"
        } else if connected {
            "current"
        } else {
            "lastConnected"
        });
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn describe_validates_before_reading_resources() {
        let directory = std::env::temp_dir().join(format!(
            "sodeck-target-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let core = AppCore::open(directory.clone(), crate::app_version()).unwrap();
        for p in [
            json!({}),
            json!({"viewId":"v","expectedContext":{"workspaceId":"w","serverId":"s","connectionId":"c","generation":0}}),
            json!({"viewId":"v","expectedContext":{"workspaceId":"w","serverId":"s","connectionId":"c","generation":1,"extra":true}}),
        ] {
            assert_eq!(
                core.describe_target("p", "i", &p).unwrap_err().code,
                "invalidParams"
            );
        }
        core.shutdown().unwrap();
        drop(core);
        assert_eq!(directory.parent(), Some(std::env::temp_dir().as_path()));
        std::fs::remove_dir_all(directory).unwrap();
    }
}

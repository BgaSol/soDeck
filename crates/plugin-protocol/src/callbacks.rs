//! Core-to-plugin calls. Payloads belong to the plugin; their envelopes do not.
use crate::{ApiError, Context, Initialize};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct View {
    pub view_id: String,
    pub view_type: Option<String>,
    pub context: Option<Context>,
    pub resource_key: Option<String>,
    #[serde(default)]
    pub params: Value,
    #[serde(default)]
    pub data: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Settings {
    pub page_id: String,
    pub definition_token: String,
    pub revision: String,
    pub values: serde_json::Map<String, Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Command {
    pub command_id: String,
    pub name: String,
    pub context: Context,
    pub payload: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConnectionChanged {
    pub server_id: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum Callback {
    #[serde(rename = "initialize")]
    Initialize(Initialize),
    #[serde(rename = "settings.changed")]
    SettingsChanged(Settings),
    #[serde(rename = "commands.handle")]
    Command(Command),
    #[serde(rename = "connections.changed")]
    ConnectionChanged(ConnectionChanged),
    #[serde(rename = "ui.createView")]
    CreateView(View),
    #[serde(rename = "ui.getView")]
    GetView(View),
    #[serde(rename = "ui.event")]
    Event(View),
    #[serde(rename = "ui.beforeClose")]
    BeforeClose(View),
    #[serde(rename = "ui.save")]
    Save(View),
    #[serde(rename = "ui.disposeView")]
    DisposeView(View),
}
impl Callback {
    pub fn decode(method: &str, params: Value) -> Result<Self, ApiError> {
        if !matches!(
            method,
            "initialize"
                | "settings.changed"
                | "commands.handle"
                | "connections.changed"
                | "ui.createView"
                | "ui.getView"
                | "ui.event"
                | "ui.beforeClose"
                | "ui.save"
                | "ui.disposeView"
        ) {
            return Err(ApiError::method_not_found());
        }
        let call: Self = serde_json::from_value(json!({"method":method,"params":params}))
            .map_err(|_| ApiError::invalid("插件回调参数不符合当前协议"))?;
        match &call {
            Self::CreateView(v)
            | Self::GetView(v)
            | Self::Event(v)
            | Self::BeforeClose(v)
            | Self::Save(v)
            | Self::DisposeView(v)
                if v.view_id.is_empty() =>
            {
                return Err(ApiError::invalid("缺少 viewId"))
            }
            Self::SettingsChanged(s)
                if s.page_id.is_empty()
                    || s.definition_token.is_empty()
                    || s.revision.parse::<u64>().is_err() =>
            {
                return Err(ApiError::invalid("设置回调身份无效"))
            }
            _ => {}
        }
        Ok(call)
    }

    /// Project a checked envelope into a plugin's existing JSON state machine.
    /// This never sends a raw RPC or chooses a different protocol.
    pub fn into_parts(self) -> (&'static str, Value) {
        macro_rules! part {
            ($value:expr, $method:literal) => {
                ($method, json!($value))
            };
        }
        match self {
            Self::Initialize(v) => part!(v, "initialize"),
            Self::SettingsChanged(v) => part!(v, "settings.changed"),
            Self::Command(v) => part!(v, "commands.handle"),
            Self::ConnectionChanged(v) => part!(v, "connections.changed"),
            Self::CreateView(v) => part!(v, "ui.createView"),
            Self::GetView(v) => part!(v, "ui.getView"),
            Self::Event(v) => part!(v, "ui.event"),
            Self::BeforeClose(v) => part!(v, "ui.beforeClose"),
            Self::Save(v) => part!(v, "ui.save"),
            Self::DisposeView(v) => part!(v, "ui.disposeView"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_retired_callbacks_and_invalid_core_fields() {
        assert_eq!(
            Callback::decode("ui.mount", json!({})).unwrap_err().code,
            -32601
        );
        assert_eq!(
            Callback::decode(
                "commands.handle",
                json!({"commandId":"c","name":"open","context":{},"payload":{}})
            )
            .unwrap_err()
            .code,
            -32602
        );
        let input = json!({"viewId":"v","context":{"workspaceId":"w","serverId":"s","connectionId":"c","generation":1},"data":{"business":"unchanged"}});
        let (method, params) = Callback::decode("ui.event", input.clone())
            .unwrap()
            .into_parts();
        assert_eq!(method, "ui.event");
        assert_eq!(params["context"], input["context"]);
        assert_eq!(params["data"], input["data"]);
    }
}

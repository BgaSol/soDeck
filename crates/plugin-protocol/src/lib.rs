//! The wire contract shared by the core and its independently packaged plugins.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const API_VERSION: &str = "2.2";
pub const FRAME_LIMIT: usize = 64 * 1024;
pub const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

pub fn supports(required: &str) -> bool {
    compatible(API_VERSION, required)
}

pub fn compatible(core: &str, required: &str) -> bool {
    fn version(value: &str) -> Option<(u32, u32)> {
        let (major, minor) = value.split_once('.')?;
        let canonical = |part: &str| {
            !part.is_empty()
                && part.bytes().all(|b| b.is_ascii_digit())
                && (part == "0" || !part.starts_with('0'))
        };
        if !canonical(major) || !canonical(minor) {
            return None;
        }
        Some((major.parse().ok()?, minor.parse().ok()?))
    }
    match (version(core), version(required)) {
        (Some((a, b)), Some((c, d))) => a == c && b >= d,
        _ => false,
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Context {
    pub workspace_id: String,
    pub server_id: String,
    pub connection_id: String,
    pub generation: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApiError {
    pub code: i64,
    pub message: String,
    #[serde(default)]
    pub data: Value,
}
impl ApiError {
    pub fn invalid(message: impl Into<String>) -> Self {
        Self {
            code: -32602,
            message: message.into(),
            data: json!({"code":"invalidParams"}),
        }
    }
    pub fn execution(message: impl Into<String>) -> Self {
        Self {
            code: -32000,
            message: message.into(),
            data: Value::Null,
        }
    }
    pub fn session(code: &str, message: String) -> Self {
        Self {
            code: -32000,
            message,
            data: json!({"code":code}),
        }
    }
    pub fn method_not_found() -> Self {
        Self {
            code: -32601,
            message: "未知核心 API 方法".into(),
            data: json!({"code":"methodNotFound"}),
        }
    }
    pub fn busy() -> Self {
        Self {
            code: -32001,
            message: "请求队列已满".into(),
            data: json!({"code":"resourceBusy","dispatch":"notSent"}),
        }
    }
    pub fn timeout() -> Self {
        Self {
            code: -32003,
            message: "请求超时，结果待确认".into(),
            data: json!({"code":"requestTimeout","dispatch":"unknown"}),
        }
    }
    pub fn local(message: impl Into<String>) -> Self {
        Self {
            code: -32003,
            message: message.into(),
            data: json!({"code":"transportUnavailable","dispatch":"unknown"}),
        }
    }
    pub fn not_sent(message: impl Into<String>) -> Self {
        Self {
            code: -32001,
            message: message.into(),
            data: json!({"code":"requestRejected","dispatch":"notSent"}),
        }
    }
    pub fn from_frame(error: &Value) -> Self {
        serde_json::from_value(error.clone()).unwrap_or_else(|_| Self::local("无效 RPC 错误"))
    }
    pub fn response(self, id: Value) -> Value {
        json!({"jsonrpc":"2.0","id":id,"error":self})
    }
}
impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for ApiError {}
impl From<String> for ApiError {
    fn from(message: String) -> Self {
        Self::execution(message)
    }
}
impl From<&str> for ApiError {
    fn from(message: &str) -> Self {
        Self::execution(message)
    }
}
impl From<ApiError> for String {
    fn from(error: ApiError) -> Self {
        error.message
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Initialize {
    pub core_api_version: String,
    pub core_version: String,
    pub data_directory: String,
    pub settings: Value,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_other_protocols_and_malformed_versions() {
        assert!(supports("2.0"));
        assert!(supports("2.1"));
        for v in ["1", "1.0", "2", "2.00", "2.3", "2.0.0", "3.0", "2.-1", ""] {
            assert!(!supports(v), "{v}");
        }
        assert!(compatible("2.1", "2.0"));
        assert!(!compatible("2.0", "2.1"));
        assert!(!compatible("3.0", "2.0"));
    }
    #[test]
    fn errors_keep_domain_facts() {
        let e = ApiError {
            code: -32000,
            message: "unknown".into(),
            data: json!({"code":"io","effect":"unknown"}),
        };
        let frame = e.clone().response(json!("request"));
        assert_eq!(ApiError::from_frame(&frame["error"]).data, e.data);
    }

    #[test]
    fn incomplete_factual_replies_never_become_success() {
        let request = requests::Request::decode(
            "session.process.input",
            json!({"processId":"p","inputSequence":"1","action":"eof"}),
        )
        .unwrap();
        let error = request
            .validate_reply(&json!({"accepted":true}))
            .unwrap_err();
        assert_eq!(error.data["dispatch"], "unknown");
        assert!(error.data["protocolError"].is_string());
    }

    #[test]
    fn retired_methods_and_old_parameters_are_rejected_before_dispatch() {
        use requests::Request;
        for method in [
            "session.state",
            "session.stream.start",
            "files.request",
            "net.listener.open",
            "net.close",
            "ui.invokeAction",
            "metrics.networkSnapshot",
        ] {
            assert_eq!(
                Request::decode(method, json!({})).unwrap_err().code,
                -32601,
                "{method}"
            );
        }
        let valid = json!({"viewId":"view","expectedContext":{"workspaceId":"workspace","serverId":"server","connectionId":"connection","generation":1}});
        assert!(Request::decode("session.target.describe", valid.clone()).is_ok());
        let mut old = valid;
        old["capabilities"] = json!(["session.target.v2"]);
        assert_eq!(
            Request::decode("session.target.describe", old)
                .unwrap_err()
                .code,
            -32602
        );
        let cancelled = json!({"action":"save.cancel","id":"file","requestId":"save","sequence":"1","expectedContext":{"workspaceId":"workspace","serverId":"server","connectionId":"connection","generation":1},"expectedDraftRef":{"documentId":"document","revision":"1"}});
        let request = Request::decode("files.editor", cancelled.clone()).unwrap();
        assert_eq!(serde_json::to_value(request).unwrap()["params"], cancelled);
    }
}

pub mod session;

pub mod program;
pub mod requests;

pub mod callbacks;
pub mod network;
pub mod replies;
pub mod servers;

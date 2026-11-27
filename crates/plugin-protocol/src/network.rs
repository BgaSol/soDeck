//! Failure facts for network resources; transport-specific classification stays in the core.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FailureCode {
    ForwardingDenied,
    TargetConnectFailed,
    ForwardingUnsupported,
    ResourceLimit,
    ConnectionRefused,
    Timeout,
    SessionLost,
    Cancelled,
    ListenFailed,
    ProtocolError,
    IoFailed,
    #[serde(other)]
    Unknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FailureStage {
    Listen,
    TargetConnect,
    Negotiation,
    Relay,
    Check,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkFailure {
    pub code: FailureCode,
    pub stage: FailureStage,
    pub message: String,
    pub hint: Option<String>,
    pub diagnostic: Option<String>,
}
impl NetworkFailure {
    pub fn new(code: FailureCode, stage: FailureStage, message: &str) -> Self {
        Self {
            code,
            stage,
            message: bounded(message, 256),
            hint: None,
            diagnostic: None,
        }
    }
    pub fn with_hint(mut self, hint: &str) -> Self {
        self.hint = Some(bounded(hint, 384));
        self
    }
    pub fn with_diagnostic(mut self, diagnostic: impl ToString) -> Self {
        self.diagnostic = Some(bounded(&diagnostic.to_string(), 1024));
        self
    }
    pub fn summary(&self) -> Self {
        Self {
            diagnostic: None,
            ..self.clone()
        }
    }
    pub fn client_summary(&self) -> serde_json::Value {
        serde_json::json!({"code":self.code,"stage":self.stage})
    }
    pub fn client_legacy_message(&self) -> String {
        bounded(
            &self
                .message
                .chars()
                .map(|c| if c.is_control() { ' ' } else { c })
                .collect::<String>(),
            128,
        )
    }
    pub fn legacy_message(&self) -> &str {
        self.diagnostic.as_deref().unwrap_or(&self.message)
    }
    pub fn timeout(stage: FailureStage, message: &str) -> Self {
        Self::new(FailureCode::Timeout, stage, message)
    }
    pub fn session_lost(stage: FailureStage) -> Self {
        Self::new(FailureCode::SessionLost, stage, "服务器连接已断开")
    }
    pub fn cancelled(stage: FailureStage) -> Self {
        Self::new(FailureCode::Cancelled, stage, "操作已取消")
    }
    pub fn io(error: std::io::Error, stage: FailureStage) -> Self {
        use std::io::ErrorKind;
        let (code, message) = match error.kind() {
            ErrorKind::ConnectionRefused => (
                FailureCode::ConnectionRefused,
                "连接被拒绝，请检查地址、端口和服务",
            ),
            ErrorKind::TimedOut | ErrorKind::WouldBlock => (FailureCode::Timeout, "网络操作超时"),
            _ => (FailureCode::IoFailed, "网络读写失败"),
        };
        Self::new(code, stage, message).with_diagnostic(error)
    }
}
fn bounded(value: &str, limit: usize) -> String {
    let mut end = value.len().min(limit);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}
impl std::fmt::Display for NetworkFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for NetworkFailure {}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounds_utf8_and_preserves_unknown_failure_facts() {
        let failure = NetworkFailure::new(
            FailureCode::Unknown,
            FailureStage::TargetConnect,
            "连接失败",
        )
        .with_diagnostic("错误".repeat(1000));
        assert!(failure.diagnostic.as_ref().unwrap().len() <= 1024);
        assert!(failure.summary().diagnostic.is_none());
        let mut value = serde_json::to_value(&failure).unwrap();
        value["code"] = "futureFailure".into();
        let decoded: NetworkFailure = serde_json::from_value(value).unwrap();
        assert_eq!(decoded.code, FailureCode::Unknown);
        assert_eq!(decoded.message, failure.message);
    }
    #[test]
    fn maximum_client_failure_summaries_fit_the_control_frame() {
        let failure = NetworkFailure::new(
            FailureCode::Unknown,
            FailureStage::TargetConnect,
            &"\0".repeat(500),
        )
        .with_hint(&"\0".repeat(500))
        .with_diagnostic("\0".repeat(5000));
        let clients:Vec<_>=(0..64).map(|i|serde_json::json!({"id":if i<4{format!("stream@probe:{}","\0".repeat(512))}else{format!("core-stream-{i}")},"state":"failed","probeId":if i<4{Some("\0".repeat(512))}else{None},"target":{"state":"failed","error":failure.client_legacy_message(),"failure":failure.client_summary()}})).collect();
        let frame=serde_json::to_vec(&serde_json::json!({"jsonrpc":"2.0","id":"snapshot","result":{"clients":clients,"failure":failure,"error":{"stage":"listen","message":failure.legacy_message()}}})).unwrap();
        assert!(
            frame.len() + 1 < crate::FRAME_LIMIT,
            "{} bytes",
            frame.len()
        );
    }
}

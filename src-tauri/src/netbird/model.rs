use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Action {
    Up,
    Down,
    ServiceStart,
    ServiceRestart,
}
impl Action {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Up => "连接 NetBird",
            Self::Down => "断开 NetBird",
            Self::ServiceStart => "启动服务",
            Self::ServiceRestart => "重启服务",
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    pub json: bool,
    pub up: bool,
    pub down: bool,
    pub service_start: bool,
    pub service_restart: bool,
    pub reason: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Binding {
    pub token: String,
    pub cli_path: String,
    pub daemon_address: String,
    pub client_version: Option<String>,
    pub daemon_version: Option<String>,
    pub profile: Option<String>,
    pub capabilities: Capabilities,
    #[serde(skip)]
    pub signature: String,
    #[serde(skip)]
    pub service: super::query::Service,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Component {
    pub state: String,
    pub address: Option<String>,
    pub message: Option<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Peer {
    pub id: String,
    pub name: String,
    pub fqdn: Option<String>,
    pub ipv4: Option<String>,
    pub ipv6: Option<String>,
    pub state: String,
    pub raw_state: Option<String>,
    pub connection_mode: String,
    pub latency_ms: Option<f64>,
    pub latency_measured_at: Option<u64>,
    pub last_handshake_at: Option<u64>,
    pub state_changed_at: Option<u64>,
    pub received_bytes: Option<String>,
    pub sent_bytes: Option<String>,
    pub networks: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub binding: Option<Binding>,
    pub collection_status: String,
    pub updated_at: Option<u64>,
    pub error: Option<String>,
    pub installation: String,
    pub service: String,
    pub authentication: String,
    pub connection: String,
    pub local_peer: Option<Peer>,
    pub management: Component,
    pub signal: Component,
    pub relay: Component,
    pub dns: Component,
    pub peers: Vec<Peer>,
    pub peers_complete: bool,
    pub reported_total: Option<u64>,
}
impl Default for Snapshot {
    fn default() -> Self {
        Self {
            binding: None,
            collection_status: "collecting".into(),
            updated_at: None,
            error: None,
            installation: "unknown".into(),
            service: "unknown".into(),
            authentication: "unknown".into(),
            connection: "unknown".into(),
            local_peer: None,
            management: Component::default(),
            signal: Component::default(),
            relay: Component::default(),
            dns: Component::default(),
            peers: vec![],
            peers_complete: false,
            reported_total: None,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Interest {
    pub level: String,
}
impl Interest {
    pub fn validate(&self) -> crate::model::AppResult<()> {
        if ["none", "background", "visible"].contains(&self.level.as_str()) {
            Ok(())
        } else {
            Err(crate::model::AppError::new(
                "validation",
                "无效的 NetBird 订阅",
            ))
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PathAssessment {
    pub result: String,
    pub checked_at: u64,
    pub reasons: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub id: String,
    pub binding_token: String,
    pub action: Action,
    pub expires_at: u64,
    pub allowed: bool,
    pub path: PathAssessment,
    pub consequences: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationRequest {
    pub request_id: String,
    pub binding_token: String,
    pub action: Action,
    pub preview_id: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Operation {
    pub id: String,
    pub request_id: String,
    pub action: Action,
    pub phase: String,
    pub message: String,
    pub created_at: u64,
    pub finished_at: Option<u64>,
    pub acknowledged: bool,
}
impl Operation {
    pub fn active(&self) -> bool {
        matches!(self.phase.as_str(), "queued" | "running" | "verifying")
    }
}
#[derive(Clone, Serialize)]
#[cfg_attr(test, derive(Deserialize))]
#[serde(rename_all = "camelCase")]
pub struct Frame {
    pub subscription_id: String,
    pub revision: u64,
    pub connection_identity: String,
    pub snapshot: Snapshot,
    pub operations: Vec<Operation>,
    pub refreshing: bool,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeRequest {
    pub peer_id: String,
    pub kind: String,
    pub port: Option<u16>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub text: String,
    pub source: String,
    pub checked_at: u64,
    pub truncated: bool,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeResult {
    pub state: String,
    pub message: String,
    pub address: String,
    pub source: String,
    pub checked_at: u64,
}

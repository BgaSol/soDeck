//! Saved paths are ordered protocol steps; resolved plans are immutable snapshots.
use crate::{CoreError, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
pub(crate) mod plan;
pub(crate) mod identity;
pub(crate) mod budget;
pub(crate) mod credentials;
pub use plan::{ResolvedStep, RoutePlan};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AccessPath { pub steps: Vec<RouteStep> }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum RouteStep {
    Ssh { #[serde(rename = "stepId")] step_id: String, #[serde(rename = "serverId")] server_id: String },
    Socks5 { #[serde(rename = "stepId")] step_id: String, #[serde(rename = "proxyId")] proxy_id: String, resolve: ProxyResolve },
}
impl RouteStep {
    pub fn id(&self) -> &str { match self { Self::Ssh { step_id, .. } | Self::Socks5 { step_id, .. } => step_id } }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProxyResolve { Proxy, Local }
impl AccessPath {
    /// A display/observation classification only; execution always uses steps.
    pub fn mode(&self) -> &'static str { if self.steps.is_empty() { "direct" } else { "route" } }
    pub fn contains_ssh(&self, id: &str) -> bool {
        self.steps.iter().any(|step| matches!(step, RouteStep::Ssh { server_id, .. } if server_id == id))
    }
    pub fn validate(&self, target: &str) -> Result<()> {
        if serde_json::to_vec(self)?.len() > 1024 * 1024 {
            return Err(CoreError::new("CONFIGURATION_TOO_LARGE", "连接路径配置超过 1 MiB"));
        }
        let mut ids = std::collections::HashSet::new();
        let mut servers = std::collections::HashSet::from([target]);
        for step in &self.steps {
            if uuid::Uuid::parse_str(step.id()).is_err() || !ids.insert(uuid::Uuid::parse_str(step.id()).unwrap()) {
                return Err(CoreError::new("INVALID_ROUTE", "中转步骤 ID 必须为唯一 UUID"));
            }
            if let RouteStep::Ssh { server_id, .. } = step {
                if !servers.insert(server_id) { return Err(CoreError::new("INVALID_ROUTE", "SSH 跳板不得重复或包含目标自身")); }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SocksProxy {
    pub id: String, pub name: String, pub host: String, pub port: u16, pub auth_type: String,
    #[serde(default)] pub username: String,
    pub revision: u64, #[serde(default)] pub has_preset: bool,
}
impl SocksProxy {
    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() || self.name.len() > 800 || self.host.is_empty() || self.host.len() > 253
            || self.host.chars().any(|c| c.is_whitespace() || c == '/' || c == '\\' || c == '\0') || self.port == 0
            || !matches!(self.auth_type.as_str(), "none" | "usernamePassword")
            || (self.auth_type == "usernamePassword" && !(1..=255).contains(&self.username.len()))
            || (self.auth_type == "none" && !self.username.is_empty()) {
            return Err(CoreError::new("INVALID_PARAMS", "网络代理地址、端口或认证配置无效"));
        }
        Ok(())
    }
}
pub fn digest(value: &Value) -> String {
    format!("{:x}", Sha256::digest(serde_json::to_vec(value).expect("JSON value")))
}

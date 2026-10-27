//! Connection identity is a snapshot of every dependency, not just the target.
use crate::{CoreError, Result, Server};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum ConnectionRoute {
    #[default]
    Direct,
    SshJump { #[serde(rename = "jumpServerId")] jump_server_id: String },
}
impl ConnectionRoute {
    pub fn jump_id(&self) -> Option<&str> {
        match self { Self::Direct => None, Self::SshJump { jump_server_id } => Some(jump_server_id) }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteNode {
    pub server: Server,
    pub credential_revision: u64,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutePlan {
    pub target_server_id: String,
    pub revision: String,
    pub nodes: Vec<RouteNode>,
}
pub fn digest(value: &Value) -> String {
    format!("{:x}", Sha256::digest(serde_json::to_vec(value).expect("JSON value")))
}
impl RoutePlan {
    pub fn new(nodes: Vec<RouteNode>) -> Result<Self> {
        let mut visited = std::collections::HashSet::new();
        if nodes.is_empty() || nodes.iter().any(|node| !visited.insert(&node.server.id)) {
            return Err(CoreError::new("INVALID_ROUTE", "连接路径不能为空或包含循环引用"));
        }
        let target_server_id = nodes.last().unwrap().server.id.clone();
        let revision = digest(&json!(nodes.iter().map(|n| json!([n.server.id,n.server.revision,n.credential_revision])).collect::<Vec<_>>()));
        Ok(Self { target_server_id, revision, nodes })
    }
    pub fn target(&self) -> &Server { &self.nodes.last().unwrap().server }
    pub fn contains(&self, id: &str) -> bool { self.nodes.iter().any(|n| n.server.id == id) }
    /// Names, config counters and password rotations do not change the identity
    /// of a network. Accounts do: a gateway account can expose a different network.
    pub fn credential_context(&self, index: usize) -> String {
        digest(&json!(self.nodes[..=index].iter().map(|n| json!([n.server.id,n.server.host,n.server.port,n.server.username,n.server.auth_type])).collect::<Vec<_>>()))
    }
    pub fn trust_key(&self, index: usize, verified: &[String]) -> String {
        let server = &self.nodes[index].server;
        if index == 0 { return format!("{}:{}", server.host, server.port); }
        format!("route:{}", digest(&json!([self.credential_context(index - 1),verified,server.host,server.port])))
    }
    pub fn summary(&self) -> Value {
        json!({"targetServerId":self.target_server_id,"routeRevision":self.revision,"nodes":(0..self.nodes.len()).map(|i|self.node_summary(i)).collect::<Vec<_>>()})
    }
    pub fn node_summary(&self, i: usize) -> Value {
        let n = &self.nodes[i];
        json!({"serverId":n.server.id,"name":n.server.name,"host":n.server.host,"port":n.server.port,"username":n.server.username,"authType":n.server.auth_type,"configRevision":n.server.revision,"role":if i+1==self.nodes.len(){"target"}else{"jump"}})
    }
    pub fn require_v1(&self) -> Result<()> {
        if self.nodes.len() > 2 { Err(CoreError::new("ROUTE_VERSION_REQUIRED", "多级路径需要路由 v2 接口")) } else { Ok(()) }
    }
}

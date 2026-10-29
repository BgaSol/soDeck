//! Resolved protocol steps. A proxy is a step, never an attribute of an SSH hop.
use super::{AccessPath, ProxyResolve, SocksProxy, budget::Cost, digest, identity};
use crate::{Server, CoreError, Result};
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SshStep {
    pub step_id: String,
    pub server: Server,
    pub credential_revision: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyStep {
    pub step_id: String,
    pub proxy: SocksProxy,
    pub resolve: ProxyResolve,
    pub credential_revision: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ResolvedStep {
    Ssh(SshStep),
    Socks5(ProxyStep),
}

impl ResolvedStep {
    pub fn id(&self) -> &str {
        match self { Self::Ssh(step) => &step.step_id, Self::Socks5(step) => &step.step_id }
    }
    pub fn endpoint(&self) -> (&str, u16) {
        match self {
            Self::Ssh(step) => (&step.server.host, step.server.port),
            Self::Socks5(step) => (&step.proxy.host, step.proxy.port),
        }
    }
    pub fn owner_id(&self) -> &str { match self { Self::Ssh(s) => &s.server.id, Self::Socks5(s) => &s.proxy.id } }
    pub fn owner_kind(&self) -> &'static str { match self { Self::Ssh(_) => "ssh", Self::Socks5(_) => "socks5" } }
    pub fn revision(&self) -> u64 { match self { Self::Ssh(s) => s.server.revision, Self::Socks5(s) => s.proxy.revision } }
    pub fn credential_revision(&self) -> u64 { match self { Self::Ssh(s) => s.credential_revision, Self::Socks5(s) => s.credential_revision } }
    pub fn ssh(&self) -> Result<&Server> { match self { Self::Ssh(s) => Ok(&s.server), _ => Err(CoreError::new("INVALID_ROUTE", "该步骤不是 SSH")) } }
    pub fn proxy(&self) -> Result<&SocksProxy> { match self { Self::Socks5(s) => Ok(&s.proxy), _ => Err(CoreError::new("INVALID_ROUTE", "该步骤不是 SOCKS5")) } }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutePlan {
    pub target_server_id: String,
    pub revision: String,
    pub identity_revision: String,
    pub steps: Vec<ResolvedStep>,
    pub access_path: AccessPath,
    pub cost: Cost,
}
impl RoutePlan {
    pub fn from_steps(steps: Vec<ResolvedStep>, access_path: AccessPath) -> Result<Self> {
        let target = steps.last().ok_or_else(|| CoreError::new("INVALID_ROUTE", "连接路径不能为空"))?.ssh()?;
        if steps.last().unwrap().id() != "target" { return Err(CoreError::new("INVALID_ROUTE", "缺少最终 SSH 目标")); }
        let target_server_id = target.id.clone();
        access_path.validate(&target_server_id)?;
        let identity: Vec<_> = steps.iter().map(|s| json!([identity::identity(s),match s { ResolvedStep::Socks5(s) => Some(s.resolve), _ => None }])).collect();
        let identity_revision = digest(&json!(identity));
        let revision = digest(&json!([identity_revision,steps.iter().map(ResolvedStep::credential_revision).collect::<Vec<_>>()]));
        let cost = Cost::for_steps(&steps);
        let mut plan=Self { target_server_id, revision, identity_revision, steps, access_path, cost };
        // Include the saved path and metadata, not only the resolved steps.
        loop {let bytes=serde_json::to_vec(&plan)?.len();if bytes==plan.cost.snapshot_bytes{break}plan.cost.snapshot_bytes=bytes;}
        Ok(plan)
    }
    pub fn target(&self) -> &Server { self.steps.last().unwrap().ssh().expect("validated target") }
    pub fn contains(&self, id: &str) -> bool { self.steps.iter().any(|s| matches!(s, ResolvedStep::Ssh(s) if s.server.id == id)) }
    pub fn contains_proxy(&self, id: &str) -> bool { self.steps.iter().any(|s| matches!(s, ResolvedStep::Socks5(s) if s.proxy.id == id)) }
    pub fn arrival(&self,index:usize)->Value {
        let (mechanism,resolve)=if index==0 {("tcp","local")}
            else {match &self.steps[index-1] {ResolvedStep::Ssh(_)=>("sshForward","ssh"),ResolvedStep::Socks5(s)=>("socksConnect",if s.resolve==ProxyResolve::Local{"local"}else{"proxy"})}};
        let from=if index==0{"local"}else{self.steps[index-1].id()};
        json!({"edgeId":format!("{from}:{}",self.steps[index].id()),"fromStepId":from,"toStepId":self.steps[index].id(),"mechanism":mechanism,"resolve":resolve})
    }
    pub fn node_summary(&self, index: usize) -> Value {
        let step = &self.steps[index];
        let (host,port) = step.endpoint();
        let mut value = json!({"stepId":step.id(),"stepIndex":index,"host":host,"port":port,"configRevision":step.revision(),"arrival":self.arrival(index),"hasBoundCredential":false,
            "fromStepId":if index == 0 { "local" } else { self.steps[index-1].id() }});
        match step {
            ResolvedStep::Ssh(s) => {
                value["kind"] = json!("ssh"); value["serverId"] = json!(s.server.id); value["name"] = json!(s.server.name);
                value["username"] = json!(s.server.username); value["authType"] = json!(s.server.auth_type);
                value["role"] = json!(if index+1==self.steps.len() { "target" } else { "jump" });
            }
            ResolvedStep::Socks5(s) => {
                value["kind"] = json!("socks5"); value["proxyId"] = json!(s.proxy.id); value["name"] = json!(s.proxy.name);
                value["username"] = json!(s.proxy.username); value["authType"] = json!(s.proxy.auth_type);
                value["role"] = json!("proxy"); value["resolve"] = json!(s.resolve);
            }
        }
        value
    }
}

//! Current target identity, read-only discovery and literal matching.
use crate::app::App;
#[cfg(test)]
use crate::identity::endpoint;
use crate::identity::{host, MatchMode, Needle};
use crate::{task::Kind, tasks::failure};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Context {
    pub workspace_id: String,
    pub server_id: String,
    pub connection_id: String,
    pub generation: u64,
}
fn valid_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 256 && !s.contains('\0')
}
impl Context {
    pub(crate) fn validate(&self) -> bool {
        valid_id(&self.workspace_id)
            && valid_id(&self.server_id)
            && valid_id(&self.connection_id)
            && self.generation > 0
    }
}
#[derive(Clone, Copy)]
pub enum AdmissionDomain {
    Exec,
    Files,
}

#[derive(Default)]
pub struct Admission {
    pub sequence: u64,
    pub file: Option<String>,
    pub busy: bool,
}
pub struct Target {
    pub id: String,
    pub order: u64,
    pub view: String,
    pub context: Context,
    pub closed: AtomicBool,
    exec: Mutex<Admission>,
    file: Mutex<Admission>,
}
impl Target {
    pub fn new(id: String, order: u64, view: String, context: Context) -> Self {
        Self {
            id,
            order,
            view,
            context,
            closed: AtomicBool::new(false),
            exec: Mutex::new(Admission::default()),
            file: Mutex::new(Admission::default()),
        }
    }
    pub fn admission(&self, kind: Kind) -> &Mutex<Admission> {
        match kind.domain() {
            AdmissionDomain::Exec => &self.exec,
            AdmissionDomain::Files => &self.file,
        }
    }
    pub fn release_admission(&self, kind: Kind) {
        self.admission(kind).lock().unwrap().busy = false;
    }
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Params {
    query: Option<String>,
    #[serde(rename = "match")]
    mode: Option<MatchMode>,
    server_id: Option<String>,
    workspace_id: Option<String>,
    target_id: Option<String>,
    #[serde(default)]
    detail: bool,
}
pub struct TargetsQuery {
    needle: Option<Needle>,
    server: Option<String>,
    workspace: Option<String>,
    target: Option<String>,
    pub detail: bool,
}
impl TargetsQuery {
    pub fn parse(p: &Value) -> Result<Self, Value> {
        let params: Params = serde_json::from_value(p.clone())
            .map_err(|e| failure("invalidParams", e.to_string()))?;
        // Explicit null is not an omitted optional argument.
        if p.as_object()
            .is_some_and(|o| o.values().any(Value::is_null))
            || params.mode.is_some() && params.query.is_none()
            || params.target_id.is_some()
                && (params.query.is_some()
                    || params.mode.is_some()
                    || params.server_id.is_some()
                    || params.workspace_id.is_some())
            || [&params.server_id, &params.workspace_id, &params.target_id]
                .iter()
                .any(|s| s.as_ref().is_some_and(|s| !valid_id(s)))
        {
            return Err(failure("invalidParams", "目标查询参数组合或身份无效"));
        }
        let needle = params
            .query
            .map(|s| {
                let text = s.trim();
                if text.is_empty() || text.len() > 256 || s.chars().any(char::is_control) {
                    return Err(failure("invalidParams", "query 为空、超限或含控制字符"));
                }
                Ok(Needle::new(text, params.mode.unwrap_or_default()))
            })
            .transpose()?;
        Ok(Self {
            needle,
            server: params.server_id,
            workspace: params.workspace_id,
            target: params.target_id,
            detail: params.detail,
        })
    }
    fn includes(&self, t: &Target) -> bool {
        self.target.as_ref().is_none_or(|s| s == &t.id)
            && self
                .server
                .as_ref()
                .is_none_or(|s| s == &t.context.server_id)
            && self
                .workspace
                .as_ref()
                .is_none_or(|s| s == &t.context.workspace_id)
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct Descriptor {
    pub name: String,
    pub protocol: String,
    pub host: String,
    pub port: u16,
    pub endpoint: String,
    pub username: Option<String>,
}
impl Descriptor {
    fn validate(&self) -> bool {
        let h = if self.host.contains(':') {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };
        !self.name.is_empty()
            && !self.protocol.is_empty()
            && host(&self.host).is_some()
            && self.port > 0
            && self.endpoint == format!("{h}:{}", self.port)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Connected,
    Disconnected,
}
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    Current,
    LastConnected,
    Unknown,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Observation {
    pub context: Context,
    pub target: Option<Descriptor>,
    pub target_source: Source,
    pub state: State,
    pub closing: bool,
    pub capabilities: Vec<String>,
}
impl Observation {
    pub(crate) fn validate(&self) -> bool {
        self.context.validate()
            && self.target.as_ref().is_none_or(Descriptor::validate)
            && match self.target_source {
                Source::Current => self.state == State::Connected && self.target.is_some(),
                Source::LastConnected => self.state == State::Disconnected && self.target.is_some(),
                Source::Unknown => self.state == State::Disconnected && self.target.is_none(),
            }
    }
    pub fn availability(&self) -> &'static str {
        if self.closing {
            "closing"
        } else if self.state == State::Connected {
            "ready"
        } else {
            "disconnected"
        }
    }
}
pub struct Candidate {
    pub target: Arc<Target>,
    pub observation: Option<Observation>,
    pub error: Option<Value>,
    pub matched_by: Vec<&'static str>,
}
pub struct Discovery {
    pub run: String,
    pub detail: bool,
    pub with_query: bool,
    pub targets: Vec<Candidate>,
    pub unresolved: Vec<Candidate>,
    pub registry_changed: bool,
}
impl Discovery {
    pub fn resolution(&self) -> &'static str {
        if self.registry_changed || !self.unresolved.is_empty() {
            "incomplete"
        } else if self.targets.len() > 1 {
            "ambiguous"
        } else if self.targets.is_empty() {
            "none"
        } else {
            "unique"
        }
    }
}
pub fn error_availability(error: &Value) -> &'static str {
    if error["code"] == "staleContext" || error["details"]["code"] == "staleContext" {
        "stale"
    } else {
        "unavailable"
    }
}
impl App {
    pub fn mount(&self, view: &str, context: &Value) -> Result<(), String> {
        let context: Context =
            serde_json::from_value(context.clone()).map_err(|_| "工作区上下文无效")?;
        if !valid_id(view) || !context.validate() {
            return Err("工作区上下文无效".into());
        }
        let mut views = self.views.lock().unwrap();
        if let Some(old) = views.get(view) {
            if old.context == context {
                return Ok(());
            }
            if context.generation < old.context.generation
                || context.generation == old.context.generation
            {
                return Err("旧连接上下文不能重绑视图".into());
            }
        }
        if views.len() >= 64 && !views.contains_key(view) {
            return Err("服务器目标已满".into());
        }
        let order = self.numbers.fetch_add(1, Ordering::Relaxed);
        let target = Arc::new(Target::new(
            format!("{}-target-{order}", self.run),
            order,
            view.into(),
            context,
        ));
        if let Some(old) = views.insert(view.into(), target) {
            old.closed.store(true, Ordering::Release);
        }
        self.target_changes.send_modify(|n| *n += 1);
        Ok(())
    }
    pub fn dispose(&self, view: &str, context: &Value) -> Result<(), String> {
        let context: Context =
            serde_json::from_value(context.clone()).map_err(|_| "工作区上下文无效")?;
        let mut views = self.views.lock().unwrap();
        if views.get(view).is_some_and(|t| t.context != context) {
            return Ok(());
        }
        if let Some(target) = views.remove(view) {
            target.closed.store(true, Ordering::Release);
            self.target_changes.send_modify(|n| *n += 1);
        }
        Ok(())
    }
    pub(crate) fn context_target(&self, context: &Context, view: &str) -> Option<Arc<Target>> {
        self.views
            .lock()
            .unwrap()
            .get(view)
            .filter(|t| t.context == *context && !t.closed.load(Ordering::Acquire))
            .cloned()
    }
    pub fn view_target(&self, view: &str) -> Result<Arc<Target>, Value> {
        self.views
            .lock()
            .unwrap()
            .get(view)
            .cloned()
            .ok_or_else(|| failure("staleContext", "视图已关闭"))
    }
    fn scope(&self, query: &TargetsQuery) -> Vec<Arc<Target>> {
        let mut targets: Vec<_> = self
            .views
            .lock()
            .unwrap()
            .values()
            .filter(|t| query.includes(t))
            .cloned()
            .collect();
        targets.sort_by_key(|t| t.order);
        targets
    }
    pub fn observe_target(&self, target: &Target) -> Result<Observation, Value> {
        if target.closed.load(Ordering::Acquire) {
            return Err(failure("staleContext", "目标已关闭"));
        }
        let value = self.rpc(
            "session.target.describe",
            json!({"viewId":target.view,"expectedContext":target.context}),
        )?;
        if target.closed.load(Ordering::Acquire) {
            return Err(failure("staleContext", "采集期间目标已关闭"));
        }
        let observation:Observation=serde_json::from_value(value.clone()).map_err(|e|json!({"code":"invalidTarget","message":format!("目标返回无效：{e}"),"details":value}))?;
        if observation.context != target.context {
            return Err(failure("staleContext", "核心返回另一连接代次"));
        }
        if !observation.validate() {
            return Err(
                json!({"code":"invalidTarget","message":"目标事实字段不一致","details":value}),
            );
        }
        Ok(observation)
    }
    fn collect_targets(
        &self,
        query: TargetsQuery,
        ct: &CancellationToken,
    ) -> Result<Discovery, Value> {
        let scope = self.scope(&query);
        let mut result = Discovery {
            run: self.run.clone(),
            detail: query.detail,
            with_query: query.needle.is_some(),
            targets: Vec::new(),
            unresolved: Vec::new(),
            registry_changed: false,
        };
        let mut communication_failure = None;
        for target in &scope {
            if ct.is_cancelled() {
                return Err(failure("requestCancelled", "目标查询已取消"));
            }
            if self.shutdown.is_cancelled() {
                return Err(failure("unavailable", "插件正在退出"));
            }
            let observation = if let Some(cause) = &communication_failure {
                Err(
                    json!({"code":"notObserved","message":"核心通信失败后未继续采集此项","details":{"cause":cause}}),
                )
            } else {
                self.observe_target(target)
            };
            if ct.is_cancelled() {
                return Err(failure("requestCancelled", "目标查询已取消"));
            }
            let candidate = match observation {
                Ok(observation) => {
                    let matched_by = match &query.needle {
                        Some(needle) => match &observation.target {
                            Some(description) => {
                                let reasons = needle.matches(description);
                                if reasons.is_empty() {
                                    continue;
                                }
                                reasons
                            }
                            None => Vec::new(),
                        },
                        None => Vec::new(),
                    };
                    Candidate {
                        target: target.clone(),
                        observation: Some(observation),
                        error: None,
                        matched_by,
                    }
                }
                Err(error) => {
                    if communication_failure.is_none()
                        && (error["code"] == -32003 || self.client.is_closed())
                    {
                        communication_failure = Some(error.clone());
                    }
                    Candidate {
                        target: target.clone(),
                        observation: None,
                        error: Some(error),
                        matched_by: Vec::new(),
                    }
                }
            };
            if result.with_query && candidate.matched_by.is_empty() {
                result.unresolved.push(candidate);
            } else {
                result.targets.push(candidate);
            }
        }
        let after = self.scope(&query);
        result.registry_changed = scope.len() != after.len()
            || scope
                .iter()
                .zip(after.iter())
                .any(|(a, b)| a.id != b.id || a.context != b.context);
        Ok(result)
    }
}
pub async fn discover(app: Arc<App>, p: &Value, ct: CancellationToken) -> Result<Discovery, Value> {
    let query = TargetsQuery::parse(p)?;
    if ct.is_cancelled() {
        return Err(failure("requestCancelled", "目标查询已取消"));
    }
    if app.shutdown.is_cancelled() {
        return Err(failure("unavailable", "插件正在退出"));
    }
    let permit = app
        .target_reads
        .clone()
        .try_acquire_owned()
        .map_err(|_| failure("resourceBusy", "目标采集已满，请稍后查询"))?;
    let child = ct.child_token();
    let _cancel = child.clone().drop_guard();
    let worker = app.clone();
    let job = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        worker.collect_targets(query, &child)
    });
    tokio::select! {
        result=job=>result.map_err(|e|failure("unavailable",e.to_string()))?,
        _=ct.cancelled()=>Err(failure("requestCancelled","目标查询已取消")),
        _=app.shutdown.cancelled()=>Err(failure("unavailable","插件正在退出")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn descriptor(name: &str, h: &str, p: u16) -> Descriptor {
        let address = if h.contains(':') {
            format!("[{h}]")
        } else {
            h.into()
        };
        Descriptor {
            name: name.into(),
            protocol: "ssh".into(),
            host: h.into(),
            port: p,
            endpoint: format!("{address}:{p}"),
            username: Some("ops".into()),
        }
    }
    #[test]
    fn queries_are_literal_and_combinations_are_validated() {
        for p in [
            json!({"query":""}),
            json!({"query":"a\n"}),
            json!({"query":"中".repeat(86)}),
            json!({"query":null}),
            json!({"detail":null}),
            json!({"detail":"true"}),
            json!({"match":"contains"}),
            json!({"targetId":"t","serverId":"s"}),
            json!({"targetId":"t","query":"q"}),
            json!({"unknown":1}),
        ] {
            assert!(TargetsQuery::parse(&p).is_err(), "{p}");
        }
        assert!(TargetsQuery::parse(
            &json!({"query":" 生产 ","serverId":"s","workspaceId":"w","detail":true})
        )
        .is_ok());
        assert!(TargetsQuery::parse(&json!({})).is_ok());
    }
    #[test]
    fn matching_unions_identity_evidence_without_address_substrings() {
        let query = Needle::new("187", MatchMode::Exact);
        assert_eq!(
            query.matches(&descriptor("187", "10.0.0.187", 22)),
            vec!["nameExact", "ipv4LastOctet"]
        );
        for (name, h) in [
            ("2187", "10.0.0.18"),
            ("db187", "db187.example.com"),
            ("x", "2001:db8::187"),
        ] {
            assert!(query.matches(&descriptor(name, h, 22)).is_empty());
        }
        assert!(Needle::new("0187", MatchMode::Exact)
            .matches(&descriptor("x", "10.0.0.187", 22))
            .is_empty());
        assert_eq!(
            Needle::new("prod", MatchMode::Contains).matches(&descriptor(
                "PROD web",
                "db.prod.example.com",
                22
            )),
            vec!["nameContains", "hostnameContains"]
        );
        assert!(Needle::new(".0.", MatchMode::Contains)
            .matches(&descriptor("x", "10.0.0.187", 22))
            .is_empty());
        assert!(Needle::new(".", MatchMode::Contains)
            .matches(&descriptor("x", "localhost", 22))
            .is_empty());
    }
    #[test]
    fn hosts_endpoints_and_ipv6_scopes_are_compared_explicitly() {
        assert_eq!(
            Needle::new("DB.EXAMPLE.COM.", MatchMode::Exact).matches(&descriptor(
                "x",
                "db.example.com",
                2222
            )),
            vec!["hostExact"]
        );
        assert!(Needle::new("db.example.com:22", MatchMode::Exact)
            .matches(&descriptor("x", "db.example.com", 2222))
            .is_empty());
        assert_eq!(
            Needle::new("db.example.com:2222", MatchMode::Exact).matches(&descriptor(
                "x",
                "db.example.com",
                2222
            )),
            vec!["endpointExact"]
        );
        assert_eq!(
            Needle::new("2001:0db8:0:0:0:0:0:1", MatchMode::Exact).matches(&descriptor(
                "x",
                "2001:db8::1",
                22
            )),
            vec!["hostExact"]
        );
        assert_eq!(
            Needle::new("[fe80::1%eth0]:22", MatchMode::Exact).matches(&descriptor(
                "x",
                "fe80::1%eth0",
                22
            )),
            vec!["endpointExact"]
        );
        assert!(Needle::new("fe80::1%eth1", MatchMode::Exact)
            .matches(&descriptor("x", "fe80::1%eth0", 22))
            .is_empty());
        assert!(endpoint("host:0").is_none());
        assert!(endpoint("host:65536").is_none());
        assert!(host("[db.example.com]").is_none());
        assert!(host("[127.0.0.1]").is_none());
    }
}

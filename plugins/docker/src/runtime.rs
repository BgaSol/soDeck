use crate::{
    browser::{self, BrowserState, Work},
    cli,
    collaboration::{self, Intent, Kind as CommandKind, Work as CommandWork},
    commands,
    connection::{self, Config, EngineOs, Snapshot, Target},
    object::ObjectInfo,
    ports,
    resources::{self, Kind, ResourceRef},
};
use serde_json::{json, Value};
use sodeck_plugin_client::protocol::requests as api;
use sodeck_plugin_client::Client;
use std::{
    collections::{BTreeMap, VecDeque},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub type Shared = Arc<Mutex<App>>;
pub struct App {
    pub views: BTreeMap<String, View>,
    pub configs: BTreeMap<String, Config>,
    bindings: BTreeMap<String, Snapshot>,
    binding_revision: u64,
    pub exec_dispatch: Arc<Mutex<()>>,
    pub directory: PathBuf,
    pub refresh_seconds: u64,
    pub settings_revision: u64,
    jobs: VecDeque<Job>,
}
pub struct View {
    pub stream_dispatch: Arc<Mutex<()>>,
    pub browser: Option<BrowserState>,
    pub epoch: u64,
    pub pull: Value,
    pub tasks: VecDeque<Value>,
    pub terminal: Intent,
    pub tunnel: Intent,
    pub kind: String,
    pub context: Value,
    pub container: String,
    pub expected_engine: String,
    pub endpoint: String,
    pub target: Option<Snapshot>,
    connection_test: Value,
    pub revision: u64,
    pub data: Value,
    pub navigation: Value,
    pub busy: bool,
    pub frozen: bool,
    pub paused: bool,
    pub cancelled: bool,
    sequence: u64,
    intent: u64,
    receipts: VecDeque<(String, u64, Value)>,
    due: Instant,
}
#[derive(Clone)]
enum ConnectionAction {
    Test,
    Save,
    Identify,
    SetEndpoint,
}
#[derive(Clone)]
enum ContainerAction {
    Start,
    Stop,
    Restart,
    Pause,
    Unpause,
    Remove,
}
#[derive(Clone)]
enum JobAction {
    Refresh,
    Stats,
    Logs,
    Connection(ConnectionAction),
    Container(ContainerAction),
    Browser(Work),
    ResourceRemove {
        observation: Value,
        scan: Option<DeleteScan>,
    },
    Collaboration(CommandWork),
}
impl JobAction {
    fn parse(name: &str, view: &View) -> Result<Self, String> {
        Ok(match name {
            "refresh" => Self::Refresh,
            "stats" => Self::Stats,
            "connection.test" => Self::Connection(ConnectionAction::Test),
            "connection.save" => Self::Connection(ConnectionAction::Save),
            "connection.identify" => Self::Connection(ConnectionAction::Identify),
            "setEndpoint" => Self::Connection(ConnectionAction::SetEndpoint),
            "start" => Self::Container(ContainerAction::Start),
            "stop" => Self::Container(ContainerAction::Stop),
            "restart" => Self::Container(ContainerAction::Restart),
            "pause" => Self::Container(ContainerAction::Pause),
            "unpause" => Self::Container(ContainerAction::Unpause),
            "remove" => Self::Container(ContainerAction::Remove),
            "resource.remove" => Self::ResourceRemove {
                observation: Value::Null,
                scan: None,
            },
            "terminal" | "terminalStatus" => Self::Collaboration(CommandWork {
                kind: CommandKind::Terminal,
                intent: view.terminal.clone(),
                submit: name == "terminal",
            }),
            "tunnel" | "tunnelStatus" => Self::Collaboration(CommandWork {
                kind: CommandKind::Tunnel,
                intent: view.tunnel.clone(),
                submit: name == "tunnel",
            }),
            _ => return Err("未知 Docker 后台任务".into()),
        })
    }
    fn as_str(&self) -> &'static str {
        match self {
            Self::Refresh => "refresh",
            Self::Stats => "stats",
            Self::Logs => "logs",
            Self::Browser(_) => "browser.read",
            Self::ResourceRemove { .. } => "resource.remove",
            Self::Connection(kind) => match kind {
                ConnectionAction::Test => "connection.test",
                ConnectionAction::Save => "connection.save",
                ConnectionAction::Identify => "connection.identify",
                ConnectionAction::SetEndpoint => "setEndpoint",
            },
            Self::Container(kind) => match kind {
                ContainerAction::Start => "start",
                ContainerAction::Stop => "stop",
                ContainerAction::Restart => "restart",
                ContainerAction::Pause => "pause",
                ContainerAction::Unpause => "unpause",
                ContainerAction::Remove => "remove",
            },
            Self::Collaboration(work) => match (work.kind, work.submit) {
                (CommandKind::Terminal, true) => "terminal",
                (CommandKind::Terminal, false) => "terminalStatus",
                (CommandKind::Tunnel, true) => "tunnel",
                (CommandKind::Tunnel, false) => "tunnelStatus",
            },
        }
    }
    fn browser(&self) -> Option<&Work> {
        if let Self::Browser(work) = self {
            Some(work)
        } else {
            None
        }
    }
    fn command(&self) -> Option<&CommandWork> {
        if let Self::Collaboration(work) = self {
            Some(work)
        } else {
            None
        }
    }
}
impl serde::Serialize for JobAction {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}
#[derive(Clone)]
struct Job {
    epoch: u64,
    resource: Option<ResourceRef>,
    view: String,
    context: Value,
    endpoint: String,
    container: String,
    engine: String,
    action: JobAction,
    target: Option<Snapshot>,
    config: Config,
    deadline: Option<Instant>,
}
struct Exec {
    bytes: Vec<u8>,
    stderr: Vec<u8>,
    response: Value,
}
#[derive(Clone)]
struct DeleteScan {
    current: Value,
    ids: Vec<String>,
    offset: usize,
    refs: Vec<Value>,
}
impl View {
    pub fn intent_receipt(&self, request: &str) -> Option<&(String, u64, Value)> {
        self.receipts.iter().find(|(r, _, _)| r == request)
    }
    pub fn sequence_for_stream(&mut self) {
        self.sequence += 1;
    }
    pub fn command_sequence(&self) -> u64 {
        self.sequence
    }
    pub fn container_rows(&self) -> &[Value] {
        self.browser
            .as_ref()
            .map(|b| b.indexes[&Kind::Container].rows.as_slice())
            .unwrap_or_else(|| {
                self.data["rows"]
                    .as_array()
                    .map(Vec::as_slice)
                    .unwrap_or(&[])
            })
    }
    pub fn cache_bytes(&self) -> usize {
        browser::bytes(&self.data)
            + self.browser.as_ref().map_or(0, BrowserState::bytes)
            + browser::bytes(&self.navigation)
            + browser::bytes(&self.pull)
            + browser::bytes(&json!([
                self.tasks,
                self.receipts,
                self.terminal.input,
                self.terminal.receipt,
                self.tunnel.input,
                self.tunnel.receipt
            ]))
            + self
                .terminal
                .ticket
                .as_ref()
                .map_or(0, |t| browser::bytes(&t.payload))
            + self
                .tunnel
                .ticket
                .as_ref()
                .map_or(0, |t| browser::bytes(&t.payload))
    }
    pub fn command_mut(&mut self, kind: CommandKind) -> &mut Intent {
        match kind {
            CommandKind::Terminal => &mut self.terminal,
            CommandKind::Tunnel => &mut self.tunnel,
        }
    }
    pub fn clear_commands(&mut self) {
        self.terminal.clear();
        self.tunnel.clear();
    }
    pub fn clear_terminal(&mut self) {
        self.terminal.clear();
    }
}
pub(crate) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
impl App {
    pub fn new(directory: PathBuf) -> Result<Self, String> {
        let configs = connection::load(&directory)?;
        Ok(Self {
            views: BTreeMap::new(),
            configs,
            bindings: BTreeMap::new(),
            binding_revision: 0,
            exec_dispatch: Arc::new(Mutex::new(())),
            directory,
            refresh_seconds: 5,
            settings_revision: 0,
            jobs: VecDeque::new(),
        })
    }
    pub fn create(&mut self, p: &Value) -> Result<(), String> {
        self.prune_bindings();
        let id = p["viewId"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 128)
            .ok_or("视图 ID 无效")?;
        let kind = p["viewType"].as_str().ok_or("缺少视图类型")?;
        if !["browser", "logs"].contains(&kind) {
            return Err("未知 Docker 视图".into());
        }
        if self.views.contains_key(id) {
            return Ok(());
        }
        if self.views.len() >= 64 {
            return Err("Docker 页面已达上限".into());
        }
        let context = p["context"].clone();
        if !context["serverId"].is_string()
            || !context["workspaceId"].is_string()
            || !context["connectionId"].is_string()
            || !context["generation"].is_u64()
        {
            return Err("连接上下文无效".into());
        }
        if p["resourceKey"].as_str().is_some_and(|s| s.len() > 2048) {
            return Err("资源标识超过 2 KiB".into());
        }
        let resource: Value = p["resourceKey"]
            .as_str()
            .map(serde_json::from_str)
            .transpose()
            .map_err(|_| "资源标识无效")?
            .unwrap_or(Value::Null);
        let container = resource["id"].as_str().unwrap_or("").to_string();
        if kind == "logs" {
            commands::id(&container)?;
        }
        let target = self.bindings.get(&context.to_string()).cloned();
        let endpoint = target
            .as_ref()
            .map(|t| t.endpoint.uri())
            .unwrap_or_else(|| resource["endpoint"].as_str().unwrap_or("").into());
        if !endpoint.is_empty() {
            connection::Endpoint::parse(&endpoint)?;
        }
        if let Some(t) = &target {
            if resource["engineId"]
                .as_str()
                .is_some_and(|id| id != t.engine_id)
                || resource["endpoint"]
                    .as_str()
                    .is_some_and(|e| e != t.endpoint.uri())
            {
                return Err("原资源连接身份已改变，请从列表重新打开".into());
            }
        }
        let view = View {
            stream_dispatch: Arc::new(Mutex::new(())),
            browser: if kind == "browser" {
                Some(BrowserState::default())
            } else {
                None
            },
            epoch: 1,
            pull: Value::Null,
            tasks: VecDeque::new(),
            terminal: Intent::default(),
            tunnel: Intent::default(),
            kind: kind.into(),
            context,
            container,
            expected_engine: resource["engineId"]
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| {
                    target
                        .as_ref()
                        .map(|t| t.engine_id.clone())
                        .unwrap_or_default()
                }),
            endpoint,
            target,
            connection_test: Value::Null,
            revision: 1,
            data: json!({"state":"loading","error":null,"engine":null,"rows":[],"stats":null,"operation":null}),
            navigation: if kind == "browser" {
                browser::navigation(&json!({}), &Value::Null)?
            } else {
                json!({})
            },
            busy: false,
            frozen: false,
            paused: false,
            cancelled: false,
            sequence: 0,
            intent: 0,
            receipts: VecDeque::new(),
            due: Instant::now() + Duration::from_secs(self.refresh_seconds),
        };
        self.views.insert(id.into(), view);
        self.enqueue(id, "refresh")
    }
    pub fn prune_bindings(&mut self) {
        self.bindings
            .retain(|_, t| self.views.values().any(|v| v.context == t.context));
    }
    fn version_target(&mut self, target: Snapshot) -> Result<Snapshot, String> {
        self.binding_revision = self
            .binding_revision
            .checked_add(1)
            .ok_or("Docker 连接版本已耗尽")?;
        let mut target = (*target).clone();
        target.revision = self.binding_revision;
        Ok(Arc::new(target))
    }
    pub fn snapshot(&self, id: &str) -> Result<Value, String> {
        let v = self.views.get(id).ok_or("视图已关闭")?;
        let mut data = json!({"terminal":v.terminal.receipt,"tunnel":v.tunnel.receipt,"kind":v.kind,"context":v.context,"containerId":v.container,"endpoint":v.endpoint,"revision":v.revision.to_string(),
            "busy":v.busy,"paused":v.paused,"nextIntent":(v.intent+1).to_string(),"navigation":v.navigation,
            "state":v.data["state"],"error":v.data["error"],"engine":v.data["engine"],"name":v.data["name"],"status":v.data["status"],
            "health":v.data["health"],"observedAt":v.data["observedAt"],"partial":v.data["partial"],"operation":v.data["operation"],
            "tty":v.data["tty"],"stats":v.data["stats"],"count":v.data["rows"].as_array().map_or(0,Vec::len)});
        data["connection"] = json!({"config":self.configs.get(v.context["serverId"].as_str().unwrap()).cloned().unwrap_or_default(),"target":v.target.as_ref().map(|t|t.info()),"test":v.connection_test});
        data["defaultShell"] = v
            .target
            .as_ref()
            .and_then(|t| t.os.shell())
            .map_or(Value::Null, |s| json!(s));
        data["scopeEpoch"] = json!(v.epoch.to_string());
        data["pull"] = v.pull.clone();
        if let Some(b) = &v.browser {
            data["indexes"] = b.metadata();
            data["tree"] = b.last_page.clone();
            data["count"] = json!(b.indexes[&Kind::Container].rows.len());
        }
        if browser::bytes(&data) > 24 * 1024 {
            data["tree"] = json!({"nodes":[],"total":v.browser.as_ref().map_or(0,|b|b.last_page["total"].as_u64().unwrap_or(0)),"reloadPage":true});
        }
        Ok(data)
    }
    pub fn response(&self, id: &str, data: Value) -> Result<Value, String> {
        if serde_json::to_vec(&data).unwrap().len() > 24 * 1024 {
            return Err("展示数据超过单次大小限制".into());
        }
        let v = self.views.get(id).ok_or("视图已关闭")?;
        let name = v.data["name"].as_str().unwrap_or("容器");
        Ok(
            json!({"metadata":{"title":match v.kind.as_str() {"browser"=>"Docker".into(),"logs"=>format!("{} · 日志",commands::text(name,100)),_=>commands::text(name,100)},"dirty":false,"busy":v.busy},"data":data}),
        )
    }
    pub fn enqueue(&mut self, id: &str, action: &str) -> Result<(), String> {
        self.enqueue_target(id, action, None)
    }
    fn enqueue_target(
        &mut self,
        id: &str,
        action: &str,
        target: Option<&Value>,
    ) -> Result<(), String> {
        if self.jobs.len() >= 16 {
            return Err("Docker 请求队列已满，请稍后重试".into());
        }
        let v = self.views.get_mut(id).ok_or("视图已关闭")?;
        if v.frozen {
            return Err("页面正在确认关闭".into());
        }
        if v.busy {
            return Err("当前页面仍有请求运行，请等待或取消".into());
        }
        let job_action = JobAction::parse(action, v)?;
        v.busy = true;
        v.cancelled = false;
        v.revision += 1;
        v.due = Instant::now()
            + Duration::from_secs(if v.terminal.receipt["status"] == "pending" {
                1
            } else {
                self.refresh_seconds
            });
        self.jobs.push_back(Job {
            epoch: v.epoch,
            resource: None,
            view: id.into(),
            context: v.context.clone(),
            endpoint: v.endpoint.clone(),
            container: target
                .and_then(|t| t["id"].as_str())
                .unwrap_or(&v.container)
                .into(),
            engine: if let Some(target) = target {
                target["engineId"].as_str().unwrap().into()
            } else {
                v.expected_engine.clone()
            },
            action: job_action,
            target: v.target.clone(),
            config: self
                .configs
                .get(v.context["serverId"].as_str().unwrap())
                .cloned()
                .unwrap_or_default(),
            deadline: None,
        });
        Ok(())
    }
    fn target_input(&self, id: &str, d: &Value) -> Result<Value, String> {
        let v = self.views.get(id).ok_or("视图已关闭")?;
        if v.kind != "browser" {
            return Err("当前页面不能操作容器".into());
        }
        let container = d["id"].as_str().ok_or("缺少完整容器 ID")?;
        commands::id(container)?;
        Ok(
            json!({"id":container,"engineId":d["engineId"],"endpoint":d["endpoint"],"expectedContext":d["expectedContext"],"scopeEpoch":d["scopeEpoch"]}),
        )
    }

    fn target(&self, id: &str, input: &Value) -> Result<Value, String> {
        let v = self.views.get(id).ok_or("视图已关闭")?;
        if v.data["state"] != "ready"
            || v.expected_engine.is_empty()
            || input["engineId"] != v.expected_engine
            || input["endpoint"] != v.endpoint
            || input["expectedContext"] != v.context
            || input["scopeEpoch"] != v.epoch.to_string()
        {
            return Err("容器连接身份已改变或数据待刷新，请重新选择".into());
        }
        let row = v
            .container_rows()
            .iter()
            .find(|row| row["id"] == input["id"])
            .ok_or("容器已不在列表中，请刷新后重新选择")?;
        Ok(json!({"name":row["name"],"status":row["status"]}))
    }
    pub fn check_scope(&self, id: &str, d: &Value) -> Result<(), String> {
        let v = self.views.get(id).ok_or("视图已关闭")?;
        if v.frozen
            || v.data["state"] != "ready"
            || v.expected_engine.is_empty()
            || d["expectedContext"] != v.context
            || d["endpoint"] != v.endpoint
            || d["engineId"] != v.expected_engine
            || d["scopeEpoch"] != v.epoch.to_string()
        {
            return Err("资源连接身份已改变或待刷新，请重新选择".into());
        }
        Ok(())
    }
    fn check_connection_change(&self, id: &str) -> Result<(), String> {
        let view = self.views.get(id).ok_or("视图已关闭")?;
        if self.jobs.len() >= 16
            || self.views.iter().any(|(other_id, other)| {
                other.context["serverId"] == view.context["serverId"]
                    && (other.frozen
                        || (other_id != id && other.busy)
                        || other.kind != "browser"
                        || other.terminal.unresolved()
                        || other.tunnel.unresolved()
                        || matches!(other.pull["state"].as_str(), Some("opening" | "running"))
                        || other
                            .browser
                            .as_ref()
                            .is_some_and(|b| b.indexes.values().any(|i| i.phase == "loading")))
            })
        {
            return Err(
                "请先关闭该服务器的资源详情和日志页，并等待请求、终端和镜像拉取结束".into(),
            );
        }
        Ok(())
    }
    pub fn record_intent(&mut self, id: &str, d: &Value, input: Value) -> Result<bool, String> {
        let request = d["requestId"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 128)
            .ok_or("请求 ID 无效")?;
        let seq = d["intentSequence"]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
            .filter(|n| *n > 0 && *n < u64::MAX)
            .ok_or("操作序号无效")?;
        let v = self.views.get_mut(id).ok_or("视图已关闭")?;
        if let Some((_, old_seq, old_input)) = v.receipts.iter().find(|(r, _, _)| r == request) {
            if *old_seq != seq || *old_input != input {
                return Err("相同请求参数不同".into());
            }
            return Ok(false);
        }
        if seq <= v.intent {
            return Err("旧操作回执已过期，不能重发".into());
        }
        v.intent = seq;
        v.receipts.push_back((request.into(), seq, input));
        if v.receipts.len() > 64 {
            v.receipts.pop_front();
        }
        Ok(true)
    }
    fn enqueue_browser(&mut self, id: &str, work: Work) -> Result<(), String> {
        if self.jobs.len() >= 16 {
            return Err("Docker 请求队列已满".into());
        }
        let v = self.views.get_mut(id).ok_or("视图已关闭")?;
        if v.frozen {
            return Err("页面正在关闭".into());
        }
        v.cancelled = false;
        v.revision += 1;
        self.jobs.push_back(Job {
            epoch: v.epoch,
            resource: None,
            view: id.into(),
            context: v.context.clone(),
            endpoint: v.endpoint.clone(),
            container: String::new(),
            engine: v.expected_engine.clone(),
            action: JobAction::Browser(work),
            target: v.target.clone(),
            config: self
                .configs
                .get(v.context["serverId"].as_str().unwrap())
                .cloned()
                .unwrap_or_default(),
            deadline: None,
        });
        Ok(())
    }
    fn repage(&mut self, id: &str) {
        let base = self.snapshot(id).unwrap_or(Value::Null);
        let mut base = base;
        base.as_object_mut().map(|m| m.remove("tree"));
        let budget = (24 * 1024usize)
            .saturating_sub(browser::bytes(&base) + 1024)
            .min(16 * 1024);
        if let Some(v) = self.views.get_mut(id) {
            if let Some(b) = &mut v.browser {
                b.page(&v.data["engine"], &v.navigation, budget);
            }
        }
    }
    pub fn refresh_loaded(&mut self, id: &str) {
        let kinds = self
            .views
            .get(id)
            .and_then(|v| v.browser.as_ref())
            .map(|b| {
                Kind::ALL
                    .into_iter()
                    .filter(|k| *k == Kind::Image || b.indexes[k].phase != "unloaded")
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if let Some(b) = self.views.get_mut(id).and_then(|v| v.browser.as_mut()) {
            b.invalidate();
        }
        for kind in kinds {
            if self.jobs.len() >= 16 {
                break;
            }
            let work = self
                .views
                .get_mut(id)
                .and_then(|v| v.browser.as_mut())
                .and_then(|b| b.load(kind));
            if let Some(work) = work {
                let _ = self.enqueue_browser(id, work);
            }
        }
        self.repage(id);
    }
    fn browser_event(&mut self, id: &str, d: &Value) -> Result<Value, String> {
        let action = d["action"].as_str().unwrap();
        let v = self.views.get(id).ok_or("视图已关闭")?;
        if v.browser.is_none() {
            return Err("不是 Docker 资源树页面".into());
        }
        if d.get("scopeEpoch")
            .is_some_and(|e| *e != v.epoch.to_string())
        {
            return Err("树连接代次已改变".into());
        }
        if action == "browser.tasks" {
            return Ok(page(
                &json!(v.tasks),
                d["offset"].as_u64().unwrap_or(0) as usize,
                10,
            ));
        }
        if action == "browser.locate" {
            let resource = ResourceRef::read(&d["resource"])?;
            return Ok(v
                .browser
                .as_ref()
                .unwrap()
                .locate(&v.data["engine"], &resource));
        }
        if action == "browser.object" {
            let resource = ResourceRef::read(&d["resource"])?;
            let b = v.browser.as_ref().unwrap();
            let row = b.row(&resource).ok_or("资源不在当前索引中")?;
            let aliases = row["tags"].as_array().map(Vec::as_slice).unwrap_or(&[]);
            let mut shown = vec![];
            let mut size = 0;
            for alias in aliases {
                let alias = commands::text(alias.as_str().unwrap_or(""), 512);
                if size + alias.len() > 8192 {
                    break;
                }
                size += alias.len();
                shown.push(alias);
            }
            let relation = b.objects.get(&resource.key());
            return Ok(
                json!({"resource":resource,"revision":b.indexes[&resource.kind].revision.to_string(),"complete":b.indexes[&resource.kind].complete,"aliases":shown,"aliasesTotal":aliases.len(),"aliasesTruncated":shown.len()<aliases.len(),"relation":relation.map(|r|json!({"phase":r.phase,"complete":r.complete,"count":r.items.len()})),"scopeEpoch":v.epoch.to_string(),"observationRevision":relation.map(|r|r.generation.to_string()),"info":relation.and_then(|r|r.info.as_ref()).map(ObjectInfo::summary),"stats":relation.map(|r|&r.stats),"error":relation.and_then(|r|r.error.as_ref())}),
            );
        }
        if action == "browser.collection" {
            let resource = ResourceRef::read(&d["resource"])?;
            let object = v
                .browser
                .as_ref()
                .unwrap()
                .objects
                .get(&resource.key())
                .ok_or("对象尚未读取")?;
            if d["observationRevision"] != object.generation.to_string() {
                return Err("对象观察已更新，请重新读取".into());
            }
            let name = d["collection"].as_str().ok_or("缺少集合名称")?;
            let rows = if name == "references" {
                object.items.clone()
            } else {
                object
                    .info
                    .as_ref()
                    .ok_or("对象尚未读取")?
                    .collection(name)?
            };
            let mut result = page(&json!(rows), d["offset"].as_u64().unwrap_or(0) as usize, 20);
            result["resource"] = json!(resource);
            result["scopeEpoch"] = json!(v.epoch.to_string());
            result["observationRevision"] = json!(object.generation.to_string());
            return Ok(result);
        }
        if action == "browser.stats" {
            self.check_scope(id, d)?;
            let resource = ResourceRef::read(&d["resource"])?;
            if resource.kind != Kind::Container {
                return Err("只有容器支持指标".into());
            }
            let mut input = d.clone();
            input["id"] = json!(resource.id);
            self.target(id, &self.target_input(id, &input)?)?;
            self.enqueue_target(id, "stats", Some(&input))?;
            return self.snapshot(id);
        }
        if action == "browser.page" {
            let state = browser::navigation(d, &v.navigation)?;
            let mut base = self.snapshot(id)?;
            base.as_object_mut().unwrap().remove("tree");
            let budget = (24 * 1024usize)
                .saturating_sub(browser::bytes(&base) + 1024)
                .min(16 * 1024);
            let v = self.views.get_mut(id).unwrap();
            return Ok(v
                .browser
                .as_mut()
                .unwrap()
                .page(&v.data["engine"], &state, budget));
        }
        if v.frozen {
            return Err("页面正在关闭".into());
        }
        if action == "browser.navigation" || action == "navigation" {
            let nav = browser::navigation(&d["state"], &v.navigation)?;
            self.views.get_mut(id).unwrap().navigation = nav;
            self.repage(id);
            return self.snapshot(id);
        }
        if action == "browser.visibility" {
            self.views
                .get_mut(id)
                .unwrap()
                .browser
                .as_mut()
                .unwrap()
                .visible = d["visible"].as_bool().ok_or("缺少可见状态")?;
            return self.snapshot(id);
        }
        if action != "browser.load" {
            return Err("未知资源树动作".into());
        }
        if self.jobs.len() >= 16 {
            return Err("Docker 请求队列已满".into());
        }
        if v.data["state"] != "ready" || v.expected_engine.is_empty() {
            return Err("请先刷新 Engine 连接".into());
        }
        let b = self.views.get_mut(id).unwrap().browser.as_mut().unwrap();
        let work = if d["resource"].is_null() {
            b.load(Kind::parse(d["branchKind"].as_str().ok_or("缺少分类")?)?)
        } else {
            b.load_object(ResourceRef::read(&d["resource"])?)?
        };
        if let Some(work) = work {
            self.enqueue_browser(id, work)?;
        }
        self.repage(id);
        self.snapshot(id)
    }
    fn resource_operation(&mut self, id: &str, d: &Value) -> Result<Value, String> {
        let resource = ResourceRef::read(&d["resource"])?;
        if d["operation"] != "remove" || resource.kind == Kind::Container {
            return Err("未知资源维护动作".into());
        }
        let v = self.views.get(id).ok_or("视图已关闭")?;
        if let Some((_, _, input)) = v.receipts.iter().find(|(r, _, _)| d["requestId"] == *r) {
            let old = input.clone();
            if old != *d {
                return Err("相同请求参数不同".into());
            }
            return self.snapshot(id);
        }
        self.check_scope(id, d)?;
        let v = self.views.get(id).unwrap();
        let b = v.browser.as_ref().ok_or("请从资源树执行维护")?;
        let index = &b.indexes[&resource.kind];
        if !index.complete || d["observationRevision"] != index.revision.to_string() {
            return Err("资源索引不完整或观察版本已改变，请重新读取和确认".into());
        }
        let row = b.row(&resource).ok_or("资源不在当前索引中")?;
        let observation = row["observation"].clone();
        let name = row["name"].clone();
        let proof = b
            .objects
            .get(&resource.key())
            .filter(|r| r.complete && r.phase == "ready")
            .ok_or("请先展开资源并完整读取引用")?;
        if !proof.items.is_empty() {
            return Err("仍有容器引用，不能删除".into());
        }
        if self.jobs.len() >= 16 || v.busy {
            return Err("请等待当前维护请求完成".into());
        }
        self.record_intent(id, d, d.clone())?;
        self.enqueue(id, "resource.remove")?;
        let job = self.jobs.back_mut().unwrap();
        job.resource = Some(resource.clone());
        if let JobAction::ResourceRemove {
            observation: fixed, ..
        } = &mut job.action
        {
            *fixed = observation;
        }
        let v = self.views.get_mut(id).unwrap();
        v.browser.as_mut().unwrap().invalidate();
        v.data["operation"] = json!({"action":"resource.remove","resource":resource,"name":name,"state":"checking","message":"重新检查身份和完整引用","submittedAt":now()});
        self.snapshot(id)
    }
    fn tunnel_event(&mut self, id: &str, d: &Value) -> Result<Value, String> {
        if d["action"] == "tunnel.status" {
            if self
                .views
                .get(id)
                .ok_or("视图已关闭")?
                .tunnel
                .ticket
                .is_some()
            {
                self.enqueue(id, "tunnelStatus")?;
            }
            return self.snapshot(id);
        }
        let request = d["requestId"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 128)
            .ok_or("请求 ID 无效")?;
        let v = self.views.get(id).ok_or("视图已关闭")?;
        if v.kind != "browser" || v.frozen {
            return Err("当前页面不能创建端口隧道".into());
        }
        if v.tunnel.input["requestId"] == request {
            if v.tunnel.input != *d {
                return Err("相同请求参数不同".into());
            }
            return self.snapshot(id);
        }
        if v.tunnel.unresolved() {
            return Err("已有隧道请求待确认，请查询原请求".into());
        }
        if d["action"] == "tunnel.create" {
            let input = self.target_input(id, d)?;
            let target = self.target(id, &input)?;
            if target["status"] != "running" {
                return Err("请先刷新并选择运行中的容器".into());
            }
            let binding: ports::Binding =
                serde_json::from_value(d["binding"].clone()).map_err(|_| "端口绑定身份无效")?;
            binding.target()?;
            if !v
                .browser
                .as_ref()
                .and_then(|b| {
                    b.objects
                        .get(&format!("container:{}", input["id"].as_str().unwrap()))
                })
                .filter(|o| o.phase == "ready")
                .and_then(|o| o.info.as_ref())
                .and_then(|info| info.collection("ports").ok())
                .is_some_and(|rows| rows.iter().any(|row| row["binding"] == d["binding"]))
            {
                return Err("端口绑定已变化，请刷新".into());
            }
        } else if d["action"] != "tunnel.reveal" || !d["revealRule"].is_string() {
            return Err("未知端口隧道操作".into());
        }
        self.check_scope(id, d)?;
        let v = self.views.get_mut(id).unwrap();
        v.tunnel.input = d.clone();
        v.tunnel.receipt = json!({"status":"pending","containerId":d["id"],"binding":d["binding"],"message":"核对端口并请求网络隧道"});
        if let Err(error) = self.enqueue_target(id, "tunnel", Some(d)) {
            self.views.get_mut(id).unwrap().tunnel.clear();
            return Err(error);
        }
        Ok(self.snapshot(id)?)
    }
    pub fn event(&mut self, id: &str, d: &Value) -> Result<Value, String> {
        let action = d["action"].as_str().ok_or("缺少操作")?;
        if action == "snapshot" {
            return self.snapshot(id);
        }
        if action.starts_with("tunnel.") {
            return self.tunnel_event(id, d);
        }
        if action.starts_with("browser.") {
            return self.browser_event(id, d);
        }
        if action == "resource.operate" {
            return self.resource_operation(id, d);
        }
        if action == "openTerminal" {
            let request = d["requestId"]
                .as_str()
                .filter(|s| !s.is_empty() && s.len() <= 128)
                .ok_or("请求 ID 无效")?;
            let shell = d["shell"]
                .as_str()
                .or_else(|| {
                    self.views
                        .get(id)
                        .and_then(|v| v.target.as_ref())
                        .and_then(|t| t.os.shell())
                })
                .ok_or("未知 Engine 系统，请指定容器程序")?;
            let mut input = self.target_input(id, d)?;
            input["requestId"] = json!(request);
            input["shell"] = json!(shell);
            let v = self.views.get(id).unwrap();
            if v.terminal.input["requestId"] == request {
                if v.terminal.input != input {
                    return Err("相同终端请求参数不同".into());
                }
                return self.snapshot(id);
            }
            if v.terminal.receipt["status"] == "pending" {
                return Err("已有终端请求待确认，请等待原请求结果".into());
            }
            let target = self.target(id, &input)?;
            if target["status"] != "running" {
                return Err("请先刷新并选择运行中的容器".into());
            }
            cli::terminal_args(
                v.target.as_ref().ok_or("Docker 连接尚未识别")?,
                input["id"].as_str().unwrap(),
                shell,
            )?;
            let v = self.views.get_mut(id).unwrap();
            v.terminal.input = input.clone();
            v.terminal.receipt = json!({"status":"pending","containerId":input["id"],"name":target["name"],"message":"检查容器并请求终端页面"});
            if let Err(error) = self.enqueue_target(id, "terminal", Some(&input)) {
                self.views.get_mut(id).unwrap().clear_terminal();
                return Err(error);
            }
            self.views.get_mut(id).unwrap().terminal.input = input;
        } else if action == "navigation" {
            if serde_json::to_vec(&d["state"]).unwrap().len() > 4096 {
                return Err("页面状态过大".into());
            }
            if self.views.get(id).is_some_and(|v| v.kind == "browser") {
                let mut input = d.clone();
                input["action"] = json!("browser.navigation");
                return self.browser_event(id, &input);
            }
            self.views.get_mut(id).ok_or("视图已关闭")?.navigation = d["state"].clone();
        } else if action == "refresh" || action == "stats" {
            self.enqueue(id, action)?;
        } else if action == "cancel" {
            let v = self.views.get_mut(id).ok_or("视图已关闭")?;
            v.cancelled = true;
            if let Some(b) = &mut v.browser {
                b.invalidate();
            }
            v.revision += 1;
        } else if action == "pauseRefresh" {
            let v = self.views.get_mut(id).ok_or("视图已关闭")?;
            v.paused = d["paused"].as_bool().ok_or("缺少暂停状态")?;
            v.revision += 1;
        } else if action == "resumeInput" {
            self.views.get_mut(id).ok_or("视图已关闭")?.frozen = false;
        } else if matches!(
            action,
            "setEndpoint" | "connection.test" | "connection.save" | "connection.identify"
        ) {
            let v = self.views.get(id).ok_or("视图已关闭")?;
            if v.kind != "browser" {
                return Err("请从 Docker 资源树调整连接".into());
            }
            let config = if action == "setEndpoint" {
                Config {
                    mode: connection::Mode::Manual,
                    endpoint: Some(d["endpoint"].as_str().ok_or("缺少端点")?.into()),
                    cli_path: None,
                }
            } else if action == "connection.identify" {
                self.configs
                    .get(v.context["serverId"].as_str().unwrap())
                    .cloned()
                    .unwrap_or_default()
            } else {
                serde_json::from_value(d["config"].clone()).map_err(|_| "连接配置格式无效")?
            };
            config.validate()?;
            if action != "connection.test" {
                self.check_connection_change(id)?;
            }
            self.enqueue(id, action)?;
            let job = self.jobs.back_mut().unwrap();
            job.config = config;
            self.views.get_mut(id).unwrap().connection_test =
                json!({"state":"checking","config":job.config});
        } else if action == "operate" {
            let request = d["requestId"]
                .as_str()
                .filter(|s| !s.is_empty() && s.len() <= 128)
                .ok_or("请求 ID 无效")?;
            let seq = d["intentSequence"]
                .as_str()
                .and_then(|s| s.parse::<u64>().ok())
                .filter(|n| *n > 0 && *n < u64::MAX)
                .ok_or("操作序号无效")?;
            let op = d["operation"].as_str().ok_or("缺少容器操作")?;
            if !["start", "stop", "restart", "pause", "unpause", "remove"].contains(&op) {
                return Err("未知容器操作".into());
            }
            let mut input = self.target_input(id, d)?;
            input["operation"] = json!(op);
            let v = self.views.get(id).unwrap();
            if let Some((_, old_seq, old_input)) = v.receipts.iter().find(|(r, _, _)| r == request)
            {
                if *old_seq != seq || old_input != &input {
                    return Err("相同请求参数不同".into());
                }
                return self.snapshot(id);
            }
            if seq <= v.intent {
                return Err("旧操作回执已过期，不能重发".into());
            }
            let target = self.target(id, &input)?;
            self.enqueue_target(id, op, Some(&input))?;
            let v = self.views.get_mut(id).unwrap();
            if let Some(b) = &mut v.browser {
                b.invalidate();
            }
            v.intent = seq;
            v.receipts.push_back((request.into(), seq, input.clone()));
            if v.receipts.len() > 64 {
                v.receipts.pop_front();
            }
            v.data["operation"] = json!({"action":op,"containerId":input["id"],"name":target["name"],"state":"checking","submittedAt":now(),"message":"检查当前对象"});
        } else {
            return Err("未知 Docker 操作".into());
        }
        self.snapshot(id)
    }
}
pub fn page(items: &Value, offset: usize, limit: usize) -> Value {
    let values = items.as_array().map(Vec::as_slice).unwrap_or(&[]);
    let mut rows = vec![];
    let mut bytes = 0;
    for v in values.iter().skip(offset).take(limit.max(1)) {
        let size = serde_json::to_vec(v).unwrap().len();
        if bytes + size > 16 * 1024 {
            break;
        }
        rows.push(v.clone());
        bytes += size;
    }
    json!({"rows":rows,"offset":offset,"nextOffset":if offset+rows.len()<values.len(){Some(offset+rows.len())}else{None},"total":values.len()})
}
fn alive(app: &Shared, job: &Job, client: &Client) -> bool {
    !client.is_closed()
        && app.lock().unwrap().views.get(&job.view).is_some_and(|v| {
            v.context == job.context
                && v.epoch == job.epoch
                && v.endpoint == job.endpoint
                && job
                    .target
                    .as_ref()
                    .is_none_or(|t| v.target.as_ref().is_some_and(|current| current.same(t)))
                && !v.cancelled
        })
}
fn execute(
    app: &Shared,
    job: &Job,
    client: &Client,
    args: &[&str],
    budget: usize,
) -> Result<Exec, String> {
    let target = job.target.as_ref().ok_or("Docker 连接尚未识别")?;
    let container =
        if !job.container.is_empty() && !matches!(args.first(), Some(&"info") | Some(&"version")) {
            Some(job.container.as_str())
        } else {
            None
        };
    let prepared =
        cli::prepare(client, &job.view, target, args, container, false).map_err(|e| e.message)?;
    execute_prepared(app, job, client, prepared, budget)
}
fn execute_prepared(
    app: &Shared,
    job: &Job,
    client: &Client,
    prepared: sodeck_plugin_client::program::PreparedProgram,
    budget: usize,
) -> Result<Exec, String> {
    let dispatch = app.lock().unwrap().exec_dispatch.clone();
    let dispatch_guard = dispatch.lock().unwrap();
    let mut params = {
        let mut a = app.lock().unwrap();
        if a.views.values().map(View::cache_bytes).sum::<usize>() + budget * 2 > 16 * 1024 * 1024 {
            return Err("Docker 缓存与临时输出超过 16 MiB，请关闭其它页面".into());
        }
        let v = a.views.get_mut(&job.view).ok_or("页面已关闭")?;
        if v.context != job.context
            || v.epoch != job.epoch
            || v.endpoint != job.endpoint
            || job
                .target
                .as_ref()
                .is_some_and(|t| !v.target.as_ref().is_some_and(|current| current.same(t)))
            || v.frozen
            || v.cancelled
        {
            return Err("请求已取消或上下文已改变".into());
        }
        v.sequence += 1;
        json!({"viewId":job.view,"expectedContext":job.context,"operationId":format!("docker-{}-{}",job.view,v.sequence),"sequence":v.sequence.to_string(),
            "timeoutMs":remaining(job)?,"maxOutputBytes":budget})
    };
    prepared.apply(&mut params)?;
    let mut task = None;
    for _ in 0..3 {
        match serde_json::from_value::<api::ExecStart>(params.clone())
            .map_err(|_| sodeck_plugin_client::RpcError::invalid("核心请求参数无效"))
            .and_then(|params| client.request(api::call::ExecStart(params)))
        {
            Ok(result) => {
                task = Some(result.task_id);
                break;
            }
            Err(error) if error.code == -32003 => continue,
            Err(error) => return Err(error.message),
        }
    }
    drop(dispatch_guard);
    let task = task.ok_or("命令启动回执未确认；结果可能未知，请刷新，禁止自动重发")?;
    wait_exec(app, job, client, &task, budget)
}
fn wait_exec(
    app: &Shared,
    job: &Job,
    client: &Client,
    task: &str,
    budget: usize,
) -> Result<Exec, String> {
    let result = (|| {
        let mut stdout = vec![];
        let mut stderr = vec![];
        let mut a = "0".to_string();
        let mut b = "0".to_string();
        let end = job
            .deadline
            .unwrap_or_else(|| Instant::now() + Duration::from_secs(35));
        loop {
            if !alive(app, job, client) || Instant::now() > end {
                return Err("本地等待已取消或超时；已发送操作的结果未确认".into());
            }
            let r = match client.request(api::call::ExecPoll(api::ExecPoll {
                task_id: task.to_string(),
                stdout_offset: a.to_string(),
                stderr_offset: b.to_string(),
            })) {
                Ok(r) => r,
                Err(e) if e.code == -32003 => continue,
                Err(e) => return Err(e.message),
            };
            let out = sodeck_plugin_client::encoding::unbase64(&r.stdout.data_base64)
                .map_err(|_| "核心输出编码无效")?;
            let err = sodeck_plugin_client::encoding::unbase64(&r.stderr.data_base64)
                .map_err(|_| "核心输出编码无效")?;
            if stdout.len() + stderr.len() + out.len() + err.len() > budget {
                return Err("命令输出超过本地预算".into());
            }
            stdout.extend(out);
            stderr.extend(err);
            a = r.stdout.next_offset.clone();
            b = r.stderr.next_offset.clone();
            if r.output_complete {
                return Ok(Exec {
                    bytes: stdout,
                    stderr,
                    response: json!(r),
                });
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    })();
    let _ = client
        .request(api::call::ExecRelease(api::Task {
            task_id: task.to_string(),
        }))
        .map_err(|e| e.message);
    result
}
fn remaining(job: &Job) -> Result<u64, String> {
    let ms = job.deadline.map_or(30000, |end| {
        end.saturating_duration_since(Instant::now())
            .as_millis()
            .min(30000) as u64
    });
    if ms < 1000 {
        Err("Docker 连接识别超过 30 秒，请检查远端程序与 Engine".into())
    } else {
        Ok(ms)
    }
}
fn discover_target(app: &Shared, job: &Job, client: &Client) -> Result<Snapshot, String> {
    let state = serde_json::from_value::<api::ViewContext>(
        json!({"viewId":job.view,"expectedContext":job.context}),
    )
    .map_err(|_| sodeck_plugin_client::RpcError::invalid("核心请求参数无效"))
    .and_then(|params| client.request(api::call::Target(params)))
    .map_err(|e| e.message)?;
    if state.state != sodeck_plugin_client::protocol::replies::ConnectionState::Connected
        || json!(state.context) != job.context
        || state.closing
    {
        return Err("服务器已断开或连接代次改变，请重新连接并刷新".into());
    }
    let mut probe = job.clone();
    probe.deadline = Some(Instant::now() + Duration::from_secs(30));
    let dispatch = app.lock().unwrap().exec_dispatch.clone();
    let guard = dispatch.lock().unwrap();
    let (operation, sequence) = {
        let mut a = app.lock().unwrap();
        let v = a.views.get_mut(&job.view).ok_or("页面已关闭")?;
        if v.context != job.context || v.epoch != job.epoch || v.cancelled || v.frozen {
            return Err("连接请求已失效".into());
        }
        v.sequence += 1;
        (
            format!("environment-{}-{}", job.view, v.sequence),
            v.sequence,
        )
    };
    let reply = sodeck_plugin_client::program::environment_start(
        client,
        &job.view,
        &job.context,
        &operation,
        sequence,
        remaining(&probe)?,
        &["DOCKER_HOST", "DOCKER_CONTEXT", "DOCKER_CONFIG"],
    )
    .map_err(|e| e.message)?;
    drop(guard);
    let environment_result = wait_exec(app, &probe, client, &reply.task_id, 16 * 1024)?;
    if environment_result.response["state"] != "completed"
        || environment_result.response["remoteExitConfirmed"] != true
    {
        return Err(format!(
            "环境识别失败：{}",
            environment_result.response["error"]["message"]
                .as_str()
                .unwrap_or("无法确认远端执行环境")
        ));
    }
    let env = commands::object(&environment_result.bytes)?;
    let environment = &env["environment"];
    let cli = job.config.cli_path.clone().unwrap_or_else(|| {
        if environment["kind"] == "windows" {
            "docker.exe".into()
        } else {
            "docker".into()
        }
    });
    if let Some(path) = &job.config.cli_path {
        let windows = environment["kind"] == "windows";
        if windows == path.starts_with('/') {
            return Err("Docker CLI 路径与远端执行环境不兼容".into());
        }
    }
    let (endpoint, source, version, info) = connection::discover(
        &job.config,
        environment,
        &env["values"],
        |endpoint, args| {
            let prepared = cli::raw(
                client,
                &job.view,
                &job.context,
                environment,
                &cli,
                endpoint,
                args,
                vec![],
                false,
            )
            .map_err(|e| e.message)?;
            checked(execute_prepared(app, &probe, client, prepared, 256 * 1024)?)
        },
    )?;
    let os = EngineOs::parse(version["Server"]["Os"].as_str().unwrap_or(""));
    Ok(Arc::new(Target {
        context: job.context.clone(),
        revision: 0,
        environment: environment.clone(),
        cli,
        endpoint,
        engine_id: info["ID"].as_str().unwrap().into(),
        os,
        source,
    }))
}
fn connection_request(app: &Shared, job: &Job, client: &Client) {
    let result = discover_target(app, job, client);
    let mut a = app.lock().unwrap();
    if !a
        .views
        .get(&job.view)
        .is_some_and(|v| v.context == job.context && v.epoch == job.epoch)
    {
        return;
    }
    if a.views[&job.view].cancelled {
        let v = a.views.get_mut(&job.view).unwrap();
        v.busy = false;
        v.connection_test =
            json!({"state":"failed","config":job.config,"message":"连接测试已取消，原配置保留"});
        v.revision += 1;
        return;
    }
    let test = job.action.as_str() == "connection.test";
    let result = result.and_then(|target| {
        if !test {
            a.check_connection_change(&job.view)?;
            let target = a.version_target(target)?;
            let server = job.context["serverId"].as_str().unwrap();
            let mut configs = a.configs.clone();
            configs.insert(server.into(), job.config.clone());
            if job.action.as_str() != "connection.identify" {
                connection::save(&a.directory, &configs)?;
                a.configs = configs;
            }
            a.bindings.retain(|_, t| t.context["serverId"] != server);
            a.bindings.insert(job.context.to_string(), target.clone());
            for v in a
                .views
                .values_mut()
                .filter(|v| v.context["serverId"] == server)
            {
                v.epoch += 1;
                v.target = if v.context == job.context {
                    Some(target.clone())
                } else {
                    None
                };
                v.endpoint = v
                    .target
                    .as_ref()
                    .map(|t| t.endpoint.uri())
                    .unwrap_or_default();
                v.expected_engine = v
                    .target
                    .as_ref()
                    .map(|t| t.engine_id.clone())
                    .unwrap_or_default();
                v.clear_terminal();
                v.browser = Some(BrowserState::default());
                v.navigation = browser::navigation(&json!({}), &Value::Null)?;
                v.data = json!({"state":"loading","rows":[]});
                v.busy = false;
                v.revision += 1;
            }
            a.enqueue(&job.view, "refresh")?;
            return Ok(target);
        }
        Ok(target)
    });
    let v = a.views.get_mut(&job.view).unwrap();
    if test || result.is_err() {
        v.busy = false;
    }
    v.connection_test = match result {
        Ok(target) => json!({"state":"succeeded","config":job.config,"target":target.info()}),
        Err(error) => {
            json!({"state":"failed","config":job.config,"message":commands::text(&error,2048)})
        }
    };
    v.revision += 1;
}
fn checked(result: Exec) -> Result<Vec<u8>, String> {
    if let Some(message) = cli::startup_error(&result.response) {
        return Err(message);
    }
    if result.response["state"] != "completed"
        || result.response["outputLimitExceeded"] == true
        || result.response["remoteExitConfirmed"] != true
        || result.response["exitCode"] != 0
    {
        return Err(format!(
            "命令未成功：{} {}",
            result.response["error"]["message"]
                .as_str()
                .unwrap_or("请检查 Engine 和当前用户的访问能力"),
            commands::text(&String::from_utf8_lossy(&result.stderr), 2048)
        ));
    }
    Ok(result.bytes)
}
pub fn log_target(
    app: &Shared,
    client: &Client,
    view: &str,
    expected: &Value,
) -> Result<Value, String> {
    let job = {
        let a = app.lock().unwrap();
        let v = a.views.get(view).ok_or("日志页面已关闭")?;
        if v.context != expected["context"] || v.frozen || v.cancelled {
            return Err("日志目标上下文已改变，请刷新后重新读取".into());
        }
        Job {
            epoch: v.epoch,
            resource: None,
            view: view.into(),
            context: v.context.clone(),
            endpoint: v.endpoint.clone(),
            container: v.container.clone(),
            engine: v.expected_engine.clone(),
            action: JobAction::Logs,
            target: v.target.clone(),
            config: a
                .configs
                .get(v.context["serverId"].as_str().unwrap())
                .cloned()
                .unwrap_or_default(),
            deadline: None,
        }
    };
    let info: Value = serde_json::from_slice(&checked(execute(
        app,
        &job,
        client,
        &["info", "--format", "{{json .}}"],
        256 * 1024,
    )?)?)
    .map_err(|_| "Engine身份输出无效")?;
    if info["ID"] != job.engine {
        return Err("Docker Engine已改变，请重新打开容器".into());
    }
    let container = inspect(app, &job, client)?;
    if container["Id"] != job.container {
        return Err("Docker容器身份已改变".into());
    }
    let mut target = expected.clone();
    target["engine"]["systemTime"] = info["SystemTime"].clone();
    target["status"] = container["State"]["Status"].clone();
    target["name"] = json!(container["Name"]
        .as_str()
        .unwrap_or("容器")
        .trim_start_matches('/'));
    target["tty"] = container["Config"]["Tty"].clone();
    target["startedAt"] = container["State"]["StartedAt"].clone();
    target["finishedAt"] = container["State"]["FinishedAt"].clone();
    Ok(target)
}
pub fn bound_target(app: &Shared, view: &str, expected: &Value) -> Result<Snapshot, String> {
    let a = app.lock().unwrap();
    let v = a.views.get(view).ok_or("页面已关闭")?;
    let target = v.target.as_ref().ok_or("请先识别 Docker 连接")?;
    if v.context != expected["context"]
        || v.endpoint != expected["endpoint"]
        || json!(target.engine_id) != expected["engine"]["id"]
        || v.data["state"] != "ready"
        || v.frozen
        || v.cancelled
    {
        return Err("Docker 连接身份已改变或状态待刷新".into());
    }
    Ok(target.clone())
}
pub fn open_stream(
    app: &Shared,
    client: &Client,
    view: &str,
    target: &Snapshot,
    prefix: &str,
    prepared: sodeck_plugin_client::program::PreparedProgram,
    budget: usize,
    fixed: bool,
) -> Result<(Value, Option<Value>), String> {
    let dispatch = app
        .lock()
        .unwrap()
        .views
        .get(view)
        .ok_or("页面已关闭")?
        .stream_dispatch
        .clone();
    let _guard = dispatch.lock().unwrap();
    let mut params = {
        let mut a = app.lock().unwrap();
        let v = a.views.get_mut(view).ok_or("页面已关闭")?;
        if !v
            .target
            .as_ref()
            .is_some_and(|current| current.same(target))
            || v.frozen
            || v.cancelled
        {
            return Err("流式请求已取消或目标改变".into());
        }
        v.sequence_for_stream();
        json!({"viewId":view,"expectedContext":target.context,"sequence":v.command_sequence().to_string(),"operationId":format!("{prefix}-{}-{}",v.epoch,v.command_sequence()),"maxOutputBytes":budget,"retention":if fixed{"fixed"}else{"rolling"}})
    };
    prepared.apply(&mut params)?;
    let request: sodeck_plugin_client::protocol::session::ProcessStart =
        serde_json::from_value(params.clone()).map_err(|_| "进程启动参数无效")?;
    match client.request(api::call::ProcessStart(request)) {
        Ok(response) => Ok((params, Some(json!(response)))),
        Err(error) if error.data["dispatch"] == "unknown" => Ok((params, None)),
        Err(error) => Err(error.message),
    }
}
fn inspect(app: &Shared, job: &Job, client: &Client) -> Result<Value, String> {
    let out = checked(execute(
        app,
        job,
        client,
        &[
            "container",
            "inspect",
            "--format",
            "{{json .}}",
            &job.container,
        ],
        1024 * 1024,
    )?)?;
    let v = commands::object(&out)?;
    if v["Id"] != job.container {
        return Err("容器身份已改变".into());
    }
    Ok(v)
}
fn collaboration_request(app: &Shared, job: &Job, client: &Client) -> Result<Value, String> {
    let work = job.action.command().ok_or("协作任务缺少载荷")?;
    let ticket = if let Some(ticket) = &work.intent.ticket {
        ticket.clone()
    } else {
        if !work.submit {
            return Err("协作票据已失效".into());
        }
        let ticket =
            if work.kind == CommandKind::Tunnel && work.intent.input["revealRule"].is_string() {
                sodeck_plugin_client::commands::Ticket::new(
                    job.context.clone(),
                    &job.view,
                    work.intent.input["requestId"]
                        .as_str()
                        .ok_or("缺少请求 ID")?,
                    "network.rule.reveal.v1",
                    json!({"version":1,"ruleId":work.intent.input["revealRule"]}),
                )
            } else {
                let info = commands::object(&checked(execute(
                    app,
                    job,
                    client,
                    &["info", "--format", "{{json .}}"],
                    256 * 1024,
                )?)?)?;
                if info["ID"] != job.engine {
                    return Err("Engine 已改变，请从资源树重新选择容器".into());
                }
                let container = inspect(app, job, client)?;
                collaboration::prepare(
                    client,
                    &job.view,
                    &job.context,
                    job.target.as_ref().ok_or("Docker 连接尚未识别")?,
                    &container,
                    work.kind,
                    &work.intent.input,
                )?
            };
        let mut a = app.lock().unwrap();
        let v = a.views.get_mut(&job.view).ok_or("页面已关闭")?;
        if v.context != job.context || v.epoch != job.epoch || v.cancelled || v.frozen {
            return Err("协作请求已取消或连接改变".into());
        }
        v.command_mut(work.kind).ticket = Some(ticket.clone());
        ticket
    };
    collaboration::dispatch(client, &ticket, work.submit)
}
fn collect(app: &Shared, job: &Job, client: &Client) -> Result<Value, String> {
    let session = serde_json::from_value::<api::ViewContext>(
        json!({"viewId":job.view,"expectedContext":job.context}),
    )
    .map_err(|_| sodeck_plugin_client::RpcError::invalid("核心请求参数无效"))
    .and_then(|params| client.request(api::call::Target(params)))
    .map_err(|e| e.message)?;
    if session.state != sodeck_plugin_client::protocol::replies::ConnectionState::Connected
        || json!(session.context) != job.context
    {
        return Err("服务器连接已断开或代次改变，请重新连接并刷新".into());
    }
    let version = commands::object(&checked(execute(
        app,
        job,
        client,
        &["version", "--format", "{{json .}}"],
        256 * 1024,
    )?)?)?;
    let info = commands::object(&checked(execute(
        app,
        job,
        client,
        &["info", "--format", "{{json .}}"],
        256 * 1024,
    )?)?)?;
    let engine_id = info["ID"]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 512 && !s.chars().any(char::is_control))
        .ok_or("Engine ID 不可用")?;
    if job
        .target
        .as_ref()
        .is_some_and(|t| t.os != EngineOs::parse(version["Server"]["Os"].as_str().unwrap_or("")))
    {
        return Err("Engine 系统已改变，请重新识别连接".into());
    }
    if !job.engine.is_empty() && job.engine != engine_id {
        return Err("Engine 已改变，原容器页面失效，请从列表重新打开".into());
    }
    let engine = json!({"id":engine_id,"name":commands::text(info["Name"].as_str().unwrap_or("Docker"),256),"version":commands::text(version["Server"]["Version"].as_str().unwrap_or(""),64),
        "os":version["Server"]["Os"],"clientOs":version["Client"]["Os"],"clientVersion":version["Client"]["Version"],"defaultShell":job.target.as_ref().and_then(|t|t.os.shell()),"arch":commands::text(version["Server"]["Arch"].as_str().unwrap_or(""),64),"containers":info["Containers"].as_u64(),"images":info["Images"].as_u64(),"systemTime":commands::text(info["SystemTime"].as_str().unwrap_or(""),128),"dockerRootDir":resources::root(&info["DockerRootDir"])});
    let kind = app
        .lock()
        .unwrap()
        .views
        .get(&job.view)
        .ok_or("页面已关闭")?
        .kind
        .clone();
    if kind == "browser" && job.container.is_empty() {
        return Ok(
            json!({"engine":engine,"rows":[],"partial":false,"observedAt":now(),"state":"ready","error":null}),
        );
    }
    let container = inspect(app, job, client)?;
    let name = commands::text(
        container["Name"]
            .as_str()
            .unwrap_or("容器")
            .trim_start_matches('/'),
        256,
    );
    let mut data = json!({"engine":engine,"name":name,"status":container["State"]["Status"],"health":container["State"]["Health"]["Status"],
        "observedAt":now(),"state":"ready","error":null,"partial":false});
    if kind == "logs" {
        data["tty"] = container["Config"]["Tty"].clone();
        data["startedAt"] = container["State"]["StartedAt"].clone();
        data["finishedAt"] = container["State"]["FinishedAt"].clone();
    } else if job.action.as_str() == "stats" {
        if container["State"]["Running"] != true {
            data["stats"] = json!({"unavailable":"容器未运行"});
        } else {
            let result = checked(execute(
                app,
                job,
                client,
                &[
                    "container",
                    "stats",
                    "--no-stream",
                    "--no-trunc",
                    "--format",
                    "{{json .}}",
                    &job.container,
                ],
                256 * 1024,
            )?)?;
            let sample = commands::object(&result)?;
            let mut stats = json!({"observedAt":now()});
            for field in ["CPUPerc", "MemUsage", "MemPerc", "NetIO", "BlockIO", "PIDs"] {
                stats[field] = json!(commands::text(
                    sample[field].as_str().unwrap_or("未知"),
                    128
                ));
            }
            data["stats"] = stats;
        }
    } else if job.action.as_str() != "refresh" {
        data["stats"] = json!({"unavailable":"容器操作后，请重新读取资源快照"});
        if job
            .target
            .as_ref()
            .is_some_and(|t| t.os == EngineOs::Windows)
            && matches!(job.action.as_str(), "pause" | "unpause")
        {
            return Err("Windows Engine 不支持暂停与恢复容器".into());
        }
        let args = commands::action(job.action.as_str(), &container)?;
        if let Some(args) = args {
            {
                let mut a = app.lock().unwrap();
                if let Some(v) = a.views.get_mut(&job.view) {
                    v.data["operation"]["state"] = json!("running");
                    v.data["operation"]["message"] = json!("等待 Docker 命令结果");
                    v.revision += 1;
                }
            }
            let r = execute(
                app,
                job,
                client,
                &args.iter().map(String::as_str).collect::<Vec<_>>(),
                256 * 1024,
            )?;
            let (state, message) = cli::outcome(&r.response, &r.stderr);
            let success = state == "succeeded";
            data["operation"] =
                json!({"action":job.action,"state":state,"message":message,"finishedAt":now()});
            if success {
                match inspect(app, job, client) {
                    Ok(after) => {
                        data["status"] = after["State"]["Status"].clone();
                        data["health"] = after["State"]["Health"]["Status"].clone();
                        data["operation"]["message"] = json!("命令成功；已刷新当前观测状态");
                    }
                    Err(_) if job.action.as_str() == "remove" => {
                        data["state"] = json!("gone");
                        data["operation"]["message"] = json!("删除命令成功；原容器页面保留");
                    }
                    Err(error) => {
                        data["operation"]["message"] =
                            json!(format!("命令成功；状态验证未完成：{error}"));
                    }
                }
            }
        } else {
            data["operation"] = json!({"action":job.action,"state":"skipped","message":"已经处于目标状态，无需重复操作"});
        }
    }
    Ok(data)
}
fn read(
    app: &Shared,
    job: &Job,
    client: &Client,
    args: &[&str],
    budget: usize,
) -> Result<Vec<u8>, String> {
    checked(execute(app, job, client, args, budget)?)
}
fn engine_check(app: &Shared, job: &Job, client: &Client) -> Result<(), String> {
    let info = commands::object(&read(
        app,
        job,
        client,
        &["info", "--format", "{{json .}}"],
        256 * 1024,
    )?)?;
    if info["ID"] != job.engine {
        return Err("Docker Engine 已改变，请刷新连接".into());
    }
    Ok(())
}
fn enumerated(
    app: &Shared,
    job: &Job,
    client: &Client,
    kind: Kind,
) -> Result<(Vec<String>, bool), String> {
    let out = read(app, job, client, &kind.list(), 256 * 1024)?;
    let ids = std::str::from_utf8(&out)
        .map_err(|_| "资源 ID 格式无效")?
        .lines()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect::<std::collections::BTreeSet<_>>();
    for id in &ids {
        kind.validate(id)?;
    }
    let partial = ids.len() > 1000;
    Ok((ids.iter().take(1000).cloned().collect(), partial))
}
fn batch(
    app: &Shared,
    job: &Job,
    client: &Client,
    kind: Kind,
    format: &str,
    ids: &[String],
    offset: usize,
) -> Result<(Vec<Value>, usize), String> {
    let mut end = (offset + 16).min(ids.len());
    loop {
        let mut args = vec![kind.name(), "inspect", "--format", format];
        args.extend(ids[offset..end].iter().map(String::as_str));
        let prepared = cli::prepare(
            client,
            &job.view,
            job.target.as_ref().ok_or("Docker 连接尚未识别")?,
            &args,
            None,
            false,
        );
        match prepared {
            Ok(prepared) => {
                let out = checked(execute_prepared(app, job, client, prepared, 1024 * 1024)?)?;
                let rows = commands::lines(&out)?;
                let seen = rows
                    .iter()
                    .filter_map(|r| r["id"].as_str())
                    .collect::<std::collections::BTreeSet<_>>();
                if seen.len() != end - offset
                    || ids[offset..end]
                        .iter()
                        .any(|id| !seen.contains(id.as_str()))
                {
                    return Err("批量摘要缺失或身份不符，读取不完整".into());
                }
                return Ok((rows, end));
            }
            Err(error) if error.data["code"] == "commandTooLong" => {
                if end <= offset + 1 {
                    return Err(error.message);
                }
            }
            Err(error) => return Err(error.message),
        }
        end -= 1;
    }
}
fn browser_read(app: &Shared, job: &Job, client: &Client) -> Result<Option<Work>, String> {
    let work = job.action.browser().ok_or("缺少树读取任务")?;
    let owned = || {
        app.lock().unwrap().views.get(&job.view).is_some_and(|v| {
            v.epoch == job.epoch
                && v.context == job.context
                && v.endpoint == job.endpoint
                && v.browser.as_ref().is_some_and(|b| b.owns(work))
        })
    };
    if !owned() {
        return Ok(None);
    }
    match work {
        Work::Index {
            kind,
            generation,
            ids,
            offset,
        } => {
            if ids.is_none() {
                engine_check(app, job, client)?;
                let (ids, partial) = enumerated(app, job, client, *kind)?;
                let mut a = app.lock().unwrap();
                let v = a.views.get_mut(&job.view).ok_or("页面已关闭")?;
                if v.epoch != job.epoch || v.context != job.context || v.endpoint != job.endpoint {
                    return Ok(None);
                }
                let b = v.browser.as_mut().unwrap();
                if !b.owns(work) {
                    return Ok(None);
                }
                b.indexes.get_mut(kind).unwrap().partial = partial;
                return Ok(Some(Work::Index {
                    kind: *kind,
                    generation: *generation,
                    ids: Some(ids),
                    offset: 0,
                }));
            }
            let ids = ids.as_ref().unwrap();
            let (rows, end) = if *offset < ids.len() {
                batch(app, job, client, *kind, kind.summary(), ids, *offset)?
            } else {
                (vec![], *offset)
            };
            let rows = rows
                .into_iter()
                .map(|r| resources::normalize(*kind, r))
                .collect::<Result<Vec<_>, _>>()?;
            let complete = end >= ids.len();
            if complete {
                engine_check(app, job, client)?;
            }
            let mut a = app.lock().unwrap();
            let used = a.views.values().map(View::cache_bytes).sum::<usize>();
            if used + browser::bytes(&json!(rows)) + 1024 * 1024 > 16 * 1024 * 1024 {
                return Err("Docker 总缓存达到 16 MiB 上限，请关闭其它页面".into());
            }
            let v = a.views.get_mut(&job.view).ok_or("页面已关闭")?;
            if v.epoch != job.epoch || v.context != job.context || v.endpoint != job.endpoint {
                return Ok(None);
            }
            let b = v.browser.as_mut().unwrap();
            if !b.owns(work) {
                return Ok(None);
            }
            let index = b.indexes.get_mut(kind).unwrap();
            let mut cap = false;
            for row in rows {
                if browser::bytes(&json!(index.staged)) + browser::bytes(&row)
                    > browser::INDEX_LIMIT
                {
                    cap = true;
                    index.partial = true;
                    break;
                }
                index.staged.push(row);
            }
            if complete || cap {
                index.rows = std::mem::take(&mut index.staged);
                index.rows.sort_by(|a, b| {
                    a["name"]
                        .as_str()
                        .cmp(&b["name"].as_str())
                        .then(a["id"].as_str().cmp(&b["id"].as_str()))
                });
                index.complete = !index.partial;
                index.phase = if index.complete { "ready" } else { "partial" }.into();
                index.revision += 1;
                v.revision += 1;
            } else {
                v.revision += 1;
                return Ok(Some(Work::Index {
                    kind: *kind,
                    generation: *generation,
                    ids: Some(ids.clone()),
                    offset: end,
                }));
            }
            Ok(None)
        }
        Work::Object {
            resource,
            generation,
            ids,
            offset,
        } => {
            if ids.is_none() {
                engine_check(app, job, client)?;
                let format = if resource.kind == Kind::Container {
                    crate::object::CONTAINER
                } else {
                    "{{json .}}"
                };
                let current = commands::object(&read(
                    app,
                    job,
                    client,
                    &[
                        resource.kind.name(),
                        "inspect",
                        "--format",
                        format,
                        &resource.id,
                    ],
                    256 * 1024,
                )?)?;
                if current[if resource.kind == Kind::Volume {
                    "Name"
                } else {
                    "Id"
                }] != resource.id
                {
                    return Err("关系对象身份已改变".into());
                }
                let direct = [Kind::Container, Kind::Network].contains(&resource.kind);
                let (scan, partial) = if direct {
                    (vec![], false)
                } else {
                    enumerated(app, job, client, Kind::Container)?
                };
                let mut a = app.lock().unwrap();
                let used = a.views.values().map(View::cache_bytes).sum::<usize>();
                if used + browser::bytes(&current) + 1024 * 1024 > 16 * 1024 * 1024 {
                    return Err("Docker 总缓存已达上限".into());
                }
                let v = a.views.get_mut(&job.view).ok_or("页面已关闭")?;
                if v.epoch != job.epoch || v.context != job.context || v.endpoint != job.endpoint {
                    return Ok(None);
                }
                let b = v.browser.as_mut().unwrap();
                if !b.owns(work) {
                    return Ok(None);
                }
                let r = b.objects.get_mut(&resource.key()).unwrap();
                r.observation = if resource.kind == Kind::Volume {
                    resources::volume_observation(&current)
                } else {
                    json!({"id":resource.id})
                };
                r.info = Some(ObjectInfo::read(resource.kind, &current));
                r.complete = !partial;
                if direct {
                    r.items = resources::relation(resource.kind, &current)
                        .as_array()
                        .cloned()
                        .unwrap_or_default();
                    if browser::bytes(&json!([r.items, r.observation, r.info, r.stats]))
                        > browser::RELATION_LIMIT
                    {
                        return Err("此对象关系超过 256 KiB，引用不完整".into());
                    }
                    b.trim_objects(&resource.key())?;
                    let r = b.objects.get_mut(&resource.key()).unwrap();
                    r.phase = "ready".into();
                    v.revision += 1;
                    return Ok(None);
                }
                return Ok(Some(Work::Object {
                    resource: resource.clone(),
                    generation: *generation,
                    ids: Some(scan),
                    offset: 0,
                }));
            }
            let ids = ids.as_ref().unwrap();
            let (rows, end) = if *offset < ids.len() {
                batch(
                    app,
                    job,
                    client,
                    Kind::Container,
                    resources::MOUNTS,
                    ids,
                    *offset,
                )?
            } else {
                (vec![], *offset)
            };
            let refs = references(resource, &rows);
            if end >= ids.len() {
                engine_check(app, job, client)?;
            }
            let mut a = app.lock().unwrap();
            let used = a.views.values().map(View::cache_bytes).sum::<usize>();
            if used + browser::bytes(&json!(refs)) + 1024 * 1024 > 16 * 1024 * 1024 {
                return Err("Docker 总缓存已达上限".into());
            }
            let v = a.views.get_mut(&job.view).ok_or("页面已关闭")?;
            if v.epoch != job.epoch || v.context != job.context || v.endpoint != job.endpoint {
                return Ok(None);
            }
            let b = v.browser.as_mut().unwrap();
            if !b.owns(work) {
                return Ok(None);
            }
            let r = b.objects.get_mut(&resource.key()).unwrap();
            r.items.extend(refs);
            if browser::bytes(&json!([r.items, r.observation, r.info, r.stats]))
                > browser::RELATION_LIMIT
            {
                return Err("此对象关系超过 256 KiB，引用不完整".into());
            }
            b.trim_objects(&resource.key())?;
            let r = b.objects.get_mut(&resource.key()).unwrap();
            v.revision += 1;
            if end >= ids.len() {
                r.phase = "ready".into();
                Ok(None)
            } else {
                Ok(Some(Work::Object {
                    resource: resource.clone(),
                    generation: *generation,
                    ids: Some(ids.clone()),
                    offset: end,
                }))
            }
        }
    }
}
fn references(resource: &ResourceRef, rows: &[Value]) -> Vec<Value> {
    rows.iter().filter(|row|if resource.kind==Kind::Image {row["imageId"]==resource.id}else{row["mounts"].as_array().is_some_and(|m|m.iter().any(|m|m["Type"]=="volume" && m["Name"]==resource.id))}).map(|row|json!({"label":commands::text(row["name"].as_str().unwrap_or("容器").trim_start_matches('/'),256),"resource":{"kind":"container","id":row["id"]}})).collect()
}
enum DeleteResult {
    More(DeleteScan),
    Done(Value),
}
fn resource_remove(app: &Shared, job: &Job, client: &Client) -> Result<DeleteResult, String> {
    let resource = job.resource.as_ref().ok_or("资源身份缺失")?;
    let inspect_resource = || {
        commands::object(&read(
            app,
            job,
            client,
            &[
                resource.kind.name(),
                "inspect",
                "--format",
                "{{json .}}",
                &resource.id,
            ],
            256 * 1024,
        )?)
    };
    let (observation, previous) = match &job.action {
        JobAction::ResourceRemove { observation, scan } => (observation, scan),
        _ => return Err("缺少删除任务".into()),
    };
    let mut scan = if let Some(scan) = previous {
        scan.clone()
    } else {
        engine_check(app, job, client)?;
        let current = inspect_resource()?;
        let (ids, partial) = if resource.kind == Kind::Network {
            (vec![], false)
        } else {
            enumerated(app, job, client, Kind::Container)?
        };
        if partial {
            return Err("容器集合超过 1000 个，引用检查不完整，不能删除".into());
        }
        DeleteScan {
            current,
            ids,
            offset: 0,
            refs: vec![],
        }
    };
    if scan.offset < scan.ids.len() {
        let (rows, end) = batch(
            app,
            job,
            client,
            Kind::Container,
            resources::MOUNTS,
            &scan.ids,
            scan.offset,
        )?;
        scan.refs.extend(references(resource, &rows));
        scan.offset = end;
        if !scan.refs.is_empty() {
            return Err(format!("仍被 {} 个容器引用，不能删除", scan.refs.len()));
        }
        return Ok(DeleteResult::More(scan));
    }
    engine_check(app, job, client)?;
    let current = inspect_resource()?;
    if resource.kind == Kind::Volume
        && resources::volume_observation(&current) != resources::volume_observation(&scan.current)
    {
        return Err("检查期间卷身份改变，请重新确认".into());
    }
    let refs = if resource.kind == Kind::Network {
        resources::relation(Kind::Network, &current)
            .as_array()
            .cloned()
            .unwrap_or_default()
    } else {
        scan.refs
    };
    let args = resources::verify_delete(resource, &current, observation, &refs, true)?;
    {
        let mut a = app.lock().unwrap();
        let v = a.views.get_mut(&job.view).ok_or("页面已关闭")?;
        v.data["operation"]["state"] = json!("running");
        v.data["operation"]["message"] = json!("等待 Docker 删除结果");
        v.revision += 1;
    }
    let output = execute(
        app,
        job,
        client,
        &args.iter().map(String::as_str).collect::<Vec<_>>(),
        256 * 1024,
    )?;
    let (state, message) = cli::outcome(&output.response, &output.stderr);
    Ok(DeleteResult::Done(
        json!({"state":"ready","operation":{"action":"resource.remove","resource":resource,"state":state,"message":message,"finishedAt":now()}}),
    ))
}
pub fn start(app: Shared, client: Client) {
    std::thread::spawn(move || {
        let mut foreground_streak = 0u8;
        while !client.is_closed() {
            let job = {
                let mut a = app.lock().unwrap();
                let due = a
                    .views
                    .iter()
                    .filter(|(_, v)| {
                        v.kind == "browser"
                            && v.browser.as_ref().is_some_and(|b| {
                                b.visible && !b.indexes.values().any(|i| i.phase == "loading")
                            })
                            && !v.terminal.pending()
                            && !v.tunnel.pending()
                            && !v.busy
                            && !v.frozen
                            && !v.paused
                            && v.due <= Instant::now()
                    })
                    .map(|(id, _)| id.clone())
                    .collect::<Vec<_>>();
                for id in due {
                    let _ = a.enqueue(&id, "refresh");
                }
                let commands = a
                    .views
                    .iter()
                    .filter(|(_, v)| !v.busy && !v.frozen && v.due <= Instant::now())
                    .filter_map(|(id, v)| {
                        if v.terminal.pending() {
                            Some((id.clone(), "terminalStatus"))
                        } else if v.tunnel.pending() {
                            Some((id.clone(), "tunnelStatus"))
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>();
                for (id, action) in commands {
                    let _ = a.enqueue(&id, action);
                }
                let index = if foreground_streak >= 3 {
                    0
                } else {
                    a.jobs
                        .iter()
                        .position(|j| j.action.browser().is_none())
                        .unwrap_or(0)
                };
                let job = a.jobs.remove(index);
                foreground_streak = if job.as_ref().is_some_and(|j| j.action.browser().is_some()) {
                    0
                } else {
                    foreground_streak.saturating_add(1).min(3)
                };
                job
            };
            if let Some(mut job) = job {
                if job.action.as_str().starts_with("connection.")
                    || job.action.as_str() == "setEndpoint"
                {
                    connection_request(&app, &job, &client);
                    continue;
                }
                if job.target.is_none()
                    && !job
                        .action
                        .command()
                        .is_some_and(|w| w.intent.input["revealRule"].is_string())
                {
                    match discover_target(&app, &job, &client) {
                        Ok(target) => {
                            let mut a = app.lock().unwrap();
                            let target = match a.version_target(target) {
                                Ok(target) => target,
                                Err(error) => {
                                    if let Some(v) = a.views.get_mut(&job.view) {
                                        v.busy = false;
                                        v.data["error"] = json!(error);
                                        v.data["state"] = json!("stale");
                                    }
                                    continue;
                                }
                            };
                            let valid = a.views.get(&job.view).is_some_and(|v| {
                                v.context == job.context && v.epoch == job.epoch && !v.cancelled
                            });
                            if !valid {
                                continue;
                            }
                            if (!job.engine.is_empty() && job.engine != target.engine_id)
                                || (!job.endpoint.is_empty()
                                    && job.endpoint != target.endpoint.uri())
                            {
                                let v = a.views.get_mut(&job.view).unwrap();
                                v.busy = false;
                                v.data["state"] = json!("stale");
                                v.data["error"] = json!("原资源连接身份已改变，请从资源树重新打开");
                                continue;
                            }
                            a.bindings.insert(job.context.to_string(), target.clone());
                            for v in a.views.values_mut().filter(|v| {
                                v.context == job.context
                                    && v.target.is_none()
                                    && (v.expected_engine.is_empty()
                                        || v.expected_engine == target.engine_id)
                            }) {
                                v.target = Some(target.clone());
                                v.endpoint = target.endpoint.uri();
                                v.expected_engine = target.engine_id.clone();
                            }
                            job.endpoint = target.endpoint.uri();
                            job.engine = target.engine_id.clone();
                            job.target = Some(target);
                        }
                        Err(error) => {
                            let mut a = app.lock().unwrap();
                            if let Some(v) = a
                                .views
                                .get_mut(&job.view)
                                .filter(|v| v.epoch == job.epoch && v.context == job.context)
                            {
                                v.busy = false;
                                v.data["state"] = json!("stale");
                                v.data["error"] = json!(commands::text(&error, 2048));
                                v.revision += 1;
                            }
                            continue;
                        }
                    }
                }
                if let Some(work) = job.action.browser() {
                    let result = browser_read(&app, &job, &client);
                    let mut a = app.lock().unwrap();
                    let owns = a.views.get(&job.view).is_some_and(|v| {
                        v.context == job.context
                            && v.epoch == job.epoch
                            && v.browser.as_ref().is_some_and(|b| b.owns(work))
                    });
                    if owns {
                        match result {
                            Ok(Some(next)) => {
                                if let Err(error) = a.enqueue_browser(&job.view, next.clone()) {
                                    let v = a.views.get_mut(&job.view).unwrap();
                                    v.browser.as_mut().unwrap().fail(&next, error);
                                    v.revision += 1;
                                }
                            }
                            Err(error) => {
                                let v = a.views.get_mut(&job.view).unwrap();
                                v.browser.as_mut().unwrap().fail(work, error);
                                v.revision += 1;
                            }
                            _ => {}
                        }
                        a.repage(&job.view);
                    }
                    let interval = a.refresh_seconds;
                    if let Some(v) = a
                        .views
                        .get_mut(&job.view)
                        .filter(|v| v.context == job.context && v.epoch == job.epoch)
                    {
                        if v.browser
                            .as_ref()
                            .is_some_and(|b| !b.indexes.values().any(|i| i.phase == "loading"))
                        {
                            v.due = Instant::now() + Duration::from_secs(interval);
                        }
                    }
                    continue;
                }
                if let Some(work) = job.action.command() {
                    let result = collaboration_request(&app, &job, &client);
                    let mut a = app.lock().unwrap();
                    if let Some(v) = a.views.get_mut(&job.view).filter(|v| {
                        v.context == job.context
                            && v.epoch == job.epoch
                            && v.endpoint == job.endpoint
                    }) {
                        v.busy = false;
                        v.due = Instant::now() + Duration::from_secs(1);
                        v.revision += 1;
                        let intent = v.command_mut(work.kind);
                        let mut receipt = result.unwrap_or_else(
                            |message| json!({"status":"failed","error":{"message":message}}),
                        );
                        receipt["containerId"] = intent.input["id"].clone();
                        receipt["name"] = intent.receipt["name"].clone();
                        intent.receipt = receipt;
                        if !intent.unresolved() {
                            intent.ticket = None;
                        }
                        let receipt = intent.receipt.clone();
                        let request = intent.input["requestId"].clone();
                        let outcome = receipt["result"]["outcome"].as_str();
                        let state = match receipt["status"].as_str() {
                            Some("completed")
                                if matches!(
                                    outcome,
                                    Some("selectionRequired" | "savedNotStarted")
                                ) =>
                            {
                                "skipped"
                            }
                            Some("completed") => "succeeded",
                            Some("failed" | "invalidated") => "failed",
                            Some("unknown") => "unknown",
                            _ => "checking",
                        };
                        let message = match outcome {
                            Some("selectionRequired") => "存在多条匹配规则，请在端口中选择已有隧道",
                            Some("savedNotStarted") => "规则已保存，尚未启动；请在网络隧道中检查",
                            _ => receipt["error"]["message"]
                                .as_str()
                                .or(receipt["message"].as_str())
                                .unwrap_or(if work.kind == CommandKind::Tunnel {
                                    "办理结果已更新；实际监听及 TCP 状态见端口"
                                } else {
                                    "终端页面办理结果已更新"
                                }),
                        };
                        let task = json!({"action":if work.kind==CommandKind::Tunnel{"tunnel"}else{"terminal"},"requestId":request,"containerId":receipt["containerId"],"name":receipt["name"],"state":state,"message":message,"result":receipt["result"]});
                        if let Some(old) = v.tasks.iter_mut().find(|t| t["requestId"] == request) {
                            *old = task;
                        } else {
                            v.tasks.push_front(task);
                            if v.tasks.len() > 64 {
                                v.tasks.pop_back();
                            }
                        }
                    }
                    continue;
                }
                let mut result = if job.action.as_str() == "resource.remove" {
                    match resource_remove(&app, &job, &client) {
                        Ok(DeleteResult::More(scan)) => {
                            let mut next = job.clone();
                            if let JobAction::ResourceRemove {
                                scan: next_scan, ..
                            } = &mut next.action
                            {
                                *next_scan = Some(scan);
                            }
                            app.lock().unwrap().jobs.push_back(next);
                            continue;
                        }
                        Ok(DeleteResult::Done(data)) => Ok(data),
                        Err(e) => Err(e),
                    }
                } else {
                    collect(&app, &job, &client)
                };
                if let Ok(data) = &mut result {
                    if !data["operation"].is_null() && !job.container.is_empty() {
                        data["operation"]["containerId"] = json!(job.container);
                        data["operation"]["name"] = if data["name"].is_null() {
                            app.lock()
                                .unwrap()
                                .views
                                .get(&job.view)
                                .map(|v| v.data["operation"]["name"].clone())
                                .unwrap_or(Value::Null)
                        } else {
                            data["name"].clone()
                        };
                    }
                    let mut a = app.lock().unwrap();
                    if let Some(v) = a.views.get_mut(&job.view).filter(|v| {
                        v.epoch == job.epoch
                            && v.context == job.context
                            && v.endpoint == job.endpoint
                            && v.kind == "browser"
                            && (!job.container.is_empty() || job.resource.is_some())
                    }) {
                        if job.action.as_str() == "stats" {
                            if let Some(o) = v.browser.as_mut().and_then(|b| {
                                b.objects.get_mut(&format!("container:{}", job.container))
                            }) {
                                o.stats = data["stats"].clone();
                            }
                        }
                        let mut rows = v.container_rows().to_vec();
                        if data["state"] == "gone" {
                            rows.retain(|row| row["id"] != job.container);
                        } else if let Some(row) =
                            rows.iter_mut().find(|row| row["id"] == job.container)
                        {
                            row["status"] = data["status"].clone();
                            row["health"] = data["health"].clone();
                        }
                        *data = json!({"engine":if data["engine"].is_null(){v.data["engine"].clone()}else{data["engine"].clone()},"rows":rows,"operation":data["operation"],
                            "observedAt":data["observedAt"],"state":"ready","partial":v.data["partial"],"error":null});
                    }
                }
                if !["refresh", "stats"].contains(&job.action.as_str()) {
                    let operation = match &result {
                        Ok(data) => data["operation"].clone(),
                        Err(_) => {
                            let a = app.lock().unwrap();
                            let running = a
                                .views
                                .get(&job.view)
                                .is_none_or(|v| v.data["operation"]["state"] == "running");
                            json!({"state":if running{"unknown"}else{"failed"}})
                        }
                    };
                    let state = operation["state"].as_str().unwrap_or("unknown");
                    let fields = json!({"viewId":job.view,"serverId":job.context["serverId"],"containerId":job.container,"resource":job.resource,"engineId":job.engine,"action":job.action,"result":state});
                    let _ = serde_json::from_value::<api::Log>(json!({"level":if ["failed","unknown"].contains(&state){"warn"}else{"info"},"message":"Docker 资源操作完成","fields":fields})).map_err(|_| sodeck_plugin_client::RpcError::invalid("核心请求参数无效")).and_then(|params| client.request(api::call::Log(params))).map_err(|e|e.message);
                    if ["failed", "unknown"].contains(&state) {
                        let _ = serde_json::from_value::<api::Message>(json!({"title":"Docker 资源操作未完成","body":format!("{} · {}：请在 Docker 资源树查看结果并刷新状态",commands::operation_name(job.action.as_str()),job.container.get(..12).unwrap_or(&job.container)),"level":"warning","notify":false})).map_err(|_| sodeck_plugin_client::RpcError::invalid("核心请求参数无效")).and_then(|params| client.request(api::call::Message(params))).map_err(|e|e.message);
                    }
                }
                let removed = ["remove", "resource.remove"].contains(&job.action.as_str())
                    && result
                        .as_ref()
                        .is_ok_and(|data| data["operation"]["state"] == "succeeded");
                let metadata = {
                    let mut a = app.lock().unwrap();
                    if removed {
                        for view in a.views.values_mut().filter(|v| {
                            v.context == job.context
                                && v.expected_engine == job.engine
                                && v.endpoint == job.endpoint
                                && if let Some(resource) = &job.resource {
                                    v.kind == "logs"
                                        && resource.kind == Kind::Container
                                        && v.container == resource.id
                                } else {
                                    v.kind == "logs" && v.container == job.container
                                }
                                && !v.busy
                        }) {
                            view.data["state"] = json!("gone");
                            view.revision += 1;
                        }
                    }
                    let used = a.views.values().map(View::cache_bytes).sum::<usize>();
                    if let Some(v) = a.views.get_mut(&job.view).filter(|v| {
                        v.context == job.context
                            && v.epoch == job.epoch
                            && v.endpoint == job.endpoint
                    }) {
                        v.busy = false;
                        v.cancelled = false;
                        v.due = Instant::now() + Duration::from_secs(5);
                        v.revision += 1;
                        match result {
                            Ok(mut data)
                                if used
                                    .saturating_sub(serde_json::to_vec(&v.data).unwrap().len())
                                    + serde_json::to_vec(&data).unwrap().len()
                                    <= 16 * 1024 * 1024 =>
                            {
                                if data["operation"].is_null() {
                                    data["operation"] = v.data["operation"].clone();
                                }
                                if data["stats"].is_null() {
                                    data["stats"] = v.data["stats"].clone();
                                }
                                if let Some(engine) = data["engine"]["id"].as_str() {
                                    v.expected_engine = engine.into();
                                }
                                if let Some(b) = &mut v.browser {
                                    if !job.container.is_empty() && !data["rows"].is_null() {
                                        b.indexes.get_mut(&Kind::Container).unwrap().rows =
                                            data["rows"].as_array().cloned().unwrap_or_default();
                                    }
                                    data["rows"] = json!([]);
                                    if job.action.as_str() == "resource.remove"
                                        && data["operation"]["state"] == "succeeded"
                                    {
                                        if let Some(r) = &job.resource {
                                            b.indexes
                                                .get_mut(&r.kind)
                                                .unwrap()
                                                .rows
                                                .retain(|row| row["id"] != r.id);
                                        }
                                    }
                                }
                                v.data = data;
                            }
                            Ok(_) => {
                                v.data["error"] =
                                    json!("Docker 缓存达到 16 MiB 上限，请关闭其它页面");
                                v.data["state"] = json!("stale");
                            }
                            Err(error) => {
                                if !["refresh", "stats"].contains(&job.action.as_str()) {
                                    let state = if v.data["operation"]["state"] == "running" {
                                        "unknown"
                                    } else {
                                        "failed"
                                    };
                                    let name = v.data["operation"]["name"].clone();
                                    v.data["operation"] = json!({"action":job.action,"containerId":job.container,"resource":job.resource,"name":name,"state":state,"message":commands::text(&error,2048),"finishedAt":now()});
                                }
                                v.data["error"] = json!(commands::text(&error, 2048));
                                v.data["state"] = json!("stale");
                            }
                        }
                        if !["refresh", "stats"].contains(&job.action.as_str()) {
                            v.tasks.push_front(v.data["operation"].clone());
                            if v.tasks.len() > 64 {
                                v.tasks.pop_back();
                            }
                        }
                        let name = v.data["name"].as_str().unwrap_or("容器");
                        Some(
                            json!({"viewId":job.view,"metadata":{"title":if v.kind=="browser"{"Docker".into()}else if v.kind=="logs"{format!("{name} · 日志")}else{name.to_string()},"dirty":false,"busy":false}}),
                        )
                    } else {
                        None
                    }
                };
                {
                    let mut a = app.lock().unwrap();
                    let kinds = a
                        .views
                        .get(&job.view)
                        .filter(|v| v.kind == "browser" && v.data["state"] == "ready")
                        .and_then(|v| v.browser.as_ref())
                        .map(|b| {
                            Kind::ALL
                                .into_iter()
                                .filter(|k| {
                                    *k == Kind::Container || b.indexes[k].phase != "unloaded"
                                })
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    for kind in kinds {
                        let work = a
                            .views
                            .get_mut(&job.view)
                            .and_then(|v| v.browser.as_mut())
                            .and_then(|b| b.load(kind));
                        if let Some(work) = work {
                            let _ = a.enqueue_browser(&job.view, work);
                        }
                    }
                    let resource = a
                        .views
                        .get(&job.view)
                        .filter(|v| {
                            v.kind == "browser"
                                && v.data["state"] == "ready"
                                && v.browser.as_ref().is_some_and(|b| b.visible)
                        })
                        .and_then(|v| ResourceRef::read(&v.navigation["inline"]["resource"]).ok());
                    if let Some(resource) = resource {
                        if let Ok(Some(work)) = a
                            .views
                            .get_mut(&job.view)
                            .unwrap()
                            .browser
                            .as_mut()
                            .unwrap()
                            .load_object(resource)
                        {
                            let _ = a.enqueue_browser(&job.view, work);
                        }
                    }
                    a.repage(&job.view);
                }
                if let Some(metadata) = metadata {
                    let _ = serde_json::from_value::<api::UpdateView>(metadata)
                        .map_err(|_| sodeck_plugin_client::RpcError::invalid("核心请求参数无效"))
                        .and_then(|params| client.request(api::call::UpdateView(params)))
                        .map_err(|e| e.message);
                }
            } else {
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    });
}

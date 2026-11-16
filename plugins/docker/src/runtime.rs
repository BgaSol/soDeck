use crate::{
    browser::{self, BrowserState, Work},
    commands,
    resources::{self, Kind, ResourceRef},
};
use serde_json::{json, Value};
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
    pub endpoints: BTreeMap<String, String>,
    pub directory: PathBuf,
    pub refresh_seconds: u64,
    pub show_log_minimap: bool,
    pub settings_revision: u64,
    jobs: VecDeque<Job>,
}
pub struct View {
    pub browser: Option<BrowserState>,
    pub resource: Option<ResourceRef>,
    pub epoch: u64,
    pub pull: Value,
    pub tasks: VecDeque<Value>,
    terminal_ticket: Option<sodeck_plugin_client::commands::Ticket>,
    terminal_receipt: Value,
    terminal_input: Value,
    pub kind: String,
    pub context: Value,
    pub container: String,
    pub expected_engine: String,
    pub endpoint: String,
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
struct Job {
    delete_scan: Option<DeleteScan>,
    epoch: u64,
    browser: Option<Work>,
    resource: Option<ResourceRef>,
    observation: Value,
    terminal: Option<sodeck_plugin_client::commands::Ticket>,
    view: String,
    context: Value,
    endpoint: String,
    container: String,
    engine: String,
    action: String,
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
            + browser::bytes(&json!([self.tasks, self.receipts]))
    }
    pub fn clear_terminal(&mut self) {
        self.terminal_ticket = None;
        self.terminal_receipt = Value::Null;
        self.terminal_input = Value::Null;
    }
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
impl App {
    pub fn new(directory: PathBuf) -> Result<Self, String> {
        let path = directory.join("endpoints.json");
        let endpoints = if path.exists() {
            if std::fs::metadata(&path).map_err(|e| e.to_string())?.len() > 64 * 1024 {
                return Err("Docker 连接配置过大".into());
            }
            let values: BTreeMap<String, String> =
                serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
                    .map_err(|_| "Docker 连接配置损坏，请检查 endpoints.json")?;
            for value in values.values() {
                commands::endpoint(value)?;
            }
            values
        } else {
            BTreeMap::new()
        };
        Ok(Self {
            views: BTreeMap::new(),
            endpoints,
            directory,
            refresh_seconds: 5,
            show_log_minimap: true,
            settings_revision: 0,
            jobs: VecDeque::new(),
        })
    }
    pub fn create(&mut self, p: &Value) -> Result<(), String> {
        let id = p["viewId"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 128)
            .ok_or("视图 ID 无效")?;
        let kind = p["viewType"].as_str().ok_or("缺少视图类型")?;
        if !["browser", "container", "logs", "resource"].contains(&kind) {
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
        let resource_ref = if kind == "resource" {
            let reference = ResourceRef::read(&resource)?;
            if reference.kind == Kind::Container {
                return Err("容器请使用容器详情入口".into());
            }
            Some(reference)
        } else {
            None
        };
        if kind != "browser" && kind != "resource" {
            commands::id(&container)?;
        }
        let mut endpoint = self
            .endpoints
            .get(context["serverId"].as_str().unwrap())
            .cloned()
            .unwrap_or("unix:///var/run/docker.sock".into());
        if kind == "resource" {
            endpoint = resource["endpoint"]
                .as_str()
                .ok_or("缺少资源连接端点")?
                .into();
            commands::endpoint(&endpoint)?;
        }
        let view = View {
            browser: if kind == "browser" {
                Some(BrowserState::default())
            } else {
                None
            },
            resource: resource_ref,
            epoch: 1,
            pull: Value::Null,
            tasks: VecDeque::new(),
            terminal_ticket: None,
            terminal_receipt: Value::Null,
            terminal_input: Value::Null,
            kind: kind.into(),
            context,
            container,
            expected_engine: resource["engineId"].as_str().unwrap_or("").into(),
            endpoint,
            revision: 1,
            data: json!({"state":"loading","error":null,"engine":null,"rows":[],"sections":{},"stats":null,"operation":null}),
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
    pub fn snapshot(&self, id: &str) -> Result<Value, String> {
        let v = self.views.get(id).ok_or("视图已关闭")?;
        let mut data = json!({"terminal":v.terminal_receipt,"kind":v.kind,"context":v.context,"containerId":v.container,"endpoint":v.endpoint,"revision":v.revision.to_string(),
            "busy":v.busy,"paused":v.paused,"nextIntent":(v.intent+1).to_string(),"navigation":v.navigation,
            "state":v.data["state"],"error":v.data["error"],"engine":v.data["engine"],"name":v.data["name"],"status":v.data["status"],
            "health":v.data["health"],"observedAt":v.data["observedAt"],"partial":v.data["partial"],"operation":v.data["operation"],
            "tty":v.data["tty"],"stats":v.data["stats"],"count":v.data["rows"].as_array().map_or(0,Vec::len),
            "sections":v.data["sections"].as_object().map(|m|m.keys().collect::<Vec<_>>()),"showLogMinimap":self.show_log_minimap});
        data["scopeEpoch"] = json!(v.epoch.to_string());
        data["resource"] = json!(v.resource);
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
        v.busy = true;
        v.cancelled = false;
        v.revision += 1;
        v.due = Instant::now()
            + Duration::from_secs(if v.terminal_ticket.is_some() {
                1
            } else {
                self.refresh_seconds
            });
        self.jobs.push_back(Job {
            delete_scan: None,
            epoch: v.epoch,
            browser: None,
            resource: v.resource.clone(),
            observation: Value::Null,
            terminal: v.terminal_ticket.clone(),
            view: id.into(),
            context: v.context.clone(),
            endpoint: v.endpoint.clone(),
            container: target
                .and_then(|t| t["id"].as_str())
                .unwrap_or(&v.container)
                .into(),
            engine: if let Some(target) = target {
                target["engineId"].as_str().unwrap().into()
            } else if v.kind == "browser" {
                String::new()
            } else {
                v.expected_engine.clone()
            },
            action: action.into(),
        });
        Ok(())
    }
    fn target_input(&self, id: &str, d: &Value) -> Result<Value, String> {
        let v = self.views.get(id).ok_or("视图已关闭")?;
        if !["browser", "container"].contains(&v.kind.as_str()) {
            return Err("当前页面不能操作容器".into());
        }
        let browser = v.kind == "browser";
        let container = d["id"]
            .as_str()
            .unwrap_or(if browser { "" } else { &v.container });
        commands::id(container)?;
        Ok(json!({"id":container,
            "engineId":d.get("engineId").cloned().unwrap_or_else(||if browser {Value::Null}else{json!(v.expected_engine)}),
            "endpoint":d.get("endpoint").cloned().unwrap_or_else(||if browser {Value::Null}else{json!(v.endpoint)}),
            "expectedContext":d.get("expectedContext").cloned().unwrap_or_else(||if browser {Value::Null}else{v.context.clone()})}))
    }
    fn target(&self, id: &str, input: &Value) -> Result<Value, String> {
        let v = self.views.get(id).ok_or("视图已关闭")?;
        if v.data["state"] != "ready"
            || v.expected_engine.is_empty()
            || input["engineId"] != v.expected_engine
            || input["endpoint"] != v.endpoint
            || input["expectedContext"] != v.context
        {
            return Err("容器连接身份已改变或数据待刷新，请重新选择".into());
        }
        let row = if v.kind == "browser" {
            v.container_rows()
                .iter()
                .find(|row| row["id"] == input["id"])
                .ok_or("容器已不在列表中，请刷新后重新选择")?
        } else {
            if input["id"] != v.container {
                return Err("容器身份已改变".into());
            }
            &v.data
        };
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
            delete_scan: None,
            epoch: v.epoch,
            browser: Some(work),
            resource: None,
            observation: Value::Null,
            terminal: None,
            view: id.into(),
            context: v.context.clone(),
            endpoint: v.endpoint.clone(),
            container: String::new(),
            engine: v.expected_engine.clone(),
            action: "browser.read".into(),
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
            let relation = b.relations.get(&resource.key());
            return Ok(
                json!({"resource":resource,"revision":b.indexes[&resource.kind].revision.to_string(),"complete":b.indexes[&resource.kind].complete,"aliases":shown,"aliasesTotal":aliases.len(),"aliasesTruncated":shown.len()<aliases.len(),"relation":relation.map(|r|json!({"phase":r.phase,"complete":r.complete,"count":r.items.len()}))}),
            );
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
            b.load_relation(ResourceRef::read(&d["resource"])?)?
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
            .relations
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
        self.enqueue_target(
            id,
            "resource.remove",
            Some(&json!({"id":resource.id,"engineId":d["engineId"]})),
        )?;
        let job = self.jobs.back_mut().unwrap();
        job.resource = Some(resource.clone());
        job.observation = observation;
        let v = self.views.get_mut(id).unwrap();
        v.browser.as_mut().unwrap().invalidate();
        v.data["operation"] = json!({"action":"resource.remove","resource":resource,"name":name,"state":"checking","message":"重新检查身份和完整引用","submittedAt":now()});
        self.snapshot(id)
    }
    pub fn event(&mut self, id: &str, d: &Value) -> Result<Value, String> {
        let action = d["action"].as_str().ok_or("缺少操作")?;
        if action == "snapshot" {
            return self.snapshot(id);
        }
        if matches!(action, "list" | "section") {
            let v = self.views.get(id).ok_or("视图已关闭")?;
            let items = match action {
                "list" => &v.data["rows"],
                _ => {
                    let section = d["section"].as_str().ok_or("缺少详情分区")?;
                    if ![
                        "basic",
                        "state",
                        "environment",
                        "ports",
                        "mounts",
                        "networks",
                        "labels",
                    ]
                    .contains(&section)
                    {
                        return Err("未知分区".into());
                    }
                    &v.data["sections"][section]
                }
            };
            if action == "list" {
                let query = commands::text(d["query"].as_str().unwrap_or(""), 1024).to_lowercase();
                let status = d["status"].as_str().unwrap_or("all");
                let health = d["health"].as_str().unwrap_or("all");
                let mut filtered = v
                    .container_rows()
                    .iter()
                    .filter(|r| {
                        let state = r["status"].as_str().unwrap_or("");
                        let abnormal = state == "restarting"
                            || state == "dead"
                            || r["health"] == "unhealthy"
                            || (state == "exited"
                                && r["exitCode"].as_u64().is_some_and(|n| n != 0));
                        (status == "all" || (status == "abnormal" && abnormal) || status == state)
                            && (health == "all"
                                || (health == "none" && r["health"].is_null())
                                || r["health"] == health)
                            && (query.is_empty()
                                || ["name", "id", "image", "search"].iter().any(|key| {
                                    r[*key]
                                        .as_str()
                                        .unwrap_or("")
                                        .to_lowercase()
                                        .contains(&query)
                                }))
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                if d["group"] == true {
                    filtered.sort_by_key(|r| r["project"].as_str().unwrap_or("").to_string());
                }
                return Ok(page(
                    &json!(filtered),
                    d["offset"].as_u64().unwrap_or(0) as usize,
                    50,
                ));
            }
            return Ok(page(
                items,
                d["offset"].as_u64().unwrap_or(0) as usize,
                d["limit"].as_u64().unwrap_or(50).min(100) as usize,
            ));
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
            let shell = d["shell"].as_str().unwrap_or("/bin/sh");
            let mut input = self.target_input(id, d)?;
            input["requestId"] = json!(request);
            input["shell"] = json!(shell);
            let v = self.views.get(id).unwrap();
            if v.terminal_input["requestId"] == request {
                if v.terminal_input != input {
                    return Err("相同终端请求参数不同".into());
                }
                return self.snapshot(id);
            }
            if v.terminal_ticket.is_some() {
                return Err("已有终端请求待确认，请等待原请求结果".into());
            }
            let target = self.target(id, &input)?;
            if target["status"] != "running" {
                return Err("请先刷新并选择运行中的容器".into());
            }
            let command = commands::terminal_launch(
                &v.endpoint,
                input["engineId"].as_str().unwrap(),
                input["id"].as_str().unwrap(),
                shell,
            )?;
            let ticket = sodeck_plugin_client::commands::Ticket::new(
                v.context.clone(),
                id,
                request,
                "terminal.open.v1",
                json!({"version":1,"title":format!("{} · 容器",commands::text(target["name"].as_str().unwrap_or("Docker"),100)),"launch":{"kind":"command","command":command},"directoryIntegration":"off"}),
            );
            let v = self.views.get_mut(id).unwrap();
            v.terminal_ticket = Some(ticket);
            v.terminal_receipt = json!({"status":"pending","containerId":input["id"],"name":target["name"],"message":"检查容器并请求终端页面"});
            if let Err(error) = self.enqueue_target(id, "terminal", Some(&input)) {
                self.views.get_mut(id).unwrap().clear_terminal();
                return Err(error);
            }
            self.views.get_mut(id).unwrap().terminal_input = input;
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
        } else if action == "setEndpoint" {
            let value = d["endpoint"].as_str().ok_or("缺少 socket")?;
            commands::endpoint(value)?;
            let server = self.views.get(id).ok_or("视图已关闭")?.context["serverId"]
                .as_str()
                .unwrap()
                .to_string();
            if self.views.values().any(|v| {
                v.context["serverId"] == server
                    && (v.busy
                        || v.kind != "browser"
                        || matches!(v.pull["state"].as_str(), Some("opening" | "running")))
            }) {
                return Err(
                    "请先关闭该服务器的资源详情和日志页，并等待当前请求及镜像拉取结束".into(),
                );
            }
            let mut endpoints = self.endpoints.clone();
            endpoints.insert(server.clone(), value.into());
            let bytes = serde_json::to_vec(&endpoints).unwrap();
            if bytes.len() > 64 * 1024 {
                return Err("连接配置已达上限".into());
            }
            std::fs::create_dir_all(&self.directory).map_err(|e| e.to_string())?;
            let temp = self.directory.join("endpoints.tmp");
            std::fs::write(&temp, bytes)
                .and_then(|_| std::fs::rename(&temp, self.directory.join("endpoints.json")))
                .map_err(|e| e.to_string())?;
            self.endpoints = endpoints;
            for v in self
                .views
                .values_mut()
                .filter(|v| v.context["serverId"] == server)
            {
                v.endpoint = value.into();
                v.epoch += 1;
                if let Some(b) = &mut v.browser {
                    *b = BrowserState::default();
                }
                v.navigation = browser::navigation(&json!({}), &Value::Null)?;
                v.clear_terminal();
                v.expected_engine.clear();
                v.data = json!({"rows":[],"state":"loading"});
                v.revision += 1;
            }
            self.enqueue(id, "refresh")?;
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
        if bytes + size > 20 * 1024 {
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
    let params = {
        let mut a = app.lock().unwrap();
        if a.views.values().map(View::cache_bytes).sum::<usize>() + budget * 2 > 16 * 1024 * 1024 {
            return Err("Docker 缓存与临时输出超过 16 MiB，请关闭其它页面".into());
        }
        let v = a.views.get_mut(&job.view).ok_or("页面已关闭")?;
        if v.context != job.context
            || v.epoch != job.epoch
            || v.endpoint != job.endpoint
            || v.frozen
            || v.cancelled
        {
            return Err("请求已取消或上下文已改变".into());
        }
        v.sequence += 1;
        let command = commands::docker(&job.endpoint, args);
        if command.len() > 16 * 1024 {
            return Err("Docker 命令超过 16 KiB".into());
        }
        json!({"viewId":job.view,"expectedContext":job.context,"operationId":format!("docker-{}-{}",job.view,v.sequence),"sequence":v.sequence.to_string(),
            "command":command,"timeoutMs":30000,"maxOutputBytes":budget})
    };
    let mut task = None;
    for _ in 0..3 {
        match client.call_detailed("session.exec.start", params.clone()) {
            Ok(result) => {
                task = result["taskId"].as_str().map(str::to_owned);
                break;
            }
            Err(error) if error.code == -32003 => continue,
            Err(error) => return Err(error.message),
        }
    }
    let task = task.ok_or("命令启动回执未确认；结果可能未知，请刷新，禁止自动重发")?;
    let result = (|| {
        let mut stdout = vec![];
        let mut stderr = vec![];
        let mut a = "0".to_string();
        let mut b = "0".to_string();
        let end = Instant::now() + Duration::from_secs(35);
        loop {
            if !alive(app, job, client) || Instant::now() > end {
                return Err("本地等待已取消或超时；已发送操作的结果未确认".into());
            }
            let r = match client.call_detailed(
                "session.exec.poll",
                json!({"taskId":task,"stdoutOffset":a,"stderrOffset":b}),
            ) {
                Ok(r) => r,
                Err(e) if e.code == -32003 => continue,
                Err(e) => return Err(e.message),
            };
            let out = sodeck_plugin_client::encoding::unbase64(
                r["stdout"]["dataBase64"].as_str().ok_or("缺少 stdout")?,
            )
            .map_err(|_| "核心输出编码无效")?;
            let err = sodeck_plugin_client::encoding::unbase64(
                r["stderr"]["dataBase64"].as_str().ok_or("缺少 stderr")?,
            )
            .map_err(|_| "核心输出编码无效")?;
            if stdout.len() + stderr.len() + out.len() + err.len() > budget {
                return Err("命令输出超过本地预算".into());
            }
            stdout.extend(out);
            stderr.extend(err);
            a = r["stdout"]["nextOffset"]
                .as_str()
                .ok_or("输出偏移无效")?
                .into();
            b = r["stderr"]["nextOffset"]
                .as_str()
                .ok_or("输出偏移无效")?
                .into();
            if r["outputComplete"] == true {
                return Ok(Exec {
                    bytes: stdout,
                    stderr,
                    response: r,
                });
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    })();
    let _ = client.call("session.exec.release", json!({"taskId":task}));
    result
}
fn checked(result: Exec) -> Result<Vec<u8>, String> {
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
            delete_scan: None,
            epoch: v.epoch,
            browser: None,
            resource: None,
            observation: Value::Null,
            terminal: None,
            view: view.into(),
            context: v.context.clone(),
            endpoint: v.endpoint.clone(),
            container: v.container.clone(),
            engine: v.expected_engine.clone(),
            action: "logs".into(),
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
    Ok(target)
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
fn terminal_request(app: &Shared, job: &Job, client: &Client) -> Result<Value, String> {
    let ticket = job.terminal.as_ref().ok_or("终端票据已失效")?;
    if job.action == "terminal" {
        ticket.resolve(client).map_err(|e| e.message)?;
        let info = commands::object(&checked(execute(
            app,
            job,
            client,
            &["info", "--format", "{{json .}}"],
            256 * 1024,
        )?)?)?;
        if info["ID"] != job.engine {
            return Err("Engine 已改变，请重新打开容器详情".into());
        }
        let container = inspect(app, job, client)?;
        if container["State"]["Running"] != true || container["State"]["Paused"] == true {
            return Err("容器当前不能接受交互终端，请刷新状态".into());
        }
    }
    let reply = if job.action == "terminal" {
        ticket.submit(client)
    } else {
        ticket.get(client)
    };
    match reply {
        Ok(receipt) => Ok(receipt),
        Err(e) if e.code == -32003 => {
            Ok(json!({"status":"pending","message":"请求回执待确认，保留原编号继续查询"}))
        }
        Err(e) if e.data["code"] == "notFound" => ticket
            .submit(client)
            .or_else(|e| {
                if e.code == -32003 {
                    Ok(json!({"status":"pending"}))
                } else {
                    Err(e)
                }
            })
            .map_err(|e| e.message),
        Err(e) => Err(e.message),
    }
}
fn collect(app: &Shared, job: &Job, client: &Client) -> Result<Value, String> {
    let session = client.call("session.state", json!({"viewId":job.view}))?;
    if session["state"] != "connected" || session["context"] != job.context {
        return Err("服务器连接已断开或代次改变，请重新连接并刷新".into());
    }
    let version = commands::object(&checked(execute(
        app,
        job,
        client,
        &["version", "--format", "{{json .}}"],
        256 * 1024,
    )?)?)?;
    if version["Server"]["Os"] != "linux" {
        return Err("第一版仅支持远端 Linux Docker Engine".into());
    }
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
    if !job.engine.is_empty() && job.engine != engine_id {
        return Err("Engine 已改变，原容器页面失效，请从列表重新打开".into());
    }
    let engine = json!({"id":engine_id,"name":commands::text(info["Name"].as_str().unwrap_or("Docker"),256),"version":commands::text(version["Server"]["Version"].as_str().unwrap_or(""),64),
        "os":"linux","arch":commands::text(version["Server"]["Arch"].as_str().unwrap_or(""),64),"containers":info["Containers"].as_u64(),"images":info["Images"].as_u64(),"systemTime":commands::text(info["SystemTime"].as_str().unwrap_or(""),128),"dockerRootDir":resources::root(&info["DockerRootDir"])});
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
    if kind == "resource" {
        let resource = job.resource.as_ref().ok_or("资源身份缺失")?;
        let value = commands::object(&checked(execute(
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
        )?)?)?;
        if value[if resource.kind == Kind::Volume {
            "Name"
        } else {
            "Id"
        }] != resource.id
        {
            return Err("资源身份已改变".into());
        }
        return Ok(
            json!({"engine":engine,"name":value["Name"].as_str().unwrap_or(&resource.id),"sections":resources::fields(resource.kind,&value),"state":"ready","observedAt":now(),"partial":false,"error":null}),
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
        "sections":commands::fields(&container),"observedAt":now(),"state":"ready","error":null,"partial":false});
    if kind == "logs" {
        data["tty"] = container["Config"]["Tty"].clone();
        data["sections"] = json!({});
    } else if job.action == "stats" {
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
    } else if job.action != "refresh" {
        data["stats"] = json!({"unavailable":"容器操作后，请重新读取资源快照"});
        let args = commands::action(&job.action, &container)?;
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
            let known =
                r.response["remoteExitConfirmed"] == true && r.response["state"] == "completed";
            let success = known && r.response["exitCode"] == 0;
            data["operation"] = json!({"action":job.action,"state":if !known{"unknown"}else if success{"succeeded"}else{"failed"},
                "message":if !known{"结果未确认，请刷新状态，操作不会自动重发".into()}else if success{"命令成功，正在确认当前状态".into()}else{commands::text(&String::from_utf8_lossy(&r.stderr),2048)},"finishedAt":now()});
            if success {
                match inspect(app, job, client) {
                    Ok(after) => {
                        data["status"] = after["State"]["Status"].clone();
                        data["health"] = after["State"]["Health"]["Status"].clone();
                        data["sections"] = commands::fields(&after);
                        data["operation"]["message"] = json!("命令成功；已刷新当前观测状态");
                    }
                    Err(_) if job.action == "remove" => {
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
        if commands::docker(&job.endpoint, &args).len() <= 16 * 1024 {
            let out = read(app, job, client, &args, 1024 * 1024)?;
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
        if end <= offset + 1 {
            return Err("命令参数超过 16 KiB".into());
        }
        end -= 1;
    }
}
fn browser_read(app: &Shared, job: &Job, client: &Client) -> Result<Option<Work>, String> {
    let work = job.browser.as_ref().ok_or("缺少树读取任务")?;
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
        Work::Relation {
            resource,
            generation,
            ids,
            offset,
        } => {
            if ids.is_none() {
                engine_check(app, job, client)?;
                let format = if resource.kind == Kind::Container {
                    resources::CONTAINER_RELATIONS
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
                let r = b.relations.get_mut(&resource.key()).unwrap();
                r.observation = if resource.kind == Kind::Volume {
                    resources::volume_observation(&current)
                } else {
                    json!({"id":resource.id})
                };
                r.complete = !partial;
                if direct {
                    r.items = resources::relation(resource.kind, &current)
                        .as_array()
                        .cloned()
                        .unwrap_or_default();
                    if browser::bytes(&json!([r.items, r.observation])) > browser::RELATION_LIMIT {
                        return Err("此对象关系超过 256 KiB，引用不完整".into());
                    }
                    if b.relations
                        .values()
                        .map(|r| browser::bytes(&json!([r.items, r.observation])))
                        .sum::<usize>()
                        > 2 * 1024 * 1024
                    {
                        return Err("关系缓存超过 2 MiB".into());
                    }
                    let r = b.relations.get_mut(&resource.key()).unwrap();
                    r.phase = "ready".into();
                    v.revision += 1;
                    return Ok(None);
                }
                return Ok(Some(Work::Relation {
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
            let r = b.relations.get_mut(&resource.key()).unwrap();
            r.items.extend(refs);
            if browser::bytes(&json!([r.items, r.observation])) > browser::RELATION_LIMIT {
                return Err("此对象关系超过 256 KiB，引用不完整".into());
            }
            if b.relations
                .values()
                .map(|r| browser::bytes(&json!([r.items, r.observation])))
                .sum::<usize>()
                > 2 * 1024 * 1024
            {
                return Err("关系缓存超过 2 MiB".into());
            }
            let r = b.relations.get_mut(&resource.key()).unwrap();
            v.revision += 1;
            if end >= ids.len() {
                r.phase = "ready".into();
                Ok(None)
            } else {
                Ok(Some(Work::Relation {
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
    let mut scan = if let Some(scan) = &job.delete_scan {
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
    let args = resources::verify_delete(resource, &current, &job.observation, &refs, true)?;
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
    let known =
        output.response["state"] == "completed" && output.response["remoteExitConfirmed"] == true;
    let success = known && output.response["exitCode"] == 0;
    Ok(DeleteResult::Done(
        json!({"state":"ready","operation":{"action":"resource.remove","resource":resource,"state":if !known{"unknown"}else if success{"succeeded"}else{"failed"},"message":if !known{"结果未确认，请刷新；不会自动重发".into()}else if success{"删除命令成功".into()}else{commands::text(&String::from_utf8_lossy(&output.stderr),2048)},"finishedAt":now()}}),
    ))
}
pub fn start(app: Shared, client: Client) {
    std::thread::spawn(move || {
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
                            && v.terminal_ticket.is_none()
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
                    .filter(|(_, v)| {
                        v.terminal_ticket.is_some()
                            && !v.busy
                            && !v.frozen
                            && v.due <= Instant::now()
                    })
                    .map(|(id, _)| id.clone())
                    .collect::<Vec<_>>();
                for id in commands {
                    let _ = a.enqueue(&id, "terminalStatus");
                }
                let index = a.jobs.iter().position(|j| j.browser.is_none()).unwrap_or(0);
                a.jobs.remove(index)
            };
            if let Some(job) = job {
                if let Some(work) = &job.browser {
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
                    continue;
                }
                if job.action.starts_with("terminal") {
                    let result = terminal_request(&app, &job, &client);
                    let mut a = app.lock().unwrap();
                    if let Some(v) = a.views.get_mut(&job.view).filter(|v| {
                        v.context == job.context
                            && v.epoch == job.epoch
                            && v.endpoint == job.endpoint
                    }) {
                        v.busy = false;
                        v.due = Instant::now() + Duration::from_secs(1);
                        v.revision += 1;
                        let mut receipt = result.unwrap_or_else(
                            |message| json!({"status":"failed","error":{"message":message}}),
                        );
                        receipt["containerId"] = v.terminal_input["id"].clone();
                        receipt["name"] = v.terminal_receipt["name"].clone();
                        v.terminal_receipt = receipt;
                        if v.terminal_receipt["status"] != "pending" {
                            v.terminal_ticket = None;
                        }
                    }
                    continue;
                }
                let mut result = if job.action == "resource.remove" {
                    match resource_remove(&app, &job, &client) {
                        Ok(DeleteResult::More(scan)) => {
                            let mut next = job.clone();
                            next.delete_scan = Some(scan);
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
                    if !data["operation"].is_null() {
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
                    let a = app.lock().unwrap();
                    if let Some(v) = a
                        .views
                        .get(&job.view)
                        .filter(|v| v.kind == "browser" && !job.container.is_empty())
                    {
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
                    let fields = json!({"viewId":job.view,"serverId":job.context["serverId"],"containerId":job.container,"engineId":job.engine,"action":job.action,"result":state});
                    let _ = client.call("logging.write", json!({"level":if ["failed","unknown"].contains(&state){"warn"}else{"info"},"message":"Docker 容器操作完成","fields":fields}));
                    if ["failed", "unknown"].contains(&state) {
                        let _ = client.call("messages.publish", json!({"title":"Docker 资源操作未完成","body":format!("{} · {}：请在 Docker 侧栏或详情查看结果并刷新状态",commands::operation_name(&job.action),job.container.get(..12).unwrap_or(&job.container)),"level":"warning","notify":false}));
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
                                    v.resource.as_ref() == Some(resource)
                                } else {
                                    v.container == job.container && v.resource.is_none()
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
                                    if v.kind == "browser"
                                        && !v.expected_engine.is_empty()
                                        && v.expected_engine != engine
                                    {
                                        v.epoch += 1;
                                        v.browser = Some(BrowserState::default());
                                        v.navigation=browser::navigation(&json!({"query":v.navigation["query"],"status":v.navigation["status"],"health":v.navigation["health"],"group":v.navigation["group"]}),&Value::Null).unwrap();
                                    }
                                    v.expected_engine = engine.into();
                                }
                                if let Some(b) = &mut v.browser {
                                    if !job.container.is_empty() && !data["rows"].is_null() {
                                        b.indexes.get_mut(&Kind::Container).unwrap().rows =
                                            data["rows"].as_array().cloned().unwrap_or_default();
                                    }
                                    data["rows"] = json!([]);
                                    if job.action == "resource.remove"
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
                                    v.data["operation"] = json!({"action":job.action,"containerId":job.container,"name":name,"state":state,"message":commands::text(&error,2048),"finishedAt":now()});
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
                    a.repage(&job.view);
                }
                if let Some(metadata) = metadata {
                    let _ = client.call("ui.updateView", metadata);
                }
            } else {
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    });
}

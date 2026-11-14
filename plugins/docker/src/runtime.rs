use crate::commands;
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
    pub tail: u64,
    pub settings_revision: u64,
    jobs: VecDeque<Job>,
}
pub struct View {
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
    terminal: Option<sodeck_plugin_client::commands::Ticket>,
    view: String,
    context: Value,
    endpoint: String,
    container: String,
    engine: String,
    action: String,
    tail: u64,
}
struct Exec {
    bytes: Vec<u8>,
    stderr: Vec<u8>,
    response: Value,
}
impl View {
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
            tail: 200,
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
        if !["browser", "container", "logs"].contains(&kind) {
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
        let resource: Value = p["resourceKey"]
            .as_str()
            .map(serde_json::from_str)
            .transpose()
            .map_err(|_| "资源标识无效")?
            .unwrap_or(Value::Null);
        let container = resource["id"].as_str().unwrap_or("").to_string();
        if kind != "browser" {
            commands::id(&container)?;
        }
        let endpoint = self
            .endpoints
            .get(context["serverId"].as_str().unwrap())
            .cloned()
            .unwrap_or("unix:///var/run/docker.sock".into());
        let view = View {
            terminal_ticket: None,
            terminal_receipt: Value::Null,
            terminal_input: Value::Null,
            kind: kind.into(),
            context,
            container,
            expected_engine: resource["engineId"].as_str().unwrap_or("").into(),
            endpoint,
            revision: 1,
            data: json!({"state":"loading","error":null,"engine":null,"rows":[],"sections":{},"logs":[],"stats":null,"operation":null}),
            navigation: json!({}),
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
        self.enqueue(id, "refresh", self.tail)
    }
    pub fn snapshot(&self, id: &str) -> Result<Value, String> {
        let v = self.views.get(id).ok_or("视图已关闭")?;
        Ok(
            json!({"terminal":v.terminal_receipt,"kind":v.kind,"context":v.context,"containerId":v.container,"endpoint":v.endpoint,"revision":v.revision.to_string(),
            "busy":v.busy,"paused":v.paused,"nextIntent":(v.intent+1).to_string(),"navigation":v.navigation,
            "state":v.data["state"],"error":v.data["error"],"engine":v.data["engine"],"name":v.data["name"],"status":v.data["status"],
            "health":v.data["health"],"observedAt":v.data["observedAt"],"partial":v.data["partial"],"operation":v.data["operation"],
            "stats":v.data["stats"],"count":v.data["rows"].as_array().map_or(0,Vec::len),"logCount":v.data["logs"].as_array().map_or(0,Vec::len),
            "sections":v.data["sections"].as_object().map(|m|m.keys().collect::<Vec<_>>()),"tail":self.tail}),
        )
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
    pub fn enqueue(&mut self, id: &str, action: &str, tail: u64) -> Result<(), String> {
        self.enqueue_target(id, action, tail, None)
    }
    fn enqueue_target(
        &mut self,
        id: &str,
        action: &str,
        tail: u64,
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
            tail,
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
            v.data["rows"]
                .as_array()
                .and_then(|rows| rows.iter().find(|row| row["id"] == input["id"]))
                .ok_or("容器已不在列表中，请刷新后重新选择")?
        } else {
            if input["id"] != v.container {
                return Err("容器身份已改变".into());
            }
            &v.data
        };
        Ok(json!({"name":row["name"],"status":row["status"]}))
    }
    pub fn event(&mut self, id: &str, d: &Value) -> Result<Value, String> {
        let action = d["action"].as_str().ok_or("缺少操作")?;
        if action == "snapshot" {
            return self.snapshot(id);
        }
        if matches!(action, "list" | "section" | "readLogs") {
            let v = self.views.get(id).ok_or("视图已关闭")?;
            let items = match action {
                "list" => &v.data["rows"],
                "readLogs" => &v.data["logs"],
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
                let mut filtered = items
                    .as_array()
                    .into_iter()
                    .flatten()
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
            if action == "readLogs" {
                let query = commands::text(d["query"].as_str().unwrap_or(""), 1024).to_lowercase();
                let filtered = items
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|row| {
                        query.is_empty()
                            || row["text"]
                                .as_str()
                                .unwrap_or("")
                                .to_lowercase()
                                .contains(&query)
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                return Ok(page(
                    &json!(filtered),
                    d["offset"].as_u64().unwrap_or(0) as usize,
                    d["limit"].as_u64().unwrap_or(50).min(100) as usize,
                ));
            }
            return Ok(page(
                items,
                d["offset"].as_u64().unwrap_or(0) as usize,
                d["limit"].as_u64().unwrap_or(50).min(100) as usize,
            ));
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
            if let Err(error) = self.enqueue_target(id, "terminal", self.tail, Some(&input)) {
                self.views.get_mut(id).unwrap().clear_terminal();
                return Err(error);
            }
            self.views.get_mut(id).unwrap().terminal_input = input;
        } else if action == "navigation" {
            if serde_json::to_vec(&d["state"]).unwrap().len() > 4096 {
                return Err("页面状态过大".into());
            }
            self.views.get_mut(id).ok_or("视图已关闭")?.navigation = d["state"].clone();
        } else if action == "refresh" || action == "stats" {
            let tail = d["tail"].as_u64().unwrap_or(self.tail);
            if !(1..=500).contains(&tail) {
                return Err("日志行数必须在 1–500 之间".into());
            }
            self.enqueue(id, action, tail)?;
        } else if action == "cancel" {
            let v = self.views.get_mut(id).ok_or("视图已关闭")?;
            v.cancelled = true;
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
            if self
                .views
                .values()
                .any(|v| v.context["serverId"] == server && (v.busy || v.kind != "browser"))
            {
                return Err("请先关闭该服务器的容器详情和日志页，并等待当前请求结束".into());
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
                v.clear_terminal();
                v.expected_engine.clear();
                v.data = json!({"rows":[],"state":"loading"});
                v.revision += 1;
            }
            self.enqueue(id, "refresh", self.tail)?;
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
            self.enqueue_target(id, op, self.tail, Some(&input))?;
            let v = self.views.get_mut(id).unwrap();
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
        && app
            .lock()
            .unwrap()
            .views
            .get(&job.view)
            .is_some_and(|v| v.context == job.context && !v.cancelled)
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
        let v = a.views.get_mut(&job.view).ok_or("页面已关闭")?;
        if v.context != job.context || v.frozen || v.cancelled {
            return Err("请求已取消或上下文已改变".into());
        }
        v.sequence += 1;
        json!({"viewId":job.view,"expectedContext":job.context,"operationId":format!("docker-{}-{}",job.view,v.sequence),"sequence":v.sequence.to_string(),
            "command":commands::docker(&job.endpoint,args),"timeoutMs":30000,"maxOutputBytes":budget})
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
        .filter(|s| !s.is_empty())
        .ok_or("Engine ID 不可用")?;
    if !job.engine.is_empty() && job.engine != engine_id {
        return Err("Engine 已改变，原容器页面失效，请从列表重新打开".into());
    }
    let engine = json!({"id":engine_id,"name":commands::text(info["Name"].as_str().unwrap_or("Docker"),256),"version":version["Server"]["Version"],
        "os":version["Server"]["Os"],"arch":version["Server"]["Arch"],"containers":info["Containers"],"images":info["Images"]});
    let kind = app
        .lock()
        .unwrap()
        .views
        .get(&job.view)
        .ok_or("页面已关闭")?
        .kind
        .clone();
    if kind == "browser" && job.container.is_empty() {
        let bytes = checked(execute(
            app,
            job,
            client,
            &["container", "ls", "--all", "--quiet", "--no-trunc"],
            256 * 1024,
        )?)?;
        let ids = std::str::from_utf8(&bytes)
            .map_err(|_| "容器 ID 格式不兼容")?
            .lines()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        let mut rows = vec![];
        let mut partial = ids.len() > 1000;
        let mut error = None;
        let mut size = 0;
        for group in ids.iter().take(1000).collect::<Vec<_>>().chunks(16) {
            for id in group {
                commands::id(id)?;
            }
            let mut args = vec!["container", "inspect", "--format", commands::SUMMARY];
            args.extend(group.iter().copied().copied());
            let r = execute(app, job, client, &args, 1024 * 1024)?;
            if r.response["state"] != "completed" || r.response["exitCode"] != 0 {
                partial = true;
                error = Some(format!(
                    "部分容器详情未能读取：{}",
                    commands::text(&String::from_utf8_lossy(&r.stderr), 1024)
                ));
            }
            for mut row in commands::lines(&r.bytes)? {
                commands::id(row["id"].as_str().ok_or("容器 ID 缺失")?)?;
                row["name"] = json!(commands::text(
                    row["name"].as_str().unwrap_or("").trim_start_matches('/'),
                    256
                ));
                row["image"] = json!(commands::text(row["image"].as_str().unwrap_or(""), 512));
                row["project"] = json!(commands::text(
                    row["labels"]["com.docker.compose.project"]
                        .as_str()
                        .unwrap_or(""),
                    256
                ));
                row["service"] = json!(commands::text(
                    row["labels"]["com.docker.compose.service"]
                        .as_str()
                        .unwrap_or(""),
                    256
                ));
                row["search"] = json!(commands::text(&row["labels"].to_string(), 4096));
                row.as_object_mut().unwrap().remove("labels");
                if serde_json::to_vec(&row).unwrap().len() > 16 * 1024 {
                    row["ports"] = json!("[端口过多，请打开详情]");
                    partial = true;
                }
                size += serde_json::to_vec(&row).unwrap().len();
                if size > 2 * 1024 * 1024 {
                    partial = true;
                    break;
                }
                rows.push(row);
            }
            if size > 2 * 1024 * 1024 {
                break;
            }
        }
        return Ok(
            json!({"engine":engine,"rows":rows,"partial":partial,"observedAt":now(),"state":"ready","error":error}),
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
        let r = execute(
            app,
            job,
            client,
            &[
                "container",
                "logs",
                "--timestamps",
                "--tail",
                &job.tail.to_string(),
                &job.container,
            ],
            256 * 1024,
        )?;
        let partial = r.response["error"]["code"] == "outputLimit";
        if !partial && (r.response["state"] != "completed" || r.response["exitCode"] != 0) {
            checked(r)?;
            unreachable!();
        }
        let mut logs = vec![];
        for (stream, bytes) in [("stdout", r.bytes), ("stderr", r.stderr)] {
            // shortcut: separate CLI streams have no strict global order; use the Engine API if ordering becomes required.
            for line in String::from_utf8_lossy(&bytes).lines().take(2000) {
                logs.push(json!({"stream":stream,"text":commands::text(line,4096)}));
            }
        }
        data["logs"] = json!(logs);
        data["sections"] = json!({});
        data["partial"] = json!(partial);
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
                            && v.terminal_ticket.is_none()
                            && !v.busy
                            && !v.frozen
                            && !v.paused
                            && v.due <= Instant::now()
                    })
                    .map(|(id, _)| id.clone())
                    .collect::<Vec<_>>();
                for id in due {
                    let tail = a.tail;
                    let _ = a.enqueue(&id, "refresh", tail);
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
                    let tail = a.tail;
                    let _ = a.enqueue(&id, "terminalStatus", tail);
                }
                a.jobs.pop_front()
            };
            if let Some(job) = job {
                if job.action.starts_with("terminal") {
                    let result = terminal_request(&app, &job, &client);
                    let mut a = app.lock().unwrap();
                    if let Some(v) = a
                        .views
                        .get_mut(&job.view)
                        .filter(|v| v.context == job.context)
                    {
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
                let mut result = collect(&app, &job, &client);
                if let Ok(data) = &mut result {
                    if !data["operation"].is_null() {
                        data["operation"]["containerId"] = json!(job.container);
                        data["operation"]["name"] = data["name"].clone();
                    }
                    let a = app.lock().unwrap();
                    if let Some(v) = a
                        .views
                        .get(&job.view)
                        .filter(|v| v.kind == "browser" && !job.container.is_empty())
                    {
                        let mut rows = v.data["rows"].as_array().cloned().unwrap_or_default();
                        if data["state"] == "gone" {
                            rows.retain(|row| row["id"] != job.container);
                        } else if let Some(row) =
                            rows.iter_mut().find(|row| row["id"] == job.container)
                        {
                            row["status"] = data["status"].clone();
                            row["health"] = data["health"].clone();
                        }
                        *data = json!({"engine":data["engine"],"rows":rows,"operation":data["operation"],
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
                        let _ = client.call("messages.publish", json!({"title":"Docker 容器操作未完成","body":format!("{} · {}：请在 Docker 侧栏或详情查看结果并刷新状态",commands::operation_name(&job.action),&job.container[..12]),"level":"warning","notify":false}));
                    }
                }
                let removed = job.action == "remove"
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
                                && v.container == job.container
                                && !v.busy
                        }) {
                            view.data["state"] = json!("gone");
                            view.revision += 1;
                        }
                    }
                    let used = a
                        .views
                        .values()
                        .map(|v| serde_json::to_vec(&v.data).unwrap().len())
                        .sum::<usize>();
                    if let Some(v) = a
                        .views
                        .get_mut(&job.view)
                        .filter(|v| v.context == job.context)
                    {
                        v.busy = false;
                        v.cancelled = false;
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
                        let name = v.data["name"].as_str().unwrap_or("容器");
                        Some(
                            json!({"viewId":job.view,"metadata":{"title":if v.kind=="browser"{"Docker".into()}else if v.kind=="logs"{format!("{name} · 日志")}else{name.to_string()},"dirty":false,"busy":false}}),
                        )
                    } else {
                        None
                    }
                };
                if let Some(metadata) = metadata {
                    let _ = client.call("ui.updateView", metadata);
                }
            } else {
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    });
}

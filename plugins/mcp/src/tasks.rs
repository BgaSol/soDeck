use crate::evidence;
use serde_json::{json, Value};
use sodeck_plugin_client::{Client, RpcError};
use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio_util::sync::CancellationToken;

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
pub fn error(message: impl Into<String>) -> Value {
    json!({"code":"requestFailed","message":message.into()})
}
fn rpc_error(e: RpcError) -> Value {
    json!({"code":e.code,"message":e.message,"details":e.data})
}
#[derive(Default)]
struct Admission {
    sequence: u64,
    file: Option<String>,
}
pub struct Target {
    pub id: String,
    pub view: String,
    pub context: Value,
    pub closed: AtomicBool,
    exec_admission: Mutex<Admission>,
    file_admission: Mutex<Admission>,
}
struct Task {
    id: String,
    target: Arc<Target>,
    input: Value,
    kind: &'static str,
    created_at: u64,
    directory: PathBuf,
    cancel: AtomicBool,
    state: Mutex<Record>,
}
struct Record {
    native: Value,
    evidence: Value,
    committed: u64,
    done: bool,
    released: bool,
    observed: u64,
    finished_at: Option<u64>,
    target: Value,
    cleanup_error: Value,
}
pub struct App {
    pub client: Client,
    pub root: PathBuf,
    pub run: String,
    pub token: String,
    pub port: u16,
    pub http_state: Mutex<String>,
    pub shutdown: CancellationToken,
    pub runtime: tokio::runtime::Handle,
    views: Mutex<HashMap<String, Arc<Target>>>,
    tasks: Mutex<HashMap<String, Arc<Task>>>,
    numbers: AtomicU64,
}
impl App {
    pub fn new(
        client: Client,
        root: PathBuf,
        token: String,
        port: u16,
        runtime: tokio::runtime::Handle,
    ) -> Self {
        Self {
            client,
            root,
            token,
            port,
            runtime,
            run: format!("{}-{:016x}", now(), rand::random::<u64>()),
            http_state: Mutex::new("starting".into()),
            shutdown: CancellationToken::new(),
            views: Mutex::new(HashMap::new()),
            tasks: Mutex::new(HashMap::new()),
            numbers: AtomicU64::new(1),
        }
    }
    pub fn mount(&self, view: &str, context: &Value) -> Result<(), String> {
        let mut views = self.views.lock().unwrap();
        if let Some(old) = views.get(view) {
            if old.context == *context {
                return Ok(());
            }
            old.closed.store(true, Ordering::Release);
        }
        if views.len() >= 64 {
            return Err("服务器目标已满".into());
        }
        if !context["workspaceId"].is_string()
            || !context["connectionId"].is_string()
            || !context["generation"].is_u64()
        {
            return Err("工作区上下文无效".into());
        }
        views.insert(
            view.into(),
            Arc::new(Target {
                id: format!(
                    "{}-target-{}",
                    self.run,
                    self.numbers.fetch_add(1, Ordering::Relaxed)
                ),
                view: view.into(),
                context: context.clone(),
                closed: AtomicBool::new(false),
                exec_admission: Mutex::new(Admission::default()),
                file_admission: Mutex::new(Admission::default()),
            }),
        );
        Ok(())
    }
    pub fn dispose(&self, view: &str) {
        if let Some(target) = self.views.lock().unwrap().remove(view) {
            target.closed.store(true, Ordering::Release);
        }
    }
    fn rpc(&self, method: &str, p: Value) -> Result<Value, Value> {
        self.client.call_detailed(method, p).map_err(rpc_error)
    }
    fn target_state(&self, target: &Target) -> Result<Value, Value> {
        if target.closed.load(Ordering::Acquire) {
            return Err(error("目标已关闭"));
        }
        let mut s = self.rpc("session.state", json!({"viewId":target.view}))?;
        if s["context"] != target.context {
            return Err(error("连接代次已改变，请重新查询目标"));
        }
        s["targetId"] = json!(target.id);
        Ok(s)
    }
    pub fn targets(&self) -> Value {
        let targets: Vec<_> = self.views.lock().unwrap().values().cloned().collect();
        json!({"targets":targets.iter().map(|t|self.target_state(t).unwrap_or_else(|e|json!({"targetId":t.id,"context":t.context,"state":"unavailable","error":e}))).collect::<Vec<_>>()})
    }
    pub fn start(self: &Arc<Self>, kind: &'static str, p: Value) -> Result<Value, Value> {
        let target_id = text(&p, "targetId", 256)?;
        let request = text(&p, "requestId", 128)?;
        let target = self
            .views
            .lock()
            .unwrap()
            .values()
            .find(|t| t.id == target_id)
            .cloned()
            .ok_or_else(|| error("目标不存在，请重新查询"))?;
        let input = if kind == "exec" {
            let command = text(&p, "command", 16 * 1024)?;
            if command.contains('\0') {
                return Err(error("命令包含空字符"));
            }
            let timeout = p
                .get("timeoutSeconds")
                .map(|v| v.as_u64().ok_or_else(|| error("期限必须为整数")))
                .transpose()?
                .unwrap_or(300);
            if !(1..=3600).contains(&timeout) {
                return Err(error("期限应为 1 至 3600 秒"));
            }
            json!({"targetId":target_id,"requestId":request,"command":command,"timeoutSeconds":timeout})
        } else {
            let path = text(&p, "remotePath", 4096)?;
            if path.contains('\0') {
                return Err(error("远程路径包含空字符"));
            }
            json!({"targetId":target_id,"requestId":request,"remotePath":path})
        };
        let key = format!("{target_id}/{request}");
        let mut tasks = self.tasks.lock().unwrap();
        if let Some(task) = tasks.get(&key) {
            if task.kind != kind || task.input != input {
                return Err(error("同一 requestId 参数不同"));
            }
            return Ok(json!({"taskId":task.id,"kind":task.kind}));
        }
        if target.closed.load(Ordering::Acquire) || self.shutdown.is_cancelled() {
            return Err(error("目标或插件正在关闭"));
        }
        if tasks.len() >= 4096
            || tasks
                .values()
                .filter(|t| !t.state.lock().unwrap().done)
                .count()
                >= 24
        {
            return Err(error("任务记录或活动任务已满"));
        }
        let id = format!("task-{}", self.numbers.fetch_add(1, Ordering::Relaxed));
        let directory = self.root.join("evidence").join(&self.run).join(&id);
        let task = Arc::new(Task {
            id: id.clone(),
            target,
            input,
            kind,
            created_at: now(),
            directory,
            cancel: AtomicBool::new(false),
            state: Mutex::new(Record {
                native: json!({"admission":"pending"}),
                evidence: json!({"state":"collecting","complete":false}),
                committed: 0,
                done: false,
                released: false,
                observed: now(),
                finished_at: None,
                target: Value::Null,
                cleanup_error: Value::Null,
            }),
        });
        tasks.insert(key, task.clone());
        drop(tasks);
        let app = self.clone();
        self.runtime.spawn_blocking(move || app.run_task(task));
        Ok(json!({"taskId":id,"kind":kind}))
    }
    pub fn get(&self, p: &Value) -> Result<Value, Value> {
        let task = self.task(p)?;
        self.get_task(task, p, false)
    }
    pub fn view_task(&self, view: &str, p: &Value) -> Result<Value, Value> {
        let task = self.task(p)?;
        let views = self.views.lock().unwrap();
        let target = views.get(view).ok_or_else(|| error("视图已关闭"))?;
        if target.id != task.target.id {
            return Err(error("任务不属于当前服务器连接"));
        }
        drop(views);
        let tail = p
            .get("tail")
            .map(|v| v.as_bool().ok_or_else(|| error("tail 必须为布尔值")))
            .transpose()?
            .unwrap_or(false);
        self.get_task(task, p, tail)
    }
    fn get_task(&self, task: Arc<Task>, p: &Value, tail: bool) -> Result<Value, Value> {
        let mut value = task.snapshot();
        if task.kind == "exec" {
            let offset = p
                .get("cursor")
                .map(|v| {
                    v.as_str()
                        .ok_or_else(|| error("cursor 必须为十进制字符串"))?
                        .parse::<u64>()
                        .map_err(|_| error("cursor 无效"))
                })
                .transpose()?
                .unwrap_or(0);
            let committed = task.state.lock().unwrap().committed;
            if task.directory.join("output.jsonl").exists() {
                let path = task.directory.join("output.jsonl");
                value["output"] = if tail {
                    evidence::tail(path, committed)
                } else {
                    evidence::page(path, committed, offset)
                }
                .map_err(error)?;
            }
        }
        Ok(value)
    }
    pub fn cancel(&self, p: &Value) -> Result<Value, Value> {
        let task = self.task(p)?;
        if !task.state.lock().unwrap().done {
            task.cancel.store(true, Ordering::Release);
        }
        Ok(json!({"taskId":task.id,"cancelRequested":true,"result":task.snapshot()}))
    }
    fn task(&self, p: &Value) -> Result<Arc<Task>, Value> {
        let id = text(p, "taskId", 256)?;
        self.tasks
            .lock()
            .unwrap()
            .values()
            .find(|t| t.id == id)
            .cloned()
            .ok_or_else(|| error("taskId 不属于本次运行"))
    }
    pub fn view(&self, view: &str) -> Result<Value, String> {
        let target = self
            .views
            .lock()
            .unwrap()
            .get(view)
            .cloned()
            .ok_or("视图已关闭")?;
        let tasks: Vec<_> = self
            .tasks
            .lock()
            .unwrap()
            .values()
            .filter(|t| t.target.id == target.id)
            .cloned()
            .collect();
        let mut tasks: Vec<_> = tasks
            .into_iter()
            .map(|t| {
                let number =
                    t.id.strip_prefix("task-")
                        .and_then(|n| n.parse::<u64>().ok())
                        .unwrap_or(0);
                let done = t.state.lock().unwrap().done;
                (done, std::cmp::Reverse(number), t)
            })
            .collect();
        tasks.sort_by_key(|(done, number, _)| (*done, *number));
        Ok(
            json!({"endpoint":format!("http://127.0.0.1:{}/mcp",self.port),"service":*self.http_state.lock().unwrap(),"target":self.target_state(&target).unwrap_or_else(|e|json!({"state":"unavailable","error":e})),"tasks":tasks.into_iter().take(50).map(|(_, _, t)|t.snapshot()).collect::<Vec<_>>(),"evidenceDirectory":self.root.join("evidence")}),
        )
    }
    pub fn config(&self) -> String {
        format!("[mcp_servers.sodeck]\nurl = \"http://127.0.0.1:{}/mcp\"\nhttp_headers = {{ Authorization = \"Bearer {}\" }}\n",self.port,self.token)
    }
    fn stopped(&self, task: &Task) -> bool {
        task.cancel.load(Ordering::Acquire)
            || task.target.closed.load(Ordering::Acquire)
            || self.shutdown.is_cancelled()
            || self.client.is_closed()
    }
    fn run_task(self: Arc<Self>, task: Arc<Task>) {
        let result = self.collect(&task);
        if let Err(e) = result {
            let mut record = task.state.lock().unwrap();
            if record.native["admission"] == "pending" {
                record.native = json!({"admission":"rejected","execSent":false});
            }
            record.evidence = json!({"state":"failed","complete":false,"error":e});
        }
        {
            let mut r = task.state.lock().unwrap();
            r.done = true;
            r.finished_at = Some(now());
        }
        if let Err(e) = evidence::metadata(&task.directory, &task.metadata()) {
            let mut r = task.state.lock().unwrap();
            r.evidence = json!({"state":"failed","complete":false,"error":e});
        }
    }
    fn collect(&self, task: &Task) -> Result<(), Value> {
        fs::create_dir_all(&task.directory).map_err(|e| error(e.to_string()))?;
        evidence::metadata(&task.directory, &task.metadata()).map_err(error)?;
        let mut output = if task.kind == "exec" {
            Some(evidence::Output::new(&task.directory).map_err(error)?)
        } else {
            None
        };
        let mut admission = if task.kind == "exec" {
            &task.target.exec_admission
        } else {
            &task.target.file_admission
        }
        .lock()
        .unwrap();
        if self.stopped(task) {
            task.observe(json!({"admission":"cancelled","execSent":false}));
            return Err(error("接纳前已取消"));
        }
        let target = self.target_state(&task.target)?;
        if target["state"] != "connected" || target["closing"] == true {
            return Err(error("服务器未连接或正在关闭"));
        }
        task.state.lock().unwrap().target = target["target"].clone();
        evidence::metadata(&task.directory, &task.metadata()).map_err(error)?;
        admission.sequence += 1;
        let mut request = json!({"viewId":task.target.view,"expectedContext":task.target.context,"operationId":task.id,"sequence":admission.sequence.to_string()});
        let file = if task.kind == "download" {
            if admission.file.is_none() {
                let opened = self.rpc(
                    "files.request",
                    json!({"action":"open","viewId":task.target.view}),
                )?;
                admission.file = Some(text(&opened, "id", 256)?.into());
            }
            let file = admission.file.clone().unwrap();
            if let Err(e)=self.rpc("files.request",json!({"id":file,"action":"batch.begin","protocolVersion":2,"batchId":task.id,"context":task.target.context})) {
                self.end_batch(task,&file);
                return Err(e);
            }
            request = json!({"id":file,"action":"start","protocolVersion":2,"batchId":task.id,"requestId":task.id,"expectedContext":task.target.context,"sequence":admission.sequence.to_string(),"operation":{"kind":"copyFile","source":{"side":"remote","path":task.input["remotePath"]},"destination":{"side":"pluginData","relative":["evidence",self.run,task.id,"download.bin"]},"destinationGuard":{"kind":"absent"}}});
            Some(file)
        } else {
            request["command"] = task.input["command"].clone();
            request["timeoutMs"] = json!(task.input["timeoutSeconds"].as_u64().unwrap() * 1000);
            request["maxOutputBytes"] = json!(16 * 1024 * 1024);
            request["retention"] = json!("fixed");
            None
        };
        if self.stopped(task) {
            task.observe(json!({"admission":"cancelled","execSent":false}));
            if let Some(file) = &file {
                self.end_batch(task, file);
            }
            return Err(error("接纳前已取消"));
        }
        let method = if file.is_some() {
            "files.operations"
        } else {
            "session.stream.open"
        };
        let accepted = match self.rpc(method, request.clone()) {
            Ok(value) => value,
            Err(e) if e["code"] == -32003 => {
                task.observe(json!({"admission":"unknown","error":e}));
                loop {
                    if self.shutdown.is_cancelled()
                        || self.client.is_closed()
                        || task.target.closed.load(Ordering::Acquire)
                    {
                        return Err(error("核心通信结束，接纳结果未知"));
                    }
                    let lookup = if let Some(file) = &file {
                        self.rpc("files.operations",json!({"id":file,"action":"lookup","protocolVersion":2,"batchId":task.id,"requestId":task.id}))
                    } else {
                        self.rpc("session.stream.lookup", request.clone())
                    };
                    if let Ok(value) = lookup {
                        if value["streamId"].is_string() || value["id"].is_string() {
                            break value;
                        }
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
            Err(e) => {
                task.observe(json!({"admission":"rejected","execSent":false,"error":e}));
                if let Some(file) = &file {
                    self.end_batch(task, file);
                }
                return Err(e);
            }
        };
        task.observe(accepted.clone());
        let native_id = text(
            &accepted,
            if file.is_some() { "id" } else { "streamId" },
            256,
        )
        .map_err(|e| {
            task.observe(json!({"admission":"unknown","response":accepted,"error":e}));
            e
        })?
        .to_string();
        drop(admission);
        let result = self.poll_task(task, &native_id, &file, &mut output);
        if result.is_err() {
            self.cancel_native(&native_id, &file);
            // The owner also cancels resources on view/plugin disposal. Wait only for bounded local cleanup.
            let end = std::time::Instant::now() + Duration::from_secs(5);
            while !self.client.is_closed()
                && !self.shutdown.is_cancelled()
                && std::time::Instant::now() < end
            {
                let polled = if let Some(file) = &file {
                    self.rpc(
                        "files.operations",
                        json!({"id":file,"action":"poll","operationId":native_id}),
                    )
                } else {
                    self.rpc("session.stream.status", json!({"streamId":native_id}))
                };
                if let Ok(native) = polled {
                    if if file.is_some() {
                        ["completed", "failed", "cancelled", "unknown"]
                            .contains(&native["state"].as_str().unwrap_or(""))
                    } else {
                        native["workerFinished"] == true
                    } {
                        self.release_native(task, &native_id, &file);
                        break;
                    }
                } else {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
        result
    }
    fn end_batch(&self, task: &Task, file: &str) {
        if let Err(e) = self.rpc(
            "files.request",
            json!({"id":file,"action":"batch.end","batchId":task.id}),
        ) {
            task.state.lock().unwrap().cleanup_error = e;
        }
    }
    fn cancel_native(&self, id: &str, file: &Option<String>) -> bool {
        if let Some(file) = file {
            self.rpc(
                "files.operations",
                json!({"id":file,"action":"cancel","operationId":id}),
            )
        } else {
            self.rpc("session.stream.close", json!({"streamId":id}))
        }
        .is_ok()
    }
    fn release_native(&self, task: &Task, id: &str, file: &Option<String>) {
        let result = if let Some(file) = file {
            let result = self.rpc(
                "files.operations",
                json!({"id":file,"action":"release","operationId":id}),
            );
            self.end_batch(task, file);
            result
        } else {
            self.rpc("session.stream.release", json!({"streamId":id}))
        };
        let mut r = task.state.lock().unwrap();
        r.released = result.is_ok();
        if let Err(e) = result {
            r.cleanup_error = e;
        }
    }
    fn poll_task(
        &self,
        task: &Task,
        native_id: &str,
        file: &Option<String>,
        output: &mut Option<evidence::Output>,
    ) -> Result<(), Value> {
        let mut offset = 0u64;
        let mut cancelled = false;
        let mut save_error = None;
        loop {
            if self.client.is_closed() || self.shutdown.is_cancelled() {
                return Err(error("通信结束，最终结果未知"));
            }
            if (self.stopped(task) || save_error.is_some()) && !cancelled {
                cancelled = self.cancel_native(native_id, file);
            }
            let polled = if let Some(file) = &file {
                self.rpc(
                    "files.operations",
                    json!({"id":file,"action":"poll","operationId":native_id}),
                )
            } else {
                self.rpc(
                    "session.stream.poll",
                    json!({"streamId":native_id,"offset":offset.to_string()}),
                )
            };
            let polled = match polled {
                Ok(v) => v,
                Err(e) => {
                    let previous = task.state.lock().unwrap().native.clone();
                    task.observe(json!({"state":"unavailable","lastResult":previous,"error":e}));
                    return Err(error("原生任务查询不可用"));
                }
            };
            let native = if file.is_some() {
                polled.clone()
            } else {
                polled["status"].clone()
            };
            task.observe(native.clone());
            if let Some(writer) = output.as_mut() {
                if save_error.is_none() {
                    let saved = (|| {
                        let chunks = polled["chunks"]
                            .as_array()
                            .ok_or_else(|| error("核心输出页无效"))?;
                        let next: u64 = text(&polled, "nextOffset", 32)?
                            .parse()
                            .map_err(|_| error("核心偏移无效"))?;
                        if next < offset {
                            return Err(error("核心偏移倒退"));
                        }
                        writer.append(chunks).map_err(error)?;
                        offset = next;
                        Ok(())
                    })();
                    task.state.lock().unwrap().committed = writer.committed;
                    if let Err(e) = saved {
                        save_error = Some(e);
                    }
                }
            }
            let finished = if file.is_some() {
                ["completed", "failed", "cancelled", "unknown"]
                    .contains(&native["state"].as_str().unwrap_or(""))
            } else {
                native["workerFinished"] == true
                    && (save_error.is_some() || polled["nextOffset"] == polled["watermark"])
            };
            if finished {
                if let Some(writer) = output.as_ref() {
                    if let Err(e) = writer.sync() {
                        save_error = Some(error(e));
                    }
                }
                let complete = save_error.is_none()
                    && if file.is_some() {
                        native["state"] == "completed"
                    } else {
                        native["outputComplete"] == true
                    };
                {
                    let mut r = task.state.lock().unwrap();
                    r.evidence = json!({"state":if complete{"saved"}else{"incomplete"},"complete":complete,"error":save_error,"path":task.directory.join(if file.is_some(){"download.bin"}else{"output.jsonl"})});
                }
                let metadata_result =
                    evidence::metadata(&task.directory, &task.metadata()).map_err(error);
                self.release_native(task, native_id, file);
                metadata_result?;
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}
impl Task {
    fn observe(&self, value: Value) {
        let mut r = self.state.lock().unwrap();
        r.native = value;
        r.observed = now();
    }
    fn snapshot(&self) -> Value {
        let r = self.state.lock().unwrap();
        json!({"taskId":self.id,"kind":self.kind,"targetId":self.target.id,"requestId":self.input["requestId"],"input":self.input,"target":r.target,"createdAt":self.created_at,"finishedAt":r.finished_at,"native":r.native,"observedAt":r.observed,"resourceReleased":r.released,"cleanupError":r.cleanup_error,"done":r.done,"evidence":r.evidence,"directory":self.directory})
    }
    fn metadata(&self) -> Value {
        let mut value = self.snapshot();
        value["schemaVersion"] = json!(1);
        value["context"] = self.target.context.clone();
        value
    }
}
pub fn text<'a>(p: &'a Value, key: &str, max: usize) -> Result<&'a str, Value> {
    p[key]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= max)
        .ok_or_else(|| error(format!("{key} 缺失或超限")))
}

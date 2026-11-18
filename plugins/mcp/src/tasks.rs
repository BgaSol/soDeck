use crate::{
    collector, query,
    task::{Kind, NewTask, Task},
};
use serde_json::{json, Value};
use sodeck_plugin_client::{Client, RpcError};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
pub fn error(message: impl Into<String>) -> Value {
    failure("requestFailed", message)
}
pub fn failure(code: &str, message: impl Into<String>) -> Value {
    json!({"code":code,"message":message.into()})
}
pub fn text<'a>(p: &'a Value, key: &str, max: usize) -> Result<&'a str, Value> {
    p[key]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= max)
        .ok_or_else(|| failure("invalidParams", format!("{key} 缺失或超限")))
}
#[derive(Default)]
pub struct Admission {
    pub sequence: u64,
    pub file: Option<String>,
    pub busy: bool,
}
pub struct Target {
    pub id: String,
    pub view: String,
    pub context: Value,
    pub closed: AtomicBool,
    exec: Mutex<Admission>,
    file: Mutex<Admission>,
}
impl Target {
    pub fn new(id: String, view: String, context: Value) -> Self {
        Self {
            id,
            view,
            context,
            closed: AtomicBool::new(false),
            exec: Mutex::new(Admission::default()),
            file: Mutex::new(Admission::default()),
        }
    }
    pub fn admission(&self, kind: Kind) -> &Mutex<Admission> {
        if kind == Kind::Exec {
            &self.exec
        } else {
            &self.file
        }
    }
    pub fn release_admission(&self, kind: Kind) {
        self.admission(kind).lock().unwrap().busy = false;
    }
}
#[derive(Default)]
struct Registry {
    tasks: HashMap<String, Arc<Task>>,
    requests: HashMap<(String, String), String>,
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
    pub waiting: Arc<Semaphore>,
    pub io: Arc<Semaphore>,
    active: Arc<Semaphore>,
    views: Mutex<HashMap<String, Arc<Target>>>,
    registry: Mutex<Registry>,
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
            waiting: Arc::new(Semaphore::new(16)),
            io: Arc::new(Semaphore::new(4)),
            active: Arc::new(Semaphore::new(24)),
            views: Mutex::new(HashMap::new()),
            registry: Mutex::new(Registry::default()),
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
        if views.len() >= 64 && !views.contains_key(view) {
            return Err("服务器目标已满".into());
        }
        if !context["workspaceId"].is_string()
            || !context["serverId"].is_string()
            || !context["connectionId"].is_string()
            || !context["generation"].is_u64()
        {
            return Err("工作区上下文无效".into());
        }
        let id = format!(
            "{}-target-{}",
            self.run,
            self.numbers.fetch_add(1, Ordering::Relaxed)
        );
        views.insert(
            view.into(),
            Arc::new(Target::new(id, view.into(), context.clone())),
        );
        Ok(())
    }
    pub fn dispose(&self, view: &str) {
        if let Some(target) = self.views.lock().unwrap().remove(view) {
            target.closed.store(true, Ordering::Release);
        }
    }
    pub fn rpc(&self, method: &str, p: Value) -> Result<Value, Value> {
        self.client
            .call_detailed(method, p)
            .map_err(|e: RpcError| json!({"code":e.code,"message":e.message,"details":e.data}))
    }
    pub fn context_view(&self, context: &Value) -> Result<String, String> {
        self.views
            .lock()
            .unwrap()
            .values()
            .find(|t| t.context == *context && !t.closed.load(Ordering::Acquire))
            .map(|t| t.view.clone())
            .ok_or_else(|| "服务器后台目标不可用，请重新连接服务器".into())
    }
    pub fn view_target(&self, view: &str) -> Result<Arc<Target>, Value> {
        self.views
            .lock()
            .unwrap()
            .get(view)
            .cloned()
            .ok_or_else(|| failure("staleContext", "视图已关闭"))
    }
    pub fn target_state(&self, target: &Target) -> Result<Value, Value> {
        if target.closed.load(Ordering::Acquire) {
            return Err(failure("staleContext", "目标已关闭"));
        }
        let mut s = self.rpc("session.state", json!({"viewId":target.view}))?;
        if s["context"] != target.context {
            return Err(failure("staleContext", "连接代次已改变，请重新查询目标"));
        }
        s["targetId"] = json!(target.id);
        Ok(s)
    }
    pub fn targets(&self) -> Value {
        let targets: Vec<_> = self.views.lock().unwrap().values().cloned().collect();
        json!({"runId":self.run,"targets":targets.iter().map(|t|self.target_state(t).unwrap_or_else(|e|json!({"targetId":t.id,"context":t.context,"state":"unavailable","error":e}))).collect::<Vec<_>>()})
    }
    pub fn start(self: &Arc<Self>, kind: Kind, p: Value) -> Result<Value, Value> {
        let target_id = text(&p, "targetId", 256)?.to_owned();
        let request = text(&p, "requestId", 128)?.to_owned();
        let input = if kind == Kind::Exec {
            let command = text(&p, "command", 16 * 1024)?;
            if command.contains('\0') {
                return Err(failure("invalidParams", "命令包含空字符"));
            }
            let timeout = p
                .get("timeoutSeconds")
                .map(|v| {
                    v.as_u64()
                        .ok_or_else(|| failure("invalidParams", "期限必须为整数"))
                })
                .transpose()?
                .unwrap_or(300);
            if !(1..=3600).contains(&timeout) {
                return Err(failure("invalidParams", "期限应为 1 至 3600 秒"));
            }
            json!({"targetId":target_id,"requestId":request,"command":command,"timeoutSeconds":timeout})
        } else {
            let path = text(&p, "remotePath", 4096)?;
            if path.contains('\0') {
                return Err(failure("invalidParams", "远程路径包含空字符"));
            }
            json!({"targetId":target_id,"requestId":request,"remotePath":path})
        };
        let key = (target_id.clone(), request);
        let mut registry = self.registry.lock().unwrap();
        if let Some(id) = registry.requests.get(&key) {
            let task = &registry.tasks[id];
            if task.kind != kind || task.input != input {
                return Err(failure("requestConflict", "同一 requestId 参数不同"));
            }
            return Ok(json!({"runId":self.run,"taskId":task.id,"kind":task.kind.name()}));
        }
        let views = self.views.lock().unwrap();
        let target = views
            .values()
            .find(|t| t.id == target_id)
            .cloned()
            .ok_or_else(|| failure("staleContext", "目标不存在，请重新查询"))?;
        if target.closed.load(Ordering::Acquire) || self.shutdown.is_cancelled() {
            return Err(failure("staleContext", "目标或插件正在关闭"));
        }
        if registry.tasks.len() >= 4096 {
            return Err(failure("resourceBusy", "任务记录已满"));
        }
        let permit = self
            .active
            .clone()
            .try_acquire_owned()
            .map_err(|_| failure("resourceBusy", "活动任务已满"))?;
        let mut admission = target.admission(kind).lock().unwrap();
        if admission.busy {
            return Err(failure(
                "resourceBusy",
                "该目标接纳处理中或结果未知，请查询原任务",
            ));
        }
        admission.busy = true;
        admission.sequence += 1;
        let order = self.numbers.fetch_add(1, Ordering::Relaxed);
        let task = Arc::new(Task::new(NewTask {
            run: self.run.clone(),
            order,
            admission_sequence: admission.sequence,
            target: target.clone(),
            input,
            kind,
            root: self.root.clone(),
        }));
        registry.requests.insert(key, task.id.clone());
        registry.tasks.insert(task.id.clone(), task.clone());
        drop(admission);
        drop(views);
        drop(registry);
        let result = json!({"runId":self.run,"taskId":task.id,"kind":kind.name()});
        let app = self.clone();
        self.runtime.spawn_blocking(move || {
            collector::run(&app, &task);
            drop(permit);
        });
        Ok(result)
    }
    pub fn resolve(&self, p: &Value, scope: Option<&str>) -> Result<Arc<Task>, Value> {
        let id = text(p, "taskId", 256)?;
        let task = self
            .registry
            .lock()
            .unwrap()
            .tasks
            .get(id)
            .cloned()
            .ok_or_else(|| failure("taskNotFound", "taskId 不属于本次运行"))?;
        if scope.is_some_and(|target| target != task.target.id) {
            return Err(failure("staleContext", "任务不属于当前服务器连接"));
        }
        Ok(task)
    }
    pub fn records(&self) -> Vec<Arc<Task>> {
        self.registry
            .lock()
            .unwrap()
            .tasks
            .values()
            .cloned()
            .collect()
    }
    pub fn cancel(&self, p: &Value, scope: Option<&str>) -> Result<Value, Value> {
        let task = self.resolve(p, scope)?;
        let snapshot = task.request_cancel();
        let result = if scope.is_some() {
            task.summary()
        } else {
            snapshot.value
        };
        Ok(json!({"taskId":task.id,"cancelRequested":result["cancelRequested"],"result":result}))
    }
    pub fn view_task(&self, view: &str, p: &Value) -> Result<Value, Value> {
        let target = self.view_target(view)?;
        let task = self.resolve(p, Some(&target.id))?;
        let options = query::Options::parse(p, true)?;
        let value = query::prepare(self, task.snapshot(), task, options)?.read()?;
        if serde_json::to_vec(&value).unwrap().len() > 56 * 1024 {
            return Err(failure(
                "resourceLimit",
                "详情超过界面传输大小，请使用本地证据或 MCP 读取",
            ));
        }
        Ok(value)
    }
    pub fn view(&self, view: &str) -> Result<Value, String> {
        let target = self.view_target(view).map_err(|e| e.to_string())?;
        let mut tasks: Vec<_> = self
            .records()
            .into_iter()
            .filter(|t| t.target.id == target.id)
            .map(|t| (t.order, t.summary()))
            .collect();
        tasks.sort_by_key(|(order, v)| (v["done"] == true, std::cmp::Reverse(*order)));
        let mut rows = Vec::new();
        let mut bytes = 0;
        let count = tasks.len();
        for (_, row) in tasks.into_iter().take(50) {
            let size = serde_json::to_vec(&row).unwrap().len();
            if bytes + size > 40 * 1024 {
                break;
            }
            bytes += size;
            rows.push(row);
        }
        Ok(
            json!({"runId":self.run,"endpoint":format!("http://127.0.0.1:{}/mcp",self.port),"service":*self.http_state.lock().unwrap(),
            "target":self.target_state(&target).unwrap_or_else(|e|json!({"state":"unavailable","error":e})),"tasks":rows,
            "recordCount":count,"evidenceDirectory":self.root.join("evidence")}),
        )
    }
    pub fn config(&self) -> String {
        format!("[mcp_servers.sodeck]\nurl = \"http://127.0.0.1:{}/mcp\"\nhttp_headers = {{ Authorization = \"Bearer {}\" }}\n",self.port,self.token)
    }
    pub fn stopped(&self, task: &Task) -> bool {
        task.cancel.is_cancelled() || self.closing(task)
    }
    pub fn closing(&self, task: &Task) -> bool {
        task.target.closed.load(Ordering::Acquire)
            || self.shutdown.is_cancelled()
            || self.client.is_closed()
    }
}

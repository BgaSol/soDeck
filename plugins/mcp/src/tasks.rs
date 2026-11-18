use crate::{
    collector, query,
    query::{TaskFailure, WaitSlots},
    response,
    targets::Target,
    task::{CapturedTask, Kind, NewTask, Phase, Task},
};
use serde_json::{json, Value};
use sodeck_plugin_client::{Client, RpcError};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn waiting_is_validated_and_never_part_of_execution_identity() {
        let input = json!({"targetId":"target","requestId":"request","command":"true"});
        let default = parse_exec_start(&input).unwrap();
        assert_eq!(default.wait_ms, 1000);
        for wait in [0, 1000, 5000, 20000] {
            let mut p = input.clone();
            p["waitMs"] = json!(wait);
            p["timeoutSeconds"] = json!(300);
            let parsed = parse_exec_start(&p).unwrap();
            assert_eq!(parsed.input, default.input);
            assert_eq!(parsed.wait_ms, wait);
        }
        for value in [
            json!(-1),
            json!(0.5),
            json!(20001),
            json!("1000"),
            Value::Null,
            json!(true),
        ] {
            let mut p = input.clone();
            p["waitMs"] = value;
            assert!(parse_exec_start(&p).is_err());
        }
        let download = json!({"targetId":"target","requestId":"request","remotePath":"/tmp/file"});
        assert_eq!(parse_download_start(&download).unwrap().wait_ms, 0);
        let mut download = download;
        download["waitMs"] = json!(0);
        assert!(parse_download_start(&download).is_err());
    }
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
pub struct StartRequest {
    pub kind: Kind,
    pub input: Value,
    pub wait_ms: u64,
}
pub struct StartedTask {
    pub task: Arc<Task>,
    pub wait_ms: u64,
    pub started: Instant,
    pub wait_slots: Option<WaitSlots>,
}
pub fn parse_exec_start(p: &Value) -> Result<StartRequest, Value> {
    let command = text(p, "command", 16 * 1024)?;
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
    let wait_ms = p
        .get("waitMs")
        .map(|v| {
            v.as_u64()
                .ok_or_else(|| failure("invalidParams", "waitMs 必须为整数"))
        })
        .transpose()?
        .unwrap_or(1000);
    if wait_ms > 20000 {
        return Err(failure("invalidParams", "waitMs 超出范围"));
    }
    Ok(StartRequest {
        kind: Kind::Exec,
        input: json!({"targetId":text(p,"targetId",256)?,"requestId":text(p,"requestId",128)?,"command":command,"timeoutSeconds":timeout}),
        wait_ms,
    })
}
pub fn parse_download_start(p: &Value) -> Result<StartRequest, Value> {
    let path = text(p, "remotePath", 4096)?;
    if path.contains('\0') || p.get("waitMs").is_some() {
        return Err(failure(
            "invalidParams",
            "远程路径包含空字符或下载等待参数无效",
        ));
    }
    Ok(StartRequest {
        kind: Kind::Download,
        input: json!({"targetId":text(p,"targetId",256)?,"requestId":text(p,"requestId",128)?,"remotePath":path}),
        wait_ms: 0,
    })
}
impl StartedTask {
    fn existing(app: &App, task: Arc<Task>, wait_ms: u64) -> Result<Self, TaskFailure> {
        let started = Instant::now();
        let wait_slots = if wait_ms == 0 || task.snapshot().phase == Phase::Done {
            None
        } else {
            match WaitSlots::acquire(&app.waiting, &task) {
                Ok(slots) => Some(slots),
                Err(error) => {
                    let captured = CapturedTask::new(task.clone());
                    if captured.record.phase != Phase::Done {
                        return Err(TaskFailure {
                            error,
                            captured: Some(captured),
                        });
                    }
                    None
                }
            }
        };
        Ok(Self {
            task,
            wait_ms,
            started,
            wait_slots,
        })
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
    pub(crate) views: Mutex<HashMap<String, Arc<Target>>>,
    registry: Mutex<Registry>,
    pub(crate) numbers: AtomicU64,
    pub(crate) target_reads: Arc<Semaphore>,
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
            target_reads: Arc::new(Semaphore::new(4)),
        }
    }
    pub fn rpc(&self, method: &str, p: Value) -> Result<Value, Value> {
        self.client
            .call_detailed(method, p)
            .map_err(|e: RpcError| json!({"code":e.code,"message":e.message,"details":e.data}))
    }
    pub fn start(
        self: &Arc<Self>,
        request: StartRequest,
        ct: &CancellationToken,
    ) -> Result<StartedTask, TaskFailure> {
        let StartRequest {
            kind,
            input,
            wait_ms,
        } = request;
        let target_id = input["targetId"].as_str().unwrap().to_owned();
        let request = input["requestId"].as_str().unwrap().to_owned();
        if ct.is_cancelled() {
            return Err(failure("requestCancelled", "启动请求已取消，未接纳新任务").into());
        }
        let key = (target_id.clone(), request);
        let mut registry = self.registry.lock().unwrap();
        if let Some(id) = registry.requests.get(&key) {
            let task = &registry.tasks[id];
            if task.kind != kind || task.input != input {
                return Err(failure("requestConflict", "同一 requestId 参数不同").into());
            }
            let task = task.clone();
            drop(registry);
            return StartedTask::existing(self, task, wait_ms);
        }
        let views = self.views.lock().unwrap();
        let target = views
            .values()
            .find(|t| t.id == target_id)
            .cloned()
            .ok_or_else(|| failure("staleContext", "目标不存在，请重新查询"))?;
        if target.closed.load(Ordering::Acquire) || self.shutdown.is_cancelled() {
            return Err(failure("staleContext", "目标或插件正在关闭").into());
        }
        if registry.tasks.len() >= 4096 {
            return Err(failure("resourceBusy", "任务记录已满").into());
        }
        let permit = self
            .active
            .clone()
            .try_acquire_owned()
            .map_err(|_| failure("resourceBusy", "活动任务已满"))?;
        let mut admission = target.admission(kind).lock().unwrap();
        if admission.busy {
            return Err(failure("resourceBusy", "该目标接纳处理中或结果未知，请查询原任务").into());
        }
        let global = if wait_ms > 0 {
            Some(
                self.waiting
                    .clone()
                    .try_acquire_owned()
                    .map_err(|_| failure("resourceBusy", "读取等待已满，未接纳新任务"))?,
            )
        } else {
            None
        };
        let order = self.numbers.fetch_add(1, Ordering::Relaxed);
        let task = Arc::new(Task::new(NewTask {
            run: self.run.clone(),
            order,
            admission_sequence: admission.sequence + 1,
            target: target.clone(),
            input,
            kind,
            root: self.root.clone(),
        }));
        let wait_slots = global.map(|global| {
            let local = task
                .waiters
                .clone()
                .try_acquire_owned()
                .expect("unregistered task has waiting slots");
            WaitSlots {
                _global: global,
                _local: local,
            }
        });
        if ct.is_cancelled() {
            return Err(failure("requestCancelled", "启动请求已取消，未接纳新任务").into());
        }
        if target.closed.load(Ordering::Acquire) || self.shutdown.is_cancelled() {
            return Err(failure("staleContext", "目标或插件正在关闭").into());
        }
        admission.busy = true;
        admission.sequence += 1;
        registry.requests.insert(key, task.id.clone());
        registry.tasks.insert(task.id.clone(), task.clone());
        let started = Instant::now();
        drop(admission);
        drop(views);
        drop(registry);
        let result = StartedTask {
            task: task.clone(),
            wait_ms,
            started,
            wait_slots,
        };
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
    pub fn cancel(&self, p: &Value, scope: Option<&str>) -> Result<CapturedTask, Value> {
        let task = self.resolve(p, scope)?;
        let record = task.request_cancel();
        Ok(CapturedTask { task, record })
    }
    pub fn view_task(&self, view: &str, p: &Value) -> Result<Value, Value> {
        let target = self.view_target(view)?;
        let task = self.resolve(p, Some(&target.id))?;
        let options = query::parse_ui_read(p)?;
        let read = query::ui_read(self, task, options).map_err(response::ui_read_error)?;
        let value = response::ui_task(&read);
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
            .map(|t| (t.order, response::ui_row(&CapturedTask::new(t))))
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

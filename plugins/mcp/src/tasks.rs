use crate::app::App;
use crate::{
    collector,
    task::{
        CapturedTask, ExecMode, ExecSpec, FileSpec, NewTask, Phase, Task, TaskFailure,
        TaskOperation, TaskSpec,
    },
};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{atomic::Ordering, Arc},
    time::Instant,
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;

pub struct WaitSlots {
    pub _global: OwnedSemaphorePermit,
    pub _local: OwnedSemaphorePermit,
}
impl WaitSlots {
    pub fn acquire(waiting: &Arc<Semaphore>, task: &Task) -> Result<Self, Value> {
        let global = waiting
            .clone()
            .try_acquire_owned()
            .map_err(|_| failure("resourceBusy", "读取等待已满"))?;
        let local = task
            .waiters
            .clone()
            .try_acquire_owned()
            .map_err(|_| failure("resourceBusy", "该任务读取等待已满"))?;
        Ok(Self {
            _global: global,
            _local: local,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn uploads_keep_fixed_parameters_and_do_not_depend_on_source_existence() {
        let p = json!({"targetId":"target","requestId":"upload","localPath":"/not-yet-present","remotePath":"/tmp/中文 $ file"});
        let first = parse_upload_start(&p).unwrap();
        let mut retry = p.clone();
        retry["overwrite"] = json!(false);
        assert_eq!(first.spec, parse_upload_start(&retry).unwrap().spec);
        for (field, value) in [
            ("requestId", json!("bad\0id")),
            ("overwrite", Value::Null),
            ("overwrite", json!("true")),
            ("remotePath", json!("relative")),
            ("remotePath", json!("/tmp/")),
            ("localPath", json!("bad\0path")),
            ("waitMs", json!(0)),
        ] {
            let mut bad = p.clone();
            bad[field] = value;
            assert!(parse_upload_start(&bad).is_err(), "{bad}");
        }
    }
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
            assert_eq!(parsed.spec, default.spec);
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
        .filter(|s| !s.is_empty() && s.len() <= max && !s.contains('\0'))
        .ok_or_else(|| failure("invalidParams", format!("{key} 缺失或超限")))
}
pub struct StartRequest {
    pub spec: TaskSpec,
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
    let mode = match p
        .get("mode")
        .map(|v| v.as_str().unwrap_or(""))
        .unwrap_or("batch")
    {
        "batch" => ExecMode::Batch,
        "pipe" => ExecMode::Pipe,
        "pty" => ExecMode::Pty,
        _ => return Err(failure("invalidParams", "mode 无效")),
    };
    let timeout = match p.get("timeoutSeconds") {
        None => {
            if mode == ExecMode::Batch {
                Some(300)
            } else {
                None
            }
        }
        Some(Value::Null) if mode != ExecMode::Batch => None,
        Some(value) => {
            let n = value
                .as_u64()
                .filter(|n| (1..=3600).contains(n))
                .ok_or_else(|| failure("invalidParams", "期限应为 1 至 3600 秒"))?;
            Some(n)
        }
    };
    let terminal_size = if mode == ExecMode::Pty {
        let cols = p
            .get("cols")
            .map(|n| n.as_u64().unwrap_or(0))
            .unwrap_or(120);
        let rows = p.get("rows").map(|n| n.as_u64().unwrap_or(0)).unwrap_or(30);
        if !(2..=1000).contains(&cols) || !(1..=500).contains(&rows) {
            return Err(failure("invalidParams", "终端尺寸超出范围"));
        }
        Some((cols as u32, rows as u32))
    } else {
        if p.get("cols").is_some() || p.get("rows").is_some() {
            return Err(failure("invalidParams", "尺寸只用于 pty"));
        }
        None
    };
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
        spec: TaskSpec {
            target_id: text(p, "targetId", 256)?.into(),
            request_id: text(p, "requestId", 128)?.into(),
            operation: TaskOperation::Exec(ExecSpec {
                command: command.into(),
                mode,
                timeout_seconds: timeout,
                terminal_size,
            }),
        },
        wait_ms,
    })
}
pub fn parse_download_start(p: &Value) -> Result<StartRequest, Value> {
    let path = text(p, "remotePath", 4096)?;
    if path.contains('\0') || p.get("waitMs").is_some() {
        return Err(failure("invalidParams", "远程路径或下载等待参数无效"));
    }
    Ok(StartRequest {
        spec: TaskSpec {
            target_id: text(p, "targetId", 256)?.into(),
            request_id: text(p, "requestId", 128)?.into(),
            operation: TaskOperation::File(FileSpec::Download {
                remote_path: path.into(),
            }),
        },
        wait_ms: 0,
    })
}

pub fn parse_upload_start(p: &Value) -> Result<StartRequest, Value> {
    let local = text(p, "localPath", 4096)?;
    let remote = text(p, "remotePath", 4096)?;
    if local.contains('\0')
        || remote.contains('\0')
        || !remote.starts_with('/')
        || remote.ends_with('/')
        || remote
            .rsplit('/')
            .next()
            .is_none_or(|n| n == "." || n == "..")
        || p.as_object().is_none_or(|v| {
            v.keys().any(|k| {
                ![
                    "targetId",
                    "requestId",
                    "localPath",
                    "remotePath",
                    "overwrite",
                ]
                .contains(&k.as_str())
            })
        })
    {
        return Err(failure("invalidParams", "上传路径或参数无效"));
    }
    let overwrite = match p.get("overwrite") {
        None => false,
        Some(Value::Bool(v)) => *v,
        _ => return Err(failure("invalidParams", "overwrite 必须是布尔值")),
    };
    Ok(StartRequest {
        spec: TaskSpec {
            target_id: text(p, "targetId", 256)?.into(),
            request_id: text(p, "requestId", 128)?.into(),
            operation: TaskOperation::File(FileSpec::Upload {
                local_path: local.into(),
                remote_path: remote.into(),
                overwrite,
            }),
        },
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
pub(crate) struct Registry {
    tasks: HashMap<String, Arc<Task>>,
    requests: HashMap<(String, String), String>,
}
impl App {
    pub fn start(
        self: &Arc<Self>,
        request: StartRequest,
        ct: &CancellationToken,
    ) -> Result<StartedTask, TaskFailure> {
        let StartRequest { spec, wait_ms } = request;
        let kind = spec.kind();
        let target_id = spec.target_id().to_owned();
        let request = spec.request_id().to_owned();
        if ct.is_cancelled() {
            return Err(failure("requestCancelled", "启动请求已取消，未接纳新任务").into());
        }
        let key = (target_id.clone(), request);
        let mut registry = self.registry.lock().unwrap();
        if let Some(id) = registry.requests.get(&key) {
            let task = &registry.tasks[id];
            if task.spec != spec {
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
            spec,
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

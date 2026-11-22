//! Bounded commands and environment probes on the common remote runner.
use crate::{
    execution_output::{MemoryOutput, OutputTarget},
    remote_execution::{self, Execution, ExecutionIo, Launch, Options},
    resource_owner::Owner,
    resource_tasks::Tasks,
    workspaces::Context,
    AppCore,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;
#[derive(Default)]
pub(crate) struct Executions(Mutex<Registry>);
#[derive(Default)]
struct Registry {
    tasks: HashMap<String, Arc<Task>>,
    watermarks: HashMap<String, (Owner, u64)>,
}
struct Task {
    owner: Owner,
    request: Start,
    cancel: CancellationToken,
    started: Instant,
    output: Mutex<Arc<Mutex<MemoryOutput>>>,
    current: Mutex<Option<Arc<Execution>>>,
    tasks: Arc<Tasks>,
    finished: AtomicBool,
    released: AtomicBool,
    ended: Mutex<Option<Instant>>,
    domain_error: Mutex<Value>,
    environment_ready: AtomicBool,
}
#[derive(Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Start {
    view_id: String,
    expected_context: Context,
    operation_id: String,
    sequence: String,
    command: String,
    timeout_ms: u64,
    #[serde(default = "output_budget")]
    max_output_bytes: usize,
    #[serde(default)]
    program_output: Option<String>,
    #[serde(skip)]
    environment_keys: Option<Vec<String>>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EnvironmentStart {
    view_id: String,
    expected_context: Context,
    operation_id: String,
    sequence: String,
    timeout_ms: u64,
    #[serde(default)]
    keys: Vec<String>,
}
fn output_budget() -> usize {
    256 * 1024
}
#[derive(Debug)]
pub(crate) struct Error {
    pub code: &'static str,
    pub message: String,
}
fn error(code: &'static str, message: impl Into<String>) -> Error {
    Error {
        code,
        message: message.into(),
    }
}

fn watermark(owner: &Owner) -> String {
    format!(
        "{}/{}/{}/{}",
        owner.instance, owner.view, owner.context.connection_id, owner.context.generation
    )
}
impl Task {
    fn stop(&self) {
        self.cancel.cancel();
        if let Some(ex) = self.current.lock().unwrap().as_ref() {
            ex.cancel.cancel();
        }
    }
    fn facts(&self) -> Value {
        let mut value = self
            .current
            .lock()
            .unwrap()
            .as_ref()
            .map(|ex| ex.snapshot())
            .unwrap_or_else(
                || json!({"execSent":false,"launchQueued":false,"remoteExitConfirmed":false}),
            );
        value["execSent"] = value["launchQueued"].clone();
        if !self.finished.load(Ordering::Acquire) {
            value["state"] = json!("running");
        }
        let domain = self.domain_error.lock().unwrap().clone();
        if !domain.is_null() {
            value["state"] = json!(match domain["code"].as_str() {
                Some("cancelled") => "cancelled",
                Some("timedOut" | "deadlineExceeded") => "timedOut",
                _ => "failed",
            });
            value["error"] = domain;
        }
        if value["error"]["code"] == "deadlineExceeded" {
            value["error"]["code"] = json!("timedOut");
            if self.finished.load(Ordering::Acquire) {
                value["state"] = json!("timedOut");
            }
        }
        if self.request.environment_keys.is_some()
            && !self.environment_ready.load(Ordering::Acquire)
        {
            value["exitCode"] = Value::Null;
            value["exitSignal"] = Value::Null;
            value["remoteExitConfirmed"] = json!(false);
        }
        value
    }
}
impl Executions {
    pub(crate) fn pending(&self, matches: impl Fn(&Owner) -> bool) -> usize {
        self.0
            .lock()
            .unwrap()
            .tasks
            .values()
            .filter(|t| matches(&t.owner) && t.tasks.active() > 0)
            .count()
    }
    pub(crate) fn reap(&self) {
        reap(&mut self.0.lock().unwrap());
    }
    pub(crate) fn cancel(&self, matches: impl Fn(&Owner) -> bool) {
        let mut registry = self.0.lock().unwrap();
        registry.watermarks.retain(|_, (owner, _)| !matches(owner));
        registry.tasks.retain(|_, task| {
            if matches(&task.owner) {
                task.stop();
                task.released.store(true, Ordering::Release);
                task.tasks.close();
            }
            task.tasks.active() > 0 || !matches(&task.owner)
        });
    }
}
impl AppCore {
    pub(crate) fn session_request(
        self: &Arc<Self>,
        plugin: &str,
        instance: &str,
        method: &str,
        p: &Value,
    ) -> Result<Value, Error> {
        let text = |name: &str| {
            p[name]
                .as_str()
                .ok_or_else(|| error("invalidParams", format!("缺少 {name}")))
        };
        match method {
            "session.target.describe" => return self.describe_target(plugin, instance, p),
            "session.program.prepare" => return self.prepare_program(plugin, instance, p),
            _ => {}
        }
        if matches!(method, "session.exec.start" | "session.environment.start") {
            let request: Start = if method == "session.environment.start" {
                let r: EnvironmentStart = serde_json::from_value(p.clone())
                    .map_err(|e| error("invalidParams", e.to_string()))?;
                crate::remote_program::validate_keys(&r.keys)?;
                Start {
                    view_id: r.view_id,
                    expected_context: r.expected_context,
                    operation_id: r.operation_id,
                    sequence: r.sequence,
                    command: crate::remote_program::probe(&r.keys, true),
                    timeout_ms: r.timeout_ms,
                    max_output_bytes: crate::remote_program::ENVIRONMENT_OUTPUT_BUDGET,
                    program_output: None,
                    environment_keys: Some(r.keys),
                }
            } else {
                serde_json::from_value(p.clone())
                    .map_err(|e| error("invalidParams", e.to_string()))?
            };
            let sequence = decimal(&request.sequence)?;
            if request.command.is_empty()
                || request.command.len() > 16 * 1024
                || request.command.contains('\0')
                || request.operation_id.is_empty()
                || request.operation_id.len() > 128
                || !(1000..=30000).contains(&request.timeout_ms)
                || !(1..=1024 * 1024).contains(&request.max_output_bytes)
                || request
                    .program_output
                    .as_deref()
                    .is_some_and(|s| s != "startup-v1")
            {
                return Err(error("invalidParams", "命令任务参数超出范围"));
            }
            let owner = self
                .resource_owner(plugin, instance, &request.view_id)
                .map_err(|e| error("workspaceClosing", e))?;
            if owner.context != request.expected_context {
                return Err(error("staleContext", "连接代次已改变"));
            }
            let session = self
                .connection_session(&owner.context)
                .map_err(|e| error("staleContext", e))?;
            let admission = self
                .resource_admission(&owner)
                .map_err(|e| error("workspaceClosing", e))?;
            let mut registry = self.executions.0.lock().unwrap();
            reap(&mut registry);
            let key = watermark(&owner);
            for (id, task) in &registry.tasks {
                if watermark(&task.owner) == key
                    && task.request.operation_id == request.operation_id
                {
                    if task.request != request {
                        return Err(error("operationConflict", "同一票据参数不同"));
                    }
                    return Ok(json!({"taskId":id,"state":task.facts()["state"]}));
                }
            }
            if registry
                .watermarks
                .get(&key)
                .is_some_and(|(_, n)| sequence <= *n)
            {
                return Err(error("operationExpired", "旧任务票据已回收"));
            }
            let own: Vec<_> = registry
                .tasks
                .values()
                .filter(|t| t.owner.instance == instance)
                .collect();
            if registry
                .tasks
                .values()
                .filter(|t| !t.finished.load(Ordering::Acquire))
                .count()
                >= 8
                || own
                    .iter()
                    .filter(|t| !t.finished.load(Ordering::Acquire))
                    .count()
                    >= 4
                || own.len() >= 64
                || registry
                    .tasks
                    .values()
                    .map(|t| t.request.max_output_bytes)
                    .sum::<usize>()
                    + request.max_output_bytes
                    > 16 * 1024 * 1024
                || own
                    .iter()
                    .map(|t| t.request.max_output_bytes)
                    .sum::<usize>()
                    + request.max_output_bytes
                    > 8 * 1024 * 1024
            {
                return Err(error("resourceBusy", "命令任务资源已满"));
            }
            if registry.watermarks.len() >= 4096 && !registry.watermarks.contains_key(&key) {
                return Err(error("resourceBusy", "命令视图数量已满"));
            }
            let output = Arc::new(Mutex::new(MemoryOutput::new(
                request.max_output_bytes,
                request.program_output.is_some(),
            )));
            let task = Arc::new(Task {
                owner,
                request,
                cancel: CancellationToken::new(),
                started: Instant::now(),
                output: Mutex::new(output),
                current: Mutex::new(None),
                tasks: Arc::new(Tasks::default()),
                finished: AtomicBool::new(false),
                released: AtomicBool::new(false),
                ended: Mutex::new(None),
                domain_error: Mutex::new(Value::Null),
                environment_ready: AtomicBool::new(false),
            });
            let guard = task
                .tasks
                .enter()
                .map_err(|e| error("workspaceClosing", e))?;
            let id = self.next_id();
            registry
                .watermarks
                .insert(key, (task.owner.clone(), sequence));
            registry.tasks.insert(id.clone(), task.clone());
            drop(registry);
            drop(admission);
            if self
                .resource_owner(plugin, instance, &task.owner.view)
                .is_err()
                || self.connection_session(&task.owner.context).is_err()
            {
                task.stop();
            }
            let core = Arc::downgrade(self);
            tauri::async_runtime::spawn(async move {
                run_commands(core, &task, session).await;
                *task.ended.lock().unwrap() = Some(Instant::now());
                drop(guard);
                task.finished.store(true, Ordering::Release);
            });
            return Ok(json!({"taskId":id,"state":"running"}));
        }
        let task = {
            let mut registry = self.executions.0.lock().unwrap();
            reap(&mut registry);
            registry
                .tasks
                .get(text("taskId")?)
                .cloned()
                .ok_or_else(|| error("taskExpired", "命令任务已回收"))?
        };
        if task.owner.plugin != plugin || task.owner.instance != instance {
            return Err(error("staleContext", "任务不属于此实例"));
        }
        if task.released.load(Ordering::Acquire) {
            return Err(error("taskExpired", "命令任务已释放"));
        }
        match method {
            "session.exec.cancel" => {
                task.stop();
                Ok(json!({"state":task.facts()["state"]}))
            }
            "session.exec.release" => {
                task.stop();
                task.released.store(true, Ordering::Release);
                Ok(json!({"released":true}))
            }
            "session.exec.poll" => {
                let a = decimal(text("stdoutOffset")?)? as usize;
                let b = decimal(text("stderrOffset")?)? as usize;
                let native = task.facts();
                let output = task.output.lock().unwrap().clone();
                let output = output.lock().unwrap();
                let visible = task.request.environment_keys.is_none()
                    || task.environment_ready.load(Ordering::Acquire);
                let stdout = if visible {
                    output.stdout.as_slice()
                } else {
                    &[]
                };
                let stderr = if visible {
                    output.stderr.as_slice()
                } else {
                    &[]
                };
                if a > stdout.len() || b > stderr.len() {
                    return Err(error("invalidOffset", "输出偏移无效"));
                }
                let ae = (a + 8192).min(stdout.len());
                let be = (b + 8192).min(stderr.len());
                let done = task.finished.load(Ordering::Acquire);
                let mut value = json!({"state":native["state"],"stdout":{"dataBase64":STANDARD.encode(&stdout[a..ae]),"nextOffset":ae.to_string()},"stderr":{"dataBase64":STANDARD.encode(&stderr[b..be]),"nextOffset":be.to_string()},"outputComplete":done&&ae==stdout.len()&&be==stderr.len(),"exitCode":native["exitCode"],"exitSignal":native["exitSignal"],"error":native["error"],"remoteExitConfirmed":native["remoteExitConfirmed"],"execSent":native["execSent"],"elapsedMs":task.ended.lock().unwrap().unwrap_or_else(Instant::now).duration_since(task.started).as_millis() as u64,"outputLimitExceeded":native["error"]["code"]=="outputLimit"});
                value
                    .as_object_mut()
                    .unwrap()
                    .extend(output.startup().as_object().unwrap().clone());
                Ok(value)
            }
            _ => Err(error("unsupported", "未知会话方法")),
        }
    }
}
fn decimal(input: &str) -> Result<u64, Error> {
    if input.is_empty() || input.len() > 20 || !input.bytes().all(|b| b.is_ascii_digit()) {
        return Err(error("invalidParams", "序号必须是十进制整数"));
    }
    input
        .parse()
        .map_err(|_| error("invalidParams", "整数超出范围"))
}
fn reap(registry: &mut Registry) {
    registry.tasks.retain(|_, task| {
        !task.finished.load(Ordering::Acquire)
            || !task.released.load(Ordering::Acquire)
                && task
                    .ended
                    .lock()
                    .unwrap()
                    .is_some_and(|at| at.elapsed() < Duration::from_secs(30))
    });
}
async fn run_commands(
    core: std::sync::Weak<AppCore>,
    task: &Task,
    session: Arc<crate::connections::Session>,
) {
    let commands = task
        .request
        .environment_keys
        .as_ref()
        .map(|keys| {
            vec![
                crate::remote_program::probe(keys, true),
                crate::remote_program::probe(keys, false),
            ]
        })
        .unwrap_or_else(|| vec![task.request.command.clone()]);
    for command in commands {
        if task.cancel.is_cancelled() {
            *task.domain_error.lock().unwrap() = json!({"code":"cancelled","message":"命令已取消"});
            break;
        }
        let output = Arc::new(Mutex::new(MemoryOutput::new(
            task.request.max_output_bytes,
            task.request.program_output.is_some(),
        )));
        *task.output.lock().unwrap() = output.clone();
        let (execution, rx) = Execution::create(task.owner.clone());
        *task.current.lock().unwrap() = Some(execution.clone());
        if task.cancel.is_cancelled() {
            execution.cancel.cancel();
        }
        let guard = match task.tasks.enter() {
            Ok(guard) => guard,
            Err(e) => {
                *task.domain_error.lock().unwrap() = json!({"code":"cancelled","message":e});
                break;
            }
        };
        let options = Options {
            launch: Launch::Command(command),
            io: ExecutionIo::default(),
            started: task.started,
            deadline: Some(task.started + Duration::from_millis(task.request.timeout_ms)),
        };
        remote_execution::run(
            core.clone(),
            execution.clone(),
            rx,
            session.clone(),
            options,
            OutputTarget::Memory(output.clone()),
            guard,
        )
        .await;
        let native = execution.snapshot();
        if !native["error"].is_null() {
            break;
        }
        let Some(keys) = &task.request.environment_keys else {
            return;
        };
        let mut output = output.lock().unwrap();
        if output.stderr.is_empty() {
            if let Some(code) = native["exitCode"].as_u64() {
                match crate::remote_program::parse_environment(&output.stdout, keys, code as u32) {
                    Ok(value) => {
                        output.stdout = serde_json::to_vec(&value).unwrap();
                        output.stderr.clear();
                        task.environment_ready.store(true, Ordering::Release);
                        return;
                    }
                    Err(e) if e.starts_with("outputLimit:") => {
                        *task.domain_error.lock().unwrap() =
                            json!({"code":"outputLimit","message":e});
                        return;
                    }
                    Err(_) => {}
                }
            }
        }
        if !native["exitSignal"].is_null() {
            *task.domain_error.lock().unwrap() =
                json!({"code":"unsupportedEnvironment","message":"环境探测异常退出"});
            return;
        }
    }
    if task.request.environment_keys.is_some()
        && task.domain_error.lock().unwrap().is_null()
        && task
            .current
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|ex| ex.snapshot()["error"].is_null())
    {
        *task.domain_error.lock().unwrap() =
            json!({"code":"unsupportedEnvironment","message":"远端缺少受支持的程序启动环境"});
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decimal_and_binary_output() {
        assert!(decimal("-1").is_err());
        assert!(decimal("18446744073709551616").is_err());
        assert_eq!(decimal("18446744073709551615").unwrap(), u64::MAX);
        assert_eq!(STANDARD.encode(b"fo"), "Zm8=");
        assert_eq!(STANDARD.encode([0, 255, 128]), "AP+A");
    }
}

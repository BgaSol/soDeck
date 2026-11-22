//! Owned command resources and ordered input. The remote runner is shared with terminals.
mod output;
use crate::{
    execution_output::{CapturedOutput, OutputTarget},
    remote_execution::{
        self, Execution, ExecutionIo, InputOperation, InputOutcome, Launch, Options,
    },
    resource_owner::Owner,
    resource_tasks::Tasks,
    streams::capture::Capture,
    AppCore,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tokio::sync::{oneshot, Notify};
use tokio_util::sync::CancellationToken;

pub(crate) use sodeck_plugin_protocol::session::ProcessStart;
pub(crate) struct Reader {
    pub ack: Mutex<(u64, u64)>,
    pub changed: Notify,
    pub cancel: CancellationToken,
}
pub(crate) struct ProcessResource {
    pub owner: Owner,
    pub request: ProcessStart,
    pub execution: Arc<Execution>,
    pub output: Arc<CapturedOutput>,
    pub tasks: Arc<Tasks>,
    pub readers: Mutex<HashMap<String, Arc<Reader>>>,
    pub release_requested: AtomicBool,
    input: Mutex<InputControl>,
}
#[derive(Default)]
pub(crate) struct Registry {
    pub resources: HashMap<String, Arc<ProcessResource>>,
    sequences: HashMap<String, (Owner, u64)>,
}
impl Registry {
    fn original(
        &self,
        plugin: &str,
        instance: &str,
        request: &ProcessStart,
    ) -> Option<(String, Arc<ProcessResource>)> {
        self.resources
            .iter()
            .find(|(_, r)| {
                r.owner.plugin == plugin
                    && r.owner.instance == instance
                    && r.request.view_id == request.view_id
                    && r.request.expected_context == request.expected_context
                    && r.request.operation_id == request.operation_id
            })
            .map(|(id, r)| (id.clone(), r.clone()))
    }
}
#[derive(Default)]
pub(crate) struct Processes(pub Mutex<Registry>);
struct InputControl {
    next: u64,
    last: Option<LastInput>,
    sealed: bool,
}
struct LastInput {
    sequence: u64,
    operation: InputOperation,
    outcome: Option<InputOutcome>,
    completion: Option<oneshot::Receiver<InputOutcome>>,
}
impl InputControl {
    fn new() -> Self {
        Self {
            next: 1,
            last: None,
            sealed: false,
        }
    }
    fn refresh(&mut self) {
        if let Some(last) = self.last.as_mut() {
            if let Some(rx) = last.completion.as_mut() {
                let outcome = match rx.try_recv() {
                    Ok(outcome) => Some(outcome),
                    Err(oneshot::error::TryRecvError::Empty) => None,
                    Err(oneshot::error::TryRecvError::Closed) => Some(InputOutcome::Unconfirmed(
                        "inputUnknown:输入完成回执缺失".into(),
                    )),
                };
                if let Some(outcome) = outcome {
                    last.outcome = Some(outcome);
                    last.completion = None;
                }
            }
        }
    }
    fn repeat(&mut self, sequence: u64, operation: &InputOperation) -> Result<bool, String> {
        self.refresh();
        if let Some(last) = &self.last {
            if sequence == last.sequence {
                return if &last.operation == operation {
                    Ok(true)
                } else {
                    Err("inputConflict:同一输入序号的参数不同".into())
                };
            }
        }
        if sequence < self.next {
            return Err("inputExpired:旧输入不会重新发送".into());
        }
        if sequence > self.next {
            return Err("invalidSequence:输入序号不连续".into());
        }
        Ok(false)
    }
    fn summary(&self, resource: &ProcessResource, native: &Value) -> Value {
        let pending = self
            .last
            .as_ref()
            .is_some_and(|last| last.outcome.is_none());
        let ended = native["inputClosed"] == true
            || resource.release_requested.load(Ordering::Acquire)
            || resource.execution.cancel.is_cancelled();
        let state = if !resource.request.io.interactive() {
            "closed"
        } else if ended {
            "closed"
        } else if self.sealed {
            "closing"
        } else {
            "open"
        };
        let last=self.last.as_ref().map(|last|{
            let (state,bytes,error)=match &last.outcome {None=>("pending",None,None),Some(InputOutcome::Sent(n))=>("sent",*n,None),Some(InputOutcome::NotSent(e))=>("failed",matches!(last.operation,InputOperation::Write(_)).then_some(0),Some(e)),Some(InputOutcome::Unconfirmed(e))=>("unknown",None,Some(e))};
            json!({"sequence":last.sequence.to_string(),"action":last.operation.action(),"state":state,"sentBytes":bytes,"error":error.map(|e|json!({"code":e.split(':').next().unwrap_or("inputFailed"),"message":e}))})
        });
        json!({"state":state,"available":resource.request.io.interactive()&&!ended&&!self.sealed&&!pending&&native["launchAccepted"]==true,"nextInputSequence":self.next.to_string(),"last":last})
    }
}
fn key(owner: &Owner) -> String {
    format!(
        "{}/{}/{}/{}",
        owner.instance, owner.view, owner.context.connection_id, owner.context.generation
    )
}
fn decimal(value: &Value, name: &str) -> Result<u64, String> {
    let value = value
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 20 && s.bytes().all(|c| c.is_ascii_digit()))
        .ok_or_else(|| format!("invalidParams:{name} 必须是十进制字符串"))?;
    value
        .parse()
        .map_err(|_| format!("invalidParams:{name} 超出范围"))
}
fn native(resource: &ProcessResource, input: &mut InputControl) -> Value {
    input.refresh();
    let mut value = resource.execution.snapshot();
    value["input"] = input.summary(resource, &value);
    value
}
pub(crate) fn status(resource: &ProcessResource) -> Value {
    let value = {
        let mut input = resource.input.lock().unwrap();
        native(resource, &mut input)
    };
    output_status(resource, value)
}
fn output_status(resource: &ProcessResource, mut value: Value) -> Value {
    let capture = resource.output.capture.lock().unwrap();
    value["producedBytes"] = json!(capture.produced.to_string());
    value["earliestAvailableOffset"] = json!(capture.earliest.to_string());
    drop(capture);
    value
        .as_object_mut()
        .unwrap()
        .extend(resource.output.startup().as_object().unwrap().clone());
    value
}
impl Processes {
    pub fn cancel(&self, matches: impl Fn(&Owner) -> bool) {
        let mut registry = self.0.lock().unwrap();
        registry.sequences.retain(|_, (owner, _)| !matches(owner));
        registry.resources.retain(|_, resource| {
            if matches(&resource.owner) {
                resource.release_requested.store(true, Ordering::Release);
                resource.execution.cancel.cancel();
                resource.execution.changed.notify_waiters();
                resource.tasks.close();
                for reader in resource.readers.lock().unwrap().values() {
                    reader.cancel.cancel();
                }
            }
            !resource.release_requested.load(Ordering::Acquire) || !resource.tasks.finished()
        });
    }
    pub fn pending(&self, matches: impl Fn(&Owner) -> bool) -> usize {
        let mut registry = self.0.lock().unwrap();
        registry.resources.retain(|_, resource| {
            !resource.release_requested.load(Ordering::Acquire) || !resource.tasks.finished()
        });
        registry
            .resources
            .values()
            .filter(|resource| matches(&resource.owner) && resource.tasks.active() > 0)
            .count()
    }
}
impl AppCore {
    pub(crate) fn process_request(
        self: &Arc<Self>,
        plugin: &str,
        instance: &str,
        method: &str,
        p: &Value,
    ) -> Result<Value, crate::session_exec::Error> {
        self.process_inner(plugin, instance, method, p)
            .map_err(|message| {
                let code = match message.split(':').next().unwrap_or("") {
                    "invalidParams" => "invalidParams",
                    "staleContext" => "staleContext",
                    "operationConflict" => "operationConflict",
                    "operationExpired" => "operationExpired",
                    "resourceBusy" => "resourceBusy",
                    "workspaceClosing" => "workspaceClosing",
                    "outputFailed" => "outputFailed",
                    "processNotFound" => "processNotFound",
                    "inputConflict" => "inputConflict",
                    "inputExpired" => "inputExpired",
                    "invalidSequence" => "invalidSequence",
                    "inputClosed" => "inputClosed",
                    "inputNotReady" => "inputNotReady",
                    "unsupported" => "unsupported",
                    _ => "processFailed",
                };
                crate::session_exec::Error { code, message }
            })
    }
    fn process_inner(
        self: &Arc<Self>,
        plugin: &str,
        instance: &str,
        method: &str,
        p: &Value,
    ) -> Result<Value, String> {
        if matches!(method, "session.process.start" | "session.process.lookup") {
            let request: ProcessStart =
                serde_json::from_value(p.clone()).map_err(|e| format!("invalidParams:{e}"))?;
            let sequence = request
                .sequence
                .parse::<u64>()
                .map_err(|_| "invalidParams:启动序号无效")?;
            request.io.validate()?;
            if request.operation_id.is_empty()
                || request.operation_id.len() > 128
                || request.command.is_empty()
                || request.command.len() > 16 * 1024
                || request.command.contains('\0')
                || !matches!(request.retention.as_str(), "fixed" | "rolling")
                || !(1024..=64 * 1024 * 1024).contains(&request.max_output_bytes)
                || request
                    .timeout_ms
                    .is_some_and(|n| !(1000..=3_600_000).contains(&n))
                || request
                    .program_output
                    .as_deref()
                    .is_some_and(|s| s != "startup-v1")
                || request.io.terminal() && request.program_output.is_some()
            {
                return Err("invalidParams:进程启动参数超出范围".into());
            }
            if method.ends_with("lookup") {
                let ui = self.ui_state();
                let view = ui
                    .resource_view(&request.view_id)
                    .map_err(|e| format!("staleContext:{e}"))?;
                if view.plugin_id != plugin || view.plugin_instance_id != instance {
                    return Err("staleContext:票据不属于此实例".into());
                }
                drop(ui);
                let found = self
                    .command_processes
                    .0
                    .lock()
                    .unwrap()
                    .original(plugin, instance, &request);
                return match found {
                    Some((id, r)) => {
                        if r.request != request {
                            return Err("operationConflict:同一启动票据参数不同".into());
                        }
                        Ok(json!({"known":true,"processId":id,"status":status(&r)}))
                    }
                    None => Ok(json!({"known":false})),
                };
            }
            let found = self
                .command_processes
                .0
                .lock()
                .unwrap()
                .original(plugin, instance, &request);
            if let Some((id, r)) = found {
                if r.request != request {
                    return Err("operationConflict:同一启动票据参数不同".into());
                }
                return Ok(json!({"processId":id,"status":status(&r)}));
            }
            let owner = self
                .resource_owner(plugin, instance, &request.view_id)
                .map_err(|e| format!("staleContext:{e}"))?;
            if owner.context != request.expected_context {
                return Err("staleContext:连接代次已改变".into());
            }
            let session = self
                .connection_session(&owner.context)
                .map_err(|e| format!("staleContext:{e}"))?;
            let id = self.next_id();
            let started = Instant::now();
            let path = self
                .store
                .lock()
                .unwrap()
                .root
                .join("command-output")
                .join(format!(
                    "{}-{}-{}",
                    std::process::id(),
                    crate::core::now(),
                    id
                ));
            let capture = Capture::new(
                path,
                request.max_output_bytes,
                request.retention == "rolling",
            )
            .map_err(|e| format!("outputFailed:{e}"))?;
            let output = Arc::new(CapturedOutput::new(
                capture,
                request.max_output_bytes,
                request.program_output.is_some(),
                request.io.terminal(),
            ));
            let options = Options {
                launch: Launch::Command(request.command.clone()),
                io: request.io.clone(),
                started,
                deadline: request
                    .timeout_ms
                    .map(|ms| started + Duration::from_millis(ms)),
            };
            let (execution, receiver) = Execution::create(owner.clone());
            let tasks = Arc::new(Tasks::default());
            let guard = tasks.enter()?;
            let resource = Arc::new(ProcessResource {
                owner: owner.clone(),
                request: request.clone(),
                execution: execution.clone(),
                output: output.clone(),
                tasks,
                readers: Mutex::new(HashMap::new()),
                release_requested: AtomicBool::new(false),
                input: Mutex::new(InputControl::new()),
            });
            let admission = self
                .resource_admission(&owner)
                .map_err(|e| format!("workspaceClosing:{e}"))?;
            let mut registry = self.command_processes.0.lock().unwrap();
            registry
                .resources
                .retain(|_, r| !r.release_requested.load(Ordering::Acquire) || !r.tasks.finished());
            let own_key = key(&owner);
            if let Some((id, r)) = registry.original(plugin, instance, &request) {
                drop(registry);
                drop(admission);
                if r.request != request {
                    return Err("operationConflict:同一启动票据参数不同".into());
                }
                return Ok(json!({"processId":id,"status":status(&r)}));
            }
            if registry
                .sequences
                .get(&own_key)
                .is_some_and(|(_, n)| sequence <= *n)
            {
                return Err("operationExpired:旧启动票据已释放".into());
            }
            let live = |r: &&Arc<ProcessResource>| !r.execution.worker_done.is_cancelled();
            if registry.resources.values().filter(live).count() >= 24
                || registry
                    .resources
                    .values()
                    .filter(|r| r.owner.instance == instance)
                    .filter(live)
                    .count()
                    >= 12
                || registry
                    .resources
                    .values()
                    .filter(|r| r.owner.view == owner.view)
                    .filter(live)
                    .count()
                    >= 3
                || registry
                    .resources
                    .values()
                    .filter(|r| r.owner.instance == instance)
                    .count()
                    >= 64
                || registry.sequences.len() >= 4096 && !registry.sequences.contains_key(&own_key)
            {
                return Err("resourceBusy:命令资源已满".into());
            }
            for retention in ["fixed", "rolling"] {
                let sum: usize = registry
                    .resources
                    .values()
                    .filter(|r| r.owner.view == owner.view)
                    .map(|r| {
                        if r.request.retention == retention {
                            r.request.max_output_bytes
                        } else if retention == "fixed" {
                            r.output.capture.lock().unwrap().pinned_bytes() as usize
                        } else {
                            0
                        }
                    })
                    .sum();
                if request.retention == retention
                    && sum + request.max_output_bytes > 64 * 1024 * 1024
                {
                    return Err("resourceBusy:页面输出预算已满".into());
                }
            }
            registry
                .sequences
                .insert(own_key, (resource.owner.clone(), sequence));
            registry.resources.insert(id.clone(), resource.clone());
            drop(registry);
            drop(admission);
            if self
                .resource_owner(plugin, instance, &resource.owner.view)
                .is_err()
                || self.connection_session(&resource.owner.context).is_err()
            {
                execution.cancel.cancel();
            }
            let core = Arc::downgrade(self);
            tauri::async_runtime::spawn(remote_execution::run(
                core,
                execution,
                receiver,
                session,
                options,
                OutputTarget::Capture(output),
                guard,
            ));
            return Ok(json!({"processId":id,"status":status(&resource)}));
        }
        let id = p["processId"]
            .as_str()
            .ok_or("invalidParams:缺少 processId")?;
        let resource = self
            .command_processes
            .0
            .lock()
            .unwrap()
            .resources
            .get(id)
            .cloned()
            .ok_or("processNotFound:原进程资源已释放")?;
        if resource.owner.plugin != plugin || resource.owner.instance != instance {
            return Err("staleContext:资源不属于此实例".into());
        }
        match method {
            "session.process.status" => Ok(status(&resource)),
            "session.process.read" => {
                let offset = decimal(&p["offset"], "offset")?;
                // Observe completion before the capture waterline; finished cannot outrun its tail.
                let native = status(&resource);
                let capture = resource.output.capture.lock().unwrap();
                let watermark = capture.produced;
                let (chunks, next) = capture.read_blocks(offset, 48 * 1024, watermark)?;
                Ok(
                    json!({"chunks":chunks,"nextOffset":next.to_string(),"watermark":watermark.to_string(),"earliestAvailableOffset":capture.earliest.to_string(),"status":native}),
                )
            }
            "session.process.input" => self.process_input(&resource, p),
            "session.process.cancel" => {
                resource.execution.cancel.cancel();
                resource.execution.changed.notify_waiters();
                Ok(status(&resource))
            }
            "session.process.release" => {
                resource.release_requested.store(true, Ordering::Release);
                resource.execution.cancel.cancel();
                resource.execution.changed.notify_waiters();
                resource.tasks.close();
                for r in resource.readers.lock().unwrap().values() {
                    r.cancel.cancel();
                }
                let native = status(&resource);
                let released = resource.tasks.finished();
                if released {
                    self.command_processes
                        .0
                        .lock()
                        .unwrap()
                        .resources
                        .remove(id);
                }
                Ok(json!({"releaseRequested":true,"resourceReleased":released,"status":native}))
            }
            _ => Err("unsupported:未知进程方法".into()),
        }
    }
    fn process_input(&self, resource: &ProcessResource, p: &Value) -> Result<Value, String> {
        let sequence = decimal(&p["inputSequence"], "inputSequence")?;
        if sequence == 0 {
            return Err("invalidSequence:输入序号从 1 开始".into());
        }
        let action = p["action"].as_str().ok_or("invalidParams:缺少 action")?;
        let fields: &[&str] = match action {
            "write" => &["processId", "inputSequence", "action", "dataBase64"],
            "eof" => &["processId", "inputSequence", "action"],
            "resize" => &["processId", "inputSequence", "action", "cols", "rows"],
            _ => return Err("invalidParams:输入 action 无效".into()),
        };
        if !p
            .as_object()
            .is_some_and(|p| p.keys().all(|name| fields.contains(&name.as_str())))
        {
            return Err("invalidParams:输入包含未知参数".into());
        }
        let operation = match action {
            "write" => {
                let data = p["dataBase64"]
                    .as_str()
                    .ok_or("invalidParams:缺少 dataBase64")?;
                if data.len() > 24 * 1024 {
                    return Err("invalidParams:输入编码超限".into());
                }
                let data = STANDARD
                    .decode(data)
                    .map_err(|_| "invalidParams:Base64 输入无效")?;
                if data.len() > 16 * 1024 {
                    return Err("invalidParams:输入超过 16 KiB".into());
                }
                InputOperation::Write(data.into())
            }
            "eof" => InputOperation::Eof,
            _ => {
                let cols = p["cols"]
                    .as_u64()
                    .and_then(|n| u32::try_from(n).ok())
                    .ok_or("invalidParams:cols 无效")?;
                let rows = p["rows"]
                    .as_u64()
                    .and_then(|n| u32::try_from(n).ok())
                    .ok_or("invalidParams:rows 无效")?;
                ExecutionIo::Pty { cols, rows }.validate()?;
                InputOperation::Resize(cols, rows)
            }
        };
        {
            let mut input = resource.input.lock().unwrap();
            if input.repeat(sequence, &operation)? {
                let value = native(resource, &mut input);
                drop(input);
                return Ok(output_status(resource, value));
            }
        }
        let session = self
            .connection_session(&resource.owner.context)
            .map_err(|e| format!("staleContext:{e}"))?;
        if session.is_closed() {
            return Err("staleContext:原会话已断开".into());
        }
        let _admission = self
            .resource_admission(&resource.owner)
            .map_err(|e| format!("inputClosed:{e}"))?;
        let mut input = resource.input.lock().unwrap();
        if input.repeat(sequence, &operation)? {
            let value = native(resource, &mut input);
            drop(input);
            return Ok(output_status(resource, value));
        }
        if input
            .last
            .as_ref()
            .is_some_and(|last| last.outcome.is_none())
        {
            return Err("resourceBusy:上一项输入尚未确认".into());
        }
        if input.sealed
            || resource.release_requested.load(Ordering::Acquire)
            || resource.execution.cancel.is_cancelled()
        {
            return Err("inputClosed:输入已封闭".into());
        }
        if !resource.request.io.interactive()
            || matches!(operation, InputOperation::Eof) && resource.request.io.terminal()
            || matches!(operation, InputOperation::Resize(..)) && !resource.request.io.terminal()
        {
            return Err("unsupported:此模式不支持输入动作".into());
        }
        if resource.execution.snapshot()["launchAccepted"] != true {
            return Err("inputNotReady:远端程序尚未接纳".into());
        }
        let next = input
            .next
            .checked_add(1)
            .ok_or("invalidSequence:输入序号已耗尽")?;
        let completion = resource.execution.submit(operation.clone())?;
        input.next = next;
        input.sealed = matches!(operation, InputOperation::Eof);
        input.last = Some(LastInput {
            sequence,
            operation,
            outcome: None,
            completion: Some(completion),
        });
        let value = native(resource, &mut input);
        drop(input);
        Ok(output_status(resource, value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn input_receipts_are_bounded_and_missing_completions_stay_unknown() {
        let mut input = InputControl::new();
        let operation = InputOperation::Write(Arc::from(&b"raw\0\r"[..]));
        for sequence in 1..=5000 {
            assert!(!input.repeat(sequence, &operation).unwrap());
            let (tx, rx) = oneshot::channel();
            input.next += 1;
            input.last = Some(LastInput {
                sequence,
                operation: operation.clone(),
                outcome: None,
                completion: Some(rx),
            });
            assert!(input.repeat(sequence, &operation).unwrap());
            assert!(input.last.as_ref().unwrap().outcome.is_none());
            tx.send(InputOutcome::Sent(Some(5))).unwrap();
            input.refresh();
            assert!(input.last.as_ref().unwrap().completion.is_none());
        }
        assert!(input
            .repeat(4999, &operation)
            .unwrap_err()
            .starts_with("inputExpired:"));
        assert!(input
            .repeat(5000, &InputOperation::Eof)
            .unwrap_err()
            .starts_with("inputConflict:"));
        assert!(input
            .repeat(5002, &operation)
            .unwrap_err()
            .starts_with("invalidSequence:"));
        let (tx, rx) = oneshot::channel();
        input.last.as_mut().unwrap().completion = Some(rx);
        input.last.as_mut().unwrap().outcome = None;
        drop(tx);
        input.refresh();
        assert!(matches!(
            input.last.unwrap().outcome,
            Some(InputOutcome::Unconfirmed(_))
        ));
    }
}

//! Owned, bounded non-interactive commands on the current authenticated session.
use crate::{resource_owner::Owner, workspaces::Context, AppCore};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
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
    result: Mutex<Output>,
    started: Instant,
}
struct Output {
    state: &'static str,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    exit_code: Option<u32>,
    exit_signal: Option<String>,
    error: Value,
    confirmed: bool,
    sent: bool,
    finished: Option<Instant>,
    released: bool,
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
impl Executions {
    pub(crate) fn reap(&self) {
        reap(&mut self.0.lock().unwrap());
    }
    pub(crate) fn cancel(&self, matches: impl Fn(&Owner) -> bool) {
        let mut registry = self.0.lock().unwrap();
        registry.watermarks.retain(|_, (owner, _)| !matches(owner));
        registry.tasks.retain(|_, task| {
            if matches(&task.owner) {
                task.cancel.cancel();
                task.result.lock().unwrap().released = true;
            }
            // Running tasks keep their reserved budget until the worker exits.
            task.result.lock().unwrap().finished.is_none() || !matches(&task.owner)
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
        let text = |key: &str| {
            p[key]
                .as_str()
                .ok_or_else(|| error("invalidParams", format!("缺少 {key}")))
        };
        if method == "session.state" {
            let ui = self.component_ui.lock().unwrap();
            let view = ui
                .resource_view(text("viewId")?)
                .map_err(|e| error("staleContext", e))?;
            if view.plugin_id != plugin || view.plugin_instance_id != instance {
                return Err(error("staleContext", "视图不属于此实例"));
            }
            let context = view.context.clone();
            let frozen = ui.frozen(&context.workspace_id);
            drop(ui);
            let connected = self.connection_session(&context).is_ok();
            return Ok(
                json!({"context":context,"state":if connected {"connected"} else {"disconnected"},"closing":frozen,"capabilities":if connected {vec!["exec"]} else {vec![]}}),
            );
        }
        if method == "session.exec.start" {
            let request: Start = serde_json::from_value(p.clone())
                .map_err(|e| error("invalidParams", e.to_string()))?;
            let sequence = decimal(&request.sequence)?;
            if request.command.is_empty()
                || request.command.len() > 16 * 1024
                || request.command.contains('\0')
                || request.operation_id.is_empty()
                || request.operation_id.len() > 128
                || !(1000..=30000).contains(&request.timeout_ms)
                || !(1..=1024 * 1024).contains(&request.max_output_bytes)
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
                    return Ok(json!({"taskId":id,"state":task.result.lock().unwrap().state}));
                }
            }
            if registry
                .watermarks
                .get(&key)
                .is_some_and(|(_, n)| sequence <= *n)
            {
                return Err(error("operationExpired", "旧任务票据已回收"));
            }
            let tasks: Vec<_> = registry.tasks.values().collect();
            let own: Vec<_> = tasks
                .iter()
                .filter(|t| t.owner.instance == instance)
                .collect();
            if tasks
                .iter()
                .filter(|t| t.result.lock().unwrap().finished.is_none())
                .count()
                >= 8
                || own
                    .iter()
                    .filter(|t| t.result.lock().unwrap().finished.is_none())
                    .count()
                    >= 4
                || own.len() >= 64
                || tasks
                    .iter()
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
            let task = Arc::new(Task {
                owner,
                request,
                cancel: CancellationToken::new(),
                started: Instant::now(),
                result: Mutex::new(Output {
                    state: "running",
                    stdout: vec![],
                    stderr: vec![],
                    exit_code: None,
                    exit_signal: None,
                    error: Value::Null,
                    confirmed: false,
                    sent: false,
                    finished: None,
                    released: false,
                }),
            });
            let id = self.next_id();
            registry
                .watermarks
                .insert(key, (task.owner.clone(), sequence));
            registry.tasks.insert(id.clone(), task.clone());
            drop(registry);
            if self
                .resource_owner(plugin, instance, &task.owner.view)
                .is_err()
                || self.connection_session(&task.owner.context).is_err()
            {
                task.cancel.cancel();
            }
            let core = Arc::downgrade(self);
            tauri::async_runtime::spawn(async move {
                run(core, task, session).await;
            });
            return Ok(json!({"taskId":id,"state":"running"}));
        }
        let mut registry = self.executions.0.lock().unwrap();
        reap(&mut registry);
        let id = text("taskId")?;
        let task = registry
            .tasks
            .get(id)
            .cloned()
            .ok_or_else(|| error("taskExpired", "命令任务已回收"))?;
        if task.owner.plugin != plugin || task.owner.instance != instance {
            return Err(error("staleContext", "任务不属于此实例"));
        }
        let mut out = task.result.lock().unwrap();
        if out.released {
            return Err(error("taskExpired", "命令任务已释放"));
        }
        match method {
            "session.exec.cancel" => {
                task.cancel.cancel();
                Ok(json!({"state":out.state}))
            }
            "session.exec.release" => {
                task.cancel.cancel();
                out.released = true;
                Ok(json!({"released":true}))
            }
            "session.exec.poll" => {
                let a = decimal(text("stdoutOffset")?)? as usize;
                let b = decimal(text("stderrOffset")?)? as usize;
                if a > out.stdout.len() || b > out.stderr.len() {
                    return Err(error("invalidOffset", "输出偏移无效"));
                }
                let ae = (a + 8192).min(out.stdout.len());
                let be = (b + 8192).min(out.stderr.len());
                Ok(
                    json!({"state":out.state,"stdout":{"dataBase64":base64(&out.stdout[a..ae]),"nextOffset":ae.to_string()},"stderr":{"dataBase64":base64(&out.stderr[b..be]),"nextOffset":be.to_string()},"outputComplete":out.finished.is_some() && ae==out.stdout.len() && be==out.stderr.len(),"exitCode":out.exit_code,"exitSignal":out.exit_signal,"error":out.error,"remoteExitConfirmed":out.confirmed,"execSent":out.sent,"elapsedMs":out.finished.unwrap_or_else(Instant::now).duration_since(task.started).as_millis() as u64,"outputLimitExceeded":out.error["code"]=="outputLimit"}),
                )
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
    registry.tasks.retain(|_, t| {
        let o = t.result.lock().unwrap();
        o.finished
            .is_none_or(|at| !o.released && at.elapsed() < Duration::from_secs(30))
    });
}
fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for b in bytes.chunks(3) {
        let n = ((b[0] as u32) << 16)
            | ((b.get(1).copied().unwrap_or(0) as u32) << 8)
            | b.get(2).copied().unwrap_or(0) as u32;
        for i in 0..4 {
            result.push(if i > b.len() {
                '='
            } else {
                ABC[((n >> (18 - i * 6)) & 63) as usize] as char
            });
        }
    }
    result
}
async fn run(
    core: std::sync::Weak<AppCore>,
    task: Arc<Task>,
    session: Arc<crate::connections::Session>,
) {
    let parent = session.cancellation();
    let deadline = tokio::time::Instant::now() + Duration::from_millis(task.request.timeout_ms);
    let mut channel = None;
    let result: Result<(), (&str, &str)> = tokio::select! {
        _ = task.cancel.cancelled() => Err(("cancelled","命令已取消")),
        _ = parent.cancelled() => Err(("sessionLost","服务器连接已断开")),
        _ = tokio::time::sleep_until(deadline) => Err(("timedOut","命令超时")),
        result = async {
            channel = Some(session.open_channel().await.map_err(|_| ("sessionLost","无法打开命令通道"))?);
            let channel = channel.as_mut().unwrap();
            channel.exec(true, task.request.command.as_str()).await.map_err(|_| ("execRejected","远端拒绝执行"))?;
            task.result.lock().unwrap().sent=true;
            if let Some(c)=core.upgrade() { c.record_business(&task.owner, task.request.command.len() as u64, 0); }
            channel.eof().await.map_err(|_| ("sessionLost","命令输入无法关闭"))?;
            let mut accepted=false;
            while let Some(message)=channel.wait().await {
                match message {
                    russh::ChannelMsg::Success => accepted=true,
                    russh::ChannelMsg::Failure => return Err(("execRejected","远端拒绝执行")),
                    ref message @ (russh::ChannelMsg::Data{..} | russh::ChannelMsg::ExtendedData{ext:1,..}) => {
                        let (data, stderr)=match message {russh::ChannelMsg::Data{data}=>(data,false),russh::ChannelMsg::ExtendedData{data,..}=>(data,true),_=>unreachable!()};
                        if let Some(c)=core.upgrade() { c.record_business(&task.owner,0,data.len() as u64); }
                        let mut o=task.result.lock().unwrap();
                        if o.stdout.len()+o.stderr.len()+data.len()>task.request.max_output_bytes { return Err(("outputLimit","命令输出超过预算")); }
                        if stderr { o.stderr.extend_from_slice(&data); } else { o.stdout.extend_from_slice(&data); }
                    }
                    russh::ChannelMsg::ExitStatus{exit_status} => { let mut o=task.result.lock().unwrap(); o.exit_code=Some(exit_status); o.confirmed=true; }
                    russh::ChannelMsg::ExitSignal{signal_name,..} => { let mut o=task.result.lock().unwrap(); o.exit_signal=Some(format!("{signal_name:?}")); o.confirmed=true; }
                    _ => {}
                }
            }
            let _=accepted;
            if !task.result.lock().unwrap().confirmed { return Err(("missingExitStatus","远端没有返回退出状态")); }
            Ok(())
        } => result,
    };
    if result.is_err() {
        if let Some(channel) = channel.as_mut() {
            let _ = tokio::time::timeout(Duration::from_secs(1), async {
                let _ = channel.signal(russh::Sig::TERM).await;
                let _ = channel.close().await;
                while let Some(msg) = channel.wait().await {
                    if matches!(
                        msg,
                        russh::ChannelMsg::ExitStatus { .. } | russh::ChannelMsg::ExitSignal { .. }
                    ) {
                        task.result.lock().unwrap().confirmed = true;
                    }
                }
            })
            .await;
        }
    }
    let mut out = task.result.lock().unwrap();
    out.state = match result {
        Ok(()) => "completed",
        Err(("cancelled", _)) => "cancelled",
        Err(("timedOut", _)) => "timedOut",
        _ => "failed",
    };
    if let Err((code, message)) = result {
        out.error = json!({"code":code,"message":message});
    }
    out.finished = Some(Instant::now());
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decimal_and_binary_output() {
        assert!(decimal("-1").is_err());
        assert!(decimal("18446744073709551616").is_err());
        assert_eq!(decimal("18446744073709551615").unwrap(), u64::MAX);
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(&[0, 255, 128]), "AP+A");
    }
}

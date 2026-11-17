//! Core PTY primitives. Terminal emulation and display checkpoints belong to plugins.
use crate::{
    resource_owner::Owner,
    streams::{Replay, Sink, BLOCK},
    AppCore,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

enum Input {
    Write(Vec<u8>, oneshot::Sender<Result<(), String>>),
    Resize(u32, u32),
    Barrier(oneshot::Sender<()>),
}
#[derive(Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Start {
    id: String,
    expected_run_id: Option<String>,
    expected_context: crate::workspaces::Context,
    cols: u32,
    rows: u32,
    command: Option<String>,
    #[serde(default)]
    output_prefix: String,
    #[serde(default)]
    metadata: Value,
}
enum Phase {
    Starting,
    Running(u64),
    Ended(Value),
}
struct Run {
    id: String,
    start_offset: u64,
    owner: Owner,
    request: Start,
    phase: Mutex<Phase>,
    input: mpsc::Sender<Input>,
    cancel: CancellationToken,
    next_input: Mutex<HashMap<String, u64>>,
}
pub(crate) struct Terminal {
    // Context here identifies the retained history. Input always uses the current Run's owner.
    owner: Owner,
    pub output: Arc<Replay>,
    persistent: bool,
    current: Mutex<Option<Arc<Run>>>,
    cancel: CancellationToken,
    tasks: Arc<crate::resource_tasks::Tasks>,
    pub sent: AtomicU64,
    pub received: AtomicU64,
}
impl Terminal {
    fn snapshot(&self) -> Value {
        let current = self.current.lock().unwrap();
        match current.as_ref() {
            None => json!({"state":"idle","runId":null}),
            Some(run) => {
                let phase = run.phase.lock().unwrap();
                let (state, result) = match &*phase {
                    Phase::Starting => ("starting", Value::Null),
                    Phase::Running(_) => ("running", Value::Null),
                    Phase::Ended(result) => ("ended", result.clone()),
                };
                json!({"runId":run.id,"previousRunId":run.request.expected_run_id,
                    "state":state,"context":run.owner.context,"result":result,"metadata":run.request.metadata,
                    "startOffset":run.start_offset,"readyOffset":match &*phase {Phase::Running(offset)=>*offset,_=>run.start_offset}})
            }
        }
    }
    fn publish(&self) {
        if self.persistent {
            let state = self.snapshot();
            self.output
                .source(state["startOffset"].as_u64().unwrap_or(0), state);
        }
    }
    fn close(&self) {
        self.tasks.close();
        self.cancel.cancel();
        if let Some(run) = self.current.lock().unwrap().as_ref() {
            run.cancel.cancel();
        }
    }
    fn input_run(&self, id: Option<&str>) -> Result<Arc<Run>, String> {
        if self.cancel.is_cancelled() {
            return Err("终端已关闭".into());
        }
        let run = self.current.lock().unwrap().clone().ok_or("终端尚未启动")?;
        if self.persistent && id != Some(run.id.as_str()) {
            return Err("终端运行身份已改变，旧输入未发送".into());
        }
        if matches!(*run.phase.lock().unwrap(), Phase::Ended(_))
            || (self.persistent && !matches!(*run.phase.lock().unwrap(), Phase::Running(_)))
        {
            return Err("终端当前不可输入".into());
        }
        Ok(run)
    }
}
impl Run {
    fn enqueue_write(
        &self,
        data: Vec<u8>,
    ) -> Result<oneshot::Receiver<Result<(), String>>, String> {
        if data.len() > BLOCK {
            return Err("终端输入块超过 16 KiB".into());
        }
        let (tx, rx) = oneshot::channel();
        self.input
            .try_send(Input::Write(data, tx))
            .map_err(|_| "终端输入队列已满".to_string())?;
        Ok(rx)
    }
    fn enqueue_surface_write(
        &self,
        token: &str,
        sequence: u64,
        data: Vec<u8>,
    ) -> Result<oneshot::Receiver<Result<(), String>>, String> {
        let mut next = self.next_input.lock().unwrap();
        let n = next.entry(token.into()).or_insert(0);
        if sequence != *n {
            return Err("终端输入序号无效，不自动重放输入".into());
        }
        let rx = self.enqueue_write(data)?;
        *n += 1;
        Ok(rx)
    }
}
#[derive(Default)]
pub struct Terminals {
    entries: Mutex<HashMap<String, Arc<Terminal>>>,
}
impl Terminals {
    pub(crate) fn reconcile(&self, live: &HashMap<String, crate::workspaces::Context>) {
        let mut entries = self.entries.lock().unwrap();
        entries.retain(|_, terminal| {
            match live.get(&terminal.owner.view) {
                None => terminal.close(),
                Some(context) if *context != terminal.owner.context && !terminal.persistent => {
                    terminal.close()
                }
                Some(context) => {
                    if let Some(run) = terminal.current.lock().unwrap().as_ref() {
                        if run.owner.context != *context {
                            run.cancel.cancel();
                        }
                    }
                }
            }
            !terminal.cancel.is_cancelled() || !terminal.tasks.finished()
        });
    }
    pub(crate) fn drain_view(&self, view: &str) -> Result<(), String> {
        let terminal = self
            .entries
            .lock()
            .unwrap()
            .values()
            .find(|t| t.owner.view == view)
            .cloned();
        if let Some(t) = terminal {
            let Some(run) = t.current.lock().unwrap().clone() else {
                return Ok(());
            };
            if matches!(*run.phase.lock().unwrap(), Phase::Ended(_)) {
                return Ok(());
            }
            let (tx, rx) = oneshot::channel();
            run.input
                .try_send(Input::Barrier(tx))
                .map_err(|_| "终端输入队列仍忙碌".to_string())?;
            tauri::async_runtime::block_on(async {
                tokio::time::timeout(std::time::Duration::from_secs(5), rx)
                    .await
                    .map_err(|_| "终端输入排空超时".to_string())?
                    .map_err(|_| "终端已结束".to_string())
            })?;
        }
        Ok(())
    }
    pub(crate) fn validation_surface(&self, view: &str) -> Option<(String, Arc<Replay>)> {
        self.entries
            .lock()
            .unwrap()
            .iter()
            .find(|(_, t)| t.owner.view == view)
            .map(|(id, t)| (id.clone(), t.output.clone()))
    }
    pub(crate) fn cancel(&self, matches: impl Fn(&Owner) -> bool) {
        let mut entries = self.entries.lock().unwrap();
        entries.retain(|_, t| {
            if matches(&t.owner) {
                t.close();
                !t.tasks.finished()
            } else {
                true
            }
        });
    }
    pub(crate) fn pending(&self, matches: impl Fn(&Owner) -> bool) -> usize {
        let mut entries = self.entries.lock().unwrap();
        entries.retain(|_, t| !t.cancel.is_cancelled() || !t.tasks.finished());
        entries
            .values()
            .filter(|t| matches(&t.owner) && !t.tasks.finished())
            .count()
    }
}
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum DataRequest {
    Attach {
        id: String,
    },
    Ack {
        id: String,
        packet: u64,
    },
    Write {
        id: String,
        sequence: u64,
        data: Vec<u8>,
        #[serde(default, rename = "runId")]
        run_id: Option<String>,
    },
    Resize {
        id: String,
        cols: u32,
        rows: u32,
        #[serde(default, rename = "runId")]
        run_id: Option<String>,
    },
    Checkpoint {
        id: String,
        offset: u64,
        data: Vec<u8>,
        #[serde(default)]
        metadata: Value,
        begin: bool,
        done: bool,
    },
    Metadata {
        id: String,
        metadata: Value,
    },
    Detach {
        id: String,
    },
}
impl DataRequest {
    fn id(&self) -> &str {
        match self {
            Self::Attach { id }
            | Self::Ack { id, .. }
            | Self::Write { id, .. }
            | Self::Resize { id, .. }
            | Self::Checkpoint { id, .. }
            | Self::Metadata { id, .. }
            | Self::Detach { id } => id,
        }
    }
}

impl AppCore {
    // Package smoke commands use the real owned PTY queue, independently of the
    // iframe's sequenced input. This crate-private entry is never an RPC operation.
    pub(crate) async fn terminal_validation_write(
        &self,
        window: &str,
        token: &str,
        id: &str,
        data: Vec<u8>,
    ) -> Result<(), String> {
        let view = self.ui_data_writable(window, token)?;
        let terminal = self
            .terminals
            .entries
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or("终端已关闭")?;
        if terminal.owner.view != view.id
            || terminal.owner.plugin != view.plugin_id
            || terminal.owner.instance != view.plugin_instance_id
        {
            return Err("终端不属于此页面".into());
        }
        let run = terminal
            .current
            .lock()
            .unwrap()
            .clone()
            .ok_or("终端尚未启动")?;
        self.workspace_state()
            .validate_owned(window, &run.owner.context)?;
        self.workspace_state()
            .validate_context(&run.owner.context)?;
        if !matches!(*run.phase.lock().unwrap(), Phase::Running(_)) {
            return Err("终端当前不可输入".into());
        }
        let rx = run.enqueue_write(data)?;
        rx.await.map_err(|_| "终端已结束".to_string())?
    }

    pub(crate) fn terminal_create(
        self: &Arc<Self>,
        plugin: &str,
        instance: &str,
        view: &str,
        cols: u32,
        rows: u32,
        command: Option<String>,
        persistent: bool,
    ) -> Result<Value, String> {
        dimensions(cols, rows)?;
        validate_command(command.as_deref())?;
        let owner = self.resource_owner(plugin, instance, view)?;
        let admission = self.resource_admission(&owner)?;
        let mut entries = self.terminals.entries.lock().unwrap();
        entries.retain(|_, t| !t.cancel.is_cancelled() || !t.tasks.finished());
        if let Some((id, terminal)) = entries
            .iter()
            .find(|(_, t)| t.owner.view == view && t.owner.instance == instance)
        {
            if terminal.persistent != persistent {
                return Err("终端输出寿命模式不同".into());
            }
            return Ok(json!({"id":id}));
        }
        if entries.len() >= 128 {
            return Err("终端数量已达上限，请先关闭不用的终端".into());
        }
        let id = self.next_id();
        let recovery = self
            .store
            .lock()
            .unwrap()
            .root
            .join("terminal-recovery")
            .join(format!(
                "{}-{}-{}",
                std::process::id(),
                crate::core::now(),
                id
            ));
        let terminal = Arc::new(Terminal {
            owner: owner.clone(),
            output: Arc::new(Replay::journaled(recovery)?),
            persistent,
            current: Mutex::new(None),
            cancel: CancellationToken::new(),
            tasks: Arc::new(crate::resource_tasks::Tasks::default()),
            sent: AtomicU64::new(0),
            received: AtomicU64::new(0),
        });
        entries.insert(id.clone(), terminal.clone());
        drop(entries);
        drop(admission);
        terminal.publish();
        if !persistent {
            self.start_terminal_run(
                terminal,
                Start {
                    id: id.clone(),
                    expected_run_id: None,
                    expected_context: owner.context,
                    cols,
                    rows,
                    command,
                    output_prefix: String::new(),
                    metadata: Value::Null,
                },
            )?;
        }
        Ok(json!({"id":id}))
    }
    pub(crate) fn terminal_run_request(
        self: &Arc<Self>,
        plugin: &str,
        instance: &str,
        method: &str,
        p: &Value,
    ) -> Result<Value, String> {
        let id = p["id"].as_str().ok_or("缺少终端 ID")?;
        let terminal = self
            .terminals
            .entries
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or("终端已关闭")?;
        if terminal.owner.plugin != plugin || terminal.owner.instance != instance {
            return Err("终端不属于此插件实例".into());
        }
        let owner = self.resource_owner(plugin, instance, &terminal.owner.view)?;
        if !terminal.persistent {
            return Err("终端不支持多次运行".into());
        }
        match method {
            "terminal.run.get" => Ok(terminal.snapshot()),
            "terminal.run.start" => {
                if !p
                    .as_object()
                    .is_some_and(|o| o.contains_key("expectedRunId"))
                {
                    return Err("缺少 expectedRunId".into());
                }
                let request: Start =
                    serde_json::from_value(p.clone()).map_err(|e| e.to_string())?;
                if request.expected_context != owner.context {
                    return Err("连接代次已改变".into());
                }
                self.workspace_state().validate_context(&owner.context)?;
                self.start_terminal_run(terminal, request)
            }
            _ => Err("未知终端运行方法".into()),
        }
    }
    fn start_terminal_run(
        self: &Arc<Self>,
        terminal: Arc<Terminal>,
        request: Start,
    ) -> Result<Value, String> {
        dimensions(request.cols, request.rows)?;
        validate_command(request.command.as_deref())?;
        if (!request.metadata.is_null() && !request.metadata.is_object())
            || request.metadata.to_string().len() > 8192
        {
            return Err("终端运行元数据无效或超过 8 KiB".into());
        }
        if request.output_prefix.len() > BLOCK {
            return Err("终端输出前缀超过 16 KiB".into());
        }
        let owner = self.resource_owner(
            &terminal.owner.plugin,
            &terminal.owner.instance,
            &terminal.owner.view,
        )?;
        if owner.context != request.expected_context {
            return Err("连接代次已改变".into());
        }
        let admission = self.resource_admission(&owner)?;
        let mut current = terminal.current.lock().unwrap();
        if terminal.cancel.is_cancelled() {
            return Err("终端已关闭".into());
        }
        if let Some(previous) = current.as_ref() {
            if previous.request.expected_run_id == request.expected_run_id {
                if previous.request != request {
                    return Err("相同运行前驱的启动参数不同".into());
                }
                drop(current);
                drop(admission);
                return Ok(terminal.snapshot());
            }
            if request.expected_run_id.as_deref() != Some(previous.id.as_str())
                || !matches!(*previous.phase.lock().unwrap(), Phase::Ended(_))
            {
                return Err("终端运行尚未结束或运行身份已改变".into());
            }
        } else if request.expected_run_id.is_some() {
            return Err("终端运行身份无效".into());
        }
        let task = terminal.tasks.enter()?;
        let (input, rx) = mpsc::channel(32);
        let run = Arc::new(Run {
            id: self.next_id(),
            start_offset: terminal.output.watermark(),
            owner,
            request,
            phase: Mutex::new(Phase::Starting),
            input,
            cancel: terminal.cancel.child_token(),
            next_input: Mutex::new(HashMap::new()),
        });
        *current = Some(run.clone());
        drop(current);
        drop(admission);
        terminal.publish();
        let core = Arc::downgrade(self);
        let target = terminal.clone();
        tauri::async_runtime::spawn(async move {
            let result = run_pty(core, target.clone(), run.clone(), rx).await;
            let result = match result {
                Ok(code) => json!({"exitCode":code}),
                Err(error) => json!({"error":error.chars().take(4096).collect::<String>()}),
            };
            drop(task); // All channel work is complete before another Run may be accepted.
            *run.phase.lock().unwrap() = Phase::Ended(result.clone());
            target.publish();
            if !target.persistent {
                target.output.finish(result);
            }
        });
        Ok(terminal.snapshot())
    }
    pub(crate) fn terminal_close(
        &self,
        plugin: &str,
        instance: &str,
        id: &str,
    ) -> Result<Value, String> {
        let entries = self.terminals.entries.lock().unwrap();
        if let Some(t) = entries.get(id) {
            if t.owner.plugin != plugin || t.owner.instance != instance {
                return Err("终端不属于此插件实例".into());
            }
            t.close();
        }
        Ok(json!({}))
    }
    pub async fn terminal_data(
        self: &Arc<Self>,
        window: &str,
        token: &str,
        request: DataRequest,
        sink: Option<Sink>,
    ) -> Result<(), String> {
        let view = self.ui_data_view(window, token)?;
        let terminal = self
            .terminals
            .entries
            .lock()
            .unwrap()
            .get(request.id())
            .cloned()
            .ok_or("终端已关闭")?;
        if terminal.owner.view != view.id
            || terminal.owner.plugin != view.plugin_id
            || terminal.owner.instance != view.plugin_instance_id
        {
            return Err("终端不属于此页面".into());
        }
        match request {
            DataRequest::Attach { .. } => {
                let live = self.ui_data_surface_ids();
                if let Some(run) = terminal.current.lock().unwrap().as_ref() {
                    run.next_input
                        .lock()
                        .unwrap()
                        .retain(|key, _| live.contains(key));
                }
                let sink = sink.ok_or("缺少数据通道")?;
                let output = terminal.output.clone();
                let core = Arc::downgrade(self);
                let window = window.to_string();
                let token = token.to_string();
                let check = token.clone();
                let task = terminal.tasks.enter()?;
                let cancel = terminal.cancel.clone();
                tauri::async_runtime::spawn(async move {
                    let _task = task;
                    tokio::select! {biased; _=cancel.cancelled()=>{}, _=output.subscribe(token, sink, move || {
                        core.upgrade().is_some_and(|c| c.ui_data_view(&window, &check).is_ok())
                    })=>{}}
                });
                Ok(())
            }
            DataRequest::Ack { packet, .. } => terminal.output.ack(token, packet),
            DataRequest::Metadata { metadata, .. } => {
                let _admission = self.surface_write_admission(window, token)?;
                terminal.output.metadata(token, metadata)
            }
            DataRequest::Checkpoint {
                offset,
                data,
                metadata,
                begin,
                done,
                ..
            } => {
                let _admission = self.surface_write_admission(window, token)?;
                terminal
                    .output
                    .checkpoint(token, offset, data, metadata, begin, done)
            }
            DataRequest::Detach { .. } => {
                terminal.output.detach(token);
                Ok(())
            }
            DataRequest::Resize {
                cols, rows, run_id, ..
            } => {
                let admission = self.surface_write_admission(window, token)?;
                dimensions(cols, rows)?;
                let run = terminal.input_run(run_id.as_deref())?;
                admission.workspaces.validate_context(&run.owner.context)?;
                run.input
                    .try_send(Input::Resize(cols, rows))
                    .map_err(|_| "终端忙碌，请稍后重试".into())
            }
            DataRequest::Write {
                sequence,
                data,
                run_id,
                ..
            } => {
                let rx = {
                    let admission = self.surface_write_admission(window, token)?;
                    if data.len() > BLOCK {
                        return Err("终端输入块超过 16 KiB".into());
                    }
                    let run = terminal.input_run(run_id.as_deref())?;
                    admission.workspaces.validate_context(&run.owner.context)?;
                    run.enqueue_surface_write(token, sequence, data)?
                };
                rx.await.map_err(|_| "终端已结束".to_string())?
            }
        }
    }
}
fn dimensions(cols: u32, rows: u32) -> Result<(), String> {
    if !(2..=1000).contains(&cols) || !(1..=500).contains(&rows) {
        Err("终端尺寸超出范围".into())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terminal() -> (Run, mpsc::Receiver<Input>) {
        let (input, rx) = mpsc::channel(32);
        (
            Run {
                id: "run".into(),
                start_offset: 0,
                request: Start {
                    id: "terminal".into(),
                    expected_run_id: None,
                    expected_context: crate::workspaces::Context {
                        workspace_id: "workspace".into(),
                        server_id: "server".into(),
                        connection_id: "connection".into(),
                        generation: 1,
                    },
                    cols: 80,
                    rows: 24,
                    command: None,
                    output_prefix: String::new(),
                    metadata: Value::Null,
                },
                phase: Mutex::new(Phase::Running(0)),
                owner: Owner {
                    plugin: "test.terminal".into(),
                    instance: "instance".into(),
                    view: "view".into(),
                    context: crate::workspaces::Context {
                        workspace_id: "workspace".into(),
                        server_id: "server".into(),
                        connection_id: "connection".into(),
                        generation: 1,
                    },
                },
                input,
                cancel: CancellationToken::new(),
                next_input: Mutex::new(HashMap::new()),
            },
            rx,
        )
    }

    fn receive(rx: &mut mpsc::Receiver<Input>) -> Vec<u8> {
        match rx.try_recv().unwrap() {
            Input::Write(bytes, _) => bytes,
            Input::Resize(..) | Input::Barrier(..) => panic!("unexpected control message"),
        }
    }

    #[test]
    fn validation_write_survives_surface_input_after_a_sequence_snapshot() {
        let (terminal, mut rx) = terminal();
        let snapshot = 0;
        terminal
            .enqueue_surface_write("surface", 0, b"reply".to_vec())
            .unwrap();
        // This is the previous package smoke race: the real iframe advances the
        // sequence while the smoke test awaits font application and PTY resize.
        assert!(terminal
            .enqueue_surface_write("surface", snapshot, b"smoke".to_vec())
            .is_err());
        terminal.enqueue_write(b"smoke".to_vec()).unwrap();
        terminal
            .enqueue_surface_write("surface", 1, b"next reply".to_vec())
            .unwrap();
        assert_eq!(receive(&mut rx), b"reply");
        assert_eq!(receive(&mut rx), b"smoke");
        assert_eq!(receive(&mut rx), b"next reply");
    }

    #[test]
    fn validation_write_does_not_consume_the_iframes_first_input_sequence() {
        let (terminal, mut rx) = terminal();
        terminal.enqueue_write(b"smoke".to_vec()).unwrap();
        terminal
            .enqueue_surface_write("surface", 0, b"first reply".to_vec())
            .unwrap();
        assert!(terminal
            .enqueue_surface_write("surface", 0, b"duplicate".to_vec())
            .is_err());
        assert_eq!(receive(&mut rx), b"smoke");
        assert_eq!(receive(&mut rx), b"first reply");
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn failed_queue_writes_keep_the_surface_sequence_and_input_bounds() {
        let (terminal, mut rx) = terminal();
        assert!(terminal.enqueue_write(vec![0; BLOCK + 1]).is_err());
        for _ in 0..32 {
            terminal.enqueue_write(b"smoke".to_vec()).unwrap();
        }
        assert!(terminal
            .enqueue_surface_write("surface", 0, b"reply".to_vec())
            .is_err());
        assert!(terminal.enqueue_write(b"overflow".to_vec()).is_err());
        assert_eq!(receive(&mut rx), b"smoke");
        terminal
            .enqueue_surface_write("surface", 0, b"reply".to_vec())
            .unwrap();
        assert!(terminal
            .enqueue_surface_write("surface", 0, b"duplicate".to_vec())
            .is_err());
    }
}
fn validate_command(command: Option<&str>) -> Result<(), String> {
    if command.is_some_and(|s| s.is_empty() || s.len() > 32 * 1024 || s.contains('\0')) {
        return Err("终端启动命令无效或超过 32 KiB".into());
    }
    Ok(())
}
async fn run_pty(
    core: std::sync::Weak<AppCore>,
    terminal: Arc<Terminal>,
    run: Arc<Run>,
    mut input: mpsc::Receiver<Input>,
) -> Result<Option<u32>, String> {
    let session = core
        .upgrade()
        .ok_or("应用已退出")?
        .connection_session(&run.owner.context)?;
    let token = session.cancellation();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
    let mut channel = tokio::select! {biased;
        _=run.cancel.cancelled()=>return Err("终端已关闭".into()),
        _=token.cancelled()=>return Err("服务器连接已断开".into()),
        result=tokio::time::timeout_at(deadline,session.open_channel())=>
            result.map_err(|_|"打开终端超时")??,
    };
    let work = async {
        let opening = async {
            terminal
                .output
                .push(run.request.output_prefix.as_bytes(), &run.cancel)
                .await?;
            channel
                .request_pty(
                    true,
                    "xterm-256color",
                    run.request.cols,
                    run.request.rows,
                    0,
                    0,
                    &[],
                )
                .await
                .map_err(|e| e.to_string())?;
            loop {
                match channel.wait().await {
                    Some(russh::ChannelMsg::Success) => break,
                    Some(
                        russh::ChannelMsg::Failure
                        | russh::ChannelMsg::Close
                        | russh::ChannelMsg::Eof,
                    )
                    | None => return Err("服务器拒绝 PTY".into()),
                    _ => {}
                }
            }
            if let Some(command) = &run.request.command {
                channel
                    .exec(true, command.as_str())
                    .await
                    .map_err(|e| e.to_string())?;
            } else {
                channel
                    .request_shell(true)
                    .await
                    .map_err(|e| e.to_string())?;
            }
            loop {
                match channel.wait().await {
                    Some(russh::ChannelMsg::Success) => break,
                    Some(russh::ChannelMsg::Data { data })
                    | Some(russh::ChannelMsg::ExtendedData { data, .. }) => {
                        terminal
                            .received
                            .fetch_add(data.len() as u64, Ordering::Relaxed);
                        if let Some(c) = core.upgrade() {
                            c.record_business(&run.owner, 0, data.len() as u64);
                        }
                        terminal.output.push(&data, &run.cancel).await?
                    }
                    Some(
                        russh::ChannelMsg::Failure
                        | russh::ChannelMsg::Close
                        | russh::ChannelMsg::Eof,
                    )
                    | None => return Err("服务器拒绝终端启动请求".into()),
                    _ => {}
                }
            }
            Ok::<_, String>(())
        };
        tokio::time::timeout_at(deadline, opening)
            .await
            .map_err(|_| "打开终端超时".to_string())??;
        *run.phase.lock().unwrap() = Phase::Running(terminal.output.watermark());
        terminal.publish();
        let mut code = None;
        loop {
            tokio::select! {biased;
                item=input.recv()=>match item {
                    Some(Input::Write(data,reply))=>{
                        let len=data.len();let result=channel.data(data.as_slice()).await.map_err(|e|e.to_string());
                        if result.is_ok(){terminal.sent.fetch_add(len as u64,Ordering::Relaxed);
                            if let Some(c)=core.upgrade(){c.record_business(&run.owner,len as u64,0);}}
                        let _=reply.send(result);
                    },
                    Some(Input::Resize(cols,rows))=>channel.window_change(cols,rows,0,0).await.map_err(|e|e.to_string())?,
                    Some(Input::Barrier(reply))=>{let _=reply.send(());},
                    None=>break,
                },
                message=channel.wait()=>match message {
                    Some(russh::ChannelMsg::Data{data})|Some(russh::ChannelMsg::ExtendedData{data,..})=>{
                        terminal.received.fetch_add(data.len() as u64,Ordering::Relaxed);
                        if let Some(c)=core.upgrade(){c.record_business(&run.owner,0,data.len() as u64);}
                        terminal.output.push(&data,&run.cancel).await?;
                    },
                    Some(russh::ChannelMsg::ExitStatus{exit_status})=>code=Some(exit_status),
                    Some(russh::ChannelMsg::Failure)=>return Err("服务器拒绝终端请求".into()),
                    Some(russh::ChannelMsg::Close)|None=>break,
                    _=>{},
                }
            }
        }
        Ok(code)
    };
    let result = tokio::select! {biased;
        _=run.cancel.cancelled()=>Err("终端已关闭".into()),
        _=token.cancelled()=>Err("服务器连接已断开".into()),
        result=work=>result,
    };
    let _ = tokio::time::timeout(std::time::Duration::from_secs(1), channel.close()).await;
    result
}

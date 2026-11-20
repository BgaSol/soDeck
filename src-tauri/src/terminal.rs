//! Core PTY primitives. Terminal emulation and display checkpoints belong to plugins.
use crate::execution_output::OutputTarget;
#[cfg(test)]
use crate::remote_execution::QueuedInput;
use crate::remote_execution::{
    self, Execution, ExecutionIo, InputOperation, InputOutcome, Launch, Options,
};
use crate::{
    resource_owner::Owner,
    streams::{Replay, Sink, BLOCK},
    AppCore,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Instant;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
#[cfg(test)]
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

pub(crate) struct Terminal {
    owner: Owner,
    pub output: Arc<Replay>,
    execution: Arc<Execution>,
    cancel: CancellationToken,
    tasks: Arc<crate::resource_tasks::Tasks>,
    next_input: Mutex<HashMap<String, u64>>,
}
impl Terminal {
    fn enqueue_write(&self, data: Vec<u8>) -> Result<oneshot::Receiver<InputOutcome>, String> {
        self.execution.submit(InputOperation::Write(data.into()))
    }

    fn enqueue_surface_write(
        &self,
        token: &str,
        sequence: u64,
        data: Vec<u8>,
    ) -> Result<oneshot::Receiver<InputOutcome>, String> {
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
    pub(crate) fn drain_view(&self, view: &str) -> Result<(), String> {
        let terminal = self
            .entries
            .lock()
            .unwrap()
            .values()
            .find(|t| t.owner.view == view)
            .cloned();
        if let Some(t) = terminal {
            if t.execution.worker_done.is_cancelled() || t.tasks.finished() {
                return Ok(());
            }
            let rx = t.execution.submit(InputOperation::Barrier)?;
            let outcome = tauri::async_runtime::block_on(async {
                tokio::time::timeout(std::time::Duration::from_secs(5), rx)
                    .await
                    .map_err(|_| "终端输入排空超时".to_string())?
                    .map_err(|_| "终端已结束".to_string())
            })?;
            input_result(outcome)?;
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
                t.tasks.close();
                t.cancel.cancel();
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
    },
    Resize {
        id: String,
        cols: u32,
        rows: u32,
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
        self.workspace_state()
            .validate_owned(window, &terminal.owner.context)?;
        self.workspace_state()
            .validate_context(&terminal.owner.context)?;
        let rx = terminal.enqueue_write(data)?;
        input_result(rx.await.map_err(|_| "终端已结束".to_string())?)
    }

    pub(crate) fn terminal_open(
        self: &Arc<Self>,
        plugin: &str,
        instance: &str,
        view: &str,
        cols: u32,
        rows: u32,
        command: Option<String>,
    ) -> Result<Value, String> {
        dimensions(cols, rows)?;
        if command
            .as_ref()
            .is_some_and(|s| s.is_empty() || s.len() > 32 * 1024 || s.contains('\0'))
        {
            return Err("终端启动命令无效或超过 32 KiB".into());
        }
        let owner = self.resource_owner(plugin, instance, view)?;
        self.workspace_state().validate_context(&owner.context)?;
        let session = self.connection_session(&owner.context)?;
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
        let output = Arc::new(Replay::journaled(recovery)?);
        let admission = self.resource_admission(&owner)?;
        let mut entries = self.terminals.entries.lock().unwrap();
        entries.retain(|_, terminal| !terminal.cancel.is_cancelled() || !terminal.tasks.finished());
        if let Some((id, _)) = entries
            .iter()
            .find(|(_, t)| t.owner.view == view && t.owner.instance == instance)
        {
            return Ok(json!({"id":id}));
        }
        if entries.len() >= 128 {
            return Err("终端数量已达上限，请先关闭不用的终端".into());
        }
        let (execution, rx) = Execution::create(owner.clone());
        let terminal = Arc::new(Terminal {
            owner,
            output,
            execution: execution.clone(),
            cancel: execution.cancel.clone(),
            tasks: Arc::new(crate::resource_tasks::Tasks::default()),
            next_input: Mutex::new(HashMap::new()),
        });
        let task = terminal.tasks.enter()?;
        entries.insert(id.clone(), terminal.clone());
        drop(entries);
        drop(admission);
        if let Err(error) = self.resource_owner(plugin, instance, view) {
            terminal.cancel.cancel();
            self.terminals.entries.lock().unwrap().remove(&id);
            return Err(error);
        }
        let core = Arc::downgrade(self);
        let started = Instant::now();
        let options = Options {
            launch: command.map(Launch::Command).unwrap_or(Launch::LoginShell),
            io: ExecutionIo::Pty { cols, rows },
            started,
            deadline: None,
        };
        tauri::async_runtime::spawn(remote_execution::run(
            core,
            execution,
            rx,
            session,
            options,
            OutputTarget::Terminal(terminal.output.clone()),
            task,
        ));
        Ok(json!({"id":id}))
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
            t.cancel.cancel();
            t.tasks.close();
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
                terminal
                    .next_input
                    .lock()
                    .unwrap()
                    .retain(|key, _| live.contains(key));
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
            DataRequest::Resize { cols, rows, .. } => {
                let admission = self.surface_write_admission(window, token)?;
                dimensions(cols, rows)?;
                admission
                    .workspaces
                    .validate_context(&terminal.owner.context)?;
                terminal
                    .execution
                    .submit(InputOperation::Resize(cols, rows))
                    .map(|_| ())
            }
            DataRequest::Write { sequence, data, .. } => {
                let rx = {
                    let admission = self.surface_write_admission(window, token)?;
                    if data.len() > BLOCK {
                        return Err("终端输入块超过 16 KiB".into());
                    }
                    admission
                        .workspaces
                        .validate_context(&terminal.owner.context)?;
                    terminal.enqueue_surface_write(token, sequence, data)?
                };
                input_result(rx.await.map_err(|_| "终端已结束".to_string())?)
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

    fn terminal() -> (Terminal, mpsc::Receiver<QueuedInput>) {
        let owner = Owner {
            plugin: "test.terminal".into(),
            instance: "instance".into(),
            view: "view".into(),
            context: crate::workspaces::Context {
                workspace_id: "workspace".into(),
                server_id: "server".into(),
                connection_id: "connection".into(),
                generation: 1,
            },
        };
        let (execution, rx) = Execution::create(owner.clone());
        (
            Terminal {
                owner,
                output: Arc::new(Replay::default()),
                execution: execution.clone(),
                cancel: execution.cancel.clone(),
                tasks: Arc::new(crate::resource_tasks::Tasks::default()),
                next_input: Mutex::new(HashMap::new()),
            },
            rx,
        )
    }
    fn receive(rx: &mut mpsc::Receiver<QueuedInput>) -> Vec<u8> {
        match rx.try_recv().unwrap().operation {
            InputOperation::Write(bytes) => bytes.to_vec(),
            _ => panic!("unexpected control message"),
        }
    }
    #[test]
    fn drain_preserves_sent_failed_and_unknown_barrier_results() {
        for outcome in [
            InputOutcome::Sent(None),
            InputOutcome::NotSent("inputClosed:stopped".into()),
            InputOutcome::Unconfirmed("inputUnknown:lost".into()),
        ] {
            let expected = matches!(outcome, InputOutcome::Sent(_));
            let (terminal, mut input) = terminal();
            let guard = terminal.tasks.enter().unwrap();
            let terminals = Arc::new(Terminals::default());
            terminals
                .entries
                .lock()
                .unwrap()
                .insert("terminal".into(), Arc::new(terminal));
            let worker = std::thread::spawn(move || terminals.drain_view("view"));
            let queued = input.blocking_recv().unwrap();
            assert_eq!(queued.operation, InputOperation::Barrier);
            queued.completion.send(outcome).unwrap();
            assert_eq!(worker.join().unwrap().is_ok(), expected);
            drop(guard);
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
fn input_result(outcome: InputOutcome) -> Result<(), String> {
    match outcome {
        InputOutcome::Sent(_) => Ok(()),
        InputOutcome::NotSent(e) | InputOutcome::Unconfirmed(e) => Err(e),
    }
}

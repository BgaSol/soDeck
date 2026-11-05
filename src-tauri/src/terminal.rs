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
}
pub(crate) struct Terminal {
    owner: Owner,
    pub output: Arc<Replay>,
    input: mpsc::Sender<Input>,
    cancel: CancellationToken,
    next_input: Mutex<HashMap<String, u64>>,
    pub sent: AtomicU64,
    pub received: AtomicU64,
}
#[derive(Default)]
pub struct Terminals {
    entries: Mutex<HashMap<String, Arc<Terminal>>>,
}
impl Terminals {
    pub(crate) fn validation_surface(
        &self,
        view: &str,
        token: &str,
    ) -> Option<(String, Arc<Replay>, u64)> {
        self.entries
            .lock()
            .unwrap()
            .iter()
            .find(|(_, t)| t.owner.view == view)
            .map(|(id, t)| {
                (
                    id.clone(),
                    t.output.clone(),
                    *t.next_input.lock().unwrap().get(token).unwrap_or(&0),
                )
            })
    }
    pub(crate) fn cancel(&self, matches: impl Fn(&Owner) -> bool) {
        let mut entries = self.entries.lock().unwrap();
        entries.retain(|_, t| {
            if matches(&t.owner) {
                t.cancel.cancel();
                false
            } else {
                true
            }
        });
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
            | Self::Detach { id } => id,
        }
    }
}

impl AppCore {
    pub(crate) fn terminal_open(
        self: &Arc<Self>,
        plugin: &str,
        instance: &str,
        view: &str,
        cols: u32,
        rows: u32,
    ) -> Result<Value, String> {
        dimensions(cols, rows)?;
        let owner = self.resource_owner(plugin, instance, view)?;
        self.workspaces
            .lock()
            .unwrap()
            .validate_context(&owner.context)?;
        let mut entries = self.terminals.entries.lock().unwrap();
        if let Some((id, _)) = entries
            .iter()
            .find(|(_, t)| t.owner.view == view && t.owner.instance == instance)
        {
            return Ok(json!({"id":id}));
        }
        if entries.len() >= 128 {
            return Err("终端数量已达上限，请先关闭不用的终端".into());
        }
        let id = self.next_id();
        let (input, rx) = mpsc::channel(32);
        let terminal = Arc::new(Terminal {
            owner,
            output: Arc::new(Replay::default()),
            input,
            cancel: CancellationToken::new(),
            next_input: Mutex::new(HashMap::new()),
            sent: AtomicU64::new(0),
            received: AtomicU64::new(0),
        });
        entries.insert(id.clone(), terminal.clone());
        drop(entries);
        if let Err(error) = self.resource_owner(plugin, instance, view) {
            terminal.cancel.cancel();
            self.terminals.entries.lock().unwrap().remove(&id);
            return Err(error);
        }
        let core = Arc::downgrade(self);
        tauri::async_runtime::spawn(async move {
            let result = run(core, terminal.clone(), rx, cols, rows).await;
            terminal.output.finish(match result {
                Ok(code) => json!({"exitCode":code}),
                Err(error) => json!({"error":error}),
            });
        });
        Ok(json!({"id":id}))
    }
    pub(crate) fn terminal_close(
        &self,
        plugin: &str,
        instance: &str,
        id: &str,
    ) -> Result<Value, String> {
        let mut entries = self.terminals.entries.lock().unwrap();
        if let Some(t) = entries.get(id) {
            if t.owner.plugin != plugin || t.owner.instance != instance {
                return Err("终端不属于此插件实例".into());
            }
            t.cancel.cancel();
            entries.remove(id);
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
                tauri::async_runtime::spawn(output.subscribe(token, sink, move || {
                    core.upgrade()
                        .is_some_and(|c| c.ui_data_view(&window, &check).is_ok())
                }));
                Ok(())
            }
            DataRequest::Ack { packet, .. } => terminal.output.ack(token, packet),
            DataRequest::Checkpoint {
                offset,
                data,
                metadata,
                begin,
                done,
                ..
            } => {
                self.workspaces
                    .lock()
                    .unwrap()
                    .validate_owned(window, &view.context)?;
                terminal
                    .output
                    .checkpoint(token, offset, data, metadata, begin, done)
            }
            DataRequest::Detach { .. } => {
                terminal.output.detach(token);
                Ok(())
            }
            DataRequest::Resize { cols, rows, .. } => {
                self.ui_data_writable(window, token)?;
                dimensions(cols, rows)?;
                self.workspaces
                    .lock()
                    .unwrap()
                    .validate_owned(window, &terminal.owner.context)?;
                self.workspaces
                    .lock()
                    .unwrap()
                    .validate_context(&terminal.owner.context)?;
                terminal
                    .input
                    .try_send(Input::Resize(cols, rows))
                    .map_err(|_| "终端忙碌，请稍后重试".into())
            }
            DataRequest::Write { sequence, data, .. } => {
                self.ui_data_writable(window, token)?;
                if data.len() > BLOCK {
                    return Err("终端输入块超过 16 KiB".into());
                }
                self.workspaces
                    .lock()
                    .unwrap()
                    .validate_owned(window, &terminal.owner.context)?;
                self.workspaces
                    .lock()
                    .unwrap()
                    .validate_context(&terminal.owner.context)?;
                let (tx, rx) = oneshot::channel();
                {
                    let mut next = terminal.next_input.lock().unwrap();
                    let n = next.entry(token.into()).or_insert(0);
                    if sequence != *n {
                        return Err("终端输入序号无效，不自动重放输入".into());
                    }
                    terminal
                        .input
                        .try_send(Input::Write(data, tx))
                        .map_err(|_| "终端输入队列已满".to_string())?;
                    *n += 1;
                }
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
async fn run(
    core: std::sync::Weak<AppCore>,
    terminal: Arc<Terminal>,
    mut input: mpsc::Receiver<Input>,
    cols: u32,
    rows: u32,
) -> Result<Option<u32>, String> {
    let session = core
        .upgrade()
        .ok_or("应用已退出")?
        .connection_session(&terminal.owner.context)?;
    let token = session.cancellation();
    let opening = async {
        let channel = session.open_channel().await?;
        channel
            .request_pty(true, "xterm-256color", cols, rows, 0, 0, &[])
            .await
            .map_err(|e| e.to_string())?;
        channel
            .request_shell(true)
            .await
            .map_err(|e| e.to_string())?;
        Ok::<_, String>(channel)
    };
    let mut channel = tokio::select! {biased;_=terminal.cancel.cancelled()=>return Err("终端已关闭".into()),_=token.cancelled()=>return Err("服务器连接已断开".into()),v=tokio::time::timeout(std::time::Duration::from_secs(30),opening)=>v.map_err(|_|"打开终端超时".to_string())??};
    let mut code = None;
    let work = async {
        loop {
            tokio::select! {biased;
                item=input.recv()=>match item{
                    Some(Input::Write(data,reply))=>{let len=data.len();let result=channel.data(data.as_slice()).await.map_err(|e|e.to_string());if result.is_ok(){terminal.sent.fetch_add(len as u64,Ordering::Relaxed);if let Some(c)=core.upgrade(){c.record_business(&terminal.owner,len as u64,0);}}let _=reply.send(result);},
                    Some(Input::Resize(cols,rows))=>channel.window_change(cols,rows,0,0).await.map_err(|e|e.to_string())?,
                    None=>break,
                },
                message=channel.wait()=>match message {
                    Some(russh::ChannelMsg::Data{data})|Some(russh::ChannelMsg::ExtendedData{data,..})=>{terminal.received.fetch_add(data.len() as u64,Ordering::Relaxed);if let Some(c)=core.upgrade(){c.record_business(&terminal.owner,0,data.len() as u64);}terminal.output.push(&data,&terminal.cancel).await?;},
                    Some(russh::ChannelMsg::ExitStatus{exit_status})=>code=Some(exit_status),
                    Some(russh::ChannelMsg::Failure)=>return Err("服务器拒绝 PTY 或登录 shell".into()),
                    Some(russh::ChannelMsg::Close)|None=>break,
                    _=>{}
                }
            }
        }
        Ok(code)
    };
    let result = tokio::select! {biased;_=terminal.cancel.cancelled()=>Err("终端已关闭".into()),_=token.cancelled()=>Err("服务器连接已断开".into()),v=work=>v};
    let _ = tokio::time::timeout(std::time::Duration::from_secs(1), channel.close()).await;
    result
}

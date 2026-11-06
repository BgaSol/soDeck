//! External plugin processes and their versioned control channel.
mod dispatch;
mod manifest;
mod process;
mod rpc;

pub use manifest::read_manifest;
pub(crate) use process::supervise;
pub(crate) use rpc::read_frame;

use crate::{
    core::AppCore,
    model::{PluginRecord, PluginStatus, PluginView},
};
use serde_json::{json, Value};
use std::{
    sync::{
        mpsc::{self, Receiver, Sender, SyncSender},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

pub(crate) struct Call {
    method: String,
    params: Value,
    reply: Sender<Result<Value, String>>,
    deadline: Instant,
}
pub(crate) struct Control {
    calls: Receiver<Call>,
    stops: Receiver<Sender<Result<(), String>>>,
}
enum Output {
    Frame(Value),
    Diagnostic(String),
    Broken(String),
}

pub(crate) struct Instance {
    pub(crate) settings_api: std::sync::atomic::AtomicBool,
    view: Mutex<PluginView>,
    calls: SyncSender<Call>,
    stops: SyncSender<Sender<Result<(), String>>>,
}

impl Instance {
    pub fn request(&self, method: &str, params: Value) -> Result<Value, String> {
        if self.view().status != PluginStatus::Running {
            return Err("插件当前不可用".into());
        }
        rpc::encode(
            &json!({"jsonrpc":"2.0","id":"ui-18446744073709551615","method":method,"params":params}),
        )?;
        let (tx, rx) = mpsc::channel();
        self.calls
            .try_send(Call {
                method: method.into(),
                params,
                reply: tx,
                deadline: Instant::now() + rpc::REQUEST_TIMEOUT,
            })
            .map_err(|error| match error {
                mpsc::TrySendError::Full(_) => "插件请求队列已满，请稍后重试",
                mpsc::TrySendError::Disconnected(_) => "插件通信已停止",
            })?;
        rx.recv_timeout(rpc::REQUEST_TIMEOUT + Duration::from_millis(100))
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => "插件界面请求超过 5 秒",
                mpsc::RecvTimeoutError::Disconnected => "插件通信已停止",
            })?
    }
    pub fn new(
        record: PluginRecord,
        instance: String,
    ) -> (
        Arc<Self>,
        Control,
        Sender<Result<(), String>>,
        Receiver<Result<(), String>>,
    ) {
        let (calls, call_rx) = mpsc::sync_channel(64);
        let (stops, stop_rx) = mpsc::sync_channel(16);
        let (ready, ready_rx) = mpsc::channel();
        (
            Arc::new(Self {
                settings_api: std::sync::atomic::AtomicBool::new(false),
                view: Mutex::new(PluginView {
                    directory: record.directory,
                    manifest: record.manifest,
                    status: PluginStatus::Starting,
                    instance_id: Some(instance),
                    pid: None,
                    started_at: None,
                    error: None,
                    cleanup_complete: false,
                    enabled: record.enabled,
                    pending_restart: false,
                    pending_version: None,
                    update_error: record.update_error,
                }),
                calls,
                stops,
            }),
            Control {
                calls: call_rx,
                stops: stop_rx,
            },
            ready,
            ready_rx,
        )
    }
    pub fn view(&self) -> PluginView {
        self.view.lock().unwrap().clone()
    }
    pub fn stop(&self) -> Result<Receiver<Result<(), String>>, String> {
        let (sender, receiver) = mpsc::channel();
        let mut view = self.view.lock().unwrap();
        if view.cleanup_complete {
            let _ = sender.send(Ok(()));
            return Ok(receiver);
        }
        self.stops
            .try_send(sender)
            .map_err(|_| "插件停止请求已满或管理进程不可用，请查看日志".to_string())?;
        view.status = PluginStatus::Stopping;
        Ok(receiver)
    }
    fn update(&self, core: &AppCore, action: impl FnOnce(&mut PluginView)) {
        action(&mut self.view.lock().unwrap());
        core.changed();
    }
}

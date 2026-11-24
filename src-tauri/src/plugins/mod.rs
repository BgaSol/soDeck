//! External plugin processes and their versioned control channel.
use sodeck_plugin_protocol::ApiError;
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
    reply: Sender<Result<Value, ApiError>>,
    deadline: Instant,
    task: Option<crate::resource_tasks::Task>,
    observation: Observation,
    expire_at_deadline: bool,
}
pub(crate) type Receipt = Arc<Mutex<Option<Result<Value, ApiError>>>>;
/// A timed-out caller keeps its receipt until the real reply or process cleanup.
struct Observation {
    completion: Option<Receipt>,
    command: Option<crate::commands::Delivery>,
}
impl Observation {
    fn sent(&mut self) {
        if let Some(command) = &mut self.command {
            command.sent();
        }
    }
    fn frame(&mut self, core: &AppCore, frame: &Value) {
        if let Some(command) = &mut self.command {
            command.complete(frame);
            command.log(core);
        }
    }
    fn complete(&self, result: &Result<Value, ApiError>) {
        if let Some(receipt) = &self.completion {
            *receipt.lock().unwrap() = Some(result.clone());
        }
    }
}
impl Drop for Observation {
    fn drop(&mut self) {
        if let Some(receipt) = &self.completion {
            receipt
                .lock()
                .unwrap()
                .get_or_insert_with(|| Err(ApiError::local("请求未发送或插件进程已结束")));
        }
    }
}
pub(crate) struct Control {
    calls: Receiver<Call>,
    stops: Receiver<Sender<Result<(), String>>>,
}
enum Output {
    Wake,
    Frame(Value),
    Diagnostic(String),
    Broken(String),
}

pub(crate) struct Instance {
    view: Mutex<PluginView>,
    calls: SyncSender<Call>,
    stops: SyncSender<Sender<Result<(), String>>>,
}

impl Instance {
    pub fn request(&self, method: &str, params: Value) -> Result<Value, ApiError> {
        self.request_tracked(method, params, None)
    }
    pub(crate) fn request_tracked(
        &self,
        method: &str,
        params: Value,
        task: Option<crate::resource_tasks::Task>,
    ) -> Result<Value, ApiError> {
        self.request_observed(method, params, task, None, rpc::REQUEST_TIMEOUT)
    }
    pub(crate) fn request_observed(
        &self,
        method: &str,
        params: Value,
        task: Option<crate::resource_tasks::Task>,
        receipt: Option<Receipt>,
        timeout: Duration,
    ) -> Result<Value, ApiError> {
        let rx = self.enqueue_request(method, params, task, receipt, timeout, false)?;
        rx.recv_timeout(timeout + Duration::from_millis(100))
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => ApiError::timeout(),
                mpsc::RecvTimeoutError::Disconnected => ApiError::local("插件通信已停止"),
            })?
    }
    pub(crate) fn enqueue_state(
        &self,
        params: Value,
    ) -> Result<Receiver<Result<Value, ApiError>>, ApiError> {
        self.enqueue_request(
            "state.changed",
            params,
            None,
            None,
            rpc::REQUEST_TIMEOUT,
            true,
        )
    }
    pub(crate) fn enqueue_connection_change(&self, params: Value) -> Result<(), ApiError> {
        self.enqueue_request(
            "connections.changed",
            params,
            None,
            None,
            rpc::REQUEST_TIMEOUT,
            true,
        )
        .map(|_| ())
    }
    fn enqueue_request(
        &self,
        method: &str,
        params: Value,
        task: Option<crate::resource_tasks::Task>,
        receipt: Option<Receipt>,
        timeout: Duration,
        expire_at_deadline: bool,
    ) -> Result<Receiver<Result<Value, ApiError>>, ApiError> {
        self.enqueue_observation(
            method,
            params,
            task,
            Observation {
                completion: receipt,
                command: None,
            },
            timeout,
            expire_at_deadline,
        )
    }
    pub(crate) fn enqueue_command(
        &self,
        method: &str,
        params: Value,
        task: crate::resource_tasks::Task,
        delivery: crate::commands::Delivery,
    ) -> Result<(), ApiError> {
        self.enqueue_observation(
            method,
            params,
            Some(task),
            Observation {
                completion: None,
                command: Some(delivery),
            },
            rpc::REQUEST_TIMEOUT,
            false,
        )
        .map(|_| ())
    }
    fn enqueue_observation(
        &self,
        method: &str,
        params: Value,
        task: Option<crate::resource_tasks::Task>,
        observation: Observation,
        timeout: Duration,
        expire_at_deadline: bool,
    ) -> Result<Receiver<Result<Value, ApiError>>, ApiError> {
        if self.view().status != PluginStatus::Running {
            return Err(ApiError::not_sent("插件当前不可用"));
        }
        rpc::encode(
            &json!({"jsonrpc":"2.0","id":"ui-18446744073709551615","method":method,"params":params}),
        ).map_err(ApiError::not_sent)?;
        let (tx, rx) = mpsc::channel();
        self.calls
            .try_send(Call {
                method: method.into(),
                params,
                reply: tx,
                deadline: Instant::now() + timeout,
                task,
                observation,
                expire_at_deadline,
            })
            .map_err(|error| match error {
                mpsc::TrySendError::Full(_) => ApiError::busy(),
                mpsc::TrySendError::Disconnected(_) => ApiError::not_sent("插件通信已停止"),
            })?;
        Ok(rx)
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

impl AppCore {
    pub(crate) fn plugin_instance(
        &self,
        plugin: &str,
        instance: &str,
    ) -> Option<Arc<crate::plugins::Instance>> {
        self.instances
            .lock()
            .unwrap()
            .get(plugin)
            .cloned()
            .filter(|p| {
                let v = p.view();
                v.instance_id.as_deref() == Some(instance)
                    && v.status == crate::model::PluginStatus::Running
            })
    }
}

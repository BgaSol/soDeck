//! One bounded, view-owned startup intent, advanced by short UI events.
use crate::{
    creation::{CreateParams, DirectoryIntegration, Launch},
    integration::{Prepared, Probe},
};
use serde_json::{json, Value};
use sodeck_plugin_client::{Client, RpcError};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Mutex,
};

#[derive(Clone)]
enum Step {
    Session,
    ConnectionSnapshot,
    Connect(Value),
    WaitConnection(String),
    Prepare,
    Probe(Probe),
    Release(String, Prepared),
    Launch(Value),
    WaitRun(String),
    Cleanup(String, String),
    Done,
    Failed(String),
}
#[derive(Clone)]
struct Intent {
    key: String,
    context: Value,
    previous_run: Value,
    sequence: u64,
    reconnect: bool,
    integrate: bool,
    cols: u32,
    rows: u32,
    step: Step,
}
pub struct Startup {
    intent: Mutex<Intent>,
    advancing: AtomicBool,
    closed: AtomicBool,
}
struct Advance<'a>(&'a AtomicBool);
impl Drop for Advance<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
impl Startup {
    pub fn initial(context: Value, integrate: bool) -> Self {
        Self {
            intent: Mutex::new(Intent {
                key: "initial".into(),
                context,
                previous_run: Value::Null,
                sequence: 1,
                reconnect: false,
                integrate,
                cols: 80,
                rows: 24,
                step: Step::Session,
            }),
            advancing: AtomicBool::new(false),
            closed: AtomicBool::new(false),
        }
    }
    pub fn snapshot(&self) -> Value {
        let intent = self.intent.lock().unwrap();
        let (status, phase, error) = match &intent.step {
            Step::Session
            | Step::ConnectionSnapshot
            | Step::Connect(_)
            | Step::WaitConnection(_) => ("pending", "connection", None),
            Step::Prepare | Step::Probe(_) | Step::Release(..) | Step::Cleanup(..) => {
                ("pending", "preparing", None)
            }
            Step::Launch(_) | Step::WaitRun(_) => ("pending", "opening", None),
            Step::Done => ("completed", "ready", None),
            Step::Failed(error) => ("failed", "failed", Some(error)),
        };
        json!({"key":intent.key,"status":status,"phase":phase,"error":error})
    }
    pub fn close(&self) {
        self.closed.store(true, Ordering::Release);
        // Core view disposal cancels owned executions, including starts whose receipt was lost.
    }
    pub fn event(
        &self,
        client: &Client,
        view: &str,
        id: &str,
        config: &CreateParams,
        context: &Value,
        data: &Value,
        sequence: &AtomicU64,
    ) -> Result<Value, String> {
        if self.closed.load(Ordering::Acquire) {
            return Err("终端页面已关闭".into());
        }
        if self
            .advancing
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Ok(json!({"startup":self.snapshot()}));
        }
        let _advance = Advance(&self.advancing);
        let mut intent = self.intent.lock().unwrap().clone();
        if data["action"] == "beginRecovery" {
            if !matches!(config.launch, Launch::LoginShell) {
                return Err("指定命令请使用重新打开终端".into());
            }
            let key = data["requestId"]
                .as_str()
                .filter(|s| !s.is_empty() && s.len() <= 128)
                .ok_or("缺少恢复请求编号")?;
            if !matches!(intent.step, Step::Done | Step::Failed(_)) || intent.key == key {
                return Ok(json!({"startup":self.snapshot()}));
            }
            let run = client.call("terminal.run.get", json!({"id":id}))?;
            if run["state"] != "ended" && run["state"] != "idle" {
                return Err("终端运行尚未结束".into());
            }
            if data.get("expectedRunId").is_none() || data["expectedRunId"] != run["runId"] {
                return Err("终端运行身份已改变".into());
            }
            let cols = data["cols"]
                .as_u64()
                .filter(|n| (2..=1000).contains(n))
                .ok_or("终端列数无效")? as u32;
            let rows = data["rows"]
                .as_u64()
                .filter(|n| (1..=500).contains(n))
                .ok_or("终端行数无效")? as u32;
            intent = Intent {
                key: key.into(),
                context: context.clone(),
                previous_run: run["runId"].clone(),
                sequence: sequence.fetch_add(1, Ordering::Relaxed),
                reconnect: true,
                integrate: intent.integrate,
                cols,
                rows,
                step: Step::Session,
            };
        } else if data["action"] == "continueStart" {
            if data["key"] != intent.key {
                return Err("终端启动意图已改变".into());
            }
            if !matches!(intent.step, Step::Done | Step::Failed(_)) {
                if let Err(error) = intent.advance(client, view, id, config) {
                    if error.code != -32003 {
                        intent.step = match &intent.step {
                            Step::Probe(probe) if probe.task.is_some() => {
                                Step::Cleanup(probe.task.clone().unwrap(), error.message)
                            }
                            Step::Release(task, _) => Step::Cleanup(task.clone(), error.message),
                            _ => Step::Failed(error.message),
                        };
                    }
                }
            }
        } else {
            return Err("未知终端启动事件".into());
        }
        if self.closed.load(Ordering::Acquire) {
            return Err("终端页面已关闭".into());
        }
        *self.intent.lock().unwrap() = intent;
        Ok(json!({"startup":self.snapshot()}))
    }
}
impl Intent {
    fn advance(
        &mut self,
        client: &Client,
        view: &str,
        id: &str,
        config: &CreateParams,
    ) -> Result<(), RpcError> {
        let next = match &mut self.step {
            Step::Session => {
                let state = client.call_detailed("session.state", json!({"viewId":view}))?;
                if state["closing"] == true {
                    return Err("终端页面正在关闭".into());
                }
                if state["context"] != self.context {
                    return Err("连接代次已改变，请重新恢复".into());
                }
                if state["state"] == "connected" {
                    Step::Prepare
                } else if self.reconnect {
                    Step::ConnectionSnapshot
                } else {
                    return Err("服务器连接已断开，请按回车恢复".into());
                }
            }
            Step::ConnectionSnapshot => Step::Connect(
                client.call_detailed("session.connection.get", json!({"viewId":view}))?
                    ["attemptId"]
                    .clone(),
            ),
            Step::Connect(previous) => {
                let result = client.call_detailed(
                    "session.connection.start",
                    json!({"viewId":view,
                    "expectedContext":self.context,"expectedAttemptId":previous}),
                )?;
                let attempt = result["attemptId"]
                    .as_str()
                    .ok_or("核心未返回连接尝试编号")?;
                Step::WaitConnection(attempt.into())
            }
            Step::WaitConnection(attempt) => {
                let state = client.call_detailed(
                    "session.connection.get",
                    json!({"viewId":view,"attemptId":attempt}),
                )?;
                if state["sourceContext"] != self.context {
                    return Err("连接恢复起点已改变".into());
                }
                match state["state"].as_str() {
                    Some("connecting") => return Ok(()),
                    Some("connected") => {
                        self.context = state["context"].clone();
                        Step::Prepare
                    }
                    _ => {
                        return Err(state["error"]["message"]
                            .as_str()
                            .unwrap_or("连接已取消或断开")
                            .into())
                    }
                }
            }
            Step::Prepare => {
                let prepared = match &config.launch {
                    Launch::Command(command) => Prepared {
                        command: Some(command.clone()),
                        integration: json!({"mode":"off"}),
                    },
                    Launch::LoginShell if config.directory != DirectoryIntegration::Auto => {
                        Prepared {
                            command: None,
                            integration: json!({"mode":config.directory.name()}),
                        }
                    }
                    Launch::LoginShell if !self.integrate => Prepared {
                        command: None,
                        integration: json!({"mode":"unavailable","reasonCode":"old-core"}),
                    },
                    Launch::LoginShell => {
                        return {
                            self.step =
                                Step::Probe(Probe::new(view, &self.context, self.sequence)?);
                            Ok(())
                        };
                    }
                };
                Step::Launch(self.launch(id, prepared))
            }
            Step::Probe(probe) => {
                let Some(prepared) = probe.advance(client)? else {
                    return Ok(());
                };
                Step::Release(probe.task.clone().ok_or("目录准备任务未登记")?, prepared)
            }
            Step::Release(task, prepared) => {
                match client.call_detailed("session.exec.release", json!({"taskId":task})) {
                    Ok(_) => {}
                    Err(error) if error.data["code"] == "taskExpired" => {}
                    Err(error) => return Err(error),
                }
                let prepared = prepared.clone();
                Step::Launch(self.launch(id, prepared))
            }
            Step::Launch(request) => {
                let state = client.call_detailed("terminal.run.start", request.clone())?;
                Step::WaitRun(state["runId"].as_str().ok_or("核心未返回运行编号")?.into())
            }
            Step::WaitRun(run) => {
                let state = client.call_detailed("terminal.run.get", json!({"id":id}))?;
                if state["runId"] != *run {
                    return Err("终端运行已被替换".into());
                }
                match state["state"].as_str() {
                    Some("starting") => return Ok(()),
                    Some("running") => Step::Done,
                    Some("ended") if state["result"]["error"].is_null() => Step::Done,
                    _ => {
                        return Err(state["result"]["error"]
                            .as_str()
                            .unwrap_or("终端启动失败")
                            .into())
                    }
                }
            }
            Step::Cleanup(task, reason) => {
                if let Err(error) =
                    client.call_detailed("session.exec.release", json!({"taskId":task}))
                {
                    if error.code == -32003 {
                        return Err(error);
                    }
                }
                Step::Failed(reason.clone())
            }
            Step::Done | Step::Failed(_) => return Ok(()),
        };
        self.step = next;
        Ok(())
    }
    fn launch(&self, id: &str, prepared: Prepared) -> Value {
        // Move the old screen into scrollback before the new shell can clear its viewport.
        let prefix = if self.previous_run.is_null() {
            String::new()
        } else {
            "\u{18}\u{1b}\\\u{1b}[?1049l\u{1b}[!p\u{1b}[?1000l\u{1b}[?1002l\u{1b}[?1003l\u{1b}[?1006l\u{1b}[?2004l\u{1b}[0m\u{1b}[9999;1H".to_string()+&"\r\n".repeat(self.rows as usize)
        };
        json!({"id":id,"expectedRunId":self.previous_run,"expectedContext":self.context,
            "cols":self.cols,"rows":self.rows,"command":prepared.command,"outputPrefix":prefix,
            "metadata":{"integration":prepared.integration}})
    }
}

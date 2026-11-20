use crate::app::App;
use crate::{
    evidence,
    targets::State,
    task::{ExecMode, FileSpec, Kind, Task, TaskOperation},
    tasks::{error, failure, text},
};
use serde_json::{json, Value};
use std::{
    fs, thread,
    time::{Duration, Instant},
};

mod command;
mod files;

enum Native {
    Command(String),
    File { file: String, operation: String },
}
struct Outcome {
    complete: bool,
    error: Value,
}
impl Outcome {
    fn failed(error: Value) -> Self {
        Self {
            complete: false,
            error,
        }
    }
}
struct Collector<'a> {
    app: &'a App,
    task: &'a Task,
    native: Option<Native>,
    batch_file: Option<String>,
    admission_unknown: bool,
}
pub fn run(app: &App, task: &Task) {
    let mut collector = Collector {
        app,
        task,
        native: None,
        batch_file: None,
        admission_unknown: false,
    };
    let result = collector.prepare().and_then(|_| match task.kind() {
        Kind::Exec => collector.command(),
        Kind::Download | Kind::Upload => collector.files(),
    });
    let outcome = result.unwrap_or_else(Outcome::failed);
    if collector.native.is_none() && !collector.admission_unknown {
        if task.snapshot().native["admission"] == "pending" {
            task.observe(
                json!({"admission":"rejected","execSent":false,"error":outcome.error}),
                None,
            );
        }
        task.target.release_admission(task.kind());
    }
    task.seal_input();
    while task.input_running() {
        thread::sleep(Duration::from_millis(10));
    }
    task.finalizing(outcome.complete, outcome.error);
    // Keep the last native facts on disk before native result buffers are released.
    let saved_native = evidence::metadata(
        &task.directory,
        &evidence::task_metadata(task, &task.snapshot()),
    );
    if let Err(message) = &saved_native {
        task.capture_error(error(message.clone()));
        let _ = app.rpc("logging.write", json!({"level":"warn","message":"释放前的任务元数据保存失败","fields":{"taskId":task.id,"error":message}}));
    }
    if saved_native.is_ok() {
        collector.cleanup();
    } else {
        task.cleanup(
            false,
            failure("recordNotSaved", "原生回执尚未落盘，保留至资源关闭"),
        );
    }
    let candidate = task.final_candidate();
    let saved = evidence::metadata(&task.directory, &evidence::task_metadata(task, &candidate))
        .err()
        .map(error);
    task.publish(candidate, saved);
}
impl Collector<'_> {
    fn prepare(&self) -> Result<(), Value> {
        fs::create_dir_all(&self.task.directory).map_err(|e| error(e.to_string()))?;
        evidence::metadata(
            &self.task.directory,
            &evidence::task_metadata(self.task, &self.task.snapshot()),
        )
        .map_err(error)?;
        if self.app.stopped(self.task) {
            return Err(error("接纳前已取消"));
        }
        let target = self.app.observe_target(&self.task.target)?;
        if target.state != State::Connected || target.closing {
            return Err(error("服务器未连接或正在关闭"));
        }
        let capability = if self.task.kind() == Kind::Exec {
            "exec"
        } else {
            "files"
        };
        if !target.capabilities.iter().any(|c| c == capability) {
            return Err(error(format!("目标不提供 {capability} 能力")));
        }
        self.task.set_target(
            target
                .target
                .expect("validated connected observation has a descriptor"),
        );
        evidence::metadata(
            &self.task.directory,
            &evidence::task_metadata(self.task, &self.task.snapshot()),
        )
        .map_err(error)
    }
    fn accept(
        &mut self,
        method: &str,
        request: Value,
        lookup_method: &str,
        lookup: Value,
        key: &str,
    ) -> Result<Value, Value> {
        if self.app.stopped(self.task) {
            return Err(error("接纳前已取消"));
        }
        let initial = self.app.rpc(method, request);
        let unknown = match initial {
            Ok(value) if text(&value, key, 256).is_ok() => {
                if key == "processId" {
                    self.task.set_process(
                        text(&value, "processId", 256)?.to_owned(),
                        value["status"].clone(),
                    );
                } else {
                    self.task.observe(value.clone(), None);
                }
                self.task.target.release_admission(self.task.kind());
                return Ok(value);
            }
            Ok(value) => json!({"response":value}),
            Err(e) if e["code"] == -32003 => e,
            Err(e) => {
                self.task.observe(
                    json!({"admission":"rejected","execSent":false,"error":e}),
                    None,
                );
                return Err(e);
            }
        };
        self.admission_unknown = true;
        self.task
            .observe(json!({"admission":"unknown","error":unknown}), None);
        while !self.app.closing(self.task) {
            if let Ok(value) = self.app.rpc(lookup_method, lookup.clone()) {
                if text(&value, key, 256).is_ok() {
                    self.admission_unknown = false;
                    if key == "processId" {
                        self.task.set_process(
                            text(&value, "processId", 256)?.to_owned(),
                            value["status"].clone(),
                        );
                    } else {
                        self.task.observe(value.clone(), None);
                    }
                    self.task.target.release_admission(self.task.kind());
                    return Ok(value);
                }
            }
            thread::sleep(Duration::from_millis(100));
        }
        Err(error("核心通信结束，接纳结果未知"))
    }
    fn unavailable(&self, error: Value) {
        let previous = self.task.snapshot().native.clone();
        let mut next = json!({"state":"unavailable","lastResult":previous,"error":error});
        if self.task.kind() != Kind::Exec {
            next["transfer"] = previous["transfer"].clone();
            if next["transfer"].is_null() {
                next["transfer"] = json!({});
            }
            if !["committed", "notCommitted"]
                .contains(&next["transfer"]["commitState"].as_str().unwrap_or(""))
            {
                next["transfer"]["commitState"] = json!("unknown");
            }
        }
        self.task.observe(next, None);
    }
    fn cleanup(&self) {
        let mut errors = Vec::new();
        let mut released = false;
        if let Some(native) = &self.native {
            let snapshot = self.task.snapshot();
            let ended = match native {
                Native::Command(_) => snapshot.native["workerFinished"] == true,
                Native::File { .. } => ["completed", "failed", "cancelled", "unknown"]
                    .contains(&snapshot.native["state"].as_str().unwrap_or("")),
            };
            let mut ready = ended;
            if !ended {
                let cancel = match native {
                    Native::Command(id) => self
                        .app
                        .rpc("session.process.cancel", json!({"processId":id})),
                    Native::File { file, operation } => self.app.rpc(
                        "files.operations",
                        json!({"id":file,"action":"cancel","operationId":operation}),
                    ),
                };
                if let Err(e) = cancel {
                    errors.push(json!({"step":"cancel","error":e}));
                }
                let until = Instant::now() + Duration::from_secs(5);
                while !self.app.client.is_closed()
                    && !self.app.shutdown.is_cancelled()
                    && Instant::now() < until
                {
                    let status = match native {
                        Native::Command(id) => self
                            .app
                            .rpc("session.process.status", json!({"processId":id})),
                        Native::File { file, operation } => self.app.rpc(
                            "files.operations",
                            json!({"id":file,"action":"poll","operationId":operation}),
                        ),
                    };
                    match status {
                        Ok(value) => {
                            ready = match native {
                                Native::Command(_) => value["workerFinished"] == true,
                                Native::File { .. } => {
                                    ["completed", "failed", "cancelled", "unknown"]
                                        .contains(&value["state"].as_str().unwrap_or(""))
                                }
                            };
                            self.task.observe(value, None);
                            if ready {
                                break;
                            }
                        }
                        Err(e) => {
                            errors.push(json!({"step":"status","error":e}));
                            break;
                        }
                    }
                    thread::sleep(Duration::from_millis(100));
                }
            }
            if ready {
                if let Err(message) = evidence::metadata(
                    &self.task.directory,
                    &evidence::task_metadata(self.task, &self.task.snapshot()),
                ) {
                    self.task.capture_error(error(message.clone()));
                    errors.push(json!({"step":"saveBeforeRelease","error":error(message)}));
                    self.task.cleanup(false,json!({"code":"recordNotSaved","message":"最终原生事实未落盘","details":errors}));
                    return;
                }
                let result = match native {
                    Native::Command(id) => self
                        .app
                        .rpc("session.process.release", json!({"processId":id})),
                    Native::File { file, operation } => self.app.rpc(
                        "files.operations",
                        json!({"id":file,"action":"release","operationId":operation}),
                    ),
                };
                released = match &result {
                    Ok(value) => {
                        if matches!(native, Native::Command(_)) {
                            value["resourceReleased"] == true
                        } else {
                            true
                        }
                    }
                    Err(_) => false,
                };
                if let Err(e) = result {
                    errors.push(json!({"step":"release","error":e}));
                }
            } else {
                errors.push(json!({"step":"release","error":failure("cleanupIncomplete","原生结束尚未确认")}));
            }
        }
        if let Some(file) = &self.batch_file {
            if let Err(e) = self.app.rpc(
                "files.request",
                json!({"id":file,"action":"batch.end","batchId":self.task.id}),
            ) {
                errors.push(json!({"step":"batch.end","error":e}));
            }
        }
        let error = if errors.is_empty() {
            Value::Null
        } else {
            json!({"code":"cleanupFailed","message":"原生清理未全部成功","details":errors})
        };
        self.task.cleanup(released, error);
    }
}

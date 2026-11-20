use crate::app::App;
use crate::{
    evidence,
    targets::State,
    task::{Kind, Task},
    tasks::{error, failure, text},
};
use serde_json::{json, Value};
use std::{
    fs, thread,
    time::{Duration, Instant},
};

enum Native {
    Command(String),
    Download { file: String, operation: String },
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
    let result = collector.prepare().and_then(|_| match task.kind {
        Kind::Exec => collector.command(),
        Kind::Download => collector.download(),
    });
    let outcome = result.unwrap_or_else(Outcome::failed);
    if collector.native.is_none() && !collector.admission_unknown {
        if task.snapshot().native["admission"] == "pending" {
            task.observe(
                json!({"admission":"rejected","execSent":false,"error":outcome.error}),
                None,
            );
        }
        task.target.release_admission(task.kind);
    }
    task.finalizing(outcome.complete, outcome.error);
    // Keep the last native facts on disk before native result buffers are released.
    if let Err(message) = evidence::metadata(
        &task.directory,
        &evidence::task_metadata(task, &task.snapshot()),
    ) {
        let _ = app.rpc("logging.write", json!({"level":"warn","message":"释放前的任务元数据保存失败","fields":{"taskId":task.id,"error":message}}));
    }
    collector.cleanup();
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
        let capability = if self.task.kind == Kind::Exec {
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
    fn command(&mut self) -> Result<Outcome, Value> {
        let mut output = evidence::Output::new(&self.task.directory).map_err(error)?;
        let request = json!({
            "viewId":self.task.target.view,"expectedContext":self.task.target.context,
            "operationId":self.task.id,"sequence":self.task.admission_sequence.to_string(),
            "command":self.task.input["command"],"timeoutMs":self.task.input["timeoutSeconds"].as_u64().unwrap()*1000,
            "maxOutputBytes":16*1024*1024,"retention":"fixed",
        });
        let accepted = self.accept(
            "session.stream.open",
            request.clone(),
            "session.stream.lookup",
            request,
            "streamId",
        )?;
        let id = text(&accepted, "streamId", 256)?.to_owned();
        self.native = Some(Native::Command(id.clone()));
        let result = self.poll_command(&id, &mut output);
        match (result, output.sync()) {
            (Ok(outcome), Ok(())) => Ok(outcome),
            (Ok(_), Err(e)) => Err(error(e)),
            (Err(e), _) => Err(e),
        }
    }
    fn poll_command(&self, id: &str, output: &mut evidence::Output) -> Result<Outcome, Value> {
        let mut offset = 0u64;
        let mut cancelled = false;
        let mut save_error = Value::Null;
        loop {
            if self.app.shutdown.is_cancelled() || self.app.client.is_closed() {
                return Err(if save_error.is_null() {
                    error("通信结束，最终结果未知")
                } else {
                    save_error
                });
            }
            if (self.app.stopped(self.task) || !save_error.is_null()) && !cancelled {
                cancelled = self
                    .app
                    .rpc("session.stream.close", json!({"streamId":id}))
                    .is_ok();
            }
            let polled = match self.app.rpc(
                "session.stream.poll",
                json!({"streamId":id,"offset":offset.to_string()}),
            ) {
                Ok(value) => value,
                Err(e) => {
                    self.unavailable(e.clone());
                    return Err(if save_error.is_null() { e } else { save_error });
                }
            };
            let native = polled["status"].clone();
            if save_error.is_null() {
                let result = (|| {
                    let chunks = polled["chunks"]
                        .as_array()
                        .ok_or_else(|| error("核心输出页无效"))?;
                    let next = text(&polled, "nextOffset", 32)?
                        .parse::<u64>()
                        .map_err(|_| error("核心偏移无效"))?;
                    if next < offset {
                        return Err(error("核心偏移倒退"));
                    }
                    output.append(chunks).map_err(error)?;
                    offset = next;
                    Ok::<_, Value>(())
                })();
                if let Err(e) = result {
                    save_error = e;
                    self.task.capture_error(save_error.clone());
                }
            }
            self.task.observe(native.clone(), Some(output.committed));
            if native["workerFinished"] == true
                && (!save_error.is_null() || polled["nextOffset"] == polled["watermark"])
            {
                return Ok(Outcome {
                    complete: save_error.is_null() && native["outputComplete"] == true,
                    error: save_error,
                });
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
    fn download(&mut self) -> Result<Outcome, Value> {
        let existing = self
            .task
            .target
            .admission(Kind::Download)
            .lock()
            .unwrap()
            .file
            .clone();
        let file = if let Some(file) = existing {
            file
        } else {
            let opened = self.app.rpc(
                "files.request",
                json!({"action":"open","viewId":self.task.target.view}),
            )?;
            let file = text(&opened, "id", 256)?.to_owned();
            self.task
                .target
                .admission(Kind::Download)
                .lock()
                .unwrap()
                .file = Some(file.clone());
            file
        };
        self.batch_file = Some(file.clone());
        self.app.rpc("files.request",json!({"id":file,"action":"batch.begin","protocolVersion":2,"batchId":self.task.id,"context":self.task.target.context}))?;
        let request = json!({
            "id":file,"action":"start","protocolVersion":2,"batchId":self.task.id,"requestId":self.task.id,
            "expectedContext":self.task.target.context,"sequence":self.task.admission_sequence.to_string(),
            "operation":{"kind":"copyFile","source":{"side":"remote","path":self.task.input["remotePath"]},
                "destination":{"side":"pluginData","relative":["evidence",self.task.run,self.task.id,"download.bin"]},
                "destinationGuard":{"kind":"absent"}},
        });
        let lookup = json!({"id":file,"action":"lookup","protocolVersion":2,"batchId":self.task.id,"requestId":self.task.id});
        let accepted = self.accept(
            "files.operations",
            request,
            "files.operations",
            lookup,
            "id",
        )?;
        let operation = text(&accepted, "id", 256)?.to_owned();
        self.native = Some(Native::Download {
            file: file.clone(),
            operation: operation.clone(),
        });
        let mut cancelled = false;
        loop {
            if self.app.shutdown.is_cancelled() || self.app.client.is_closed() {
                return Err(error("通信结束，最终结果未知"));
            }
            if self.app.stopped(self.task) && !cancelled {
                cancelled = self
                    .app
                    .rpc(
                        "files.operations",
                        json!({"id":file,"action":"cancel","operationId":operation}),
                    )
                    .is_ok();
            }
            let native = match self.app.rpc(
                "files.operations",
                json!({"id":file,"action":"poll","operationId":operation}),
            ) {
                Ok(value) => value,
                Err(e) => {
                    self.unavailable(e.clone());
                    return Err(e);
                }
            };
            self.task.observe(native.clone(), None);
            if ["completed", "failed", "cancelled", "unknown"]
                .contains(&native["state"].as_str().unwrap_or(""))
            {
                return Ok(Outcome {
                    complete: native["state"] == "completed",
                    error: Value::Null,
                });
            }
            thread::sleep(Duration::from_millis(100));
        }
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
                self.task.observe(value.clone(), None);
                self.task.target.release_admission(self.task.kind);
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
                    self.task.observe(value.clone(), None);
                    self.task.target.release_admission(self.task.kind);
                    return Ok(value);
                }
            }
            thread::sleep(Duration::from_millis(100));
        }
        Err(error("核心通信结束，接纳结果未知"))
    }
    fn unavailable(&self, error: Value) {
        let previous = self.task.snapshot().native.clone();
        self.task.observe(
            json!({"state":"unavailable","lastResult":previous,"error":error}),
            None,
        );
    }
    fn cleanup(&self) {
        let mut errors = Vec::new();
        let mut released = false;
        if let Some(native) = &self.native {
            let snapshot = self.task.snapshot();
            let ended = match native {
                Native::Command(_) => snapshot.native["workerFinished"] == true,
                Native::Download { .. } => ["completed", "failed", "cancelled", "unknown"]
                    .contains(&snapshot.native["state"].as_str().unwrap_or("")),
            };
            let mut ready = ended;
            if !ended {
                let cancel = match native {
                    Native::Command(id) => {
                        self.app.rpc("session.stream.close", json!({"streamId":id}))
                    }
                    Native::Download { file, operation } => self.app.rpc(
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
                            .rpc("session.stream.status", json!({"streamId":id})),
                        Native::Download { file, operation } => self.app.rpc(
                            "files.operations",
                            json!({"id":file,"action":"poll","operationId":operation}),
                        ),
                    };
                    match status {
                        Ok(value) => {
                            ready = match native {
                                Native::Command(_) => value["workerFinished"] == true,
                                Native::Download { .. } => {
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
                let result = match native {
                    Native::Command(id) => self
                        .app
                        .rpc("session.stream.release", json!({"streamId":id})),
                    Native::Download { file, operation } => self.app.rpc(
                        "files.operations",
                        json!({"id":file,"action":"release","operationId":operation}),
                    ),
                };
                released = result.is_ok();
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

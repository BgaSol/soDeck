use super::*;
use sodeck_plugin_client::protocol::requests as api;
impl Collector<'_> {
    pub(super) fn command(&mut self) -> Result<Outcome, Value> {
        let mut output =
            evidence::Output::new(&self.task.directory, self.task.terminal()).map_err(error)?;
        let spec = match &self.task.spec.operation {
            TaskOperation::Exec(spec) => spec,
            TaskOperation::File(_) => unreachable!(),
        };
        let io = match spec.mode {
            ExecMode::Batch => json!({"kind":"pipe","stdin":"closed"}),
            ExecMode::Pipe => json!({"kind":"pipe","stdin":"open"}),
            ExecMode::Pty => {
                let (cols, rows) = spec.terminal_size.unwrap();
                json!({"kind":"pty","cols":cols,"rows":rows})
            }
        };
        let request = json!({"viewId":self.task.target.view,"expectedContext":self.task.target.context,"operationId":self.task.id,"sequence":self.task.admission_sequence.to_string(),"command":spec.command,"timeoutMs":spec.timeout_seconds.map(|n|n*1000),"maxOutputBytes":16*1024*1024,"retention":"fixed","io":io});
        let accepted = self.accept(
            api::call::ProcessStart(
                serde_json::from_value(request.clone())
                    .map_err(|_| failure("invalidParams", "进程参数无效"))?,
            ),
            api::call::ProcessLookup(
                serde_json::from_value(request)
                    .map_err(|_| failure("invalidParams", "进程票据无效"))?,
            ),
            "processId",
        )?;
        let id = text(&accepted, "processId", 256)?.to_owned();
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
                cancelled = serde_json::from_value::<api::Process>(json!({"processId":id}))
                    .map_err(|_| failure("invalidParams", "核心请求参数无效"))
                    .and_then(|params| self.app.rpc(api::call::ProcessCancel(params)))
                    .is_ok();
            }
            let polled = match serde_json::from_value::<api::ProcessRead>(
                json!({"processId":id,"offset":offset.to_string()}),
            )
            .map_err(|_| failure("invalidParams", "核心请求参数无效"))
            .and_then(|params| self.app.rpc(api::call::ProcessRead(params)))
            {
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
}

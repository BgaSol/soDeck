use super::*;
impl Collector<'_> {
    pub(super) fn files(&mut self) -> Result<Outcome, Value> {
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
        let operation = match &self.task.spec.operation {
            TaskOperation::File(FileSpec::Download { remote_path }) => {
                json!({"kind":"copyFile","source":{"side":"remote","path":remote_path},"destination":{"side":"pluginData","relative":["evidence",self.task.run,self.task.id,"download.bin"]},"destinationGuard":{"kind":"absent"}})
            }
            TaskOperation::File(FileSpec::Upload {
                local_path,
                remote_path,
                overwrite,
            }) => {
                json!({"kind":"copyFile","source":{"side":"localPath","path":local_path},"destination":{"side":"remote","path":remote_path},"destinationGuard":{"kind":if *overwrite {"capture"}else{"absent"}},"metadataPolicy":"contentOnly"})
            }
            TaskOperation::Exec(_) => unreachable!(),
        };
        let request = json!({"id":file,"action":"start","protocolVersion":2,"batchId":self.task.id,"requestId":self.task.id,"expectedContext":self.task.target.context,"sequence":self.task.admission_sequence.to_string(),"operation":operation});
        let lookup = json!({"id":file,"action":"lookup","protocolVersion":2,"batchId":self.task.id,"requestId":self.task.id});
        let accepted = self.accept(
            "files.operations",
            request,
            "files.operations",
            lookup,
            "id",
        )?;
        let operation = text(&accepted, "id", 256)?.to_owned();
        self.native = Some(Native::File {
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
            if self.task.kind() == Kind::Upload
                && native["state"] == "completed"
                && native["transfer"]["commitState"] != "committed"
            {
                let fault = failure("invalidCopyReceipt", "核心未提供已确认的上传提交回执");
                self.unavailable(fault.clone());
                return Err(fault);
            }
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
}

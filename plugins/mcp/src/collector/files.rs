use super::*;
use sodeck_plugin_client::protocol::requests as api;
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
            let opened =
                serde_json::from_value::<api::FileOpen>(json!({"viewId":self.task.target.view}))
                    .map_err(|_| failure("invalidParams", "核心请求参数无效"))
                    .and_then(|params| self.app.rpc(api::call::FileOpen(params)))?;
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
        serde_json::from_value::<api::BatchBegin>(
            json!({"id":file,"batchId":self.task.id,"context":self.task.target.context}),
        )
        .map_err(|_| failure("invalidParams", "核心请求参数无效"))
        .and_then(|params| self.app.rpc(api::call::BatchBegin(params)))?;
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
        let request = json!({"id":file,"action":"start","batchId":self.task.id,"requestId":self.task.id,"expectedContext":self.task.target.context,"sequence":self.task.admission_sequence.to_string(),"operation":operation});
        let lookup =
            json!({"id":file,"action":"lookup","batchId":self.task.id,"requestId":self.task.id});
        let accepted = self.accept(
            api::call::FileOperation(
                serde_json::from_value(request)
                    .map_err(|_| failure("invalidParams", "文件参数无效"))?,
            ),
            api::call::FileOperation(
                serde_json::from_value(lookup)
                    .map_err(|_| failure("invalidParams", "文件票据无效"))?,
            ),
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
                cancelled = serde_json::from_value::<api::FileRequest>(
                    json!({"id":file,"action":"cancel","operationId":operation}),
                )
                .map_err(|_| failure("invalidParams", "核心请求参数无效"))
                .and_then(|params| self.app.rpc(api::call::FileOperation(params)))
                .is_ok();
            }
            let native = match serde_json::from_value::<api::FileRequest>(
                json!({"id":file,"action":"poll","operationId":operation}),
            )
            .map_err(|_| failure("invalidParams", "核心请求参数无效"))
            .and_then(|params| self.app.rpc(api::call::FileOperation(params)))
            {
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

use super::editor::{number, reference, same_request};
use super::*;
impl AppCore {
    pub fn files_export_begin(
        self: &Arc<Self>,
        window: &str,
        token: &str,
        p: &Value,
    ) -> Result<(Value, Option<ExportTask>), String> {
        self.export_begin(window, token, p)
    }
    fn export_begin(
        self: &Arc<Self>,
        window: &str,
        token: &str,
        p: &Value,
    ) -> Result<(Value, Option<ExportTask>), String> {
        let v = self.ui_data_view(window, token)?;
        let r = self.file_resource(&v.plugin_id, &v.plugin_instance_id, string(p, "id")?)?;
        if r.owner.view != v.id {
            return Err("文件资源不属于此页面".into());
        }
        let _admission = self.surface_write_admission(window, token)?;
        let mut e = r.editor.lock().unwrap();

        if same_request(&e.export["request"], p)? {
            return Ok((e.export.clone(), None));
        }
        if !e.export.is_null() {
            return Err("请先确认并释放上次导出回执".into());
        }
        let sequence = number(p, "sequence")?;
        if sequence <= e.export_sequence {
            return Err("expired: 导出序号已退役".into());
        }
        if p["requestId"]
            .as_str()
            .is_none_or(|s| s.is_empty() || s.len() > 160)
        {
            return Err("导出身份无效".into());
        }
        let doc = e.document.as_ref().ok_or("尚未打开文件")?;
        if p["expectedDraftRef"] != reference(doc) {
            return Err("revisionConflict: 草稿版本已变化".into());
        }
        let bytes = doc.bytes.clone();
        let task = r.tasks.enter()?;
        e.export_cancel = r.cancel.child_token();
        let cancel = e.export_cancel.clone();
        e.export_sequence = sequence;
        e.export = json!({"state":"waitingInput", "request":p, "draftRef":p["expectedDraftRef"]});
        let receipt = e.export.clone();
        drop(e);
        Ok((
            receipt,
            Some(ExportTask {
                resource: r,
                bytes,
                cancel,
                _task: task,
                window: window.into(),
                surface: token.into(),
            }),
        ))
    }
    pub async fn files_export_finish(
        &self,
        task: ExportTask,
        chosen: Result<Option<PathBuf>, String>,
    ) -> Value {
        let r = &task.resource;
        let result = async {
            let Some(local) = chosen? else {
                return Ok(json!({"state":"cancelled"}));
            };
            if task.cancel.is_cancelled() {
                return Ok(json!({"state":"cancelled"}));
            }
            drop(self.surface_write_admission(&task.window, &task.surface)?);
            r.editor.lock().unwrap().export["state"] = json!("writing");
            let temporary = LocalTemporary(
                local
                    .parent()
                    .ok_or("目标没有父目录")?
                    .join(format!(".sodeck-{}.part", self.next_id())),
            );
            let before = std::fs::metadata(&local)
                .ok()
                .map(|m| (m.len(), m.modified().ok()));
            let mut output = tokio::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary.0)
                .await
                .map_err(err)?;
            output.write_all(&task.bytes).await.map_err(err)?;
            output.sync_all().await.map_err(err)?;
            drop(output);
            let _admission = self.surface_write_admission(&task.window, &task.surface)?;
            if task.cancel.is_cancelled() {
                return Ok(json!({"state":"cancelled"}));
            }
            if std::fs::metadata(&local)
                .ok()
                .map(|m| (m.len(), m.modified().ok()))
                != before
            {
                return Err("本地目标已变化，未覆盖".to_string());
            }
            std::fs::rename(&temporary.0, &local).map_err(err)?;
            Ok(json!({"state":"completed", "path":local, "size":task.bytes.len()}))
        }
        .await;
        let mut e = r.editor.lock().unwrap();
        let mut result = result.unwrap_or_else(|error| json!({"state":"failed", "error":error}));
        result["request"] = e.export["request"].clone();
        result["draftRef"] = e.export["draftRef"].clone();
        e.export = result.clone();
        result
    }
}
pub struct ExportTask {
    resource: Arc<Resource>,
    bytes: Arc<[u8]>,
    cancel: CancellationToken,
    _task: crate::resource_tasks::Task,
    window: String,
    surface: String,
}

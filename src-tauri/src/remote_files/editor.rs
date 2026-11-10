//! In-memory editor facts; no network IO while holding this state.
use super::*;

#[derive(Default)]
pub(super) struct Editor {
    pub document: Option<Document>,
    pub legacy_upload: Option<Vec<u8>>,
    pub protocol: bool,
    upload: Option<Upload>,
    surface: String,
    sequence: u64,
    committed: Value,
    pub saved: u64,
    pub write_block: Option<String>,
    pub save_sequence: u64,
    pub save_request: Value,
    pub save_result: Value,
    pub export_sequence: u64,
    pub export: Value,
    pub export_cancel: CancellationToken,
}
struct Upload {
    id: String,
    sequence: u64,
    edit_revision: u64,
    size: usize,
    digest: String,
    bytes: Vec<u8>,
}
pub(super) fn number(p: &Value, key: &str) -> Result<u64, String> {
    p[key]
        .as_str()
        .and_then(|s| s.parse().ok())
        .or_else(|| p[key].as_u64())
        .ok_or_else(|| format!("缺少或无效 {key}"))
}
pub(super) fn reference(d: &Document) -> Value {
    json!({"documentId":d.id,"revision":d.revision.to_string()})
}
impl Editor {
    pub fn describe(&self) -> Value {
        self.document.as_ref().map(|d|json!({"path":d.path,"size":d.bytes.len(),"version":d.version,"revision":d.revision,"draftRef":reference(d),"savedRevision":self.saved.to_string(),"writeBlock":self.write_block,"saveSequence":self.save_sequence.to_string(),"exportSequence":self.export_sequence.to_string(),"exportState":self.export["state"],"source":self.committed})).unwrap_or(json!({}))
    }
    pub fn draft(&mut self, surface: &str, p: &Value) -> Result<Value, String> {
        let action = string(p, "action")?;
        let doc = self.document.as_ref().ok_or("尚未打开文件")?;
        if action == "draft.describe" {
            return Ok(self.describe());
        }
        if action == "draft.chunk" {
            if p["draftRef"] != reference(doc) {
                return Err("revisionConflict: 草稿版本已变化".into());
            }
            let offset = number(p, "offset")? as usize;
            if offset > doc.bytes.len() {
                return Err("草稿偏移无效".into());
            }
            return Ok(
                json!({"data":&doc.bytes[offset..(offset+BLOCK).min(doc.bytes.len())],"draftRef":reference(doc)}),
            );
        }
        if action == "draft.status" {
            if self.committed["uploadId"] == p["uploadId"] && p["uploadId"].is_string() {
                return Ok(self.committed.clone());
            }
            return Ok(self
                .upload
                .as_ref()
                .filter(|u| p["uploadId"] == u.id)
                .map(|u| json!({"state":"pending","nextOffset":u.bytes.len()}))
                .unwrap_or(json!({"state":"expired"})));
        }
        if p["documentId"] != doc.id {
            return Err("revisionConflict: 文档身份已变化".into());
        }
        let id = string(p, "uploadId")?;
        if id.is_empty() || id.len() > 160 {
            return Err("上传身份无效".into());
        }
        let sequence = number(p, "uploadSequence")?;
        if self.surface != surface {
            self.surface = surface.into();
            self.sequence = 0;
            self.upload = None;
        }
        if action == "draft.abort" {
            self.sequence = self.sequence.max(sequence);
            if self.upload.as_ref().is_some_and(|u| u.id == id) {
                self.upload = None;
            }
            return Ok(json!({"aborted":true}));
        }
        if self.committed["uploadId"] == id {
            if self.committed["surfaceId"] != surface
                || self.committed["uploadSequence"] != sequence.to_string()
            {
                return Err("上传身份不一致".into());
            }
            if action == "draft.commit" {
                return Ok(self.committed.clone());
            }
            if action == "draft.begin"
                && (self.committed["sha256"] != p["sha256"]
                    || self.committed["editRevision"] != p["editRevision"]
                    || self.committed["size"] != p["size"])
            {
                return Err("相同上传身份内容变化".into());
            }
            if action == "draft.append" {
                let bytes: Vec<u8> = serde_json::from_value(p["data"].clone()).map_err(err)?;
                let offset = number(p, "offset")? as usize;
                let end = offset.checked_add(bytes.len()).ok_or("草稿偏移溢出")?;
                if bytes.len() > BLOCK || doc.bytes.get(offset..end) != Some(bytes.as_slice()) {
                    return Err("重复分片内容不同".into());
                }
            } else if action != "draft.begin" {
                return Err("未知草稿操作".into());
            }
            return Ok(self.committed.clone());
        }
        if action == "draft.begin" {
            let size = number(p, "size")? as usize;
            let hash = string(p, "sha256")?;
            let revision = number(p, "editRevision")?;
            if size > LIMIT || hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err("草稿超过 1 MiB 或摘要无效".into());
            }
            if let Some(u) = self.upload.as_ref().filter(|u| u.id == id) {
                if u.sequence != sequence
                    || u.size != size
                    || u.digest != hash
                    || u.edit_revision != revision
                {
                    return Err("相同上传身份内容变化".into());
                }
                return Ok(json!({"state":"pending","nextOffset":u.bytes.len()}));
            }
            if sequence <= self.sequence {
                return Err("expired: 上传序号已退役".into());
            }
            self.protocol = true;
            self.sequence = sequence;
            self.upload = Some(Upload {
                id: id.into(),
                sequence,
                edit_revision: revision,
                size,
                digest: hash.into(),
                bytes: Vec::with_capacity(size),
            });
            return Ok(json!({"state":"pending","nextOffset":0}));
        }
        let u = self
            .upload
            .as_mut()
            .filter(|u| u.id == id && u.sequence == sequence)
            .ok_or("expired: 上传已失效")?;
        if action == "draft.append" {
            let bytes: Vec<u8> = serde_json::from_value(p["data"].clone()).map_err(err)?;
            let offset = number(p, "offset")? as usize;
            let end = offset.checked_add(bytes.len()).ok_or("草稿偏移溢出")?;
            if bytes.len() > BLOCK || end > u.size || offset > u.bytes.len() {
                return Err("草稿分片越界".into());
            }
            if offset == u.bytes.len() {
                u.bytes.extend(bytes);
            } else if end > u.bytes.len() || u.bytes[offset..end] != bytes {
                return Err("重复分片内容不同".into());
            }
            return Ok(json!({"state":"pending","nextOffset":u.bytes.len()}));
        }
        if action != "draft.commit" {
            return Err("未知草稿操作".into());
        }
        if u.bytes.len() != u.size || digest(&u.bytes) != u.digest {
            return Err("草稿大小或摘要不匹配".into());
        }
        let revision = doc.revision.checked_add(1).ok_or("草稿修订已用尽")?;
        let u = self.upload.take().unwrap();
        let d = self.document.as_mut().unwrap();
        d.bytes = u.bytes.into();
        d.revision = revision;
        self.committed = json!({"state":"committed","draftRef":reference(d),"uploadId":u.id,"uploadSequence":u.sequence.to_string(),"surfaceId":surface,"editRevision":u.edit_revision.to_string(),"sha256":u.digest,"size":u.size});
        Ok(self.committed.clone())
    }
}

pub(super) fn pending(state: &Value) -> bool {
    ["running", "committing", "waitingInput", "writing"]
        .contains(&state["state"].as_str().unwrap_or(""))
}
pub(super) fn legacy_job(mut job: Value) -> Value {
    if job["state"] == "committing" {
        job["state"] = json!("running");
    }
    if job["state"] == "unknown" {
        job["state"] = json!("failed");
    }
    job
}
fn same_request(saved: &Value, p: &Value) -> Result<bool, String> {
    if p["requestId"].is_string() && saved["requestId"] == p["requestId"] {
        for key in ["sequence", "expectedDraftRef", "expectedContext"] {
            if saved[key] != p[key] {
                return Err("相同请求身份参数变化".into());
            }
        }
        return Ok(true);
    }
    Ok(false)
}
impl AppCore {
    pub(crate) fn files_editor(
        self: &Arc<Self>,
        plugin: &str,
        instance: &str,
        p: &Value,
    ) -> Result<Value, String> {
        let r = self.file_resource(plugin, instance, string(p, "id")?)?;
        match string(p, "action")? {
            "draft.describe" => Ok(r.editor.lock().unwrap().describe()),
            "save.start" => {
                {
                    let e = r.editor.lock().unwrap();
                    if same_request(&e.save_request, p)? {
                        return editor_status_locked(&e);
                    }
                }
                let context = self
                    .ui_state()
                    .resource_view(&r.owner.view)?
                    .context
                    .clone();
                self.start_file_job(r.clone(), context, None, Some(p))?;
                editor_status(&r, p)
            }
            "save.status" => editor_status(&r, p),
            "save.cancel" => {
                let mut e = r.editor.lock().unwrap();
                if e.save_request.is_null() && number(p, "sequence")? > e.save_sequence {
                    string(p, "requestId")?;
                    e.save_sequence = number(p, "sequence")?;
                    e.save_request = p.clone();
                    e.save_result = json!({"state":"failed","error":"保存已取消；请求未被接纳"});
                }
                verify_receipt(&e.save_request, p)?;
                if pending(&e.save_result) {
                    r.job_cancel.lock().unwrap().cancel();
                }
                editor_status_locked(&e)
            }
            "save.release" => {
                let mut e = r.editor.lock().unwrap();
                verify_receipt(&e.save_request, p)?;
                if pending(&e.save_result) {
                    return Err("保存尚未结束".into());
                }
                e.save_request = Value::Null;
                e.save_result = Value::Null;
                Ok(json!({"released":true}))
            }
            "export.status" | "export.cancel" | "export.release" => {
                let mut e = r.editor.lock().unwrap();
                if p["action"] == "export.cancel"
                    && e.export.is_null()
                    && number(p, "sequence")? > e.export_sequence
                {
                    string(p, "requestId")?;
                    e.export_sequence = number(p, "sequence")?;
                    e.export = json!({"state":"cancelled","request":p});
                }
                verify_receipt(&e.export["request"], p)?;
                if p["action"] == "export.cancel" {
                    e.export_cancel.cancel();
                }
                if p["action"] == "export.release" {
                    if pending(&e.export) {
                        return Err("导出对话框或写入尚未结束".into());
                    }
                    e.export = Value::Null;
                    return Ok(json!({"released":true}));
                }
                Ok(e.export.clone())
            }
            _ => Err("未知编辑器控制方法".into()),
        }
    }
    pub fn files_export_legacy_begin(
        self: &Arc<Self>,
        window: &str,
        token: &str,
        p: &Value,
    ) -> Result<ExportTask, String> {
        let v = self.ui_data_view(window, token)?;
        let r = self.file_resource(&v.plugin_id, &v.plugin_instance_id, string(p, "id")?)?;
        let request = {
            let e = r.editor.lock().unwrap();
            json!({"id":p["id"],"requestId":self.next_id(),"sequence":e.export_sequence.checked_add(1).ok_or("导出序号已用尽")?.to_string(),"expectedDraftRef":e.describe()["draftRef"]})
        };
        self.export_begin(window, token, &request, false)?
            .1
            .ok_or("导出未接纳".into())
    }
    pub fn files_export_begin(
        self: &Arc<Self>,
        window: &str,
        token: &str,
        p: &Value,
    ) -> Result<(Value, Option<ExportTask>), String> {
        self.export_begin(window, token, p, true)
    }
    fn export_begin(
        self: &Arc<Self>,
        window: &str,
        token: &str,
        p: &Value,
        protocol: bool,
    ) -> Result<(Value, Option<ExportTask>), String> {
        let v = self.ui_data_view(window, token)?;
        let r = self.file_resource(&v.plugin_id, &v.plugin_instance_id, string(p, "id")?)?;
        if r.owner.view != v.id {
            return Err("文件资源不属于此页面".into());
        }
        let _admission = self.surface_write_admission(window, token)?;
        let mut e = r.editor.lock().unwrap();
        if !protocol && e.protocol {
            return Err("新版草稿必须按固定修订导出".into());
        }
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
        e.protocol |= protocol;
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
        e.export = if e.protocol {
            result.clone()
        } else {
            Value::Null
        };
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
fn verify_receipt(saved: &Value, p: &Value) -> Result<(), String> {
    if !p["requestId"].is_string() || saved["requestId"] != p["requestId"] {
        return Err("expired: 回执不存在或已退役".into());
    }
    Ok(())
}
pub(super) fn editor_status(r: &Resource, p: &Value) -> Result<Value, String> {
    let e = r.editor.lock().unwrap();
    if p["requestId"].is_string() {
        verify_receipt(&e.save_request, p)?;
    }
    editor_status_locked(&e)
}
fn editor_status_locked(e: &Editor) -> Result<Value, String> {
    let mut job = if e.save_result.is_null() {
        json!({"state":"idle"})
    } else {
        e.save_result.clone()
    };
    if job["state"] == "completed" {
        job["state"] = json!("committed");
    }
    if job["state"] == "failed" {
        job["state"] = json!("notCommitted");
    }
    job["request"] = e.save_request.clone();
    job["draft"] = e.describe();
    Ok(job)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn editor() -> Editor {
        Editor {
            document: Some(Document {
                id: "doc".into(),
                path: "/a".into(),
                resolved: "/a".into(),
                version: "base".into(),
                attrs: FileAttributes::default(),
                bytes: Arc::from(b"old".as_slice()),
                revision: 0,
            }),
            ..Editor::default()
        }
    }
    fn request(action: &str) -> Value {
        json!({"action":action,"documentId":"doc","uploadId":"upload","uploadSequence":"1","editRevision":"1","size":3,"sha256":digest(b"new")})
    }
    #[test]
    fn retry_commit_preserves_exact_revision_and_rejects_mixed_chunks() {
        let mut e = editor();
        e.draft("surface", &request("draft.begin")).unwrap();
        let mut chunk = request("draft.append");
        chunk["offset"] = json!(0);
        chunk["data"] = json!([110, 101, 119]);
        e.draft("surface", &chunk).unwrap();
        e.draft("surface", &chunk).unwrap();
        chunk["data"] = json!([111, 108, 100]);
        assert!(e.draft("surface", &chunk).is_err());
        let first = e.draft("surface", &request("draft.commit")).unwrap();
        assert!(e.draft("surface", &chunk).is_err());
        chunk["data"] = json!([110, 101, 119]);
        assert_eq!(first, e.draft("surface", &chunk).unwrap());
        assert_eq!(first, e.draft("surface", &request("draft.commit")).unwrap());
        assert_eq!(e.document.unwrap().revision, 1);
    }
    #[test]
    fn cancelled_upload_never_restarts_and_snapshot_is_immutable() {
        let mut e = editor();
        let before = e.document.as_ref().unwrap().clone();
        e.draft("surface", &request("draft.abort")).unwrap();
        assert!(e.draft("surface", &request("draft.begin")).is_err());
        assert_eq!(&*before.bytes, b"old");
    }
}

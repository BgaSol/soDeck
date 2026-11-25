use super::*;
impl AppCore {
    pub(super) fn start_save(
        self: &Arc<Self>,
        r: Arc<Resource>,
        context: Context,
        p: &Value,
    ) -> Result<Value, String> {
        {
            let e = r.editor.lock().unwrap();
            if e.save_request["requestId"] == p["requestId"] && p["requestId"].is_string() {
                for key in ["sequence", "expectedDraftRef", "expectedContext"] {
                    if e.save_request[key] != p[key] {
                        return Err("相同保存身份参数变化".into());
                    }
                }
                return Ok(e.save_result.clone());
            }
        }
        let task = r.tasks.enter()?;
        self.connection_session(&context)?;
        let ui = self.ui_state();
        let view = ui.resource_view(&r.owner.view)?;
        if view.context != context || view.plugin_instance_id != r.owner.instance {
            return Err("文件目标上下文已改变".into());
        }
        let close_step = if self.is_closing() || ui.lifecycle.view_frozen(&view.id) {
            Some(
                ui.lifecycle
                    .operations
                    .values()
                    .find(|o| {
                        o.adopted_by.is_none()
                            && o.phase == crate::lifecycle::Phase::Deciding
                            && o.decision.as_deref() == Some("save")
                            && o.working
                            && o.saving_view.as_deref() == Some(r.owner.view.as_str())
                    })
                    .ok_or("此文件保存不属于当前关闭步骤")?
                    .id
                    .clone(),
            )
        } else {
            None
        };
        let mut close_saves = r.close_saves.lock().unwrap();
        if let Some(result) = close_step.as_ref().and_then(|id| close_saves.get(id)) {
            return if r.job.lock().unwrap()["id"] == result["id"] {
                Ok(r.job.lock().unwrap().clone())
            } else {
                Err("关闭保存回执已退役".into())
            };
        }
        let mut editor = r.editor.lock().unwrap();
        {
            if editor.save_request["requestId"] == p["requestId"] && p["requestId"].is_string() {
                for key in ["sequence", "expectedDraftRef", "expectedContext"] {
                    if editor.save_request[key] != p[key] {
                        return Err("相同保存身份参数变化".into());
                    }
                }
                return Ok(editor.save_result.clone());
            }
            if !editor.save_request.is_null() {
                return Err("请先确认并释放上次保存回执".into());
            }
        }
        let snapshot = {
            if let Some(reason) = &editor.write_block {
                return Err(reason.clone());
            }
            let d = editor.document.as_ref().ok_or("尚未打开文件")?.clone();
            {
                if p["expectedDraftRef"] != editor::reference(&d) {
                    return Err("revisionConflict: 草稿版本已变化".into());
                }
                if p["expectedContext"] != serde_json::to_value(&context).map_err(err)? {
                    return Err("保存上下文已变化".into());
                }
                let sequence = editor::number(p, "sequence")?;
                if sequence <= editor.save_sequence {
                    return Err("expired: 保存序号已退役".into());
                }
                if p["requestId"]
                    .as_str()
                    .is_none_or(|s| s.is_empty() || s.len() > 160)
                {
                    return Err("保存请求身份无效".into());
                }
            }
            d
        };
        let id = self.next_id();
        let cancel = r.cancel.child_token();
        {
            let mut job = r.job.lock().unwrap();
            if ["running", "committing"].contains(&job["state"].as_str().unwrap_or("")) {
                return Err("文件任务正在进行".into());
            }
            // Publish the cancellation token before callers can observe running.
            *r.job_cancel.lock().unwrap() = cancel.clone();
            *job = json!({"id":id,"state":"running","kind":"save","received":"0"});
        }
        {
            editor.save_sequence = editor::number(p, "sequence")?;
            editor.save_request = p.clone();
            editor.save_result = r.job.lock().unwrap().clone();
        }
        drop(editor);
        if let Some(step) = close_step {
            close_saves.insert(step, json!({"id":id,"state":"running"}));
        }
        drop(close_saves);
        drop(ui);
        let core = self.clone();
        let job_id = id.clone();
        tauri::async_runtime::spawn(async move {
            let _task = task;
            let result = save_file(&core, &r, &context, &cancel, snapshot).await;
            let result = match result {
                Ok(data) => {
                    json!({"id":job_id,"kind":"save","state":"committed","result":data})
                }
                Err(error) => {
                    json!({"id":job_id,"kind":"save","state":if error.unknown{"unknown"}else{"notCommitted"},"error":error.message})
                }
            };
            let mut editor = r.editor.lock().unwrap();
            editor.save_result = result.clone();
            *r.job.lock().unwrap() = result;
        });
        Ok(json!({"id":id,"state":"running"}))
    }
}
#[derive(Debug)]
pub(super) struct SaveFailure {
    pub(super) unknown: bool,
    message: String,
}
impl From<String> for SaveFailure {
    fn from(message: String) -> Self {
        Self {
            unknown: false,
            message,
        }
    }
}
impl From<&str> for SaveFailure {
    fn from(message: &str) -> Self {
        message.to_owned().into()
    }
}
async fn save_file(
    core: &Arc<AppCore>,
    r: &Resource,
    context: &Context,
    cancel: &CancellationToken,
    doc: Document,
) -> Result<Value, SaveFailure> {
    let mut state = State {
        transports: r.transports.clone(),
        ..State::default()
    };
    let s = tokio::select! {
        biased;
        _ = cancel.cancelled() => return Err("保存已取消，草稿保留".into()),
        session = sftp(core, &mut state, context) => session?,
    };
    drop(state);
    if !s.atomic {
        return Err("服务器不支持原子替换，未修改原文件；请下载后通过其他方式保存".into());
    }
    let temporary = format!("{}.sodeck-{}.tmp", doc.resolved, core.next_id());
    let work = async {
        let verify = async {
            if canonical(&s.raw, &doc.path).await? != doc.resolved {
                return Err("文件链接目标已经变化，草稿保留".to_string());
            }
            let attrs = s.raw.lstat(doc.resolved.clone()).await.map_err(err)?.attrs;
            if !attrs.file_type().is_file() {
                return Err("文件类型发生变化，草稿保留".into());
            }
            let current = read_small(&s.raw, &doc.resolved, &attrs, |n| {
                core.record_business(&r.owner, 0, n as u64)
            })
            .await?;
            if version(&attrs, &current) != doc.version {
                return Err("远端文件已被修改，未覆盖；请下载草稿并重新打开比较".into());
            }
            Ok(())
        };
        verify.await?;
        let handle = s
            .raw
            .open(
                temporary.clone(),
                OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::EXCLUDE,
                FileAttributes {
                    permissions: Some(0o600),
                    ..Default::default()
                },
            )
            .await
            .map_err(err)?
            .handle;
        let written = async {
            for (i, block) in doc.bytes.chunks(BLOCK).enumerate() {
                s.raw
                    .write(handle.clone(), (i * BLOCK) as u64, block.to_vec())
                    .await
                    .map_err(err)?;
                core.record_business(&r.owner, block.len() as u64, 0);
            }
            s.raw
                .fsetstat(
                    handle.clone(),
                    FileAttributes {
                        permissions: doc.attrs.permissions.map(|p| p & 0o7777),
                        ..Default::default()
                    },
                )
                .await
                .map_err(err)?;
            Ok::<_, String>(())
        }
        .await;
        let closed = s.raw.close(handle).await.map_err(err);
        written?;
        closed?;
        if canonical(&s.raw, &doc.path).await? != doc.resolved {
            return Err("文件链接目标已经变化，未替换原文件".into());
        }
        let a = s.raw.lstat(doc.resolved.clone()).await.map_err(err)?.attrs;
        let bytes = read_small(&s.raw, &doc.resolved, &a, |n| {
            core.record_business(&r.owner, 0, n as u64)
        })
        .await?;
        if version(&a, &bytes) != doc.version {
            return Err("远端文件已被修改，未覆盖；草稿保留".into());
        }
        Ok::<_, String>(())
    };
    let prepared = tokio::select! {biased;_=cancel.cancelled()=>Err("保存已取消，草稿保留".into()),v=tokio::time::timeout(Duration::from_secs(120),work)=>v.map_err(|_|"保存准备超时".to_string()).and_then(|v|v)};
    if let Err(error) = prepared {
        let _ = s.raw.close_session();
        return Err(format!("{error}；如已创建临时文件则保留在 {temporary}").into());
    }
    if cancel.is_cancelled() {
        let _ = s.raw.remove(temporary).await;
        return Err("保存已取消，未提交".into());
    }
    r.job.lock().unwrap()["state"] = json!("committing");
    r.editor.lock().unwrap().save_result["state"] = json!("committing");
    let attrs = match commit_file(&s.raw, &temporary, &doc.resolved).await {
        Ok(attrs) => attrs,
        Err(error) => {
            if error.unknown {
                r.editor.lock().unwrap().write_block = Some(error.message.clone());
            }
            return Err(error);
        }
    };
    let ui = core.ui_state();
    let current = ui
        .resource_view(&r.owner.view)
        .is_ok_and(|v| v.context == *context);
    let mut editor = r.editor.lock().unwrap();
    let matches = editor
        .document
        .as_ref()
        .is_some_and(|d| d.id == doc.id && d.version == doc.version);
    if current && matches {
        editor.saved = doc.revision;
    }
    let mut result = json!({"revision":doc.revision,"draftRef":editor::reference(&doc),"size":doc.bytes.len(),"committed":true});
    if let Some(a) = attrs.filter(|_| current && matches) {
        let version = version(&a, &doc.bytes);
        let d = editor.document.as_mut().unwrap();
        d.version = version.clone();
        d.attrs = a;
        result["version"] = json!(version);
    } else {
        let warning =
            "该修订已提交，但后续基线或连接身份未确认；请核对后重新打开，期间可以导出草稿";
        editor.write_block = Some(warning.into());
        result["baselineUnavailable"] = json!(true);
        result["warning"] = json!(warning);
    }
    Ok(result)
}

// This phase has no cancellation select: sending a commit cannot be undone locally.
pub(super) async fn commit_file(
    raw: &RawSftpSession,
    temporary: &str,
    destination: &str,
) -> Result<Option<FileAttributes>, SaveFailure> {
    let mut data = Vec::new();
    ssh_string(&mut data, temporary).map_err(|e| e.to_string())?;
    ssh_string(&mut data, destination).map_err(|e| e.to_string())?;
    match raw.extended("posix-rename@openssh.com", data).await {
        Ok(Packet::Status(status)) if status.status_code == StatusCode::Ok => (),
        Ok(Packet::Status(status)) | Err(Error::Status(status))
            if !matches!(
                status.status_code,
                StatusCode::NoConnection | StatusCode::ConnectionLost
            ) =>
        {
            let _ = raw.remove(temporary).await;
            return Err(format!("服务器拒绝保存：{}", status.error_message).into());
        }
        other => {
            let message = format!("提交结果未知，请核对服务器；草稿保留，不能直接重试：{other:?}");
            let _ = raw.close_session();
            return Err(SaveFailure {
                unknown: true,
                message,
            });
        }
    }
    Ok(raw.stat(destination).await.ok().map(|r| r.attrs))
}

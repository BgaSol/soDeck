use super::*;
mod destination;
mod metadata;
mod request;
mod state;
use request::CopyRequest;
use sodeck_plugin_protocol::replies::CopyPhase;
use state::CopyProgress;

enum Reader {
    Local(crate::local_files::ReadSource),
    Remote(String),
}
enum Writer {
    Local(tokio::fs::File),
    Remote(String),
}
impl Io {
    pub(super) async fn copy(&self, p: &Value) -> Result<Value, OperationError> {
        let mut progress = CopyProgress::new(self.operation.clone());
        let result = self.copy_inner(p, &mut progress).await;
        match result {
            Ok(value) => {
                progress.phase(CopyPhase::Finished, "finished");
                Ok(value)
            }
            Err(error) => Err(progress.error(error)),
        }
    }
    async fn copy_inner(
        &self,
        p: &Value,
        progress: &mut CopyProgress,
    ) -> Result<Value, OperationError> {
        let request = CopyRequest::parse(p)?;
        let host_source = p["source"]["side"] == "localPath";
        let source = self.endpoint(&p["source"])?;
        let destination = self.endpoint(&p["destination"])?;
        let source_parent = if matches!(source, Endpoint::Remote(_)) {
            Some(self.parent_identity(&source).await?)
        } else {
            None
        };
        let destination_parent = self.parent_identity(&destination).await?;
        let guard = if request.capture {
            match self.inspect(&destination).await? {
                None => json!({"kind":"absent"}),
                Some(attrs) if attrs["kind"] == "file" => {
                    json!({"kind":"match","stamp":attrs["stamp"]})
                }
                Some(_) => return Err(failure("typeConflict", "目标不是普通文件")),
            }
        } else {
            p["destinationGuard"].clone()
        };
        let before = self.inspect(&source).await?.ok_or_else(|| {
            failure(
                if host_source {
                    "sourceNotFound"
                } else {
                    "missing"
                },
                "源文件不存在",
            )
        })?;
        if before["kind"] != "file" {
            return Err(failure(
                if host_source {
                    "sourceNotFile"
                } else {
                    "unsupported"
                },
                "只复制普通文件，符号链接与特殊条目由批次跳过",
            ));
        }
        if !p["sourceStamp"].is_null() && p["sourceStamp"] != before["stamp"] {
            return Err(failure("sourceChanged", "源文件已变化"));
        }
        let original = self.check_guard(&destination, &guard).await?;
        if original.as_ref().is_some_and(|a| a["kind"] != "file") {
            return Err(failure("typeConflict", "目标不是普通文件"));
        }
        if matches!((&source,&destination),(Endpoint::Remote(a),Endpoint::Remote(b))if a==b)
            || matches!((&source,&destination),(Endpoint::Local(a),Endpoint::Local(b))if a==b)
        {
            return Err(failure("invalid", "不能覆盖源文件自身"));
        }
        if original.is_some() && matches!(destination, Endpoint::Remote(_)) && !self.session.atomic
        {
            return Err(failure("unsupported", "服务器不支持原子替换，未覆盖原文件"));
        }
        let temporary = match &destination {
            Endpoint::Local(path) => Endpoint::Local(
                path.parent()
                    .ok_or("目标没有父目录")?
                    .join(format!(".sodeck-{}.part", self.operation.id)),
            ),
            Endpoint::Remote(path) => {
                let parent = path
                    .rsplit_once('/')
                    .map(|v| if v.0.is_empty() { "/" } else { v.0 })
                    .ok_or("目标需要完整路径")?;
                Endpoint::Remote(format!(
                    "{}/.sodeck-{}.part",
                    parent.trim_end_matches('/'),
                    self.operation.id
                ))
            }
        };
        let mut reader = match &source {
            Endpoint::Local(path) => Reader::Local(
                crate::local_files::ReadSource::open(path.clone())
                    .await
                    .map_err(|e| {
                        if host_source {
                            failure("sourceUnreadable", e)
                        } else {
                            local_error(e)
                        }
                    })?,
            ),
            Endpoint::Remote(path) => Reader::Remote(
                self.session
                    .raw
                    .open(path, OpenFlags::READ, FileAttributes::default())
                    .await
                    .map_err(err)?
                    .handle,
            ),
        };
        progress.source(&before, &reader);
        let writer = match &temporary {
            Endpoint::Local(path) => tokio::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .await
                .map(Writer::Local)
                .map_err(err),
            Endpoint::Remote(path) => self
                .session
                .raw
                .open(
                    path,
                    OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::EXCLUDE,
                    FileAttributes {
                        permissions: request.content_only.then_some(0o600),
                        ..Default::default()
                    },
                )
                .await
                .map(|h| Writer::Remote(h.handle))
                .map_err(err),
        };
        let mut writer = match writer {
            Ok(w) => w,
            Err(e) => {
                if let Reader::Remote(h) = reader {
                    let _ = self.session.raw.close(h).await;
                }
                return Err(e);
            }
        };
        progress.phase(CopyPhase::Transferring, "read");
        let mut offset = 0u64;
        let streaming = async {
            let mut buffer = vec![0u8; 64 * 1024];
            loop {
                if self.operation.cancel.is_cancelled() {
                    return Err(failure("cancelled", "传输已取消，原目标保留"));
                }
                self.core.connection_session(&self.owner.context)?;
                progress.stage("read");
                let block = match &mut reader {
                    Reader::Local(file) => {
                        let n = file.file.read(&mut buffer).await.map_err(local_error)?;
                        if n == 0 {
                            break;
                        }
                        buffer[..n].to_vec()
                    }
                    Reader::Remote(handle) => match self
                        .session
                        .raw
                        .read(handle.clone(), offset, buffer.len() as u32)
                        .await
                    {
                        Ok(data) => {
                            if data.data.is_empty() {
                                return Err(failure("io", "远程读取意外返回空块"));
                            }
                            self.core
                                .record_business(&self.owner, 0, data.data.len() as u64);
                            data.data
                        }
                        Err(e) if eof(&e) => break,
                        Err(e) => return Err(err(e)),
                    },
                };
                progress.stage("write");
                match &mut writer {
                    Writer::Local(file) => file.write_all(&block).await.map_err(local_error)?,
                    Writer::Remote(handle) => {
                        self.session
                            .raw
                            .write(handle.clone(), offset, block.clone())
                            .await
                            .map_err(err)?;
                        self.core
                            .record_business(&self.owner, block.len() as u64, 0);
                    }
                }
                offset += block.len() as u64;
                progress.bytes(offset, before["size"].clone());
            }
            progress.phase(CopyPhase::Finalizing, "verifySource");
            if before["size"]
                .as_str()
                .and_then(|s| s.parse::<u64>().ok())
                .is_some_and(|n| n != offset)
            {
                return Err(failure("sourceChanged", "源文件大小在传输期间变化"));
            }
            if let Writer::Local(file) = &mut writer {
                progress.stage("flush");
                file.flush().await.map_err(local_error)?;
                file.sync_all().await.map_err(local_error)?;
            }
            Ok::<_, OperationError>(())
        };
        // Each issued write finishes before cancellation is acted on.
        let result = streaming.await;
        if let Reader::Remote(handle) = &reader {
            let _ = self.session.raw.close(handle.clone()).await;
        }
        let result = result.map_err(|e| progress.error(e));
        if result.is_ok() {
            progress.stage("close");
        }
        let closed = match writer {
            Writer::Remote(handle) => self
                .session
                .raw
                .close(handle)
                .await
                .map(|_| ())
                .map_err(err),
            Writer::Local(file) => {
                drop(file);
                Ok(())
            }
        };
        let result=async{
            result?;closed?;
            progress.stage("verifySource");
            if let Some(parent) = &source_parent { if &self.parent_identity(&source).await? != parent { return Err(failure("sourceChanged","来源父目录发生变化，未提交")); } }
            progress.stage("verifyTarget");
            if self.parent_identity(&destination).await? != destination_parent { return Err(failure("targetChanged","目标父目录发生变化，未提交")); }
            progress.stage("verifySource");
            if let Reader::Local(source) = &reader { source.verify().await.map_err(|e|failure("sourceChanged",e))?; }
            // Preserve ordinary modification time and existing target mode where available.
            // Metadata failure is reported before replacing the destination.
            progress.stage("metadata");
            self.preserve_metadata(&temporary, &before, &original, request.content_only).await?;
            progress.stage("verifySource");
            if self.inspect(&source).await?.as_ref().map(|a|&a["stamp"])!=Some(&before["stamp"]){return Err(failure("sourceChanged","传输期间源文件发生变化，未提交"));}
            if self.operation.cancel.is_cancelled(){return Err(failure("cancelled","传输已取消，原目标保留"));}
            progress.stage("verifyTarget");
            self.check_guard(&destination,&guard).await?;
            self.core.connection_session(&self.owner.context)?;
            progress.committing();
            match (&temporary,&destination){
                (Endpoint::Local(temp),Endpoint::Local(target))=>{
                    crate::local_files::commit_temporary(temp.clone(), target.clone(), guard["kind"]!="absent")
                        .await.map_err(local_error)?;
                },
                (Endpoint::Remote(temp),Endpoint::Remote(target))=>{
                    let committed = destination::commit_remote(&self.session.raw,temp,target,original.is_some()).await;
                    if let Err(e) = committed { progress.rejected(&e); return Err(e); }
                },_=>unreachable!(),
            }
            progress.committed();
            match self.inspect(&destination).await {
                Ok(Some(committed))=>Ok(json!({"size":offset.to_string(),"stamp":committed["stamp"]})),
                result=>Ok(json!({"size":offset.to_string(),"cleanupWarning":format!("文件已提交，但后续目标查询未确认：{result:?}")})),
            }
        }.await;
        // An unacknowledged commit may still use the temporary name on the server.
        if progress.is_unknown() {
            return result.map_err(|mut error| {
                let path = match &temporary {
                    Endpoint::Local(path) => path.to_string_lossy().into_owned(),
                    Endpoint::Remote(path) => path.clone(),
                };
                progress.temporary(path.clone());
                error
                    .message
                    .push_str(&format!("；提交未确认，保留临时路径供核对：{path}"));
                error
            });
        }
        // Only remove the operation's temporary name, never the destination.
        let cleanup = match &temporary {
            Endpoint::Local(path) => match tokio::fs::remove_file(path).await {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error.to_string()),
            },
            Endpoint::Remote(path) => match self.session.raw.remove(path).await {
                Ok(_) => Ok(()),
                Err(Error::Status(status)) if status.status_code == StatusCode::NoSuchFile => {
                    Ok(())
                }
                Err(error) => Err(error.to_string()),
            },
        };
        match (result, cleanup) {
            (Ok(mut value), Err(error)) => {
                value["cleanupWarning"] = json!(format!("临时文件清理失败：{error}"));
                Ok(value)
            }
            (Err(mut error), Err(cleanup)) => {
                error
                    .message
                    .push_str(&format!("；临时文件清理失败：{cleanup}"));
                Err(error)
            }
            (result, _) => result,
        }
    }
}

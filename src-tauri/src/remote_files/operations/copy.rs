use super::*;

enum Reader {
    Local(tokio::fs::File),
    Remote(String),
}
enum Writer {
    Local(tokio::fs::File),
    Remote(String),
}
impl Io {
    pub(super) async fn copy(&self, p: &Value) -> Result<Value, String> {
        let source = self.endpoint(&p["source"])?;
        let destination = self.endpoint(&p["destination"])?;
        let source_parent = self.parent_identity(&source).await?;
        let destination_parent = self.parent_identity(&destination).await?;
        if !["absent", "match"].contains(&p["destinationGuard"]["kind"].as_str().unwrap_or("")) {
            return Err(failure("invalid", "复制缺少目标版本检查"));
        }
        let before = self
            .inspect(&source)
            .await?
            .ok_or_else(|| failure("missing", "源文件不存在"))?;
        if before["kind"] != "file" {
            return Err(failure(
                "unsupported",
                "只复制普通文件，符号链接与特殊条目由批次跳过",
            ));
        }
        if !p["sourceStamp"].is_null() && p["sourceStamp"] != before["stamp"] {
            return Err(failure("sourceChanged", "源文件已变化"));
        }
        let original = self
            .check_guard(&destination, &p["destinationGuard"])
            .await?;
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
            Endpoint::Local(path) => {
                Reader::Local(tokio::fs::File::open(path).await.map_err(local_error)?)
            }
            Endpoint::Remote(path) => Reader::Remote(
                self.session
                    .raw
                    .open(path, OpenFlags::READ, FileAttributes::default())
                    .await
                    .map_err(err)?
                    .handle,
            ),
        };
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
                    FileAttributes::default(),
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
        let mut offset = 0u64;
        let streaming = async {
            let mut buffer = vec![0u8; 64 * 1024];
            loop {
                self.core.connection_session(&self.owner.context)?;
                let block = match &mut reader {
                    Reader::Local(file) => {
                        let n = file.read(&mut buffer).await.map_err(local_error)?;
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
                *self.operation.state.lock().unwrap() = json!({"id":self.operation.id,"state":"running","received":offset.to_string(),"total":before["size"]});
            }
            if before["size"]
                .as_str()
                .and_then(|s| s.parse::<u64>().ok())
                .is_some_and(|n| n != offset)
            {
                return Err(failure("sourceChanged", "源文件大小在传输期间变化"));
            }
            if let Writer::Local(file) = &mut writer {
                file.flush().await.map_err(local_error)?;
                file.sync_all().await.map_err(local_error)?;
            }
            Ok::<_, String>(())
        };
        let result = tokio::select! {biased;_=self.operation.cancel.cancelled()=>Err(failure("cancelled","传输已取消，原目标保留")),v=streaming=>v};
        if let Reader::Remote(handle) = reader {
            let _ = self.session.raw.close(handle).await;
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
            if self.parent_identity(&source).await? != source_parent || self.parent_identity(&destination).await? != destination_parent { return Err(failure("sourceChanged","传输期间父目录发生变化，未提交")); }
            // Preserve ordinary modification time and existing target mode where available.
            // Metadata failure is reported before replacing the destination.
            match &temporary {
                Endpoint::Local(path) => {
                    let path=path.clone();
                    let modified=before["mtime"].as_u64().map(|s|std::time::UNIX_EPOCH+Duration::from_secs(s));
                    #[cfg(unix)]
                    let permissions=original.as_ref().and_then(|v|v["permissions"].as_u64()).or_else(||before["permissions"].as_u64());
                    tokio::task::spawn_blocking(move || {
                        let file=std::fs::OpenOptions::new().write(true).open(&path).map_err(err)?;
                        if let Some(time)=modified {file.set_times(std::fs::FileTimes::new().set_modified(time)).map_err(err)?;}
                        #[cfg(unix)]
                        if let Some(mode)=permissions {use std::os::unix::fs::PermissionsExt;file.set_permissions(std::fs::Permissions::from_mode(mode as u32 & 0o777)).map_err(err)?;}
                        Ok::<_,String>(())
                    }).await.map_err(err)??;
                }
                Endpoint::Remote(path) => {
                    let mtime=before["mtime"].as_u64().and_then(|s|u32::try_from(s).ok());
                    let permissions=original.as_ref().and_then(|v|v["permissions"].as_u64()).or_else(||before["permissions"].as_u64()).map(|p|p as u32 & 0o777);
                    self.session.raw.setstat(path,FileAttributes{atime:mtime,mtime,permissions,..Default::default()}).await.map_err(err)?;
                }
            }
            if self.inspect(&source).await?.as_ref().map(|a|&a["stamp"])!=Some(&before["stamp"]){return Err(failure("sourceChanged","传输期间源文件发生变化，未提交"));}
            if self.operation.cancel.is_cancelled(){return Err(failure("cancelled","传输已取消，原目标保留"));}
            self.check_guard(&destination,&p["destinationGuard"]).await?;
            self.core.connection_session(&self.owner.context)?;
            *self.operation.state.lock().unwrap()=json!({"id":self.operation.id,"state":"committing","received":offset.to_string(),"total":before["size"]});
            match (&temporary,&destination){
                (Endpoint::Local(temp),Endpoint::Local(target))=>{
                    if p["destinationGuard"]["kind"]=="absent" {
                        // A hard link is an atomic exclusive install of this same-directory ordinary file.
                        tokio::fs::hard_link(temp,target).await.map_err(|e|failure(if e.kind()==std::io::ErrorKind::AlreadyExists{"targetExists"}else{"io"},e))?;
                        let _=tokio::fs::remove_file(temp).await;
                    }else{tokio::fs::rename(temp,target).await.map_err(err)?;}
                },
                (Endpoint::Remote(temp),Endpoint::Remote(target))=>{
                    let committed=if original.is_some(){let mut data=Vec::new();ssh_string(&mut data,temp);ssh_string(&mut data,target);match self.session.raw.extended("posix-rename@openssh.com",data).await{Ok(Packet::Status(status))if status.status_code==StatusCode::Ok=>Ok(()),Ok(packet)=>Err(failure("io",format!("服务器拒绝提交：{packet:?}"))),Err(Error::Status(status))=>Err(failure("io",status.error_message)),Err(e)=>Err(failure("unknown",format!("服务器未确认提交结果，请刷新核对：{e}")))}}
                    else{match self.session.raw.rename(temp,target).await{Ok(_)=>Ok(()),Err(Error::Status(status))=>Err(failure("targetExists",status.error_message)),Err(e)=>Err(failure("unknown",format!("服务器未确认提交结果，请刷新核对：{e}")))}};
                    committed?;
                },_=>unreachable!(),
            }
            let committed=self.inspect(&destination).await.map_err(|e|failure("unknown",format!("文件已提交，目标状态未确认：{e}")))?.ok_or_else(||failure("targetChanged","提交后目标已被移除"))?;
            Ok(json!({"size":offset.to_string(),"stamp":committed["stamp"]}))
        }.await;
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
            (Err(error), Err(cleanup)) => Err(format!(
                "{error}；临时文件 .sodeck-{}.part 清理失败：{cleanup}",
                self.operation.id
            )),
            (result, _) => result,
        }
    }
}

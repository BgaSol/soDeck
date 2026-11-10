use super::*;

enum Reader {
    Local(tokio::fs::ReadDir),
    Remote { session: Arc<Sftp>, handle: String },
}
pub(super) struct Cursor {
    pub(super) batch: String,
    reader: Reader,
    context: Context,
    pending: VecDeque<Value>,
    eof: bool,
    touched: Instant,
}
impl Drop for Cursor {
    fn drop(&mut self) {
        if let Reader::Remote { session, .. } = &self.reader {
            // Each cursor owns its own channel. Closing it also releases the directory handle.
            let _ = session.raw.close_session();
        }
    }
}
impl Cursor {
    async fn page(&mut self) -> Result<Value, OperationError> {
        self.touched = Instant::now();
        let mut entries = Vec::new();
        let mut bytes = 0;
        loop {
            if self.pending.is_empty() && !self.eof {
                match &mut self.reader {
                    Reader::Local(directory) => match directory.next_entry().await.map_err(err)? {
                        Some(entry) => {
                            let name = entry
                                .file_name()
                                .into_string()
                                .map_err(|_| failure("invalidName", "文件名无法用 UTF-8 表示"))?;
                            let attrs = local_attrs(
                                &tokio::fs::symlink_metadata(entry.path())
                                    .await
                                    .map_err(err)?,
                            );
                            self.pending.push_back(json!({"name":name,"attrs":attrs}));
                        }
                        None => self.eof = true,
                    },
                    Reader::Remote { session, handle } => {
                        match session.raw.readdir(handle.clone()).await {
                            Ok(page) => {
                                for entry in page.files {
                                    if entry.filename != "." && entry.filename != ".." {
                                        self.pending.push_back(json!({"name":entry.filename,"attrs":remote_attrs(&entry.attrs)}));
                                    }
                                }
                            }
                            Err(e) if eof(&e) => self.eof = true,
                            Err(e) => return Err(err(e)),
                        }
                    }
                }
            }
            let Some(entry) = self.pending.front() else {
                if self.eof {
                    break;
                }
                continue;
            };
            let size = serde_json::to_vec(entry).map_err(err)?.len();
            if size > PAGE_BYTES {
                return Err(failure("invalidName", "目录条目超过单页大小限制"));
            }
            if entries.len() >= 100 || bytes + size > PAGE_BYTES {
                break;
            }
            bytes += size;
            entries.push(self.pending.pop_front().unwrap());
        }
        Ok(json!({"entries":entries,"more":!self.eof || !self.pending.is_empty()}))
    }
}
impl Io {
    pub(super) async fn open_cursor(&self, endpoint: Endpoint) -> Result<Value, OperationError> {
        if self
            .inspect(&endpoint)
            .await?
            .is_none_or(|a| a["kind"] != "directory")
        {
            return Err(failure("invalid", "此条目不是普通目录"));
        }
        {
            let mut cursors = self.resource.operations.cursors.lock().unwrap();
            cursors.retain(|_, c| {
                c.try_lock()
                    .map_or(true, |c| c.touched.elapsed() < Duration::from_secs(120))
            });
            if cursors.len() >= 4 {
                return Err(failure("busy", "目录游标已满，请关闭已完成的遍历"));
            }
        }
        let reader = match endpoint {
            Endpoint::Local(path) => Reader::Local(tokio::fs::read_dir(path).await.map_err(err)?),
            Endpoint::Remote(path) => Reader::Remote {
                handle: self.session.raw.opendir(path).await.map_err(err)?.handle,
                session: self.session.clone(),
            },
        };
        let mut cursor = Cursor {
            batch: self.operation.request["batchId"]
                .as_str()
                .unwrap_or("")
                .into(),
            reader,
            context: self.owner.context.clone(),
            pending: VecDeque::new(),
            eof: false,
            touched: Instant::now(),
        };
        let mut page = cursor.page().await?;
        if page["more"] == true {
            let id = self.core.next_id();
            page["cursor"] = json!(id);
            self.resource
                .operations
                .cursors
                .lock()
                .unwrap()
                .insert(id, Arc::new(tokio::sync::Mutex::new(cursor)));
        }
        Ok(page)
    }
    pub(super) async fn cursor_action(&self, p: &Value) -> Result<Value, OperationError> {
        let id = string(p, "cursor")?;
        if p["kind"] == "closeCursor" {
            self.resource.operations.cursors.lock().unwrap().remove(id);
            return Ok(json!({}));
        }
        let cursor = self
            .resource
            .operations
            .cursors
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(|| failure("expired", "目录游标已失效"))?;
        let mut cursor = cursor.lock().await;
        if cursor.context != self.owner.context
            || cursor.touched.elapsed() > Duration::from_secs(120)
        {
            return Err(failure("expired", "目录游标已失效，请重新枚举"));
        }
        let mut page = cursor.page().await?;
        if page["more"] == true {
            page["cursor"] = json!(id);
        } else {
            self.resource.operations.cursors.lock().unwrap().remove(id);
        }
        Ok(page)
    }
}

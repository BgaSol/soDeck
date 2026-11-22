use super::*;
impl AppCore {
    pub(super) async fn list_directory(
        &self,
        s: &mut State,
        p: &Value,
        context: &Context,
    ) -> Result<Value, String> {
        let mut cursor = p["cursor"].as_str().unwrap_or("").to_string();
        if p["next"] != true {
            let target = path(&p)?.to_string();
            let old = s.directories.remove(&cursor);
            if let Some(old) = old {
                if let Some(f) = &s.session {
                    if !old.eof {
                        let _ = f.raw.close(old.handle).await;
                    }
                }
            }
            let f = sftp(self, s, context).await?;
            {
                let State {
                    directory_order,
                    directories,
                    ..
                } = &mut *s;
                directory_order.retain(|id| directories.contains_key(id));
                if s.directories.len() >= 32 {
                    if let Some(id) = s.directory_order.pop_front() {
                        if let Some(old) = s.directories.remove(&id) {
                            if !old.eof {
                                let _ = f.raw.close(old.handle).await;
                            }
                        }
                    }
                }
            }
            let resolved = canonical(&f.raw, &target).await?;
            let handle = f.raw.opendir(resolved.clone()).await.map_err(err)?.handle;
            let directory = Directory {
                path: resolved,
                handle,
                pending: VecDeque::new(),
                eof: false,
            };
            cursor = self.next_id();
            s.directories.insert(cursor.clone(), directory);
            s.directory_order.push_back(cursor.clone());
        }
        let State {
            session,
            directories,
            ..
        } = &mut *s;
        let f = session.as_ref().ok_or("文件会话已失效，请刷新目录")?;
        if f.context != *context {
            return Err("连接已更换，请刷新目录".into());
        }
        let d = directories
            .get_mut(&cursor)
            .ok_or("目录游标已失效，请刷新此目录")?;
        let mut entries = Vec::new();
        while entries.len() < 100 {
            if let Some(v) = d.pending.pop_front() {
                entries.push(v);
                continue;
            }
            if d.eof {
                break;
            }
            match f.raw.readdir(d.handle.clone()).await {
                Ok(batch) => {
                    for entry in batch.files {
                        if entry.filename != "." && entry.filename != ".." {
                            d.pending.push_back(
                                json!({"name":entry.filename,"attrs":attrs(&entry.attrs)}),
                            );
                        }
                    }
                }
                Err(e) if eof(&e) => {
                    d.eof = true;
                    let _ = f.raw.close(d.handle.clone()).await;
                }
                Err(e) => return Err(err(e)),
            }
        }
        let more = !d.eof || !d.pending.is_empty();
        let result = json!({"path":d.path,"entries":entries,"more":more,"cursor":if more {Some(cursor.as_str())}else{None}});
        if !more {
            directories.remove(&cursor);
        }
        Ok(result)
    }
}

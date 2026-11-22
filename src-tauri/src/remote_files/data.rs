use super::*;
impl AppCore {
    pub async fn files_data(
        self: &Arc<Self>,
        window: &str,
        token: &str,
        mut p: Value,
    ) -> Result<Value, sodeck_plugin_protocol::ApiError> {
        if let Some(action) = p["action"]
            .as_str()
            .and_then(|a| a.strip_prefix("operation."))
        {
            let mut request = p.clone();
            request
                .as_object_mut()
                .ok_or("文件请求必须为对象")?
                .remove("op");
            request["action"] = json!(action);
            let request: sodeck_plugin_protocol::requests::FileRequest =
                serde_json::from_value(request)
                    .map_err(|_| sodeck_plugin_protocol::ApiError::invalid("文件操作参数无效"))?;
            p = serde_json::to_value(request)
                .map_err(|_| sodeck_plugin_protocol::ApiError::invalid("文件操作编码失败"))?;
            p["action"] = json!(format!("operation.{}", p["action"].as_str().unwrap()));
        }
        let (view, _surface_call) = if p["action"] == "document.load" {
            // Initial document loading is allowed before prepared on the active source only.
            let state = self.workbench_state();
            let surface = state
                .ui
                .surfaces
                .get(token)
                .filter(|s| {
                    s.window == window && s.phase == crate::component_ui::SurfacePhase::Active
                })
                .ok_or("候选表面不能打开文件")?;
            let view = state.ui.resource_view(&surface.view)?.clone();
            state.workspaces.validate_owned(window, &view.context)?;
            if state.ui.lifecycle.view_frozen(&view.id) {
                return Err("文件页面正在关闭".into());
            }
            let task = view.calls.enter()?;
            (view, Some(task))
        } else if [
            "home",
            "stat",
            "list",
            "draft.describe",
            "draft.chunk",
            "draft.status",
            "save.status",
            "export.status",
        ]
        .contains(&p["action"].as_str().unwrap_or_default())
        {
            (self.ui_data_view(window, token)?, None)
        } else {
            let (v, task) = self.surface_call_admission(window, token)?;
            (v, Some(task))
        };
        let r = self.file_resource(&view.plugin_id, &view.plugin_instance_id, string(&p, "id")?)?;
        let _task = r.tasks.enter()?;
        if r.owner.view != view.id {
            return Err("文件资源不属于此页面".into());
        }
        let action = string(&p, "action")?;
        if action.starts_with("operation.") || action.starts_with("input.") {
            self.ui_data_writable(window, token)?;
            return self.file_operation_control(r, &p).map_err(Into::into);
        }

        // Local draft reads are allowed while a replacement surface is mounting.
        if ![
            "home",
            "stat",
            "list",
            "document.load",
            "draft.describe",
            "draft.chunk",
            "draft.status",
            "save.status",
            "export.status",
        ]
        .contains(&action)
        {
            self.ui_data_writable(window, token)?;
        }

        if action.starts_with("draft.") {
            let readonly = ["draft.describe", "draft.chunk", "draft.status"].contains(&action);
            let _admission = if readonly {
                None
            } else {
                Some(self.surface_write_admission(window, token)?)
            };
            return r.editor.lock().unwrap().draft(token, &p);
        }
        if action.starts_with("save.") || action.starts_with("export.") {
            return self
                .files_editor(&view.plugin_id, &view.plugin_instance_id, &p)
                .map_err(Into::into);
        }

        let cancel = r.cancel.clone();
        let work = async {
            let mut s = r.state.lock().await;
            match action {
                "home" => {
                    let f = sftp(self, &mut s, &view.context).await?;
                    Ok(json!({"path":canonical(&f.raw,".").await?}))
                }
                "list" => self.list_directory(&mut s, &p, &view.context).await,
                "closeDirectory" => {
                    if let Some(old) = s.directories.remove(string(&p, "cursor")?) {
                        if !old.eof {
                            if let Some(f) = &s.session {
                                let _ = f.raw.close(old.handle).await;
                            }
                        }
                    }
                    Ok(json!({}))
                }
                "stat" => {
                    let f = sftp(self, &mut s, &view.context).await?;
                    let target = path(&p)?;
                    let a = f.raw.stat(target).await.map_err(err)?.attrs;
                    Ok(json!({"path":canonical(&f.raw,target).await?,"attrs":attrs(&a)}))
                }
                "resolveLocation" => {
                    let target = location::normalize(&p)?;
                    let f = sftp(self, &mut s, &view.context).await?;
                    if p["filesystemView"] == "shell-reported" {
                        let witness = &p["witness"];
                        let marker = witness["path"]
                            .as_str()
                            .filter(|s| {
                                s.starts_with('/')
                                    && s.len() <= 4096
                                    && !s.chars().any(char::is_control)
                            })
                            .ok_or("unsupportedLocation: 缺少文件系统对应标记")?;
                        let expected = witness["value"]
                            .as_str()
                            .filter(|s| !s.is_empty() && s.len() <= 128)
                            .ok_or("unsupportedLocation: 文件系统标记无效")?;
                        let a = f
                            .raw
                            .lstat(marker)
                            .await
                            .map_err(|_| "unsupportedLocation: 终端与文件服务的文件系统无法对应")?
                            .attrs;
                        if !a.file_type().is_file() || a.size != Some(expected.len() as u64) {
                            return Err(
                                "unsupportedLocation: 终端与文件服务的文件系统无法对应".into()
                            );
                        }
                        let bytes = read_small(&f.raw, marker, &a, |n| {
                            self.record_business(&r.owner, 0, n as u64)
                        })
                        .await?;
                        if bytes != expected.as_bytes() {
                            return Err(
                                "unsupportedLocation: 终端与文件服务的文件系统无法对应".into()
                            );
                        }
                    }
                    let resolved = canonical(&f.raw, &target).await?;
                    let a = f.raw.stat(resolved.clone()).await.map_err(err)?.attrs;
                    self.workspace_state().validate_identity(&view.context)?;
                    Ok(json!({"path":resolved,"attrs":attrs(&a)}))
                }
                "document.load" => {
                    drop(s);
                    self.load_document(window, token, &p, &view, &r).await
                }
                _ => Err("未知文件数据方法".into()),
            }
        };
        let result = tokio::select! {biased;_=cancel.cancelled()=>Err("文件页面已关闭".into()),v=tokio::time::timeout(Duration::from_secs(35),work)=>v.map_err(|_|"文件操作超时；请刷新后重试".to_string()).and_then(|v|v)};
        if result.is_err() && ["list", "home", "stat", "document.load"].contains(&action) {
            let mut state = r.state.lock().await;
            if let Some(session) = state.session.take() {
                let _ = session.raw.close_session();
            }
            state.directories.clear();
            state.directory_order.clear();
        }
        result.map_err(Into::into)
    }
}

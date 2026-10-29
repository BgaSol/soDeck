use super::*;

impl State {
    pub(super) fn presentation_for(&self, id: &str) -> Result<Value> {
        let w = self.workbench.lock().unwrap();
        let v = w.view(id).map_err(native_error)?;
        let ws = w
            .workspaces
            .get(&v.workspace_id)
            .ok_or_else(|| err("STALE_WORKSPACE", "工作区已关闭"))?;
        let visible = self
            .view_binding(id)
            .is_some_and(|b| b.phase.load(Ordering::SeqCst) == 1 && b.geometry.lock().unwrap().1);
        Ok(
            json!({"viewId":id,"scope":{"workspaceId":v.workspace_id,"workspaceGeneration":ws.generation},"visible":visible,"focused":visible&&ws.focus_target.as_deref()==Some(id),"revision":w.revision}),
        )
    }
    pub(super) fn publish_presentation(self: &Arc<Self>, id: &str) {
        let Ok(plugin) = self.plugin_for_view(id) else {
            return;
        };
        if !plugin
            .package
            .manifest
            .requires
            .capabilities
            .iter()
            .any(|c| c == "workspace.presentation.v1")
        {
            return;
        }
        let Ok(value) = self.presentation_for(id) else {
            return;
        };
        if plugin.package.manifest.requires.capabilities.iter().any(|c|c=="core.state.v1") {
            self.publish_state_presentation(value);
            return;
        }
        if let Some(b) = self.view_binding(id) {
            let _ = self
                .native
                .notify(b.id.clone(), "view.presentation", value.clone());
        }
        // The worker callback may call the host, so never hold layout or mount
        // locks while waiting for it. Presentation is idempotent and revisioned.
        std::thread::spawn(move || {
            let _ = plugin.process.invoke("lifecycle.presentation", value);
        });
    }
    pub(super) fn extended_workbench(
        &self,
        caller: &Caller,
        source: Option<&str>,
        method: &str,
        p: &Value,
    ) -> Result<Value> {
        match method {
            "views.presentation" | "views.requestFocus" => {
                let id = required(p, "viewId")?;
                let v = self
                    .workbench
                    .lock()
                    .unwrap()
                    .view(id)
                    .map_err(native_error)?;
                if caller
                    .plugin_id
                    .as_ref()
                    .is_some_and(|owner| owner != &v.plugin_id)
                {
                    return Err(err("SOURCE_INVALID", "视图不属于此插件"));
                }
                if method == "views.requestFocus" {
                    let source =
                        source.ok_or_else(|| err("SOURCE_INVALID", "焦点请求需要有效界面"))?;
                    let b = self.binding(source)?;
                    if b.view_id != id
                        || b.phase.load(Ordering::SeqCst) != 1
                        || !b.geometry.lock().unwrap().1
                        || !self.page_allows_surface(&v,&b.window)
                        || self.shell_modals.lock().unwrap().contains(&b.window)
                    {
                        return Err(err("STALE_MOUNT", "表面不可见或已失效"));
                    }
                    if p["userGesture"] != true {
                        return Err(err("INVALID_PARAMS", "焦点请求需要用户操作"));
                    }
                    {
                        let mut w = self.workbench.lock().unwrap();
                        w.activate(id).map_err(native_error)?;
                        if let Some(ws) = w.workspaces.get_mut(&v.workspace_id) {
                            ws.focus_intent += 1;
                        }
                    }
                    self.native.focus(b.id.clone()).map_err(native_error)?;
                }
                self.presentation_for(id)
            }
            "views.badge.set" => {
                let id = required(p, "viewId")?;
                let view = self.workbench.lock().unwrap().view(id).map_err(native_error)?;
                if caller.plugin_id.as_deref() != Some(view.plugin_id.as_str()) { return Err(err("SOURCE_INVALID", "徽标不属于当前插件")); }
                self.view_badges.lock().unwrap().insert(id.into(), p["count"].as_u64().unwrap_or(0));
                for window in self.app.webview_windows().values() { let _ = window.eval(&format!("window.__sodeckNotify?.({});", json!({"type":"views.badge.changed","payload":p}))); }
                Ok(json!({"ok":true}))
            }
            "workbench.contributions.v2" => {
                let views=self.workbench.lock().unwrap().views.iter().map(|v|v.view_id.clone()).collect::<std::collections::HashSet<_>>();
                let badges={let mut badges=self.view_badges.lock().unwrap();badges.retain(|id,_|views.contains(id));badges.iter().map(|(id,count)|json!({"viewId":id,"count":count})).collect::<Vec<_>>()};
                Ok(json!({"badges":badges,"plugins":self.all_plugins().iter().map(|p| {
                    let m=&p.package.manifest;
                    json!({"pluginId":m.plugin_id,"displayName":m.display_name(),"commands":m.contributes.commands,
                        "views":m.contributes.views.iter().map(|v|json!({"id":v.id,"title":v.title,"scope":v.scope,"supportedHosts":v.supported_hosts,"dockEdges":v.dock_edges,"multiple":v.multiple})).collect::<Vec<_>>(),
                        "presentation":m.contributes.presentation.clone().unwrap_or_default()})
                }).collect::<Vec<_>>()}))
            }
            "workbench.contributions" => {
                let plugins = self.all_plugins();
                let commands = plugins
                    .iter()
                    .flat_map(|p| {
                        p.package.manifest.contributes.commands.iter().map(|c| {
                            let mut value = serde_json::to_value(c).unwrap();
                            value["pluginId"] = json!(p.package.manifest.plugin_id);
                            value
                        })
                    })
                    .collect::<Vec<_>>();
                let views=plugins.iter().flat_map(|p|p.package.manifest.contributes.views.iter().map(|v|json!({"pluginId":p.package.manifest.plugin_id,"id":v.id,"minWidth":v.min_width,"minHeight":v.min_height}))).collect::<Vec<_>>();
                Ok(json!({"commands":commands,"views":views}))
            }
            "workbench.dock.configure" => {
                let edge = required(p, "edge")?;
                let default_width = self.all_plugins().iter().filter_map(|p| p.package.manifest.contributes.presentation.as_ref()).flat_map(|p| &p.tools).find(|t| t.edge == edge).map(|t|t.default_width as u64).unwrap_or(320);
                let mut w = self.workbench.lock().unwrap();
                let ws = w
                    .workspaces
                    .get_mut(required(p, "workspaceId")?)
                    .ok_or_else(|| err("STALE_WORKSPACE", "工作区已关闭"))?;
                if p["workspaceGeneration"].as_str() != Some(&ws.generation) {
                    return Err(err("STALE_WORKSPACE", "工作区创建身份已变化"));
                }
                if let Some(width) = p["width"].as_u64() {
                    ws.dock_widths.insert(edge.into(), width.clamp(260, 480));
                }
                Ok(json!({"width":ws.dock_widths.get(edge).copied().unwrap_or(default_width)}))
            }
            _ => Err(err("UNSUPPORTED_API", "未知呈现方法")),
        }
    }
}

//! Coordinated save/cancel/dispose before any view resources are released.
use super::*;

impl AppCore {
    pub fn ui_close_prepare(
        &self,
        window: &str,
        workspace_ids: Vec<String>,
        view_id: Option<String>,
    ) -> Result<Value, String> {
        let projection = self.workspace_projection(window)?;
        let names = self.workspaces.lock().unwrap().names();
        let allowed: HashSet<_> = projection
            .workspaces
            .iter()
            .filter(|w| window == "main" || w.window_id == window)
            .map(|w| w.context.workspace_id.clone())
            .collect();
        // Main-window exit can coordinate views owned by all native windows.
        let mut ui = self.component_ui.lock().unwrap();
        if window != "main" && workspace_ids.iter().any(|id| !allowed.contains(id)) {
            return Err("工作区不属于此窗口".into());
        }
        if workspace_ids.iter().any(|id| ui.frozen(id)) {
            return Err("已有关闭确认，请先处理".into());
        }
        let views: Vec<_> = workspace_ids
            .iter()
            .filter_map(|id| ui.workspaces.get(id))
            .flat_map(|w| &w.views)
            .filter(|v| view_id.as_ref().is_none_or(|id| id == &v.id))
            .cloned()
            .collect();
        let token = self.next_id();
        ui.plans.insert(
            token.clone(),
            ClosePlan {
                window: window.into(),
                workspaces: workspace_ids,
                views: views.iter().map(|v| v.id.clone()).collect(),
            },
        );
        drop(ui);
        let checked = (|| {
            for v in &views {
                match self.ui_plugin_call(v, "ui.beforeClose", Value::Null) {
                    Ok(reply) => {
                        self.component_ui
                            .lock()
                            .unwrap()
                            .update_metadata(&v.id, &reply["metadata"])?;
                    }
                    Err(e) => {
                        let mut ui = self.component_ui.lock().unwrap();
                        if let Some(v) = ui
                            .workspaces
                            .values_mut()
                            .flat_map(|w| &mut w.views)
                            .find(|x| x.id == v.id)
                        {
                            v.error = Some(e);
                        }
                    }
                }
            }
            let ui = self.component_ui.lock().unwrap();
            let dirty: Vec<_> = views
                .iter()
                .filter_map(|v| ui.view(&v.id).ok())
                .filter(|v| v.dirty || v.busy || v.error.is_some())
                .map(|v| json!({"id":v.id,"title":v.title,"dirty":v.dirty,"busy":v.busy,"error":v.error,"serverName":names.get(&v.context.workspace_id)}))
                .collect();
            Ok(
                json!({"token":token,"views":views.iter().map(|v| &v.id).collect::<Vec<_>>(),"dirty":dirty}),
            )
        })();
        if checked.is_err() {
            self.component_ui.lock().unwrap().plans.remove(&token);
        }
        checked
    }
    pub fn ui_close_finish(
        &self,
        token: &str,
        ids: Vec<String>,
        decision: &str,
    ) -> Result<(), String> {
        let views = {
            let mut ui = self.component_ui.lock().unwrap();
            let plan = ui.plans.get(token).ok_or("关闭确认已失效")?;
            if decision == "cancel" {
                ui.plans.remove(token);
                return Ok(());
            }
            if !["save", "discard"].contains(&decision) {
                return Err("无效关闭决定".into());
            }
            if ids.len() != plan.views.len() || ids.iter().any(|id| !plan.views.contains(id)) {
                return Err("关闭页面集合与确认时不一致".into());
            }
            let mut views = vec![];
            for id in &ids {
                let v = ui.view(id)?;
                if !plan.workspaces.contains(&v.context.workspace_id) {
                    return Err("页面不属于此关闭操作".into());
                }
                views.push(v.clone());
            }
            views
        };
        let result = (|| {
            if decision == "save" {
                for v in &views {
                    let mut reply = self.ui_plugin_call(v, "ui.save", Value::Null)?;
                    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(125);
                    while reply["metadata"]["busy"] == true {
                        if std::time::Instant::now() >= deadline {
                            return Err("保存仍在进行，关闭已取消；请检查任务结果".into());
                        }
                        std::thread::sleep(std::time::Duration::from_millis(100));
                        reply = self.ui_plugin_call(v, "ui.beforeClose", Value::Null)?;
                    }
                    self.component_ui
                        .lock()
                        .unwrap()
                        .update_metadata(&v.id, &reply["metadata"])?;
                    if self.component_ui.lock().unwrap().view(&v.id)?.dirty {
                        return Err("组件保存后仍有未保存修改，关闭已取消".into());
                    }
                }
            }
            for v in &views {
                if let Err(e) = self.ui_plugin_call(v, "ui.disposeView", Value::Null) {
                    self.log(
                        None,
                        "warn",
                        "关闭不可用的组件页面",
                        Some(json!({"viewId":v.id,"pluginId":v.plugin_id,"error":e})),
                    );
                }
                self.terminals.cancel(|owner| owner.view == v.id);
                self.executions.cancel(|owner| owner.view == v.id);
                self.network.cancel(|owner| owner.view == v.id);
                self.files.cancel(|owner| owner.view == v.id);
            }
            let mut ui = self.component_ui.lock().unwrap();
            let editors: HashSet<_> = ui
                .definitions
                .iter()
                .filter(|(_, d)| d.definition.container == "editor")
                .map(|(key, _)| key.clone())
                .collect();
            for w in ui.workspaces.values_mut() {
                let old = w
                    .editor
                    .as_ref()
                    .and_then(|id| w.views.iter().position(|v| &v.id == id))
                    .unwrap_or(0);
                w.views.retain(|v| !ids.contains(&v.id));
                if w.left.as_ref().is_some_and(|id| ids.contains(id)) {
                    w.left = None;
                }
                if w.right.as_ref().is_some_and(|id| ids.contains(id)) {
                    w.right = None;
                }
                if w.editor.as_ref().is_some_and(|id| ids.contains(id)) {
                    let remaining: Vec<_> = w
                        .views
                        .iter()
                        .filter(|v| editors.contains(&v.definition_key))
                        .collect();
                    w.editor = remaining
                        .get(old.min(remaining.len().saturating_sub(1)))
                        .map(|v| v.id.clone());
                }
            }
            ui.surfaces.retain(|_, s| !ids.contains(&s.view));
            Ok(())
        })();
        self.component_ui.lock().unwrap().plans.remove(token);
        self.workspace_changed();
        result
    }
}
impl AppCore {
    pub(super) fn component_close_owner(&self, window: &str, token: &str) -> Result<(), String> {
        if self
            .component_ui
            .lock()
            .unwrap()
            .plans
            .get(token)
            .is_some_and(|p| p.window == window)
        {
            Ok(())
        } else {
            Err("关闭确认不属于此窗口或已失效".into())
        }
    }
}

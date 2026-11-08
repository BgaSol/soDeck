use super::*;
use std::time::{Duration, Instant};

impl AppCore {
    fn close_handed_off(&self, id: &str) -> bool {
        if self.operation(id).adopted_by.is_some() {
            self.close_mutate(id, |o| o.working = false);
            true
        } else {
            false
        }
    }
    fn operation(&self, id: &str) -> Operation {
        self.component_ui.lock().unwrap().lifecycle.operations[id].clone()
    }
    fn close_views(&self, id: &str) -> Vec<View> {
        let ui = self.component_ui.lock().unwrap();
        ui.lifecycle.operations[id]
            .views
            .iter()
            .filter_map(|id| ui.resource_view(id).ok().cloned())
            .collect()
    }
    fn close_mutate(&self, id: &str, change: impl FnOnce(&mut Operation)) {
        let progress = {
            let mut ui = self.component_ui.lock().unwrap();
            let op = ui.lifecycle.operations.get_mut(id).unwrap();
            let before = (op.phase, op.error.clone());
            change(op);
            op.revision += 1;
            let progress = (before!=(op.phase,op.error.clone())).then(||json!({"operationId":id,"target":op.target,"phase":op.phase,"error":op.error,"views":op.views}));
            if op.phase == Phase::Aborted && matches!(op.target, Target::Application { .. }) {
                // Aborted is observable only after admission has been restored
                // in the same reservation critical section.
                let active = ui.lifecycle.operations.values().any(|o| {
                    !o.phase.terminal()
                        && o.adopted_by.is_none()
                        && matches!(o.target, Target::Application { .. })
                });
                self.set_closing(active);
            }
            progress
        };
        if let Some(progress) = progress {
            self.log(None, "info", "关闭步骤变更", Some(progress));
        }
        self.close_changed(id);
    }
    pub(crate) fn close_abort(&self, id: &str) {
        if self.close_handed_off(id) {
            return;
        }
        if self.operation(id).orphaned {
            self.close_mutate(id, |o| {
                o.cancel = false;
                o.phase = Phase::Cleaning;
            });
            self.close_execute(id, "discard");
            return;
        }
        if matches!(self.operation(id).target, Target::Application { .. }) {
            drop(self.mutations.lock().unwrap());
        }
        if let Err(error) = self.wait_calls(&self.close_views(id), Duration::from_secs(6)) {
            self.close_mutate(id, |o| {
                o.phase = Phase::Deciding;
                o.error = Some(error);
                o.working = false;
                o.cancel = false;
                o.decision = None;
                o.decision_revision += 1;
            });
            return;
        }
        let deadline = Instant::now() + Duration::from_secs(6);
        for view in self.close_views(id).iter().filter(|v| {
            self.operation(id).save_started.contains(&v.id)
                && !self.operation(id).saved.contains(&v.id)
        }) {
            loop {
                let reply = self.ui_plugin_call_observed(
                    view,
                    "ui.beforeClose",
                    Value::Null,
                    None,
                    Duration::from_secs(5).min(deadline.saturating_duration_since(Instant::now())),
                );
                if let Ok(reply) = reply {
                    let _ = self
                        .component_ui
                        .lock()
                        .unwrap()
                        .update_metadata(&view.id, &reply["metadata"]);
                    if reply["metadata"]["busy"] != true && !self.files.saving(&view.id) {
                        break;
                    }
                }
                if Instant::now() >= deadline || view.calls.active() > 0 {
                    self.close_mutate(id, |o| {
                        o.phase = Phase::Deciding;
                        o.working = false;
                        o.cancel = false;
                        o.decision = None;
                        o.error =
                            Some("已接受的保存仍未确认；页面保留，请核对原任务或明确放弃".into());
                        o.decision_revision += 1;
                    });
                    return;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
        if self.close_handed_off(id) {
            return;
        }
        let adopted = self.operation(id).adopted;
        {
            let mut ui = self.component_ui.lock().unwrap();
            for old in adopted {
                if let Some(o) = ui.lifecycle.operations.get_mut(&old) {
                    o.adopted_by = None;
                }
            }
        }
        self.close_mutate(id, |o| {
            o.phase = Phase::Aborted;
            o.working = false;
            o.ended_at = Some(crate::core::now());
        });
        self.workspace_changed();
    }
    fn close_decision_error(&self, id: &str, error: String) {
        self.close_mutate(id, |o| {
            o.phase = Phase::Deciding;
            o.working = false;
            o.decision = None;
            o.saving_view = None;
            o.error = Some(error);
            o.decision_revision += 1;
        });
        if self.operation(id).cancel {
            self.close_abort(id);
        }
    }
    fn wait_calls(&self, views: &[View], budget: Duration) -> Result<(), String> {
        let deadline = Instant::now() + budget;
        while views.iter().any(|v| v.calls.active() > 0) {
            if Instant::now() >= deadline {
                return Err("组件调用的真实结果尚未确认；可以返回处理，或明确放弃后关闭".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    }
    pub(crate) fn close_check_views(&self, id: &str) {
        if self.close_handed_off(id) {
            return;
        }
        if self.operation(id).orphaned {
            self.close_mutate(id, |o| o.phase = Phase::Cleaning);
            self.close_execute(id, "discard");
            return;
        }
        let views = self.close_views(id);
        let deadline = Instant::now() + Duration::from_secs(125);
        let mut errors = Vec::new();
        if let Err(e) = self.wait_calls(&views, Duration::from_secs(5)) {
            errors.push(e);
        }
        if errors.is_empty() {
            for view in &views {
                if self.operation(id).cancel {
                    self.close_abort(id);
                    return;
                }
                if Instant::now() >= deadline {
                    errors.push("组件状态检查尚未完成，请返回处理或明确放弃".into());
                    break;
                }
                match self.ui_plugin_call_observed(
                    view,
                    "ui.beforeClose",
                    Value::Null,
                    None,
                    Duration::from_secs(5).min(deadline.saturating_duration_since(Instant::now())),
                ) {
                    Ok(reply) => {
                        let _ = self
                            .component_ui
                            .lock()
                            .unwrap()
                            .update_metadata(&view.id, &reply["metadata"]);
                    }
                    Err(e) => errors.push(format!("{}：{e}", view.title)),
                }
                if self.close_handed_off(id) {
                    return;
                }
            }
        }
        if self.operation(id).orphaned {
            self.close_execute(id, "discard");
            return;
        }
        let op = self.operation(id);
        for (window, report) in &op.inputs {
            if let Some(report) = report {
                if let Some(e) = report["error"].as_str() {
                    errors.push(format!("{window}：{e}"));
                }
            }
        }
        let mut dirty:Vec<Value>=self.close_views(id).iter().filter(|v|v.dirty||v.busy||v.error.is_some())
            .map(|v|json!({"id":v.id,"title":v.title,"dirty":v.dirty,"busy":v.busy,"error":v.error})).collect();
        for (window, report) in op
            .inputs
            .iter()
            .filter_map(|(w, r)| r.as_ref().map(|r| (w, r)))
        {
            if let Some(drafts) = report["drafts"].as_array() {
                dirty.extend(
                    drafts
                        .iter()
                        .filter(|d| d["dirty"] == true || d["busy"] == true)
                        .map(|d| {
                            let mut d = d.clone();
                            d["windowId"] = json!(window);
                            d
                        }),
                );
            }
        }
        self.close_mutate(id, |o| {
            o.phase = Phase::Deciding;
            o.working = false;
            o.dirty = dirty;
            o.error = (!errors.is_empty()).then(|| errors.join("；"));
            o.decision_revision += 1;
        });
        if self.operation(id).cancel {
            self.close_abort(id);
        }
    }
    pub(crate) fn close_execute(&self, id: &str, decision: &str) {
        if self.close_handed_off(id) {
            return;
        }
        if self.operation(id).orphaned
            && matches!(self.operation(id).phase, Phase::Preparing | Phase::Deciding)
        {
            self.close_mutate(id, |o| {
                o.cancel = false;
                o.phase = Phase::Cleaning;
            });
        }
        if self.operation(id).cancel {
            self.close_abort(id);
            return;
        }
        let mut scope_changed = false;
        let result = (|| {
            let op = self.operation(id);
            let views = self.close_views(id);
            if op.phase == Phase::Deciding {
                let input_errors: Vec<_> = op
                    .inputs
                    .values()
                    .flatten()
                    .filter(|r| decision != "discard" || r["unconfirmed"] != true)
                    .filter_map(|r| r["error"].as_str())
                    .collect();
                if !input_errors.is_empty() {
                    return Err(input_errors.join("；"));
                }
                if decision == "save" {
                    let deadline = Instant::now() + Duration::from_secs(125);
                    self.wait_calls(&views, Duration::from_secs(5))?;
                    for view in &views {
                        if self.operation(id).adopted_by.is_some() {
                            return Ok(());
                        }
                        if self.operation(id).orphaned {
                            break;
                        }
                        if self.operation(id).cancel {
                            return Err("关闭已取消；已接受的保存结果保留".into());
                        }
                        if !view.dirty && !view.busy || self.operation(id).saved.contains(&view.id)
                        {
                            continue;
                        }
                        // Record acceptance before the RPC, including a possible unknown result.
                        let started = self.operation(id).save_started.contains(&view.id);
                        if Instant::now() >= deadline {
                            return Err("保存结果尚未全部确认，请核对原任务结果".into());
                        }
                        if !started {
                            self.close_mutate(id, |o| o.save_started.push(view.id.clone()));
                        }
                        self.close_mutate(id, |o| o.saving_view = Some(view.id.clone()));
                        let mut reply = self.ui_plugin_call_observed(
                            view,
                            if started { "ui.beforeClose" } else { "ui.save" },
                            Value::Null,
                            None,
                            Duration::from_secs(5)
                                .min(deadline.saturating_duration_since(Instant::now())),
                        )?;
                        while reply["metadata"]["busy"] == true {
                            self.component_ui
                                .lock()
                                .unwrap()
                                .update_metadata(&view.id, &reply["metadata"])?;
                            if self.operation(id).adopted_by.is_some()
                                || self.operation(id).orphaned
                            {
                                break;
                            }
                            if Instant::now() >= deadline {
                                return Err("保存仍在进行，请核对原任务结果".into());
                            }
                            std::thread::sleep(Duration::from_millis(100));
                            reply = self.ui_plugin_call_observed(
                                view,
                                "ui.beforeClose",
                                Value::Null,
                                None,
                                Duration::from_secs(5)
                                    .min(deadline.saturating_duration_since(Instant::now())),
                            )?;
                        }
                        self.component_ui
                            .lock()
                            .unwrap()
                            .update_metadata(&view.id, &reply["metadata"])?;
                        if self
                            .component_ui
                            .lock()
                            .unwrap()
                            .resource_view(&view.id)?
                            .dirty
                        {
                            return Err("组件仍有未保存修改，请返回原页面处理".into());
                        }
                        self.close_mutate(id, |o| {
                            o.saved.push(view.id.clone());
                            o.saving_view = None;
                        });
                    }
                }
                if self.operation(id).cancel {
                    return Err("关闭已取消".into());
                }
                self.close_mutate(id, |o| {
                    o.phase = Phase::Cleaning;
                    o.error = None;
                });
            }
            if self.operation(id).phase == Phase::Cleaning {
                let views = self.close_views(id);
                let global = matches!(self.operation(id).target, Target::Application { .. });
                let deadline = Instant::now() + Duration::from_secs(6);
                let op = self.operation(id);
                if !global && !matches!(op.target, Target::Views { .. }) {
                    for workspace in &op.workspaces {
                        self.connections.cancel_workspace(workspace);
                    }
                    if let Target::Window { id } = &op.target {
                        self.connections.cancel_window(id);
                    }
                }
                while self
                    .operation(id)
                    .adopted
                    .iter()
                    .any(|child| self.operation(child).working)
                {
                    if Instant::now() >= deadline {
                        return Err("已接管的关闭调用仍在收尾".into());
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                for view in &views {
                    self.terminals.cancel(|o| o.view == view.id);
                    self.executions.cancel(|o| o.view == view.id);
                    self.network.cancel(|o| o.view == view.id);
                    self.files.cancel(|o| o.view == view.id);
                }
                if !global {
                    self.wait_calls(&views, deadline.saturating_duration_since(Instant::now()))?;
                }
                let mut disposal_errors = Vec::new();
                for view in &views {
                    if !self.operation(id).disposed.contains(&view.id) {
                        let stopped = self
                            .instances
                            .lock()
                            .unwrap()
                            .get(&view.plugin_id)
                            .is_none_or(|i| {
                                i.view().cleanup_complete
                                    || i.view().instance_id.as_ref()
                                        != Some(&view.plugin_instance_id)
                            });
                        let receipt = self.operation(id).dispose_requests.get(&view.id).cloned();
                        let outcome = receipt.as_ref().and_then(|r| r.lock().unwrap().clone());
                        if stopped || matches!(outcome, Some(Ok(_))) {
                            self.close_mutate(id, |o| o.disposed.push(view.id.clone()));
                        } else if receipt.is_some() {
                            if let Some(Err(error)) = outcome {
                                self.close_mutate(id, |o| {
                                    o.dispose_requests.remove(&view.id);
                                });
                                disposal_errors.push(format!("{}：{error}", view.title));
                            } else {
                                disposal_errors.push(format!("{}：释放结果尚未确认", view.title));
                            }
                        } else if view.calls.active() == 0 && Instant::now() < deadline {
                            let receipt = Arc::new(std::sync::Mutex::new(None));
                            self.close_mutate(id, |o| {
                                o.dispose_requests.insert(view.id.clone(), receipt.clone());
                            });
                            let result = self.ui_plugin_call_observed(
                                view,
                                "ui.disposeView",
                                Value::Null,
                                Some(receipt.clone()),
                                Duration::from_secs(5)
                                    .min(deadline.saturating_duration_since(Instant::now())),
                            );
                            if result.is_ok() {
                                self.close_mutate(id, |o| o.disposed.push(view.id.clone()));
                            } else {
                                if view.calls.active() == 0 && receipt.lock().unwrap().is_none() {
                                    *receipt.lock().unwrap() = Some(result.clone());
                                }
                                disposal_errors.push(format!(
                                    "{}：{}",
                                    view.title,
                                    result.unwrap_err()
                                ));
                            }
                        } else {
                            disposal_errors.push(format!("{}：仍有调用未结束", view.title));
                        }
                    }
                    self.terminals.cancel(|o| o.view == view.id);
                    self.executions.cancel(|o| o.view == view.id);
                    self.network.cancel(|o| o.view == view.id);
                    self.files.cancel(|o| o.view == view.id);
                    if global && !disposal_errors.is_empty() {
                        break;
                    }
                }
                if matches!(self.operation(id).target, Target::Application { .. }) {
                    self.shutdown_resources()?;
                } else {
                    let op = self.operation(id);
                    if !matches!(op.target, Target::Views { .. }) {
                        for workspace in &op.workspaces {
                            self.connections.cancel_workspace(workspace);
                        }
                        if let Target::Window { id } = &op.target {
                            self.connections.cancel_window(id);
                        }
                    }
                    self.wait_resources(
                        |owner| op.views.contains(&owner.view),
                        deadline.saturating_duration_since(Instant::now()),
                    )?;
                    if !disposal_errors.is_empty() {
                        return Err(disposal_errors.join("；"));
                    }
                    let window = if let Target::Window { id } = &op.target {
                        Some(id.as_str())
                    } else {
                        None
                    };
                    let deadline = Instant::now() + Duration::from_secs(3);
                    while !matches!(op.target, Target::Views { .. })
                        && self.connections.pending_close(&op.workspaces, window) > 0
                    {
                        if Instant::now() >= deadline {
                            return Err("连接资源仍在收尾，请继续完成关闭".into());
                        }
                        std::thread::sleep(Duration::from_millis(10));
                    }
                }
                self.wait_calls(&views, Duration::from_secs(1))?;
                let ids = self.operation(id).views;
                if ids.iter().any(|id| !views.iter().any(|v| &v.id == id)) {
                    // A native host was lost while this worker was cleaning.
                    // Drain the expanded scope before removing its identities.
                    scope_changed = true;
                    return Ok(());
                }
                self.component_ui.lock().unwrap().remove_views(&ids);
                self.close_mutate(id, |o| o.phase = Phase::Finalizing);
            }
            if self.operation(id).phase == Phase::Finalizing {
                let op = self.operation(id);
                match &op.target {
                    Target::Workspace { id } => {
                        let mut workspaces = self.workspaces.lock().unwrap();
                        if workspaces.ids().contains(id) {
                            workspaces.close(&op.window, id)?;
                        }
                    }
                    Target::Window { id } => {
                        self.workspaces.lock().unwrap().close_window(id)?;
                    }
                    Target::Application { .. } => {
                        self.workspaces.lock().unwrap().clear();
                    }
                    Target::Views { .. } => {}
                }
                let finalizer = self.finalizer.lock().unwrap().clone();
                if let Some(finalizer) = finalizer {
                    finalizer(op.target.clone())?;
                }
                if self.operation(id).target != op.target {
                    self.close_mutate(id, |o| o.phase = Phase::Cleaning);
                    scope_changed = true;
                    return Ok(());
                }
                self.close_mutate(id, |o| {
                    o.phase = Phase::Completed;
                    o.working = false;
                    o.ended_at = Some(crate::core::now());
                });
                for child in self.operation(id).adopted {
                    self.close_mutate(&child, |o| {
                        o.phase = Phase::Completed;
                        o.working = false;
                        o.ended_at = Some(crate::core::now());
                    });
                }
                self.workspace_changed();
            }
            Ok::<(), String>(())
        })();
        if self.close_handed_off(id) {
            return;
        }
        if scope_changed {
            self.close_execute(id, decision);
            return;
        }
        if let Err(error) = result {
            let phase = self.operation(id).phase;
            if self.operation(id).orphaned && matches!(phase, Phase::Preparing | Phase::Deciding) {
                self.close_mutate(id, |o| o.phase = Phase::Cleaning);
                self.close_execute(id, "discard");
                return;
            }
            if matches!(phase, Phase::Preparing | Phase::Deciding) {
                self.close_decision_error(id, error);
            } else {
                self.close_mutate(id, |o| {
                    o.phase = Phase::Failed;
                    o.failed_stage = Some(phase);
                    o.error = Some(error);
                    o.working = false;
                });
            }
        }
    }
}

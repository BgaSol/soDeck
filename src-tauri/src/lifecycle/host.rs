//! A missing native window has no remaining UI that can confirm or save input.
//! Its actual owned scope is interrupted, then drained by the ordinary close worker.
use super::*;

impl AppCore {
    pub fn close_unhosted(self: &Arc<Self>, id: &str) -> bool {
        let window = {
            let mut ui = self.component_ui.lock().unwrap();
            let Some(op) = ui.lifecycle.operations.get(id) else {
                return false;
            };
            if op.phase != Phase::Preparing
                || !op.views.is_empty()
                || op.inputs.keys().any(|w| ui.lifecycle.hosts.contains(w))
            {
                return false;
            }
            let op = ui.lifecycle.operations.get_mut(id).unwrap();
            op.unhosted = true;
            op.inputs.values_mut().for_each(|v| *v = Some(json!({})));
            op.working = true;
            op.window.clone()
        };
        let core = self.clone();
        let id = id.to_owned();
        std::thread::spawn(move || {
            core.close_check_views(&id);
            let Ok(op) = core.close_status(&window, &id) else {
                return;
            };
            if op.phase != Phase::Deciding {
                return;
            }
            if let Ok(op) = core.close_decide(&window, &id, op.decision_revision, "discard") {
                for w in op.inputs.keys() {
                    let _ = core.close_report_inputs(w, &id, op.input_round, json!({}));
                }
            }
        });
        true
    }

    pub fn close_host_lost(self: &Arc<Self>, window: &str) {
        let scope = {
            let state = self.workspaces.lock().unwrap();
            let Ok(projection) = state.projection(window) else {
                let mut ui = self.component_ui.lock().unwrap();
                ui.lifecycle.hosts.remove(window);
                ui.remove_window_surfaces(window);
                self.sync_shared_ui_locked(&ui);
                return;
            };
            if window == "main" {
                state.ids()
            } else {
                projection.window.workspace_ids
            }
        };
        let existing = {
            let ui = self.component_ui.lock().unwrap();
            let mut operations = ui
                .lifecycle
                .operations
                .values()
                .filter(|o| {
                    !o.phase.terminal()
                        && o.adopted_by.is_none()
                        && (window == "main"
                            || o.inputs.contains_key(window)
                            || scope.iter().any(|w| o.workspaces.contains(w)))
                })
                .collect::<Vec<_>>();
            operations.sort_by_key(|o| !matches!(o.target, Target::Application { .. }));
            operations
                .into_iter()
                .map(|o| o.id.clone())
                .collect::<Vec<_>>()
        };
        let id = if let Some(id) = existing.first() {
            id.clone()
        } else {
            let target = if window == "main" {
                Target::Application {
                    action: Action::Exit,
                }
            } else {
                Target::Window { id: window.into() }
            };
            match self.close_prepare_internal(window, &format!("lost-{window}"), target, true) {
                Ok(op) => op.id,
                Err(error) => {
                    self.log(
                        None,
                        "error",
                        "窗口中断后的关闭准备失败",
                        Some(json!({"windowId":window,"error":error})),
                    );
                    return;
                }
            }
        };
        let (start, global, round) = {
            let mut state = self.workspaces.lock().unwrap();
            let mut ui = self.component_ui.lock().unwrap();
            ui.lifecycle.hosts.remove(window);
            ui.lifecycle.lost_hosts.insert(window.into());
            ui.invalidate_workspace_actions(&scope, "原生窗口已关闭，行为已失效");
            ui.remove_window_surfaces(window);
            self.sync_shared_ui_locked(&ui);
            let op = &ui.lifecycle.operations[&id];
            // Normal destroy occurs after the parent registry has been finalized.
            if op.phase == Phase::Finalizing
                && (window != "main" || matches!(op.target, Target::Application { .. }))
            {
                return;
            }
            let global = window == "main" || matches!(op.target, Target::Application { .. });
            let views = scope
                .iter()
                .filter_map(|w| ui.workspaces.get(w))
                .flat_map(|w| w.views.iter().map(|v| v.id.clone()))
                .collect::<Vec<_>>();
            let siblings = existing
                .iter()
                .filter(|other| *other != &id)
                .map(|other| ui.lifecycle.operations[other].clone())
                .collect::<Vec<_>>();
            for other in &siblings {
                ui.lifecycle
                    .operations
                    .get_mut(&other.id)
                    .unwrap()
                    .adopted_by = Some(id.clone());
            }
            let op = ui.lifecycle.operations.get_mut(&id).unwrap();
            if op.window == window {
                op.window = "main".into();
            }
            op.error = Some("原生窗口意外关闭，未持久化输入无法恢复；正在释放其资源".into());
            op.decision_revision += 1;
            op.revision += 1;
            if global && window != "main" {
                op.inputs.insert(window.into(), None);
                (false, true, op.input_round)
            } else {
                op.orphaned = true;
                op.cancel = false;
                if window == "main" {
                    op.target = Target::Application {
                        action: Action::Exit,
                    };
                    self.set_closing(true);
                } else if !global {
                    op.target = Target::Window { id: window.into() };
                }
                for w in &scope {
                    if !op.workspaces.contains(w) {
                        op.workspaces.push(w.clone());
                    }
                }
                for view in views {
                    if !op.views.contains(&view) {
                        op.views.push(view);
                    }
                }
                for other in siblings {
                    op.adopted.push(other.id);
                    op.saved.extend(other.saved);
                    op.save_started.extend(other.save_started);
                    op.disposed.extend(other.disposed);
                    op.dispose_requests.extend(other.dispose_requests);
                }
                op.inputs
                    .values_mut()
                    .for_each(|v| *v = Some(json!({"unconfirmed":true})));
                let start = !op.working;
                if start {
                    op.phase = Phase::Cleaning;
                    op.working = true;
                }
                state.cancel_moves_for_close(&scope, Some(window), false);
                (start, false, op.input_round)
            }
        };
        for workspace in &scope {
            self.connections.cancel_workspace(workspace);
        }
        self.connections.cancel_window(window);
        self.terminals
            .cancel(|o| scope.contains(&o.context.workspace_id));
        self.executions
            .cancel(|o| scope.contains(&o.context.workspace_id));
        self.network
            .cancel(|o| scope.contains(&o.context.workspace_id));
        self.files
            .cancel(|o| scope.contains(&o.context.workspace_id));
        self.log(
            None,
            "warn",
            "原生窗口意外中断",
            Some(json!({"windowId":window,"operationId":id,"workspaces":scope})),
        );
        self.event(
            "core-exit-error",
            json!("窗口意外关闭，未持久化输入无法恢复；资源正在收尾"),
        );
        if global {
            let _ = self.close_report_inputs(
                window,
                &id,
                round,
                json!({"error":"窗口已消失，输入无法确认","unconfirmed":true}),
            );
        } else if start {
            let core = self.clone();
            let key = id.clone();
            std::thread::spawn(move || core.close_execute(&key, "discard"));
        }
        self.close_changed(&id);
    }
}

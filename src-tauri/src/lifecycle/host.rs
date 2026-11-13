//! A missing native window has no remaining UI that can confirm or save input.
//! Its actual owned scope is interrupted, then drained by the ordinary close worker.
use super::*;

impl AppCore {
    pub fn close_unhosted(self: &Arc<Self>, id: &str) -> bool {
        let window = {
            let mut ui = self.ui_state();
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

    /// Losing a display does not end the business context or its workers.
    pub fn close_host_lost(self: &Arc<Self>, window: &str) {
        let mut state = self.workbench_state();
        let crate::workbench::State {
            workspaces,
            ui,
            commands,
        } = &mut *state;
        commands.lost_host(window, ui);
        ui.lifecycle.hosts.remove(window);
        let surfaces: Vec<_> = ui
            .surfaces
            .iter()
            .filter(|(_, s)| s.window == window)
            .map(|(id, _)| id.clone())
            .collect();
        for token in &surfaces {
            self.shared_state
                .remove_surface(window, token, std::time::Instant::now());
        }
        ui.remove_window_surfaces(window);
        // Normal destruction happens after registry finalization. It needs no recovery.
        let Some(lost) = workspaces.windows.get(window).cloned() else {
            return;
        };
        if window == "main"
            && self.is_closing()
            && ui
                .lifecycle
                .operations
                .values()
                .filter(|o| !o.phase.terminal() && o.adopted_by.is_none())
                .all(|o| matches!(o.phase, Phase::Preparing | Phase::Deciding))
        {
            for o in ui
                .lifecycle
                .operations
                .values_mut()
                .filter(|o| !o.phase.terminal() && o.adopted_by.is_none())
            {
                o.phase = Phase::Aborted;
                o.cancel = true;
                o.error = Some("主窗口已消失，尚未开始释放的退出已取消".into());
                o.ended_at = Some(crate::core::now());
                o.revision += 1;
            }
            self.set_closing(false);
        }
        if self.is_closing()
            || ui.lifecycle.operations.values().any(|o| {
                o.phase == Phase::Finalizing && matches!(&o.target,Target::Window{id} if id==window)
            })
        {
            let mut reports = vec![];
            for op in ui
                .lifecycle
                .operations
                .values_mut()
                .filter(|o| !o.phase.terminal() && o.inputs.contains_key(window))
            {
                op.inputs.insert(
                    window.into(),
                    Some(json!({"unconfirmed":true,"error":"窗口已消失，未同步输入无法确认"})),
                );
                if op.window == window {
                    op.window = "main".into();
                }
                op.revision += 1;
                reports.push((op.id.clone(), op.input_round));
            }
            drop(state);
            for (id, round) in reports {
                let _ = self.close_report_inputs(
                    window,
                    &id,
                    round,
                    json!({"unconfirmed":true,"error":"窗口已消失，输入无法确认"}),
                );
            }
            self.workspace_changed();
            return;
        }
        let moves: Vec<_> = ui
            .transfers
            .values()
            .filter(|t| t.movement.source == window || t.movement.target == window)
            .map(|t| t.movement.clone())
            .collect();
        for m in moves {
            self.cancel_transfer_locked(workspaces, ui, &m);
        }
        for m in workspaces
            .pending_moves()
            .into_iter()
            .filter(|m| m.source == window || m.target == window)
        {
            workspaces.cancel(&m);
        }
        let mut continuing = vec![];
        for o in ui.lifecycle.operations.values_mut().filter(|o| {
            !o.phase.terminal()
                && o.adopted_by.is_none()
                && (o.window == window || o.inputs.contains_key(window))
        }) {
            if matches!(o.phase, Phase::Preparing | Phase::Deciding)
                || o.phase == Phase::Failed
                    && matches!(o.failed_stage, Some(Phase::Preparing | Phase::Deciding))
            {
                o.phase = Phase::Aborted;
                o.cancel = true;
                o.ended_at = Some(crate::core::now());
                o.error = Some("窗口已消失，关闭已取消；页面与任务已恢复".into());
            } else {
                continuing.extend(
                    o.views
                        .iter()
                        .filter(|id| !o.returning_tools.contains(id))
                        .cloned(),
                );
            }
            if o.window == window {
                o.window = "main".into();
            }
            o.revision += 1;
        }
        let mut recovered = vec![];
        for workspace in &lost.workspace_ids {
            let target = workspaces
                .host_windows(workspace)
                .into_iter()
                .filter(|w| w != window)
                .min_by_key(|w| (w != "main", w.clone()))
                .unwrap_or_else(|| "main".into());
            let Some(w) = ui.workspaces.get_mut(workspace) else {
                continue;
            };
            let Some(mut host) = w.hosts.remove(window) else {
                continue;
            };
            for view in &continuing {
                host.remove(view);
            }
            let ids = host.view_ids();
            if let Some(existing) = w.hosts.get_mut(&target) {
                if existing
                    .merge_host(host.clone(), format!("recovery-{}", self.next_id()))
                    .is_err()
                {
                    // A full 16-group layout can still recover every instance into its root.
                    let group = existing.focused_group_id.clone();
                    for id in &ids {
                        let is_tool = w.views.iter().find(|v| v.id == *id).is_some_and(|v| {
                            ui.definitions
                                .get(&v.definition_key)
                                .is_some_and(|d| d.definition.container == "sidebar")
                        });
                        if is_tool {
                            let key = &w.views.iter().find(|v| v.id == *id).unwrap().definition_key;
                            let _ = existing.dock(id, key, "right", false);
                        } else if host.dialogs.contains(id) {
                            existing.dialogs.push(id.clone());
                        } else {
                            let _ = existing.place(id, &group, usize::MAX, false);
                        }
                    }
                }
            } else {
                w.hosts.insert(target.clone(), host);
            }
            workspaces.add_host(&target, workspace, usize::MAX).ok();
            if target != window {
                workspaces.remove_host(window, workspace);
            }
            if w.focused_window_id.as_deref() == Some(window) {
                w.focused_window_id = Some(target.clone());
            }
            recovered.extend(ids);
        }
        if window != "main" {
            workspaces.close_window(window).ok();
        } else if let Some(w) = workspaces.windows.get_mut(window) {
            w.ready = false;
        }
        self.sync_shared_ui_locked(ui);
        drop(state);
        self.connections.cancel_window(window);
        self.log(
            None,
            "warn",
            "窗口显示已恢复",
            Some(json!({"windowId":window,"viewIds":recovered})),
        );
        let _ = self.publish(
            None,
            crate::model::PublishMessage {
                title: "窗口意外关闭，页面已恢复".into(),
                body:
                    "任务和已同步状态保留在可用工作台。尚未同步到核心的输入可能丢失，请核对草稿。"
                        .into(),
                level: "warning".into(),
                notify: true,
                dedupe_key: Some(format!("window-recovery:{window}")),
            },
        );
        self.workspace_changed();
    }
}

//! Core-owned close operations. State shares the view admission mutex, so a
//! reservation and a view/resource commit cannot pass each other between locks.
use crate::{component_ui::View, AppCore};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::HashMap, sync::Arc};

mod close;
mod host;

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Target {
    Views {
        ids: Vec<String>,
    },
    Workspace {
        id: String,
    },
    Host {
        id: String,
    },
    #[serde(rename_all = "camelCase")]
    Group {
        id: String,
        workspace_id: String,
    },
    Window {
        id: String,
    },
    Application {
        action: Action,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn global_input_deadline_reports_unknown_and_keeps_resources_reserved_until_a_decision() {
        let root =
            std::env::temp_dir().join(format!("sodeck-close-deadline-{}", std::process::id()));
        let core = AppCore::open(root.clone(), "test".into()).unwrap();
        core.register_close_host("main").unwrap();
        let op = core
            .close_prepare(
                "main",
                "deadline",
                Target::Application {
                    action: Action::Exit,
                },
            )
            .unwrap();
        core.expire_close_inputs(std::time::Instant::now() + std::time::Duration::from_secs(46));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        loop {
            let current = core.close_status("main", &op.id).unwrap();
            if current.phase == Phase::Deciding {
                assert_eq!(
                    current.inputs["main"].as_ref().unwrap()["unconfirmed"],
                    true
                );
                assert!(current.error.is_some());
                assert!(core.is_closing());
                core.close_decide("main", &op.id, current.decision_revision, "cancel")
                    .unwrap();
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        while core.close_status("main", &op.id).unwrap().phase != Phase::Aborted {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(!core.is_closing());
        {
            let mut ui = core.ui_state();
            ui.lifecycle.lost_hosts.insert("main".into());
            ui.lifecycle.operations.get_mut(&op.id).unwrap().ended_at =
                Some(crate::core::now() - 60_001);
        }
        core.expire_close_inputs(std::time::Instant::now());
        {
            let ui = core.ui_state();
            assert!(!ui.lifecycle.operations.contains_key(&op.id));
            assert!(ui.lifecycle.lost_hosts.is_empty());
        }
        drop(core);
        let _ = std::fs::remove_dir_all(root);
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Action {
    Exit,
    Restart,
    InstallAndRestart,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Preparing,
    Deciding,
    Cleaning,
    Finalizing,
    Failed,
    Completed,
    Aborted,
}
impl Phase {
    pub(crate) fn terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Aborted)
    }
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Operation {
    pub id: String,
    pub window: String,
    pub target: Target,
    pub phase: Phase,
    pub revision: u64,
    pub input_round: u64,
    pub decision_revision: u64,
    pub workspaces: Vec<String>,
    pub ending_workspaces: Vec<String>,
    pub returning_tools: Vec<String>,
    pub return_moves: Vec<String>,
    pub views: Vec<String>,
    pub surfaces: HashMap<String, Vec<String>>,
    pub inputs: HashMap<String, Option<Value>>,
    pub decision: Option<String>,
    pub dirty: Vec<Value>,
    pub error: Option<String>,
    pub failed_stage: Option<Phase>,
    pub adopted_by: Option<String>,
    #[serde(skip)]
    pub(crate) request: String,
    #[serde(skip)]
    pub(crate) working: bool,
    #[serde(skip)]
    pub(crate) cancel: bool,
    #[serde(skip)]
    pub(crate) saved: Vec<String>,
    #[serde(skip)]
    pub(crate) save_started: Vec<String>,
    #[serde(skip)]
    pub(crate) disposed: Vec<String>,
    #[serde(skip)]
    pub(crate) dispose_requests: HashMap<String, crate::plugins::Receipt>,
    #[serde(skip)]
    pub(crate) saving_view: Option<String>,
    #[serde(skip)]
    pub(crate) orphaned: bool,
    #[serde(skip)]
    pub(crate) unhosted: bool,
    #[serde(skip)]
    pub(crate) adopted: Vec<String>,
    #[serde(skip)]
    pub(crate) ended_at: Option<u64>,
    #[serde(skip)]
    pub(crate) input_deadline: Option<std::time::Instant>,
}
#[derive(Default)]
pub(crate) struct Lifecycle {
    pub(crate) hosts: std::collections::HashSet<String>,
    pub(crate) lost_hosts: std::collections::HashSet<String>,
    pub(crate) operations: HashMap<String, Operation>,
}
impl Lifecycle {
    pub(crate) fn view_frozen(&self, view: &str) -> bool {
        self.operations.values().any(|o| {
            !o.phase.terminal() && o.adopted_by.is_none() && o.views.iter().any(|id| id == view)
        })
    }
    pub(crate) fn host_closing(&self, workspace: &str, window: &str) -> bool {
        self.frozen(workspace)
            || self.window_closing(window)
            || self.operations.values().any(|o| {
                !o.phase.terminal()
                    && o.adopted_by.is_none()
                    && o.window == window
                    && matches!(&o.target,Target::Host{id} if id==workspace)
            })
    }
    pub(crate) fn view_input_allowed(&self, view: &str, window: &str) -> bool {
        self.operations
            .values()
            .filter(|o| {
                !o.phase.terminal() && o.adopted_by.is_none() && o.views.iter().any(|id| id == view)
            })
            .all(|o| {
                o.phase == Phase::Preparing && o.inputs.get(window).is_some_and(Option::is_none)
            })
    }
    pub(crate) fn view_input_work_allowed(&self, view: &str) -> bool {
        self.operations
            .values()
            .filter(|o| {
                !o.phase.terminal() && o.adopted_by.is_none() && o.views.iter().any(|id| id == view)
            })
            .all(|o| matches!(o.phase, Phase::Preparing | Phase::Deciding))
    }
    pub(crate) fn frozen(&self, workspace: &str) -> bool {
        self.operations.values().any(|o| {
            !o.phase.terminal()
                && o.adopted_by.is_none()
                && o.ending_workspaces.iter().any(|w| w == workspace)
        })
    }
    pub(crate) fn window_closing(&self, window: &str) -> bool {
        self.operations.values().any(|o| {
            !o.phase.terminal()
                && o.adopted_by.is_none()
                && (matches!(&o.target, Target::Window {id} if id==window)
                    || matches!(o.target, Target::Application { .. }))
        })
    }
    fn reap(&mut self) {
        let now = crate::core::now();
        self.operations
            .retain(|_, o| o.ended_at.is_none_or(|at| now.saturating_sub(at) < 60_000));
        if self.operations.len() >= 160 {
            if let Some(id) = self
                .operations
                .values()
                .filter(|o| o.phase.terminal())
                .min_by_key(|o| o.ended_at)
                .map(|o| o.id.clone())
            {
                self.operations.remove(&id);
            }
        }
        self.lost_hosts.retain(|window| {
            self.operations
                .values()
                .any(|op| !op.phase.terminal() && op.inputs.contains_key(window))
        });
    }
}

impl AppCore {
    pub fn register_close_host(&self, window: &str) -> Result<(), String> {
        let mut _workbench = self.workbench_state();
        let crate::workbench::State { workspaces, ui } = &mut *_workbench;
        workspaces.projection(window)?;

        if ui
            .lifecycle
            .operations
            .values()
            .any(|o| !o.phase.terminal() && o.unhosted && o.inputs.contains_key(window))
        {
            return Err("应用已在界面启动前进入关闭流程".into());
        }
        ui.lifecycle.hosts.insert(window.into());
        ui.lifecycle.lost_hosts.remove(window);
        Ok(())
    }
    pub fn close_host_registered(&self, window: &str) -> bool {
        self.ui_state().lifecycle.hosts.contains(window)
    }
    pub fn close_operations(&self, window: &str) -> Vec<Operation> {
        self.ui_state()
            .lifecycle
            .operations
            .values()
            .filter(|o| {
                !o.phase.terminal() && (o.window == window || o.inputs.contains_key(window))
            })
            .cloned()
            .collect()
    }
    pub fn close_exit_after_failure(
        self: &Arc<Self>,
        window: &str,
        id: &str,
    ) -> Result<Operation, String> {
        {
            let mut ui = self.ui_state();
            let op = ui
                .lifecycle
                .operations
                .get_mut(id)
                .ok_or("关闭操作已失效")?;
            if op.window != window
                || op.phase != Phase::Failed
                || op.failed_stage != Some(Phase::Finalizing)
                || op.working
                || !matches!(op.target, Target::Application { .. })
            {
                return Err("只有已结束的应用收尾失败可以改为退出".into());
            }
            op.target = Target::Application {
                action: Action::Exit,
            };
        }
        self.close_retry(window, id)
    }
    pub fn close_prepare(
        self: &Arc<Self>,
        window: &str,
        request: &str,
        target: Target,
    ) -> Result<Operation, String> {
        self.close_prepare_internal(window, request, target, false)
    }
    /// Native events are new user intents. Active targets still coalesce, but
    /// a later click must not inherit a previous event's cancelled receipt.
    pub fn close_native_prepare(
        self: &Arc<Self>,
        window: &str,
        target: Target,
    ) -> Result<Operation, String> {
        self.close_prepare(window, &self.next_id(), target)
    }
    fn close_prepare_internal(
        self: &Arc<Self>,
        window: &str,
        request: &str,
        mut target: Target,
        interrupted: bool,
    ) -> Result<Operation, String> {
        if request.is_empty() || request.len() > 128 {
            return Err("关闭请求 ID 无效".into());
        }
        if matches!(&target,Target::Window{id} if id=="main") {
            target = Target::Application {
                action: Action::Exit,
            };
        }
        if matches!(target, Target::Application { .. }) && window != "main" {
            return Err("应用操作须由主窗口发起".into());
        }
        let updates = self.updates.lock().unwrap();
        if matches!(target, Target::Application { .. })
            && !interrupted
            && (!updates.busy.is_empty() || updates.checking_all)
        {
            return Err("请等待更新任务完成后关闭或重启".into());
        }
        let mut _workbench = self.workbench_state();
        let crate::workbench::State { workspaces, ui } = &mut *_workbench;
        workspaces.projection(window)?;
        if let Target::Workspace { id } | Target::Host { id } = &target {
            if workspaces.closing_last_secondary(window, id)
                && (matches!(target, Target::Host { .. }) || workspaces.host_windows(id).len() == 1)
            {
                // Closing the last tab ends its native host too. Reserve the
                // whole window before collecting its settings and pending work.
                target = Target::Window { id: window.into() };
            }
        }
        let all: Vec<_> = workspaces
            .windows
            .keys()
            .flat_map(|id| workspaces.projection(id).unwrap().workspaces)
            .collect();

        ui.lifecycle.reap();
        if let Some(op) = ui
            .lifecycle
            .operations
            .values()
            .find(|o| o.window == window && o.request == request)
            .cloned()
        {
            if op.target != target {
                return Err("相同关闭请求不能更改目标".into());
            }
            return Ok(op);
        }
        if let Some(op) = ui
            .lifecycle
            .operations
            .values()
            .find(|o| o.window == window && o.target == target && !o.phase.terminal())
            .cloned()
        {
            return Ok(op);
        }
        if ui
            .lifecycle
            .operations
            .values()
            .filter(|o| !o.phase.terminal() && o.adopted_by.is_none())
            .count()
            >= 31
            && !matches!(target, Target::Application { .. })
        {
            return Err("关闭操作已满，请先处理已有操作".into());
        }
        let scope: Vec<String> = match &target {
            Target::Application { .. } => {
                all.iter().map(|w| w.context.workspace_id.clone()).collect()
            }
            Target::Window { id } => {
                if id != window {
                    return Err("窗口不属于此关闭请求".into());
                }
                workspaces.projection(id)?.window.workspace_ids
            }
            Target::Workspace { id } | Target::Host { id } => {
                if !all
                    .iter()
                    .any(|w| &w.context.workspace_id == id && workspaces.contains(window, id))
                {
                    return Err("工作区已移动或关闭".into());
                }
                vec![id.clone()]
            }
            Target::Group { id, workspace_id } => {
                if !workspaces.contains(window, workspace_id)
                    || ui.workspaces.get(workspace_id).is_none_or(|w| {
                        w.host(window).is_err() || !w.host(window).unwrap().groups.contains_key(id)
                    })
                {
                    return Err("标签组已移动或关闭".into());
                }
                vec![workspace_id.clone()]
            }
            Target::Views { ids } => {
                if ids.is_empty() || ids.len() > 64 {
                    return Err("关闭页面集合无效".into());
                }
                let mut scope = Vec::new();
                let mut seen = std::collections::HashSet::new();
                for id in ids {
                    if !seen.insert(id) {
                        return Err("关闭页面集合重复".into());
                    }
                    let v = ui.resource_view(id)?;
                    if !all.iter().any(|w| {
                        w.context == v.context
                            && ui.workspaces[&v.context.workspace_id]
                                .view_window(&v.id)
                                .as_deref()
                                == Some(window)
                    }) {
                        return Err("页面不属于此窗口".into());
                    }
                    if ui
                        .definitions
                        .get(&v.definition_key)
                        .is_some_and(|d| d.definition.background_on_workspace)
                    {
                        return Err("后台组件请关闭工作区；收起面板不会停止后台任务".into());
                    }
                    if !scope.contains(&v.context.workspace_id) {
                        scope.push(v.context.workspace_id.clone());
                    }
                }
                scope
            }
        };
        let ending_workspaces: Vec<String> = scope
            .iter()
            .filter(|id| match &target {
                Target::Application { .. } | Target::Workspace { .. } => true,
                Target::Host { .. } | Target::Window { .. } => {
                    workspaces.host_windows(id).iter().all(|w| w == window)
                }
                Target::Views { .. } | Target::Group { .. } => false,
            })
            .cloned()
            .collect();
        let mut views: Vec<View> = scope
            .iter()
            .filter_map(|id| ui.workspaces.get(id))
            .flat_map(|w| &w.views)
            .filter(|v| match &target {
                Target::Views { ids } => ids.contains(&v.id),
                Target::Group { id, workspace_id } => {
                    v.context.workspace_id == *workspace_id
                        && ui.workspaces[workspace_id].host(window).is_ok_and(|h| {
                            h.groups.get(id).is_some_and(|g| g.view_ids.contains(&v.id))
                        })
                }
                Target::Host { .. } | Target::Window { .. } => {
                    ui.workspaces[&v.context.workspace_id]
                        .view_window(&v.id)
                        .as_deref()
                        == Some(window)
                        || ending_workspaces.contains(&v.context.workspace_id) && !v.ui_requested
                }
                _ => true,
            })
            .cloned()
            .collect();
        loop {
            let children: Vec<_> = scope
                .iter()
                .filter_map(|id| ui.workspaces.get(id))
                .flat_map(|w| &w.views)
                .filter(|v| {
                    v.parent_view_id
                        .as_ref()
                        .is_some_and(|p| views.iter().any(|v| &v.id == p))
                        && !views.iter().any(|current| current.id == v.id)
                })
                .cloned()
                .collect();
            if children.is_empty() {
                break;
            }
            views.extend(children);
        }
        views.sort_by_key(|v| v.parent_view_id.is_none());
        let returning_tools: Vec<String> = views
            .iter()
            .filter(|v| {
                ui.definitions[&v.definition_key].definition.container == "sidebar"
                    && (matches!(target, Target::Group { .. })
                        || matches!(target, Target::Host { .. } | Target::Window { .. })
                            && !ending_workspaces.contains(&v.context.workspace_id))
            })
            .map(|v| v.id.clone())
            .collect();
        let mut adopted = Vec::new();
        for op in ui
            .lifecycle
            .operations
            .values()
            .filter(|o| !o.phase.terminal() && o.adopted_by.is_none())
        {
            if matches!(target, Target::Application { .. })
                || matches!(op.target, Target::Application { .. })
                || ending_workspaces.iter().any(|w| op.workspaces.contains(w))
                || op.ending_workspaces.iter().any(|w| scope.contains(w))
                || views.iter().any(|v| op.views.contains(&v.id))
                || matches!(target, Target::Window { .. }) && op.inputs.contains_key(window)
            {
                if matches!(target, Target::Application { .. })
                    && op.phase == Phase::Failed
                    && !op.working
                {
                    adopted.push(op.id.clone());
                } else {
                    return Err("已有关闭操作，请先处理当前操作".into());
                }
            }
        }
        let close_window = if let Target::Window { id } = &target {
            Some(id.as_str())
        } else {
            None
        };
        let cancelled: Vec<_> = ui
            .transfers
            .values()
            .filter(|t| {
                matches!(target, Target::Application { .. })
                    || ending_workspaces.contains(&t.movement.workspace_id)
                    || close_window
                        .is_some_and(|w| t.movement.source == w || t.movement.target == w)
                    || matches!(target, Target::Host { .. })
                        && t.movement.workspace_id == scope[0]
                        && (t.movement.source == window || t.movement.target == window)
                    || views.iter().any(|v| t.views.contains(&v.id))
                    || matches!(&target,Target::Group{id,..} if t.blocks(window,Some(id),None,None))
            })
            .map(|t| t.movement.clone())
            .collect();
        for m in cancelled {
            self.cancel_transfer_locked(workspaces, ui, &m);
        }
        for m in workspaces
            .pending_moves()
            .into_iter()
            .filter(|m| !ui.transfers.contains_key(&m.token))
        {
            if matches!(target, Target::Application { .. })
                || scope.contains(&m.workspace_id)
                || close_window.is_some_and(|w| m.source == w || m.target == w)
            {
                workspaces.cancel(&m);
            }
        }
        let surfaces = ui.close_surfaces(&views.iter().map(|v| v.id.clone()).collect::<Vec<_>>());
        // An empty group still needs a host report to advance the close operation.
        let mut inputs = HashMap::from([(window.to_owned(), None)]);
        for window in surfaces.keys() {
            inputs.insert(window.clone(), None);
        }
        for w in &all {
            if ending_workspaces.contains(&w.context.workspace_id) {
                inputs.insert(w.window_id.clone(), None);
            }
        }
        if matches!(target, Target::Application { .. }) {
            for host in &ui.lifecycle.hosts {
                inputs.insert(host.clone(), None);
            }
            for w in workspaces.windows.keys() {
                inputs.insert(w.clone(), None);
            }
        }
        let id = self.next_id();
        if let Target::Window { id } = &target {
            inputs.insert(id.clone(), None);
        }
        for old in &adopted {
            ui.lifecycle.operations.get_mut(old).unwrap().adopted_by = Some(id.clone());
        }
        let disposed = adopted
            .iter()
            .flat_map(|id| ui.lifecycle.operations[id].disposed.clone())
            .collect();
        let saved = adopted
            .iter()
            .flat_map(|id| ui.lifecycle.operations[id].saved.clone())
            .collect();
        let save_started = adopted
            .iter()
            .flat_map(|id| ui.lifecycle.operations[id].save_started.clone())
            .collect();
        let dispose_requests = adopted
            .iter()
            .flat_map(|id| ui.lifecycle.operations[id].dispose_requests.clone())
            .collect();
        let return_moves = if matches!(target, Target::Group { .. }) {
            vec![]
        } else {
            self.reserve_close_tools(workspaces, ui, window, &id, &returning_tools)?
        };
        let op = Operation {
            id: id.clone(),
            window: window.into(),
            request: request.into(),
            target: target.clone(),
            phase: Phase::Preparing,
            revision: 1,
            input_round: 1,
            decision_revision: 1,
            workspaces: scope,
            ending_workspaces,
            returning_tools,
            return_moves,
            views: views.iter().map(|v| v.id.clone()).collect(),
            surfaces,
            inputs,
            decision: None,
            dirty: Vec::new(),
            error: None,
            failed_stage: None,
            adopted_by: None,
            working: false,
            cancel: false,
            saved,
            save_started,
            disposed,
            dispose_requests,
            saving_view: None,
            orphaned: false,
            unhosted: false,
            adopted,
            ended_at: None,
            input_deadline: None,
        };
        ui.invalidate_workspace_actions(&op.ending_workspaces, "工作区正在关闭，行为已失效");
        ui.lifecycle.operations.insert(id.clone(), op.clone());
        if matches!(target, Target::Application { .. }) {
            self.set_closing(true);
        }
        self.sync_shared_ui_locked(&ui);
        drop(_workbench);
        drop(updates);
        self.log(None,"info","关闭操作已保留目标",Some(json!({"operationId":id,"target":target,"workspaces":op.workspaces,"views":op.views})));
        self.close_changed(&id);
        self.event("core-close-inputs", json!(op));
        self.close_input_timeout(&id, 1);
        Ok(op)
    }
    pub fn close_status(&self, window: &str, id: &str) -> Result<Operation, String> {
        let ui = self.ui_state();
        let op = ui.lifecycle.operations.get(id).ok_or("关闭操作已失效")?;
        if op.window != window && !op.inputs.contains_key(window) {
            return Err("关闭操作不属于此窗口".into());
        }
        Ok(op.clone())
    }
    pub fn close_report_inputs(
        self: &Arc<Self>,
        window: &str,
        id: &str,
        round: u64,
        mut report: Value,
    ) -> Result<Operation, String> {
        if !report.is_object() {
            return Err("关闭输入状态必须是对象".into());
        }
        if serde_json::to_vec(&report)
            .map_err(|e| e.to_string())?
            .len()
            > 24 * 1024
        {
            return Err("关闭输入状态超过上限".into());
        }
        let start = {
            let mut ui = self.ui_state();
            let op = ui
                .lifecycle
                .operations
                .get_mut(id)
                .ok_or("关闭操作已失效")?;
            if op.input_round != round {
                return Err("输入同步轮次已失效".into());
            }
            if op.phase != Phase::Preparing
                && !(op.phase == Phase::Deciding && op.decision.is_some())
            {
                return Ok(op.clone());
            }
            let slot = op.inputs.get_mut(window).ok_or("此窗口不是输入参与者")?;
            if round == 1
                && op.surfaces.get(window).is_some_and(|expected| {
                    expected.iter().any(|id| {
                        !report["surfaces"]
                            .as_array()
                            .is_some_and(|list| list.iter().any(|token| token.as_str() == Some(id)))
                    })
                })
            {
                report["error"] = json!("部分原有界面输入无法确认，请返回处理或明确放弃");
            }
            if slot.is_none() {
                *slot = Some(report);
                op.revision += 1;
            }
            if op.inputs.values().all(Option::is_some) && !op.working {
                op.working = true;
                true
            } else {
                false
            }
        };
        if start {
            let core = self.clone();
            let id = id.to_owned();
            std::thread::spawn(move || {
                let decision = core.ui_state().lifecycle.operations[&id].decision.clone();
                if let Some(decision) = decision {
                    core.close_execute(&id, &decision);
                } else {
                    core.close_check_views(&id);
                }
            });
        }
        self.close_changed(id);
        self.close_status(window, id)
    }
    pub fn close_decide(
        self: &Arc<Self>,
        window: &str,
        id: &str,
        revision: u64,
        decision: &str,
    ) -> Result<Operation, String> {
        let start = {
            let mut ui = self.ui_state();
            let lost_hosts = ui.lifecycle.lost_hosts.clone();
            let op = ui
                .lifecycle
                .operations
                .get_mut(id)
                .ok_or("关闭操作已失效")?;
            if op.window != window || op.adopted_by.is_some() {
                return Err("关闭操作已被接管或不属于此窗口".into());
            }
            if op.phase.terminal() {
                return Ok(op.clone());
            }
            if decision == "cancel" && matches!(op.phase, Phase::Preparing | Phase::Deciding) {
                op.cancel = true;
                let start = !op.working
                    && !(op.decision.is_some() && op.inputs.values().any(Option::is_none));
                if start {
                    op.working = true;
                }
                start
            } else {
                if op.phase != Phase::Deciding || op.decision_revision != revision {
                    return Err("关闭内容已改变，请检查当前状态".into());
                }
                if !matches!(decision, "save" | "discard") {
                    return Err("关闭决定无效".into());
                }
                if op.working {
                    return Ok(op.clone());
                }
                if op.decision.is_some() {
                    return Ok(op.clone());
                }
                if decision == "save" && op.error.is_some() {
                    return Err("存在无法确认的状态，请返回处理或明确放弃".into());
                }
                op.decision = Some(decision.into());
                op.input_round += 1;
                for (w, v) in &mut op.inputs {
                    *v = lost_hosts
                        .contains(w)
                        .then(|| json!({"error":"窗口已消失，输入无法确认","unconfirmed":true}));
                }
                op.revision += 1;
                let start = op.inputs.values().all(Option::is_some);
                op.working = start;
                start
            }
        };
        if start {
            let core = self.clone();
            let id = id.to_owned();
            let decision = decision.to_owned();
            std::thread::spawn(move || core.close_execute(&id, &decision));
        } else if decision != "cancel" {
            self.event("core-close-inputs", json!(self.close_status(window, id)?));
            let round = self.close_status(window, id)?.input_round;
            self.close_input_timeout(id, round);
        }
        self.close_status(window, id)
    }
    pub fn close_retry(self: &Arc<Self>, window: &str, id: &str) -> Result<Operation, String> {
        {
            let mut ui = self.ui_state();
            let op = ui
                .lifecycle
                .operations
                .get_mut(id)
                .ok_or("关闭操作已失效")?;
            if op.window != window
                || op.phase != Phase::Failed
                || op.working
                || op.adopted_by.is_some()
            {
                return Err("当前关闭步骤不能重试".into());
            }
            op.phase = op.failed_stage.ok_or("关闭步骤缺失")?;
            // Retry only confirmed failures. A missing reply is still the
            // original request, and must never produce another dispose.
            op.dispose_requests
                .retain(|_, receipt| !matches!(*receipt.lock().unwrap(), Some(Err(_))));
            op.working = true;
            op.error = None;
            op.revision += 1;
        }
        let core = self.clone();
        let key = id.to_owned();
        std::thread::spawn(move || core.close_execute(&key, "discard"));
        self.close_status(window, id)
    }
    fn close_changed(&self, id: &str) {
        let op = self.ui_state().lifecycle.operations.get(id).cloned();
        if let Some(op) = op {
            self.event("core-close-changed", json!(op));
        }
    }
    fn close_input_timeout(self: &Arc<Self>, id: &str, round: u64) {
        let mut ui = self.ui_state();
        if let Some(op) = ui.lifecycle.operations.get_mut(id) {
            if op.input_round == round && matches!(op.phase, Phase::Preparing | Phase::Deciding) {
                op.input_deadline =
                    Some(std::time::Instant::now() + std::time::Duration::from_secs(45));
            }
        }
        drop(ui);
        self.wake_background();
    }
    pub(crate) fn expire_close_inputs(self: &Arc<Self>, now: std::time::Instant) {
        let expired = {
            let mut ui = self.ui_state();
            ui.lifecycle.reap();
            ui.lifecycle
                .operations
                .values_mut()
                .filter_map(|op| {
                    if op.adopted_by.is_some()
                        || !matches!(op.phase, Phase::Preparing | Phase::Deciding)
                        || !op.input_deadline.is_some_and(|deadline| deadline <= now)
                    {
                        return None;
                    }
                    op.input_deadline = None;
                    Some((
                        op.id.clone(),
                        op.input_round,
                        op.inputs
                            .iter()
                            .filter(|(_, v)| v.is_none())
                            .map(|(w, _)| w.clone())
                            .collect::<Vec<_>>(),
                    ))
                })
                .collect::<Vec<_>>()
        };
        for (id, round, windows) in expired {
            for window in windows {
                let _ = self.close_report_inputs(
                    &window,
                    &id,
                    round,
                    json!({"error":"窗口输入同步未完成，请返回处理或明确放弃","unconfirmed":true}),
                );
            }
        }
    }
}

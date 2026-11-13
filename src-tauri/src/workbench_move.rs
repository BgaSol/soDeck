//! Display transfer reservations. Business identities remain in their workspace.
use crate::{component_ui::SurfacePhase, workbench_layout::Host, workspaces::Move, AppCore};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Scope {
    Host,
    View { id: String },
    Group { id: String },
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Destination {
    pub group_id: Option<String>,
    pub side: Option<String>,
    pub index: Option<usize>,
    pub edge: Option<String>,
    pub before_tool_key: Option<String>,
    pub source_revision: Option<u64>,
    pub source_context: Option<crate::workspaces::Context>,
    #[serde(default)]
    pub return_to_source: bool,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReturnLocation {
    pub window_id: String,
    pub group_id: Option<String>,
    pub side: Option<String>,
    pub previous_view_id: Option<String>,
    pub next_view_id: Option<String>,
    pub previous_tool_key: Option<String>,
    pub next_tool_key: Option<String>,
}
impl ReturnLocation {
    fn capture(window: &str, host: &Host, view: Option<&str>, group: Option<&str>) -> Self {
        let group_id = group
            .map(str::to_owned)
            .or_else(|| view.and_then(|v| host.group_of(v)));
        let side = view
            .and_then(|v| {
                if host.left.view_ids().iter().any(|id| id == v) {
                    Some("left")
                } else if host.right.view_ids().iter().any(|id| id == v) {
                    Some("right")
                } else {
                    None
                }
            })
            .map(str::to_owned);
        let ids = group_id
            .as_ref()
            .and_then(|id| host.groups.get(id))
            .map(|g| g.view_ids.clone())
            .or_else(|| {
                side.as_ref().map(|s| {
                    if s == "left" {
                        host.left.view_ids()
                    } else {
                        host.right.view_ids()
                    }
                })
            });
        let at = view.and_then(|v| ids.as_ref()?.iter().position(|id| id == v));
        let tools = side.as_deref().map(|s| {
            if s == "left" {
                &host.left.entries
            } else {
                &host.right.entries
            }
        });
        let slot = view.and_then(|view| {
            tools?
                .iter()
                .position(|e| e.view_id.as_deref() == Some(view))
        });
        Self {
            window_id: window.into(),
            group_id,
            side,
            previous_view_id: at
                .and_then(|i| i.checked_sub(1))
                .and_then(|i| ids.as_ref()?.get(i).cloned()),
            next_view_id: at.and_then(|i| ids.as_ref()?.get(i + 1).cloned()),
            previous_tool_key: slot
                .and_then(|i| i.checked_sub(1))
                .and_then(|i| tools?.get(i))
                .map(|e| e.definition_key.clone()),
            next_tool_key: slot
                .and_then(|i| tools?.get(i + 1))
                .map(|e| e.definition_key.clone()),
        }
    }
}
#[derive(Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Preparing,
    Draining,
    Restoring,
}
#[derive(Clone)]
pub(crate) struct Transfer {
    pub movement: Move,
    pub scope: Scope,
    pub source_after: Option<Host>,
    pub target_after: Host,
    pub views: Vec<String>,
    pub source_before: Host,
    pub target_before: Option<Host>,
    pub destination: Destination,
    pub identities: Vec<(String, crate::workspaces::Context, String)>,
    pub focus_clock: u64,
    pub source_revision: u64,
    pub target_revision: Option<u64>,
    pub phase: Phase,
    pub watermarks: HashMap<String, u64>,
    pub close_owner: Option<String>,
}
pub(crate) struct MoveReceipt {
    fingerprint: Vec<u8>,
    movement: Move,
    result: Option<Result<(), String>>,
    ended_at: Option<std::time::Instant>,
    scope: Scope,
    returning: bool,
}
pub(crate) struct WindowReturn {
    source: String,
    movements: Vec<Move>,
}
impl MoveReceipt {
    pub(crate) fn expired(&self) -> bool {
        self.ended_at
            .is_some_and(|at| at.elapsed() > std::time::Duration::from_secs(300))
    }
}
fn move_fingerprint(
    source: &str,
    id: &str,
    target: &str,
    index: usize,
    scope: &Scope,
    destination: &Destination,
) -> Vec<u8> {
    use sha2::{Digest, Sha256};
    Sha256::digest(
        serde_json::to_vec(&serde_json::json!([
            source,
            id,
            target,
            index,
            scope,
            destination
        ]))
        .unwrap(),
    )
    .to_vec()
}
impl Transfer {
    fn reservations(&self, window: &str) -> Vec<String> {
        if window == self.movement.source {
            if self.source_after.is_none() {
                return vec!["*".into()];
            }
            self.views
                .iter()
                .map(|id| {
                    self.source_before.group_of(id).unwrap_or_else(|| {
                        if self.source_before.left.view_ids().contains(id) {
                            "@left".into()
                        } else {
                            "@right".into()
                        }
                    })
                })
                .chain(match &self.scope {
                    Scope::Group { id } => Some(id.clone()),
                    _ => None,
                })
                .collect()
        } else if window == self.movement.target {
            if self.close_owner.is_some() {
                self.views
                    .iter()
                    .map(|id| {
                        if self.target_after.left.view_ids().contains(id) {
                            "@left".into()
                        } else {
                            "@right".into()
                        }
                    })
                    .collect()
            } else if let Some(side) = &self.destination.side {
                vec![format!("@{side}")]
            } else if matches!(self.scope, Scope::Host) {
                vec!["*".into()]
            } else {
                vec![self
                    .destination
                    .group_id
                    .clone()
                    .unwrap_or_else(|| "*".into())]
            }
        } else {
            vec![]
        }
    }
    fn conflicts(&self, other: &Self) -> bool {
        if self.movement.workspace_id != other.movement.workspace_id {
            return false;
        }
        if self.views.iter().any(|v| other.views.contains(v)) {
            return true;
        }
        [&self.movement.source, &self.movement.target]
            .iter()
            .any(|window| {
                let a = self.reservations(window);
                let b = other.reservations(window);
                !a.is_empty()
                    && !b.is_empty()
                    && (a.iter().chain(&b).any(|k| k == "*") || a.iter().any(|k| b.contains(k)))
            })
    }
    pub(crate) fn blocks(
        &self,
        window: &str,
        group: Option<&str>,
        view: Option<&str>,
        side: Option<&str>,
    ) -> bool {
        let keys = self.reservations(window);
        !keys.is_empty()
            && (keys.iter().any(|k| k == "*")
                || group.is_some_and(|g| keys.iter().any(|k| k == g))
                || side.is_some_and(|s| keys.contains(&format!("@{s}")))
                || view.is_some_and(|v| self.views.iter().any(|id| id == v)))
    }
    fn check_portion(&self, window: &str, current: &Host, before: &Host) -> Result<(), String> {
        for key in self.reservations(window) {
            let equal = match key.as_str() {
                "*" => {
                    current.center == before.center
                        && current.groups.keys().eq(before.groups.keys())
                        && current.groups.iter().all(|(id, g)| {
                            before
                                .groups
                                .get(id)
                                .is_some_and(|b| g.view_ids == b.view_ids)
                        })
                        && current.left.entries == before.left.entries
                        && current.right.entries == before.right.entries
                        && current.dialogs == before.dialogs
                }
                "@left" => current.left.entries == before.left.entries,
                "@right" => current.right.entries == before.right.entries,
                id => current
                    .groups
                    .get(id)
                    .zip(before.groups.get(id))
                    .is_some_and(|(a, b)| {
                        a.view_ids == b.view_ids && a.explicit_empty == b.explicit_empty
                    }),
            };
            if !equal {
                return Err("交接页面或目标标签组已改变".into());
            }
        }
        Ok(())
    }
    pub(crate) fn target_projection(&self, current: Option<&Host>) -> Result<Host, String> {
        let mut target = current.cloned().unwrap_or_else(|| {
            let mut h = Host::new(format!(
                "{}@{}",
                self.movement.workspace_id, self.movement.target
            ));
            h.layout = self.source_before.layout.clone();
            h.layout_slot = self.target_after.layout_slot;
            h.bounds = self.target_after.bounds.clone();
            h
        });
        if self.close_owner.is_some() {
            for view in &self.views {
                target.dock(
                    view,
                    self.target_after.tool_key(view).ok_or("工具槽位已改变")?,
                    if self.target_after.left.view_ids().contains(view) {
                        "left"
                    } else {
                        "right"
                    },
                    false,
                )?;
            }
        } else {
            match &self.scope {
                Scope::Host => target.merge_host(
                    self.source_before.clone(),
                    format!("split-{}", self.movement.token),
                )?,
                Scope::View { id } => {
                    if let Some(side) = &self.destination.side {
                        let key = self.target_after.tool_key(id).ok_or("工具槽位已改变")?;
                        target.dock(id, key, side, true)?;
                        if let Some(before) = self.destination.before_tool_key.as_deref() {
                            target.move_tool(key, side, Some(before))?;
                        }
                    } else {
                        let mut group = self
                            .destination
                            .group_id
                            .clone()
                            .unwrap_or_else(|| target.focused_group_id.clone());
                        if let Some(edge) = self
                            .destination
                            .edge
                            .as_deref()
                            .filter(|e| ["left", "right", "top", "bottom"].contains(e))
                        {
                            let next = format!("group-{}", self.movement.token);
                            target.split(
                                &group,
                                if ["left", "right"].contains(&edge) {
                                    "x"
                                } else {
                                    "y"
                                },
                                next.clone(),
                                format!("split-{}", self.movement.token),
                                ["left", "top"].contains(&edge),
                            )?;
                            group = next;
                        }
                        target.place(
                            id,
                            &group,
                            self.destination.index.unwrap_or(usize::MAX),
                            true,
                        )?;
                    }
                }
                Scope::Group { id } => {
                    let g = self
                        .source_before
                        .groups
                        .get(id)
                        .ok_or("标签组已关闭")?
                        .clone();
                    let mut portion = Host::new(format!("portion-{}", self.movement.token));
                    portion.groups.clear();
                    portion.groups.insert(id.clone(), g.clone());
                    portion.center = crate::workbench_layout::Node::Group {
                        group_id: id.clone(),
                    };
                    portion.focused_group_id = id.clone();
                    if let Some(group) = self.destination.group_id.as_deref() {
                        let edge = self.destination.edge.as_deref().unwrap_or("center");
                        if ["center", "tab"].contains(&edge) {
                            for view in &g.view_ids {
                                target.place(
                                    view,
                                    group,
                                    usize::MAX,
                                    g.active_view_id.as_ref() == Some(view),
                                )?;
                            }
                        } else {
                            target.insert_portion(
                                portion,
                                group,
                                if ["left", "right"].contains(&edge) {
                                    "x"
                                } else {
                                    "y"
                                },
                                ["left", "top"].contains(&edge),
                                format!("split-{}", self.movement.token),
                            )?;
                        }
                    } else {
                        target.merge_host(portion, format!("split-{}", self.movement.token))?;
                    }
                }
            }
        }
        target.validate()?;
        Ok(target)
    }
    fn rebase(&self, source: &Host, target: Option<&Host>) -> Result<(Option<Host>, Host), String> {
        self.check_portion(&self.movement.source, source, &self.source_before)?;
        match (target, &self.target_before) {
            (Some(a), Some(b)) => self.check_portion(&self.movement.target, a, b)?,
            (None, None) => {}
            _ => return Err("交接期间目标工作台已改变".into()),
        }
        let mut source_after = source.clone();
        if let Scope::Group { id } = &self.scope {
            if source_after.groups.len() > 1 {
                source_after.remove_group(id)?;
            } else {
                source_after = Host::new(source.id.clone());
                source_after.layout = source.layout.clone();
                source_after.layout_slot = source.layout_slot;
                source_after.bounds = source.bounds.clone();
                source_after.left = source.left.clone();
                source_after.right = source.right.clone();
            }
        } else {
            for view in &self.views {
                source_after.remove(view);
            }
        }
        let mut live = self.clone();
        live.source_before = source.clone();
        let target_after = live.target_projection(target)?;
        Ok((
            self.source_after.as_ref().map(|_| source_after),
            target_after,
        ))
    }
    pub(crate) fn wire(&self) -> serde_json::Value {
        serde_json::json!({"id":self.movement.token,"workspaceId":self.movement.workspace_id,
            "source":self.movement.source,"target":self.movement.target,"viewIds":self.views,"phase":self.phase,"scope":self.scope,"closeOwner":self.close_owner,"sourceRevision":self.source_revision,"targetRevision":self.target_revision})
    }
}
impl AppCore {
    pub(crate) fn existing_return_move(
        &self,
        token: &str,
        source: &str,
        id: &str,
        scope: &Scope,
    ) -> Option<Result<(), String>> {
        let ui = self.ui_state();
        let r = ui.move_receipts.get(token)?;
        if !r.returning
            || r.movement.source != source
            || r.movement.workspace_id != id
            || &r.scope != scope
        {
            return Some(Err("返回 requestId 已用于不同页面或参数".into()));
        }
        if r.expired() {
            return Some(Err("expired：返回回执已过期".into()));
        }
        Some(
            r.result
                .clone()
                .unwrap_or_else(|| Err("返回正在进行，请查询原 requestId 的结果".into())),
        )
    }
    pub fn resolve_workbench_return(
        &self,
        source: &str,
        workspace: &str,
        scope: &Scope,
    ) -> Result<(String, Destination), String> {
        let state = self.workbench_state();
        let w = state.ui.workspaces.get(workspace).ok_or("工作区已关闭")?;
        let host = w.host(source)?;
        let book = match scope {
            Scope::View { id } if host.contains(id) => w.return_locations.get(id),
            Scope::Group { id } if host.groups.contains_key(id) => {
                w.group_return_locations.get(id).or_else(|| {
                    host.groups[id]
                        .view_ids
                        .iter()
                        .find_map(|v| w.return_locations.get(v))
                })
            }
            _ => return Err("页面或标签组已移动或关闭".into()),
        }
        .ok_or("此页面没有跨窗口来源")?;
        let target = if state.workspaces.windows.contains_key(&book.window_id) {
            book.window_id.clone()
        } else {
            state
                .workspaces
                .owner(workspace)
                .filter(|id| id != source)
                .unwrap_or_else(|| "main".into())
        };
        let target_host = w.hosts.get(&target);
        let mut destination = Destination {
            return_to_source: true,
            ..Default::default()
        };
        if let Some(side) = &book.side {
            destination.side = Some(side.clone());
            if let Some(host) = target_host {
                let tools = if side == "left" {
                    &host.left.entries
                } else {
                    &host.right.entries
                };
                destination.before_tool_key = book
                    .next_tool_key
                    .as_ref()
                    .filter(|key| tools.iter().any(|e| &e.definition_key == *key))
                    .cloned()
                    .or_else(|| {
                        book.previous_tool_key
                            .as_ref()
                            .and_then(|key| tools.iter().position(|e| &e.definition_key == key))
                            .and_then(|i| tools.get(i + 1))
                            .map(|e| e.definition_key.clone())
                    });
            }
        } else if let Some(host) = target_host {
            let group = book
                .group_id
                .as_ref()
                .filter(|id| host.groups.contains_key(*id))
                .unwrap_or(&host.focused_group_id);
            destination.group_id = Some(group.clone());
            let ids = &host.groups[group].view_ids;
            destination.index = book
                .previous_view_id
                .as_ref()
                .and_then(|v| ids.iter().position(|id| id == v))
                .map(|i| i + 1)
                .or_else(|| {
                    book.next_view_id
                        .as_ref()
                        .and_then(|v| ids.iter().position(|id| id == v))
                });
        }
        Ok((target, destination))
    }
    pub(crate) fn existing_window_return(
        &self,
        source: &str,
        token: &str,
    ) -> Option<Result<(), String>> {
        let ui = self.ui_state();
        let batch = ui.window_returns.get(token)?;
        if batch.source != source {
            return Some(Err("返回 requestId 已用于其他窗口".into()));
        }
        Some(batch.movements.iter().try_for_each(|m| {
            let r = ui.move_receipts.get(&m.token).ok_or("返回回执不存在")?;
            if r.expired() {
                return Err("expired：返回回执已过期".into());
            }
            r.result
                .clone()
                .unwrap_or_else(|| Err("返回正在进行，请查询原 requestId 的结果".into()))
        }))
    }
    pub fn prepare_window_return(&self, source: &str, token: String) -> Result<Vec<Move>, String> {
        let mut state = self.workbench_state();
        if source == "main" || token.is_empty() || token.len() > 112 {
            return Err("返回窗口或请求 ID 无效".into());
        }
        if let Some(batch) = state.ui.window_returns.get(&token) {
            if batch.source != source {
                return Err("返回 requestId 已用于其他窗口".into());
            }
            return Ok(batch.movements.clone());
        }
        if state.ui.move_receipts.contains_key(&token) {
            return Err("返回 requestId 已使用，请查询原结果".into());
        }
        let ids = state.workspaces.projection(source)?.window.workspace_ids;
        let index = state
            .workspaces
            .projection("main")?
            .window
            .workspace_ids
            .len();
        let mut movements = Vec::new();
        for (at, id) in ids.iter().enumerate() {
            let child_token = if at == 0 {
                token.clone()
            } else {
                format!("{token}:{at}")
            };
            match self.prepare_workbench_move_locked(
                &mut state,
                source,
                id,
                "main",
                index + at,
                child_token,
                false,
                Scope::Host,
                Destination::default(),
            ) {
                Ok(m) => movements.push(m),
                Err(error) => {
                    let crate::workbench::State { workspaces, ui, .. } = &mut *state;
                    for m in &movements {
                        self.cancel_transfer_locked(workspaces, ui, m);
                    }
                    return Err(error);
                }
            }
        }
        state.ui.window_returns.insert(
            token,
            WindowReturn {
                source: source.into(),
                movements: movements.clone(),
            },
        );
        Ok(movements)
    }
    pub(crate) fn existing_workbench_move(
        &self,
        token: &str,
        source: &str,
        id: &str,
        target: &str,
        index: usize,
        scope: &Scope,
        destination: &Destination,
    ) -> Option<Result<(), String>> {
        let ui = self.ui_state();
        let r = ui.move_receipts.get(token)?;
        if r.fingerprint != move_fingerprint(source, id, target, index, scope, destination) {
            return Some(Err("移动 requestId 已用于不同目标或参数".into()));
        }
        if r.expired() {
            return Some(Err("expired：移动回执已过期".into()));
        }
        Some(
            r.result
                .clone()
                .unwrap_or_else(|| Err("移动正在进行，请查询原 requestId 的结果".into())),
        )
    }
    pub fn workbench_move_status(
        &self,
        window: &str,
        token: &str,
    ) -> Result<serde_json::Value, String> {
        let ui = self.ui_state();
        if let Some(r) = ui.tool_receipts.get(token) {
            if r.window != window {
                return Err("移动回执不属于此窗口".into());
            }
            return Ok(r.wire());
        }
        let r = ui.move_receipts.get(token).ok_or("移动请求不存在")?;
        if r.movement.source != window && r.movement.target != window {
            return Err("移动回执不属于此窗口".into());
        }
        let status = if r.expired() {
            "expired"
        } else {
            match &r.result {
                Some(Ok(())) => "completed",
                Some(Err(_)) => "cancelled",
                None => "pending",
            }
        };
        Ok(
            serde_json::json!({"id":token,"status":status,"movement":r.movement,"phase":ui.transfers.get(token).map(|t|t.phase),"error":r.result.as_ref().and_then(|r|r.as_ref().err())}),
        )
    }
    pub(crate) fn reserve_close_tools(
        &self,
        workspaces: &mut crate::workspaces::Runtime,
        ui: &mut crate::component_ui::Runtime,
        source: &str,
        operation: &str,
        tools: &[String],
    ) -> Result<Vec<String>, String> {
        let mut by_workspace: std::collections::BTreeMap<String, Vec<String>> = Default::default();
        for id in tools {
            let v = ui.resource_view(id)?;
            if ui.definitions[&v.definition_key].ui_api_version != "2" {
                return Err(format!(
                    "{} 需要跨窗口返回，请先升级此插件，或关闭服务器全部工作区",
                    v.title
                ));
            }
            by_workspace
                .entry(v.context.workspace_id.clone())
                .or_default()
                .push(id.clone());
        }
        let mut planned = vec![];
        let mut next_workspaces = workspaces.clone();
        for (workspace, views) in by_workspace {
            let target = workspaces
                .host_windows(&workspace)
                .into_iter()
                .filter(|w| w != source && !ui.lifecycle.host_closing(&workspace, w))
                .next()
                .ok_or("工具尚无可返回的工作台")?;
            let w = &ui.workspaces[&workspace];
            let source_host = w.host(source)?.clone();
            let mut source_after = source_host.clone();
            let target_host = w.host(&target)?.clone();
            let mut target_after = target_host.clone();
            for id in &views {
                source_after.remove(id);
                let side = &w.preferences[&ui.resource_view(id)?.definition_key].side;
                target_after.dock(id, &ui.resource_view(id)?.definition_key, side, false)?;
            }
            let token = self.next_id();
            let movement = Move {
                source: source.into(),
                workspace_id: workspace,
                target,
                index: usize::MAX,
                token: token.clone(),
            };
            let identities = views
                .iter()
                .map(|id| {
                    ui.resource_view(id)
                        .map(|v| (id.clone(), v.context.clone(), v.plugin_instance_id.clone()))
                })
                .collect::<Result<_, _>>()?;
            let t = Transfer {
                movement: movement.clone(),
                scope: Scope::Host,
                source_after: Some(source_after),
                target_after,
                views,
                source_revision: source_host.revision,
                target_revision: Some(target_host.revision),
                source_before: source_host,
                target_before: Some(target_host),
                destination: Default::default(),
                identities,
                focus_clock: ui.clock,
                phase: Phase::Preparing,
                watermarks: HashMap::new(),
                close_owner: Some(operation.into()),
            };
            if ui.transfers.values().any(|other| t.conflicts(other)) {
                return Err("工具返回位置正在交接，请稍后关闭".into());
            }
            next_workspaces.reserve_transfer(movement)?;
            planned.push(t);
        }
        *workspaces = next_workspaces;
        let tokens = planned.iter().map(|t| t.movement.token.clone()).collect();
        for t in planned {
            ui.transfers.insert(t.movement.token.clone(), t);
        }
        Ok(tokens)
    }
    pub(crate) fn finish_close_tool_returns(&self, operation: &str) -> Result<(), String> {
        let tokens = self.ui_state().lifecycle.operations[operation]
            .return_moves
            .clone();
        for token in tokens {
            let movement = {
                let ui = self.ui_state();
                let Some(t) = ui.transfers.get(&token) else {
                    continue;
                };
                t.movement.clone()
            };
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
            while !self.transfer_prepared(&movement) {
                if std::time::Instant::now() >= deadline {
                    return Err("工具返回窗口尚未就绪；任务仍在运行，请重试或取消".into());
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            let phase = self.ui_state().transfers[&token].phase;
            if phase != Phase::Restoring {
                self.begin_transfer_drain(&movement)?;
                self.transfer_drained(&movement.source, &token)?;
            }
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
            while !self.transfer_can_commit(&movement) {
                if std::time::Instant::now() >= deadline {
                    return Err("工具显示恢复尚未确认；任务保留，请继续完成关闭".into());
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            self.commit_workbench_move(&movement)?;
            self.workspace_changed();
        }
        Ok(())
    }
    pub fn prepare_workbench_move(
        &self,
        source: &str,
        id: &str,
        target: &str,
        index: usize,
        token: String,
        register: bool,
        scope: Scope,
        destination: Destination,
    ) -> Result<Move, String> {
        let mut state = self.workbench_state();
        self.prepare_workbench_move_locked(
            &mut state,
            source,
            id,
            target,
            index,
            token,
            register,
            scope,
            destination,
        )
    }
    fn prepare_workbench_move_locked(
        &self,
        state: &mut crate::workbench::State,
        source: &str,
        id: &str,
        target: &str,
        index: usize,
        token: String,
        register: bool,
        scope: Scope,
        mut destination: Destination,
    ) -> Result<Move, String> {
        let crate::workbench::State { workspaces, ui, .. } = &mut *state;
        if token.is_empty() || token.len() > 128 {
            return Err("移动请求 ID 无效".into());
        }
        let fingerprint = move_fingerprint(source, id, target, index, &scope, &destination);
        if let Some(r) = ui.move_receipts.get(&token) {
            if r.fingerprint != fingerprint {
                return Err("移动 requestId 已用于不同目标或参数".into());
            }
            if r.expired() {
                return Err("expired：移动回执已过期，请重新读取当前位置".into());
            }
            return Ok(r.movement.clone());
        }
        if ui.tool_receipts.contains_key(&token) {
            return Err("移动 requestId 已被占用".into());
        }
        if ui.move_receipts.len() + ui.tool_receipts.len() >= 4096 {
            return Err("移动请求记录已满，请重启后继续".into());
        }
        workspaces.validate_owned(
            source,
            &workspaces
                .projection(source)?
                .workspaces
                .iter()
                .find(|w| w.context.workspace_id == id)
                .ok_or("工作台已关闭")?
                .context,
        )?;
        if let Some(context) = &destination.source_context {
            if context.workspace_id != id {
                return Err("拖动上下文不属于此工作台".into());
            }
            workspaces.validate_owned(source, context)?;
        }
        if self.is_closing()
            || ui.frozen(id)
            || ui.lifecycle.host_closing(id, source)
            || ui.lifecycle.host_closing(id, target)
        {
            return Err("目标正在关闭，不能移动".into());
        }
        let w = ui.workspaces.get(id).ok_or("工作台尚未就绪")?;
        if source == target {
            return Err("同窗口移动应使用布局操作".into());
        }
        let source_host = w.host(source)?.clone();
        if destination
            .source_revision
            .is_some_and(|revision| revision != source_host.revision)
        {
            return Err("布局已改变，请重新拖动".into());
        }
        let mut source_after = source_host.clone();
        let current_target = w.hosts.get(target);
        let target_revision = current_target.map(|h| h.revision);
        let mut target_after = current_target.cloned().unwrap_or_else(|| {
            let mut h = Host::new(format!("{id}@{target}"));
            h.layout = source_host.layout.clone();
            h.layout_slot = if matches!(scope, Scope::Host) {
                source_host.layout_slot
            } else {
                w.hosts.values().map(|h| h.layout_slot).max().unwrap_or(0) + 1
            };
            h
        });
        if destination.side.is_none() && matches!(scope, Scope::View { .. }) {
            destination.group_id = Some(
                destination
                    .group_id
                    .clone()
                    .unwrap_or_else(|| target_after.focused_group_id.clone()),
            );
        }
        if destination
            .edge
            .as_deref()
            .is_some_and(|e| !["left", "right", "top", "bottom", "center", "tab"].contains(&e))
        {
            return Err("无效移动方向".into());
        }
        let views = match &scope {
            Scope::Host => {
                target_after.merge_host(source_host.clone(), format!("split-{token}"))?;
                source_after = Host::new(source_host.id.clone());
                source_after.layout_slot = source_host.layout_slot;
                source_after.bounds = source_host.bounds.clone();
                source_host.view_ids()
            }
            Scope::View { id: view } => {
                if !source_host.contains(view) {
                    return Err("页面已移动或关闭".into());
                }
                let v = ui.resource_view(view)?;
                if ui.definitions[&v.definition_key].definition.container == "dialog" {
                    return Err("弹窗不能单独移动".into());
                }
                if destination.side.is_some()
                    && ui.definitions[&v.definition_key].definition.container != "sidebar"
                {
                    return Err("只有工具视图可以停靠到侧栏".into());
                }
                source_after.remove(view);
                if let Some(side) = &destination.side {
                    target_after.dock(view, &v.definition_key, side, true)?;
                    if let Some(before) = destination.before_tool_key.as_deref() {
                        target_after.move_tool(&v.definition_key, side, Some(before))?;
                    }
                } else {
                    let mut group = destination
                        .group_id
                        .as_deref()
                        .unwrap_or(&target_after.focused_group_id)
                        .to_owned();
                    if let Some(edge) = destination
                        .edge
                        .as_deref()
                        .filter(|s| ["left", "right", "top", "bottom"].contains(s))
                    {
                        let next = format!("group-{token}");
                        target_after.split(
                            &group,
                            if ["left", "right"].contains(&edge) {
                                "x"
                            } else {
                                "y"
                            },
                            next.clone(),
                            format!("split-{token}"),
                            ["left", "top"].contains(&edge),
                        )?;
                        group = next;
                    }
                    target_after.place(
                        view,
                        &group,
                        destination.index.unwrap_or(usize::MAX),
                        true,
                    )?;
                }
                vec![view.clone()]
            }
            Scope::Group { id: group } => {
                let g = source_host.groups.get(group).ok_or("标签组已关闭")?.clone();
                if source_after.groups.len() > 1 {
                    source_after.remove_group(group)?;
                } else {
                    source_after = Host::new(format!("{}-empty-{token}", source_host.id));
                    source_after.layout_slot = source_host.layout_slot;
                    source_after.layout = source_host.layout.clone();
                    source_after.bounds = source_host.bounds.clone();
                    source_after.left = source_host.left.clone();
                    source_after.right = source_host.right.clone();
                }
                let mut portion = Host::new(format!("portion-{token}"));
                portion.groups.clear();
                portion.groups.insert(group.clone(), g.clone());
                portion.center = crate::workbench_layout::Node::Group {
                    group_id: group.clone(),
                };
                portion.focused_group_id = group.clone();
                if let Some(target) = destination.group_id.as_deref() {
                    let edge = destination.edge.as_deref().unwrap_or("center");
                    if ["center", "tab"].contains(&edge) {
                        for view in &g.view_ids {
                            target_after.place(
                                view,
                                target,
                                usize::MAX,
                                g.active_view_id.as_ref() == Some(view),
                            )?;
                        }
                    } else {
                        target_after.insert_portion(
                            portion,
                            target,
                            if ["left", "right"].contains(&edge) {
                                "x"
                            } else {
                                "y"
                            },
                            ["left", "top"].contains(&edge),
                            format!("split-{token}"),
                        )?;
                    }
                } else {
                    target_after.merge_host(portion, format!("split-{token}"))?;
                }
                g.view_ids
            }
        };
        if source_host.dialogs.iter().any(|id| {
            ui.resource_view(id)
                .is_ok_and(|v| v.parent_view_id.as_ref().is_none_or(|p| views.contains(p)))
        }) {
            return Err("此页面有编辑弹窗，请先完成或关闭弹窗再移动".into());
        }
        for view in &views {
            if ui.definitions[&ui.resource_view(view)?.definition_key].ui_api_version != "2" {
                return Err(format!(
                    "{} 使用 UI v1，请先升级此插件后跨窗口移动",
                    ui.resource_view(view)?.title
                ));
            }
        }
        source_after.validate()?;
        target_after.validate()?;
        let movement = Move {
            source: source.into(),
            workspace_id: id.into(),
            target: target.into(),
            index,
            token: token.clone(),
        };
        let identities = views
            .iter()
            .map(|id| {
                ui.resource_view(id)
                    .map(|v| (id.clone(), v.context.clone(), v.plugin_instance_id.clone()))
            })
            .collect::<Result<_, _>>()?;
        if views.iter().any(|v| ui.lifecycle.view_frozen(v)) {
            return Err("页面正在关闭".into());
        }
        let t = Transfer {
            movement: movement.clone(),
            scope: scope.clone(),
            source_after: if matches!(scope, Scope::Host)
                || (source != "main"
                    && source_after.view_ids().is_empty()
                    && source_after.left.entries.is_empty()
                    && source_after.right.entries.is_empty()
                    && source_after.groups.len() == 1)
            {
                None
            } else {
                Some(source_after)
            },
            target_after,
            views,
            source_revision: source_host.revision,
            target_revision,
            source_before: source_host,
            target_before: current_target.cloned(),
            destination,
            identities,
            focus_clock: ui.clock,
            phase: Phase::Preparing,
            watermarks: HashMap::new(),
            close_owner: None,
        };
        if ui.transfers.len() >= 32 || ui.transfers.values().any(|other| t.conflicts(other)) {
            return Err("页面或目标标签组正在交接".into());
        }
        if workspaces.pending_moves().iter().any(|m| {
            !ui.transfers.contains_key(&m.token) && (m.workspace_id == id || m.target == target)
        }) {
            return Err("工作台正在移动".into());
        }
        if register {
            workspaces.register(target)?;
        }
        if let Err(e) = workspaces.reserve_transfer(movement.clone()) {
            if register {
                let _ = workspaces.close_window(target);
            }
            return Err(e);
        }
        ui.move_receipts.insert(
            token.clone(),
            MoveReceipt {
                fingerprint,
                movement: movement.clone(),
                result: None,
                ended_at: None,
                scope,
                returning: t.destination.return_to_source,
            },
        );
        ui.transfers.insert(token, t);
        Ok(movement)
    }
    pub(crate) fn transfer_prepared(&self, m: &Move) -> bool {
        let state = self.workbench_state();
        state.ui.transfers.get(&m.token).is_some_and(|t| {
            t.views.iter().all(|v| {
                state.ui.surfaces.values().any(|s| {
                    s.view == *v
                        && s.window == m.target
                        && s.ready
                        && s.phase == SurfacePhase::Candidate
                })
            })
        })
    }
    pub fn begin_transfer_drain(&self, m: &Move) -> Result<(), String> {
        let mut state = self.workbench_state();
        let t = state.ui.transfers.get_mut(&m.token).ok_or("移动已取消")?;
        if t.phase == Phase::Preparing {
            t.phase = Phase::Draining;
        }
        drop(state);
        self.workspace_changed();
        Ok(())
    }
    pub fn transfer_drained(&self, window: &str, token: &str) -> Result<(), String> {
        let views = {
            let mut state = self.workbench_state();
            let t = state.ui.transfers.get(token).ok_or("移动已取消")?;
            if t.movement.source != window || t.phase != Phase::Draining {
                return Err("移动阶段或源窗口已改变".into());
            }
            let views = t.views.clone();
            for s in state
                .ui
                .surfaces
                .values_mut()
                .filter(|s| s.window == window && views.contains(&s.view))
            {
                s.phase = SurfacePhase::Frozen;
            }
            views
        };
        // A queue barrier includes old resize requests, which return before SSH applies them.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let active = {
                let state = self.workbench_state();
                views.iter().any(|id| {
                    state
                        .ui
                        .resource_view(id)
                        .is_ok_and(|v| v.calls.active() > 0)
                })
            };
            if !active {
                break;
            }
            if std::time::Instant::now() >= deadline {
                return Err("页面调用结果尚未确认，交接已停止".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        for view in &views {
            self.terminals.drain_view(view)?;
        }
        let mut state = self.workbench_state();
        let mut watermarks = HashMap::new();
        for view in &views {
            if let Some((_, stream)) = self.terminals.validation_surface(view) {
                watermarks.insert(view.clone(), stream.watermark());
            }
        }
        let t = state.ui.transfers.get_mut(token).ok_or("移动已取消")?;
        if t.phase != Phase::Draining {
            return Err("交接阶段已改变".into());
        }
        t.phase = Phase::Restoring;
        t.watermarks = watermarks;
        let target = t.movement.target.clone();
        state
            .ui
            .surfaces
            .retain(|_, s| s.window != target || !views.contains(&s.view));
        drop(state);
        self.workspace_changed();
        Ok(())
    }
    pub(crate) fn transfer_can_commit(&self, m: &Move) -> bool {
        let state = self.workbench_state();
        let Some(t) = state.ui.transfers.get(&m.token) else {
            return false;
        };
        t.phase == Phase::Restoring
            && t.views.iter().all(|v| {
                let Some((token, _)) = state.ui.surfaces.iter().find(|(_, s)| {
                    s.window == m.target
                        && s.view == *v
                        && s.ready
                        && s.phase == SurfacePhase::Candidate
                }) else {
                    return false;
                };
                t.watermarks.get(v).is_none_or(|mark| {
                    self.terminals
                        .validation_surface(v)
                        .is_some_and(|(_, stream)| stream.caught_up(token, *mark))
                })
            })
    }
    pub fn commit_workbench_move(&self, m: &Move) -> Result<(), String> {
        self.commit_workbench_moves(std::slice::from_ref(m))
    }
    /// Validate every member and rehearse workspace changes before releasing any surface.
    pub fn commit_workbench_moves(&self, movements: &[Move]) -> Result<(), String> {
        let mut state = self.workbench_state();
        let crate::workbench::State { workspaces, ui, .. } = &mut *state;
        let mut next_workspaces = workspaces.clone();
        let mut prepared = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for m in movements {
            if !seen.insert(&m.workspace_id) {
                return Err("一笔返回事务不能重复包含同一工作区".into());
            }
            if let Some(r) = ui.move_receipts.get(&m.token) {
                if r.expired() {
                    return Err("expired：移动回执已过期".into());
                }
                if let Some(result) = &r.result {
                    result.clone()?;
                    continue;
                }
            }
            let t = ui.transfers.get(&m.token).ok_or("移动已取消")?.clone();
            if t.phase != Phase::Restoring
                || !t.views.iter().all(|v| {
                    ui.surfaces
                        .iter()
                        .find(|(_, s)| {
                            s.window == m.target
                                && s.view == *v
                                && s.ready
                                && s.phase == SurfacePhase::Candidate
                        })
                        .is_some_and(|(token, _)| {
                            t.watermarks.get(v).is_none_or(|mark| {
                                self.terminals
                                    .validation_surface(v)
                                    .is_some_and(|(_, stream)| stream.caught_up(token, *mark))
                            })
                        })
                })
            {
                return Err("目标页面尚未恢复到交接位置".into());
            }
            let w = ui.workspaces.get(&m.workspace_id).ok_or("工作区已关闭")?;
            let reserved_close = t.close_owner.as_ref().is_some_and(|id| {
                ui.lifecycle.operations.get(id).is_some_and(|o| {
                    o.phase == crate::lifecycle::Phase::Cleaning
                        && o.return_moves.contains(&m.token)
                })
            });
            if !workspaces.has_move(m)
                || !workspaces.is_ready(&m.target)
                || (!reserved_close
                    && (ui.lifecycle.host_closing(&m.workspace_id, &m.source)
                        || t.views.iter().any(|v| ui.lifecycle.view_frozen(v))))
                || ui.lifecycle.host_closing(&m.workspace_id, &m.target)
            {
                return Err("交接期间关闭状态已改变".into());
            }
            for (id, context, instance) in &t.identities {
                workspaces.validate_identity(context)?;
                let v = ui.resource_view(id)?;
                if &v.context != context
                    || &v.plugin_instance_id != instance
                    || ui
                        .definitions
                        .get(&v.definition_key)
                        .is_none_or(|d| !d.available || &d.plugin_instance_id != instance)
                {
                    return Err("交接期间插件或连接代次已改变".into());
                }
            }
            let (source_after, mut target_after) =
                t.rebase(w.host(&m.source)?, w.hosts.get(&m.target))?;
            let changed_focus = ui.clock != t.focus_clock || reserved_close;
            if changed_focus {
                if let Some(before) = w.hosts.get(&m.target) {
                    target_after.preserve_focus(before);
                }
            }
            let previous_active = workspaces.windows[&m.target].active_workspace.clone();
            if t.source_after.is_none() {
                next_workspaces.commit(m)?;
            } else {
                next_workspaces.add_host(&m.target, &m.workspace_id, m.index)?;
                next_workspaces.cancel(m);
            }
            if changed_focus {
                next_workspaces
                    .windows
                    .get_mut(&m.target)
                    .unwrap()
                    .active_workspace = previous_active;
            }
            prepared.push((m.clone(), t, source_after, target_after));
        }
        *workspaces = next_workspaces;
        for (m, t, source_after, target_after) in prepared {
            let w = ui.workspaces.get_mut(&m.workspace_id).unwrap();
            if t.close_owner.is_none() {
                for id in &t.views {
                    if t.destination.return_to_source {
                        w.return_locations.remove(id);
                    } else {
                        w.return_locations.insert(
                            id.clone(),
                            ReturnLocation::capture(&m.source, &t.source_before, Some(id), None),
                        );
                    }
                }
                if let Scope::Group { id } = &t.scope {
                    if t.destination.return_to_source {
                        w.group_return_locations.remove(id);
                    } else {
                        w.group_return_locations.insert(
                            id.clone(),
                            ReturnLocation::capture(&m.source, &t.source_before, None, Some(id)),
                        );
                    }
                }
            }
            if let Some(host) = source_after {
                w.hosts.insert(m.source.clone(), host);
            } else {
                w.hosts.remove(&m.source);
            }
            w.hosts.insert(m.target.clone(), target_after);
            for (token, s) in &ui.surfaces {
                if s.window == m.source && t.views.contains(&s.view) {
                    self.shared_state
                        .remove_surface(&s.window, token, std::time::Instant::now());
                }
            }
            ui.surfaces
                .retain(|_, s| s.window != m.source || !t.views.contains(&s.view));
            for s in ui
                .surfaces
                .values_mut()
                .filter(|s| s.window == m.target && t.views.contains(&s.view))
            {
                s.phase = SurfacePhase::Active;
            }
            ui.transfers.remove(&m.token);
            if let Some(r) = ui.move_receipts.get_mut(&m.token) {
                r.result = Some(Ok(()));
                r.ended_at = Some(std::time::Instant::now());
            }
        }
        Ok(())
    }
    pub fn cancel_workbench_move(&self, m: &Move) {
        let mut state = self.workbench_state();
        let crate::workbench::State { workspaces, ui, .. } = &mut *state;
        self.cancel_transfer_locked(workspaces, ui, m);
        drop(state);
        self.workspace_changed();
    }
    pub(crate) fn cancel_transfer_locked(
        &self,
        workspaces: &mut crate::workspaces::Runtime,
        ui: &mut crate::component_ui::Runtime,
        m: &Move,
    ) {
        workspaces.cancel(m);
        if let Some(r) = ui.move_receipts.get_mut(&m.token) {
            if r.result.is_none() {
                r.result = Some(Err("移动已取消，原位置保留".into()));
                r.ended_at = Some(std::time::Instant::now());
            }
        }
        if let Some(t) = ui.transfers.remove(&m.token) {
            for (token, s) in &ui.surfaces {
                if s.window == m.target && t.views.contains(&s.view) {
                    self.shared_state
                        .remove_surface(&s.window, token, std::time::Instant::now());
                }
            }
            ui.surfaces
                .retain(|_, s| s.window != m.target || !t.views.contains(&s.view));
            for s in ui
                .surfaces
                .values_mut()
                .filter(|s| s.window == m.source && t.views.contains(&s.view))
            {
                s.phase = SurfacePhase::Active;
            }
        }
    }
}

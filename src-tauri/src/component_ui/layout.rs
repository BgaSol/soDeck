//! Registration, persisted defaults and per-workspace view selection.
use super::*;

impl Runtime {
    pub(crate) fn layout_admission(
        &self,
        workspace: &str,
        window: &str,
        group: Option<&str>,
        view: Option<&str>,
        side: Option<&str>,
    ) -> Result<(), String> {
        if self.lifecycle.host_closing(workspace,window)
            || view.is_some_and(|v|self.lifecycle.view_frozen(v))
            || self.lifecycle.operations.values().any(|o|!o.phase.terminal()&&o.adopted_by.is_none()&&o.window==window&&matches!(&o.target,crate::lifecycle::Target::Group{id,workspace_id} if workspace_id==workspace && group==Some(id))) {
            return Err("页面或标签组正在关闭".into());
        }
        if self
            .transfers
            .values()
            .any(|t| t.movement.workspace_id == workspace && t.blocks(window, group, view, side))
        {
            return Err("页面或目标标签组正在交接".into());
        }
        Ok(())
    }
    pub(crate) fn remove_window_surfaces(&mut self, window: &str) {
        self.surfaces.retain(|_, s| s.window != window);
    }
    pub(crate) fn close_surfaces(&self, ids: &[String]) -> HashMap<String, Vec<String>> {
        let mut windows: HashMap<String, Vec<String>> = HashMap::new();
        for (id, surface) in &self.surfaces {
            if ids.contains(&surface.view) {
                windows
                    .entry(surface.window.clone())
                    .or_default()
                    .push(id.clone());
            }
        }
        windows
    }
    pub(crate) fn remove_views(&mut self, ids: &[String]) {
        for request in self.open_requests.values_mut() {
            if ids.contains(&request.view_id) {
                request.closed = true;
            }
        }
        for w in self.workspaces.values_mut() {
            w.views.retain(|v| !ids.contains(&v.id));
            w.return_locations.retain(|id, _| !ids.contains(id));
            for host in w.hosts.values_mut() {
                for id in ids {
                    let slot = [&host.left, &host.right].into_iter().enumerate().find_map(
                        |(side, tools)| {
                            tools
                                .entries
                                .iter()
                                .position(|e| e.view_id.as_ref() == Some(id))
                                .map(|at| (side, at, tools.entries[at].definition_key.clone()))
                        },
                    );
                    host.remove(id);
                    if let Some((side, at, key)) = slot {
                        let tools = if side == 0 {
                            &mut host.left
                        } else {
                            &mut host.right
                        };
                        tools.entries.insert(
                            at.min(tools.entries.len()),
                            crate::workbench_layout::ToolSlot {
                                definition_key: key,
                                view_id: None,
                            },
                        );
                    }
                }
            }
            if w.focused_view_id
                .as_ref()
                .is_some_and(|id| ids.contains(id))
            {
                w.focused_view_id = None;
            }
        }
        self.surfaces.retain(|_, s| !ids.contains(&s.view));
    }
    pub(crate) fn rebind_context(&mut self, context: &Context) {
        self.retain_request_contexts(std::slice::from_ref(context), Some(&context.workspace_id));
        if let Some(w) = self.workspaces.get_mut(&context.workspace_id) {
            let ids: HashSet<_> = w.views.iter().map(|v| v.id.clone()).collect();
            for view in &mut w.views {
                view.context = context.clone();
                view.creation = Default::default();
            }
            self.surfaces.retain(|_, s| !ids.contains(&s.view));
        }
    }
    pub fn open(root: &Path) -> Result<Self, String> {
        let (preferences, error) = match fs::read(root.join("ui-layout.json")) {
            Ok(bytes) => match serde_json::from_slice::<Preferences>(&bytes) {
                Ok(p)
                    if p.schema_version == 1
                        && (160..=480).contains(&p.left_width)
                        && (160..=480).contains(&p.right_width)
                        && p.definitions.values().all(|v| {
                            ["left", "right"].contains(&v.side.as_str())
                                && ["none", "reveal", "createOne"].contains(&v.startup.as_str())
                        }) =>
                {
                    (p, None)
                }
                _ => (
                    Preferences::default(),
                    Some("组件布局无法读取，原文件已保留".into()),
                ),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Preferences::default(), None),
            Err(e) => (
                Preferences::default(),
                Some(format!("组件布局无法读取：{e}")),
            ),
        };
        Ok(Self {
            root: root.into(),
            memory: crate::workbench_memory::Memory::open(root),
            restoring_layout: false,
            preferences,
            error,
            definitions: BTreeMap::new(),
            actions: BTreeMap::new(),
            open_requests: HashMap::new(),
            workspaces: HashMap::new(),
            surfaces: HashMap::new(),
            lifecycle: crate::lifecycle::Lifecycle::default(),
            requests: HashMap::new(),
            clock: 0,
            startup_loading: false,
            transfers: HashMap::new(),
            move_receipts: HashMap::new(),
            tool_receipts: HashMap::new(),
            window_returns: HashMap::new(),
        })
    }
    pub fn frozen(&self, workspace: &str) -> bool {
        self.lifecycle.frozen(workspace)
    }
    pub fn widths(&self) -> (u16, u16) {
        (self.preferences.left_width, self.preferences.right_width)
    }
    pub fn save_widths(&mut self, left: u16, right: u16) -> Result<(), String> {
        if self.error.is_some() {
            return Err("组件布局原文件异常，未覆盖".into());
        }
        if !(160..=480).contains(&left) || !(160..=480).contains(&right) {
            return Err("面板宽度超出范围".into());
        }
        let mut preferences = self.preferences.clone();
        preferences.left_width = left;
        preferences.right_width = right;
        storage::write_json(&self.root.join("ui-layout.json"), &preferences)?;
        self.preferences = preferences;
        Ok(())
    }
    pub(super) fn view(&self, id: &str) -> Result<&View, String> {
        self.workspaces
            .values()
            .flat_map(|w| &w.views)
            .find(|v| v.id == id)
            .ok_or_else(|| "组件页面已关闭".into())
    }
    pub(crate) fn resource_view(&self, id: &str) -> Result<&View, String> {
        self.view(id)
    }
    pub(crate) fn update_metadata(&mut self, id: &str, metadata: &Value) -> Result<bool, String> {
        let v = self
            .workspaces
            .values_mut()
            .flat_map(|w| &mut w.views)
            .find(|v| v.id == id)
            .ok_or("组件页面已关闭")?;
        let before = (v.title.clone(), v.dirty, v.busy);
        if let Some(title) = metadata["title"].as_str() {
            if title.is_empty() || title.len() > 512 {
                return Err("组件标题无效".into());
            }
            v.title = title.into();
        }
        if let Some(dirty) = metadata["dirty"].as_bool() {
            v.dirty = dirty;
        }
        if let Some(busy) = metadata["busy"].as_bool() {
            v.busy = busy;
        }
        Ok(before != (v.title.clone(), v.dirty, v.busy))
    }
    pub(crate) fn open_view(
        &mut self,
        context: &Context,
        window: &str,
        group: Option<&str>,
        key: &str,
        resource_key: Option<String>,
        params: Value,
        id: String,
        locate: bool,
    ) -> Result<String, String> {
        if self.frozen(&context.workspace_id) {
            return Err("工作区正在确认关闭".into());
        }
        let d = self
            .definitions
            .get(key)
            .filter(|d| d.available)
            .ok_or("组件未登记或不可用")?
            .clone();
        if d.definition.instance_policy == "perResource"
            && resource_key
                .as_ref()
                .is_none_or(|s| s.is_empty() || s.len() > 2048)
        {
            return Err("缺少稳定资源标识".into());
        }
        let w = self
            .workspaces
            .get(&context.workspace_id)
            .ok_or("组件工作区尚未初始化")?;
        let actual_window = if d.definition.container == "sidebar" {
            w.views
                .iter()
                .find(|v| v.definition_key == key)
                .and_then(|v| w.view_window(&v.id))
                .or_else(|| {
                    w.hosts
                        .iter()
                        .find(|(_, h)| h.tool_side(key).is_some())
                        .map(|(id, _)| id.clone())
                })
                .unwrap_or_else(|| window.into())
        } else {
            window.into()
        };
        let host = self
            .workspaces
            .get(&context.workspace_id)
            .ok_or("组件工作区尚未初始化")?
            .host(&actual_window)?;
        let destination_group = group.unwrap_or(&host.focused_group_id);
        if d.definition.container == "sidebar"
            && host.tool_side(key).is_none()
            && host.left.entries.len() + host.right.entries.len() >= 128
        {
            return Err("此工作台的工具槽位已满".into());
        }
        self.layout_admission(
            &context.workspace_id,
            &actual_window,
            Some(destination_group),
            None,
            if d.definition.container == "sidebar" {
                host.tool_side(key).or(Some(&d.definition.default_side))
            } else {
                None
            },
        )?;
        let w = self.workspaces.get_mut(&context.workspace_id).unwrap();
        self.clock += 1;
        let existing = w
            .views
            .iter_mut()
            .filter(|v| {
                v.definition_key == key
                    && (locate
                        || d.definition.instance_policy == "perWorkspace"
                        || (d.definition.instance_policy == "perResource"
                            && v.resource_key == resource_key))
            })
            .max_by_key(|v| v.last_used);
        let selected = if let Some(v) = existing {
            v.last_used = self.clock;
            v.ui_requested = true;
            v.id.clone()
        } else {
            w.views.push(View {
                calls: std::sync::Arc::new(crate::resource_tasks::Tasks::default()),
                creation: Default::default(),
                parent_view_id: None,
                id: id.clone(),
                definition_key: key.into(),
                plugin_id: d.plugin_id,
                plugin_instance_id: d.plugin_instance_id,
                context: context.clone(),
                resource_key,
                params,
                title: d.definition.title,
                dirty: false,
                busy: false,
                error: None,
                last_used: self.clock,
                ui_requested: true,
            });
            id
        };
        // Locate an existing singleton/resource in its current window; opening is not a move.
        let actual_window = w.view_window(&selected).unwrap_or(actual_window);
        let side = w.preferences[key].side.clone();
        let host = w.host_mut(&actual_window)?;
        if host.contains(&selected) {
            host.select(&selected)?;
        } else if d.definition.container == "sidebar" {
            let side = host.tool_side(key).unwrap_or(&side).to_owned();
            host.dock(&selected, key, &side, true)?;
        } else if d.definition.container == "editor" {
            let group = group
                .filter(|id| host.groups.contains_key(*id))
                .unwrap_or(&host.focused_group_id)
                .to_owned();
            host.place(&selected, &group, usize::MAX, true)?;
        } else if d.definition.container == "dialog" {
            host.dialogs.push(selected.clone());
        }
        Ok(selected)
    }
    pub fn target_ready(&self, workspace: &str, window: &str) -> bool {
        self.workspaces.get(workspace).is_none_or(|w| {
            w.views.iter().filter(|v| v.ui_requested).all(|v| {
                self.surfaces
                    .values()
                    .any(|s| s.view == v.id && s.window == window && s.ready)
            })
        })
    }
    pub fn mounted_surface(&self, view: &str, window: &str) -> Option<String> {
        self.surfaces
            .iter()
            .find(|(_, s)| s.view == view && s.window == window && s.ready)
            .map(|(id, _)| id.clone())
    }
    pub fn require_closed(&self, ids: &[String]) -> Result<(), String> {
        if ids
            .iter()
            .any(|id| self.workspaces.get(id).is_some_and(|w| !w.views.is_empty()))
        {
            return Err("请先完成组件页面的关闭确认".into());
        }
        Ok(())
    }
}
impl AppCore {
    pub(crate) fn reconcile_ui(&self) -> Result<(), String> {
        let plugins = self.snapshot().plugins;
        let mut _workbench = self.workbench_state();
        let crate::workbench::State {
            workspaces,
            ui,
            commands,
        } = &mut *_workbench;
        let contexts = workspaces.contexts();
        commands.retain_contexts(&contexts);
        for p in plugins.iter().filter(|p| p.status != PluginStatus::Running) {
            if let Some(instance) = &p.instance_id {
                commands.stop_instance(&p.manifest.id, instance);
            }
        }
        let mut restored = Vec::new();
        let mut startup_commands = Vec::new();
        let live = workspaces.ids();

        let initializing =
            ui.startup_loading || plugins.iter().any(|p| p.status == PluginStatus::Starting);
        ui.retain_request_contexts(&contexts, None);
        for context in &contexts {
            if workspaces.validate_context(context).is_err() {
                commands.invalidate_context(context, "连接已断开");
            }
        }
        ui.workspaces.retain(|id, _| live.contains(id));
        let live_views: HashSet<_> = ui
            .workspaces
            .values()
            .flat_map(|w| w.views.iter().map(|v| v.id.clone()))
            .collect();
        ui.surfaces.retain(|_, s| live_views.contains(&s.view));
        ui.surfaces.retain(|_, s| {
            ui.workspaces
                .values()
                .any(|w| w.view_window(&s.view).as_deref() == Some(&s.window))
                || ui
                    .transfers
                    .values()
                    .any(|t| t.movement.target == s.window && t.views.contains(&s.view))
                || workspaces.pending_for_target(&s.window).is_some_and(|m| {
                    ui.workspaces.get(&m.workspace_id).is_some_and(|w| {
                        w.hosts.get(&m.source).is_some_and(|h| h.contains(&s.view))
                    })
                })
        });
        for d in ui.definitions.values_mut() {
            d.available = false;
        }
        for a in ui.actions.values_mut() {
            a.available = false;
            a.reason = Some("行为提供者未运行".into());
        }
        let mut preferences = ui.preferences.clone();
        for p in plugins.iter().filter(|p| p.status == PluginStatus::Running) {
            if let Some(declaration) = &p.manifest.ui {
                let available = self
                    .instances
                    .lock()
                    .unwrap()
                    .get(&p.manifest.id)
                    .is_some_and(|i| i.ui_actions_api.load(std::sync::atomic::Ordering::Acquire));
                for a in &declaration.actions {
                    let command_error = a
                        .command
                        .as_ref()
                        .and_then(|name| self.command_provider(name).err());
                    let available = available && command_error.is_none();
                    let key = format!("{}:{}", p.manifest.id, a.id);
                    ui.actions.insert(
                        key.clone(),
                        RegisteredAction {
                            definition: a.clone(),
                            key,
                            plugin_id: p.manifest.id.clone(),
                            plugin_name: p.manifest.name.clone(),
                            plugin_instance_id: p.instance_id.clone().unwrap(),
                            available,
                            reason: command_error
                                .map(|e| e.to_string())
                                .or_else(|| (!available).then(|| "行为提供者需要升级".into())),
                        },
                    );
                }
                for d in &declaration.views {
                    let key = format!("{}:{}", p.manifest.id, d.id);
                    preferences
                        .definitions
                        .entry(key.clone())
                        .or_insert(Preference {
                            side: d.default_side.clone(),
                            pinned: d.default_pinned,
                            startup: d.open_on_workspace.clone(),
                            order: d.startup_order,
                        });
                    ui.definitions.insert(
                        key.clone(),
                        Registered {
                            definition: d.clone(),
                            key,
                            plugin_id: p.manifest.id.clone(),
                            plugin_name: p.manifest.name.clone(),
                            plugin_instance_id: p.instance_id.clone().unwrap(),
                            entry: declaration.entry.clone(),
                            available: true,
                            ui_api_version: declaration.api_version.clone(),
                        },
                    );
                }
            }
        }
        if !initializing && preferences.default_create_action.is_none() {
            preferences.default_create_action = ui
                .actions
                .values()
                .find(|a| a.available && a.definition.default_create)
                .map(|a| a.key.clone());
        }
        if ui.error.is_none()
            && serde_json::to_value(&preferences).unwrap()
                != serde_json::to_value(&ui.preferences).unwrap()
            && !initializing
        {
            storage::write_json(&ui.root.join("ui-layout.json"), &preferences)?;
            ui.preferences = preferences;
        }
        for context in &contexts {
            let id = &context.workspace_id;
            if ui.frozen(id) || self.is_closing() {
                continue;
            }
            if initializing {
                continue;
            }
            if !ui.workspaces.contains_key(id) {
                let prefs = ui.preferences.definitions.clone();
                let window = workspaces.owner(id).ok_or("工作区已关闭")?;
                let mut host = crate::workbench_layout::Host::new(format!("{id}@{window}"));
                host.layout = workspaces
                    .projection(&window)?
                    .workspaces
                    .iter()
                    .find(|w| w.context.workspace_id == *id)
                    .ok_or("工作区已关闭")?
                    .layout
                    .clone();
                if let Some(saved) = ui.memory.primary(&context.server_id) {
                    host = saved.seed(host.id.clone())?;
                    saved.seed_order(&mut host, &prefs);
                    restored.push((id.clone(), window.clone(), saved));
                }
                ui.workspaces.insert(
                    id.clone(),
                    WorkspaceUi {
                        preferences: prefs,
                        hosts: BTreeMap::from([(window.clone(), host)]),
                        ..Default::default()
                    },
                );
                ui.seed_tool_slots(id, &window, &context.server_id);
                let mut defs: Vec<_> = ui
                    .definitions
                    .values()
                    .filter(|d| d.available)
                    .cloned()
                    .collect();
                defs.sort_by_key(|d| (d.definition.startup_order, d.key.clone()));
                for d in defs {
                    if let Some(preference) = ui.preferences.definitions.get(&d.key).cloned() {
                        ui.workspaces
                            .get_mut(id)
                            .unwrap()
                            .preferences
                            .entry(d.key.clone())
                            .or_insert(preference);
                    }
                    let policy = ui.workspaces[id]
                        .preferences
                        .get(&d.key)
                        .map(|p| p.startup.clone())
                        .unwrap_or_else(none);
                    if restored.iter().any(|(workspace, _, saved)| {
                        workspace == id && saved.tools.get(&d.key).is_some_and(|t| !t.visible)
                    }) {
                        continue;
                    }
                    if d.definition.container == "sidebar"
                        && ui
                            .memory
                            .tool_slot(&context.server_id, &d.key)
                            .is_some_and(|slot| {
                                ui.workspaces[id]
                                    .hosts
                                    .values()
                                    .all(|h| h.layout_slot != slot)
                            })
                    {
                        continue;
                    }
                    let state = &ui.workspaces[id];
                    let side = state
                        .host(&window)?
                        .tool_side(&d.key)
                        .or_else(|| state.preferences.get(&d.key).map(|p| p.side.as_str()))
                        .unwrap_or("left");
                    let occupied = if d.definition.container == "editor" {
                        state.host(&window)?.active_view().is_some()
                    } else if side == "left" {
                        state.host(&window)?.left.active_view_id.is_some()
                    } else {
                        state.host(&window)?.right.active_view_id.is_some()
                    };
                    if policy != "none" && !occupied {
                        if let Some(action_id) = &d.definition.create_action_id {
                            if let Some(action) =
                                ui.actions.get(&format!("{}:{action_id}", d.plugin_id))
                            {
                                if let Some(name) = &action.definition.command {
                                    startup_commands.push((window.clone(),json!({"op":"submit","expectedContext":context,"name":name,"requestId":format!("startup:{}",d.definition.id),"startupDefinition":d.key,"payload":null})));
                                    continue;
                                }
                            }
                            continue;
                        }
                        ui.open_view(
                            context,
                            &window,
                            None,
                            &d.key,
                            None,
                            Value::Null,
                            self.next_id(),
                            false,
                        )?;
                    }
                }
            }
        }
        let preferences = ui.preferences.definitions.clone();
        for w in ui.workspaces.values_mut() {
            for (key, p) in &preferences {
                w.preferences
                    .entry(key.clone())
                    .or_insert_with(|| p.clone());
            }
        }
        for context in &contexts {
            if let Some(window) = workspaces.owner(&context.workspace_id) {
                ui.seed_tool_slots(&context.workspace_id, &window, &context.server_id);
            }
        }
        for context in &contexts {
            if ui.frozen(&context.workspace_id)
                || !ui.workspaces.contains_key(&context.workspace_id)
            {
                continue;
            }
            let defs: Vec<_> = ui
                .definitions
                .values()
                .filter(|d| d.available && d.definition.background_on_workspace)
                .cloned()
                .collect();
            for d in defs {
                let w = ui.workspaces.get_mut(&context.workspace_id).unwrap();
                if let Some(v) = w.views.iter_mut().find(|v| v.definition_key == d.key) {
                    if v.plugin_instance_id != d.plugin_instance_id {
                        v.plugin_instance_id = d.plugin_instance_id.clone();
                        v.creation = Default::default();
                        v.error = None;
                    }
                } else {
                    w.views.push(View {
                        calls: std::sync::Arc::new(crate::resource_tasks::Tasks::default()),
                        creation: Default::default(),
                        parent_view_id: None,
                        id: self.next_id(),
                        definition_key: d.key,
                        plugin_id: d.plugin_id,
                        plugin_instance_id: d.plugin_instance_id,
                        context: context.clone(),
                        resource_key: None,
                        params: Value::Null,
                        title: d.definition.title,
                        dirty: false,
                        busy: false,
                        error: None,
                        last_used: 0,
                        ui_requested: false,
                    });
                }
            }
        }
        for (id, window, saved) in restored {
            for (key, tool) in &saved.tools {
                if !tool.visible {
                    continue;
                }
                if ui.definitions.get(key).is_some_and(|d| {
                    d.available
                        && d.definition.container == "sidebar"
                        && d.definition.instance_policy == "perWorkspace"
                }) {
                    let context = contexts.iter().find(|c| c.workspace_id == id).unwrap();
                    ui.open_view(
                        context,
                        &window,
                        None,
                        key,
                        None,
                        Value::Null,
                        self.next_id(),
                        false,
                    )?;
                }
            }
            let w = ui.workspaces.get_mut(&id).unwrap();
            let tools = w
                .views
                .iter()
                .filter(|v| w.host(&window).is_ok_and(|h| h.contains(&v.id)))
                .map(|v| (v.id.clone(), v.definition_key.clone()))
                .collect::<Vec<_>>();
            saved.apply_tools(w.host_mut(&window)?, &tools)?;
        }
        self.sync_shared_ui_locked(&ui);
        drop(_workbench);
        for (window, params) in startup_commands {
            if let Err(error) = self.command_host_request(&window, &params) {
                self.log(
                    None,
                    "warn",
                    "默认页面命令未接纳",
                    Some(json!({"code":error.code,"message":error.message})),
                );
            }
        }
        Ok(())
    }
    pub fn ui_snapshot(&self, window: &str) -> Result<Value, String> {
        let mut state = self.workbench_state();
        state.commands.refresh();
        let ui = &state.ui;
        let projection = state.workspaces.projection(window)?;
        let ids: HashSet<_> = projection
            .workspaces
            .iter()
            .chain(projection.incoming_workspaces.iter())
            .map(|w| w.context.workspace_id.clone())
            .chain(
                ui.transfers
                    .values()
                    .filter(|t| t.movement.target == window)
                    .map(|t| t.movement.workspace_id.clone()),
            )
            .collect();
        let mut displayed = HashMap::new();
        for (id, w) in ui.workspaces.iter().filter(|(id, _)| ids.contains(*id)) {
            let mut display = w.clone();
            let mut transfers: Vec<_> = ui
                .transfers
                .values()
                .filter(|t| t.movement.target == window && t.movement.workspace_id == *id)
                .collect();
            transfers.sort_by_key(|t| &t.movement.token);
            for t in &transfers {
                let target = t.target_projection(display.hosts.get(window))?;
                display.hosts.insert(window.into(), target);
            }
            if transfers.is_empty() {
                if let Some(m) = state.workspaces.pending_for_workspace(window, id) {
                    if let Some(host) = w.hosts.get(&m.source) {
                        display.hosts.insert(window.into(), host.clone());
                    }
                }
            }
            let mut projected = display.projection(window);
            projected["tools"] = ui.tool_projection(w);
            if let Some(host) = display.hosts.get(window) {
                let server = state
                    .workspaces
                    .contexts()
                    .into_iter()
                    .find(|c| c.workspace_id == *id)
                    .map(|c| c.server_id);
                if let Some(saved) = server.and_then(|s| {
                    ui.memory
                        .hosts(&s)
                        .into_iter()
                        .find(|h| h.slot == host.layout_slot)
                }) {
                    projected["unavailableTools"]=json!(saved.tools.iter().filter(|(key,_)|!ui.definitions.get(*key).is_some_and(|d|d.available)).map(|(key,t)|json!({"key":key,"groupId":t.group.as_ref().map(|g|format!("{}-{g}",host.id)),"side":t.side})).collect::<Vec<_>>());
                }
            }
            displayed.insert(id, projected);
        }
        Ok(
            json!({"definitions":ui.definitions,"actions":ui.actions,"defaultCreateAction":ui.preferences.default_create_action,
          "pendingAction":state.commands.pending(window,ui).into_iter().collect::<Vec<_>>(),
          "transfers":ui.transfers.values().filter(|t|t.movement.source==window||t.movement.target==window).map(|t|t.wire()).collect::<Vec<_>>(),
          "workspaces":displayed,"error":ui.error.as_ref().or(ui.memory.error.as_ref()),"layoutSaveError":ui.memory.error}),
        )
    }
    pub fn ui_action(
        &self,
        window: &str,
        context: &Context,
        action: Value,
    ) -> Result<Value, String> {
        if serde_json::to_vec(&action)
            .map_err(|e| e.to_string())?
            .len()
            > 24 * 1024
        {
            return Err("UI 操作超过 24 KiB".into());
        }
        if action["op"] == "open" {
            let binding = self
                .ui_state()
                .definitions
                .get(action["key"].as_str().unwrap_or_default())
                .and_then(|d| {
                    d.definition
                        .create_action_id
                        .as_ref()
                        .map(|id| format!("{}:{id}", d.plugin_id))
                });
            if let Some(key) = binding {
                return self.ui_invoke_action(window,&json!({"context":context,"requestId":action["requestId"],"actionKey":key,"groupId":action["groupId"],"data":action["params"]}));
            }
        }
        if action["op"] == "moveTool" {
            return self.ui_move_tool(window, context, action);
        }
        let mut _workbench = self.workbench_state();
        let crate::workbench::State { workspaces, ui, .. } = &mut *_workbench;
        workspaces.validate_owned(window, context)?;

        if ui.lifecycle.host_closing(&context.workspace_id, window) {
            return Err("工作区正在确认关闭".into());
        }
        let key = action["key"].as_str().unwrap_or_default();
        let op = action["op"].as_str().ok_or("缺少组件操作")?;
        if ![
            "select",
            "focus",
            "focusGroup",
            "zoom",
            "splitRatio",
            "preference",
            "recommend",
            "collapse",
            "locate",
        ]
        .contains(&op)
        {
            let host = ui
                .workspaces
                .get(&context.workspace_id)
                .ok_or("工作台已关闭")?
                .host(window)?;
            let view = action["viewId"].as_str();
            let group = action["groupId"]
                .as_str()
                .or(Some(host.focused_group_id.as_str()));
            ui.layout_admission(
                &context.workspace_id,
                window,
                group,
                view,
                action["side"].as_str(),
            )?;
            if let Some(source) = action["sourceGroupId"].as_str() {
                ui.layout_admission(&context.workspace_id, window, Some(source), None, None)?;
            }
            if let Some(view) = view {
                let source = host.group_of(view);
                ui.layout_admission(
                    &context.workspace_id,
                    window,
                    source.as_deref(),
                    Some(view),
                    None,
                )?;
            }
        }
        let mut result = Value::Null;
        match op {
            "split" | "moveView" | "dock" | "hideTool" | "splitRatio" | "zoom" | "mergeGroup"
            | "moveGroup" | "closeEmptyGroup" | "focusGroup" => {
                let w = ui
                    .workspaces
                    .get_mut(&context.workspace_id)
                    .ok_or("工作台已关闭")?;
                let mut host = w.host(window)?.clone();
                if action["revision"]
                    .as_u64()
                    .is_some_and(|r| r != host.revision)
                {
                    return Err("布局已改变，请重新读取".into());
                }
                let group = action["groupId"]
                    .as_str()
                    .unwrap_or(&host.focused_group_id)
                    .to_owned();
                match op {
                    "split" => {
                        let id = self.next_id();
                        host.split(
                            &group,
                            action["axis"].as_str().ok_or("缺少分屏方向")?,
                            id.clone(),
                            self.next_id(),
                            action["before"] == true,
                        )?;
                        if let Some(view) = action["viewId"].as_str() {
                            if !host.contains(view) {
                                return Err("页面不属于此工作台".into());
                            }
                            // A requested split keeps its source usable even when its only page moves.
                            let source = host.groups.get_mut(&group).ok_or("源标签组已关闭")?;
                            if source.view_ids.as_slice() == [view] {
                                source.explicit_empty = true;
                            }
                            host.place(view, &id, 0, true)?;
                        }
                        result = json!({"groupId":id});
                    }
                    "moveView" => {
                        let view = action["viewId"].as_str().ok_or("缺少页面")?;
                        if !host.contains(view) {
                            return Err("页面不属于此工作台".into());
                        }
                        host.place(
                            view,
                            &group,
                            action["index"].as_u64().unwrap_or(u64::MAX) as usize,
                            true,
                        )?;
                    }
                    "dock" | "hideTool" => {
                        let view = action["viewId"].as_str().ok_or("缺少页面")?;
                        if !host.contains(view)
                            || w.views
                                .iter()
                                .find(|v| v.id == view)
                                .and_then(|v| ui.definitions.get(&v.definition_key))
                                .is_none_or(|d| d.definition.container != "sidebar")
                        {
                            return Err("只有此工作台的工具可以停靠".into());
                        }
                        let side = action["side"].as_str().unwrap_or_else(|| {
                            if host.left.view_ids().iter().any(|id| id == view) {
                                "left"
                            } else if host.right.view_ids().iter().any(|id| id == view) {
                                "right"
                            } else {
                                &w.preferences[&w
                                    .views
                                    .iter()
                                    .find(|v| v.id == view)
                                    .unwrap()
                                    .definition_key]
                                    .side
                            }
                        });
                        let key = &w
                            .views
                            .iter()
                            .find(|v| v.id == view)
                            .unwrap()
                            .definition_key;
                        if host.tool_side(key).is_some() {
                            host.move_tool(key, side, action["beforeKey"].as_str())?;
                        } else {
                            host.dock(view, key, side, op != "hideTool")?;
                        }
                        if op == "hideTool" {
                            if side == "left" && host.left.active_view_id.as_deref() == Some(view) {
                                host.left.active_view_id = None;
                            } else if side == "right"
                                && host.right.active_view_id.as_deref() == Some(view)
                            {
                                host.right.active_view_id = None;
                            }
                        }
                    }
                    "splitRatio" => host.resize(
                        action["splitId"].as_str().ok_or("缺少分屏节点")?,
                        action["ratio"].as_f64().ok_or("缺少比例")?,
                    )?,
                    "zoom" => {
                        if !host.groups.contains_key(&group) {
                            return Err("标签组已关闭".into());
                        }
                        host.zoom_group_id = if host.zoom_group_id.as_deref() == Some(&group) {
                            None
                        } else {
                            Some(group)
                        };
                        host.revision += 1;
                    }
                    "moveGroup" => host.relocate_group(
                        action["sourceGroupId"].as_str().ok_or("缺少源标签组")?,
                        &group,
                        action["edge"].as_str().unwrap_or("center"),
                        self.next_id(),
                    )?,
                    "mergeGroup" => host.merge_group(
                        action["sourceGroupId"].as_str().ok_or("缺少源标签组")?,
                        &group,
                    )?,
                    "closeEmptyGroup" => {
                        if host
                            .groups
                            .get(&group)
                            .is_none_or(|g| !g.view_ids.is_empty())
                        {
                            return Err("标签组仍有页面，请先关闭页面".into());
                        }
                        host.remove_group(&group)?;
                    }
                    "focusGroup" => {
                        if !host.groups.contains_key(&group) {
                            return Err("标签组已关闭".into());
                        }
                        host.focused_group_id = group;
                        host.revision += 1;
                    }
                    _ => unreachable!(),
                }
                host.validate()?;
                w.hosts.insert(window.into(), host);
            }
            "open" | "toggle" | "locate" => {
                let request = action["requestId"].as_str().ok_or("缺少 requestId")?;
                if request.len() > 128 {
                    return Err("requestId 过长".into());
                }
                let request_key = format!("{}:{request}", context.workspace_id);
                if let Some(id) = ui.requests.get(&request_key) {
                    return Ok(
                        json!({"viewId":id,"windowId":ui.workspaces[&context.workspace_id].view_window(id)}),
                    );
                }
                let d = ui.definitions.get(key).ok_or("组件未登记")?.clone();
                let state = ui
                    .workspaces
                    .get(&context.workspace_id)
                    .ok_or("组件尚未就绪")?;
                let host = state.host(window)?;
                let current = [&host.left.active_view_id, &host.right.active_view_id]
                    .into_iter()
                    .filter_map(|id| id.as_ref())
                    .find(|id| {
                        state
                            .views
                            .iter()
                            .any(|v| &v.id == *id && v.definition_key == key)
                    })
                    .cloned();
                let toggled = op == "toggle"
                    && d.definition.container == "sidebar"
                    && current.as_ref().is_some_and(|id| {
                        state
                            .views
                            .iter()
                            .any(|v| &v.id == id && v.definition_key == key)
                    });
                if toggled {
                    let w = ui.workspaces.get_mut(&context.workspace_id).unwrap();
                    if w.host(window)?.left.active_view_id == current {
                        w.host_mut(window)?.left.active_view_id = None;
                    } else {
                        w.host_mut(window)?.right.active_view_id = None;
                    }
                    w.host_mut(window)?.revision += 1;
                } else {
                    let id = ui.open_view(
                        context,
                        window,
                        action["groupId"].as_str(),
                        key,
                        action["resourceKey"].as_str().map(str::to_owned),
                        action["params"].clone(),
                        self.next_id(),
                        op == "locate",
                    )?;
                    if d.definition.container == "dialog" {
                        let parent = action["sourceViewId"].as_str().map(str::to_owned);
                        if let Some(v) = ui
                            .workspaces
                            .get_mut(&context.workspace_id)
                            .unwrap()
                            .views
                            .iter_mut()
                            .find(|v| v.id == id)
                        {
                            v.parent_view_id = parent;
                        }
                    }
                    if ui.requests.len() >= 256 {
                        ui.requests.clear();
                    }
                    ui.requests.insert(request_key, id.clone());
                    let location = ui.workspaces[&context.workspace_id].view_window(&id);
                    result = json!({"viewId":id,"windowId":location});
                }
            }
            "select" | "focus" => {
                let id = action["viewId"].as_str().ok_or("缺少 viewId")?;
                ui.clock += 1;
                let clock = ui.clock;
                let w = ui
                    .workspaces
                    .get_mut(&context.workspace_id)
                    .ok_or("组件工作区已关闭")?;
                let v = w
                    .views
                    .iter_mut()
                    .find(|v| v.id == id)
                    .ok_or("组件页面已关闭")?;
                v.last_used = clock;
                if ui
                    .definitions
                    .get(&v.definition_key)
                    .is_some_and(|d| d.definition.location_provider.is_some())
                {
                    w.last_location_view_id = Some(id.into());
                }
                w.host_mut(window)?.select(id)?;
                w.focused_view_id = Some(id.into());
                w.focused_window_id = Some(window.into());
            }
            "collapse" => {
                let w = ui
                    .workspaces
                    .get_mut(&context.workspace_id)
                    .ok_or("组件工作区已关闭")?;
                match action["side"].as_str() {
                    Some("left") => w.host_mut(window)?.left.active_view_id = None,
                    Some("right") => w.host_mut(window)?.right.active_view_id = None,
                    _ => return Err("无效侧栏".into()),
                }
                w.host_mut(window)?.revision += 1;
            }
            "preference" | "recommend" => {
                if ui.error.is_some() {
                    return Err("布局原文件异常，请先修复 ui-layout.json".into());
                }
                let d = ui.definitions.get(key).ok_or("组件未登记")?.clone();
                let mut prefs = ui.preferences.clone();
                let p = prefs.definitions.get_mut(key).ok_or("布局尚未就绪")?;
                if op == "recommend" {
                    p.side = d.definition.default_side;
                    p.pinned = d.definition.default_pinned;
                    p.startup = d.definition.open_on_workspace;
                    p.order = d.definition.startup_order;
                }
                if let Some(side) = action["side"].as_str() {
                    if !["left", "right"].contains(&side) {
                        return Err("无效侧栏".into());
                    }
                    p.side = side.into();
                }
                if let Some(pinned) = action["pinned"].as_bool() {
                    p.pinned = pinned;
                }
                if let Some(startup) = action["startup"].as_str() {
                    if !["none", "reveal", "createOne"].contains(&startup)
                        || (d.definition.instance_policy == "perResource" && startup != "none")
                    {
                        return Err("无效默认打开方式".into());
                    }
                    p.startup = startup.into();
                }
                if let Some(order) = action["order"].as_u64() {
                    p.order = u32::try_from(order).map_err(|_| "顺序过大")?;
                }
                let next = p.clone();
                storage::write_json(&ui.root.join("ui-layout.json"), &prefs)?;
                ui.preferences = prefs;
                for w in ui.workspaces.values_mut() {
                    w.preferences.insert(key.into(), next.clone());
                }
            }
            "reorder" => {
                let id = action["viewId"].as_str().ok_or("缺少 viewId")?;
                let index = action["index"].as_u64().ok_or("缺少顺序")? as usize;
                let w = ui
                    .workspaces
                    .get_mut(&context.workspace_id)
                    .ok_or("组件工作区已关闭")?;
                let host = w.host_mut(window)?;
                let group = host.group_of(id).ok_or("页面不在中央标签组中")?;
                host.place(id, &group, index, false)?;
            }
            _ => return Err("未知组件 UI 操作".into()),
        }
        self.sync_shared_ui_locked(&ui);
        drop(_workbench);
        self.workspace_changed();
        Ok(result)
    }
}

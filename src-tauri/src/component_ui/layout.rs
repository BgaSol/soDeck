//! Registration, persisted defaults and per-workspace view selection.
use super::*;

impl Runtime {
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
        let editors: HashSet<_> = self
            .definitions
            .iter()
            .filter(|(_, d)| d.definition.container == "editor")
            .map(|(key, _)| key.clone())
            .collect();
        for w in self.workspaces.values_mut() {
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
        self.surfaces.retain(|_, s| !ids.contains(&s.view));
    }
    pub(crate) fn rebind_context(&mut self, context: &Context) {
        if let Some(w) = self.workspaces.get_mut(&context.workspace_id) {
            let ids: HashSet<_> = w.views.iter().map(|v| v.id.clone()).collect();
            for view in &mut w.views {
                view.context = context.clone();
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
            preferences,
            error,
            definitions: BTreeMap::new(),
            workspaces: HashMap::new(),
            surfaces: HashMap::new(),
            lifecycle: crate::lifecycle::Lifecycle::default(),
            requests: HashMap::new(),
            clock: 0,
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
            if title.is_empty() || title.len() > 256 {
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
    pub(super) fn open_view(
        &mut self,
        context: &Context,
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
            .get_mut(&context.workspace_id)
            .ok_or("组件工作区尚未初始化")?;
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
        if d.definition.container == "sidebar" {
            let side = &w.preferences[key].side;
            if side == "left" {
                w.left = Some(selected.clone());
            } else {
                w.right = Some(selected.clone());
            }
        } else if d.definition.container == "editor" {
            w.editor = Some(selected.clone());
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
        let workspaces = self.workspaces.lock().unwrap();
        let contexts = workspaces.contexts();
        let live = workspaces.ids();
        let initializing = plugins.iter().any(|p| p.status == PluginStatus::Starting);
        let mut ui = self.component_ui.lock().unwrap();
        ui.workspaces.retain(|id, _| live.contains(id));
        let live_views: HashSet<_> = ui
            .workspaces
            .values()
            .flat_map(|w| w.views.iter().map(|v| v.id.clone()))
            .collect();
        ui.surfaces.retain(|_, s| live_views.contains(&s.view));
        let owners: HashMap<_, _> = ui
            .workspaces
            .iter()
            .flat_map(|(workspace, w)| {
                let owner = workspaces.owner(workspace);
                let target = owner
                    .as_ref()
                    .and_then(|owner| workspaces.pending_move(workspace, owner))
                    .map(|m| m.target);
                w.views
                    .iter()
                    .map(move |v| (v.id.clone(), (owner.clone(), target.clone())))
            })
            .collect();
        ui.surfaces.retain(|_, s| {
            owners.get(&s.view).is_some_and(|(owner, target)| {
                owner.as_deref() == Some(&s.window) || target.as_deref() == Some(&s.window)
            })
        });
        for d in ui.definitions.values_mut() {
            d.available = false;
        }
        let mut preferences = ui.preferences.clone();
        for p in plugins.iter().filter(|p| p.status == PluginStatus::Running) {
            if let Some(declaration) = &p.manifest.ui {
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
                        },
                    );
                }
            }
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
                ui.workspaces.insert(
                    id.clone(),
                    WorkspaceUi {
                        preferences: prefs,
                        ..Default::default()
                    },
                );
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
                    let state = &ui.workspaces[id];
                    let side = state
                        .preferences
                        .get(&d.key)
                        .map(|p| p.side.as_str())
                        .unwrap_or("left");
                    let occupied = if d.definition.container == "editor" {
                        state.editor.is_some()
                    } else if side == "left" {
                        state.left.is_some()
                    } else {
                        state.right.is_some()
                    };
                    if policy != "none" && !occupied {
                        ui.open_view(context, &d.key, None, Value::Null, self.next_id(), false)?;
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
        for context in contexts {
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
                        v.error = None;
                    }
                } else {
                    w.views.push(View {
                        calls: std::sync::Arc::new(crate::resource_tasks::Tasks::default()),
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
        Ok(())
    }
    pub fn ui_snapshot(&self, window: &str) -> Result<Value, String> {
        let projection = self.workspace_projection(window)?;
        let ui = self.component_ui.lock().unwrap();
        let ids: HashSet<_> = projection
            .workspaces
            .iter()
            .chain(projection.incoming.iter())
            .map(|w| &w.context.workspace_id)
            .collect();
        Ok(
            json!({"definitions":ui.definitions, "workspaces":ui.workspaces.iter().filter(|(id,_)| ids.contains(id)).collect::<HashMap<_,_>>(), "error":ui.error}),
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
        let workspaces = self.workspaces.lock().unwrap();
        workspaces.validate_owned(window, context)?;
        let mut ui = self.component_ui.lock().unwrap();
        if ui.frozen(&context.workspace_id) {
            return Err("工作区正在确认关闭".into());
        }
        let key = action["key"].as_str().unwrap_or_default();
        let op = action["op"].as_str().ok_or("缺少组件操作")?;
        let mut result = Value::Null;
        match op {
            "open" | "toggle" | "locate" => {
                let request = action["requestId"].as_str().ok_or("缺少 requestId")?;
                if request.len() > 128 {
                    return Err("requestId 过长".into());
                }
                let request_key = format!("{}:{request}", context.workspace_id);
                if let Some(id) = ui.requests.get(&request_key) {
                    return Ok(json!({"viewId":id}));
                }
                let d = ui.definitions.get(key).ok_or("组件未登记")?.clone();
                let state = ui
                    .workspaces
                    .get(&context.workspace_id)
                    .ok_or("组件尚未就绪")?;
                let current =
                    if state.preferences.get(key).map(|p| p.side.as_str()) == Some("right") {
                        &state.right
                    } else {
                        &state.left
                    };
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
                    if w.preferences[key].side == "left" {
                        w.left = None;
                    } else {
                        w.right = None;
                    }
                } else {
                    let id = ui.open_view(
                        context,
                        key,
                        action["resourceKey"].as_str().map(str::to_owned),
                        action["params"].clone(),
                        self.next_id(),
                        op == "locate",
                    )?;
                    if ui.requests.len() >= 256 {
                        ui.requests.clear();
                    }
                    ui.requests.insert(request_key, id.clone());
                    result = json!({"viewId":id});
                }
            }
            "select" => {
                let id = action["viewId"].as_str().ok_or("缺少 viewId")?;
                if ui
                    .definitions
                    .get(&ui.view(id)?.definition_key)
                    .is_none_or(|d| d.definition.container != "editor")
                {
                    return Err("只能选择中央页面".into());
                }
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
                w.editor = Some(id.into());
            }
            "collapse" => {
                let w = ui
                    .workspaces
                    .get_mut(&context.workspace_id)
                    .ok_or("组件工作区已关闭")?;
                match action["side"].as_str() {
                    Some("left") => w.left = None,
                    Some("right") => w.right = None,
                    _ => return Err("无效侧栏".into()),
                }
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
                let w = ui
                    .workspaces
                    .get_mut(&context.workspace_id)
                    .ok_or("组件工作区已关闭")?;
                let moving = w.preferences.get(key).is_some_and(|p| p.side != next.side);
                w.preferences.insert(key.into(), next.clone());
                if moving && d.definition.container == "sidebar" {
                    if let Some(id) = w
                        .views
                        .iter()
                        .find(|v| v.definition_key == key)
                        .map(|v| v.id.clone())
                    {
                        if w.left.as_ref() == Some(&id) {
                            w.left = None;
                        }
                        if w.right.as_ref() == Some(&id) {
                            w.right = None;
                        }
                        if next.side == "left" {
                            w.left = Some(id);
                        } else {
                            w.right = Some(id);
                        }
                    }
                }
            }
            "reorder" => {
                let id = action["viewId"].as_str().ok_or("缺少 viewId")?;
                if ui
                    .definitions
                    .get(&ui.view(id)?.definition_key)
                    .is_none_or(|d| d.definition.container != "editor")
                {
                    return Err("只能排序中央页面".into());
                }
                let index = action["index"].as_u64().ok_or("缺少顺序")? as usize;
                let w = ui
                    .workspaces
                    .get_mut(&context.workspace_id)
                    .ok_or("组件工作区已关闭")?;
                let old = w
                    .views
                    .iter()
                    .position(|v| v.id == id)
                    .ok_or("组件页面已关闭")?;
                let v = w.views.remove(old);
                w.views.insert(index.min(w.views.len()), v);
            }
            _ => return Err("未知组件 UI 操作".into()),
        }
        drop(ui);
        drop(workspaces);
        self.workspace_changed();
        Ok(result)
    }
}

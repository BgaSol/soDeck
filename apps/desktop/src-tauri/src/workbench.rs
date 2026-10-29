//! Authoritative in-memory workbench. Legacy snapshot code is retained only for historical tests.
use crate::window_registry::WindowRecord;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sodeck_plugin_manager::ViewDeclaration;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
use uuid::Uuid;
fn one() -> u32 {
    1
}
fn mount_one() -> u64 {
    1
}
fn main_window() -> String {
    "main".into()
}
fn default_ratio() -> f64 {
    0.5
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub view_id: String,
    #[serde(default)]
    pub plugin_id: String,
    pub view_type: String,
    pub workspace_id: String,
    pub server_id: String,
    pub title: String,
    pub status: String,
    pub placement: String,
    pub group: u8,
    #[serde(default)]
    pub instance_key: String,
    #[serde(default)]
    pub plugin_state: Value,
    #[serde(default)]
    pub actions: Vec<String>,
    #[serde(default)]
    pub revision: u64,
    #[serde(default = "one")]
    pub state_version: u32,
    #[serde(default)]
    pub failure_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery_status: Option<String>,
    #[serde(default)]
    pub active: bool,
    #[serde(default = "mount_one", skip_serializing)]
    pub mount_generation: u64,
    #[serde(skip)]
    pub runtime_data: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DockSlot {
    pub view_id: String,
    pub edge: String,
    pub visible: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    #[serde(skip)]
    pub directory_source_view: Option<String>,
    #[serde(skip)]
    pub directory_selection_revision: u64,
    #[serde(default = "new_generation")]
    pub generation: String,
    #[serde(default)]
    pub initializers: Vec<Initializer>,
    #[serde(default)]
    pub initialization_error: Option<String>,
    #[serde(default = "main_window")]
    pub owner_window_id: String,
    pub split: String,
    #[serde(default)]
    pub dock_slots: Vec<DockSlot>,
    #[serde(default = "default_ratio")]
    pub split_ratio: f64,
    #[serde(default)]
    pub focus_target: Option<String>,
    #[serde(skip)]
    pub dock_widths: BTreeMap<String,u64>,
    #[serde(skip)]
    pub focus_intent: u64,
}
impl Default for Workspace {
    fn default() -> Self {
        Self {
            directory_source_view: None,
            directory_selection_revision: 0,
            generation: new_generation(),
            initializers: vec![],
            initialization_error: None,
            owner_window_id: main_window(),
            split: "none".into(),
            dock_slots: vec![],
            split_ratio: 0.5,
            focus_target: None,
            dock_widths: BTreeMap::new(),
            focus_intent: 0,
        }
    }
}
fn new_generation() -> String { Uuid::new_v4().to_string() }
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Initializer {
    pub id: String,
    pub command_id: String,
    pub operation_id: String,
    pub state: String,
    pub view_id: Option<String>,
    pub error: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workbench {
    pub schema_version: u32,
    #[cfg(test)]
    #[serde(skip)]
    pub migration_pending: bool,
    pub views: Vec<View>,
    pub workspaces: BTreeMap<String, Workspace>,
    pub windows: BTreeMap<String, WindowRecord>,
    pub revision: u64,
    #[serde(skip)]
    pub resources: BTreeMap<String, Vec<Value>>,
    #[serde(skip)]
    declarations: BTreeMap<String, ViewDeclaration>,
    #[serde(skip)]
    pub recovery_warnings: Vec<String>,
    #[cfg(test)]
    #[serde(skip)]
    pub persistence_blocked: Option<String>,
}
impl Default for Workbench {
    fn default() -> Self {
        Self {
            schema_version: 3,
            #[cfg(test)]
            migration_pending: false,
            views: vec![],
            workspaces: BTreeMap::new(),
            windows: [("main".into(), WindowRecord::new("main"))].into(),
            revision: 0,
            resources: BTreeMap::new(),
            declarations: BTreeMap::new(),
            recovery_warnings: vec![],
            #[cfg(test)]
            persistence_blocked: None,
        }
    }
}
impl Workbench {
    #[cfg(test)]
    pub fn load(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let mut value: Value =
            serde_json::from_slice(&bytes).map_err(|_| "布局无法解析；原文件未修改，请恢复备份")?;
        let legacy = value.get("schemaVersion").is_none();
        let upgrading = legacy || value["schemaVersion"] == 2;
        if upgrading {
            use std::io::Write;
            let mut backup = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path.with_file_name(format!("workbench.pre-v3-{}.json", Uuid::new_v4())))
                .map_err(|e| e.to_string())?;
            backup
                .write_all(&bytes)
                .and_then(|_| backup.sync_all())
                .map_err(|e| e.to_string())?;
            if legacy { migrate_legacy(&mut value)?; }
        }
        if ![json!(2),json!(3)].contains(&value["schemaVersion"]) {
            return Err("不支持的布局版本，原文件未修改".into());
        }
        value["schemaVersion"] = json!(3);
        let views = value["views"].as_array().cloned().ok_or("缺少视图列表")?;
        value["views"] = json!([]);
        let mut saved: Self =
            serde_json::from_value(value).map_err(|e| format!("布局结构无效：{e}"))?;
        if saved.workspaces.len() > 100 {
            return Err("布局规模无效".into());
        }
        saved
            .windows
            .entry("main".into())
            .or_insert_with(|| WindowRecord::new("main"));
        let mut ids = BTreeSet::new();
        for raw in views {
            let id = raw["viewId"].as_str().unwrap_or("未知视图").to_string();
            match serde_json::from_value::<View>(raw) {
                Ok(mut v)
                    if !v.view_id.is_empty()
                        && ids.insert(v.view_id.clone())
                        && placement_host(&v.placement).is_some()
                        && v.group <= 1
                        && v.state_version > 0
                        && saved.workspaces.contains_key(&v.workspace_id) =>
                {
                    v.mount_generation = 1;
                    v.runtime_data = Value::Null;
                    if matches!(v.status.as_str(), "running" | "loading" | "connected") {
                        v.status = "ended".into();
                    }
                    saved.views.push(v);
                }
                _ => saved
                    .recovery_warnings
                    .push(format!("已隔离损坏条目 {id}，原文件/迁移备份仍可恢复")),
            }
        }
        for (workspace_id, ws) in &mut saved.workspaces {
            if !["none", "horizontal", "vertical"].contains(&ws.split.as_str()) {
                ws.split = "none".into();
            }
            if !ws.split_ratio.is_finite() || !(0.2..=0.8).contains(&ws.split_ratio) {
                ws.split_ratio = 0.5;
            }
            if !saved.windows.contains_key(&ws.owner_window_id) {
                ws.owner_window_id = "main".into();
            }
            ws.dock_slots.retain(|s| {
                saved.views.iter().any(|v| {
                    v.view_id == s.view_id
                        && v.workspace_id == *workspace_id
                        && v.placement == s.edge
                }) && ["left", "right", "bottom"].contains(&s.edge.as_str())
            });
        }
        let mut seen = BTreeSet::new();
        for (id, w) in &mut saved.windows {
            w.window_id = id.clone();
            w.role = if id == "main" { "main" } else { "workspace" }.into();
            w.workspace_ids.retain(|ws| {
                ws != "@app"
                    && saved
                        .workspaces
                        .get(ws)
                        .is_some_and(|v| v.owner_window_id == *id)
                    && seen.insert(ws.clone())
            });
        }
        for (id, ws) in &saved.workspaces {
            if id == "@app" {
                continue;
            }
            if seen.insert(id.clone()) {
                saved
                    .windows
                    .get_mut(&ws.owner_window_id)
                    .unwrap()
                    .workspace_ids
                    .push(id.clone());
            }
        }
        for w in saved.windows.values_mut() {
            if !w
                .selected_workspace()
                .is_some_and(|id| w.workspace_ids.contains(id))
            {
                w.select_workspace(w.workspace_ids.first().cloned());
            }
        }
        if !legacy && !saved.recovery_warnings.is_empty() {
            std::fs::copy(
                path,
                path.with_file_name(format!("workbench.damaged-{}.json", Uuid::new_v4())),
            )
            .map_err(|e| e.to_string())?;
        }
        saved.migration_pending = upgrading;
        saved.views.sort_by_key(|v| if v.placement == "center" {v.group as u16} else {2});
        for v in &mut saved.views { v.group=0; }
        for ws in saved.workspaces.values_mut() { ws.split="none".into();ws.split_ratio=0.5; }
        Ok(saved)
    }
    #[cfg(test)]
    pub fn set_declarations(&mut self,declarations:Vec<ViewDeclaration>)->Result<(),String>{self.set_owned_declarations(declarations.into_iter().map(|d|(String::new(),d)).collect())}
    pub fn set_owned_declarations(&mut self, declarations: Vec<(String,ViewDeclaration)>) -> Result<(), String> {
        let mut next = BTreeMap::new();
        for (owner,d) in declarations {
            validate_declaration(&d)?;
            if next.insert(format!("{owner}::{}",d.id), d).is_some() {
                return Err("视图声明重复".into());
            }
        }
        self.declarations = next;
        let mut singles = BTreeSet::new();
        let mut relocated = BTreeSet::new();
        for v in &mut self.views {
            match self.declarations.get(&format!("{}::{}",v.plugin_id,v.view_type)).or_else(||self.declarations.get(&format!("::{}",v.view_type))) {
                None => {
                    if v.status != "viewFailed" {
                        v.recovery_status = Some(v.status.clone());
                    }
                    v.status = "viewFailed".into();
                    v.failure_reason = Some("所属插件不可用，请恢复兼容插件".into());
                }
                Some(d) => {
                    if v.state_version != d.state_version
                        || (!d.multiple
                            && !singles.insert((v.plugin_id.clone(),v.view_type.clone(), v.workspace_id.clone())))
                    {
                        v.status = "viewFailed".into();
                        v.failure_reason = Some("保存状态不兼容，请明确重新加载".into());
                        continue;
                    }
                    if !supports(d, &v.placement) {
                        relocated.insert(v.view_id.clone());
                        v.placement = default_placement(d)?;
                        v.group = 0;
                    }
                    if let Some(status) = v.recovery_status.take() {
                        v.status = status;
                        v.failure_reason = None;
                    }
                }
            }
        }
        self.views.sort_by_key(|v| relocated.contains(&v.view_id));
        for id in self.workspaces.keys().cloned().collect::<Vec<_>>() {
            let ws = self.workspaces.get_mut(&id).unwrap();
            ws.dock_slots.retain(|slot| {
                self.views.iter().any(|v| {
                    v.view_id == slot.view_id && v.workspace_id == id && v.placement == slot.edge
                })
            });
            for view in self.views.iter().filter(|v| {
                v.workspace_id == id && ["left", "right", "bottom"].contains(&v.placement.as_str())
            }) {
                if !ws
                    .dock_slots
                    .iter()
                    .any(|slot| slot.view_id == view.view_id)
                {
                    ws.dock_slots.push(DockSlot {
                        view_id: view.view_id.clone(),
                        edge: view.placement.clone(),
                        visible: true,
                    });
                }
            }
            if let Some(focus)=ws.focus_target.clone().or_else(||self.windows.values().find(|w|w.selected_workspace().map(String::as_str)==Some(&id)).and_then(|w|w.focused_view_id.clone())) {
                if self.views.iter().any(|v|v.view_id==focus && v.placement=="center") { for v in self.views.iter_mut().filter(|v|v.workspace_id==id && v.placement=="center") {v.active=v.view_id==focus;} }
            }
            self.normalize(&id);
        }
        Ok(())
    }
    #[cfg(test)]
    pub fn retire_views(&mut self, plugin: &str, ids: &[String]) {
        self.views.retain(|v| !(v.plugin_id==plugin && ids.contains(&v.view_type) && v.runtime_data.is_null() && !self.resources.contains_key(&v.view_id)));
        for ws in self.workspaces.values_mut() { ws.dock_slots.retain(|s|self.views.iter().any(|v|v.view_id==s.view_id)); }
    }
    pub fn reset_incompatible_state(&mut self, id: &str) -> Result<bool, String> {
        let v = self.view(id)?;
        let d = self.declarations.get(&format!("{}::{}",v.plugin_id,v.view_type)).or_else(||self.declarations.get(&format!("::{}",v.view_type))).ok_or("插件不可用")?;
        if d.state_version == v.state_version {
            return Ok(false);
        }
        if self.resources.get(id).is_some_and(|r| !r.is_empty()) {
            return Err("视图仍有运行资源".into());
        }
        let version = d.state_version;
        let v = self.views.iter_mut().find(|v| v.view_id == id).unwrap();
        v.state_version = version;
        v.plugin_state = Value::Null;
        v.failure_reason = None;
        self.revision += 1;
        Ok(true)
    }
    pub fn owner(&self, id: &str) -> Result<String, String> {
        self.workspaces
            .get(id)
            .map(|w| w.owner_window_id.clone())
            .ok_or_else(|| "工作区已关闭".into())
    }
    pub fn view_window(&self, v: &View) -> Result<String, String> {
        if v.placement == "detached" {
            Ok(format!("detached-{}", v.view_id))
        } else {
            self.owner(&v.workspace_id)
        }
    }
    pub fn open_workspace(&mut self, id: &str, window: &str) -> Result<bool, String> {
        if id.is_empty() {
            return Err("服务器标识为空".into());
        }
        if self.workspaces.contains_key(id) {
            self.activate_workspace(id)?;
            return Ok(false);
        }
        let w = self.windows.get_mut(window).ok_or("窗口不存在")?;
        w.workspace_ids.push(id.into());
        w.select_workspace(Some(id.into()));
        self.workspaces.insert(
            id.into(),
            Workspace {
                owner_window_id: window.into(),
                ..Workspace::default()
            },
        );
        self.revision += 1;
        Ok(true)
    }
    pub fn activate_workspace(&mut self, id: &str) -> Result<(), String> {
        if id == "@app" {
            return Ok(());
        }
        let owner = self.owner(id)?;
        self.windows
            .get_mut(&owner)
            .ok_or("窗口不存在")?
            .select_workspace(Some(id.into()));
        self.windows.get_mut(&owner).unwrap().focused_view_id =
            self.workspaces.get(id).and_then(|w| w.focus_target.clone());
        self.revision += 1;
        Ok(())
    }
    pub fn reorder(&mut self, window: &str, ids: Vec<String>) -> Result<(), String> {
        let w = self.windows.get_mut(window).ok_or("窗口已关闭")?;
        let existing: BTreeSet<_> = w.workspace_ids.iter().collect();
        if ids.len() != existing.len() || ids.iter().collect::<BTreeSet<_>>() != existing {
            return Err("排序须包含全部工作区且无重复".into());
        }
        w.workspace_ids = ids;
        self.revision += 1;
        Ok(())
    }
    pub fn move_workspaces(
        &mut self,
        ids: &[String],
        target: &str,
        index: usize,
        active: Option<&str>,
    ) -> Result<(), String> {
        if !self.windows.contains_key(target)
            || ids.is_empty()
            || ids.iter().collect::<BTreeSet<_>>().len() != ids.len()
        {
            return Err("移动目标或批次无效".into());
        }
        for id in ids {
            self.owner(id)?;
        }
        for id in ids {
            let source = self.owner(id)?;
            self.windows.get_mut(&source).unwrap().remove(id);
        }
        let w = self.windows.get_mut(target).unwrap();
        let at = index.min(w.workspace_ids.len());
        w.workspace_ids.splice(at..at, ids.iter().cloned());
        w.select_workspace(Some(
            active
                .filter(|a| ids.iter().any(|s| s == a))
                .unwrap_or(&ids[0])
                .into(),
        ));
        for id in ids {
            self.workspaces.get_mut(id).unwrap().owner_window_id = target.into();
        }
        self.revision += 1;
        Ok(())
    }
    pub fn remove_workspace(&mut self, id: &str) -> Result<(), String> {
        if self.views.iter().any(|v| v.workspace_id == id) {
            return Err("请先关闭工作区视图".into());
        }
        if let Some(ws) = self.workspaces.remove(id) {
            if let Some(w) = self.windows.get_mut(&ws.owner_window_id) {
                w.remove(id);
            }
            self.revision += 1;
        }
        Ok(())
    }
    pub fn snapshot(&self, window: &str) -> Value {
        let record = self.windows.get(window);
        let detached = window.strip_prefix("detached-");
        let active = detached
            .and_then(|id| self.views.iter().find(|v| v.view_id == id))
            .map(|v| v.workspace_id.clone())
            .or_else(|| record.and_then(|w| w.selected_workspace().cloned()));
        let mut ws = active
            .as_ref()
            .and_then(|id| self.workspaces.get(id))
            .cloned()
            .unwrap_or_default();
        if window == "main" && record.is_none_or(|w|w.selection!=crate::window_registry::Selection::ConnectionHome) {
            if let Some(global) = self.workspaces.get("@app") {
                ws.dock_slots.extend(global.dock_slots.clone());
            }
        }
        let views = self
            .views
            .iter()
            .map(|v| {
                let mut value = serde_json::to_value(v).unwrap();
                value["mountGeneration"] = json!(v.mount_generation);
                value
            })
            .collect::<Vec<_>>();
        json!({"schemaVersion":4,"windowId":window,"windowRole":if detached.is_some(){"view"}else if window=="main"{"main"}else{"workspace"},"workspaceIds":record.map(|w|w.workspace_ids.clone()).unwrap_or_default(),"activeWorkspaceId":active,"focusedViewId":record.filter(|w|w.selected_workspace().is_some()).and_then(|w|w.focused_view_id.clone()),"workspaces":self.workspaces.iter().map(|(id,w)|json!({"workspaceId":id,"serverId":id,"ownerWindowId":w.owner_window_id,"generation":w.generation,"initializers":w.initializers,"initializationError":w.initialization_error})).collect::<Vec<_>>(),"views":views,"split":ws.split,"splitRatio":ws.split_ratio,"dockSlots":ws.dock_slots,"revision":self.revision,"recoveryWarnings":self.recovery_warnings})
    }
    pub fn open_view(
        &mut self,
        plugin: &str,
        kind: &str,
        workspace: &str,
        key: &str,
    ) -> Result<View, String> {
        self.open_view_with_activation(plugin, kind, workspace, key, true)
    }
    /// Automatic workspace contributions must not steal focus or an occupied dock.
    pub fn open_default_view(&mut self, plugin: &str, kind: &str, workspace: &str, key: &str) -> Result<View, String> {
        self.open_view_with_activation(plugin, kind, workspace, key, false)
    }
    fn open_view_with_activation(&mut self, plugin: &str, kind: &str, workspace: &str, key: &str, activate: bool) -> Result<View, String> {
        let d = self
            .declarations
            .get(&format!("{plugin}::{kind}")).or_else(||self.declarations.get(&format!("::{kind}")))
            .cloned()
            .ok_or("插件未声明视图")?;
        let workspace = if d.scope == "global" {
            "@app"
        } else {
            workspace
        };
        if workspace == "@app" {
            self.workspaces.entry(workspace.into()).or_default();
        }
        self.owner(workspace)?;
        if let Some(v) = self
            .views
            .iter()
            .find(|v| {
                v.plugin_id == plugin
                    && v.view_type == kind
                    && v.workspace_id == workspace
                    && (!d.multiple || (!key.is_empty() && v.instance_key == key))
            })
            .cloned()
        {
            return if activate { self.activate(&v.view_id) } else { Ok(v) };
        }
        let placement = default_placement(&d)?;
        let mut v = View {
            view_id: Uuid::new_v4().to_string(),
            plugin_id: plugin.into(),
            view_type: kind.into(),
            workspace_id: workspace.into(),
            server_id: workspace.into(),
            title: d.title,
            status: "ready".into(),
            placement: placement.clone(),
            group: 0,
            instance_key: key.into(),
            plugin_state: Value::Null,
            actions: vec![],
            revision: 1,
            state_version: d.state_version,
            failure_reason: None,
            recovery_status: None,
            active: true,
            mount_generation: 1,
            runtime_data: Value::Null,
        };
        v.active = activate || !self.views.iter().any(|current| same_container(current, &v) && current.active);
        for current in &mut self.views {
            if same_container(current, &v) {
                if v.active { current.active = false; }
            }
        }
        if ["left", "right", "bottom"].contains(&placement.as_str()) {
            self.workspaces
                .get_mut(workspace)
                .unwrap()
                .dock_slots
                .push(DockSlot {
                    view_id: v.view_id.clone(),
                    edge: placement,
                    visible: v.active,
                });
        }
        self.views.push(v.clone());
        if !activate { self.revision += 1; return Ok(v); }
        self.activate_workspace(workspace)?;
        self.activate(&v.view_id)
    }
    pub fn view(&self, id: &str) -> Result<View, String> {
        self.views
            .iter()
            .find(|v| v.view_id == id)
            .cloned()
            .ok_or_else(|| "视图已关闭".into())
    }
    pub fn activate(&mut self, id: &str) -> Result<View, String> {
        let view = self.view(id)?;
        let directory=self.provides_directory(&view);
        self.activate_workspace(&view.workspace_id)?;
        let owner = self.owner(&view.workspace_id)?;
        if let Some(window) = self.windows.get_mut(&owner) {
            window.focused_view_id = Some(id.into());
        }
        self.revision += 1;
        for v in &mut self.views {
            if same_container(v, &view) {
                v.active = v.view_id == id;
            }
        }
        if let Some(ws) = self.workspaces.get_mut(&view.workspace_id) {
            if directory && ws.directory_source_view.as_deref()!=Some(id) {
                ws.directory_source_view=Some(id.into());
                ws.directory_selection_revision+=1;
            }
            ws.focus_target = Some(id.into());
            for slot in &mut ws.dock_slots {
                if slot.view_id == id {
                    slot.visible = true;
                }
            }
        }
        self.view(id)
    }
    pub fn provides_directory(&self,view:&View)->bool {
        self.declarations.get(&format!("{}::{}",view.plugin_id,view.view_type))
            .or_else(||self.declarations.get(&format!("::{}",view.view_type)))
            .is_some_and(|d|d.context_providers.iter().any(|p|p=="directory"))
    }
    pub fn close(&mut self, id: &str) -> Result<View, String> {
        let view = self.view(id)?;
        self.views.retain(|v| v.view_id != id);
        self.resources.remove(id);
        if let Some(w) = self.workspaces.get_mut(&view.workspace_id) {
            w.dock_slots.retain(|s| s.view_id != id);
        }
        self.normalize(&view.workspace_id);
        if self.workspaces.get(&view.workspace_id).is_some_and(|ws|ws.directory_source_view.as_deref()==Some(id)) {
            let replacement=self.workspaces.get(&view.workspace_id).and_then(|ws|ws.focus_target.as_deref()).and_then(|id|self.view(id).ok()).filter(|v|self.provides_directory(v)).map(|v|v.view_id);
            if let Some(ws)=self.workspaces.get_mut(&view.workspace_id){ws.directory_source_view=replacement;ws.directory_selection_revision+=1;}
        }
        self.revision += 1;
        Ok(view)
    }
    pub fn validate_placement(&self, kind: &str, location: &str) -> Result<(), String> {
        if self
            .declarations
            .get(kind).or_else(||self.declarations.get(&format!("::{}",kind.rsplit("::").next().unwrap_or(kind))))
            .is_some_and(|d| supports(d, location))
        {
            Ok(())
        } else {
            Err("该视图不支持此位置".into())
        }
    }
    pub fn moved(
        &mut self,
        id: &str,
        placement: &str,
        group: u8,
        generation: u64,
    ) -> Result<View, String> {
        let old = self.view(id)?;
        self.validate_placement(&format!("{}::{}",old.plugin_id,old.view_type), placement)?;
        if group != 0 {
            return Err("当前仅支持一个中央区域".into());
        }
        let mut next = old.clone();
        next.placement = placement.into();
        next.group = group;
        next.mount_generation = generation;
        next.active = true;
        for v in &mut self.views {
            if same_container(v, &next) {
                v.active = false;
            }
        }
        *self.views.iter_mut().find(|v| v.view_id == id).unwrap() = next;
        let ws = self.workspaces.get_mut(&old.workspace_id).unwrap();
        ws.dock_slots.retain(|s| s.view_id != id);
        if ["left", "right", "bottom"].contains(&placement) {
            ws.dock_slots.push(DockSlot {
                view_id: id.into(),
                edge: placement.into(),
                visible: true,
            });
        }
        self.normalize(&old.workspace_id);
        self.revision += 1;
        self.view(id)
    }
    pub fn configure(&mut self, window: &str, params: &Value) -> Result<(), String> {
        let global_dock = params["dockViewId"].as_str().is_some_and(|id| {
            self.views
                .iter()
                .any(|v| v.view_id == id && v.workspace_id == "@app")
        });
        let id = if global_dock && window == "main" {
            Some("@app".into())
        } else {
            self.windows
                .get(window)
                .and_then(|w| w.selected_workspace().cloned())
                .or_else(|| {
                    (window == "main" && self.workspaces.contains_key("@app"))
                        .then(|| "@app".into())
                })
        }
        .ok_or("请先打开工作区")?;
        let mut next = self.clone();
        let w = next.workspaces.get_mut(&id).unwrap();
        if params.get("split").is_some_and(|s|s != "none") || params.get("splitRatio").is_some() { return Err("不再支持分屏".into()); }
        if let Some(view) = params["dockViewId"].as_str() {
            let slot = w
                .dock_slots
                .iter_mut()
                .find(|s| s.view_id == view)
                .ok_or("停靠视图不存在")?;
            if let Some(visible) = params["visible"].as_bool() {
                slot.visible = visible;
            }
        }
        next.normalize_active(&id);
        next.revision += 1;
        *self = next;
        Ok(())
    }
    pub fn commit(&mut self, candidate: Self) -> Result<(), String> {
        *self = candidate;
        Ok(())
    }
    #[cfg(test)]
    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(reason) = &self.persistence_blocked {
            return Err(format!("原布局已保护，恢复备份前不会覆盖：{reason}"));
        }
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        let pending = path.with_extension("pending");
        use std::io::Write;
        let mut file = std::fs::File::create(&pending).map_err(|e| e.to_string())?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
        drop(file);
        replace(&pending, path)
    }
    #[cfg(test)]
    pub fn retry_save(&mut self,path:&Path)->Result<(),String>{
        if self.persistence_blocked.is_some() && !self.migration_pending {return Err("原布局无法读取，请退出应用后恢复 workbench.pre-v3 备份；不会用空布局覆盖".into());}
        let protected=self.persistence_blocked.take();
        if let Err(error)=self.save(path){self.persistence_blocked=protected;return Err(error);}
        self.migration_pending=false;Ok(())
    }
    fn normalize(&mut self, id: &str) {
        for v in self.views.iter_mut().filter(|v|v.workspace_id==id){v.group=0;}
        if let Some(w)=self.workspaces.get_mut(id){w.split="none".into();w.split_ratio=0.5;}
        self.normalize_active(id);
    }
    fn normalize_active(&mut self, id: &str) {
        let candidates = self
            .views
            .iter()
            .filter(|v| v.workspace_id == id)
            .cloned()
            .collect::<Vec<_>>();
        let mut seen = BTreeSet::new();
        for candidate in &candidates {
            let key = if candidate.placement == "detached" {
                candidate.view_id.clone()
            } else {
                format!("{}:{}", candidate.placement, candidate.group)
            };
            if !seen.insert(key) {
                continue;
            }
            let chosen = candidates
                .iter()
                .find(|v| same_container(v, candidate) && v.active)
                .unwrap_or(candidate);
            for v in &mut self.views {
                if same_container(v, candidate) {
                    v.active = v.view_id == chosen.view_id;
                }
            }
        }
    }
}
#[cfg(test)]
fn migrate_legacy(value: &mut Value) -> Result<(), String> {
    let active = value["activeWorkspaceId"].clone();
    let views = value["views"].as_array_mut().ok_or("旧布局缺少视图")?;
    for v in views {
        v["pluginId"] = json!("sodeck.terminal");
        if v["viewType"] == "terminal.session" {
            v["status"] = json!("ended");
            v["actions"] = json!(["terminal.new"]);
        }
        if let Some(obj) = v.as_object_mut() {
            for key in ["sessionId", "ptyId", "generation"] {
                obj.remove(key);
            }
        }
    }
    let views = value["views"].as_array().cloned().unwrap();
    let workspaces = value["workspaces"]
        .as_object_mut()
        .ok_or("旧布局缺少工作区")?;
    let ids = workspaces.keys().cloned().collect::<Vec<_>>();
    for (id, ws) in workspaces {
        ws["ownerWindowId"] = json!("main");
        let visible = ws["sessionsVisible"].as_bool().unwrap_or(true);
        ws["dockSlots"] = json!(views
            .iter()
            .filter(|v| v["workspaceId"] == *id
                && ["left", "right", "bottom"].contains(&v["placement"].as_str().unwrap_or("")))
            .map(|v| json!({"viewId":v["viewId"],"edge":v["placement"],"visible":visible}))
            .collect::<Vec<_>>());
        if let Some(obj) = ws.as_object_mut() {
            obj.remove("sessionsSide");
            obj.remove("sessionsVisible");
        }
    }
    value["schemaVersion"] = json!(2);
    value["revision"] = json!(value["revision"].as_u64().unwrap_or(0));
    value["windows"] = json!({"main":{"windowId":"main","role":"main","workspaceIds":ids,"activeWorkspaceId":active}});
    Ok(())
}
fn same_container(a: &View, b: &View) -> bool {
    a.workspace_id == b.workspace_id
        && a.placement == b.placement
        && a.group == b.group
        && (a.placement != "detached" || a.view_id == b.view_id)
}
fn placement_host(p: &str) -> Option<&str> {
    match p {
        "center" => Some("tab"),
        "detached" => Some("window"),
        "left" | "right" | "bottom" => Some("dock"),
        _ => None,
    }
}
fn supports(d: &ViewDeclaration, p: &str) -> bool {
    placement_host(p).is_some_and(|host| {
        d.supported_hosts.iter().any(|s| s == host)
            && (host != "dock" || d.dock_edges.iter().any(|s| s == p))
    })
}
fn default_placement(d: &ViewDeclaration) -> Result<String, String> {
    let p = match d.default_location.host.as_str() {
        "tab" => "center",
        "window" => "detached",
        "dock" => d.default_location.edge.as_deref().ok_or("缺少默认停靠边")?,
        _ => return Err("默认位置无效".into()),
    };
    if !supports(d, p) {
        return Err("默认位置不受支持".into());
    }
    Ok(p.into())
}
fn validate_declaration(d: &ViewDeclaration) -> Result<(), String> {
    if d.id.is_empty()
        || d.title.trim().is_empty()
        || d.state_version == 0
        || !["server", "global"].contains(&d.scope.as_str())
        || d.supported_hosts
            .iter()
            .any(|s| !["tab", "dock", "window"].contains(&s.as_str()))
        || d.dock_edges
            .iter()
            .any(|s| !["left", "right", "bottom"].contains(&s.as_str()))
    {
        return Err("视图声明无效".into());
    }
    default_placement(d).map(|_| ())
}
pub(crate) fn replace(source: &Path, target: &Path) -> Result<(), String> { crate::atomic_file::replace(source,target) }

#[cfg(test)]
mod tests {
    use super::*;
    fn declaration(id: &str, multiple: bool) -> ViewDeclaration {
        serde_json::from_value(json!({"id":id,"title":format!("{id} title"),"scope":"server","supportedHosts":["tab","dock","window"],"dockEdges":["left","right","bottom"],"defaultLocation":{"host":"tab"},"multiple":multiple,"stateVersion":1,"minWidth":200,"minHeight":160})).unwrap()
    }
    fn model() -> Workbench {
        let mut w = Workbench::default();
        w.set_declarations(vec![
            declaration("sample.list", false),
            declaration("sample.page", true),
        ])
        .unwrap();
        w.open_workspace("a", "main").unwrap();
        w.open_workspace("b", "main").unwrap();
        w
    }
    fn open(w: &mut Workbench, ws: &str) -> View {
        w.open_view("sample.plugin", "sample.page", ws, "").unwrap()
    }
    #[test]
    fn default_views_preserve_focus_and_dock_selection() {
        let mut w=model();
        let mut first=declaration("first",false); first.default_location.host="dock".into();first.default_location.edge=Some("right".into());
        let mut second=first.clone(); second.id="second".into();
        w.set_declarations(vec![declaration("sample.page",true),first,second]).unwrap();
        let terminal=open(&mut w,"a");
        let a=w.open_default_view("sample.plugin","first","a","").unwrap();
        let b=w.open_default_view("sample.plugin","second","a","").unwrap();
        assert!(a.active); assert!(!b.active);
        assert_eq!(w.windows["main"].focused_view_id.as_deref(),Some(terminal.view_id.as_str()));
        assert_eq!(w.open_default_view("sample.plugin","first","a","").unwrap().view_id,a.view_id);
        w.activate(&b.view_id).unwrap();
        assert!(!w.open_default_view("sample.plugin","first","a","").unwrap().active);
        assert_eq!(w.windows["main"].focused_view_id.as_deref(),Some(b.view_id.as_str()));
    }
    #[test]
    fn directory_selection_is_separate_from_focus_and_workspace() {
        let mut w = model();
        let mut provider = declaration("sample.page", true);
        provider.context_providers=vec!["directory".into()];
        w.set_declarations(vec![provider,declaration("sample.list",false)]).unwrap();
        let a=open(&mut w,"a");let b=open(&mut w,"a");let other=open(&mut w,"b");
        w.activate(&a.view_id).unwrap();
        let revision=w.workspaces["a"].directory_selection_revision;
        let files=w.open_view("sample.plugin","sample.list","a","").unwrap();
        w.activate(&files.view_id).unwrap();
        assert_eq!(w.workspaces["a"].directory_source_view.as_deref(),Some(a.view_id.as_str()));
        assert_eq!(w.workspaces["a"].directory_selection_revision,revision);
        w.activate(&b.view_id).unwrap();
        assert_eq!(w.workspaces["a"].directory_selection_revision,revision+1);
        assert_eq!(w.workspaces["b"].directory_source_view.as_deref(),Some(other.view_id.as_str()));
    }
    #[test]
    fn singleton_multiple_and_instance_key_are_independent_of_workers() {
        let mut w = model();
        let a = w
            .open_view("sample.plugin", "sample.list", "a", "")
            .unwrap();
        assert_eq!(
            a.view_id,
            w.open_view("sample.plugin", "sample.list", "a", "")
                .unwrap()
                .view_id
        );
        assert_ne!(
            a.view_id,
            w.open_view("sample.plugin", "sample.list", "b", "")
                .unwrap()
                .view_id
        );
        let x = open(&mut w, "a");
        let y = open(&mut w, "a");
        assert_ne!(x.view_id, y.view_id);
        let x = w
            .open_view("sample.plugin", "sample.page", "a", "key")
            .unwrap();
        assert_eq!(
            x.view_id,
            w.open_view("sample.plugin", "sample.page", "a", "key")
                .unwrap()
                .view_id
        );
    }
    #[test]
    fn two_plugins_can_own_the_same_local_singleton_type() {
        let mut w=Workbench::default();
        let declarations=vec![("plugin.a".into(),declaration("same.local",false)),("plugin.b".into(),declaration("same.local",false))];
        w.set_owned_declarations(declarations.clone()).unwrap();w.open_workspace("server","main").unwrap();
        let a=w.open_view("plugin.a","same.local","server","").unwrap();let b=w.open_view("plugin.b","same.local","server","").unwrap();
        assert_ne!(a.view_id,b.view_id);w.set_owned_declarations(declarations).unwrap();
        assert!(w.views.iter().all(|v|v.status!="viewFailed"));assert_eq!(w.views.len(),2);
    }
    #[test]
    fn global_singleton_keeps_app_scope() {
        let mut w = model();
        let mut d = declaration("global", false);
        d.scope = "global".into();
        d.default_location.host = "window".into();
        w.set_declarations(vec![d]).unwrap();
        let a = w.open_view("sample.plugin", "global", "a", "").unwrap();
        let b = w.open_view("sample.plugin", "global", "b", "").unwrap();
        assert_eq!(a.workspace_id, "@app");
        assert_eq!(a.view_id, b.view_id);
        assert_eq!(a.placement, "detached");
    }
    #[test]
    fn global_workspace_stays_out_of_server_tabs_after_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("layout.json");
        let mut w = model();
        let mut d = declaration("global", false);
        d.scope = "global".into();
        d.default_location.host = "window".into();
        w.set_declarations(vec![d]).unwrap();
        w.open_view("sample.plugin", "global", "a", "").unwrap();
        w.save(&path).unwrap();
        let restored = Workbench::load(&path).unwrap();
        assert!(restored.workspaces.contains_key("@app"));
        assert!(!restored.windows["main"]
            .workspace_ids
            .contains(&"@app".into()));
    }
    #[test]
    fn recovery_fallback_cannot_overwrite_broken_original() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("layout.json");
        std::fs::write(&path, b"broken").unwrap();
        let mut fallback = Workbench::default();
        fallback.persistence_blocked = Some(Workbench::load(&path).err().unwrap());
        assert!(fallback.save(&path).is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"broken");
    }
    #[test]
    fn arbitrary_defaults_and_unsupported_placements_are_atomic() {
        let mut w = model();
        let mut d = declaration("inspector", false);
        d.supported_hosts = vec!["dock".into(), "window".into()];
        d.dock_edges = vec!["right".into()];
        d.default_location.host = "dock".into();
        d.default_location.edge = Some("right".into());
        w.set_declarations(vec![d]).unwrap();
        let a = w.open_view("sample.plugin", "inspector", "a", "").unwrap();
        assert_eq!(a.title, "inspector title");
        assert_eq!(a.placement, "right");
        assert!(w.moved(&a.view_id, "left", 0, 2).is_err());
        assert!(w.moved(&a.view_id, "center", 0, 2).is_err());
        assert_eq!(w.view(&a.view_id).unwrap().placement, "right");
        assert!(w.moved(&a.view_id, "detached", 0, 2).is_ok());
        assert!(w.open_view("sample.plugin", "missing", "a", "").is_err());
    }
    #[test]
    fn schema_two_merges_in_order_and_retires_only_owned_navigation() {
        let dir = tempfile::tempdir().unwrap(); let path = dir.path().join("layout.json");
        let mut w=model();
        let a=open(&mut w,"a");let b=open(&mut w,"a");let c=open(&mut w,"a");
        w.views[0].group=1;w.views[1].group=0;w.views[2].placement="bottom".into();
        let mut retired=w.views[0].clone();retired.view_id="navigation".into();retired.view_type="old.navigation".into();retired.placement="left".into();w.views.push(retired);
        let mut foreign=w.views[3].clone();foreign.view_id="foreign".into();foreign.plugin_id="other.plugin".into();w.views.push(foreign);
        w.workspaces.get_mut("a").unwrap().focus_target=Some(c.view_id.clone());
        let mut raw=serde_json::to_value(&w).unwrap();raw["schemaVersion"]=json!(2);
        let bytes=serde_json::to_vec(&raw).unwrap();std::fs::write(&path,&bytes).unwrap();
        let mut upgraded=Workbench::load(&path).unwrap();assert_eq!(std::fs::read(&path).unwrap(),bytes);
        upgraded.retire_views("sample.plugin",&["old.navigation".into()]);
        let mut page=declaration("sample.page",true);page.supported_hosts=vec!["tab".into(),"window".into()];page.dock_edges.clear();
        upgraded.set_declarations(vec![page]).unwrap();
        assert_eq!(upgraded.views.iter().filter(|v|v.placement=="center").map(|v|v.view_id.as_str()).collect::<Vec<_>>(),vec![b.view_id.as_str(),a.view_id.as_str(),c.view_id.as_str()]);
        assert!(upgraded.view("navigation").is_err());assert!(upgraded.view("foreign").is_ok());
        assert!(upgraded.view(&c.view_id).unwrap().active);assert!(upgraded.views.iter().all(|v|v.group==0));
        upgraded.save(&path).unwrap();let count=std::fs::read_dir(dir.path()).unwrap().count();
        let again=Workbench::load(&path).unwrap();assert!(!again.migration_pending);assert_eq!(again.views.len(),upgraded.views.len());assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(),count);
    }
    #[test]
    fn invalid_catalog_keeps_previous_catalog() {
        let mut w = model();
        let mut bad = declaration("bad", false);
        bad.default_location.host = "invalid".into();
        assert!(w.set_declarations(vec![bad]).is_err());
        assert!(w.open_view("sample.plugin", "sample.list", "a", "").is_ok());
        let d = declaration("duplicate", false);
        assert!(w.set_declarations(vec![d.clone(), d]).is_err());
    }
    #[test]
    fn single_group_rejects_split_and_close_keeps_other_workspace() {
        let mut w = model();
        let a = open(&mut w, "a");
        let b = open(&mut w, "a");
        let other = open(&mut w, "b");
        let before = w.snapshot("main");
        assert!(w.moved(&b.view_id, "center", 1, 2).is_err());
        assert_eq!(w.snapshot("main"), before);
        assert_eq!(w.workspaces["b"].split, "none");
        w.activate_workspace("a").unwrap();
        w.configure("main", &json!({"split":"none"})).unwrap();
        assert_eq!(
            w.views
                .iter()
                .filter(|v| v.workspace_id == "a" && v.active)
                .count(),
            1
        );
        w.close(&a.view_id).unwrap();
        assert!(w.view(&b.view_id).unwrap().active);
        assert!(w.view(&other.view_id).is_ok());
    }
    #[test]
    fn bad_configuration_does_not_partially_change_layout() {
        let mut w = model();
        let before = w.snapshot("main");
        assert!(w
            .configure(
                "main",
                &json!({"split":"vertical","dockViewId":"missing","visible":false})
            )
            .is_err());
        assert_eq!(w.snapshot("main"), before);
        assert!(w
            .configure("main", &json!({"split":"vertical","splitRatio":2}))
            .is_err());
        assert_eq!(w.snapshot("main"), before);
    }
    #[test]
    fn retry_migration_save_keeps_original_until_success_and_rejects_broken_fallback() {
        let dir=tempfile::tempdir().unwrap();let path=dir.path().join("layout.json");let mut w=model();w.save(&path).unwrap();let original=std::fs::read(&path).unwrap();
        w.migration_pending=true;w.persistence_blocked=Some("write failed".into());std::fs::create_dir(path.with_extension("pending")).unwrap();
        assert!(w.retry_save(&path).is_err());assert_eq!(std::fs::read(&path).unwrap(),original);assert!(w.persistence_blocked.is_some());
        std::fs::remove_dir(path.with_extension("pending")).unwrap();w.retry_save(&path).unwrap();assert!(!w.migration_pending);assert!(w.persistence_blocked.is_none());
        w.persistence_blocked=Some("broken source".into());assert!(w.retry_save(&path).is_err());
    }
    #[test]
    fn runtime_commit_does_not_write_or_depend_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("workbench.json");
        let mut w = model(); open(&mut w, "a");
        std::fs::create_dir(path.with_extension("pending")).unwrap();
        let mut next = w.clone(); open(&mut next, "b");
        w.commit(next).unwrap(); assert_eq!(w.views.len(),2); assert!(!path.exists());
    }
    #[test]
    fn layout_never_persists_runtime_handles_or_catalog() {
        let mut w = model();
        let v = open(&mut w, "a");
        w.views[0].runtime_data = json!({"sessionId":"secret-runtime"});
        w.resources
            .insert(v.view_id, vec![json!({"kind":"pty","id":"runtime-pty"})]);
        let raw = serde_json::to_string(&w).unwrap();
        assert!(!raw.contains("secret-runtime"));
        assert!(!raw.contains("runtime-pty"));
        assert!(!raw.contains("mountGeneration"));
        assert!(!raw.contains("declarations"));
    }
    #[test]
    fn legacy_backup_and_restart_preserve_placeholders_without_handles() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("workbench.json");
        let legacy = json!({"activeWorkspaceId":"a","workspaces":{"a":{"split":"none","sessionsSide":"left","sessionsVisible":true}},"views":[{"viewId":"one","viewType":"terminal.session","workspaceId":"a","serverId":"a","title":"saved title","status":"running","placement":"center","group":0,"sessionId":"obsolete","ptyId":"obsolete","generation":5}]});
        std::fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
        let mut w = Workbench::load(&path).unwrap();
        assert_eq!(w.views[0].title, "saved title");
        assert_eq!(w.views[0].status, "ended");
        assert!(w.resources.is_empty());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
        assert!(std::fs::read_to_string(&path).unwrap().contains("obsolete"));
        w.save(&path).unwrap();
        assert!(!std::fs::read_to_string(&path).unwrap().contains("obsolete"));
        w.views[0].status = "viewFailed".into();
        w.views[0].failure_reason = Some("failure retained".into());
        w.save(&path).unwrap();
        let w = Workbench::load(&path).unwrap();
        assert_eq!(w.views[0].status, "viewFailed");
        assert_eq!(
            w.views[0].failure_reason.as_deref(),
            Some("failure retained")
        );
        assert_eq!(w.views[0].mount_generation, 1);
    }
    #[test]
    fn malformed_entries_are_isolated_and_bad_file_is_not_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("layout.json");
        let mut w = model();
        open(&mut w, "a");
        let mut raw = serde_json::to_value(w).unwrap();
        raw["views"]
            .as_array_mut()
            .unwrap()
            .push(json!({"viewId":"bad"}));
        std::fs::write(&path, serde_json::to_vec(&raw).unwrap()).unwrap();
        let restored = Workbench::load(&path).unwrap();
        assert_eq!(restored.views.len(), 1);
        assert_eq!(restored.recovery_warnings.len(), 1);
        std::fs::write(&path, b"broken").unwrap();
        assert!(Workbench::load(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"broken");
    }
    #[test]
    fn incompatible_state_requires_explicit_recovery_and_cannot_drop_resources() {
        let mut w = model();
        let v = open(&mut w, "a");
        w.views[0].plugin_state = json!({"old":true});
        let mut d = declaration("sample.page", true);
        d.state_version = 2;
        w.set_declarations(vec![d]).unwrap();
        assert_eq!(w.view(&v.view_id).unwrap().status, "viewFailed");
        w.resources
            .insert(v.view_id.clone(), vec![json!({"id":"running"})]);
        assert!(w.reset_incompatible_state(&v.view_id).is_err());
        w.resources.clear();
        assert!(w.reset_incompatible_state(&v.view_id).unwrap());
        assert!(w.view(&v.view_id).unwrap().plugin_state.is_null());
        assert_eq!(w.view(&v.view_id).unwrap().state_version, 2);
    }
    #[test]
    fn missing_declarations_keep_views_and_catalog_adjusts_location() {
        let mut w = model();
        let a = open(&mut w, "a");
        w.set_declarations(vec![]).unwrap();
        assert_eq!(w.view(&a.view_id).unwrap().status, "viewFailed");
        let mut d = declaration("sample.page", true);
        d.supported_hosts = vec!["dock".into()];
        d.dock_edges = vec!["right".into()];
        d.default_location.host = "dock".into();
        d.default_location.edge = Some("right".into());
        w.set_declarations(vec![d]).unwrap();
        assert_eq!(w.view(&a.view_id).unwrap().placement, "right");
    }
    #[test]
    fn windows_have_independent_activation_and_batch_ownership() {
        let mut w = model();
        w.windows
            .insert("workspace-x".into(), WindowRecord::new("workspace-x"));
        let v = open(&mut w, "a");
        w.move_workspaces(&["a".into()], "workspace-x", 0, Some("a"))
            .unwrap();
        assert_eq!(w.windows["main"].selected_workspace().map(String::as_str), Some("b"));
        w.activate_workspace("b").unwrap();
        assert_eq!(w.snapshot("workspace-x")["activeWorkspaceId"], "a");
        assert_eq!(w.view_window(&v).unwrap(), "workspace-x");
        let before = w.snapshot("main");
        assert!(w
            .move_workspaces(&["b".into(), "missing".into()], "workspace-x", 0, None)
            .is_err());
        assert_eq!(w.snapshot("main"), before);
        w.move_workspaces(&["a".into()], "main", 1, None).unwrap();
        assert_eq!(w.windows["main"].workspace_ids, vec!["b", "a"]);
        assert!(w.reorder("main", vec!["a".into(), "a".into()]).is_err());
    }
}

#[cfg(test)]
mod connection_home_tests {
    use super::*;
    #[test]
    fn home_projects_no_active_workspace_without_discarding_resource_context(){
        let mut w=Workbench::default();w.open_workspace("server-a","main").unwrap();
        w.workspaces.get_mut("server-a").unwrap().focus_target=Some("terminal-a".into());
        w.workspaces.get_mut("server-a").unwrap().directory_source_view=Some("terminal-a".into());
        w.windows.get_mut("main").unwrap().focused_view_id=Some("terminal-a".into());
        w.windows.get_mut("main").unwrap().select_home();let snapshot=w.snapshot("main");
        assert_eq!(snapshot["activeWorkspaceId"],Value::Null);assert_eq!(snapshot["focusedViewId"],Value::Null);assert_eq!(snapshot["dockSlots"],json!([]));
        assert_eq!(w.workspaces["server-a"].directory_source_view.as_deref(),Some("terminal-a"));
        crate::workbench_contract::validate_kind("x-results","workbench.snapshot",&snapshot).unwrap();
        w.activate_workspace("server-a").unwrap();assert_eq!(w.snapshot("main")["focusedViewId"],"terminal-a");
        assert_eq!(w.windows["main"].workspace_ids,vec!["server-a"]);
    }
    #[test]
    fn home_stays_with_its_window_and_is_not_a_movable_workspace(){
        let mut w=Workbench::default();w.open_workspace("a","main").unwrap();w.windows.get_mut("main").unwrap().select_home();
        let generation=w.windows["main"].home_generation.clone();w.windows.insert("child".into(),WindowRecord::new("child"));
        w.move_workspaces(&["a".into()],"child",0,None).unwrap();assert_eq!(w.windows["main"].home_generation,generation);assert_eq!(w.snapshot("main")["activeWorkspaceId"],Value::Null);
        assert_eq!(w.snapshot("child")["activeWorkspaceId"],"a");assert!(!w.windows["child"].home_open);
        assert!(w.reorder("main",vec!["@home".into()]).is_err());
    }
}

//! Authoritative layout and view identities. Runtime handles and declaration
//! catalogs are never restored from persisted layout.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sodeck_plugin_manager::ViewDeclaration;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub view_id: String,
    pub view_type: String,
    pub workspace_id: String,
    pub server_id: String,
    pub title: String,
    pub status: String,
    pub placement: String,
    pub group: u8,
    pub mount_generation: u64,
    #[serde(default = "initial_state_version")]
    pub state_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>,
    #[serde(default)]
    pub active: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pty_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<u64>,
}
fn initial_state_version() -> u32 {
    1
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub split: String,
    pub sessions_side: String,
    pub sessions_visible: bool,
}
impl Default for Workspace {
    fn default() -> Self {
        Self {
            split: "none".into(),
            sessions_side: "left".into(),
            sessions_visible: true,
        }
    }
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workbench {
    pub views: Vec<View>,
    pub active_workspace_id: Option<String>,
    pub workspaces: BTreeMap<String, Workspace>,
    pub revision: u64,
    #[serde(skip)]
    declarations: BTreeMap<String, ViewDeclaration>,
}
impl Workbench {
    pub fn load(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let mut saved: Self =
            serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
                .map_err(|_| "工作台布局已损坏，请保留文件并恢复布局".to_string())?;
        if saved.views.len() > 100 || saved.workspaces.len() > 100 {
            return Err("布局规模无效".into());
        }
        let mut ids = BTreeSet::new();
        for view in &mut saved.views {
            if view.view_id.is_empty()
                || !ids.insert(view.view_id.clone())
                || placement_host(&view.placement).is_none()
                || view.group > 1
                || view.state_version == 0
            {
                return Err("布局视图或分组无效".into());
            }
            let had_resource = view.pty_id.is_some() || view.session_id.is_some();
            view.session_id = None;
            view.pty_id = None;
            view.generation = None;
            view.mount_generation = 1;
            view.status = if had_resource || ["running", "ended"].contains(&view.status.as_str()) {
                "ended"
            } else if view.status == "viewFailed" {
                "viewFailed"
            } else {
                "ready"
            }
            .into();
        }
        for workspace in saved.workspaces.values() {
            if !["none", "horizontal", "vertical"].contains(&workspace.split.as_str())
                || !["left", "right"].contains(&workspace.sessions_side.as_str())
            {
                return Err("布局配置无效".into());
            }
        }
        Ok(saved)
    }
    pub fn set_declarations(&mut self, declarations: Vec<ViewDeclaration>) -> Result<(), String> {
        let mut catalog = BTreeMap::new();
        for declaration in declarations {
            validate_declaration(&declaration)?;
            if catalog
                .insert(declaration.id.clone(), declaration)
                .is_some()
            {
                return Err("视图声明 ID 重复".into());
            }
        }
        self.declarations = catalog;
        let mut singletons = BTreeSet::new();
        for view in &mut self.views {
            let Some(declaration) = self.declarations.get(&view.view_type) else {
                view.status = "viewFailed".into();
                continue;
            };
            if view.state_version != declaration.state_version {
                view.status = "viewFailed".into();
                view.failure_reason = Some(format!(
                    "视图状态版本不兼容（保存为 {}，当前为 {}）。重新加载将清除旧展示状态并重建此视图。",
                    view.state_version, declaration.state_version
                ));
                continue;
            }
            if view.failure_reason.take().is_some() {
                view.status = if view.view_type == "terminal.session" {
                    "ended"
                } else {
                    "ready"
                }
                .into();
            }
            let scope = if declaration.scope == "global" {
                String::new()
            } else {
                view.workspace_id.clone()
            };
            if !declaration.multiple && !singletons.insert((view.view_type.clone(), scope)) {
                view.status = "viewFailed".into();
                continue;
            }
            if !supports(declaration, &view.placement) {
                view.placement = default_placement(declaration)?;
                view.group = 0;
            }
        }
        let ids = self.workspaces.keys().cloned().collect::<Vec<_>>();
        for id in ids {
            self.normalize(&id);
        }
        Ok(())
    }
    /// Explicit recovery may discard incompatible display state, but never a
    /// live resource. Schema acknowledgement is persisted by the caller.
    pub fn reset_incompatible_state(&mut self, view_id: &str) -> Result<bool, String> {
        let index = self
            .views
            .iter()
            .position(|view| view.view_id == view_id)
            .ok_or("视图已关闭")?;
        let view = &self.views[index];
        let declaration = self
            .declarations
            .get(&view.view_type)
            .ok_or("插件没有声明此视图")?;
        if view.state_version == declaration.state_version {
            return Ok(false);
        }
        if view.pty_id.is_some() || view.session_id.is_some() || view.generation.is_some() {
            return Err("视图仍有运行资源，不能重置展示状态版本".into());
        }
        let view = &mut self.views[index];
        view.state_version = declaration.state_version;
        view.failure_reason = None;
        self.revision += 1;
        Ok(true)
    }
    pub fn validate_placement(&self, view_type: &str, placement: &str) -> Result<(), String> {
        let declaration = self
            .declarations
            .get(view_type)
            .ok_or("插件没有声明此视图")?;
        if supports(declaration, placement) {
            Ok(())
        } else {
            Err("该视图不支持此展示位置".into())
        }
    }
    pub fn commit(&mut self, candidate: Self, path: &Path) -> Result<(), String> {
        candidate.save(path)?;
        *self = candidate;
        Ok(())
    }
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let mut persistent = self.clone();
        for view in &mut persistent.views {
            view.session_id = None;
            view.pty_id = None;
            view.generation = None;
        }
        let bytes = serde_json::to_vec_pretty(&persistent).map_err(|e| e.to_string())?;
        let temporary = path.with_extension("pending");
        use std::io::Write;
        let mut file = std::fs::File::create(&temporary).map_err(|e| e.to_string())?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
        drop(file);
        replace(&temporary, path)
    }
    pub fn snapshot(&self) -> Value {
        let workspace = self
            .active_workspace_id
            .as_ref()
            .and_then(|id| self.workspaces.get(id))
            .cloned()
            .unwrap_or_default();
        json!({"views":self.views,"activeWorkspaceId":self.active_workspace_id,"split":workspace.split,"sessionsSide":workspace.sessions_side,"sessionsVisible":workspace.sessions_visible,"revision":self.revision})
    }
    /// Generic view opening is driven entirely by the registered declaration.
    pub fn open_view(&mut self, view_type: &str, server_id: &str) -> Result<View, String> {
        let declaration = self
            .declarations
            .get(view_type)
            .cloned()
            .ok_or("插件没有声明此视图")?;
        let global = declaration.scope == "global";
        let workspace_id = if global { "@app" } else { server_id };
        if !global && server_id.is_empty() {
            return Err("服务器视图需要工作区".into());
        }
        if !declaration.multiple {
            if let Some(existing) = self
                .views
                .iter()
                .find(|v| v.view_type == view_type && (global || v.workspace_id == workspace_id))
                .cloned()
            {
                return self.activate(&existing.view_id);
            }
        }
        let placement = default_placement(&declaration)?;
        self.workspaces.entry(workspace_id.into()).or_default();
        self.active_workspace_id = Some(workspace_id.into());
        let view = View {
            view_id: Uuid::new_v4().to_string(),
            view_type: view_type.into(),
            workspace_id: workspace_id.into(),
            server_id: if global {
                String::new()
            } else {
                server_id.into()
            },
            title: declaration.title,
            status: "ready".into(),
            placement,
            group: 0,
            mount_generation: 1,
            state_version: declaration.state_version,
            failure_reason: None,
            active: true,
            session_id: None,
            pty_id: None,
            generation: None,
        };
        for current in &mut self.views {
            if same_container(current, &view) {
                current.active = false;
            }
        }
        self.views.push(view.clone());
        self.revision += 1;
        Ok(view)
    }
    pub fn activate_workspace(&mut self, server_id: &str) -> Result<View, String> {
        let default_side = self
            .declarations
            .get("terminal.sessions")
            .ok_or("插件没有声明会话列表")?
            .default_location
            .edge
            .as_deref()
            .filter(|s| ["left", "right"].contains(s))
            .unwrap_or("left")
            .to_string();
        self.workspaces
            .entry(server_id.into())
            .or_insert_with(|| Workspace {
                sessions_side: default_side,
                ..Workspace::default()
            });
        let existing = self
            .views
            .iter()
            .find(|v| v.workspace_id == server_id && v.view_type == "terminal.sessions")
            .cloned();
        self.active_workspace_id = Some(server_id.into());
        if let Some(view) = existing {
            self.revision += 1;
            return Ok(view);
        }
        let view = self.open_view("terminal.sessions", server_id)?;
        let preferred = self.workspaces[server_id].sessions_side.clone();
        if preferred != view.placement
            && self.validate_placement(&view.view_type, &preferred).is_ok()
        {
            return self.moved(&view.view_id, &preferred, 0, view.mount_generation);
        }
        Ok(view)
    }
    pub fn add_terminal(&mut self, server_id: &str, session: &Value) -> Result<View, String> {
        if self
            .views
            .iter()
            .filter(|v| v.view_type == "terminal.session")
            .count()
            >= 8
        {
            return Err("最多同时打开 8 个终端视图".into());
        }
        let declaration = self
            .declarations
            .get("terminal.session")
            .ok_or("插件没有声明终端视图")?;
        if !declaration.multiple
            && self.views.iter().any(|v| {
                v.view_type == "terminal.session"
                    && (declaration.scope == "global" || v.workspace_id == server_id)
                    && v.session_id.is_some()
            })
        {
            return Err("单实例终端视图已经打开".into());
        }
        let session_id = session["sessionId"]
            .as_str()
            .ok_or("插件未返回真实终端会话")?
            .to_owned();
        let pty_id = session["ptyId"]
            .as_str()
            .ok_or("插件未返回真实终端会话")?
            .to_owned();
        self.activate_workspace(server_id)?;
        let mut view = self.open_view("terminal.session", server_id)?;
        view.title = session["title"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or(view.title);
        view.status = "running".into();
        view.session_id = Some(session_id);
        view.pty_id = Some(pty_id);
        view.generation = session["generation"].as_u64();
        *self
            .views
            .iter_mut()
            .find(|v| v.view_id == view.view_id)
            .unwrap() = view.clone();
        Ok(view)
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
        self.active_workspace_id = Some(view.workspace_id.clone());
        for v in &mut self.views {
            if same_container(v, &view) {
                v.active = v.view_id == id;
            }
        }
        self.revision += 1;
        self.view(id)
    }
    pub fn close(&mut self, id: &str) -> Result<View, String> {
        let view = self.view(id)?;
        self.views.retain(|v| v.view_id != id);
        if view.view_type == "terminal.sessions" {
            if let Some(ws) = self.workspaces.get_mut(&view.workspace_id) {
                ws.sessions_visible = false;
            }
        }
        self.normalize(&view.workspace_id);
        self.revision += 1;
        Ok(view)
    }
    pub fn moved(
        &mut self,
        id: &str,
        placement: &str,
        group: u8,
        generation: u64,
    ) -> Result<View, String> {
        let old = self.view(id)?;
        self.validate_placement(&old.view_type, placement)?;
        if group > 1 || placement != "center" && group != 0 {
            return Err("只支持两个中央区域".into());
        }
        let mut moved = old.clone();
        moved.placement = placement.into();
        moved.group = group;
        moved.mount_generation = generation;
        moved.active = true;
        for view in &mut self.views {
            if same_container(view, &moved) {
                view.active = false;
            }
        }
        *self.views.iter_mut().find(|v| v.view_id == id).unwrap() = moved;
        if placement == "center" && group == 1 {
            let w = self
                .workspaces
                .get_mut(&old.workspace_id)
                .ok_or("工作区不存在")?;
            if w.split == "none" {
                w.split = "horizontal".into();
            }
        }
        if old.view_type == "terminal.sessions" {
            let w = self
                .workspaces
                .get_mut(&old.workspace_id)
                .ok_or("工作区不存在")?;
            w.sessions_side = placement.into();
            w.sessions_visible = true;
        }
        self.normalize(&old.workspace_id);
        self.revision += 1;
        self.view(id)
    }
    pub fn configure(&mut self, params: &Value) -> Result<(), String> {
        let id = self
            .active_workspace_id
            .clone()
            .ok_or("请先打开服务器工作区")?;
        // Validate all fields before mutating any part of the layout.
        let split = params["split"].as_str();
        if split.is_some_and(|s| !["none", "horizontal", "vertical"].contains(&s)) {
            return Err("分屏方向无效".into());
        }
        let side = params["sessionsSide"].as_str();
        if let Some(side) = side {
            if !["left", "right"].contains(&side) {
                return Err("会话列表只能停靠左侧或右侧".into());
            }
            self.validate_placement("terminal.sessions", side)?;
        }
        let w = self.workspaces.get_mut(&id).ok_or("工作区不存在")?;
        if let Some(split) = split {
            w.split = split.into();
            if split == "none" {
                for v in &mut self.views {
                    if v.workspace_id == id && v.placement == "center" {
                        v.group = 0;
                    }
                }
            }
        }
        if let Some(side) = side {
            w.sessions_side = side.into();
            for v in &mut self.views {
                if v.workspace_id == id && v.view_type == "terminal.sessions" {
                    v.placement = side.into();
                }
            }
        }
        if let Some(visible) = params["sessionsVisible"].as_bool() {
            w.sessions_visible = visible;
        }
        self.normalize_active(&id);
        self.revision += 1;
        Ok(())
    }
    fn normalize(&mut self, id: &str) {
        let has_second = self
            .views
            .iter()
            .any(|v| v.workspace_id == id && v.placement == "center" && v.group == 1);
        let has_first = self
            .views
            .iter()
            .any(|v| v.workspace_id == id && v.placement == "center" && v.group == 0);
        if !has_first || !has_second {
            for v in &mut self.views {
                if v.workspace_id == id && v.placement == "center" {
                    v.group = 0;
                }
            }
            if let Some(w) = self.workspaces.get_mut(id) {
                w.split = "none".into();
            }
        }
        self.normalize_active(id);
    }
    fn normalize_active(&mut self, id: &str) {
        let candidates = self
            .views
            .iter()
            .filter(|v| v.workspace_id == id)
            .cloned()
            .collect::<Vec<_>>();
        let mut visited = BTreeSet::new();
        for candidate in &candidates {
            let key = if candidate.placement == "detached" {
                candidate.view_id.clone()
            } else {
                format!("{}:{}", candidate.placement, candidate.group)
            };
            if !visited.insert(key) {
                continue;
            }
            let chosen = candidates
                .iter()
                .find(|v| same_container(v, candidate) && v.active)
                .unwrap_or(candidate)
                .view_id
                .clone();
            for view in &mut self.views {
                if same_container(view, candidate) {
                    view.active = view.view_id == chosen;
                }
            }
        }
    }
}
fn same_container(a: &View, b: &View) -> bool {
    a.workspace_id == b.workspace_id
        && a.placement == b.placement
        && a.group == b.group
        && (a.placement != "detached" || a.view_id == b.view_id)
}
fn placement_host(placement: &str) -> Option<&str> {
    match placement {
        "center" => Some("tab"),
        "detached" => Some("window"),
        "left" | "right" | "bottom" => Some("dock"),
        _ => None,
    }
}
fn supports(declaration: &ViewDeclaration, placement: &str) -> bool {
    let Some(host) = placement_host(placement) else {
        return false;
    };
    declaration.supported_hosts.iter().any(|h| h == host)
        && (host != "dock" || declaration.dock_edges.iter().any(|edge| edge == placement))
}
fn default_placement(declaration: &ViewDeclaration) -> Result<String, String> {
    let placement = match declaration.default_location.host.as_str() {
        "tab" => "center",
        "window" => "detached",
        "dock" => declaration
            .default_location
            .edge
            .as_deref()
            .ok_or("默认停靠位置缺失")?,
        _ => return Err("默认承载形式无效".into()),
    };
    if !supports(declaration, placement) {
        return Err("默认展示位置不在视图支持范围内".into());
    }
    Ok(placement.into())
}
fn validate_declaration(declaration: &ViewDeclaration) -> Result<(), String> {
    if declaration.id.is_empty()
        || declaration.title.trim().is_empty()
        || !["server", "global"].contains(&declaration.scope.as_str())
        || declaration.state_version == 0
        || declaration.supported_hosts.is_empty()
        || declaration
            .supported_hosts
            .iter()
            .any(|s| !["tab", "dock", "window"].contains(&s.as_str()))
        || declaration
            .dock_edges
            .iter()
            .any(|s| !["left", "right", "bottom"].contains(&s.as_str()))
    {
        return Err("视图声明无效".into());
    }
    default_placement(declaration).map(|_| ())
}
fn replace(source: &Path, target: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::core::PCWSTR;
        let source: Vec<_> = source.as_os_str().encode_wide().chain(Some(0)).collect();
        let target: Vec<_> = target.as_os_str().encode_wide().chain(Some(0)).collect();
        unsafe {
            windows::Win32::Storage::FileSystem::MoveFileExW(
                PCWSTR(source.as_ptr()),
                PCWSTR(target.as_ptr()),
                windows::Win32::Storage::FileSystem::MOVEFILE_REPLACE_EXISTING
                    | windows::Win32::Storage::FileSystem::MOVEFILE_WRITE_THROUGH,
            )
            .map_err(|e| e.to_string())
        }
    }
    #[cfg(not(windows))]
    {
        std::fs::rename(source, target).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sodeck_plugin_manager::{Location, Manifest};
    fn session(n: u8) -> Value {
        json!({"sessionId":format!("s{n}"),"ptyId":format!("p{n}"),"generation":1})
    }
    fn workbench() -> Workbench {
        let manifest: Manifest =
            serde_json::from_str(include_str!("../../../../plugins/terminal/manifest.json"))
                .unwrap();
        let mut w = Workbench::default();
        w.set_declarations(manifest.contributes.views).unwrap();
        w
    }
    fn declaration(
        id: &str,
        hosts: &[&str],
        edges: &[&str],
        host: &str,
        edge: Option<&str>,
        multiple: bool,
    ) -> ViewDeclaration {
        ViewDeclaration {
            id: id.into(),
            title: format!("{id} title"),
            scope: "server".into(),
            supported_hosts: hosts.iter().map(|v| v.to_string()).collect(),
            dock_edges: edges.iter().map(|v| v.to_string()).collect(),
            default_location: Location {
                host: host.into(),
                edge: edge.map(str::to_string),
            },
            multiple,
            state_version: 1,
            min_width: None,
            min_height: None,
        }
    }
    #[test]
    fn legacy_view_schema_is_preserved_and_requires_explicit_recovery() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("workbench.json");
        let mut original = workbench();
        let view = original.activate_workspace("server").unwrap();
        original.save(&path).unwrap();
        let mut legacy: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        for item in legacy["views"].as_array_mut().unwrap() {
            item.as_object_mut().unwrap().remove("stateVersion");
        }
        std::fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
        let mut restored = Workbench::load(&path).unwrap();
        assert_eq!(restored.view(&view.view_id).unwrap().state_version, 1);
        let mut declarations: Vec<_> = original.declarations.values().cloned().collect();
        for declaration in &mut declarations {
            declaration.state_version = 2;
        }
        restored.set_declarations(declarations.clone()).unwrap();
        let incompatible = restored.view(&view.view_id).unwrap();
        assert_eq!(incompatible.status, "viewFailed");
        assert_eq!(incompatible.state_version, 1);
        assert!(incompatible
            .failure_reason
            .as_ref()
            .unwrap()
            .contains("版本不兼容"));
        restored.save(&path).unwrap();
        let mut again = Workbench::load(&path).unwrap();
        again.set_declarations(declarations.clone()).unwrap();
        assert_eq!(again.view(&view.view_id).unwrap().status, "viewFailed");
        assert!(again.reset_incompatible_state(&view.view_id).unwrap());
        let reset = again.view(&view.view_id).unwrap();
        assert_eq!(reset.state_version, 2);
        assert!(reset.failure_reason.is_none());
        assert!(reset.pty_id.is_none() && reset.session_id.is_none());
        assert!(!again.reset_incompatible_state(&view.view_id).unwrap());
        again.save(&path).unwrap();
        assert_eq!(
            Workbench::load(&path)
                .unwrap()
                .view(&view.view_id)
                .unwrap()
                .state_version,
            2
        );
        let new_view = again.activate_workspace("new-server").unwrap();
        assert_eq!(new_view.state_version, 2);
    }
    #[test]
    fn schema_reset_cannot_discard_running_resources() {
        let mut w = workbench();
        let running = w.add_terminal("server", &session(1)).unwrap();
        let mut declarations: Vec<_> = w.declarations.values().cloned().collect();
        for declaration in &mut declarations {
            declaration.state_version = 2;
        }
        w.set_declarations(declarations).unwrap();
        assert!(w.reset_incompatible_state(&running.view_id).is_err());
        let view = w.view(&running.view_id).unwrap();
        assert_eq!(view.state_version, 1);
        assert_eq!(view.pty_id, running.pty_id);
        assert_eq!(view.session_id, running.session_id);
    }
    #[test]
    fn layout_does_not_persist_runtime_handles_or_catalog() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("layout.json");
        let mut w = workbench();
        w.add_terminal("server", &session(1)).unwrap();
        w.save(&p).unwrap();
        let bytes = std::fs::read_to_string(&p).unwrap();
        assert!(!bytes.contains("declarations"));
        assert!(!bytes.contains("ptyId"));
        let restored = Workbench::load(&p).unwrap();
        let v = restored
            .views
            .iter()
            .find(|v| v.view_type == "terminal.session")
            .unwrap();
        assert_eq!(v.status, "ended");
        assert!(v.pty_id.is_none());
        assert!(restored.declarations.is_empty());
    }
    #[test]
    fn failed_layout_commit_preserves_memory_and_disk_then_allows_retry() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("workbench.json");
        let mut current = workbench();
        let terminal = current.add_terminal("server", &session(1)).unwrap();
        current.save(&path).unwrap();
        let before = current.snapshot();
        let disk_before = std::fs::read(&path).unwrap();
        let mut candidate = current.clone();
        candidate.configure(&json!({"split":"vertical"})).unwrap();
        candidate
            .moved(&terminal.view_id, "bottom", 0, terminal.mount_generation)
            .unwrap();
        std::fs::create_dir(path.with_extension("pending")).unwrap();
        assert!(current.commit(candidate.clone(), &path).is_err());
        assert_eq!(current.snapshot(), before);
        assert_eq!(std::fs::read(&path).unwrap(), disk_before);
        std::fs::remove_dir(path.with_extension("pending")).unwrap();
        current.commit(candidate, &path).unwrap();
        assert_eq!(current.view(&terminal.view_id).unwrap().placement, "bottom");
        assert_eq!(
            Workbench::load(&path)
                .unwrap()
                .view(&terminal.view_id)
                .unwrap()
                .placement,
            "bottom"
        );
    }
    #[test]
    fn single_list_and_multiple_terminal_views() {
        let mut w = workbench();
        let a = w.activate_workspace("a").unwrap();
        assert_eq!(a.view_id, w.activate_workspace("a").unwrap().view_id);
        let x = w.add_terminal("a", &session(1)).unwrap();
        let y = w.add_terminal("a", &session(2)).unwrap();
        assert_ne!(x.view_id, y.view_id);
        assert!(w.moved(&a.view_id, "detached", 0, 2).is_err());
    }
    #[test]
    fn splitting_and_close_never_change_another_workspace() {
        let mut w = workbench();
        let a = w.add_terminal("a", &session(1)).unwrap();
        let b = w.add_terminal("a", &session(2)).unwrap();
        let c = w.add_terminal("b", &session(3)).unwrap();
        w.moved(&b.view_id, "center", 1, 2).unwrap();
        assert_eq!(w.workspaces["a"].split, "horizontal");
        w.close(&a.view_id).unwrap();
        assert_eq!(w.view(&b.view_id).unwrap().group, 0);
        assert_eq!(w.workspaces["a"].split, "none");
        assert_eq!(w.view(&c.view_id).unwrap().pty_id, Some("p3".into()));
    }
    #[test]
    fn arbitrary_declarations_own_default_title_and_supported_positions() {
        let mut w = Workbench::default();
        w.set_declarations(vec![declaration(
            "custom.inspector",
            &["dock", "window"],
            &["right"],
            "dock",
            Some("right"),
            false,
        )])
        .unwrap();
        let view = w.open_view("custom.inspector", "a").unwrap();
        assert_eq!(view.title, "custom.inspector title");
        assert_eq!(view.placement, "right");
        assert!(w.moved(&view.view_id, "center", 0, 2).is_err());
        assert!(w.moved(&view.view_id, "left", 0, 2).is_err());
        assert_eq!(w.view(&view.view_id).unwrap().placement, "right");
        assert!(w.moved(&view.view_id, "detached", 0, 2).is_ok());
        assert!(w.open_view("unknown", "a").is_err());
    }
    #[test]
    fn declaration_defaults_and_singleton_scope_are_generic() {
        let mut global = declaration("custom.global", &["window"], &[], "window", None, false);
        global.scope = "global".into();
        let mut w = Workbench::default();
        w.set_declarations(vec![
            global,
            declaration("custom.single", &["tab"], &[], "tab", None, false),
            declaration("custom.multi", &["tab"], &[], "tab", None, true),
        ])
        .unwrap();
        let a = w.open_view("custom.single", "a").unwrap();
        assert_eq!(a.placement, "center");
        assert_eq!(
            a.view_id,
            w.open_view("custom.single", "a").unwrap().view_id
        );
        assert_ne!(
            a.view_id,
            w.open_view("custom.single", "b").unwrap().view_id
        );
        let x = w.open_view("custom.global", "a").unwrap();
        assert_eq!(x.placement, "detached");
        assert_eq!(x.workspace_id, "@app");
        assert_eq!(
            x.view_id,
            w.open_view("custom.global", "b").unwrap().view_id
        );
        let m = w.open_view("custom.multi", "a").unwrap();
        assert_ne!(m.view_id, w.open_view("custom.multi", "a").unwrap().view_id);
    }
    #[test]
    fn invalid_catalog_is_rejected_without_replacing_previous_declarations() {
        let valid = declaration(
            "custom.valid",
            &["dock"],
            &["left"],
            "dock",
            Some("left"),
            false,
        );
        let mut w = Workbench::default();
        w.set_declarations(vec![valid.clone()]).unwrap();
        let invalid = declaration(
            "custom.invalid",
            &["tab"],
            &["left"],
            "dock",
            Some("left"),
            false,
        );
        assert!(w.set_declarations(vec![invalid]).is_err());
        assert!(w.validate_placement("custom.valid", "left").is_ok());
        assert!(w.set_declarations(vec![valid.clone(), valid]).is_err());
    }
    #[test]
    fn restored_locations_follow_new_catalog_and_missing_views_remain_visible() {
        let mut w = Workbench::default();
        w.set_declarations(vec![
            declaration(
                "custom.a",
                &["dock"],
                &["left"],
                "dock",
                Some("left"),
                false,
            ),
            declaration("custom.b", &["tab"], &[], "tab", None, false),
        ])
        .unwrap();
        let a = w.open_view("custom.a", "a").unwrap();
        let b = w.open_view("custom.b", "a").unwrap();
        w.set_declarations(vec![declaration(
            "custom.a",
            &["dock"],
            &["right"],
            "dock",
            Some("right"),
            false,
        )])
        .unwrap();
        assert_eq!(w.view(&a.view_id).unwrap().placement, "right");
        assert_eq!(w.view(&b.view_id).unwrap().status, "viewFailed");
        assert_eq!(w.views.len(), 2);
    }
    #[test]
    fn merging_groups_and_moving_active_tabs_selects_exactly_one_page_per_container() {
        let mut w = workbench();
        let a = w.add_terminal("a", &session(1)).unwrap();
        let b = w.add_terminal("a", &session(2)).unwrap();
        let c = w.add_terminal("a", &session(3)).unwrap();
        w.moved(&c.view_id, "center", 1, 2).unwrap();
        assert_eq!(
            w.views
                .iter()
                .filter(|v| v.placement == "center" && v.active)
                .count(),
            2
        );
        w.configure(&json!({"split":"none"})).unwrap();
        assert_eq!(
            w.views
                .iter()
                .filter(|v| v.placement == "center" && v.active)
                .count(),
            1
        );
        w.activate(&b.view_id).unwrap();
        w.moved(&b.view_id, "bottom", 0, 2).unwrap();
        assert_eq!(
            w.views
                .iter()
                .filter(|v| v.placement == "center" && v.active)
                .count(),
            1
        );
        assert!(w.view(&b.view_id).unwrap().active);
        w.close(&a.view_id).unwrap();
        assert_eq!(
            w.views
                .iter()
                .filter(|v| v.placement == "center" && v.active)
                .count(),
            1
        );
    }
    #[test]
    fn configuring_an_unsupported_side_is_atomic() {
        let mut w = workbench();
        let a = w.add_terminal("a", &session(1)).unwrap();
        let b = w.add_terminal("a", &session(2)).unwrap();
        w.moved(&b.view_id, "center", 1, 2).unwrap();
        assert!(w
            .configure(&json!({"split":"none","sessionsSide":"bottom"}))
            .is_err());
        assert_eq!(w.workspaces["a"].split, "horizontal");
        assert_eq!(w.view(&a.view_id).unwrap().group, 0);
        assert_eq!(w.view(&b.view_id).unwrap().group, 1);
    }
    #[test]
    fn repeated_restarts_preserve_ended_and_failed_placeholders_without_runtime_handles() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("layout.json");
        let declarations = vec![
            declaration("custom.resource", &["tab"], &[], "tab", None, false),
            declaration("custom.failed", &["tab"], &[], "tab", None, false),
        ];
        let mut current = Workbench::default();
        current.set_declarations(declarations.clone()).unwrap();
        let resource = current.open_view("custom.resource", "a").unwrap();
        let failed = current.open_view("custom.failed", "a").unwrap();
        let running = current
            .views
            .iter_mut()
            .find(|view| view.view_id == resource.view_id)
            .unwrap();
        running.status = "running".into();
        running.pty_id = Some("old-pty".into());
        running.session_id = Some("old-session".into());
        running.generation = Some(8);
        current
            .views
            .iter_mut()
            .find(|view| view.view_id == failed.view_id)
            .unwrap()
            .status = "viewFailed".into();
        for _ in 0..3 {
            current.save(&path).unwrap();
            current = Workbench::load(&path).unwrap();
            current.set_declarations(declarations.clone()).unwrap();
            let restored = current.view(&resource.view_id).unwrap();
            assert_eq!(restored.status, "ended");
            assert!(
                restored.pty_id.is_none()
                    && restored.session_id.is_none()
                    && restored.generation.is_none()
            );
            assert_eq!(current.view(&failed.view_id).unwrap().status, "viewFailed");
        }
    }
    #[test]
    fn missing_declarations_cannot_open_terminal_convenience_routes() {
        let mut w = Workbench::default();
        assert!(w.activate_workspace("a").is_err());
        assert!(w.add_terminal("a", &session(1)).is_err());
        assert!(w.views.is_empty());
    }
}

//! Core-owned view identities and layout. Plugin code and business state stay outside the core.
use crate::{model::PluginStatus, storage, workspaces::Context, AppCore};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    path::{Component, Path, PathBuf},
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Declaration {
    pub api_version: String,
    pub entry: String,
    pub views: Vec<Definition>,
    #[serde(default)]
    pub actions: Vec<ActionDefinition>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionDefinition {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub default_create: bool,
    pub data_version: u32,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisteredAction {
    #[serde(flatten)]
    pub definition: ActionDefinition,
    pub key: String,
    pub plugin_id: String,
    pub plugin_name: String,
    pub plugin_instance_id: String,
    pub available: bool,
    pub reason: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Definition {
    pub id: String,
    pub title: String,
    pub icon: String,
    pub container: String,
    pub instance_policy: String,
    #[serde(default = "left")]
    pub default_side: String,
    #[serde(default)]
    pub default_pinned: bool,
    #[serde(default = "none")]
    pub open_on_workspace: String,
    #[serde(default)]
    pub startup_order: u32,
    #[serde(default)]
    pub create_action: Option<String>,
    #[serde(default)]
    pub locate_entry: bool,
    #[serde(default)]
    pub background_on_workspace: bool,
    #[serde(default)]
    pub location_provider: Option<String>,
}
fn left() -> String {
    "left".into()
}
fn none() -> String {
    "none".into()
}
pub fn resource(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let p = Path::new(relative);
    if relative.is_empty()
        || relative.contains(['\\', ':', '%'])
        || p.components().any(|v| !matches!(v, Component::Normal(_)))
    {
        return Err("界面资源必须是插件目录内的相对路径".into());
    }
    let path = root
        .join(p)
        .canonicalize()
        .map_err(|e| format!("界面资源不存在：{e}"))?;
    if !path.starts_with(root) || !path.is_file() {
        return Err("界面资源必须位于插件包内".into());
    }
    Ok(path)
}
impl Declaration {
    pub fn validate(&self, root: &Path) -> Result<(), String> {
        if !["1", "2"].contains(&self.api_version.as_str())
            || self.views.is_empty()
            || self.views.len() > 64
        {
            return Err("插件 UI API 或视图数量无效".into());
        }
        resource(root, &self.entry)?;
        let mut actions = HashSet::new();
        if self.actions.len() > 32
            || self.actions.iter().filter(|a| a.default_create).count() > 1
            || self.actions.iter().any(|a| {
                a.id.is_empty()
                    || a.id.len() > 80
                    || !a
                        .id
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
                    || !actions.insert(&a.id)
                    || a.title.trim().is_empty()
                    || a.title.len() > 256
                    || a.data_version == 0
            })
        {
            return Err("插件 UI 行为声明无效或重复".into());
        }
        let mut ids = HashSet::new();
        for d in &self.views {
            if d.id.is_empty()
                || d.id.len() > 80
                || !d
                    .id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
                || !ids.insert(&d.id)
                || d.title.trim().is_empty()
                || d.title.len() > 256
                || d.icon.len() > 80
                || !["sidebar", "editor", "dialog"].contains(&d.container.as_str())
                || !["perWorkspace", "perResource", "multiple"]
                    .contains(&d.instance_policy.as_str())
                || !["left", "right"].contains(&d.default_side.as_str())
                || !["none", "reveal", "createOne"].contains(&d.open_on_workspace.as_str())
                || (["sidebar", "dialog"].contains(&d.container.as_str())
                    && d.instance_policy != "perWorkspace")
                || (d.container == "dialog"
                    && (d.open_on_workspace != "none"
                        || d.default_pinned
                        || d.locate_entry
                        || d.create_action.is_some()))
                || (d.instance_policy == "perResource" && d.open_on_workspace != "none")
                || (d.locate_entry && d.container != "editor")
                || (d.background_on_workspace
                    && (d.container != "sidebar" || d.instance_policy != "perWorkspace"))
                || d.create_action
                    .as_ref()
                    .is_some_and(|s| s.trim().is_empty() || s.len() > 256)
            {
                return Err("插件 UI 视图声明无效或重复".into());
            }
        }
        Ok(())
    }
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Registered {
    #[serde(flatten)]
    pub definition: Definition,
    pub key: String,
    pub plugin_id: String,
    pub plugin_name: String,
    pub plugin_instance_id: String,
    pub entry: String,
    pub available: bool,
    pub ui_api_version: String,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preference {
    pub side: String,
    pub pinned: bool,
    pub startup: String,
    pub order: u32,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Preferences {
    schema_version: u32,
    definitions: BTreeMap<String, Preference>,
    #[serde(default)]
    default_create_action: Option<String>,
    #[serde(default = "default_width")]
    left_width: u16,
    #[serde(default = "default_width")]
    right_width: u16,
}
fn default_width() -> u16 {
    280
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            schema_version: 1,
            definitions: BTreeMap::new(),
            default_create_action: None,
            left_width: default_width(),
            right_width: default_width(),
        }
    }
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub id: String,
    #[serde(skip)]
    pub(crate) calls: std::sync::Arc<crate::resource_tasks::Tasks>,
    #[serde(skip)]
    pub(crate) creation: std::sync::Arc<std::sync::Mutex<Option<Result<(), String>>>>,
    pub definition_key: String,
    pub plugin_id: String,
    pub plugin_instance_id: String,
    pub context: Context,
    pub resource_key: Option<String>,
    pub parent_view_id: Option<String>,
    pub params: Value,
    pub title: String,
    pub dirty: bool,
    pub busy: bool,
    pub error: Option<String>,
    pub last_used: u64,
    pub ui_requested: bool,
}
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceUi {
    pub views: Vec<View>,
    pub preferences: BTreeMap<String, Preference>,
    pub hosts: BTreeMap<String, crate::workbench_layout::Host>,
    pub focused_view_id: Option<String>,
    pub focused_window_id: Option<String>,
    pub last_location_view_id: Option<String>,
    pub return_locations: BTreeMap<String, crate::workbench_move::ReturnLocation>,
    pub group_return_locations: BTreeMap<String, crate::workbench_move::ReturnLocation>,
}
impl WorkspaceUi {
    pub fn view_window(&self, view: &str) -> Option<String> {
        self.hosts
            .iter()
            .find(|(_, h)| h.contains(view))
            .map(|(window, _)| window.clone())
    }
    pub fn host(&self, window: &str) -> Result<&crate::workbench_layout::Host, String> {
        self.hosts
            .get(window)
            .ok_or_else(|| "工作台已移动或关闭".into())
    }
    pub fn host_mut(&mut self, window: &str) -> Result<&mut crate::workbench_layout::Host, String> {
        self.hosts
            .get_mut(window)
            .ok_or_else(|| "工作台已移动或关闭".into())
    }
    pub fn projection(&self, window: &str) -> Value {
        let host = self.hosts.get(window);
        let ids = host.map(|h| h.view_ids()).unwrap_or_default();
        json!({"views":self.views.iter().filter(|v| ids.contains(&v.id) || !v.ui_requested).collect::<Vec<_>>(),
          "returnLocations":self.return_locations,"groupReturnLocations":self.group_return_locations,
          "preferences":self.preferences,"host":host,
          "left":host.and_then(|h| h.left.active_view_id.clone()),
          "right":host.and_then(|h| h.right.active_view_id.clone()),
          "editor":host.and_then(|h| h.active_view()),
          "otherViews":self.views.iter().filter_map(|v|self.view_window(&v.id).filter(|w|w!=window).map(|w|json!({"id":v.id,"definitionKey":v.definition_key,"title":v.title,"windowId":w}))).collect::<Vec<_>>(),"focusedViewId":self.focused_view_id,"focusedWindowId":self.focused_window_id})
    }
}
pub(crate) struct Surface {
    pub(crate) window: String,
    pub(crate) view: String,
    pub(crate) ready: bool,
    pub(crate) phase: SurfacePhase,
}
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum SurfacePhase {
    Active,
    Candidate,
    Frozen,
}
pub struct Runtime {
    root: PathBuf,
    pub(crate) memory: crate::workbench_memory::Memory,
    pub(crate) restoring_layout: bool,
    preferences: Preferences,
    error: Option<String>,
    pub definitions: BTreeMap<String, Registered>,
    pub actions: BTreeMap<String, RegisteredAction>,
    action_requests: HashMap<String, actions::ActionRequest>,
    open_requests: HashMap<String, actions::OpenRequest>,
    pub workspaces: HashMap<String, WorkspaceUi>,
    pub(crate) surfaces: HashMap<String, Surface>,
    pub(crate) lifecycle: crate::lifecycle::Lifecycle,
    requests: HashMap<String, String>,
    pub(crate) clock: u64,
    pub(crate) startup_loading: bool,
    pub(crate) move_receipts: HashMap<String, crate::workbench_move::MoveReceipt>,
    pub(crate) tool_receipts: HashMap<String, tools::ToolReceipt>,
    pub(crate) window_returns: HashMap<String, crate::workbench_move::WindowReturn>,
    pub(crate) transfers: HashMap<String, crate::workbench_move::Transfer>,
}
mod actions;
mod background;
mod close;
mod close_poll;
mod host;
mod layout;
mod tools;
pub(crate) use background::start_background;

#[tauri::command]
pub async fn component_ui(
    window: tauri::WebviewWindow,
    desktop: tauri::State<'_, crate::windows::DesktopWindows>,
    operation: String,
    args: Value,
) -> Result<Value, String> {
    let core = desktop.core.clone();
    let window = window.label().to_owned();
    tauri::async_runtime::spawn_blocking(move || dispatch(&core, &window, &operation, args))
        .await
        .map_err(|e| e.to_string())?
}
pub fn dispatch(
    core: &std::sync::Arc<AppCore>,
    window: &str,
    operation: &str,
    args: Value,
) -> Result<Value, String> {
    let text = |key: &str| args[key].as_str().ok_or_else(|| format!("缺少 {key}"));
    match operation {
        "snapshot" => core.ui_snapshot(window),
        "invokeAction" => core.ui_invoke_action(window, &args),
        "actionStatus" => core.ui_action_status(window, &args),
        "action" => core.ui_action(
            window,
            &serde_json::from_value(args["context"].clone()).map_err(|e| e.to_string())?,
            args["action"].clone(),
        ),
        "attach" => core.ui_attach(window, text("viewId")?),
        "ready" => core
            .ui_surface_ready(window, text("surfaceId")?)
            .map(|_| Value::Null),
        "moveStatus" => core.workbench_move_status(window, text("id")?),
        "retryLayoutSave" => core
            .save_workbench_layout()
            .map(|saved| json!({"persistence":if saved {"saved"} else {"pending"}})),
        "transferDrained" => core
            .transfer_drained(window, text("id")?)
            .map(|_| Value::Null),
        "detach" => {
            core.ui_detach(window, text("surfaceId")?);
            Ok(Value::Null)
        }
        "event" => core.ui_event(window, text("surfaceId")?, args["event"].clone()),
        "state" => Ok(
            match core.state_surface_request(window, text("surfaceId")?, &args["input"]) {
                Ok(result) => json!({"result":result}),
                Err(error) => json!({"error":error}),
            },
        ),
        #[cfg(feature = "test-fixtures")]
        "prepareClose" => core.ui_close_prepare(
            window,
            serde_json::from_value(args["workspaceIds"].clone()).map_err(|e| e.to_string())?,
            args["viewId"].as_str().map(str::to_owned),
        ),
        #[cfg(feature = "test-fixtures")]
        "pollClose" => core.ui_close_poll(window, text("token")?, args["save"] == true),
        #[cfg(feature = "test-fixtures")]
        "finishClose" => core
            .component_close_owner(window, text("token")?)
            .and_then(|_| {
                core.ui_close_finish(
                    text("token")?,
                    serde_json::from_value(args["views"].clone()).map_err(|e| e.to_string())?,
                    text("decision")?,
                )
            })
            .map(|_| Value::Null),
        #[cfg(feature = "test-fixtures")]
        "allWorkspaces" if window == "main" => {
            Ok(json!(core.ui_state().workspaces.keys().collect::<Vec<_>>()))
        }
        _ => Err("未知组件界面操作".into()),
    }
}

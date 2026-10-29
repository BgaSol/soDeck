#[path = "settings_window.rs"]
mod settings_window;
#[path = "settings_exit.rs"]
mod settings_exit;
#[path = "connection_home.rs"]
mod connection_home;
#[path = "window_close_review.rs"]
mod window_close_review;
#[path = "plugin_preferences.rs"]
mod plugin_preferences;
#[path = "settings/mod.rs"]
mod settings;
#[path = "plugin_dialogs.rs"]
mod plugin_dialogs;
#[path = "surface_endpoint.rs"]
mod surface_endpoint;
#[path = "application_menu.rs"]
mod application_menu;
#[path = "plugin_registry.rs"]
mod plugin_registry;
#[path = "plugin_management.rs"]
mod plugin_management;
#[path = "updates/mod.rs"]
mod updates;
#[path = "communication_services.rs"]
mod communication_services;
#[path = "observation_services.rs"]
mod observation_services;
#[path = "monitor_window.rs"]
mod monitor_window;
#[path = "appearance_services.rs"]
mod appearance_services;
#[path = "directory_context.rs"]
mod directory_context;
#[path = "plugin_presentation.rs"]
mod plugin_presentation;
#[path = "presentation_services.rs"]
mod presentation_services;
#[path = "workbench_services.rs"]
mod services;
#[path = "window_services.rs"]
mod window_services;
#[path = "workspace_move.rs"]
mod workspace_moves;
use crate::{
    native::{Assets, Bounds, NativeViews},
    view_operations::ViewOperations,
    workbench::{View, Workbench},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sodeck_core::{Caller, Core, CoreError};
use sodeck_plugin_host::{PluginProcess, RpcError};
use sodeck_plugin_manager::{InstalledPlugin, PluginManager, TrustedKeys};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering},
        mpsc, Arc, Condvar, Mutex,
    },
    time::{Duration, Instant},
};
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
use uuid::Uuid;

type Result<T> = std::result::Result<T, CoreError>;
fn err(code: &str, message: impl Into<String>) -> CoreError {
    CoreError::new(code, message)
}
fn native_error(message: String) -> CoreError {
    err("NATIVE_ERROR", message)
}

fn restore_main_window<W>(
    shutdown_lock: &Mutex<()>,
    stopping: &AtomicBool,
    find_window: impl FnOnce() -> Option<W>,
    restore: impl FnOnce(W) -> Result<()>,
) -> Result<()> {
    // Call from a worker: native window calls must be able to use the event loop.
    let _shutdown = shutdown_lock.lock().unwrap();
    if stopping.load(Ordering::SeqCst) {
        return Err(err("APP_STOPPING", "应用正在退出"));
    }
    let window = find_window().ok_or_else(|| err("WINDOW_CLOSED", "主窗口不可用"))?;
    restore(window)
}

fn show_main_from_tray(app: &tauri::AppHandle) {
    let state = app.state::<Arc<State>>().inner().clone();
    if state.stopping.load(Ordering::SeqCst) {
        return;
    }
    // Dispatch can write audit records and wait for shutdown. Keep it off the
    // native event loop, which must remain available for window operations.
    tauri::async_runtime::spawn_blocking(move || {
        let _ = state.dispatch(&Caller::shell(), None, "app.show", json!({}));
    });
}

fn required<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value[key]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| err("INVALID_PARAMS", format!("缺少 {key}")))
}

#[cfg(test)]
mod tray_tests {
    use super::*;

    #[test]
    fn missing_main_and_stopping_never_run_window_operations() {
        let gate = Mutex::new(());
        let stopping = AtomicBool::new(false);
        let missing = restore_main_window(&gate, &stopping, || None::<()>, |_| panic!("missing window"));
        assert_eq!(missing.unwrap_err().code, "WINDOW_CLOSED");
        stopping.store(true, Ordering::SeqCst);
        let stopped = restore_main_window(&gate, &stopping, || -> Option<()> { panic!("lookup after shutdown") }, |_| Ok(()));
        assert_eq!(stopped.unwrap_err().code, "APP_STOPPING");
    }

    #[test]
    fn partial_restore_failure_is_returned_without_retry_or_rollback() {
        let visible = AtomicBool::new(false);
        let attempts = AtomicUsize::new(0);
        let result = restore_main_window(&Mutex::new(()), &AtomicBool::new(false), || Some(()), |_| {
            attempts.fetch_add(1, Ordering::SeqCst);
            visible.store(true, Ordering::SeqCst);
            Err(native_error("focus failed".into()))
        });
        assert_eq!(result.unwrap_err().message, "focus failed");
        assert!(visible.load(Ordering::SeqCst));
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn shutdown_waits_for_restore_and_later_restore_cannot_revive_window() {
        let gate = Mutex::new(());
        let stopping = AtomicBool::new(false);
        let visible = AtomicBool::new(false);
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (stopped_tx, stopped_rx) = mpsc::channel();
        std::thread::scope(|scope| {
            scope.spawn(|| restore_main_window(&gate, &stopping, || Some(()), move |_| {
                entered_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(2)).unwrap();
                Ok(())
            }).unwrap());
            entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            scope.spawn(|| {
                let _guard = gate.lock().unwrap();
                stopping.store(true, Ordering::SeqCst);
                visible.store(false, Ordering::SeqCst);
                stopped_tx.send(()).unwrap();
            });
            assert!(stopped_rx.recv_timeout(Duration::from_millis(50)).is_err());
            release_tx.send(()).unwrap();
            stopped_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            let late = restore_main_window(&gate, &stopping, || Some(()), |_| {
                visible.store(true, Ordering::SeqCst);
                Ok(())
            });
            assert_eq!(late.unwrap_err().code, "APP_STOPPING");
            assert!(!visible.load(Ordering::SeqCst));
        });
    }
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    api_version: u32,
    request_id: String,
    method: String,
    #[serde(default)]
    params: Value,
    timeout_ms: Option<u64>,
    #[serde(default)]
    metadata: Option<Value>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Response {
    request_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<CoreError>,
}
impl Response {
    fn from(id: String, result: Result<Value>) -> Self {
        match result {
            Ok(result) => Self {
                request_id: id,
                result: Some(result),
                error: None,
            },
            Err(error) => Self {
                request_id: id,
                result: None,
                error: Some(error),
            },
        }
    }
}

struct Loaded {
    preferences: Arc<plugin_preferences::Store>,
    settings_descriptor: std::result::Result<Option<sodeck_plugin_manager::settings_descriptor::Descriptor>,String>,
    package: InstalledPlugin,
    process: Arc<PluginProcess>,
    instance: String,
    assets: Assets,
    gateway: Arc<Mutex<Option<std::sync::Weak<State>>>>,
}
struct Binding {
    id: String,
    view_id: String,
    generation: u64,
    window: String,
    phase: AtomicU8,
    ready: Mutex<bool>,
    changed: Condvar,
    geometry: Mutex<(Bounds, bool)>,
    resume: Mutex<Value>,
}
struct Cached {
    fingerprint: String,
    started: Instant,
    result: Mutex<Option<Response>>,
    changed: Condvar,
}
struct SnapshotTransfer {
    source: String,
    sender: mpsc::SyncSender<Result<Value>>,
    data: String,
    total: usize,
    next: usize,
}
struct PendingOpen {
    plugin_id: String,
    generation: Option<String>,
    created: Instant,
    server_id: Option<String>,
    window_id: String,
    command_id: String,
    canceled: bool,
    finished: bool,
    view_id: Option<String>,
    request: Value,
    result: Option<Result<Value>>,
    waiting_auth: bool,
}
pub struct State {
    settings: Mutex<settings::Registry>,
    settings_window: Mutex<settings_window::SettingsWindowRegistry>,
    settings_transition: Mutex<()>,
    settings_exit: Mutex<settings_exit::ExitReview>,
    plugin_management: Mutex<plugin_management::Registry>,
    updates: updates::Updates,
    connection_home: Mutex<connection_home::Registry>,
    close_review: Mutex<Option<window_close_review::Review>>,
    close_intent: Mutex<()>,
    home_navigation: Mutex<()>,
    runtime_mode: String,
    monitor_window: Mutex<monitor_window::MonitorWindowRegistry>,
    monitor_transition: Mutex<()>,
    communication: Mutex<sodeck_communication::Hub>,
    state_participants: Mutex<HashMap<String,std::collections::HashSet<String>>>,
    directories: Mutex<HashMap<String,directory_context::DirectorySource>>,
    theme: Mutex<crate::theme::Appearance>,
    app_generation: String,
    menus: Arc<crate::presentation::Menus>,
    application_menus: Mutex<HashMap<String,application_menu::ApplicationMenu>>,
    menu_opening: Mutex<()>,
    core: Arc<Core>,
    plugins: Mutex<PluginManager>,
    loaded: Mutex<HashMap<String,Arc<Loaded>>>,
    workbench: Mutex<Workbench>,
    bindings: Mutex<HashMap<String, Arc<Binding>>>,
    view_badges: Mutex<HashMap<String, u64>>,
    dialogs: Mutex<HashMap<String, Arc<plugin_dialogs::Session>>>,
    dialog_transition: Mutex<()>,
    shell_modals: Mutex<std::collections::HashSet<String>>,
    view_operations: ViewOperations,
    snapshots: Mutex<HashMap<String, SnapshotTransfer>>,
    cache: Mutex<HashMap<String, Arc<Cached>>>,
    pending_opens: Mutex<HashMap<String, PendingOpen>>,
    mounting: Mutex<()>,
    shutdown_lock: Mutex<()>,
    workspace_busy: Mutex<std::collections::BTreeSet<String>>,
    workspace_creation: Mutex<()>,
    chrome_reports: Mutex<HashMap<String, crate::window_chrome::Report>>,
    dragging: AtomicBool,
    seen_window_focus: Mutex<std::collections::HashSet<String>>,
    queued_window_closes: Mutex<std::collections::BTreeSet<String>>,
    layout_operations: Mutex<HashMap<String, Arc<services::LayoutOperation>>>,
    native: NativeViews,
    app: tauri::AppHandle,
    data: PathBuf,
    stopping: AtomicBool,
    update_handoff: AtomicBool,
    inflight: AtomicUsize,
    _data_lock: fs::File,
    startup_error: Mutex<Option<String>>,
}

fn spawn_plugin(
    core: Arc<Core>,
    package: &InstalledPlugin,
) -> std::result::Result<Arc<Loaded>, String> {
    let assets = Assets::from_verified(&package.snapshot);
    let source=std::str::from_utf8(package.snapshot.file(&package.manifest.entry.worker).ok_or("Missing verified worker")?).map_err(|_|"Worker is not UTF-8")?.to_owned();
    let instance = Uuid::new_v4().to_string();
    let caller = Caller {
        source: "plugin".into(),
        plugin_id: Some(package.manifest.plugin_id.clone()),
        plugin_version: Some(package.manifest.version.clone()),
        instance_id: instance.clone(),
        view_id: None,
        mount_generation: None,
    };
    let data = package.data_dir.clone();
    let descriptor=package.manifest.observability.as_ref().map(|reference|package.snapshot.file(&reference.path).ok_or_else(||"无法读取已验证的观测描述".to_owned())).transpose()?;
    core.register_observation_source(&caller,descriptor).map_err(|e|e.to_string())?;
    let settings_descriptor=package.manifest.contributes.settings.as_ref().map(|reference| {
        let bytes=package.snapshot.file(&reference.path).ok_or_else(||"缺少已验证的设置描述".to_owned())?;
        sodeck_plugin_manager::settings_descriptor::Descriptor::parse(&bytes,&package.manifest.plugin_id)
    }).transpose();
    let preferences=plugin_preferences::Store::shared(&data);
    preferences.activate(&package.manifest.plugin_id,&instance,settings_descriptor.as_ref().ok().cloned().flatten());
    let callback_preferences=preferences.clone();
    let gateway = Arc::new(Mutex::new(None::<std::sync::Weak<State>>));
    let public_gateway = gateway.clone();
    let initializing = Arc::new(AtomicBool::new(true));
    let init = initializing.clone();
    let callback_core = core.clone();
    let binary_owner = caller.clone();
    let callback = Arc::new(move |method: &str, params: Value, metadata:Option<Value>| {
        let core = &callback_core;
        if method=="observability.console.summary"{return core.record(&caller,method,&params,||{core.observe_host_event(&caller,"host.console.summary",params.clone())?;Ok(json!({"accepted":true}))}).map_err(|e|RpcError::new(&e.code,e.message));}
        let active_gateway = public_gateway
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|w| w.upgrade());
        let result = core.diagnostic_request(&caller,method,&params,metadata.as_ref(),|| if init.load(Ordering::SeqCst)
            && !sodeck_plugin_host::dispatcher::observation(method)
            && !matches!(
                method,
                "storage.preferences.read"
                    | "storage.get"
                    | "storage.set"
                    | "settings.get"
                    | "servers.list"
                    | "servers.get"
                    | "diagnostics.get"
            ) {
            core.record(&caller, method, &params, || {
                Err(err("INVALID_STATE", "插件初始化期间不能启动远端业务"))
            })
        } else if matches!(method,"storage.preferences.read"|"storage.preferences.commit"|"settings.provider.applied") {
            core.record(&caller,method,&params,|| {
                settings::check("x-methods",method,&params)?;
                let result=match method {
                    "storage.preferences.read"=>callback_preferences.read(&caller.instance_id,required(&params,"groupId")?),
                    "storage.preferences.commit"=>callback_preferences.commit(&caller.instance_id,required(&params,"commitTicket")?,serde_json::from_value(params["changes"].clone()).map_err(|e|err("INVALID_PARAMS",e.to_string()))?),
                    _=>callback_preferences.applied(&caller.instance_id,required(&params,"ticket")?,required(&params,"settingsRevision")?).map(|_|json!({"accepted":true})),
                }?;
                settings::check("x-results",method,&result)?;Ok(result)
            })
        } else if ["storage.get", "storage.set"].contains(&method)
            && params.get("namespace").is_none()
        {
            core.record(&caller, method, &params, || {
                callback_preferences.raw(&caller.instance_id,method,&params)
            })
        } else if let Some(state) = active_gateway {
            state.dispatch(&caller, None, method, params.clone())
        } else {
            core.call(&caller, method, params.clone())
        });
        result.map_err(|error| {
            serde_json::from_value::<RpcError>(serde_json::to_value(error).unwrap())
                .unwrap_or_else(|_| RpcError::new("CORE_ERROR", "核心调用失败"))
        })
    });
    let process = Arc::new(
        PluginProcess::spawn_source_with_context(
            std::env::current_exe().map_err(|e| e.to_string())?,
            source,
            callback,
            Some(sodeck_plugin_host::BinaryCallbacks {
                receive: {let core=core.clone();let owner=binary_owner.clone();Arc::new(move|frame|core.network_input(&owner,frame).map_err(|e|RpcError::new(&e.code,e.message)))},
                produce: {let core=core.clone();let owner=binary_owner.clone();Arc::new(move|sink|core.network_output(&owner,|frame|sink(frame)))},
            }),
        )
        .map_err(|e| {core.close_instance(&instance);e.to_string()})?,
    );
    if let Err(error)=process.initialize(json!({"pluginId":package.manifest.plugin_id,"pluginVersion":package.manifest.version,"instanceId":instance,"observationCreditBytes":core.grant_observation_credit(&instance,&format!("{instance}:worker:"),128*1024),"dataSchemaVersion":package.manifest.data_schema_version})) {
        process.terminate();core.close_instance(&instance);return Err(error.to_string());
    }
    initializing.store(false, Ordering::SeqCst);
    let _=core.observe_host_event(&binary_owner,"core.plugin.started",json!({"phase":"activate"}));
    Ok(Arc::new(Loaded {
        preferences, settings_descriptor,
        package: package.clone(),
        process,
        instance,
        assets,
        gateway,
    }))
}

impl State {
    fn changed(&self) {
        self.sync_directories();
        self.prune_state_feeds();
        let payload = {
            let w = self.workbench.lock().unwrap();
            json!({"revision":w.revision,"windowIds":w.windows.keys().collect::<Vec<_>>(),"workspaceIds":w.workspaces.keys().collect::<Vec<_>>(),"viewIds":w.views.iter().map(|v|&v.view_id).collect::<Vec<_>>(),"changeKind":"snapshot"})
        };
        debug_assert!(crate::workbench_contract::validate_kind(
            "x-events",
            "workbench.changed",
            &payload
        )
        .is_ok());
        let notification = json!({"type":"workbench.changed","payload":payload});
        for window in self.app.webview_windows().values() {
            let _ = window.eval(&format!("window.__sodeckNotify?.({notification});"));
        }
        let ids = self
            .bindings
            .lock()
            .unwrap()
            .values()
            .filter(|b| b.phase.load(Ordering::SeqCst) == 1)
            .map(|b| b.id.clone())
            .collect::<Vec<_>>();
        for id in ids {
            let _ = self.native.notify(id, "workbench.changed", payload.clone());
        }
    }
    fn save_preferences(&self) -> Result<()> {
        let w = self.workbench.lock().unwrap();
        crate::window_preferences::save(&self.data, w.windows.get("main").and_then(|w| w.bounds.clone()))
            .map_err(|e| err("STORAGE_ERROR", e))
    }
    fn warning(&self, warning: &Value) {
        let notification = json!({"type":"workbench.warning","payload":warning});
        for window in self.app.webview_windows().values() {
            let _ = window.eval(&format!("window.__sodeckNotify?.({notification});"));
        }
    }
    fn commit_runtime(&self, mut result: Value) -> Value {
        self.update_window_titles();
        result["committed"] = json!(true);
        result["persisted"] = json!(false);
        self.changed();
        result
    }
    fn binding(&self, id: &str) -> Result<Arc<Binding>> {
        self.bindings
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(|| err("INSTANCE_EXPIRED", "视图挂载已经结束"))
    }
    fn view_binding(&self, id: &str) -> Option<Arc<Binding>> {
        self.bindings
            .lock()
            .unwrap()
            .values()
            .filter(|b| b.view_id == id && matches!(b.phase.load(Ordering::SeqCst), 1 | 2))
            .max_by_key(|b| b.generation)
            .cloned()
    }
    fn bind_resources(&self, view: &View) -> Result<Value> {
        let references = self
            .workbench
            .lock()
            .unwrap()
            .resources
            .get(&view.view_id)
            .cloned()
            .unwrap_or_default();
        let cursors=self.core.bind_view_resources(&self.loaded_for(&view.plugin_id)?.instance,&[json!({"viewId":view.view_id,"workspaceId":view.workspace_id,"mountGeneration":view.mount_generation,"resources":references})])?;
        Ok(cursors[&view.view_id].clone())
    }
    fn create_binding(
        self: &Arc<Self>,
        view: &View,
        window: &str,
        snapshot: Option<Value>,
        candidate: bool,
    ) -> Result<Arc<Binding>> {
        let loaded = self.loaded_for(&view.plugin_id)?;
        let server = if view.workspace_id == "@app" {
            json!({"name":null})
        } else {
            self.core.call(
                &Caller::shell(),
                "servers.get",
                json!({"serverId":view.server_id}),
            )?
        };
        let binding_state = if !candidate {
            self.bind_resources(view)?
        } else {
            Value::Null
        };
        let snapshot = snapshot.map(|mut state| {
            if state["recovered"].as_bool() == Some(true) {
                state["acknowledged"] = binding_state["acknowledged"].clone();
                state["lastInputSeq"] = binding_state["lastInputSeq"].clone();
            }
            state
        });
        let id = Uuid::new_v4().to_string();
        let binding = Arc::new(Binding {
            id: id.clone(),
            view_id: view.view_id.clone(),
            generation: view.mount_generation,
            window: window.into(),
            phase: AtomicU8::new(if candidate { 0 } else { 1 }),
            ready: Mutex::new(false),
            changed: Condvar::new(),
            geometry: Mutex::new((Bounds::default(), false)),
            resume: Mutex::new(binding_state),
        });
        self.bindings
            .lock()
            .unwrap()
            .insert(id.clone(), binding.clone());
        let context = json!({"diagnosticContextVersion":1,"observationCreditBytes":self.core.grant_observation_credit(&loaded.instance,&format!("{}:{}:{}",loaded.instance,view.view_id,view.mount_generation),32*1024),"workspaceGeneration":self.workbench.lock().unwrap().workspaces.get(&view.workspace_id).map(|w|w.generation.clone()),"pluginId":view.plugin_id,"theme":self.theme_snapshot(loaded.package.manifest.requires.capabilities.iter().any(|c|c=="ui.theme.live.v1")),"viewId":view.view_id,"viewType":view.view_type,"workspaceId":view.workspace_id,"serverId":view.server_id,"serverName":server["name"],"pluginVersion":loaded.package.manifest.version,"mountGeneration":view.mount_generation,"windowId":window,"pluginData":view.runtime_data,"pluginState":view.plugin_state,"snapshot":snapshot});
        let weak = Arc::downgrade(self);
        let fail = weak.clone();
        let ipc = Arc::new(move |source: String, body: String| {
            if let Some(state) = weak.upgrade() {
                state.receive(source, body);
            }
        });
        let failure = Arc::new(move |source: String| {
            if let Some(state) = fail.upgrade() {
                std::thread::spawn(move || state.surface_failed(&source));
            }
        });
        let result = self.native.create(
            id.clone(),
            window.into(),
            loaded.package.manifest.entry.ui.clone(),
            context,
            loaded.assets.clone(),
            ipc,
            failure,
            self.data
                .join("profiles")
                .join(&loaded.package.manifest.plugin_id),
        );
        if let Err(error) = result {
            self.bindings.lock().unwrap().remove(&id);
            if !candidate {
                if let Some(v) = self
                    .workbench
                    .lock()
                    .unwrap()
                    .views
                    .iter_mut()
                    .find(|v| v.view_id == view.view_id)
                {
                    v.status = "viewFailed".into();
                }
            }
            return Err(native_error(error));
        }
        Ok(binding)
    }
    fn receive(self: &Arc<Self>, source: String, body: String) {
        #[derive(Deserialize, Default)]
        #[serde(rename_all = "camelCase")]
        struct Header {
            request_id: Option<String>,
            method: Option<String>,
        }
        let header = serde_json::from_str::<Header>(&body).unwrap_or_default();
        let cleanup = matches!(
            header.method.as_deref(),
            Some(
                "app.exit"
                    | "state.close" | "state.ack" | "pty.observe.close" | "directories.unregister"
                    | "views.close"
                    | "workspaces.close"
                    | "windows.close"
                    | "workbench.operation.cancel"
                    | "operations.cancel"
                    | "pty.close"
                    | "connections.release"
                    | "resources.stop"
            )
        );
        let admitted =
            self.inflight.fetch_add(1, Ordering::SeqCst) < if cleanup { 128 } else { 112 };
        if !admitted || body.len() > 1024 * 1024 {
            self.inflight.fetch_sub(1, Ordering::SeqCst);
            let caller = self.caller(Some(&source)).unwrap_or_else(|_| Caller {
                source: "expired-surface".into(),
                instance_id: source.clone(),
                plugin_id: None,
                plugin_version: None,
                view_id: None,
                mount_generation: None,
            });
            let result = self.core.record(
                &caller,
                header.method.as_deref().unwrap_or("rpc.invalid"),
                &json!({"requestBytes":body.len()}),
                || {
                    Err(err(
                        "CAPACITY",
                        if !admitted {
                            "公开调用队列已满，请稍后重试"
                        } else {
                            "控制请求超过 1 MiB"
                        },
                    ))
                },
            );
            let response = Response::from(
                header.request_id.unwrap_or_else(|| "invalid".into()),
                result,
            );
            let native = self.native.clone();
            let _ = native.post_eval(
                source,
                format!(
                    "window.__sodeckReceive?.({});",
                    serde_json::to_string(&response).unwrap()
                ),
            );
            return;
        }
        let state = self.clone();
        let method=header.method.unwrap_or_else(||"rpc.invalid".into());
        let short=sodeck_plugin_host::dispatcher::short(&method);
        let request_bytes=body.len();
        let rejected_source=source.clone();let request_id=header.request_id.unwrap_or_else(||"invalid".into());
        let job=Box::new(move || {
            let response = match serde_json::from_str::<Request>(&body) {
                Ok(request) => state.request(Some(&source), request),
                Err(_) => {
                    let result = state.caller(Some(&source)).and_then(|caller| {
                        state.core.record(
                            &caller,
                            "rpc.invalid",
                            &json!({"requestBytes":body.len()}),
                            || Err(err("INVALID_REQUEST", "请求格式无效")),
                        )
                    });
                    Response::from("invalid".into(), result)
                }
            };
            let script = format!(
                "window.__sodeckReceive?.({});",
                serde_json::to_string(&response).unwrap()
            );
            if short {let _=state.native.post_eval(source,script);} else {let _=state.native.eval(source,script);}
            state.inflight.fetch_sub(1, Ordering::SeqCst);
        });
        if short {
            if !sodeck_plugin_host::dispatcher::submit_sized(&method,request_bytes,job){
                self.inflight.fetch_sub(1,Ordering::SeqCst);
                let result=self.caller(Some(&rejected_source)).and_then(|caller|self.core.record(&caller,&method,&json!({"capacity":true}),||Err(err("CAPACITY","短调用队列已满"))));
                let response=Response::from(request_id,result);
                let _=self.native.post_eval(rejected_source,format!("window.__sodeckReceive?.({});",serde_json::to_string(&response).unwrap()));
            }
        } else {std::thread::spawn(job);}
    }
    fn caller(&self, source: Option<&str>) -> Result<Caller> {
        if let Some(source) = source {
            if let Some(caller)=self.endpoint_caller(source)? {return Ok(caller);}
            if source.starts_with("@window:") {
                let window=self.endpoint_window(source,false)?;
                if self.app.get_webview_window(&window).is_none() {
                    return Err(err("SOURCE_INVALID", "窗口已关闭"));
                }
                return Ok(Caller::shell());
            }
            let b = self.binding(source)?;
            let l = self.plugin_for_view(&b.view_id)?;
            Ok(Caller {
                source: "plugin".into(),
                plugin_id: Some(l.package.manifest.plugin_id.clone()),
                plugin_version: Some(l.package.manifest.version.clone()),
                instance_id: l.instance.clone(),
                view_id: Some(b.view_id.clone()),
                mount_generation: Some(b.generation),
            })
        } else {
            Ok(Caller::shell())
        }
    }
    pub fn request(self: &Arc<Self>, source: Option<&str>, request: Request) -> Response {
        let caller = match self.caller(source) {
            Ok(c) => c,
            Err(e) => {
                let caller = Caller {
                    source: "expired-surface".into(),
                    instance_id: source.unwrap_or("shell").into(),
                    plugin_id: None,
                    plugin_version: None,
                    view_id: None,
                    mount_generation: None,
                };
                return Response::from(
                    request.request_id,
                    self.core
                        .record(&caller, &request.method, &request.params, || Err(e)),
                );
            }
        };
        if request.api_version != 1
            || request.request_id.is_empty()
            || request.request_id.len() > 128
            || request.method.len() > 128
        {
            return Response::from(
                request.request_id,
                self.core
                    .record(&caller, &request.method, &request.params, || {
                        Err(err("INVALID_REQUEST", "请求版本或标识无效"))
                    }),
            );
        }
        if serde_json::to_vec(&request.params)
            .unwrap_or_default()
            .len()
            > 1024 * 1024 - 1024
        {
            return Response::from(
                request.request_id,
                self.core
                    .record(&caller, &request.method, &json!({"oversize":true}), || {
                        Err(err("CAPACITY", "控制请求超过 1 MiB"))
                    }),
            );
        }
        let query = request.method.ends_with(".get")
            || request.method.ends_with(".list")
            || matches!(
                request.method.as_str(),
                "pty.read"
                    | "pty.ack"
                    | "pty.observe.read"
                    | "state.ack"
                    | "view.initialize"
                    | "workbench.snapshot"
                    | "workbench.surface"
                    | "workbench.contributions"
                    | "workbench.contributions.v2"
                    | "ui.connectionHome.snapshot"
                    | "app.info"
                    | "communication.summary"
                    | "quality.summary"
                    | "events.query" | "events.aggregate"
                    | "communication.sources"
                    | "communication.connections"
                    | "communication.history.query"
                    | "communication.history.compare"
            )
            || (request.method == "plugin.invoke"
                && request.params["command"]
                    .as_str()
                    .is_some_and(|c| c.ends_with(".get") || c.ends_with(".list") || c.ends_with(".snapshot")));
        if query {
            return Response::from(
                request.request_id,
                self.core.diagnostic_request(&caller,&request.method,&request.params,request.metadata.as_ref(),|| self.dispatch(&caller, source, &request.method, request.params.clone())),
            );
        }
        let cleanup = matches!(
            request.method.as_str(),
            "app.exit"
                | "views.close"
                | "workspaces.close"
                | "windows.close"
                | "workbench.operation.cancel"
                | "operations.cancel"
                | "pty.close"
                | "connections.release"
                | "resources.stop"
                | "quality.check.cancel"
        );
        if cleanup && self.cache.lock().unwrap().len() >= 10000 {
            return Response::from(
                request.request_id,
                self.core.diagnostic_request(&caller,&request.method,&request.params,request.metadata.as_ref(),|| self.dispatch(&caller, source, &request.method, request.params.clone())),
            );
        }
        let key = format!(
            "{}:{}:{}",
            caller.instance_id,
            source.unwrap_or("shell"),
            request.request_id
        );
        use sha2::{Digest, Sha256};
        let fingerprint = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&json!([request.method, request.params, request.metadata])).unwrap())
        );
        let (cached, existing) = {
            let mut cache = self.cache.lock().unwrap();
            cache.retain(|_, c| {
                c.started.elapsed() < Duration::from_secs(600) || c.result.lock().unwrap().is_none()
            });
            if let Some(c) = cache.get(&key) {
                (c.clone(), true)
            } else {
                if cache.len() >= 10000 {
                    return Response::from(
                        request.request_id,
                        self.core
                            .record(&caller, &request.method, &request.params, || {
                                Err(err("CAPACITY", "请求记录已达到容量上限"))
                            }),
                    );
                }
                let c = Arc::new(Cached {
                    fingerprint: fingerprint.clone(),
                    started: Instant::now(),
                    result: Mutex::new(None),
                    changed: Condvar::new(),
                });
                cache.insert(key, c.clone());
                (c, false)
            }
        };
        if existing {
            let result = self
                .core
                .record(&caller, &request.method, &request.params, || {
                    if cached.fingerprint != fingerprint {
                        return Err(err("REQUEST_CONFLICT", "同一请求标识不能提交不同内容"));
                    }
                    let guard = cached.result.lock().unwrap();
                    let (guard, _) = cached
                        .changed
                        .wait_timeout_while(
                            guard,
                            Duration::from_millis(request.timeout_ms.unwrap_or(35000).min(960000)),
                            |r| r.is_none(),
                        )
                        .unwrap();
                    match guard.as_ref() {
                        Some(r) => r.result.clone().ok_or_else(|| r.error.clone().unwrap()),
                        None => Err(err("RESULT_UNKNOWN", "操作仍在处理中，请查询状态")),
                    }
                });
            return Response::from(request.request_id, result);
        }
        let result = if self.stopping.load(Ordering::SeqCst) && request.method != "app.exit" {
            self.core
                .record(&caller, &request.method, &request.params, || {
                    Err(err("APP_STOPPING", "应用正在退出"))
                })
        } else {
            self.dispatch(&caller, source, &request.method, request.params.clone())
        };
        let response = Response::from(request.request_id, result);
        *cached.result.lock().unwrap() = Some(response.clone());
        cached.changed.notify_all();
        response
    }
    fn dispatch(
        self: &Arc<Self>,
        caller: &Caller,
        source: Option<&str>,
        method: &str,
        params: Value,
    ) -> Result<Value> {
        if matches!(method,"workspaces.open"|"workspaces.activate"|"workspaces.move"|"workspaces.close"|"windows.merge"|"views.activate"|"ui.settings.open"|"plugins.enablement.commit"|"plugins.enablement.repair"|"plugins.package.prepare"|"plugins.prepareUpdate") {
            if let Err(e)=self.ensure_review_mutable(){return self.core.record(caller,method,&params,||Err(e));}
        }
        if caller.plugin_id.is_some() && !sodeck_plugin_host::dispatcher::observation(method) && (params.get("scope").is_some() || method=="connections.acquire") {
            let checked=(|| {
                let owner=caller.plugin_id.as_deref().unwrap();
                let w=self.workbench.lock().unwrap();
                if let Some(scope)=params.get("scope") {
                    let id=required(scope,"workspaceId")?;
                    let generation=required(scope,"workspaceGeneration")?;
                    let ws=w.workspaces.get(id).ok_or_else(||err("STALE_WORKSPACE","工作区已关闭"))?;
                    if ws.generation!=generation || !w.views.iter().any(|v|v.workspace_id==id&&v.plugin_id==owner){return Err(err("STALE_WORKSPACE","插件工作区来源已失效"));}
                    if let Some(view)=&caller.view_id {if w.view(view).map_err(native_error)?.workspace_id!=id{return Err(err("SOURCE_INVALID","请求不属于当前视图工作区"));}}
                }
                if method=="connections.acquire" {
                    let operation=required(&params,"operationId")?;
                    let ops=self.pending_opens.lock().unwrap();
                    let op=ops.get(operation).ok_or_else(||err("STALE_OPERATION","连接操作已失效"))?;
                    let workspace=required(&params,"workspaceId")?;
                    if op.plugin_id!=owner||op.canceled||op.server_id.as_deref()!=Some(workspace)||w.workspaces.get(workspace).is_none_or(|ws|Some(&ws.generation)!=op.generation.as_ref()){return Err(err("CANCELED","连接所属工作区已关闭"));}
                }
                Ok(())
            })();
            if let Err(error)=checked{return self.core.record(caller,method,&params,||Err(error));}
        }
        if source.is_some_and(|s|self.dialog_endpoint(s).is_some()) && (method.starts_with("view.")||matches!(method,"views.requestFocus"|"views.recover"|"views.presentation"|"views.move"|"views.close"|"workbench.surface"|"workbench.modal")) {
            return self.core.record(caller,method,&params,||Err(err("INVALID_CONTEXT","弹窗不是主视图挂载")));
        }
        if let Some(source) = source.filter(|s| !s.starts_with("@window:") && self.dialog_endpoint(s).is_none()) {
            let b = self.binding(source)?;
            let phase = b.phase.load(Ordering::SeqCst);
            if phase == 3
                || (phase != 1
                    && !(phase == 0 && (method.starts_with("ui.theme.") || matches!(method,"state.ack"|"state.close") || (matches!(method,"state.get"|"state.open") && params["contract"]=="app.theme.v1")))
                    && !matches!(
                        method,
                        "view.initialize"
                            | "view.ready"
                            | "view.snapshotChunk"
                            | "view.moveRejected"
                    ))
            {
                return self.core.record(caller, method, &params, || {
                    Err(err("MOUNT_EXPIRED", "当前视图没有有效输入绑定"))
                });
            }
        }
        if sodeck_plugin_host::dispatcher::observation(method) {
            return self.observation_call(caller,source,method,params);
        }
        if method.starts_with("settings.") && !matches!(method,"settings.get"|"settings.set") {
            return self.core.record(caller,method,&params,||self.settings_call(caller,method,&params));
        }
        if sodeck_updater::contract::METHODS.contains(&method) {
            return self.core.record(caller,method,&params,||self.updates_call(caller,source,method,&params));
        }
        if method.starts_with("state.") {
            return self.core.record(caller,method,&params,||self.state_call(caller,source,method,&params));
        }
        if method=="communication.policy.set" {
            let result=self.core.call(caller,method,params)?;
            let event=json!({"type":"communication.policy.changed","payload":result});
            for window in self.app.webview_windows().values(){let _=window.eval(&format!("window.__sodeckNotify?.({event});"));}
            return Ok(result);
        }
        if method.starts_with("quality.")||method.starts_with("events.")||matches!(method,"communication.summary"|"communication.sources"|"communication.connections") {
            let mut owner=caller.clone();owner.instance_id=self.endpoint_feed_owner(caller,source);
            return self.core.call(&owner,method,params);
        }
        if method.starts_with("directories.") {
            return self.core.record(caller,method,&params,||self.directory_call(caller,method,&params));
        }
        if method.starts_with("workspaces.")
            || method.starts_with("windows.")
            || method.starts_with("views.")
            || method.starts_with("commands.")
            || method.starts_with("workbench.")
            || method.starts_with("view.")
            || method.starts_with("plugin.")
            || method.starts_with("plugins.")
            || method.starts_with("clipboard.")
            || method.starts_with("app.")
            || method.starts_with("ui.")
        {
            let result = self.core.record(caller, method, &params, || {
                let result = self.external(caller, source, method, &params)?;
                if method.starts_with("ui.connectionHome."){sodeck_core::connection_home_contract::validate("x-results",method,&result)?;}
                sodeck_core::workbench_ui_contract::validate("x-results", method, &result)?;
                crate::workbench_contract::validate_kind("x-results", method, &result)
                    .map_err(|e| err("PROTOCOL_ERROR", e))?;
                Ok(result)
            });
            if method=="app.exit" && result.is_ok() {
                // The exit call's final fact must precede sealing the writer.
                if !self.core.flush_observability() {crate::startup_diagnostics::record("observation drain incomplete at exit");}
                let app=self.app.clone();
                std::thread::spawn(move||{std::thread::sleep(Duration::from_millis(150));app.exit(0);});
            }
            return result;
        }
        let mut result = self.core.call(caller, method, params.clone());
        if method == "diagnostics.get" {
            if let Ok(value) = &mut result {
                // Read each owner separately: diagnostics must not invert the
                // lock order used by migration or report cached history as live work.
                let bindings = self.bindings.lock().unwrap().len();
                let snapshots = self.snapshots.lock().unwrap().len();
                let busy = self.workspace_busy.lock().unwrap().len();
                let queued_windows = self.queued_window_closes.lock().unwrap().len();
                let (view_operations, queued_views) = self.view_operations.counts();
                let pending_commands = self
                    .pending_opens
                    .lock()
                    .unwrap()
                    .values()
                    .filter(|v| !v.finished)
                    .count();
                let native = self.native.diagnostics().map_err(native_error)?;
                value["communication"]=serde_json::to_value(self.communication.lock().unwrap().stats()).unwrap();
                value["pluginInstances"] = json!(self.all_plugins().iter().map(|p|json!({"pluginId":p.package.manifest.plugin_id,"instanceId":p.instance,"processId":p.process.process_id(),"alive":p.process.is_alive()})).collect::<Vec<_>>());
                value["pluginDialogs"] = json!({"active":self.dialogs.lock().unwrap().values().filter(|d|!d.cleaned.load(Ordering::SeqCst)).count()});
                value["workbench"] = json!({"bindings":bindings,"snapshots":snapshots,"busyWorkspaces":busy,"queuedWindowCloses":queued_windows,"viewOperations":view_operations,"queuedViewCloses":queued_views,"pendingCommands":pending_commands,"native":native});
            }
        }
        if result.is_ok() && method == "servers.delete" {
            let id = params["serverId"].as_str().unwrap_or("");
            let ids = self
                .workbench
                .lock()
                .unwrap()
                .views
                .iter()
                .filter(|v| v.server_id == id)
                .map(|v| v.view_id.clone())
                .collect::<Vec<_>>();
            for view in ids {
                let _ = self.close_view(&view);
            }
            let mut w = self.workbench.lock().unwrap();
            let _ = w.remove_workspace(id);
            drop(w);
            let _ = self.save_preferences();
            self.changed();
        }
        result
    }
    fn external(
        self: &Arc<Self>,
        _caller: &Caller,
        source: Option<&str>,
        method: &str,
        params: &Value,
    ) -> Result<Value> {
        if let Some(result) = self.window_service(source, method, params) {
            return result;
        }
        crate::workbench_contract::validate(method, params)
            .map_err(|e| err("INVALID_PARAMS", e))?;
        sodeck_core::workbench_ui_contract::validate("x-methods", method, params)?;
        if method=="ui.connectionDiagnostics.open" {
            sodeck_core::connection_diagnostics::validate("x-methods",method,params)?;
            let source=source.ok_or_else(||err("SOURCE_INVALID","缺少详情来源"))?;
            let window=self.endpoint_window(source,true)?;
            let record=self.core.call(_caller,"connections.diagnostics.get",json!({"reference":params["reference"]}))?;
            let w=self.workbench.lock().unwrap();
            if !w.workspaces.get(record["workspaceId"].as_str().unwrap_or("")).is_some_and(|ws|record["workspaceGeneration"].is_null()||record["workspaceGeneration"]==ws.generation){return Err(err("STALE_WORKSPACE","连接所属工作区已结束"));}
            drop(w);
            self.endpoint_notify(&format!("@window:{window}"),"connectionDiagnostics.open",json!({"reference":params["reference"]}));
            return Ok(json!({"accepted":true}));
        }
        if method.starts_with("ui.connectionHome."){return self.connection_home_call(source,method,params);}
        if method.starts_with("ui.settings."){return self.settings_window_call(source,method,params);}
        if method.starts_with("ui.communicationMonitor."){return self.monitor_call(source,method,params);}
        if matches!(method,"views.presentation"|"views.requestFocus"|"workbench.contributions"|"workbench.contributions.v2"|"views.badge.set"|"workbench.dock.configure") {return self.extended_workbench(_caller,source,method,params);}
        if method.starts_with("ui.dialog.") {return self.dialog_call(_caller,source,method,params);}
        if method.starts_with("ui.theme.") { return self.appearance_service(_caller,method,params); }
        if method.starts_with("ui.") { return self.presentation_service(source,method,params); }
        if matches!(method,"views.move"|"views.activate"|"views.close"|"workspaces.activate"|"workspaces.move"|"workspaces.close"|"windows.merge"|"workspaces.drag"|"app.exit") {
            for id in self.app.webview_windows().keys() { self.cancel_menus(id); }
        }
        if let Some(result) = self.workbench_service(_caller, source, method, params) {
            return result;
        }
        match method {
            "app.runtimeInfo.get"=>{settings::check("x-methods",method,params)?;Ok(json!({"version":env!("CARGO_PKG_VERSION"),"channel":self.updates.build.channel,"mode":self.runtime_mode,"dataPath":self.data.to_string_lossy()}))},
            "app.dataDirectory.copyPath"=>{settings::check("x-methods",method,params)?;arboard::Clipboard::new().and_then(|mut c|c.set_text(self.data.to_string_lossy().into_owned())).map_err(|e|err("CLIPBOARD_ERROR",e.to_string()))?;Ok(json!({"accepted":true}))},
            "app.dataDirectory.open"=>{settings::check("x-methods",method,params)?;
                #[cfg(windows)] let program="explorer.exe";
                #[cfg(target_os="macos")] let program="open";
                #[cfg(all(not(windows),not(target_os="macos")))] let program="xdg-open";
                let mut command=std::process::Command::new(program);command.arg(&self.data);
                #[cfg(windows)] {use std::os::windows::process::CommandExt;command.creation_flags(0x08000000);}
                command.spawn().map_err(|e|native_error(e.to_string()))?;Ok(json!({"accepted":true}))
            },
            "app.info" => Ok(
                json!({"version":env!("CARGO_PKG_VERSION"),"platform":std::env::consts::OS,"startupError":self.startup_error.lock().unwrap().clone()}),
            ),
            "app.show" => {
                restore_main_window(&self.shutdown_lock, &self.stopping,
                    || self.app.get_webview_window("main"),
                    |w| {
                        w.show().map_err(|e| native_error(format!("显示主窗口失败：{e}")))?;
                        if w.is_minimized().map_err(|e| native_error(format!("读取主窗口状态失败：{e}")))? {
                            w.unminimize().map_err(|e| native_error(format!("还原主窗口失败：{e}")))?;
                        }
                        w.set_focus().map_err(|e| native_error(format!("聚焦主窗口失败：{e}")))
                    })?;
                Ok(Value::Null)
            }
            "app.hide" => {
                let w = self
                    .app
                    .get_webview_window("main")
                    .ok_or_else(|| err("WINDOW_CLOSED", "主窗口不可用"))?;
                w.hide().map_err(|e| native_error(e.to_string()))?;
                Ok(Value::Null)
            }
            "app.exit" => {
                if self.begin_close_review(window_close_review::Action::Exit,source)?{return Err(err("CLOSE_REVIEW_REQUIRED","请先处理各窗口的未保存配置"));}
                self.shutdown();
                Ok(Value::Null)
            }
            "plugins.management.list"|"plugins.enablement.get"|"plugins.enablement.validate"|"plugins.enablement.commit"|"plugins.enablement.operation.get"|"plugins.enablement.repair"|"plugins.package.prepare"|"plugins.package.operation.get"=>self.plugin_management_call(_caller,method,params),
            "plugins.list" => {
                Ok(serde_json::to_value(self.plugins.lock().unwrap().status()).unwrap())
            }
            "plugins.metadata.list" => Ok(json!({"plugins":self.plugins.lock().unwrap().metadata()})),
            "plugins.prepareUpdate" => self.legacy_prepare_plugin(_caller,params),
            "plugin.invoke" => {
                if source.is_none_or(|s| s.starts_with("@window:")) {
                    return Err(err("SOURCE_INVALID", "插件调用须来自其自身视图"));
                }
                if params.get("pluginId").is_some() {
                    return Err(err("UNSUPPORTED_API", "首批不开放跨插件调用"));
                }
                let command = required(params, "command")?;
                if command == "command.execute" || command.starts_with("settings.provider.") || command.starts_with("lifecycle.") {
                    return Err(err("SOURCE_INVALID", "宿主生命周期不能通过业务路由调用"));
                }
                let mut args=params.get("args").cloned().unwrap_or(json!({}));
                let owner=_caller.plugin_id.as_deref().ok_or_else(||err("SOURCE_INVALID","插件来源缺失"))?;
                if self.loaded_for(owner)?.package.manifest.requires.capabilities.iter().any(|c|c=="workspace.presentation.v1") {
                    args["viewId"]=json!(_caller.view_id);
                }
                self.worker_for(owner, command, args)
            }
            "clipboard.readText" => {
                let text = arboard::Clipboard::new()
                    .and_then(|mut c| c.get_text())
                    .map_err(|_| err("CLIPBOARD_ERROR", "无法读取剪贴板"))?;
                if text.len() > 128 * 1024 {
                    return Err(err("CAPACITY", "单次剪贴板文本最多 128 KiB，请分段粘贴"));
                }
                Ok(json!({"text":text}))
            }
            "clipboard.writeText" => {
                let text = required(params, "text")?;
                if text.len() > 128 * 1024 {
                    return Err(err("CAPACITY", "单次复制最多 128 KiB 文本，请分段复制"));
                }
                arboard::Clipboard::new()
                    .and_then(|mut c| c.set_text(text))
                    .map_err(|_| err("CLIPBOARD_ERROR", "无法写入剪贴板"))?;
                Ok(Value::Null)
            }
            "views.recover" => {
                let id = required(params, "viewId")?;
                let _mount = self.mounting.lock().unwrap();
                if self.view_operations.busy(id) {
                    return Err(err("MOVE_IN_PROGRESS", "正在移动或关闭视图"));
                }
                let owner=self.workbench.lock().unwrap().view(id).map_err(native_error)?.plugin_id;
                self.recover_plugin(&owner)?;
                {
                    let mut w = self.workbench.lock().unwrap();
                    let mut candidate = w.clone();
                    let reset = candidate
                        .reset_incompatible_state(id)
                        .map_err(native_error)?;
                    let v = candidate
                        .views
                        .iter_mut()
                        .find(|v| v.view_id == id)
                        .ok_or_else(|| err("VIEW_CLOSED", "视图已关闭"))?;
                    if v.status == "ended" {
                        return Ok(json!({"ended":true}));
                    }
                    if reset {
                        w.commit(candidate)
                            .map_err(|e| err("STORAGE_ERROR", e))?;
                    }
                }
                let view = {
                    let mut w = self.workbench.lock().unwrap();
                    let v = w
                        .views
                        .iter_mut()
                        .find(|v| v.view_id == id)
                        .ok_or_else(|| err("VIEW_CLOSED", "视图已关闭"))?;
                    if v.status != "viewFailed" {
                        return Err(err("INVALID_STATE", "视图尚未失效"));
                    }
                    v.mount_generation += 1;
                    v.status = "ready".into();
                    v.clone()
                };
                let window = self
                    .workbench
                    .lock()
                    .unwrap()
                    .view_window(&view)
                    .map_err(native_error)?;
                if let Err(error) =
                    self.create_binding(&view, &window, Some(json!({"recovered":true})), false)
                {
                    if let Some(v) = self
                        .workbench
                        .lock()
                        .unwrap()
                        .views
                        .iter_mut()
                        .find(|v| v.view_id == id)
                    {
                        v.status = "viewFailed".into();
                    }
                    return Err(error);
                }
                Ok(self.commit_runtime(
                    json!({"viewId":id,"recovered":true})))
            }
            "views.activate" => {
                let id = required(params, "viewId")?;
                let v = self
                    .workbench
                    .lock()
                    .unwrap()
                    .activate(id)
                    .map_err(native_error)?;
                if let Some(b) = self.view_binding(id) {
                    let _ = self.native.focus(b.id.clone());
                    let _ = self.native.notify(b.id.clone(), "view.focus", json!({}));
                }
                if v.placement == "detached" {
                    if let Some(w) = self
                        .app
                        .get_webview_window(&format!("detached-{}", v.view_id))
                    {
                        let _ = w.show();
                        let _ = w.set_focus();
                    }
                }
                self.changed();
                Ok(serde_json::to_value(v).unwrap())
            }
            "views.close" => {
                let id = required(params, "viewId")?;
                if let Err(error) =
                    self.close_view_for_operation(id, params["operationId"].as_str())
                {
                    if error.code == "MOVE_IN_PROGRESS" && source.is_some() {
                        self.queue_native_close(id.into());
                        return Ok(json!({"queued":true}));
                    }
                    return Err(error);
                }
                Ok(Value::Null)
            }
            "views.move" => {
                if let Some(revision) = params["expectedRevision"].as_u64() {
                    if revision != self.workbench.lock().unwrap().revision {
                        return Err(err("LAYOUT_CONFLICT", "布局已更新，请重新选择目标"));
                    }
                }
                if params.get("screenPosition").is_some() {
                    self.drop_view(required(params, "viewId")?)
                } else {
                    self.move_view(
                        required(params, "viewId")?,
                        required(params, "placement")?,
                        params["group"].as_u64().unwrap_or(0) as u8,
                    )
                }
            }
            "view.initialize" => {
                let b =
                    self.binding(source.ok_or_else(|| err("SOURCE_INVALID", "需要视图上下文"))?)?;
                let view = self
                    .workbench
                    .lock()
                    .unwrap()
                    .view(&b.view_id)
                    .map_err(native_error)?;
                let generation=self.workbench.lock().unwrap().workspaces.get(&view.workspace_id).map(|w|w.generation.clone());
                let plugin=self.loaded_for(&view.plugin_id)?;
                self.core.register_network_view(&plugin.instance,&view.view_id,sodeck_core::WorkspaceScope {
                    workspace_id:view.workspace_id.clone(),workspace_generation:generation.clone().ok_or_else(||err("STALE_WORKSPACE","工作区已关闭"))?,
                })?;
                self.worker("lifecycle.initialize",json!({"viewId":view.view_id,"viewType":view.view_type,"workspaceId":view.workspace_id,"workspaceGeneration":generation,"pluginData":view.runtime_data,"instanceKey":view.instance_key,"serverId":view.server_id,"title":view.title}))
            }
            "view.ready" => {
                let b =
                    self.binding(source.ok_or_else(|| err("INVALID_STATE", "需要视图上下文"))?)?;
                *b.ready.lock().unwrap() = true;
                b.changed.notify_all();
                let guard = b.ready.lock().unwrap();
                let (_ready, timeout) = b
                    .changed
                    .wait_timeout_while(guard, Duration::from_secs(30), |_| {
                        b.phase.load(Ordering::SeqCst) == 0
                    })
                    .unwrap();
                if timeout.timed_out() || b.phase.load(Ordering::SeqCst) == 3 {
                    Err(err("MOVE_CANCELLED", "视图挂载未提交"))
                } else {
                    let mut result = b.resume.lock().unwrap().clone();
                    if !result.is_object() {
                        result = json!({});
                    }
                    result["ready"] = json!(true);
                    Ok(result)
                }
            }
            "view.snapshotChunk" | "view.moveRejected" => {
                self.snapshot_chunk(source, method, params)
            }
            _ => Err(err("UNSUPPORTED_API", format!("未实现的方法：{method}"))),
        }
    }
    fn snapshot_chunk(&self, source: Option<&str>, method: &str, params: &Value) -> Result<Value> {
        let source = source.ok_or_else(|| err("SOURCE_INVALID", "快照必须来自源视图"))?;
        let id = required(params, "operationId")?;
        let mut transfers = self.snapshots.lock().unwrap();
        let t = transfers
            .get_mut(id)
            .ok_or_else(|| err("OPERATION_EXPIRED", "迁移操作已结束"))?;
        if t.source != source {
            return Err(err("SOURCE_INVALID", "快照来源与迁移不一致"));
        }
        if method == "view.moveRejected" {
            let t = transfers.remove(id).unwrap();
            let _ = t.sender.send(Err(err(
                "MOVE_CANCELLED",
                params["reason"].as_str().unwrap_or("视图暂时无法移动"),
            )));
            return Ok(Value::Null);
        }
        let index = params["index"]
            .as_u64()
            .ok_or_else(|| err("INVALID_PARAMS", "缺少快照块序号"))? as usize;
        let total = params["total"]
            .as_u64()
            .ok_or_else(|| err("INVALID_PARAMS", "缺少快照块数量"))? as usize;
        let data = params["data"]
            .as_str()
            .ok_or_else(|| err("INVALID_PARAMS", "快照块无效"))?;
        if total == 0
            || total > 64
            || index != t.next
            || (index > 0 && total != t.total)
            || data.len() > 256 * 1024
            || t.data.len() + data.len() > 4 * 1024 * 1024
        {
            let t = transfers.remove(id).unwrap();
            let error = err("SNAPSHOT_INVALID", "快照块序号或大小无效");
            let _ = t.sender.send(Err(error.clone()));
            return Err(error);
        }
        t.total = total;
        t.next += 1;
        t.data.push_str(data);
        if t.next == total {
            let t = transfers.remove(id).unwrap();
            let result = serde_json::from_str(&t.data)
                .map_err(|_| err("SNAPSHOT_INVALID", "快照不是有效 JSON"));
            if result.is_ok() {
                self.binding(source)?.phase.store(2, Ordering::SeqCst);
            }
            let response = result
                .as_ref()
                .map(|_| json!({"accepted":true}))
                .map_err(Clone::clone);
            let _ = t.sender.send(result);
            response
        } else {
            Ok(json!({"accepted":true,"next":t.next}))
        }
    }
    fn present(
        self: &Arc<Self>,
        window: &str,
        id: &str,
        generation: u64,
        bounds: Bounds,
        visible: bool,
    ) -> Result<Value> {
        let _mount = self.mounting.lock().unwrap();
        if self.stopping.load(Ordering::SeqCst) {
            return Err(err("APP_STOPPING", "应用正在退出"));
        }
        let view = self
            .workbench
            .lock()
            .unwrap()
            .view(id)
            .map_err(native_error)?;
        if self
            .workbench
            .lock()
            .unwrap()
            .view_window(&view)
            .map_err(native_error)?
            != window
            || view.mount_generation != generation
        {
            return Err(err("STALE_MOUNT", "旧窗口或挂载尺寸已失效"));
        }
        let visible=visible&&self.page_allows_surface(&view,window)&&!self.shell_modals.lock().unwrap().contains(window);
        if view.status == "viewFailed" {
            return Ok(json!({"failed":true}));
        }
        let window = self
            .workbench
            .lock()
            .unwrap()
            .view_window(&view)
            .map_err(native_error)?;
        let binding = match self.view_binding(id) {
            Some(b) => b,
            None => {
                if !visible {
                    return Ok(Value::Null);
                }
                if view.status == "ended" {
                    return Ok(json!({"ended":true}));
                }
                self.create_binding(&view, &window, None, false)?
            }
        };
        if binding.phase.load(Ordering::SeqCst) != 3 {
            let (previous_bounds,was_visible)=*binding.geometry.lock().unwrap();
            // Hiding a native surface must not resize its PTY to an unmounted DOM's zero rectangle.
            let bounds=if visible{bounds}else{previous_bounds};
            self.native
                .present(binding.id.clone(), bounds, visible)
                .map_err(native_error)?;
            *binding.geometry.lock().unwrap() = (bounds, visible);
            if visible&&!was_visible {
                let focused=self.workbench.lock().unwrap().workspaces.get(&view.workspace_id).is_some_and(|w|w.focus_target.as_deref()==Some(id));
                if focused {let _=self.native.focus(binding.id.clone());let _=self.native.notify(binding.id.clone(),"view.focus",json!({}));}
            }
            if visible!=was_visible {drop(_mount);self.publish_presentation(id);}
        }
        Ok(Value::Null)
    }
    fn close_view(self: &Arc<Self>, id: &str) -> Result<()> {
        self.close_view_for_operation(id, None)
    }
    fn close_view_for_operation(self: &Arc<Self>, id: &str, operation: Option<&str>) -> Result<()> {
        let view = match self.workbench.lock().unwrap().view(id) {
            Ok(view) => view,
            Err(_) => return Ok(()),
        };
        self.cancel_view_open(id)?;
        let _workspace = self.workspace_guard_except(&[view.workspace_id.clone()], operation)?;
        if self.loaded_for(&view.plugin_id).is_ok() {
            let prepare=self.worker("lifecycle.prepareClose",json!({"viewId":id,"workspaceId":view.workspace_id,"pluginData":view.runtime_data,"operationId":operation}))?;
            if prepare["allow"] != true {
                return Err(err("CANCELED", "关闭已取消"));
            }
        }
        self.close_reserved_view(id)
    }
    fn close_reserved_view(self: &Arc<Self>, id: &str) -> Result<()> {
        let _operation = self
            .view_operations
            .begin(id)
            .ok_or_else(|| err("MOVE_IN_PROGRESS", "正在移动或关闭，请稍后重试"))?;
        self.close_view_inner(id)
    }
    fn queue_native_close(self: &Arc<Self>, id: String) {
        if !self.view_operations.queue_close(&id) {
            return;
        }
        let state = self.clone();
        std::thread::spawn(move || {
            struct QueuedClose<'a>(&'a ViewOperations, String);
            impl Drop for QueuedClose<'_> {
                fn drop(&mut self) {
                    self.0.cancel_close(&self.1);
                }
            }
            let _queued = QueuedClose(&state.view_operations, id.clone());
            let view = match state.workbench.lock().unwrap().view(&id) {
                Ok(view) => view,
                Err(_) => return,
            };
            let _workspace = loop {
                if state.stopping.load(Ordering::SeqCst) {
                    return;
                }
                match state.workspace_guard(&[view.workspace_id.clone()]) {
                    Ok(guard) => break guard,
                    Err(e) if e.code == "MOVE_IN_PROGRESS" => {
                        std::thread::sleep(Duration::from_millis(50))
                    }
                    Err(e) => {
                        state.warning(&json!({"code":e.code,"message":e.message}));
                        return;
                    }
                }
            };
            let _operation = state.view_operations.wait_close(&id);
            if state.stopping.load(Ordering::SeqCst) {
                return;
            }
            let result = (|| {
                if state.loaded_for(&view.plugin_id).is_ok() {
                    let prepare=state.worker("lifecycle.prepareClose",json!({"viewId":id,"workspaceId":view.workspace_id,"pluginData":view.runtime_data}))?;
                    if prepare["allow"] != true {
                        return Err(err("CANCELED", "关闭已取消"));
                    }
                }
                state.close_view_inner(&id)
            })();
            if let Err(error) = result {
                state.warning(
                    &json!({"code":error.code,"message":format!("关闭视图失败：{}",error.message)}),
                );
            }
        });
    }
    fn close_view_inner(self: &Arc<Self>, id: &str) -> Result<()> {
        self.cancel_plugin_dialogs(Some(id));
        let view = self
            .workbench
            .lock()
            .unwrap()
            .view(id)
            .map_err(native_error)?;
        if let Ok(plugin) = self.loaded_for(&view.plugin_id) {
            // Close is already committed. Revoke native listeners before waiting
            // for worker cleanup; a failed worker cannot keep a port alive.
            self.core.close_network_view(&plugin.instance,id);
            let _cleanup = self.worker(
                "lifecycle.close",
                json!({"viewId":id,"workspaceId":view.workspace_id,"pluginData":view.runtime_data}),
            );
            let resources = self
                .workbench
                .lock()
                .unwrap()
                .resources
                .get(id)
                .cloned()
                .unwrap_or_default();
            self.core
                .release_view_resources(&plugin.instance, &resources)?;
        }
        // A late geometry report must not remount this view between native
        // teardown and removal from the logical layout.
        let _mount = self.mounting.lock().unwrap();
        let bindings = self
            .bindings
            .lock()
            .unwrap()
            .values()
            .filter(|b| b.view_id == id)
            .cloned()
            .collect::<Vec<_>>();
        for b in bindings {
            b.phase.store(3, Ordering::SeqCst);
            b.changed.notify_all();
            let _ = self.native.close(b.id.clone());
            self.bindings.lock().unwrap().remove(&b.id);
        }
        self.workbench
            .lock()
            .unwrap()
            .close(id)
            .map_err(native_error)?;
        if let Some(w) = self.app.get_webview_window(&format!("detached-{id}")) {
            let _ = w.destroy();
        }
        if let Ok(target)=self.core.observation_target("view",id,Some(&view.mount_generation.to_string())){let _=self.core.observe_host_event(&Caller::shell(),"core.view.closed",json!({"objectRef":target["ref"],"generation":view.mount_generation.to_string()}));}
        self.commit_runtime(json!({}));Ok(())
    }
    fn move_view(self: &Arc<Self>, id: &str, placement: &str, group: u8) -> Result<Value> {
        let workspace = self
            .workbench
            .lock()
            .unwrap()
            .view(id)
            .map_err(native_error)?
            .workspace_id;
        let _workspace = self.workspace_guard(&[workspace])?;
        let _operation = self
            .view_operations
            .begin(id)
            .ok_or_else(|| err("MOVE_IN_PROGRESS", "此视图正在移动或等待关闭"))?;
        self.move_inner(id, placement, group)
    }
    fn drop_view(self: &Arc<Self>, id: &str) -> Result<Value> {
        #[cfg(windows)]
        let point = {
            let mut p = windows::Win32::Foundation::POINT::default();
            unsafe {
                windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut p)
                    .map_err(|e| native_error(e.to_string()))?;
            }
            tauri::PhysicalPosition::new(p.x, p.y)
        };
        #[cfg(not(windows))]
        return Err(err("UNSUPPORTED_PLATFORM", "本批仅验证 Windows 拖放"));
        #[cfg(windows)]
        {
            if let Some(main) = self
                .app
                .get_webview_window("main")
                .filter(|w| w.is_visible().unwrap_or(false))
            {
                let position = main
                    .inner_position()
                    .map_err(|e| native_error(e.to_string()))?;
                let size = main.inner_size().map_err(|e| native_error(e.to_string()))?;
                if point.x >= position.x
                    && point.y >= position.y
                    && point.x < position.x + size.width as i32
                    && point.y < position.y + size.height as i32
                {
                    let scale = main
                        .scale_factor()
                        .map_err(|e| native_error(e.to_string()))?;
                    let x = (point.x - position.x) as f64 / scale;
                    let y = (point.y - position.y) as f64 / scale;
                    let candidates = self
                        .bindings
                        .lock()
                        .unwrap()
                        .values()
                        .filter(|b| b.window == "main" && b.phase.load(Ordering::SeqCst) == 1)
                        .cloned()
                        .collect::<Vec<_>>();
                    let target = candidates.iter().find_map(|b| {
                        let (r, visible) = *b.geometry.lock().unwrap();
                        if visible
                            && x >= r.x
                            && y >= r.y
                            && x < r.x + r.width
                            && y < r.y + r.height
                        {
                            self.workbench
                                .lock()
                                .unwrap()
                                .view(&b.view_id)
                                .ok()
                                .filter(|v| ["center", "bottom"].contains(&v.placement.as_str()))
                        } else {
                            None
                        }
                    });
                    return self.move_view(
                        id,
                        target
                            .as_ref()
                            .map(|v| v.placement.as_str())
                            .unwrap_or("center"),
                        target.as_ref().map(|v| v.group).unwrap_or(0),
                    );
                }
            }
            let result = self.move_view(id, "detached", 0)?;
            if let Some(window) = self.app.get_webview_window(&format!("detached-{id}")) {
                let _ =
                    window.set_position(tauri::PhysicalPosition::new(point.x - 80, point.y - 20));
            }
            Ok(result)
        }
    }
    fn move_inner(self: &Arc<Self>, id: &str, placement: &str, group: u8) -> Result<Value> {
        self.cancel_plugin_dialogs(Some(id));
        let deadline = Instant::now() + Duration::from_secs(10);
        let old = self
            .workbench
            .lock()
            .unwrap()
            .view(id)
            .map_err(native_error)?;
        self.workbench
            .lock()
            .unwrap()
            .validate_placement(&format!("{}::{}",old.plugin_id,old.view_type), placement)
            .map_err(native_error)?;
        if group > 1 {
            return Err(err("UNSUPPORTED_LAYOUT", "最多支持两个区域"));
        }
        let old_window = self
            .workbench
            .lock()
            .unwrap()
            .view_window(&old)
            .map_err(native_error)?;
        let target_window = if placement == "detached" {
            format!("detached-{id}")
        } else {
            self.workbench
                .lock()
                .unwrap()
                .owner(&old.workspace_id)
                .map_err(native_error)?
        };
        if placement == "detached"
            && old.placement != "detached"
            && self
                .workbench
                .lock()
                .unwrap()
                .views
                .iter()
                .filter(|v| v.placement == "detached")
                .count()
                >= 2
        {
            return Err(err("CAPACITY", "最多同时打开两个独立视图窗口"));
        }
        if old_window == target_window {
            let moved = {
                let mut current = self.workbench.lock().unwrap();
                let mut candidate = current.clone();
                let moved = candidate
                    .moved(id, placement, group, old.mount_generation)
                    .map_err(native_error)?;
                current
                    .commit(candidate)
                    .map_err(|e| err("STORAGE_ERROR", e))?;
                moved
            };
            self.changed();
            let mut result = serde_json::to_value(moved).unwrap();
            result["committed"] = json!(true);
            result["persisted"] = json!(false);
            return Ok(result);
        }
        let source = self
            .view_binding(id)
            .ok_or_else(|| err("VIEW_NOT_READY", "请先显示此终端再移动"))?;
        let operation = Uuid::new_v4().to_string();
        let (tx, rx) = mpsc::sync_channel(1);
        self.snapshots.lock().unwrap().insert(
            operation.clone(),
            SnapshotTransfer {
                source: source.id.clone(),
                sender: tx,
                data: String::new(),
                total: 0,
                next: 0,
            },
        );
        let snapshot_result = (|| {
            self.native
                .notify(
                    source.id.clone(),
                    "view.prepareMove",
                    json!({"operationId":operation}),
                )
                .map_err(native_error)?;
            rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .map_err(|_| err("MOVE_TIMEOUT", "终端状态交接超时"))?
        })();
        self.snapshots.lock().unwrap().remove(&operation);
        let snapshot = match snapshot_result {
            Ok(v) => v,
            Err(e) => {
                source.phase.store(1, Ordering::SeqCst);
                let _ = self.native.notify(
                    source.id.clone(),
                    "view.moveCancelled",
                    json!({"operationId":operation}),
                );
                return Err(e);
            }
        };
        let mut target_view = old.clone();
        target_view.mount_generation += 1;
        target_view.placement = placement.into();
        target_view.group = group;
        let mut target: Option<Arc<Binding>> = None;
        let result = (|| {
            if placement == "detached" {
                self.open_detached(&target_view)?;
            }
            let b = self.create_binding(&target_view, &target_window, Some(snapshot), true)?;
            target = Some(b.clone());
            let ready = b.ready.lock().unwrap();
            let (ready, timeout) = b
                .changed
                .wait_timeout_while(
                    ready,
                    deadline.saturating_duration_since(Instant::now()),
                    |ready| !*ready,
                )
                .unwrap();
            if timeout.timed_out() || !*ready {
                return Err(err("MOVE_TIMEOUT", "目标终端未准备完成"));
            }
            drop(ready);
            if self.stopping.load(Ordering::SeqCst) {
                return Err(err("APP_STOPPING", "应用正在退出"));
            }
            let mount = self.mounting.lock().unwrap();
            let mut current = self.workbench.lock().unwrap();
            if source.phase.load(Ordering::SeqCst) != 2
                || b.phase.load(Ordering::SeqCst) != 0
                || current.view(id).map_err(native_error)?.mount_generation != old.mount_generation
            {
                return Err(err("STALE_MOUNT", "迁移期间表面已失效"));
            }
            let mut next = current.clone();
            let moved = next
                .moved(id, placement, group, target_view.mount_generation)
                .map_err(native_error)?;
            if placement != "detached" {
                next.activate_workspace(&old.workspace_id)
                    .map_err(native_error)?;
            }
            let references = current.resources.get(id).cloned().unwrap_or_default();
            let cursors=self.core.bind_view_resources(&self.loaded_for(&old.plugin_id)?.instance,&[json!({"viewId":id,"workspaceId":old.workspace_id,"mountGeneration":target_view.mount_generation,"resources":references})])?;
            *b.resume.lock().unwrap() = cursors[id].clone();
            *current = next;
            source.phase.store(3, Ordering::SeqCst);
            {
                let _ready = b.ready.lock().unwrap();
                b.phase.store(1, Ordering::SeqCst);
                b.changed.notify_all();
            }
            drop(current);
            drop(mount);
            let _ = self.native.notify(
                source.id.clone(),
                "view.moveCommitted",
                json!({"operationId":operation}),
            );
            let _ = self.native.close(source.id.clone());
            self.bindings.lock().unwrap().remove(&source.id);
            if old.placement == "detached" {
                if let Some(w) = self.app.get_webview_window(&old_window) {
                    let _ = w.destroy();
                }
            }
            Ok(self.commit_runtime(
                serde_json::to_value(moved).unwrap()))
        })();
        if result.is_err() && source.phase.load(Ordering::SeqCst) != 3 {
            if let Some(b) = target {
                b.phase.store(3, Ordering::SeqCst);
                b.changed.notify_all();
                let _ = self.native.close(b.id.clone());
                self.bindings.lock().unwrap().remove(&b.id);
            }
            if placement == "detached" {
                if let Some(w) = self.app.get_webview_window(&target_window) {
                    let _ = w.destroy();
                }
            }
            source.phase.store(1, Ordering::SeqCst);
            let _ = self.native.notify(
                source.id.clone(),
                "view.moveCancelled",
                json!({"operationId":operation}),
            );
        }
        result
    }
    fn open_detached(&self, view: &View) -> Result<()> {
        let label = format!("detached-{}", view.view_id);
        if self.app.get_webview_window(&label).is_some() {
            return Ok(());
        }
        let server = if view.workspace_id == "@app" {
            json!({"name":"SoDeck"})
        } else {
            self.core.call(
                &Caller::shell(),
                "servers.get",
                json!({"serverId":view.server_id}),
            )?
        };
        let mut window =
            WebviewWindowBuilder::new(&self.app, &label, WebviewUrl::App("index.html".into()))
                .icon(crate::brand::icon(false)).map_err(|e|native_error(e.to_string()))?
                .background_color(self.theme_background())
                .initialization_script(crate::theme::bootstrap(&self.theme_snapshot(true)))
                .title(format!(
                    "{} · {} — SoDeck",
                    server["name"].as_str().unwrap_or("服务器"),
                    view.title
                ))
                .decorations(false)
                .inner_size(960., 640.)
                .min_inner_size(480., 320.)
                .initialization_script(format!(
                    "window.__SODECK_CONTEXT__={{detachedViewId:{}}};",
                    serde_json::to_string(&view.view_id).unwrap()
                ))
                .data_directory(self.data.join("profiles/shell"));
        if cfg!(debug_assertions)||crate::startup_config::qa_build() {
            if let Some(port) = self.app.state::<crate::startup_config::DebugConfig>().shell_port {
                window = window.additional_browser_args(&format!("--remote-debugging-port={port}"));
            }
        }
        window.build().map_err(|e| native_error(e.to_string()))?;
        Ok(())
    }
    fn surface_failed(self: &Arc<Self>, source: &str) {
        let _mount = self.mounting.lock().unwrap();
        if let Ok(b) = self.binding(source) {
            let candidate = b.phase.load(Ordering::SeqCst) == 0;
            b.phase.store(3, Ordering::SeqCst);
            b.changed.notify_all();
            self.bindings.lock().unwrap().remove(source);
            let _ = self.native.close(source.into());
            if !candidate {
                if let Some(v) = self
                    .workbench
                    .lock()
                    .unwrap()
                    .views
                    .iter_mut()
                    .find(|v| v.view_id == b.view_id)
                {
                    if v.mount_generation == b.generation {
                        v.status = "viewFailed".into();
                    }
                }
            }
            self.changed();
        }
    }
    fn shutdown(&self) {
        let _shutdown = self.shutdown_lock.lock().unwrap();
        if self.stopping.swap(true, Ordering::SeqCst) {
            return;
        }
        drop(_shutdown);
        self.shutdown_resources();
    }
    fn shutdown_resources(&self) {
        self.updates.stop();
        self.stop_evidence_exports();
        self.cancel_plugin_dialogs(None);
        let _mount = self.mounting.lock().unwrap();
        let bindings = self
            .bindings
            .lock()
            .unwrap()
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for b in bindings {
            b.phase.store(3, Ordering::SeqCst);
            b.changed.notify_all();
            let _ = self.native.close(b.id.clone());
        }
        self.bindings.lock().unwrap().clear();
        let plugins=std::mem::take(&mut *self.loaded.lock().unwrap());
        // All producers drain concurrently; one slow plugin cannot multiply the deadline.
        std::thread::scope(|scope|{for plugin in plugins.values(){scope.spawn(move||{let _=plugin.process.stop_with_timeout(Duration::from_secs(2));});}});
        for plugin in plugins.values(){self.core.observe_plugin_lifecycle(&plugin.instance,"stop",None);self.core.close_instance(&plugin.instance);}
        self.core.shutdown();
        let _ = self.save_preferences();
        // Destroy shell WebViews while the event loop can still dispatch their
        // native teardown. Dropping them from the event-handler's destructor
        // can re-enter that handler while its captured state is being freed.
        for window in self.app.webview_windows().values() {
            let _ = window.destroy();
        }
    }

}

#[tauri::command]
async fn rpc(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Arc<State>>,
    request: Request,
) -> std::result::Result<Response, String> {
    let state = state.inner().clone();
    let id = request.request_id.clone();
    let registered_dialog=state.dialogs.lock().unwrap().values().find(|d|d.label==window.label()&&!d.cleaned.load(Ordering::SeqCst)).map(|d|d.source.clone());
    let settings_source=if state.settings_registered(window.label()){state.settings_source()}else{None};
    let monitor_source=if state.monitor_registered(window.label()){state.monitor_source()}else{None};
    if registered_dialog.is_none() && monitor_source.is_none() && settings_source.is_none() && window.label() != "main"
        && !window.label().starts_with("workspace-")
        && !window.label().starts_with("detached-")
    {
        return Ok(Response::from(
            id,
            Err(err("SOURCE_INVALID", "未知原生调用来源")),
        ));
    }
    Ok(tauri::async_runtime::spawn_blocking(move || {
        state.request(Some(&registered_dialog.or(monitor_source).or(settings_source).unwrap_or_else(||format!("@window:{}", window.label()))), request)
    })
    .await
    .unwrap_or_else(|_| Response::from(id, Err(err("INTERNAL", "调用任务意外结束")))))
}

fn read_keys(path: &Path) -> std::result::Result<TrustedKeys, String> {
    use base64::Engine;
    let value: Value = serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let mut keys = TrustedKeys::new();
    if let (Some(id), Some(key)) = (value["keyId"].as_str(), value["publicKey"].as_str()) {
        keys.insert(id.into(),sodeck_plugin_manager::TrustedKey {
            public_key:base64::engine::general_purpose::STANDARD.decode(key).map_err(|e|e.to_string())?,
            development_only:value["developmentOnly"].as_bool().unwrap_or(false),
        });
    } else if let Some(map) = value.as_object() {
        for (id, key) in map {
            if let Some(key) = key.as_str() {
                keys.insert(
                    id.into(),
                    base64::engine::general_purpose::STANDARD
                        .decode(key)
                        .map_err(|e| e.to_string())?.into(),
                );
            }
        }
    }
    if keys.is_empty() {
        return Err("开发插件签名公钥无效".into());
    }
    Ok(keys)
}
pub fn run() -> std::result::Result<(), Box<dyn std::error::Error>> {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![rpc,application_menu::application_menu_open,application_menu::application_menu_reply])
        .setup(|app| {
            // Tauri panics on a setup Err from its event callback. Handle the
            // failure here so GUI-only launches always have a visible channel.
            let result=(|| -> std::result::Result<(),Box<dyn std::error::Error>> {
            let handle = app.handle().clone();
            let config=crate::startup_config::StartupConfig::resolve(&std::env::current_exe()?,&handle.path().app_data_dir()?,&handle.path().resource_dir()?)?;
            let data=config.data;
            app.manage(config.debug);
            fs::create_dir_all(&data)?;
            let lock = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(data.join("app.lock"))?;
            fs2::FileExt::try_lock_exclusive(&lock)
                .map_err(|_| std::io::Error::other("此数据目录已有 SoDeck 实例运行"))?;
            crate::startup_diagnostics::use_data(&data);
            let updates=updates::Updates::open(&data,&config.mode,&std::env::current_exe()?);
            crate::startup_diagnostics::record("database preflight");
            sodeck_core::store_bootstrap::prepare_database(&data, |notice| {
                let impact=if notice.rebuild_core {
                    "将完整备份旧数据库和加密凭据，再创建全新配置库。原服务器档案、已保存凭据、信任记录、数据库设置与旧调用记录不会自动导入。插件包和独立外观偏好保留；旧服务器引用不会自动关联同名新服务器。"
                }else{"将完整备份并重建观测数据集，旧观测历史不会自动导入。服务器档案、凭据、信任记录和配置库保留。"};
                let mut versions=Vec::new();if notice.rebuild_core{versions.push(format!("配置库版本：{} → {}",notice.previous_version,notice.current_version));}
                for (name,previous,current) in &notice.observation_versions{versions.push(format!("{}：{} → {}",name,previous.map_or_else(||"新建".into(),|v|v.to_string()),current));}
                let description=format!("{}\n{}\n\n{}{}\n\n备份位置：{}",
                    if notice.recovering {"上次数据库重建未完成，将先恢复该操作。"}else{"数据库版本与当前程序不一致。"},
                    versions.join("\n"),impact,
                    if notice.rebuild_core && !notice.rebuild_observability {"\n现有观测历史保留，仅可作为旧配置的历史查阅。"}else{""},notice.backup_path.display());
                rfd::MessageDialog::new().set_title("SoDeck · 数据库版本更新").set_description(description).set_level(rfd::MessageLevel::Info).set_buttons(rfd::MessageButtons::Ok).show();
                Ok(())
            })?;
            let core = Arc::new(Core::open_dataset(&data)?);
            crate::startup_diagnostics::record("database ready");
            let (theme, mut startup_error)=crate::theme::load(&data);
            let resources=match config.resources {Ok(resources)=>Some(resources),Err(error)=>{startup_error=Some(error);None}};
            if let Some(resources)=&resources {for warning in &resources.warnings {crate::startup_diagnostics::record(&format!("startup warning: {warning}"));startup_error.get_or_insert_with(||warning.clone());}}
            let app_generation=Uuid::new_v4().to_string();
            let mut communication=sodeck_communication::Hub::default();
            let theme_key=sodeck_communication::Key{contract:"app.theme.v1".into(),scope:app_generation.clone(),object:"theme".into()};
            communication.reserve(theme_key.clone(),32*1024).map_err(|e|std::io::Error::other(e.0))?;
            communication.put(theme_key,json!({"appGeneration":app_generation}),theme.effective.clone()).map_err(|e|std::io::Error::other(e.0))?;
            let trusted = match resources.as_ref().map_or_else(||Err("随附插件资源不可用".into()),|r|read_keys(&r.keys)) {
                Ok(k) => k,
                Err(e) => {
                    if startup_error.is_none(){startup_error = Some(e);}
                    TrustedKeys::new()
                }
            };
            let mut plugins = PluginManager::open(data.join("plugins"), trusted)?;
            if let Some(resources)=resources {
                for bundle in resources.packages {
                    let prepared=plugins.stager().stage(&bundle.path).map_err(|e|e.to_string()).and_then(|staged|updates.accept_package(&mut plugins,staged,sodeck_updater::Origin::Bundled,&Uuid::new_v4().to_string(),true,bundle.plugin_id.as_deref()).map_err(|e|e.to_string()));
                    match prepared {
                        Ok(_)=>{},Err(error)=>{startup_error.get_or_insert_with(||error.to_string());},
                    }
                }
            }
            let init_core = core.clone();
            let activation_ready=match updates.before_activation(&mut plugins,&app_generation){Ok(())=>true,Err(e)=>{startup_error.get_or_insert(e.to_string());false}};
            if let Err(e) = if activation_ready { plugins.activate_pending(|candidate, _| {
                let loaded = spawn_plugin(init_core.clone(), candidate)?;
                let _ = loaded.process.stop();
                init_core.observe_plugin_lifecycle(&loaded.instance,"stop",None);init_core.close_instance(&loaded.instance);
                Ok(())
            }) }else{Ok(plugins.status())} {
                startup_error.get_or_insert_with(||e.to_string());
            }
            let mut loaded=HashMap::new();
            if let Err(e)=updates.after_activation(&plugins){startup_error.get_or_insert(e.to_string());}
            for status in plugins.status() {
                if !plugins.boot_enabled(&status.plugin_id) {continue;}
                match plugins.load_for_run(&status.plugin_id) {
                    Ok(Some(package))=>match spawn_plugin(core.clone(),&package){Ok(plugin)=>{loaded.insert(status.plugin_id.clone(),plugin);},Err(e)=>{startup_error.get_or_insert(e);}},
                    Err(e)=>{startup_error.get_or_insert_with(||e.to_string());},Ok(None)=>{},
                }
            }
            let (main_bounds, warnings) = crate::window_preferences::load(&data);
            let mut workbench = Workbench::default();
            workbench.windows.get_mut("main").unwrap().bounds = main_bounds;
            workbench.recovery_warnings = warnings;
            workbench.set_owned_declarations(loaded.values().flat_map(|p|p.package.manifest.contributes.views.iter().cloned().map(|v|(p.package.manifest.plugin_id.clone(),v))).collect()).map_err(std::io::Error::other)?;
            if let Some(error)=&startup_error {crate::startup_diagnostics::record(&format!("startup warning: {error}"));}
            let state = Arc::new(State {
                settings: Mutex::new(settings::Registry::default()),
                settings_window: Mutex::new(settings_window::SettingsWindowRegistry::default()),
                settings_transition: Mutex::new(()),
                settings_exit: Mutex::new(settings_exit::ExitReview::default()),
                plugin_management: Mutex::new(plugin_management::Registry::default()),
                updates,
                connection_home: Mutex::new(connection_home::Registry::default()),
                close_review: Mutex::new(None),
                close_intent: Mutex::new(()),
                home_navigation: Mutex::new(()),
                runtime_mode: config.mode.clone(),
                monitor_window: Mutex::new(monitor_window::MonitorWindowRegistry::default()),
                monitor_transition: Mutex::new(()),
                communication: Mutex::new(communication),
                state_participants: Mutex::new(HashMap::new()),
                directories: Mutex::new(HashMap::new()),
                theme: Mutex::new(theme),
                app_generation,
                menus: Arc::new(crate::presentation::Menus::default()),
                application_menus: Mutex::new(HashMap::new()),
                menu_opening: Mutex::new(()),
                core,
                plugins: Mutex::new(plugins),
                loaded: Mutex::new(loaded),
                workbench: Mutex::new(workbench),
                bindings: Mutex::new(HashMap::new()),
                view_badges: Mutex::new(HashMap::new()),
                dialogs: Mutex::new(HashMap::new()),
                dialog_transition: Mutex::new(()),
                shell_modals: Mutex::new(std::collections::HashSet::new()),
                view_operations: ViewOperations::default(),
                snapshots: Mutex::new(HashMap::new()),
                pending_opens: Mutex::new(HashMap::new()),
                cache: Mutex::new(HashMap::new()),
                mounting: Mutex::new(()),
                shutdown_lock: Mutex::new(()),
                workspace_busy: Mutex::new(std::collections::BTreeSet::new()),
                workspace_creation: Mutex::new(()),
                chrome_reports: Mutex::new(HashMap::new()),
                dragging: AtomicBool::new(false),
                seen_window_focus: Mutex::new(std::collections::HashSet::new()),
                queued_window_closes: Mutex::new(std::collections::BTreeSet::new()),
                layout_operations: Mutex::new(HashMap::new()),
                native: NativeViews::new(handle.clone()).map_err(std::io::Error::other)?,
                app: handle.clone(),
                data,
                stopping: AtomicBool::new(false),
                update_handoff: AtomicBool::new(false),
                inflight: AtomicUsize::new(0),
                _data_lock: lock,
                startup_error: Mutex::new(startup_error),
            });
            app.manage(state.clone());
            state.start_updates();
            for plugin in state.loaded.lock().unwrap().values() {
                *plugin.gateway.lock().unwrap() = Some(Arc::downgrade(&state));
            }
            state.monitor();
            state.start_state_pump();
            state.start_observation_pump();
            let mut main =
                WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                    .title("SoDeck v2")
                    .icon(crate::brand::icon(false))?
                    .background_color(state.theme_background())
                    .initialization_script(crate::theme::bootstrap(&state.theme_snapshot(true)))
                    .decorations(false)
                    .initialization_script(r#"window.__SODECK_CONTEXT__={windowId:"main"};"#)
                    .inner_size(1280., 820.)
                    .min_inner_size(760., 480.)
                    .data_directory(state.data.join("profiles/shell"));
            if cfg!(debug_assertions)||crate::startup_config::qa_build() {
                if let Some(port) = handle.state::<crate::startup_config::DebugConfig>().shell_port {
                    main = main.additional_browser_args(&format!("--remote-debugging-port={port}"));
                }
            }
            main.build()?;
            state.update_window_titles();
            crate::startup_diagnostics::record("main window ready");
            state.restore_window("main");
            use tauri::{
                menu::{Menu, MenuItem},
                tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
            };
            let show = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
            let exit = MenuItem::with_id(app, "exit", "退出 SoDeck", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &exit])?;
            let tray = TrayIconBuilder::with_id("sodeck")
                .icon(crate::brand::icon(true))
                .tooltip("SoDeck v2")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_tray_icon_event(|tray, event| {
                    if matches!(event, TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    }) {
                        show_main_from_tray(tray.app_handle());
                    }
                })
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => show_main_from_tray(app),
                    "exit" => {
                        let state = app.state::<Arc<State>>().inner().clone();
                        std::thread::spawn(move || {
                            let _=state.dispatch(&Caller::shell(),None,"app.exit",json!({}));
                        });
                    }
                    _ => {}
                })
                .build(app);
            if let Err(e) = tray {
                *state.startup_error.lock().unwrap() = Some(format!("系统托盘不可用：{e}"));
            }
            Ok(())
            })();
            if let Err(error)=result {
                if let Some(state)=app.try_state::<Arc<State>>() {state.shutdown();state.core.flush_observability();}
                crate::startup_diagnostics::fatal(&error);
                std::process::exit(1);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            let Some(state) = window.app_handle().try_state::<Arc<State>>() else {
                return;
            };
            let state = state.inner().clone();
            if window.label().starts_with("application-menu-"){return;}
            if state.dialog_window_event(window.label(),event){return;}
            if window.label()==settings_window::LABEL {
                if let tauri::WindowEvent::CloseRequested{api,..}=event {if !state.stopping.load(Ordering::SeqCst){api.prevent_close();let state=state.clone();std::thread::spawn(move||{let _=state.settings_request_close("close");});return;}}
            }
            if window.label()==monitor_window::LABEL {
                match event {
                    tauri::WindowEvent::CloseRequested{api,..} if !state.stopping.load(Ordering::SeqCst)=>{api.prevent_close();let source=state.monitor_source();std::thread::spawn(move||{let _=state.close_monitor(source.as_deref());});return;},
                    _=>{}
                }
            }
            match event {
                tauri::WindowEvent::CloseRequested { api, .. } => {
                    state.cancel_menus(window.label());
                    if state.stopping.load(Ordering::SeqCst) {
                        return;
                    }
                    api.prevent_close();
                    if window.label() == "main" {
                        if state.app.tray_by_id("sodeck").is_some() {
                            let _ = window.hide();
                        }
                    } else if let Some(id) = window.label().strip_prefix("detached-") {
                        state.queue_native_close(id.to_owned());
                    } else if window.label().starts_with("workspace-") {
                        state.queue_workspace_close(window.label());
                    }
                }
                tauri::WindowEvent::Focused(true) => {
                    let seen=!state.seen_window_focus.lock().unwrap().insert(window.label().into());
                    if seen {let label=window.label().to_string();std::thread::spawn(move||state.select_directory_window(&label));}
                },
                tauri::WindowEvent::Focused(false) => { let label=window.label().to_string();let menu=state.application_menus.lock().unwrap().get(&label).map(|e|e.token.clone()); std::thread::spawn(move||{std::thread::sleep(Duration::from_millis(80));let current=state.application_menus.lock().unwrap().get(&label).map(|e|e.token.clone());if current==menu&&!crate::presentation::owns_focus()&&!state.application_menu_has_focus(&label)&&!state.app.get_webview_window(&label).is_some_and(|w|w.is_focused().unwrap_or(false)){state.cancel_menus(&label);}}); },
                tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_) => {
                    state.cancel_menus(window.label());
                    let label=window.label().to_string();
                    std::thread::spawn(move||{state.align_plugin_dialogs(&label);state.remember_window(&label);});
                },
                tauri::WindowEvent::Destroyed => { state.communication.lock().unwrap().remove_owner(&format!("@window:{}:{}",state.app_generation,window.label()));state.seen_window_focus.lock().unwrap().remove(window.label());state.cancel_menus(window.label());state.chrome_reports.lock().unwrap().remove(window.label()); },
                _ => {}
            }
        })
        .build(tauri::generate_context!())?
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
                if let Some(state) = app.try_state::<Arc<State>>() {
                    // Destroying the final WebView must not end the main loop
                    // while the verified installer is still being handed off.
                    if state.update_handoff.load(Ordering::SeqCst) && code.is_none() {
                        api.prevent_exit();
                        return;
                    }
                    if !state.stopping.load(Ordering::SeqCst) {
                        api.prevent_exit();
                        if code.is_some() {
                            let state = state.inner().clone();
                            std::thread::spawn(move || {
                                let _=state.dispatch(&Caller::shell(),None,"app.exit",json!({}));
                            });
                        }
                    }
                }
            }
        });
    Ok(())
}

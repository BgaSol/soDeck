#![cfg(feature = "test-fixtures")]
use serde_json::{json, Value};
use sodeck::{workspaces::ConnectedSession, AppCore, TARGET};
use std::{
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};
static SEQUENCE: AtomicUsize = AtomicUsize::new(0);
// Keep native process launch pressure out of the five-second initialization budget.
// Calls inside each fixture remain concurrent in the RPC integration tests.
static STARTUP: Mutex<()> = Mutex::new(());
#[test]
fn shared_state_crosses_two_real_plugin_processes_and_cleans_stopped_sources() {
    let f = Fixture::new();
    f.load().unwrap();
    let second = f.root.join("consumer");
    fs::create_dir_all(second.join("ui")).unwrap();
    fs::copy(
        env!("CARGO_BIN_EXE_plugin-fixture"),
        second.join(format!("fixture{}", std::env::consts::EXE_SUFFIX)),
    )
    .unwrap();
    fs::write(second.join("ui/index.html"), "fixture").unwrap();
    let mut manifest: Value =
        serde_json::from_slice(&fs::read(f.plugin.join("plugin.json")).unwrap()).unwrap();
    manifest["id"] = json!("test.consumer");
    fs::write(second.join("plugin.json"), manifest.to_string()).unwrap();
    {
        let _startup = STARTUP.lock().unwrap();
        f.core.load(&second).unwrap();
    }
    let w = f.open("shared");
    f.core.ui_snapshot("main").unwrap();
    let mut surfaces = std::collections::HashMap::new();
    for plugin in ["test.ui", "test.consumer"] {
        let opened=f.core.ui_action("main",&w.context,json!({"op":"open","key":format!("{plugin}:browser"),"requestId":format!("open-{plugin}")})).unwrap();
        let mount = f
            .core
            .ui_attach("main", opened["viewId"].as_str().unwrap())
            .unwrap();
        let surface = mount["surfaceId"].as_str().unwrap().to_string();
        f.core.ui_surface_ready("main", &surface).unwrap();
        surfaces.insert(plugin, surface);
    }
    let invoke = |plugin: &str, method: &str, params: Value| {
        f.core
            .ui_event(
                "main",
                &surfaces[plugin],
                json!({"rpc":method,"params":params}),
            )
            .unwrap()
    };
    let initial = invoke(
        "test.consumer",
        "state.subscribe",
        json!({"clientSubscriptionId":"consumer","filter":{"topics":["test.shared.v1"]}}),
    );
    assert!(initial["records"].as_array().unwrap().is_empty());
    invoke(
        "test.consumer",
        "state.subscribe",
        json!({"clientSubscriptionId":"consumer","filter":{"topics":["test.shared.v1"]},"acknowledgedRevision":initial["revision"]}),
    );
    assert_eq!(
        invoke(
            "test.ui",
            "state.put",
            json!({"topic":"test.shared.v1","key":"current","expectedRevision":"0","value":{"path":"/shared"}})
        )["changed"],
        true
    );
    let frame = f.root.join("data/plugin-data/test.consumer/shared-frame");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        if let Ok(bytes) = fs::read(&frame) {
            let received: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(received["changes"][0]["record"]["value"]["path"], "/shared");
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    f.core.unload("test.ui").unwrap();
    assert!(invoke(
        "test.consumer",
        "state.list",
        json!({"filter":{"topics":["test.shared.v1"]}})
    )["records"]
        .as_array()
        .unwrap()
        .is_empty());
}
fn wait_close(
    core: &AppCore,
    id: &str,
    phase: sodeck::lifecycle::Phase,
) -> sodeck::lifecycle::Operation {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(12);
    loop {
        let op = core.close_status("main", id).unwrap();
        if op.phase == phase {
            return op;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "{:?}: {:?}",
            op.phase,
            op.error
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
fn dispose_fixture(f: &Fixture, text: &str) -> (String, String) {
    f.load().unwrap();
    let w = f.open("dispose");
    let opened=f.core.ui_action("main",&w.context,json!({"op":"open","key":"test.ui:editor","resourceKey":"/dispose","requestId":"dispose"})).unwrap();
    let id = opened["viewId"].as_str().unwrap().to_owned();
    let mount = f.core.ui_attach("main", &id).unwrap();
    let token = mount["surfaceId"].as_str().unwrap();
    f.core.ui_surface_ready("main", token).unwrap();
    f.core
        .ui_event("main", token, json!({"text":text}))
        .unwrap();
    let op = f
        .core
        .close_prepare(
            "main",
            "dispose",
            sodeck::lifecycle::Target::Views {
                ids: vec![id.clone()],
            },
        )
        .unwrap();
    f.core
        .close_report_inputs("main", &op.id, op.input_round, json!({"surfaces":[token]}))
        .unwrap();
    let ready = wait_close(&f.core, &op.id, sodeck::lifecycle::Phase::Deciding);
    let decision = f
        .core
        .close_decide("main", &op.id, ready.decision_revision, "discard")
        .unwrap();
    f.core
        .close_report_inputs("main", &op.id, decision.input_round, json!({}))
        .unwrap();
    (id, op.id)
}
#[test]
fn confirmed_dispose_failure_retries_once_on_the_next_user_retry() {
    use sodeck::lifecycle::Phase;
    let f = Fixture::new();
    let (view, op) = dispose_fixture(&f, "dispose-fail-once");
    wait_close(&f.core, &op, Phase::Failed);
    let count = f
        .root
        .join(format!("data/plugin-data/test.ui/dispose-count-{view}"));
    assert_eq!(fs::read_to_string(&count).unwrap(), "1");
    f.core.close_retry("main", &op).unwrap();
    wait_close(&f.core, &op, Phase::Completed);
    assert_eq!(fs::read_to_string(count).unwrap(), "2");
}
#[test]
fn unknown_dispose_result_is_not_resent_and_late_success_finishes_the_original_request() {
    use sodeck::lifecycle::Phase;
    let f = Fixture::new();
    let (view, op) = dispose_fixture(&f, "dispose-late");
    wait_close(&f.core, &op, Phase::Failed);
    let count = f
        .root
        .join(format!("data/plugin-data/test.ui/dispose-count-{view}"));
    f.core.close_retry("main", &op).unwrap();
    wait_close(&f.core, &op, Phase::Failed);
    assert_eq!(fs::read_to_string(&count).unwrap(), "1");
    fs::write(
        f.root
            .join(format!("data/plugin-data/test.ui/dispose-release-{view}")),
        "release",
    )
    .unwrap();
    f.core.close_retry("main", &op).unwrap();
    wait_close(&f.core, &op, Phase::Completed);
    assert_eq!(fs::read_to_string(count).unwrap(), "1");
}
#[test]
fn closing_background_workspace_cannot_recreate_views_before_parent_removal() {
    let f = Fixture::new();
    let path = f.plugin.join("plugin.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    manifest["ui"]["views"] = json!([{"id":"status","title":"状态","icon":"activity","container":"sidebar","instancePolicy":"perWorkspace","openOnWorkspace":"none","backgroundOnWorkspace":true}]);
    fs::write(path, manifest.to_string()).unwrap();
    f.load().unwrap();
    let w = f.open("close-background");
    let initial = f.core.ui_snapshot("main").unwrap();
    let original = initial["workspaces"][&w.context.workspace_id]["views"][0]["id"].clone();
    for _ in 0..20 {
        assert_eq!(
            f.core.ui_snapshot("main").unwrap()["workspaces"][&w.context.workspace_id]["views"][0]
                ["id"],
            original
        );
    }
    let cancelled = f
        .core
        .ui_close_prepare("main", vec![w.context.workspace_id.clone()], None)
        .unwrap();
    f.finish(&cancelled, "cancel").unwrap();
    assert_eq!(
        f.core.ui_snapshot("main").unwrap()["workspaces"][&w.context.workspace_id]["views"][0]
            ["id"],
        original
    );
    let plan = f
        .core
        .ui_close_prepare("main", vec![w.context.workspace_id.clone()], None)
        .unwrap();
    f.finish(&plan, "discard").unwrap();
    let state = f.core.ui_snapshot("main").unwrap();
    assert!(
        state["workspaces"][&w.context.workspace_id].is_null(),
        "the lifecycle removes the parent before releasing admission"
    );
    f.core.shutdown().unwrap();
}
#[test]
fn hidden_background_mount_has_no_surface_and_does_not_block_a_move() {
    let f = Fixture::new();
    let path = f.plugin.join("plugin.json");
    let mut m: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    m["ui"]["views"] = json!([{"id":"status","title":"状态","icon":"activity","container":"sidebar","instancePolicy":"perWorkspace","defaultSide":"right","defaultPinned":true,"openOnWorkspace":"none","backgroundOnWorkspace":true}]);
    fs::write(path, m.to_string()).unwrap();
    f.load().unwrap();
    let w = f.open("background");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(6);
    let view = loop {
        let ids: Vec<_> = f
            .core
            .ui_state()
            .workspaces
            .get(&w.context.workspace_id)
            .map(|w| w.views.iter().map(|v| v.id.clone()).collect())
            .unwrap_or_default();
        if let Some(id) = ids.first().filter(|id| {
            f.root
                .join(format!("data/plugin-data/test.ui/mounted-{id}"))
                .is_file()
        }) {
            break id.clone();
        }
        assert!(
            std::time::Instant::now() < deadline,
            "background mount never ran without ui_snapshot"
        );
        std::thread::sleep(std::time::Duration::from_millis(30));
    };
    let ui = f.core.ui_state();
    let state = &ui.workspaces[&w.context.workspace_id];
    assert!(state.host("main").unwrap().right.active_view_id.is_none());
    assert!(!state.views[0].ui_requested);
    assert_eq!(state.views.len(), 1);
    assert!(ui.target_ready(&w.context.workspace_id, "secondary"));
    drop(ui);
    f.core.ui_snapshot("main").unwrap();
    assert_eq!(
        f.core.ui_state().workspaces[&w.context.workspace_id].views[0].id,
        view
    );
    f.core
        .ui_action(
            "main",
            &w.context,
            json!({"op":"open","key":"test.ui:status","requestId":"reveal"}),
        )
        .unwrap();
    assert!(f.core.ui_state().workspaces[&w.context.workspace_id].views[0].ui_requested);
}
#[test]
fn component_settings_bind_reports_to_current_window_surface_and_revision() {
    let f = Fixture::new();
    let path = f.plugin.join("plugin.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    manifest["settings"] = json!({"apiVersion":"1","schemaVersion":1,"pages":[{"id":"browser","title":"列表","viewTypes":["browser"],"groups":[{"id":"display","title":"显示","fields":[{"key":"hidden","type":"boolean","title":"隐藏条目","default":true,"apply":"live"}]}]}]});
    fs::write(path, manifest.to_string()).unwrap();
    let token = f.surface();
    let snapshot = f.core.settings_for_surface("main", &token).unwrap();
    assert_eq!(snapshot["values"]["hidden"], true);
    assert!(f.core.settings_for_surface("wrong-window", &token).is_err());
    let report = json!({"pageId":snapshot["pageId"],"definitionToken":snapshot["definitionToken"],"revision":snapshot["revision"]});
    assert!(f
        .core
        .settings_applied("wrong-window", &token, report.clone())
        .is_err());
    assert!(f
        .core
        .settings_applied("main", &token, report.clone())
        .is_ok());
    assert_eq!(
        f.core.settings_page("test.ui", "browser").unwrap()["applications"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let saved=f.core.save_settings("test.ui","browser",json!({"definitionToken":snapshot["definitionToken"],"baseRevision":snapshot["revision"],"requestId":"settings-report","values":{"hidden":false}})).unwrap();
    assert_eq!(saved["snapshot"]["values"]["hidden"], false);
    assert!(f.core.settings_applied("main", &token, report).is_err());
    assert_eq!(
        f.core.settings_page("test.ui", "browser").unwrap()["applications"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
}
#[test]
fn separately_registered_plugins_keep_their_own_recommended_defaults() {
    let f = Fixture::new();
    f.load().unwrap();
    // Preferences already exist for another plugin: first launch is per declaration,
    // not per entire application. A user preference still wins on later snapshots.
    let second = f.root.join("second");
    fs::create_dir_all(second.join("ui")).unwrap();
    let binary = format!("fixture{}", std::env::consts::EXE_SUFFIX);
    fs::copy(env!("CARGO_BIN_EXE_plugin-fixture"), second.join(&binary)).unwrap();
    fs::write(second.join("ui/index.html"), "ok").unwrap();
    fs::write(second.join("plugin.json"),json!({"id":"test.second","name":"Second","version":"0.1.0","apiVersion":"1","target":TARGET,"entry":binary,"ui":{"apiVersion":"1","entry":"ui/index.html","views":[{"id":"browser","title":"Second files","icon":"folder","container":"sidebar","instancePolicy":"perWorkspace","defaultPinned":true,"openOnWorkspace":"reveal"}]}}).to_string()).unwrap();
    f.core.ui_snapshot("main").unwrap();
    {
        let _startup = STARTUP.lock().unwrap();
        f.core.load(&second).unwrap();
    }
    let w = f.open("defaults");
    let state = f.core.ui_snapshot("main").unwrap();
    let p = &state["workspaces"][&w.context.workspace_id]["preferences"]["test.second:browser"];
    assert_eq!(p["pinned"], true);
    assert_eq!(p["startup"], "reveal");
}
struct Fixture {
    root: PathBuf,
    plugin: PathBuf,
    core: Arc<AppCore>,
}
impl Fixture {
    fn actions(&self) {
        let path = self.plugin.join("plugin.json");
        let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        manifest["ui"]["actions"] =
            json!([{"id":"create","title":"默认行为","defaultCreate":true,"dataVersion":1}]);
        fs::write(path, manifest.to_string()).unwrap();
    }
    fn invoke(&self, context: &sodeck::workspaces::Context, id: &str, data: Value) -> Value {
        self.core
            .ui_invoke_action(
                "main",
                &json!({"context":context,"requestId":id,"actionKey":"test.ui:create","data":data}),
            )
            .unwrap()
    }
    fn load(&self) -> Result<sodeck::model::PluginView, String> {
        let _startup = STARTUP.lock().unwrap();
        self.core.load(&self.plugin)
    }
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "sodeck-components-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let plugin = root.join("plugin");
        fs::create_dir_all(plugin.join("ui")).unwrap();
        let binary = format!("fixture{}", std::env::consts::EXE_SUFFIX);
        fs::copy(env!("CARGO_BIN_EXE_plugin-fixture"), plugin.join(&binary)).unwrap();
        fs::write(plugin.join("ui/index.html"), "<p>fixture</p>").unwrap();
        fs::write(plugin.join("plugin.json"),json!({"id":"test.ui","name":"UI fixture","version":"0.1.0","apiVersion":"1","target":TARGET,"entry":binary,"ui":{"apiVersion":"1","entry":"ui/index.html","views":[
            {"id":"browser","title":"Files","icon":"folder","container":"sidebar","instancePolicy":"perWorkspace","defaultPinned":true,"openOnWorkspace":"reveal"},
            {"id":"terminal","title":"Terminal","icon":"terminal","container":"editor","instancePolicy":"multiple","openOnWorkspace":"createOne","startupOrder":20},
            {"id":"editor","title":"Editor","icon":"log","container":"editor","instancePolicy":"perResource"}
        ]}}).to_string()).unwrap();
        let core = AppCore::open(root.join("data"), "test".into()).unwrap();
        Self { root, plugin, core }
    }
    fn open(&self, name: &str) -> sodeck::workspaces::Workspace {
        let catalog = self.core.server_catalog();
        let result=self.core.change_catalog(catalog["revision"].as_u64().unwrap(),serde_json::from_value(json!({"op":"saveServer","draft":{"name":name,"host":"same.invalid","port":22,"username":"ops","authentication":{"type":"password","secret":{"action":"clear"}},"route":[]}})).unwrap()).unwrap();
        self.core
            .accept_connected(ConnectedSession {
                server_id: result["selectedId"].as_str().unwrap().into(),
                connection_id: name.into(),
                endpoint: "ops@same.invalid:22".into(),
            })
            .unwrap()
    }
    fn surface(&self) -> String {
        self.load().unwrap();
        let workspace = self.open("RPC");
        let state = self.core.ui_snapshot("main").unwrap();
        let view = state["workspaces"][&workspace.context.workspace_id]["left"]
            .as_str()
            .unwrap();
        let result = self.core.ui_attach("main", view).unwrap();
        let token = result["surfaceId"].as_str().unwrap();
        self.core.ui_surface_ready("main", token).unwrap();
        token.into()
    }
    fn finish(&self, plan: &Value, decision: &str) -> Result<(), String> {
        self.core.ui_close_finish(
            plan["token"].as_str().unwrap(),
            serde_json::from_value(plan["views"].clone()).unwrap(),
            decision,
        )
    }
}
#[test]
fn declared_behavior_chooses_own_page_and_deduplicates_without_a_surface() {
    let f = Fixture::new();
    f.actions();
    let plugin = f.load().unwrap();
    let w = f.open("action");
    let snapshot = f.core.ui_snapshot("main").unwrap();
    assert_eq!(snapshot["defaultCreateAction"], "test.ui:create");
    assert_eq!(snapshot["actions"]["test.ui:create"]["available"], true);
    let config =
        json!({"title":"application terminal","launch":{"kind":"command","command":"example"}});
    let result = f.invoke(&w.context, "create", json!({"params":config}));
    assert_eq!(result["status"], "completed");
    assert_eq!(
        f.invoke(&w.context, "create", json!({"params":config})),
        result
    );
    assert!(f.core.ui_invoke_action("main",&json!({"context":w.context,"requestId":"create","actionKey":"test.ui:create","data":{}})).is_err());
    let process = json!({"expectedContext":w.context,"requestId":"create","viewType":"terminal","params":config});
    assert_eq!(
        f.core
            .ui_process_open("test.ui", plugin.instance_id.as_ref().unwrap(), &process)
            .unwrap()["viewId"],
        result["viewId"]
    );
    let state = f.core.ui_snapshot("main").unwrap();
    let view = state["workspaces"][&w.context.workspace_id]["views"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["id"] == result["viewId"])
        .unwrap();
    assert_eq!(view["params"], config);
    assert!(f
        .core
        .ui_process_open("test.ui", "other-instance", &process)
        .is_err());
    let mut conflict = process.clone();
    conflict["params"] = json!({"different":true});
    assert!(f
        .core
        .ui_process_open("test.ui", plugin.instance_id.as_ref().unwrap(), &conflict)
        .is_err());
    let plan = f
        .core
        .ui_close_prepare(
            "main",
            vec![w.context.workspace_id.clone()],
            Some(result["viewId"].as_str().unwrap().into()),
        )
        .unwrap();
    f.finish(&plan, "discard").unwrap();
    assert!(f
        .core
        .ui_process_open("test.ui", plugin.instance_id.as_ref().unwrap(), &process)
        .unwrap_err()
        .contains("viewClosed"));
}
#[test]
fn behavior_can_return_no_page_or_select_a_different_declared_view() {
    let f = Fixture::new();
    f.actions();
    f.load().unwrap();
    let w = f.open("no-view");
    let before = f.core.ui_snapshot("main").unwrap()["workspaces"][&w.context.workspace_id]
        ["views"]
        .as_array()
        .unwrap()
        .len();
    assert_eq!(
        f.invoke(&w.context, "none", json!({"none":true}))["status"],
        "completed"
    );
    assert_eq!(
        f.core.ui_snapshot("main").unwrap()["workspaces"][&w.context.workspace_id]["views"]
            .as_array()
            .unwrap()
            .len(),
        before
    );
    let result = f.invoke(&w.context, "sidebar", json!({"viewType":"browser"}));
    assert_eq!(result["status"], "completed");
    assert_eq!(
        result["viewId"],
        f.core.ui_snapshot("main").unwrap()["workspaces"][&w.context.workspace_id]["left"]
    );
    let other = f.open("other");
    let other_view = f.core.ui_snapshot("main").unwrap()["workspaces"][&other.context.workspace_id]
        ["editor"]
        .clone();
    assert_eq!(
        f.invoke(
            &w.context,
            "wrong-context",
            json!({"returnViewId":other_view})
        )["status"],
        "failed"
    );
}
#[test]
fn behavior_error_keeps_the_page_that_was_already_committed() {
    let f = Fixture::new();
    f.actions();
    f.load().unwrap();
    let w = f.open("partial-result");
    let result = f.invoke(&w.context, "failure", json!({"failAfterOpen":true}));
    assert_eq!(result["status"], "failed");
    assert!(result["viewId"].is_string());
    assert_eq!(
        f.invoke(&w.context, "failure", json!({"failAfterOpen":true})),
        result
    );
}
#[test]
fn timeout_status_queries_original_delivery_and_late_creation_is_fenced_by_close() {
    let f = Fixture::new();
    f.actions();
    f.load().unwrap();
    let w = f.open("timeout");
    let result = f.invoke(&w.context, "late", json!({"delayMs":6100}));
    assert_eq!(result["status"], "pending");
    assert_eq!(
        f.invoke(&w.context, "late", json!({"delayMs":6100}))["status"],
        "pending"
    );
    let query = json!({"context":w.context,"requestId":"late"});
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        let status = f.core.ui_action_status("main", &query).unwrap();
        if status["status"] == "completed" {
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    assert_eq!(
        fs::read_to_string(f.root.join("data/plugin-data/test.ui/action-count")).unwrap(),
        "1"
    );
    assert_eq!(
        f.invoke(&w.context, "closed-late", json!({"delayMs":6100}))["status"],
        "pending"
    );
    let count = f.core.ui_snapshot("main").unwrap()["workspaces"][&w.context.workspace_id]["views"]
        .as_array()
        .unwrap()
        .len();
    let plan = f
        .core
        .ui_close_prepare("main", vec![w.context.workspace_id.clone()], None)
        .unwrap();
    f.finish(&plan, "cancel").unwrap();
    assert_eq!(
        f.core
            .ui_action_status(
                "main",
                &json!({"context":w.context,"requestId":"closed-late"})
            )
            .unwrap()["status"],
        "invalidated"
    );
    std::thread::sleep(std::time::Duration::from_millis(1300));
    assert_eq!(
        f.core.ui_snapshot("main").unwrap()["workspaces"][&w.context.workspace_id]["views"]
            .as_array()
            .unwrap()
            .len(),
        count
    );
}
#[test]
fn saved_default_survives_provider_stop_and_unsupported_old_plugin_is_unavailable() {
    let f = Fixture::new();
    f.actions();
    f.load().unwrap();
    let saved = fs::read(f.root.join("data/ui-layout.json")).unwrap();
    f.core.unload("test.ui").unwrap();
    let snapshot = f.core.ui_snapshot("main").unwrap();
    assert_eq!(snapshot["defaultCreateAction"], "test.ui:create");
    assert_eq!(snapshot["actions"]["test.ui:create"]["available"], false);
    assert_eq!(fs::read(f.root.join("data/ui-layout.json")).unwrap(), saved);
    let old = Fixture::new();
    old.actions();
    fs::write(old.plugin.join("mode"), "old-actions").unwrap();
    old.load().unwrap();
    let snapshot = old.core.ui_snapshot("main").unwrap();
    assert_eq!(snapshot["actions"]["test.ui:create"]["available"], false);
    assert!(snapshot["defaultCreateAction"].is_null());
}
#[test]
fn action_declaration_validates_ids_titles_versions_and_default_count() {
    let f = Fixture::new();
    let path = f.plugin.join("plugin.json");
    let original: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for actions in [
        json!([{"id":"bad:id","title":"x","dataVersion":1}]),
        json!([{"id":"a","title":" ","dataVersion":1}]),
        json!([{"id":"a","title":"x","dataVersion":0}]),
        json!([{"id":"a","title":"x","dataVersion":1},{"id":"a","title":"y","dataVersion":1}]),
        json!([{"id":"a","title":"x","dataVersion":1,"defaultCreate":true},{"id":"b","title":"y","dataVersion":1,"defaultCreate":true}]),
    ] {
        let mut m = original.clone();
        m["ui"]["actions"] = actions;
        fs::write(&path, m.to_string()).unwrap();
        assert!(
            serde_json::from_value::<sodeck::component_ui::Declaration>(m["ui"].clone())
                .unwrap()
                .validate(&f.plugin.canonicalize().unwrap())
                .is_err()
        );
    }
    fs::write(path, original.to_string()).unwrap();
    assert!(
        serde_json::from_value::<sodeck::component_ui::Declaration>(original["ui"].clone())
            .unwrap()
            .validate(&f.plugin.canonicalize().unwrap())
            .is_ok()
    );
}
#[test]
fn startup_default_is_stable_across_saved_plugin_order_and_queries_do_not_write() {
    let f = Fixture::new();
    f.actions();
    let second = f.root.join("second");
    fs::create_dir_all(second.join("ui")).unwrap();
    fs::copy(
        env!("CARGO_BIN_EXE_plugin-fixture"),
        second.join(format!("fixture{}", std::env::consts::EXE_SUFFIX)),
    )
    .unwrap();
    fs::write(second.join("ui/index.html"), "fixture").unwrap();
    let mut manifest: Value =
        serde_json::from_slice(&fs::read(f.plugin.join("plugin.json")).unwrap()).unwrap();
    manifest["id"] = json!("aaa.default");
    fs::write(second.join("plugin.json"), manifest.to_string()).unwrap();
    f.core.add_plugin(&f.plugin).unwrap();
    f.core.add_plugin(&second).unwrap();
    {
        let _startup = STARTUP.lock().unwrap();
        f.core.start_enabled();
    }
    let path = f.root.join("data/ui-layout.json");
    let saved = fs::read(&path).unwrap();
    for _ in 0..2 {
        assert_eq!(
            f.core.ui_snapshot("main").unwrap()["defaultCreateAction"],
            "aaa.default:create"
        );
    }
    assert_eq!(fs::read(path).unwrap(), saved);
}
#[test]
fn accepted_behavior_follows_workspace_ownership_and_disconnect_fences_new_creation() {
    let f = Fixture::new();
    f.actions();
    f.load().unwrap();
    let w = f.open("moving-action");
    let core = f.core.clone();
    let context = w.context.clone();
    let action = std::thread::spawn(move || {
        core.ui_invoke_action("main",&json!({"context":context,"requestId":"moving","actionKey":"test.ui:create","data":{"delayMs":500}})).unwrap()
    });
    let marker = f.root.join("data/plugin-data/test.ui/action-count");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while !marker.exists() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(f
        .core
        .ui_invoke_action(
            "main",
            &json!({"context":w.context,"requestId":"second","actionKey":"test.ui:create"})
        )
        .is_err());
    let movement = f
        .core
        .prepare_workspace_move(
            "main",
            &w.context.workspace_id,
            "moved",
            0,
            "action-move".into(),
            true,
        )
        .unwrap();
    f.core.workspace_state().ready("moved").unwrap();
    f.core.commit_workspace_move(&movement).unwrap();
    f.core.workspace_changed();
    let result = action.join().unwrap();
    assert_eq!(result["status"], "completed");
    let query = json!({"context":w.context,"requestId":"moving"});
    assert!(f.core.ui_action_status("main", &query).is_err());
    assert_eq!(
        f.core.ui_action_status("moved", &query).unwrap()["viewId"],
        result["viewId"]
    );
    f.core.workspace_state().disconnected(&w.context).unwrap();
    f.core.workspace_changed();
    assert!(f
        .core
        .ui_invoke_action(
            "moved",
            &json!({"context":w.context,"requestId":"offline","actionKey":"test.ui:create"})
        )
        .is_err());
    assert_eq!(
        f.core.ui_action_status("moved", &query).unwrap()["status"],
        "completed"
    );
    let next = f
        .core
        .workspace_state()
        .reconnect(&w.context, "next".into(), "next".into())
        .unwrap();
    f.core.workspace_changed();
    assert!(f.core.ui_action_status("moved", &query).is_err());
    assert!(f
        .core
        .ui_action_status("moved", &json!({"context":next,"requestId":"moving"}))
        .is_err());
    {
        let mut state = f.core.workspace_state();
        state.ready("main").unwrap();
        let m = state
            .prepare(
                "moved",
                &w.context.workspace_id,
                "main",
                0,
                "action-return".into(),
            )
            .unwrap();
        state.commit(&m).unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let ids = self.core.workspace_state().ids();
        if let Ok(plan) = self.core.ui_close_prepare("main", ids, None) {
            let _ = self.finish(&plan, "discard");
        }
        let _ = self.core.shutdown();
        let _ = fs::remove_dir_all(&self.root);
    }
}
#[test]
fn plugin_request_can_reenter_the_same_plugin_without_blocking_its_response_reader() {
    use std::{sync::mpsc, time::Duration};
    let f = Fixture::new();
    f.load().unwrap();
    let workspace = f.open("RPC");
    let state = f.core.ui_snapshot("main").unwrap();
    let view_id = state["workspaces"][&workspace.context.workspace_id]["left"]
        .as_str()
        .unwrap()
        .to_owned();
    let surface = f.core.ui_attach("main", &view_id).unwrap();
    let token = surface["surfaceId"].as_str().unwrap();
    f.core.ui_surface_ready("main", token).unwrap();
    let weak = Arc::downgrade(&f.core);
    let (sent, received) = mpsc::sync_channel(1);
    f.core.set_listener(Arc::new(move |name, payload| {
        if name == "core-message" && payload["title"] == "RPC reentry" {
            let result = weak.upgrade().unwrap().ui_attach("main", &view_id);
            let _ = sent.send(result);
        }
    }));
    let result = f.core.ui_event("main", token, json!({"rpc":"reentry"}));
    let nested = received.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(nested.is_ok(), "nested mount failed: {nested:?}");
    assert_eq!(result.unwrap()["reentered"], true);
}
#[test]
fn concurrent_plugin_responses_keep_their_request_ids_when_completed_out_of_order() {
    use std::{
        thread,
        time::{Duration, Instant},
    };
    let f = Fixture::new();
    let token = f.surface();
    let core = f.core.clone();
    let first_token = token.clone();
    let first = thread::spawn(move || core.ui_event("main", &first_token, json!({"rpc":"defer"})));
    let path = f.root.join("data/plugin-data/test.ui/deferred");
    let deadline = Instant::now() + Duration::from_secs(3);
    while !path.exists() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        f.core
            .ui_event("main", &token, json!({"rpc":"release"}))
            .unwrap()["order"],
        2
    );
    assert_eq!(first.join().unwrap().unwrap()["order"], 1);
}
#[test]
fn missing_plugin_reply_times_out_without_preventing_further_requests() {
    let f = Fixture::new();
    let token = f.surface();
    let started = std::time::Instant::now();
    assert!(f
        .core
        .ui_event("main", &token, json!({"rpc":"ignore"}))
        .unwrap_err()
        .contains("超时"));
    assert!(started.elapsed() < std::time::Duration::from_secs(6));
    assert!(f
        .core
        .ui_event("main", &token, json!({"text":"still usable"}))
        .is_ok());
}
#[test]
fn stopping_a_plugin_cancels_waiting_requests_before_their_timeout() {
    use std::{
        thread,
        time::{Duration, Instant},
    };
    let f = Fixture::new();
    let token = f.surface();
    let core = f.core.clone();
    let waiting = thread::spawn(move || core.ui_event("main", &token, json!({"rpc":"defer"})));
    let path = f.root.join("data/plugin-data/test.ui/deferred");
    let deadline = Instant::now() + Duration::from_secs(3);
    while !path.exists() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    let started = Instant::now();
    f.core.unload("test.ui").unwrap();
    assert!(waiting.join().unwrap().is_err());
    assert!(started.elapsed() < Duration::from_secs(2));
}
#[test]
fn plugin_exit_releases_pending_call_instead_of_waiting_for_its_timeout() {
    let f = Fixture::new();
    let token = f.surface();
    let started = std::time::Instant::now();
    assert!(f
        .core
        .ui_event("main", &token, json!({"rpc":"exit"}))
        .is_err());
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !f.core.snapshot().plugins[0].cleanup_complete {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
#[test]
fn rpc_errors_distinguish_parameters_unknown_methods_and_persistence_failures() {
    let f = Fixture::new();
    let token = f.surface();
    let path = f.root.join("data/messages.json");
    // Initialization publishes one fixture message; wait for its acknowledgement.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while f.core.messages().is_empty() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    let result = f
        .core
        .ui_event("main", &token, json!({"rpc":"probe"}))
        .unwrap();
    assert_eq!(result["probe-invalid"]["error"]["code"], -32602);
    assert_eq!(result["probe-unknown"]["error"]["code"], -32601);
    assert_eq!(result["probe-storage"]["error"]["code"], -32000);
    fs::remove_dir(&path).unwrap();
}
#[test]
fn external_manifest_ui_is_optional_but_invalid_paths_and_duplicates_are_rejected() {
    let f = Fixture::new();
    let mut manifest: Value =
        serde_json::from_slice(&fs::read(f.plugin.join("plugin.json")).unwrap()).unwrap();
    manifest["ui"]["entry"] = json!("../outside.html");
    fs::write(f.plugin.join("plugin.json"), manifest.to_string()).unwrap();
    assert!(f.core.add_plugin(&f.plugin).is_err());
    manifest["ui"]["entry"] = json!("ui/index.html");
    manifest["ui"]["views"][1]["id"] = json!("browser");
    fs::write(f.plugin.join("plugin.json"), manifest.to_string()).unwrap();
    assert!(f.core.add_plugin(&f.plugin).is_err());
    manifest.as_object_mut().unwrap().remove("ui");
    fs::write(f.plugin.join("plugin.json"), manifest.to_string()).unwrap();
    assert!(f.load().is_ok());
}
#[test]
fn defaults_resource_identity_retry_and_move_preserve_view_and_reject_old_surface() {
    let f = Fixture::new();
    let a = f.open("A");
    let b = f.open("B");
    assert_eq!(
        f.core.ui_snapshot("main").unwrap()["definitions"],
        json!({})
    );
    // A plugin arriving after workspace initialization does not steal focus.
    f.load().unwrap();
    let state = f.core.ui_snapshot("main").unwrap();
    assert_eq!(
        state["workspaces"][&a.context.workspace_id]["views"],
        json!([])
    );
    let c = f.open("C");
    let state = f.core.ui_snapshot("main").unwrap();
    let browser = state["workspaces"][&c.context.workspace_id]["left"]
        .as_str()
        .unwrap();
    assert!(f
        .core
        .ui_action("main", &c.context, json!({"op":"select","viewId":browser}))
        .is_ok());
    assert_eq!(
        state["workspaces"][&c.context.workspace_id]["views"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let action = json!({"op":"open","key":"test.ui:editor","resourceKey":"/etc/Case","requestId":"same-click"});
    let one = f
        .core
        .ui_action("main", &a.context, action.clone())
        .unwrap();
    let retry = f
        .core
        .ui_action("main", &a.context, action.clone())
        .unwrap();
    assert_eq!(one, retry);
    assert_ne!(one, f.core.ui_action("main", &b.context, action).unwrap());
    let id = one["viewId"].as_str().unwrap();
    let mount = f.core.ui_attach("main", id).unwrap();
    let old = mount["surfaceId"].as_str().unwrap();
    f.core.ui_surface_ready("main", old).unwrap();
    f.core
        .ui_event("main", old, json!({"text":"draft A"}))
        .unwrap();
    let movement = f
        .core
        .prepare_workspace_move(
            "main",
            &a.context.workspace_id,
            "child",
            0,
            "move".into(),
            true,
        )
        .unwrap();
    f.core.workspace_state().ready("child").unwrap();
    f.core.commit_workspace_move(&movement).unwrap();
    assert!(f
        .core
        .ui_event("main", old, json!({"text":"stale"}))
        .is_err());
    let remount = f.core.ui_attach("child", id).unwrap();
    assert_eq!(remount["data"]["text"], "draft A");
}
#[test]
fn save_failure_cancels_whole_close_and_layout_is_persisted_without_sessions() {
    let f = Fixture::new();
    f.load().unwrap();
    let a = f.open("A");
    f.core.ui_snapshot("main").unwrap();
    let mut ids = vec![];
    for (index, text) in ["draft", "fail"].iter().enumerate() {
        let result=f.core.ui_action("main",&a.context,json!({"op":"open","key":"test.ui:editor","resourceKey":format!("/{index}"),"requestId":format!("open{index}")})).unwrap();
        let id = result["viewId"].as_str().unwrap();
        let mount = f.core.ui_attach("main", id).unwrap();
        let surface = mount["surfaceId"].as_str().unwrap();
        f.core.ui_surface_ready("main", surface).unwrap();
        f.core
            .ui_event("main", surface, json!({"text":text}))
            .unwrap();
        ids.push(id.to_owned());
    }
    let plan = f
        .core
        .ui_close_prepare("main", vec![a.context.workspace_id.clone()], None)
        .unwrap();
    assert_eq!(plan["dirty"].as_array().unwrap().len(), 2);
    assert!(f
        .core
        .ui_action(
            "main",
            &a.context,
            json!({"op":"open","key":"test.ui:terminal","requestId":"during-close"})
        )
        .is_err());
    assert!(f.finish(&plan, "save").is_err());
    let state = f.core.ui_snapshot("main").unwrap();
    assert!(ids
        .iter()
        .all(|id| state["workspaces"][&a.context.workspace_id]["views"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["id"] == *id)));
    f.core
        .ui_action(
            "main",
            &a.context,
            json!({"op":"preference","key":"test.ui:browser","side":"right","pinned":false}),
        )
        .unwrap();
    f.core.ui_state().save_widths(300, 280).unwrap();
    let disk = fs::read_to_string(f.root.join("data/ui-layout.json")).unwrap();
    assert!(disk.contains("right"));
    assert!(!disk.contains("connectionId"));
    assert!(!disk.contains("draft"));
    let restored = sodeck::component_ui::Runtime::open(&f.root.join("data")).unwrap();
    assert_eq!(restored.widths(), (300, 280));
}

fn v2_fixture() -> Fixture {
    let f = Fixture::new();
    let path = f.plugin.join("plugin.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    manifest["ui"]["apiVersion"] = json!("2");
    fs::write(path, manifest.to_string()).unwrap();
    f.load().unwrap();
    f
}
#[test]
fn tool_toggle_and_hide_use_actual_dock_after_moving_without_changing_default_side() {
    let f = v2_fixture();
    let w = f.open("actual-dock");
    let id = f.core.ui_state().workspaces[&w.context.workspace_id]
        .host("main")
        .unwrap()
        .left
        .view_ids[0]
        .clone();
    f.core
        .ui_action(
            "main",
            &w.context,
            json!({"op":"dock","viewId":id,"side":"right"}),
        )
        .unwrap();
    assert_eq!(
        f.core.ui_state().workspaces[&w.context.workspace_id].preferences["test.ui:browser"].side,
        "left"
    );
    f.core
        .ui_action(
            "main",
            &w.context,
            json!({"op":"toggle","key":"test.ui:browser","requestId":"toggle-actual"}),
        )
        .unwrap();
    assert!(f.core.ui_state().workspaces[&w.context.workspace_id]
        .host("main")
        .unwrap()
        .right
        .active_view_id
        .is_none());
    f.core
        .ui_action(
            "main",
            &w.context,
            json!({"op":"open","key":"test.ui:browser","requestId":"reveal-actual"}),
        )
        .unwrap();
    f.core
        .ui_action("main", &w.context, json!({"op":"hideTool","viewId":id}))
        .unwrap();
    let state = f.core.ui_state();
    let host = state.workspaces[&w.context.workspace_id]
        .host("main")
        .unwrap();
    assert!(host.right.view_ids.contains(&id) && host.right.active_view_id.is_none());
    assert!(!host.left.view_ids.contains(&id));
}

#[test]
fn page_split_preserves_only_the_intended_empty_source_and_merge_keeps_instances() {
    for axis in ["x", "y"] {
        let f = v2_fixture();
        let w = f.open("page-split");
        let state = f.core.ui_state();
        let workspace = &state.workspaces[&w.context.workspace_id];
        let host = workspace.host("main").unwrap();
        let original = host.focused_group_id.clone();
        let page = host.groups[&original].view_ids[0].clone();
        let views = workspace.views.clone();
        drop(state);
        let split = f
            .core
            .ui_action(
                "main",
                &w.context,
                json!({"op":"split","axis":axis,"groupId":original,"viewId":page}),
            )
            .unwrap()["groupId"]
            .as_str()
            .unwrap()
            .to_owned();
        let state = f.core.ui_state();
        let host = state.workspaces[&w.context.workspace_id]
            .host("main")
            .unwrap();
        assert_eq!(host.groups.len(), 2);
        assert!(host.groups[&original].view_ids.is_empty());
        assert!(host.groups[&original].explicit_empty);
        assert_eq!(host.groups[&split].view_ids, [page.clone()]);
        let revision = host.revision;
        drop(state);
        assert!(f.core.ui_action("main", &w.context, json!({"op":"mergeGroup","sourceGroupId":split,"groupId":original,"revision":revision-1})).is_err());
        f.core.ui_action("main", &w.context, json!({"op":"mergeGroup","sourceGroupId":split,"groupId":original,"revision":revision})).unwrap();
        let state = f.core.ui_state();
        let workspace = &state.workspaces[&w.context.workspace_id];
        assert_eq!(workspace.host("main").unwrap().groups.len(), 1);
        assert_eq!(
            serde_json::to_value(&workspace.views).unwrap(),
            serde_json::to_value(&views).unwrap()
        );
        drop(state);
        // A populated explicit-empty group becomes ordinary again and prunes on its last move.
        let empty = f
            .core
            .ui_action(
                "main",
                &w.context,
                json!({"op":"split","axis":axis,"groupId":original}),
            )
            .unwrap()["groupId"]
            .as_str()
            .unwrap()
            .to_owned();
        f.core
            .ui_action(
                "main",
                &w.context,
                json!({"op":"moveView","viewId":page,"groupId":empty}),
            )
            .unwrap();
        let state = f.core.ui_state();
        let host = state.workspaces[&w.context.workspace_id]
            .host("main")
            .unwrap();
        assert_eq!(host.groups.len(), 1);
        assert!(!host.groups[&empty].explicit_empty);
        drop(state);
        let key = &views.iter().find(|v| v.id == page).unwrap().definition_key;
        let other = f
            .core
            .ui_action(
                "main",
                &w.context,
                json!({"op":"open","key":key,"groupId":empty,"requestId":"second-page"}),
            )
            .unwrap()["viewId"]
            .as_str()
            .unwrap()
            .to_owned();
        let split = f
            .core
            .ui_action(
                "main",
                &w.context,
                json!({"op":"split","axis":axis,"groupId":empty,"viewId":page}),
            )
            .unwrap()["groupId"]
            .as_str()
            .unwrap()
            .to_owned();
        let state = f.core.ui_state();
        let host = state.workspaces[&w.context.workspace_id]
            .host("main")
            .unwrap();
        assert!(
            !host.groups[&empty].explicit_empty,
            "Moving one of several pages must not pin its source open"
        );
        drop(state);
        f.core
            .ui_action(
                "main",
                &w.context,
                json!({"op":"moveView","viewId":other,"groupId":split}),
            )
            .unwrap();
        assert_eq!(
            f.core.ui_state().workspaces[&w.context.workspace_id]
                .host("main")
                .unwrap()
                .groups
                .len(),
            1
        );
    }
}

#[test]
fn layout_memory_restores_empty_groups_and_tools_without_runtime_identities() {
    let f = v2_fixture();
    let w = f.open("remember-layout");
    let group = f
        .core
        .ui_action("main", &w.context, json!({"op":"split","axis":"y"}))
        .unwrap()["groupId"]
        .as_str()
        .unwrap()
        .to_owned();
    let state = f.core.ui_snapshot("main").unwrap();
    let tool = state["workspaces"][&w.context.workspace_id]["left"]
        .as_str()
        .unwrap();
    f.core
        .ui_action(
            "main",
            &w.context,
            json!({"op":"moveView","viewId":tool,"groupId":group}),
        )
        .unwrap();
    let state = f.core.ui_snapshot("main").unwrap();
    let split = state["workspaces"][&w.context.workspace_id]["host"]["center"]["id"]
        .as_str()
        .unwrap();
    f.core
        .ui_action(
            "main",
            &w.context,
            json!({"op":"splitRatio","splitId":split,"ratio":0.65}),
        )
        .unwrap();
    let path = f.root.join("data/workbench-layout.json");
    let text = fs::read_to_string(&path).unwrap();
    for value in [
        &w.context.workspace_id,
        &w.context.connection_id,
        tool,
        split,
    ] {
        assert!(!text.contains(value));
    }
    let memory = sodeck::workbench_memory::Memory::open(&f.root.join("data"));
    assert!(memory.error.is_none());
    let saved = memory.primary(&w.context.server_id).unwrap();
    let host = saved.seed("new-container".into()).unwrap();
    assert_eq!(host.groups.len(), 2);
    assert!(host.view_ids().is_empty());
    assert_eq!(serde_json::to_value(&host.center).unwrap()["ratio"], 0.65);
    assert!(saved.tools["test.ui:browser"].group.is_some());
    assert!(f.root.join("data/workbench-layout.json.bak").exists());
    let plan = f
        .core
        .ui_close_prepare("main", vec![w.context.workspace_id.clone()], None)
        .unwrap();
    f.finish(&plan, "discard").unwrap();
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        text,
        "server close preserves its last configured containers"
    );
    let reopened = f
        .core
        .accept_connected(ConnectedSession {
            server_id: w.context.server_id,
            connection_id: "new-session".into(),
            endpoint: w.endpoint,
        })
        .unwrap();
    let ui = f.core.ui_snapshot("main").unwrap();
    assert_eq!(
        ui["workspaces"][&reopened.context.workspace_id]["host"]["groups"]
            .as_object()
            .unwrap()
            .len(),
        2
    );
    assert!(ui["workspaces"][&reopened.context.workspace_id]["views"]
        .as_array()
        .unwrap()
        .iter()
        .all(|v| v["resourceKey"].is_null()));
}

#[test]
fn corrupt_or_unknown_layout_is_preserved_and_does_not_disable_workbench() {
    let f = Fixture::new();
    let root = f.root.join("invalid-layout");
    fs::create_dir_all(&root).unwrap();
    for original in ["{broken", "{\"schemaVersion\":99,\"servers\":{}}"] {
        fs::write(root.join("workbench-layout.json"), original).unwrap();
        let mut memory = sodeck::workbench_memory::Memory::open(&root);
        assert!(memory.error.is_some());
        assert!(memory.primary("server").is_none());
        memory
            .capture(
                &[],
                &Default::default(),
                &Default::default(),
                &Default::default(),
            )
            .unwrap();
        assert_eq!(
            fs::read_to_string(root.join("workbench-layout.json")).unwrap(),
            original
        );
    }
}

#[test]
fn closing_one_host_keeps_other_window_pages_and_server_context_alive() {
    use sodeck::{
        lifecycle::{Phase, Target},
        workbench_move::{Destination, Scope},
    };
    let f = v2_fixture();
    let w = f.open("two-hosts");
    let (remote, token) = page(&f, &w.context, "main", "/remote-host", None);
    let (local, _) = page(&f, &w.context, "main", "/local-host", None);
    let m = f
        .core
        .prepare_workbench_move(
            "main",
            &w.context.workspace_id,
            "host-child",
            0,
            "host-move".into(),
            true,
            Scope::View { id: remote.clone() },
            Destination::default(),
        )
        .unwrap();
    finish_transfer(&f, &m, std::slice::from_ref(&remote));
    let remote_surface = f.core.ui_attach("host-child", &remote).unwrap();
    let active = remote_surface["surfaceId"].as_str().unwrap();
    f.core.ui_surface_ready("host-child", active).unwrap();
    f.core
        .ui_event("host-child", active, json!({"text":"child draft"}))
        .unwrap();
    let op = f
        .core
        .close_prepare(
            "main",
            "close-main-host",
            Target::Host {
                id: w.context.workspace_id.clone(),
            },
        )
        .unwrap();
    assert!(op.views.contains(&local));
    assert!(!op.views.contains(&remote));
    f.core
        .close_report_inputs("main", &op.id, op.input_round, json!({}))
        .unwrap();
    let ready = wait_close(&f.core, &op.id, Phase::Deciding);
    let decision = f
        .core
        .close_decide("main", &op.id, ready.decision_revision, "discard")
        .unwrap();
    f.core
        .close_report_inputs("main", &op.id, decision.input_round, json!({}))
        .unwrap();
    // The other window prepares and restores the singleton tools being returned.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(12);
    let result = loop {
        let snapshot = f.core.ui_snapshot("host-child").unwrap();
        for transfer in snapshot["transfers"].as_array().unwrap() {
            for view in transfer["viewIds"].as_array().unwrap() {
                if let Ok(surface) = f.core.ui_attach("host-child", view.as_str().unwrap()) {
                    let _ = f
                        .core
                        .ui_surface_ready("host-child", surface["surfaceId"].as_str().unwrap());
                }
            }
        }
        let current = f.core.close_status("main", &op.id).unwrap();
        if current.phase == Phase::Completed {
            break current;
        }
        assert!(
            current.phase != Phase::Failed && std::time::Instant::now() < deadline,
            "{:?}: {:?}",
            current.phase,
            current.error
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    assert_eq!(result.phase, Phase::Completed);
    f.core
        .workspace_state()
        .validate_owned("host-child", &w.context)
        .unwrap();
    assert!(f
        .core
        .ui_event("main", &token, json!({"text":"retired"}))
        .is_err());
    f.core
        .ui_event("host-child", active, json!({"text":"still usable"}))
        .unwrap();
}

#[test]
fn move_receipt_retries_keep_identity_and_reject_changed_scope_or_target() {
    use sodeck::workbench_move::{Destination, Scope};
    let f = v2_fixture();
    let w = f.open("receipts");
    let (id, _) = page(&f, &w.context, "main", "/receipt", None);
    let m = f
        .core
        .prepare_workbench_move(
            "main",
            &w.context.workspace_id,
            "receipt-child",
            0,
            "receipt".into(),
            true,
            Scope::View { id: id.clone() },
            Destination::default(),
        )
        .unwrap();
    let retry = f
        .core
        .prepare_workbench_move(
            "main",
            &w.context.workspace_id,
            "receipt-child",
            0,
            "receipt".into(),
            true,
            Scope::View { id: id.clone() },
            Destination::default(),
        )
        .unwrap();
    assert_eq!(retry.token, m.token);
    assert_eq!(
        f.core.workbench_move_status("main", "receipt").unwrap()["status"],
        "pending"
    );
    assert!(f
        .core
        .prepare_workbench_move(
            "main",
            &w.context.workspace_id,
            "different",
            0,
            "receipt".into(),
            true,
            Scope::View { id: id.clone() },
            Destination::default()
        )
        .is_err());
    finish_transfer(&f, &m, std::slice::from_ref(&id));
    f.core.commit_workbench_move(&m).unwrap();
    assert_eq!(
        f.core.workbench_move_status("main", "receipt").unwrap()["status"],
        "completed"
    );
    assert!(f
        .core
        .workbench_move_status("unrelated", "receipt")
        .is_err());
}
fn page(
    f: &Fixture,
    c: &sodeck::workspaces::Context,
    window: &str,
    name: &str,
    group: Option<&str>,
) -> (String, String) {
    let opened=f.core.ui_action(window,c,json!({"op":"open","key":"test.ui:editor","resourceKey":name,"requestId":name,"groupId":group})).unwrap();
    let id = opened["viewId"].as_str().unwrap().to_owned();
    let surface = f.core.ui_attach(window, &id).unwrap();
    let token = surface["surfaceId"].as_str().unwrap().to_owned();
    f.core.ui_surface_ready(window, &token).unwrap();
    (id, token)
}
fn finish_transfer(f: &Fixture, m: &sodeck::workspaces::Move, views: &[String]) {
    restore_transfer(f, m, views);
    f.core.commit_workbench_move(m).unwrap();
    f.core.workspace_changed();
}
fn restore_transfer(f: &Fixture, m: &sodeck::workspaces::Move, views: &[String]) {
    f.core.workspace_state().ready(&m.target).unwrap();
    for view in views {
        let s = f.core.ui_attach(&m.target, view).unwrap();
        assert_eq!(s["readOnly"], true);
        f.core
            .ui_surface_ready(&m.target, s["surfaceId"].as_str().unwrap())
            .unwrap();
    }
    f.core.begin_transfer_drain(m).unwrap();
    f.core.transfer_drained(&m.source, &m.token).unwrap();
    for view in views {
        let s = f.core.ui_attach(&m.target, view).unwrap();
        f.core
            .ui_surface_ready(&m.target, s["surfaceId"].as_str().unwrap())
            .unwrap();
    }
}
#[test]
fn window_return_commits_every_host_together_and_failed_member_preserves_all_sources() {
    use sodeck::workbench_move::{Destination, Scope};
    let f = v2_fixture();
    let a = f.open("return-a");
    let b = f.open("return-b");
    for (at, w) in [&a, &b].iter().enumerate() {
        let ids = f.core.ui_state().workspaces[&w.context.workspace_id]
            .host("main")
            .unwrap()
            .view_ids();
        for id in &ids {
            let s = f.core.ui_attach("main", id).unwrap();
            f.core
                .ui_surface_ready("main", s["surfaceId"].as_str().unwrap())
                .unwrap();
        }
        let m = f
            .core
            .prepare_workbench_move(
                "main",
                &w.context.workspace_id,
                "return-child",
                at,
                format!("detach-{at}"),
                at == 0,
                Scope::Host,
                Destination::default(),
            )
            .unwrap();
        finish_transfer(&f, &m, &ids);
    }
    let old: Vec<_> = [&a, &b]
        .iter()
        .map(|w| {
            let ids = f.core.ui_state().workspaces[&w.context.workspace_id]
                .host("return-child")
                .unwrap()
                .view_ids();
            let surface = f.core.ui_attach("return-child", &ids[0]).unwrap();
            f.core
                .ui_surface_ready("return-child", surface["surfaceId"].as_str().unwrap())
                .unwrap();
            (ids, surface["surfaceId"].as_str().unwrap().to_owned())
        })
        .collect();
    let moves = f
        .core
        .prepare_window_return("return-child", "all-fail".into())
        .unwrap();
    assert_eq!(moves.len(), 2);
    assert_eq!(
        f.core
            .workspace_projection("main")
            .unwrap()
            .incoming_workspaces
            .len(),
        2
    );
    restore_transfer(&f, &moves[0], &old[0].0);
    assert!(f.core.commit_workbench_moves(&moves).is_err());
    for (w, (ids, _)) in [&a, &b].iter().zip(&old) {
        assert_eq!(
            f.core.ui_state().workspaces[&w.context.workspace_id]
                .view_window(&ids[0])
                .as_deref(),
            Some("return-child")
        );
    }
    for m in &moves {
        f.core.cancel_workbench_move(m);
    }
    for (_, token) in &old {
        assert!(f
            .core
            .ui_event(
                "return-child",
                token,
                json!({"text":"source still writable"})
            )
            .is_ok());
    }
    let moves = f
        .core
        .prepare_window_return("return-child", "all-ok".into())
        .unwrap();
    for (m, (ids, _)) in moves.iter().zip(&old) {
        restore_transfer(&f, m, ids);
    }
    f.core.commit_workbench_moves(&moves).unwrap();
    f.core.commit_workbench_moves(&moves).unwrap();
    assert!(f
        .core
        .workspace_projection("return-child")
        .unwrap()
        .workspaces
        .is_empty());
    assert_eq!(
        f.core
            .workspace_projection("main")
            .unwrap()
            .workspaces
            .len(),
        2
    );
    assert_eq!(
        f.core
            .workbench_move_status("return-child", "all-ok")
            .unwrap()["status"],
        "completed"
    );
    for (_, token) in &old {
        assert!(f
            .core
            .ui_event("return-child", token, json!({"text":"stale"}))
            .is_err());
    }
}
#[test]
fn return_bookmark_restores_original_group_between_neighboring_tabs_and_tool_dock() {
    use sodeck::workbench_move::{Destination, Scope};
    let f = v2_fixture();
    let w = f.open("return-bookmark");
    let (a, _) = page(&f, &w.context, "main", "/before", None);
    let (b, _) = page(&f, &w.context, "main", "/return", None);
    let (c, _) = page(&f, &w.context, "main", "/after", None);
    let group = f.core.ui_state().workspaces[&w.context.workspace_id]
        .host("main")
        .unwrap()
        .group_of(&b)
        .unwrap();
    let m = f
        .core
        .prepare_workbench_move(
            "main",
            &w.context.workspace_id,
            "book-child",
            0,
            "book-detach".into(),
            true,
            Scope::View { id: b.clone() },
            Destination::default(),
        )
        .unwrap();
    finish_transfer(&f, &m, std::slice::from_ref(&b));
    f.core
        .ui_action("main", &w.context, json!({"op":"split","axis":"y"}))
        .unwrap();
    let scope = Scope::View { id: b.clone() };
    let (target, destination) = f
        .core
        .resolve_workbench_return("book-child", &w.context.workspace_id, &scope)
        .unwrap();
    assert_eq!(target, "main");
    assert_eq!(destination.group_id.as_deref(), Some(group.as_str()));
    let m = f
        .core
        .prepare_workbench_move(
            "book-child",
            &w.context.workspace_id,
            &target,
            0,
            "book-return".into(),
            false,
            scope,
            destination,
        )
        .unwrap();
    finish_transfer(&f, &m, std::slice::from_ref(&b));
    let ids = f.core.ui_state().workspaces[&w.context.workspace_id]
        .host("main")
        .unwrap()
        .groups[&group]
        .view_ids
        .clone();
    let at = ids.iter().position(|id| id == &b).unwrap();
    assert_eq!(&ids[at - 1], &a);
    assert_eq!(&ids[at + 1], &c);
    assert!(!f.core.ui_state().workspaces[&w.context.workspace_id]
        .return_locations
        .contains_key(&b));
    let tool = f.core.ui_state().workspaces[&w.context.workspace_id]
        .host("main")
        .unwrap()
        .left
        .view_ids[0]
        .clone();
    let s = f.core.ui_attach("main", &tool).unwrap();
    f.core
        .ui_surface_ready("main", s["surfaceId"].as_str().unwrap())
        .unwrap();
    let m = f
        .core
        .prepare_workbench_move(
            "main",
            &w.context.workspace_id,
            "book-child",
            0,
            "tool-detach".into(),
            false,
            Scope::View { id: tool.clone() },
            Destination::default(),
        )
        .unwrap();
    finish_transfer(&f, &m, std::slice::from_ref(&tool));
    let (_, destination) = f
        .core
        .resolve_workbench_return(
            "book-child",
            &w.context.workspace_id,
            &Scope::View { id: tool },
        )
        .unwrap();
    assert_eq!(destination.side.as_deref(), Some("left"));
}
#[test]
fn group_transfer_keeps_group_identity_tab_order_and_return_bookmark_in_an_empty_window() {
    use sodeck::workbench_move::{Destination, Scope};
    let f = v2_fixture();
    let w = f.open("group-identity");
    let group = f
        .core
        .ui_action("main", &w.context, json!({"op":"split","axis":"y"}))
        .unwrap()["groupId"]
        .as_str()
        .unwrap()
        .to_owned();
    let (a, _) = page(&f, &w.context, "main", "/group-a", Some(&group));
    let (b, _) = page(&f, &w.context, "main", "/group-b", Some(&group));
    let m = f
        .core
        .prepare_workbench_move(
            "main",
            &w.context.workspace_id,
            "group-child",
            0,
            "whole-group".into(),
            true,
            Scope::Group { id: group.clone() },
            Destination::default(),
        )
        .unwrap();
    finish_transfer(&f, &m, &[a.clone(), b.clone()]);
    let ui = f.core.ui_state().workspaces[&w.context.workspace_id].clone();
    assert!(!ui.host("main").unwrap().groups.contains_key(&group));
    assert_eq!(
        ui.host("group-child").unwrap().groups[&group].view_ids,
        vec![a, b.clone()]
    );
    assert_eq!(
        ui.host("group-child").unwrap().groups[&group]
            .active_view_id
            .as_deref(),
        Some(b.as_str())
    );
    assert_eq!(ui.group_return_locations[&group].window_id, "main");
}
#[test]
fn v2_page_transfer_keeps_draft_denies_candidate_input_and_retires_old_surface() {
    use sodeck::workbench_move::{Destination, Scope};
    let f = v2_fixture();
    let w = f.open("transfer");
    let (id, old) = page(&f, &w.context, "main", "/draft", None);
    f.core
        .ui_event("main", &old, json!({"text":"unsaved"}))
        .unwrap();
    let m = f
        .core
        .prepare_workbench_move(
            "main",
            &w.context.workspace_id,
            "child",
            0,
            "transfer".into(),
            true,
            Scope::View { id: id.clone() },
            Destination::default(),
        )
        .unwrap();
    let candidate = f.core.ui_attach("child", &id).unwrap();
    let token = candidate["surfaceId"].as_str().unwrap();
    f.core.ui_surface_ready("child", token).unwrap();
    assert_eq!(candidate["data"]["text"], "unsaved");
    assert!(f
        .core
        .ui_event("child", token, json!({"text":"wrong"}))
        .is_err());
    assert!(f.core.commit_workbench_move(&m).is_err());
    finish_transfer(&f, &m, std::slice::from_ref(&id));
    assert!(f
        .core
        .ui_event("main", &old, json!({"text":"stale"}))
        .is_err());
    let current = f.core.ui_attach("child", &id).unwrap();
    assert_eq!(current["data"]["text"], "unsaved");
    assert_eq!(current["readOnly"], false);
    assert!(f
        .root
        .join(format!("data/plugin-data/test.ui/created-{id}"))
        .exists());
    assert_eq!(
        f.core.workspace_projection("main").unwrap().workspaces[0].context,
        w.context
    );
    assert_eq!(
        f.core.workspace_projection("child").unwrap().workspaces[0].context,
        w.context
    );
}
#[test]
fn disjoint_groups_remain_editable_and_commit_preserves_their_changes() {
    use sodeck::workbench_move::{Destination, Scope};
    let f = v2_fixture();
    let w = f.open("groups");
    let (a, _) = page(&f, &w.context, "main", "/a", None);
    let group = f
        .core
        .ui_action("main", &w.context, json!({"op":"split","axis":"x"}))
        .unwrap()["groupId"]
        .as_str()
        .unwrap()
        .to_owned();
    let (b, _) = page(&f, &w.context, "main", "/b", Some(&group));
    let m = f
        .core
        .prepare_workbench_move(
            "main",
            &w.context.workspace_id,
            "child",
            0,
            "moving-a".into(),
            true,
            Scope::View { id: a.clone() },
            Destination::default(),
        )
        .unwrap();
    let (c, _) = page(&f, &w.context, "main", "/c", Some(&group));
    assert!(f
        .core
        .ui_action(
            "main",
            &w.context,
            json!({"op":"moveView","viewId":a,"groupId":group})
        )
        .is_err());
    finish_transfer(&f, &m, std::slice::from_ref(&a));
    let state = f.core.ui_state();
    let host = state.workspaces[&w.context.workspace_id]
        .host("main")
        .unwrap();
    assert!(host.contains(&b) && host.contains(&c) && !host.contains(&a));
    host.validate().unwrap();
}
#[test]
fn lost_secondary_recovers_the_same_view_without_recreating_the_plugin_instance() {
    use sodeck::workbench_move::{Destination, Scope};
    let f = v2_fixture();
    let w = f.open("recovery");
    let (id, old) = page(&f, &w.context, "main", "/recover", None);
    f.core
        .ui_event("main", &old, json!({"text":"preserved"}))
        .unwrap();
    let m = f
        .core
        .prepare_workbench_move(
            "main",
            &w.context.workspace_id,
            "child",
            0,
            "recover".into(),
            true,
            Scope::View { id: id.clone() },
            Destination::default(),
        )
        .unwrap();
    finish_transfer(&f, &m, std::slice::from_ref(&id));
    let before = f.core.ui_attach("child", &id).unwrap();
    let instance = before["view"]["pluginInstanceId"].clone();
    f.core.close_host_lost("child");
    let recovered = f.core.ui_attach("main", &id).unwrap();
    assert_eq!(recovered["data"]["text"], "preserved");
    assert_eq!(recovered["view"]["pluginInstanceId"], instance);
    assert!(f.core.workspace_projection("child").is_err());
}
#[test]
fn generation_change_fences_a_prepared_transfer_and_cancel_preserves_source() {
    use sodeck::workbench_move::{Destination, Scope};
    let f = v2_fixture();
    let w = f.open("generation");
    let (id, _) = page(&f, &w.context, "main", "/generation", None);
    let m = f
        .core
        .prepare_workbench_move(
            "main",
            &w.context.workspace_id,
            "child",
            0,
            "generation".into(),
            true,
            Scope::View { id: id.clone() },
            Destination::default(),
        )
        .unwrap();
    f.core.workspace_state().ready("child").unwrap();
    let s = f.core.ui_attach("child", &id).unwrap();
    f.core
        .ui_surface_ready("child", s["surfaceId"].as_str().unwrap())
        .unwrap();
    f.core.begin_transfer_drain(&m).unwrap();
    f.core.transfer_drained("main", &m.token).unwrap();
    let s = f.core.ui_attach("child", &id).unwrap();
    f.core
        .ui_surface_ready("child", s["surfaceId"].as_str().unwrap())
        .unwrap();
    f.core.workspace_state().disconnected(&w.context).unwrap();
    f.core
        .workspace_state()
        .reconnect(&w.context, "next".into(), "next".into())
        .unwrap();
    assert!(f.core.commit_workbench_move(&m).is_err());
    f.core.cancel_workbench_move(&m);
    assert_eq!(
        f.core.ui_state().workspaces[&w.context.workspace_id]
            .view_window(&id)
            .as_deref(),
        Some("main")
    );
}

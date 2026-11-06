use serde_json::{json, Value};
use sodeck::{settings::declaration::Page, AppCore, TARGET};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
static SEQUENCE: AtomicUsize = AtomicUsize::new(0);
struct Fixture {
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "sodeck-settings-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("plugin/ui")).unwrap();
        fs::write(
            root.join("plugin/entry"),
            "disabled fixture; never executed",
        )
        .unwrap();
        fs::write(root.join("plugin/ui/index.html"), "settings fixture").unwrap();
        Self { root }
    }
    fn definition(&self, version: &str, fields: Value) {
        fs::write(self.root.join("plugin/plugin.json"), json!({"id":"test.settings","name":"设置夹具","version":version,"apiVersion":"1","target":TARGET,"entry":"entry","ui":{"apiVersion":"1","entry":"ui/index.html","views":[{"id":"test","title":"test","icon":"info","container":"editor","instancePolicy":"multiple"}]},"settings":{"apiVersion":"1","schemaVersion":1,"pages":[{"id":"appearance","title":"外观","viewTypes":["test"],"groups":[{"id":"text","title":"文字","fields":fields}]}]}}).to_string()).unwrap();
    }
    fn open(&self) -> std::sync::Arc<AppCore> {
        AppCore::open(self.root.join("data"), "test".into()).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn field() -> Value {
    json!({"key":"size","title":"字号","type":"number","min":11,"max":24,"step":1,"default":14,"apply":"live"})
}
fn save(core: &std::sync::Arc<AppCore>, page: &Value, request: &str, values: Value) -> Value {
    core.save_settings("test.settings", "appearance", json!({"definitionToken":page["definitionToken"],"baseRevision":page["revision"],"requestId":request,"values":values})).unwrap()
}
#[test]
fn disabled_plugin_save_reopen_conflict_and_deduplication() {
    let f = Fixture::new();
    f.definition("1.0.0", json!([field()]));
    let core = f.open();
    core.add_plugin(&f.root.join("plugin")).unwrap();
    let first = core.settings_page("test.settings", "appearance").unwrap();
    assert_eq!(first["values"]["size"], 14);
    let result = save(&core, &first, "request-1", json!({"size":18}));
    assert_eq!(result["snapshot"]["values"]["size"], 18);
    assert_eq!(result, save(&core, &first, "request-1", json!({"size":18})));
    assert!(save(&core, &first, "stale", json!({"size":20}))["conflict"] == true);
    assert!(core
        .save_settings(
            "test.settings",
            "appearance",
            json!({"requestId":"invalid","values":{"size":25}})
        )
        .is_err());
    assert!(core.snapshot().plugins.iter().all(|p| p.pid.is_none()));
    drop(core);
    let core = f.open();
    assert_eq!(
        core.settings_page("test.settings", "appearance").unwrap()["values"]["size"],
        18
    );
}
#[test]
fn compatible_upgrade_and_invalid_original_are_preserved() {
    let f = Fixture::new();
    f.definition("1.0.0", json!([field()]));
    let core = f.open();
    core.add_plugin(&f.root.join("plugin")).unwrap();
    let first = core.settings_page("test.settings", "appearance").unwrap();
    let saved = save(&core, &first, "one", json!({"size":20}));
    let mut smaller = field();
    smaller["max"] = json!(18);
    smaller["default"] = json!(16);
    f.definition("1.1.0", json!([smaller,{"key":"blink","type":"boolean","title":"闪烁","default":true,"apply":"live"}]));
    core.set_enabled("test.settings", false).unwrap();
    core.forget("test.settings").unwrap();
    core.add_plugin(&f.root.join("plugin")).unwrap();
    let next = core.settings_page("test.settings", "appearance").unwrap();
    assert_eq!(next["savedValues"]["size"], 20);
    assert_eq!(next["values"]["size"], 16);
    assert_eq!(next["values"]["blink"], true);
    assert!(!next["warnings"].as_array().unwrap().is_empty());
    assert!(next["definitionToken"] != first["definitionToken"]);
    assert!(
        save(
            &core,
            &saved["snapshot"],
            "old-definition",
            json!({"size":17,"blink":true})
        )["conflict"]
            == true
    );
    f.definition("1.0.0", json!([field()]));
    core.set_enabled("test.settings", false).unwrap();
    core.forget("test.settings").unwrap();
    core.add_plugin(&f.root.join("plugin")).unwrap();
    assert_eq!(
        core.settings_page("test.settings", "appearance").unwrap()["values"]["size"],
        20
    );
}
#[test]
fn corrupt_main_recovers_backup_and_failed_write_keeps_memory() {
    let f = Fixture::new();
    f.definition("1.0.0", json!([field()]));
    let core = f.open();
    core.add_plugin(&f.root.join("plugin")).unwrap();
    let first = core.settings_page("test.settings", "appearance").unwrap();
    let a = save(&core, &first, "one", json!({"size":18}));
    save(&core, &a["snapshot"], "two", json!({"size":20}));
    drop(core);
    fs::write(f.root.join("data/component-settings.json"), "broken").unwrap();
    let core = f.open();
    let recovered = core.settings_page("test.settings", "appearance").unwrap();
    assert_eq!(recovered["values"]["size"], 18);
    assert!(!core.snapshot().storage_errors.is_empty());
    save(&core, &recovered, "recovered", json!({"size":19}));
    assert!(fs::read_dir(f.root.join("data")).unwrap().any(|p| p
        .unwrap()
        .file_name()
        .to_string_lossy()
        .contains(".corrupt-")));
    fs::create_dir(f.root.join("data/component-settings.json.tmp")).unwrap();
    let before = core.settings_page("test.settings", "appearance").unwrap();
    assert!(core.save_settings("test.settings","appearance",json!({"definitionToken":before["definitionToken"],"baseRevision":before["revision"],"requestId":"fail","values":{"size":21}})).is_err());
    assert_eq!(
        core.settings_page("test.settings", "appearance").unwrap()["values"]["size"],
        19
    );
}
#[test]
fn declaration_rejects_unknown_types_invalid_steps_and_defaults() {
    let mut raw = json!({"id":"a","title":"A","viewTypes":["test"],"groups":[{"id":"g","title":"G","fields":[field()]}]});
    let p: Page = serde_json::from_value(raw.clone()).unwrap();
    assert!(p.validate().is_ok());
    raw["groups"][0]["fields"][0]["step"] = json!(0);
    assert!(serde_json::from_value::<Page>(raw.clone())
        .unwrap()
        .validate()
        .is_err());
    raw["groups"][0]["fields"][0]["type"] = json!("script");
    assert!(serde_json::from_value::<Page>(raw)
        .unwrap()
        .validate()
        .is_err());
}

#[cfg(feature = "test-fixtures")]
#[test]
fn startup_settings_rpc_has_no_lifecycle_lock_and_changed_has_request_id() {
    let f = Fixture::new();
    f.definition("1.0.0", json!([field()]));
    let exe = format!("fixture{}", std::env::consts::EXE_SUFFIX);
    fs::copy(
        env!("CARGO_BIN_EXE_plugin-fixture"),
        f.root.join("plugin").join(&exe),
    )
    .unwrap();
    let path = f.root.join("plugin/plugin.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    manifest["entry"] = json!(exe);
    fs::write(path, manifest.to_string()).unwrap();
    fs::write(f.root.join("plugin/mode"), "settings-init").unwrap();
    let core = f.open();
    core.add_plugin(&f.root.join("plugin")).unwrap();
    drop(core);
    let core = f.open();
    core.start_enabled();
    assert_eq!(
        core.snapshot().plugins[0].status,
        sodeck::model::PluginStatus::Running
    );
    let init: Value = serde_json::from_slice(
        &fs::read(
            f.root
                .join("data/plugin-data/test.settings/settings-init.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(init["result"]["values"]["size"], 14);
    let page = core.settings_page("test.settings", "appearance").unwrap();
    save(&core, &page, "changed", json!({"size":17}));
    let changed = f
        .root
        .join("data/plugin-data/test.settings/settings-changed.json");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !changed.exists() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let message: Value = serde_json::from_slice(&fs::read(changed).unwrap()).unwrap();
    assert_eq!(message["values"]["size"], 17);
    core.shutdown().unwrap();
}

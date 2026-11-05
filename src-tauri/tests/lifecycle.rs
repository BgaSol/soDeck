#![cfg(feature = "test-fixtures")]
use serde_json::json;
use sodeck::{
    model::{LogFilter, PluginStatus},
    AppCore, TARGET,
};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
    thread,
    time::{Duration, Instant},
};
static SEQUENCE: AtomicUsize = AtomicUsize::new(0);
struct Fixture {
    root: PathBuf,
    plugin: PathBuf,
}
impl Fixture {
    fn new(mode: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "sodeck-test-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let plugin = root.join("external plugin");
        fs::create_dir_all(&plugin).unwrap();
        let binary = format!("fixture{}", std::env::consts::EXE_SUFFIX);
        fs::copy(env!("CARGO_BIN_EXE_plugin-fixture"), plugin.join(&binary)).unwrap();
        fs::write(plugin.join("plugin.json"),json!({"id":"test.fixture","name":"Fixture","version":"0.1.0","apiVersion":"1","target":TARGET,"entry":binary}).to_string()).unwrap();
        fs::write(plugin.join("mode"), mode).unwrap();
        Self { root, plugin }
    }
    fn open(&self) -> std::sync::Arc<AppCore> {
        AppCore::open(self.root.join("data"), "test".into()).unwrap()
    }
    fn pid(&self, file: &str) -> u32 {
        fs::read_to_string(self.root.join("data/plugin-data/test.fixture").join(file))
            .unwrap()
            .parse()
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn wait_for(mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(8);
    while !check() {
        assert!(Instant::now() < deadline, "condition timed out");
        thread::sleep(Duration::from_millis(20));
    }
}
#[cfg(windows)]
fn alive(pid: u32) -> bool {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
    };
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return false;
        }
        let mut code = 0;
        let ok = GetExitCodeProcess(h, &mut code);
        CloseHandle(h);
        ok != 0 && code == 259
    }
}
#[cfg(unix)]
fn alive(pid: u32) -> bool {
    if cfg!(target_os = "linux")
        && fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|s| {
            s.rsplit_once(')')
                .is_some_and(|(_, s)| s.trim_start().starts_with('Z'))
        })
    {
        return false;
    }
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

#[test]
fn load_stop_reload_and_persist() {
    let f = Fixture::new("");
    let core = f.open();
    let first = core.load(&f.plugin).unwrap();
    let initialization: serde_json::Value = serde_json::from_slice(
        &fs::read(f.root.join("data/plugin-data/test.fixture/initialize.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(initialization["coreVersion"], "test");
    assert!(initialization["capabilities"]
        .as_array()
        .unwrap()
        .contains(&json!("rpc.concurrent")));
    wait_for(|| !core.messages().is_empty());
    assert_eq!(
        core.messages().len(),
        1,
        "error log must not create a message"
    );
    let msg = core.messages()[0].clone();
    assert_eq!(msg.plugin_id.as_deref(), Some("test.fixture"));
    assert_eq!(msg.instance_id, first.instance_id);
    assert!(core.load(&f.plugin).is_err());
    let pid = f.pid("pid");
    core.unload("test.fixture").unwrap();
    assert!(!alive(pid));
    assert!(core
        .logs(&LogFilter::default())
        .iter()
        .any(|l| l.message == "fixture stopping"));
    let second = core.load(&f.plugin).unwrap();
    assert_ne!(first.instance_id, second.instance_id);
    wait_for(|| core.messages()[0].count == 2);
    core.unload("test.fixture").unwrap();
    core.mark_read(None).unwrap();
    core.set_theme("dark").unwrap();
    core.shutdown().unwrap();
    drop(core);
    let reopened = f.open();
    assert_eq!(
        reopened.snapshot().plugins[0].status,
        PluginStatus::Unloaded
    );
    assert!(reopened.messages()[0].read);
    assert_eq!(reopened.snapshot().preferences.theme, "dark");
    reopened.forget("test.fixture").unwrap();
    assert!(f.plugin.exists());
    assert!(!reopened.messages().is_empty());
}
#[test]
fn invalid_manifest_and_api_are_rejected_without_starting() {
    let f = Fixture::new("");
    let core = f.open();
    for (key, value) in [
        ("apiVersion", "2"),
        ("target", "wrong"),
        ("entry", "../escape"),
    ] {
        let path = f.plugin.join("plugin.json");
        let original = fs::read(&path).unwrap();
        let mut manifest: serde_json::Value = serde_json::from_slice(&original).unwrap();
        manifest[key] = json!(value);
        fs::write(&path, manifest.to_string()).unwrap();
        assert!(core.load(&f.plugin).is_err());
        assert!(core.snapshot().plugins.is_empty());
        fs::write(&path, original).unwrap();
    }
}
#[test]
fn timeout_and_ignored_shutdown_are_bounded() {
    for mode in ["timeout", "no-read", "ignore-stop"] {
        let f = Fixture::new(mode);
        let core = f.open();
        let start = Instant::now();
        if mode == "ignore-stop" {
            core.load(&f.plugin).unwrap();
            core.unload("test.fixture").unwrap();
        } else {
            assert!(core.load(&f.plugin).is_err());
        }
        assert!(
            start.elapsed() < Duration::from_secs(7),
            "mode={mode}, elapsed={:?}, status={:?}",
            start.elapsed(),
            core.snapshot().plugins[0].error
        );
        if mode != "no-read" {
            assert!(!alive(f.pid("pid")));
        }
        assert!(core.snapshot().plugins[0].cleanup_complete);
        core.shutdown().unwrap();
    }
}
#[test]
fn shutdown_cleans_managed_children() {
    let f = Fixture::new("child");
    let core = f.open();
    core.load(&f.plugin).unwrap();
    let parent = f.pid("pid");
    let child = f.pid("child-pid");
    assert!(alive(child));
    core.shutdown().unwrap();
    wait_for(|| !alive(parent) && !alive(child));
}
#[test]
fn crashes_and_broken_protocol_leave_core_usable() {
    for mode in ["crash", "oversized", "write-flood"] {
        let f = Fixture::new(mode);
        let core = f.open();
        let _ = core.load(&f.plugin);
        wait_for(|| core.snapshot().plugins[0].cleanup_complete);
        assert_eq!(core.snapshot().plugins[0].status, PluginStatus::Failed);
        assert!(!alive(f.pid("pid")));
        fs::write(f.plugin.join("mode"), "").unwrap();
        core.load(&f.plugin).unwrap();
        core.shutdown().unwrap();
    }
}
#[test]
fn output_flood_does_not_block_shutdown() {
    let f = Fixture::new("flood");
    let core = f.open();
    core.load(&f.plugin).unwrap();
    thread::sleep(Duration::from_millis(150));
    let start = Instant::now();
    core.shutdown().unwrap();
    assert!(start.elapsed() < Duration::from_secs(6));
    assert!(!alive(f.pid("pid")));
}
#[test]
fn corrupt_and_unwritable_storage_preserves_original() {
    let f = Fixture::new("");
    fs::create_dir_all(f.root.join("data")).unwrap();
    let path = f.root.join("data/preferences.json");
    fs::write(&path, "broken json").unwrap();
    let core = f.open();
    assert!(core.set_theme("dark").is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), "broken json");
    assert!(!core.snapshot().storage_errors.is_empty());
    drop(core);
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    let core = f.open();
    assert!(core.set_theme("dark").is_err());
    core.shutdown().unwrap();
}

#[test]
fn retired_examples_are_removed_before_startup_and_removal_is_persisted() {
    let f = Fixture::new("");
    let core = f.open();
    core.add_plugin(&f.plugin).unwrap();
    drop(core);
    let path = f.root.join("data/plugins.json");
    let mut records: Vec<serde_json::Value> =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for id in ["sodeck.hello", "sodeck.ui-example"] {
        let mut record = records[0].clone();
        record["manifest"]["id"] = json!(id);
        record["enabled"] = json!(true);
        records.push(record);
    }
    fs::write(&path, serde_json::to_vec(&records).unwrap()).unwrap();
    // An unwritable migration leaves the original records available for recovery.
    fs::create_dir(path.with_extension("json.tmp")).unwrap();
    assert!(AppCore::open(f.root.join("data"), "test".into()).is_err());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&fs::read(&path).unwrap()).unwrap(),
        json!(records)
    );
    fs::remove_dir(path.with_extension("json.tmp")).unwrap();
    for _ in 0..2 {
        let core = f.open();
        core.start_enabled();
        let snapshot = core.snapshot();
        assert_eq!(snapshot.plugins.len(), 1);
        assert_eq!(snapshot.plugins[0].manifest.id, "test.fixture");
        assert_eq!(snapshot.plugins[0].status, PluginStatus::Running);
        assert!(snapshot.storage_errors.is_empty());
        core.shutdown().unwrap();
    }
    let saved: Vec<serde_json::Value> = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved.len(), 1);
    assert!(f.plugin.join("plugin.json").is_file());
}

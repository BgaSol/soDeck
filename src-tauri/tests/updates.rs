#![cfg(feature = "test-fixtures")]
use serde_json::json;
use sha2::{Digest, Sha256};
use sodeck::{
    model::{PluginStatus, UpdateSource},
    updates::ReleaseClient,
    AppCore, TARGET,
};
use std::{
    collections::HashMap,
    fs,
    io::{Cursor, Read, Write},
    net::TcpListener,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};
static SEQUENCE: AtomicUsize = AtomicUsize::new(0);
struct Server {
    base: String,
    routes: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    stopped: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Server {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let routes = Arc::new(Mutex::new(HashMap::<String, Vec<u8>>::new()));
        let stopped = Arc::new(AtomicBool::new(false));
        let (map, done) = (routes.clone(), stopped.clone());
        let thread = thread::spawn(move || {
            while !done.load(Ordering::Acquire) {
                if let Ok((mut stream, _)) = listener.accept() {
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let mut header = Vec::new();
                    let mut byte = [0];
                    while !header.ends_with(b"\r\n\r\n") && header.len() < 8192 {
                        if stream.read(&mut byte).unwrap_or(0) == 0 {
                            break;
                        }
                        header.push(byte[0]);
                    }
                    let request = String::from_utf8_lossy(&header);
                    let path = request
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or("")
                        .split('?')
                        .next()
                        .unwrap_or("");
                    let content = map.lock().unwrap().get(path).cloned();
                    let status = if content.is_some() {
                        "200 OK"
                    } else {
                        "404 Not Found"
                    };
                    let content = content.unwrap_or_default();
                    let _ = write!(
                        stream,
                        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        content.len()
                    );
                    let _ = stream.write_all(&content);
                } else {
                    thread::sleep(Duration::from_millis(5));
                }
            }
        });
        Self {
            base,
            routes,
            stopped,
            thread: Some(thread),
        }
    }
    fn put(&self, path: &str, bytes: impl Into<Vec<u8>>) {
        self.routes
            .lock()
            .unwrap()
            .insert(path.into(), bytes.into());
    }
    fn release(&self, bytes: Vec<u8>, api: &str, target: &str) {
        let hash = format!("{:x}", Sha256::digest(&bytes));
        self.put("/plugin.zip", bytes);
        self.put("/manifest.json", json!({"id":"test.fixture","version":"0.1.1","apiVersion":api,"notes":"new plugin","platforms":{target:{"url":format!("{}/plugin.zip", self.base),"sha256":hash}}}).to_string());
        self.put("/repos/test/repo/releases", json!([
            {"tag_name":"v99.0.0","draft":false,"prerelease":false,"assets":[]},
            {"tag_name":"plugin-fixture-v0.1.1","draft":false,"prerelease":false,"assets":[{"name":"plugin-update.json","browser_download_url":format!("{}/manifest.json",self.base)}]},
            {"tag_name":"plugin-fixture-v99.0.0","draft":true,"prerelease":false,"assets":[]}
        ]).to_string());
    }
    fn client(&self) -> ReleaseClient {
        ReleaseClient::test_server(&self.base)
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        self.thread.take().unwrap().join().unwrap();
    }
}
struct Fixture {
    root: PathBuf,
    external: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "sodeck-update-test-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let external = root.join("external");
        fs::create_dir_all(&external).unwrap();
        let name = format!("fixture{}", std::env::consts::EXE_SUFFIX);
        fs::copy(env!("CARGO_BIN_EXE_plugin-fixture"), external.join(&name)).unwrap();
        fs::write(external.join("plugin.json"), manifest("0.1.0").to_string()).unwrap();
        Self { root, external }
    }
    fn open(&self) -> Arc<AppCore> {
        AppCore::open(self.root.join("data"), "0.2.0".into()).unwrap()
    }
    fn add(&self) {
        let core = self.open();
        core.add_plugin(&self.external).unwrap();
        core.set_update_source(
            "test.fixture",
            Some(UpdateSource {
                repository: "test/repo".into(),
                tag_prefix: "plugin-fixture-v".into(),
            }),
        )
        .unwrap();
    }
    fn package(&self, mode: &str) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (path, bytes) in [
            (
                "plugin/plugin.json".into(),
                manifest("0.1.1").to_string().into_bytes(),
            ),
            (
                format!("plugin/fixture{}", std::env::consts::EXE_SUFFIX),
                fs::read(env!("CARGO_BIN_EXE_plugin-fixture")).unwrap(),
            ),
            ("plugin/mode".into(), mode.as_bytes().to_vec()),
        ] {
            writer
                .start_file(
                    path,
                    zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Deflated),
                )
                .unwrap();
            writer.write_all(&bytes).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn manifest(version: &str) -> serde_json::Value {
    json!({"id":"test.fixture","name":"Update Fixture","version":version,"apiVersion":"1","target":TARGET,"entry":format!("fixture{}",std::env::consts::EXE_SUFFIX)})
}

#[test]
fn enable_and_disable_wait_for_restart_and_keep_data() {
    let f = Fixture::new();
    let core = f.open();
    core.add_plugin(&f.external).unwrap();
    assert_eq!(core.snapshot().plugins[0].status, PluginStatus::Unloaded);
    assert!(core.snapshot().restart_required);
    drop(core);
    let core = f.open();
    core.start_enabled();
    assert_eq!(core.snapshot().plugins[0].status, PluginStatus::Running);
    assert!(!core.snapshot().restart_required);
    core.set_enabled("test.fixture", false).unwrap();
    assert_eq!(core.snapshot().plugins[0].status, PluginStatus::Running);
    assert!(core.forget("test.fixture").is_err());
    core.shutdown().unwrap();
    drop(core);
    let core = f.open();
    core.start_enabled();
    assert_eq!(core.snapshot().plugins[0].status, PluginStatus::Unloaded);
    assert!(!core.snapshot().plugins[0].enabled);
    assert!(core
        .snapshot()
        .data_directory
        .join("plugin-data/test.fixture/pid")
        .is_file());
    core.forget("test.fixture").unwrap();
    assert!(f.external.exists());
}

#[test]
fn clearing_update_source_survives_autoload() {
    let f = Fixture::new();
    let mut value = manifest("0.1.0");
    value["updateSource"] = json!({"repository":"test/repo","tagPrefix":"plugin-fixture-v"});
    fs::write(f.external.join("plugin.json"), value.to_string()).unwrap();
    let core = f.open();
    core.add_plugin(&f.external).unwrap();
    core.set_update_source("test.fixture", None).unwrap();
    drop(core);
    let core = f.open();
    core.start_enabled();
    assert!(core.snapshot().plugins[0].manifest.update_source.is_none());
    core.shutdown().unwrap();
}

#[test]
fn enabled_update_keeps_old_process_until_restart_then_runs_new_program() {
    let f = Fixture::new();
    f.add();
    let server = Server::new();
    server.release(f.package(""), "1", TARGET);
    let core = f.open();
    core.start_enabled();
    let old_pid = core.snapshot().plugins[0].pid;
    let candidate = core
        .check_component("test.fixture", &server.client())
        .unwrap()
        .unwrap();
    assert_eq!(candidate.version, "0.1.1");
    assert!(core
        .download_plugin("test.fixture", &server.client())
        .unwrap());
    let plugin = &core.snapshot().plugins[0];
    assert_eq!(plugin.pid, old_pid);
    assert_eq!(plugin.manifest.version, "0.1.0");
    assert_eq!(plugin.pending_version.as_deref(), Some("0.1.1"));
    core.shutdown().unwrap();
    drop(core);
    let core = f.open();
    core.start_enabled();
    let plugin = &core.snapshot().plugins[0];
    assert_eq!(plugin.status, PluginStatus::Running);
    assert_eq!(plugin.manifest.version, "0.1.1");
    assert!(!plugin.pending_restart);
    assert_ne!(plugin.directory, f.external);
    assert!(f.external.join("plugin.json").exists());
    core.shutdown().unwrap();
}

#[test]
fn failed_new_initialization_restores_previous_program() {
    let f = Fixture::new();
    f.add();
    let server = Server::new();
    server.release(f.package("init-error"), "1", TARGET);
    let core = f.open();
    core.start_enabled();
    core.check_component("test.fixture", &server.client())
        .unwrap();
    core.download_plugin("test.fixture", &server.client())
        .unwrap();
    core.shutdown().unwrap();
    drop(core);
    let core = f.open();
    core.start_enabled();
    let plugin = &core.snapshot().plugins[0];
    assert_eq!(plugin.status, PluginStatus::Running);
    assert_eq!(plugin.manifest.version, "0.1.0");
    assert!(plugin
        .update_error
        .as_ref()
        .unwrap()
        .contains("已恢复旧程序"));
    core.shutdown().unwrap();
}

#[test]
fn disabled_plugin_updates_without_starting_a_process() {
    let f = Fixture::new();
    f.add();
    {
        let core = f.open();
        core.set_enabled("test.fixture", false).unwrap();
    }
    let core = f.open();
    core.start_enabled();
    let server = Server::new();
    server.release(f.package(""), "1", TARGET);
    core.check_component("test.fixture", &server.client())
        .unwrap();
    assert!(!core
        .download_plugin("test.fixture", &server.client())
        .unwrap());
    let plugin = &core.snapshot().plugins[0];
    assert_eq!(plugin.manifest.version, "0.1.1");
    assert_eq!(plugin.status, PluginStatus::Unloaded);
    assert!(!plugin.enabled);
    assert!(!core.snapshot().restart_required);
}

#[test]
fn invalid_packages_sources_and_platforms_preserve_old_program() {
    let f = Fixture::new();
    f.add();
    let core = f.open();
    let server = Server::new();
    server.release(f.package(""), "2", TARGET);
    assert!(core
        .check_component("test.fixture", &server.client())
        .is_err());
    server.release(f.package(""), "1", "wrong-platform");
    assert!(core
        .check_component("test.fixture", &server.client())
        .is_err());
    server.release(f.package(""), "1", TARGET);
    core.check_component("test.fixture", &server.client())
        .unwrap();
    server.put("/plugin.zip", b"corrupt".to_vec());
    assert!(core
        .download_plugin("test.fixture", &server.client())
        .is_err());
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    archive
        .start_file("../outside", zip::write::SimpleFileOptions::default())
        .unwrap();
    archive.write_all(b"escape").unwrap();
    server.release(archive.finish().unwrap().into_inner(), "1", TARGET);
    core.check_component("test.fixture", &server.client())
        .unwrap();
    assert!(core
        .download_plugin("test.fixture", &server.client())
        .unwrap_err()
        .contains("不安全"));
    assert!(!f.root.join("data/plugins/test.fixture/outside").exists());
    server.release(f.package(""), "1", TARGET);
    core.check_component("test.fixture", &server.client())
        .unwrap();
    core.set_update_source("test.fixture", None).unwrap();
    assert!(core
        .download_plugin("test.fixture", &server.client())
        .is_err());
    assert_eq!(
        core.snapshot().plugins[0].directory,
        fs::canonicalize(&f.external).unwrap()
    );
    assert!(core.snapshot().plugins[0].pending_version.is_none());
    assert!(server
        .client()
        .discover(
            &UpdateSource {
                repository: "../repo".into(),
                tag_prefix: "v".into()
            },
            "test.fixture",
            "0.1.0"
        )
        .is_err());
}

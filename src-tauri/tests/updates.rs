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

#[test]
fn latest_discovery_does_not_read_oversized_historical_releases() {
    let server = Server::new();
    server.put("/repos/test/repo/releases", vec![b' '; 2 * 1024 * 1024 + 1]);
    server.put(
        "/repos/test/repo/git/matching-refs/tags/plugin-fixture-v",
        json!([
            {"ref":"refs/tags/plugin-fixture-v0.9.0"},
            {"ref":"refs/tags/plugin-fixture-v0.10.0"},
            {"ref":"refs/tags/plugin-fixture-v1.0.0-beta.1"}
        ])
        .to_string(),
    );
    server.put(
        "/repos/test/repo/releases/tags/plugin-fixture-v0.10.0",
        json!({"tag_name":"plugin-fixture-v0.10.0","draft":false,"prerelease":false,
            "assets":[{"name":"plugin-update.json","browser_download_url":format!("{}/manifest.json",server.base)}]})
        .to_string(),
    );
    server.put(
        "/manifest.json",
        json!({"id":"test.fixture","version":"0.10.0","apiVersion":"1","notes":"current version only",
            "platforms":{TARGET:{"url":format!("{}/plugin.zip",server.base),"sha256":"a".repeat(64)}}})
        .to_string(),
    );
    let update = server
        .client()
        .discover(
            &UpdateSource {
                repository: "test/repo".into(),
                tag_prefix: "plugin-fixture-v".into(),
            },
            "test.fixture",
            "0.8.0",
        )
        .unwrap()
        .unwrap();
    assert_eq!(update.version, "0.10.0");
    assert_eq!(update.notes, "current version only");
    assert_eq!(
        *server.requests.lock().unwrap(),
        [
            "/repos/test/repo/git/matching-refs/tags/plugin-fixture-v",
            "/repos/test/repo/releases/tags/plugin-fixture-v0.10.0",
            "/manifest.json"
        ]
    );
}

#[test]
fn missing_and_unpublished_tags_are_checked_in_version_order_then_stop() {
    let server = Server::new();
    server.release(Vec::new(), "1", TARGET);
    server.tags(
        "test/repo",
        "plugin-fixture-v",
        &[
            "plugin-fixture-v0.1.1",
            "plugin-fixture-v0.1.5",
            "plugin-fixture-v0.1.2",
            "plugin-fixture-v0.1.0",
            "plugin-fixture-v0.1.3",
            "plugin-fixture-v0.1.4",
            "plugin-fixture-v0.1.1",
            "plugin-fixture-v0.1.6-beta.1",
            "plugin-fixture-v0.1.6+build",
            "plugin-fixture-vmain",
            "plugin-fixture-v0.01.6",
        ],
    );
    for (version, draft, prerelease) in [("0.1.4", true, false), ("0.1.3", false, true)] {
        server.put(&format!("/repos/test/repo/releases/tags/plugin-fixture-v{version}"),
            json!({"tag_name":format!("plugin-fixture-v{version}"),"draft":draft,"prerelease":prerelease,"assets":[]}).to_string());
    }
    let update = server
        .client()
        .discover(
            &UpdateSource {
                repository: "test/repo".into(),
                tag_prefix: "plugin-fixture-v".into(),
            },
            "test.fixture",
            "0.1.0",
        )
        .unwrap()
        .unwrap();
    assert_eq!(update.version, "0.1.1");
    assert_eq!(
        *server.requests.lock().unwrap(),
        [
            "/repos/test/repo/git/matching-refs/tags/plugin-fixture-v",
            "/repos/test/repo/releases/tags/plugin-fixture-v0.1.5",
            "/repos/test/repo/releases/tags/plugin-fixture-v0.1.4",
            "/repos/test/repo/releases/tags/plugin-fixture-v0.1.3",
            "/repos/test/repo/releases/tags/plugin-fixture-v0.1.2",
            "/repos/test/repo/releases/tags/plugin-fixture-v0.1.1",
            "/manifest.json",
        ]
    );
}

#[test]
fn missing_new_releases_never_probe_current_or_older_versions() {
    for tags in [
        vec![],
        vec!["vmain", "v1.0.1-beta.1", "v1.0.1+build"],
        vec!["v0.9.0", "v1.0.0"],
        vec!["v1.0.1", "v0.9.0", "v1.0.2", "v1.0.0"],
    ] {
        let server = Server::new();
        server.tags("test/repo", "v", &tags);
        assert!(server
            .client()
            .discover(
                &UpdateSource {
                    repository: "test/repo".into(),
                    tag_prefix: "v".into()
                },
                "sodeck.core",
                "1.0.0",
            )
            .unwrap()
            .is_none());
        let expected = if tags.contains(&"v1.0.2") {
            vec![
                "/repos/test/repo/git/matching-refs/tags/v",
                "/repos/test/repo/releases/tags/v1.0.2",
                "/repos/test/repo/releases/tags/v1.0.1",
            ]
        } else {
            vec!["/repos/test/repo/git/matching-refs/tags/v"]
        };
        assert_eq!(*server.requests.lock().unwrap(), expected);
    }
}

#[test]
fn tag_directory_rejects_wrong_refs_invalid_json_and_large_responses() {
    for (body, message) in [
        (
            json!([{"ref":"refs/tags/other-v1.0.0"}])
                .to_string()
                .into_bytes(),
            "其他来源",
        ),
        (
            json!([{"ref":"refs/heads/v1.0.0"}])
                .to_string()
                .into_bytes(),
            "其他来源",
        ),
        (b"not json".to_vec(), "格式错误"),
        (vec![b' '; 2 * 1024 * 1024 + 1], "过大"),
    ] {
        let server = Server::new();
        server.put("/repos/test/repo/git/matching-refs/tags/v", body);
        let error = server
            .client()
            .discover(
                &UpdateSource {
                    repository: "test/repo".into(),
                    tag_prefix: "v".into(),
                },
                "sodeck.core",
                "0.1.0",
            )
            .unwrap_err();
        assert!(error.contains(message), "{error}");
        assert_eq!(server.requests.lock().unwrap().len(), 1);
    }
}

#[test]
fn first_official_release_errors_are_not_bypassed() {
    for (case, message) in [
        ("missing-asset", "缺少更新清单"),
        ("release-tag", "发行标签不一致"),
        ("release-json", "格式错误"),
        ("missing-manifest", "404"),
        ("manifest-json", "格式错误"),
        ("id", "组件或版本不一致"),
        ("version", "组件或版本不一致"),
        ("api", "核心 API"),
        ("platform", "本机平台"),
        ("url", "HTTPS"),
    ] {
        let server = Server::new();
        server.release(Vec::new(), "1", TARGET);
        server.tags(
            "test/repo",
            "plugin-fixture-v",
            &["plugin-fixture-v0.1.1", "plugin-fixture-v0.1.2"],
        );
        let mut release: serde_json::Value = serde_json::from_slice(
            &server.routes.lock().unwrap()["/repos/test/repo/releases/tags/plugin-fixture-v0.1.1"],
        )
        .unwrap();
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&server.routes.lock().unwrap()["/manifest.json"]).unwrap();
        release["tag_name"] = json!("plugin-fixture-v0.1.2");
        manifest["version"] = json!("0.1.2");
        match case {
            "missing-asset" => release["assets"] = json!([]),
            "release-tag" => release["tag_name"] = json!("plugin-fixture-v0.1.1"),
            "id" => manifest["id"] = json!("other.plugin"),
            "version" => manifest["version"] = json!("0.1.1"),
            "api" => manifest["apiVersion"] = json!("2"),
            "platform" => manifest["platforms"] = json!({}),
            "url" => {
                manifest["platforms"][TARGET]["url"] = json!("http://unsafe.example/package.zip")
            }
            _ => {}
        }
        server.put(
            "/repos/test/repo/releases/tags/plugin-fixture-v0.1.2",
            release.to_string(),
        );
        server.put("/manifest.json", manifest.to_string());
        match case {
            "release-json" => server.put(
                "/repos/test/repo/releases/tags/plugin-fixture-v0.1.2",
                "not json",
            ),
            "manifest-json" => server.put("/manifest.json", "not json"),
            "missing-manifest" => {
                server.routes.lock().unwrap().remove("/manifest.json");
            }
            _ => {}
        }
        let error = server
            .client()
            .discover(
                &UpdateSource {
                    repository: "test/repo".into(),
                    tag_prefix: "plugin-fixture-v".into(),
                },
                "test.fixture",
                "0.1.0",
            )
            .unwrap_err();
        assert!(error.contains(message), "{case}: {error}");
        assert!(
            !server
                .requests
                .lock()
                .unwrap()
                .iter()
                .any(|p| p.ends_with("plugin-fixture-v0.1.1")),
            "{case}"
        );
    }
}

struct Server {
    base: String,
    routes: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    requests: Arc<Mutex<Vec<String>>>,
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
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = requests.clone();
        let (map, done) = (routes.clone(), stopped.clone());
        let thread = thread::spawn(move || {
            while !done.load(Ordering::Acquire) {
                if let Ok((mut stream, _)) = listener.accept() {
                    // macOS/Windows may inherit the listener's nonblocking mode.
                    stream.set_nonblocking(false).unwrap();
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    stream
                        .set_write_timeout(Some(Duration::from_secs(5)))
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
                    recorded.lock().unwrap().push(path.to_string());
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
            requests,
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
    fn tags(&self, repository: &str, prefix: &str, tags: &[&str]) {
        self.put(
            &format!("/repos/{repository}/git/matching-refs/tags/{prefix}"),
            json!(tags
                .iter()
                .map(|tag| json!({"ref":format!("refs/tags/{tag}")}))
                .collect::<Vec<_>>())
            .to_string(),
        );
    }
    fn publish(&self, repository: &str, prefix: &str, releases: Vec<serde_json::Value>) {
        let tags: Vec<_> = releases
            .iter()
            .filter_map(|release| release["tag_name"].as_str())
            .filter(|tag| tag.starts_with(prefix))
            .collect();
        self.tags(repository, prefix, &tags);
        for release in &releases {
            self.put(
                &format!(
                    "/repos/{repository}/releases/tags/{}",
                    release["tag_name"].as_str().unwrap()
                ),
                release.to_string(),
            );
        }
    }
    fn release(&self, bytes: Vec<u8>, api: &str, target: &str) {
        let hash = format!("{:x}", Sha256::digest(&bytes));
        self.put("/plugin.zip", bytes);
        self.put("/manifest.json", json!({"id":"test.fixture","version":"0.1.1","apiVersion":api,"notes":"new plugin","platforms":{target:{"url":format!("{}/plugin.zip", self.base),"sha256":hash}}}).to_string());
        self.publish("test/repo", "plugin-fixture-v", vec![
            json!({"tag_name":"v99.0.0","draft":false,"prerelease":false,"assets":[]}),
            json!({"tag_name":"plugin-fixture-v0.1.1","draft":false,"prerelease":false,"assets":[{"name":"plugin-update.json","browser_download_url":format!("{}/manifest.json",self.base)}]}),
            json!({"tag_name":"plugin-fixture-v99.0.0","draft":true,"prerelease":false,"assets":[]})
        ]);
    }
    fn client(&self) -> ReleaseClient {
        ReleaseClient::test_server(&self.base)
    }
    fn official_release(&self, bytes: Vec<u8>, api: &str, target: &str) {
        self.official_release_for("sodeck.terminal", "plugin-terminal-v", bytes, api, target);
    }
    fn catalog(&self, plugins: Vec<serde_json::Value>) {
        let tree: Vec<_> = plugins.into_iter().enumerate().map(|(index, plugin)| {
            let content = plugin.to_string();
            let sha = format!("{:x}", Sha256::digest(content.as_bytes()))[..40].to_string();
            self.put(&format!("/repos/BgaSol/soDeck/git/blobs/{sha}"), content);
            json!({"path":format!("plugins/plugin-{index}/release.json"),"type":"blob","sha":sha})
        }).collect();
        self.put(
            "/repos/BgaSol/soDeck/git/trees/main",
            json!({"tree":tree,"truncated":false}).to_string(),
        );
    }
    fn official_release_for(
        &self,
        id: &str,
        prefix: &str,
        bytes: Vec<u8>,
        api: &str,
        target: &str,
    ) {
        self.catalog(vec![json!({"id":id,"name":"仓库插件","description":"来自仓库的发行配置","tagPrefix":prefix})]);
        self.release(bytes, api, target);
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&self.routes.lock().unwrap()["/manifest.json"]).unwrap();
        manifest["id"] = json!(id);
        self.put("/manifest.json", manifest.to_string());
        self.publish("BgaSol/soDeck", prefix, vec![
            json!({"tag_name":format!("{prefix}0.1.1"),"draft":false,"prerelease":false,"assets":[{"name":"plugin-update.json","browser_download_url":format!("{}/manifest.json",self.base)}]})
        ]);
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
        self.package_for("test.fixture", mode)
    }
    fn package_for(&self, id: &str, mode: &str) -> Vec<u8> {
        let mut metadata = if mode.starts_with("settings-") {
            settings_manifest("0.1.1")
        } else {
            manifest("0.1.1")
        };
        let mode = if mode == "settings-error" {
            "init-error"
        } else if mode == "settings-upgrade" {
            ""
        } else {
            mode
        };
        metadata["id"] = json!(id);
        // The core must keep its selected source, not a source supplied by the archive.
        metadata["updateSource"] =
            json!({"repository":"other/source","tagPrefix":"plugin-other-v"});
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (path, bytes) in [
            (
                "plugin/plugin.json".into(),
                metadata.to_string().into_bytes(),
            ),
            (
                format!("plugin/fixture{}", std::env::consts::EXE_SUFFIX),
                fs::read(env!("CARGO_BIN_EXE_plugin-fixture")).unwrap(),
            ),
            ("plugin/mode".into(), mode.as_bytes().to_vec()),
            ("plugin/ui.html".into(), b"settings fixture".to_vec()),
            (
                "plugin/资料/说明.txt".into(),
                "更新包的中文文件".as_bytes().to_vec(),
            ),
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
fn official_install_uses_fallback_for_tree_blob_release_manifest_and_package() {
    let fixture = Fixture::new();
    let direct = Server::new();
    let bad = Server::new();
    let good = Server::new();
    direct.official_release(fixture.package_for("sodeck.terminal", ""), "1", TARGET);
    let canonical = direct.routes.lock().unwrap().clone();
    for (path, bytes) in canonical {
        good.put(&format!("/{}{path}", direct.base), bytes);
        bad.put(
            &format!("/{}{path}", direct.base),
            b"<html>bad cached response</html>".to_vec(),
        );
    }
    direct.routes.lock().unwrap().clear();
    let client = ReleaseClient::test_mirrors(
        &direct.base,
        vec![
            (format!("{}/", bad.base), true),
            (format!("{}/", good.base), true),
        ],
        Arc::new(Default::default()),
    );
    let core = fixture.open();
    let catalog = core.official_plugins(&client, true).unwrap();
    assert!(catalog.error.is_none());
    assert_eq!(catalog.plugins[0].id, "sodeck.terminal");
    core.install_official_plugin("sodeck.terminal", &client)
        .unwrap();
    assert_eq!(core.snapshot().plugins[0].manifest.version, "0.1.1");
    let requests = good.requests.lock().unwrap();
    for part in [
        "git/trees/main",
        "git/blobs/",
        "/git/matching-refs/tags/",
        "/releases/tags/",
        "/manifest.json",
        "/plugin.zip",
    ] {
        assert!(
            requests.iter().any(|path| path.contains(part)),
            "{part}: {requests:?}"
        );
    }
    assert!(requests
        .iter()
        .all(|path| path.starts_with(&format!("/{}", direct.base))));
    core.shutdown().unwrap();
}

#[test]
#[ignore = "Explicit public mirror test downloads an official plugin into isolated temporary data"]
fn official_install_live_mirror() {
    let prefix = std::env::var("SODECK_TEST_MIRROR")
        .expect("Set SODECK_TEST_MIRROR to the single HTTPS prefix to test");
    let fixture = Fixture::new();
    let core = fixture.open();
    let client = if prefix == "auto" {
        core.release_client().unwrap()
    } else {
        ReleaseClient::test_proxy(&prefix).unwrap()
    };
    let catalog = core.official_plugins(&client, true).unwrap();
    assert!(catalog.error.is_none());
    println!(
        "{} official plugins discovered via {prefix}",
        catalog.plugins.len()
    );
    let candidate = client
        .discover(
            &UpdateSource {
                repository: "BgaSol/soDeck".into(),
                tag_prefix: "v".into(),
            },
            sodeck::updates::CORE_ID,
            "0.0.1",
        )
        .unwrap()
        .unwrap();
    assert!(candidate.manifest_url.starts_with("https://github.com/"));
    println!(
        "Core {} manifest discovered with original URLs",
        candidate.version
    );
    core.install_official_plugin("sodeck.terminal", &client)
        .unwrap();
    let snapshot = core.snapshot();
    assert_eq!(snapshot.plugins[0].manifest.id, "sodeck.terminal");
    println!("Official terminal {} package downloaded, hashed, unpacked and registered for restart on {}",snapshot.plugins[0].manifest.version,TARGET);
    core.shutdown().unwrap();
}
fn settings_manifest(version: &str) -> serde_json::Value {
    let mut m = manifest(version);
    m["ui"] = json!({"apiVersion":"1","entry":"ui.html","views":[{"id":"test","title":"Test","icon":"info","container":"editor","instancePolicy":"multiple"}]});
    let mut fields = vec![
        json!({"key":"size","title":"字号","type":"number","default":if version=="0.1.0"{14}else{16},"min":11,"max":24,"step":1,"unit":"px","apply":"live"}),
    ];
    if version != "0.1.0" {
        fields.push(
            json!({"key":"blink","title":"闪烁","type":"boolean","default":true,"apply":"live"}),
        );
    }
    m["settings"] = json!({"apiVersion":"1","schemaVersion":1,"pages":[{"id":"appearance","title":"外观","viewTypes":["test"],"groups":[{"id":"text","title":"文字","fields":fields}]}]});
    m
}

#[test]
fn settings_follow_pending_activation_and_program_rollback() {
    for failed in [false, true] {
        let f = Fixture::new();
        fs::write(
            f.external.join("plugin.json"),
            settings_manifest("0.1.0").to_string(),
        )
        .unwrap();
        fs::write(f.external.join("ui.html"), "old UI").unwrap();
        f.add();
        let core = f.open();
        core.start_enabled();
        let old = core.settings_page("test.fixture", "appearance").unwrap();
        let saved=core.save_settings("test.fixture","appearance",json!({"definitionToken":old["definitionToken"],"baseRevision":old["revision"],"requestId":"before-upgrade","values":{"size":18}})).unwrap()["snapshot"].clone();
        let server = Server::new();
        server.release(
            f.package(if failed {
                "settings-error"
            } else {
                "settings-upgrade"
            }),
            "1",
            TARGET,
        );
        core.check_component("test.fixture", &server.client())
            .unwrap();
        core.download_plugin("test.fixture", &server.client())
            .unwrap();
        let pending = core.settings_page("test.fixture", "appearance").unwrap();
        assert_eq!(pending["definitionToken"], old["definitionToken"]);
        assert_eq!(pending["values"]["size"], 18);
        assert!(pending["values"].get("blink").is_none());
        core.shutdown().unwrap();
        drop(core);
        let core = f.open();
        core.start_enabled();
        let active = core.settings_page("test.fixture", "appearance").unwrap();
        assert_eq!(active["values"]["size"], 18);
        assert_eq!(active["version"], if failed { "0.1.0" } else { "0.1.1" });
        if failed {
            assert!(active["values"].get("blink").is_none());
        } else {
            assert_eq!(active["values"]["blink"], true);
        }
        let values = active["values"].clone();
        assert_eq!(core.save_settings("test.fixture","appearance",json!({"definitionToken":saved["definitionToken"],"baseRevision":saved["revision"],"requestId":"stale","values":values})).unwrap()["conflict"],true);
        core.shutdown().unwrap();
    }
}

#[test]
fn official_catalog_discovers_new_plugins_and_removals_without_core_changes() {
    let f = Fixture::new();
    let core = f.open();
    let server = Server::new();
    let first = json!({"id":"example.new-tool","name":"新工具","tagPrefix":"new-tool-v","order":20,"icon":"terminal"});
    let second = json!({"id":"example.another","name":"另一款","tagPrefix":"another-v","order":10,"description":"从仓库读取的说明"});
    server.catalog(vec![first.clone()]);
    let initial = core.official_plugins(&server.client(), false).unwrap();
    assert_eq!(initial.plugins[0].id, "example.new-tool");
    assert_eq!(initial.plugins[0].source().repository, "BgaSol/soDeck");
    assert!(initial.error.is_none());
    server.requests.lock().unwrap().clear();
    server.catalog(vec![first, second]);
    assert_eq!(
        core.official_plugins(&server.client(), false)
            .unwrap()
            .plugins
            .len(),
        1
    );
    assert!(
        server.requests.lock().unwrap().is_empty(),
        "Fresh cache avoids GitHub rate limits"
    );
    let updated = core.official_plugins(&server.client(), true).unwrap();
    assert_eq!(updated.plugins.len(), 2);
    assert_eq!(updated.plugins[0].id, "example.another");
    assert_eq!(updated.plugins[0].description, "从仓库读取的说明");
    drop(core);
    let core = f.open();
    server.requests.lock().unwrap().clear();
    assert_eq!(
        core.official_plugins(&server.client(), false)
            .unwrap()
            .plugins
            .len(),
        2
    );
    assert!(
        server.requests.lock().unwrap().is_empty(),
        "Cache survives restarting the core"
    );
    server.catalog(vec![]);
    assert!(core
        .official_plugins(&server.client(), true)
        .unwrap()
        .plugins
        .is_empty());
}

#[test]
fn official_catalog_errors_preserve_last_complete_list_and_installed_records() {
    let f = Fixture::new();
    let core = f.open();
    let server = Server::new();
    assert!(core
        .official_plugins(&server.client(), false)
        .unwrap_err()
        .contains("无法获取"));
    core.add_plugin(&f.external).unwrap();
    server.catalog(vec![
        json!({"id":"example.tool","name":"工具","tagPrefix":"tool-v"}),
    ]);
    let good = core.official_plugins(&server.client(), true).unwrap();
    let disk = fs::read(f.root.join("data/official-plugins.json")).unwrap();
    for invalid in ["not json", r#"{"tree":[],"truncated":true}"#] {
        server.put(
            "/repos/BgaSol/soDeck/git/trees/main",
            invalid.as_bytes().to_vec(),
        );
        let fallback = core.official_plugins(&server.client(), true).unwrap();
        assert_eq!(fallback.fetched_at, good.fetched_at);
        assert_eq!(fallback.plugins[0].id, "example.tool");
        assert!(fallback.error.unwrap().contains("上次成功"));
        assert_eq!(
            fs::read(f.root.join("data/official-plugins.json")).unwrap(),
            disk
        );
        assert_eq!(core.snapshot().plugins[0].manifest.id, "test.fixture");
    }
    server.catalog(vec![]);
    let empty = core.official_plugins(&server.client(), true).unwrap();
    assert!(empty.error.is_none() && empty.plugins.is_empty());
}

#[test]
fn official_catalog_rejects_invalid_metadata_and_ignores_non_plugin_files() {
    let f = Fixture::new();
    let core = f.open();
    let server = Server::new();
    let valid = json!({"id":"example.tool","name":"工具","tagPrefix":"tool-v"});
    for invalid in [
        json!({"id":"../../escape","name":"工具","tagPrefix":"tool-v"}),
        json!({"id":"sodeck.core","name":"核心","tagPrefix":"v"}),
        json!({"id":"example.tool","name":" ","tagPrefix":"tool-v"}),
        json!({"id":"example.tool","name":"工具","tagPrefix":"../../v"}),
        json!({"id":"example.tool","name":"工具","tagPrefix":"tool-v","icon":"<svg>"}),
    ] {
        server.catalog(vec![invalid]);
        assert!(core.official_plugins(&server.client(), true).is_err());
    }
    for duplicate in [
        valid.clone(),
        json!({"id":"example.other","name":"其他","tagPrefix":"tool-v"}),
    ] {
        server.catalog(vec![valid.clone(), duplicate]);
        assert!(core
            .official_plugins(&server.client(), true)
            .unwrap_err()
            .contains("重复"));
    }
    server.put(
        "/repos/BgaSol/soDeck/git/trees/main",
        json!({"tree":[
        {"path":"scripts/fixtures/example/release.json","type":"blob","sha":"bad"},
        {"path":"plugins/test/ui/release.json","type":"blob","sha":"bad"},
        {"path":"plugins/retired/release.json","type":"tree","sha":"bad"}
    ],"truncated":false})
        .to_string(),
    );
    assert!(core
        .official_plugins(&server.client(), true)
        .unwrap()
        .plugins
        .is_empty());
}

#[test]
fn a_new_repository_plugin_can_be_installed_without_a_compiled_in_id() {
    let f = Fixture::new();
    let core = f.open();
    let server = Server::new();
    let id = "example.future-plugin";
    server.official_release_for(id, "future-release-v", f.package_for(id, ""), "1", TARGET);
    assert_eq!(
        core.install_official_plugin(id, &server.client()).unwrap(),
        id
    );
    let plugin = &core.snapshot().plugins[0];
    assert_eq!(
        plugin.manifest.update_source.as_ref().unwrap().tag_prefix,
        "future-release-v"
    );
    assert!(plugin.enabled && plugin.pending_restart);
}

#[test]
fn expired_catalog_cache_is_refreshed_and_partial_metadata_never_replaces_it() {
    let f = Fixture::new();
    let core = f.open();
    let server = Server::new();
    let original = json!({"id":"example.original","name":"原插件","tagPrefix":"original-v"});
    server.catalog(vec![original.clone()]);
    let mut catalog = core.official_plugins(&server.client(), false).unwrap();
    catalog.fetched_at = 0;
    let path = f.root.join("data/official-plugins.json");
    drop(core);
    fs::write(&path, serde_json::to_vec(&catalog).unwrap()).unwrap();
    let core = f.open();
    let new = json!({"id":"example.new","name":"新插件","tagPrefix":"new-v"});
    server.catalog(vec![original.clone(), new]);
    assert_eq!(
        core.official_plugins(&server.client(), false)
            .unwrap()
            .plugins
            .len(),
        2
    );
    let disk = fs::read(&path).unwrap();
    server.catalog(vec![
        original,
        json!({"id":"example.broken","name":"坏配置","tagPrefix":"broken-v"}),
    ]);
    let tree: serde_json::Value = serde_json::from_slice(
        &server.routes.lock().unwrap()["/repos/BgaSol/soDeck/git/trees/main"],
    )
    .unwrap();
    let missing = format!(
        "/repos/BgaSol/soDeck/git/blobs/{}",
        tree["tree"][1]["sha"].as_str().unwrap()
    );
    server.routes.lock().unwrap().remove(&missing);
    let fallback = core.official_plugins(&server.client(), true).unwrap();
    assert!(fallback.error.is_some());
    assert_eq!(fallback.plugins.len(), 2);
    assert_eq!(fs::read(&path).unwrap(), disk);
    assert!(core.snapshot().plugins.is_empty());
}

#[test]
#[ignore = "Explicit smoke test against the live public GitHub repository"]
fn official_catalog_live_repository() {
    let f = Fixture::new();
    let core = f.open();
    let catalog = core
        .official_plugins(&ReleaseClient::github().unwrap(), true)
        .unwrap();
    assert!(!catalog.plugins.is_empty());
    assert!(catalog.error.is_none());
    println!(
        "Live official catalog: {:?}",
        catalog.plugins.iter().map(|p| &p.id).collect::<Vec<_>>()
    );
    assert!(
        core.snapshot().plugins.is_empty(),
        "Reading the catalog never installs programs"
    );
}

#[test]
fn official_first_install_downloads_registers_and_waits_for_restart() {
    let f = Fixture::new();
    let core = f.open();
    let server = Server::new();
    server.official_release(f.package_for("sodeck.terminal", ""), "1", TARGET);
    assert!(core
        .install_official_plugin("unknown.plugin", &server.client())
        .is_err());
    assert_eq!(
        core.install_official_plugin("sodeck.terminal", &server.client())
            .unwrap(),
        "sodeck.terminal"
    );
    let snapshot = core.snapshot();
    let plugin = &snapshot.plugins[0];
    assert_eq!(plugin.status, PluginStatus::Unloaded);
    assert!(plugin.enabled && plugin.pending_restart && snapshot.restart_required);
    assert!(plugin
        .directory
        .starts_with(fs::canonicalize(f.root.join("data/plugins")).unwrap()));
    assert_eq!(
        plugin.manifest.update_source.as_ref().unwrap().repository,
        "BgaSol/soDeck"
    );
    assert_eq!(
        plugin.manifest.update_source.as_ref().unwrap().tag_prefix,
        "plugin-terminal-v"
    );
    let installed = plugin.directory.clone();
    assert!(core
        .install_official_plugin("sodeck.terminal", &server.client())
        .is_err());
    assert_eq!(core.snapshot().plugins.len(), 1);
    assert_eq!(core.snapshot().plugins[0].directory, installed);
    drop(core);
    let core = f.open();
    core.start_enabled();
    assert_eq!(core.snapshot().plugins[0].status, PluginStatus::Running);
    assert!(!core.snapshot().restart_required);
    core.shutdown().unwrap();
}

#[test]
fn official_install_failures_leave_no_record_and_can_retry() {
    let f = Fixture::new();
    let core = f.open();
    let server = Server::new();
    for (api, target) in [("2", TARGET), ("1", "unsupported-target")] {
        server.official_release(f.package_for("sodeck.terminal", ""), api, target);
        assert!(core
            .install_official_plugin("sodeck.terminal", &server.client())
            .is_err());
        assert!(core.snapshot().plugins.is_empty());
    }
    server.official_release(f.package_for("sodeck.terminal", ""), "1", TARGET);
    server.put("/plugin.zip", b"broken".to_vec());
    assert!(core
        .install_official_plugin("sodeck.terminal", &server.client())
        .unwrap_err()
        .contains("SHA-256"));
    server.official_release(f.package(""), "1", TARGET);
    assert!(core
        .install_official_plugin("sodeck.terminal", &server.client())
        .unwrap_err()
        .contains("ID"));
    server.official_release(f.package_for("sodeck.terminal", ""), "1", TARGET);
    fs::create_dir_all(f.root.join("data/plugins.json.tmp")).unwrap();
    assert!(core
        .install_official_plugin("sodeck.terminal", &server.client())
        .is_err());
    assert!(core.snapshot().plugins.is_empty());
    assert_eq!(
        fs::read_dir(f.root.join("data/plugins/sodeck.terminal"))
            .unwrap()
            .count(),
        0
    );
    assert!(!fs::read_dir(f.root.join("data")).unwrap().any(|e| e
        .unwrap()
        .path()
        .extension()
        .is_some_and(|e| e == "part")));
    fs::remove_dir(f.root.join("data/plugins.json.tmp")).unwrap();
    core.install_official_plugin("sodeck.terminal", &server.client())
        .unwrap();
    assert_eq!(core.update_views()["sodeck.terminal"].phase, "ready");
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
fn checking_prepared_plugin_keeps_ready_state() {
    let f = Fixture::new();
    f.add();
    let server = Server::new();
    server.release(f.package(""), "1", TARGET);
    let core = f.open();
    core.check_component("test.fixture", &server.client())
        .unwrap();
    assert!(core
        .download_plugin("test.fixture", &server.client())
        .unwrap());
    assert!(core
        .check_component("test.fixture", &server.client())
        .unwrap()
        .is_none());
    assert_eq!(core.update_views()["test.fixture"].phase, "ready");
    assert_eq!(
        core.snapshot().plugins[0].pending_version.as_deref(),
        Some("0.1.1")
    );
}

#[test]
fn batch_reservations_preserve_other_results_and_interrupted_tasks_can_retry() {
    let f = Fixture::new();
    f.add();
    let server = Server::new();
    server.release(f.package(""), "1", TARGET);
    let core = f.open();
    core.check_component("test.fixture", &server.client())
        .unwrap();
    let ids = vec!["test.fixture".to_string()];
    let tasks = core.reserve_update_downloads(&ids).unwrap();
    assert_eq!(core.update_views()["test.fixture"].phase, "waiting");
    assert!(core.reserve_update_downloads(&ids).is_err());
    assert!(core.begin_update_check(true).is_none());
    assert!(core.begin_update_check(false).is_none());
    assert!(core
        .close_prepare(
            "main",
            "busy-restart",
            sodeck::lifecycle::Target::Application {
                action: sodeck::lifecycle::Action::Restart,
            }
        )
        .is_err());
    assert!(!core.is_closing());
    drop(tasks);
    assert!(!core.update_overview().busy);
    assert_eq!(
        core.update_views()["test.fixture"].error_stage.as_deref(),
        Some("download")
    );
    let tasks = core.reserve_update_downloads(&ids).unwrap();
    for (id, op) in tasks {
        core.download_plugin_reserved(&id, &server.client(), op)
            .unwrap();
    }
    assert_eq!(core.update_views()["test.fixture"].phase, "ready");
}

#[test]
fn automatic_check_preferences_and_candidates_survive_restart() {
    let f = Fixture::new();
    f.add();
    let server = Server::new();
    server.release(f.package(""), "1", TARGET);
    let core = f.open();
    let mut batch = core.begin_update_check(false).unwrap();
    assert!(core.begin_update_check(false).is_none());
    core.check_component("test.fixture", &server.client())
        .unwrap();
    batch.succeeded();
    drop(batch);
    assert!(core.begin_update_check(true).is_none());
    core.check_component("test.fixture", &server.client())
        .unwrap();
    assert_eq!(
        core.messages().len(),
        1,
        "Repeated checks do not create duplicate update messages"
    );
    core.set_automatic_updates(false).unwrap();
    core.set_theme("dark").unwrap();
    drop(core);
    let core = f.open();
    assert!(!core.snapshot().preferences.automatic_updates);
    assert!(core.begin_update_check(true).is_none());
    assert_eq!(core.update_views()["test.fixture"].phase, "available");
    core.download_plugin("test.fixture", &server.client())
        .unwrap();
    core.check_component("test.fixture", &server.client())
        .unwrap();
    assert_eq!(core.update_views()["test.fixture"].phase, "ready");
}

#[test]
fn failed_checks_remain_visible_after_reopening_without_a_candidate() {
    let f = Fixture::new();
    f.add();
    let server = Server::new();
    let core = f.open();
    let batch = core.begin_update_check(false).unwrap();
    assert!(core
        .check_component("test.fixture", &server.client())
        .is_err());
    drop(batch);
    let retry_at = core.update_overview().next_check_at;
    assert_eq!(core.update_overview().last_checked_at, 0);
    drop(core);
    let core = f.open();
    let view = &core.update_views()["test.fixture"];
    assert_eq!(view.phase, "error");
    assert_eq!(view.error_stage.as_deref(), Some("check"));
    assert!(view.version.is_none());
    assert_eq!(core.update_overview().next_check_at, retry_at);
    assert!(core.begin_update_check(true).is_none());
    server.release(f.package(""), "1", TARGET);
    core.check_component("test.fixture", &server.client())
        .unwrap();
    assert_eq!(core.update_views()["test.fixture"].phase, "available");
}

#[test]
fn failed_tag_checks_are_independent_and_can_retry() {
    let server = Server::new();
    let client = server.client();
    for (id, prefix) in [("sodeck.core", "v"), ("test.fixture", "plugin-fixture-v")] {
        assert!(client
            .discover(
                &UpdateSource {
                    repository: "test/repo".into(),
                    tag_prefix: prefix.into(),
                },
                id,
                "0.1.0",
            )
            .is_err());
    }
    assert_eq!(server.requests.lock().unwrap().len(), 2);
    // A new request retries the source rather than reusing a repository-wide failure.
    assert!(server
        .client()
        .discover(
            &UpdateSource {
                repository: "test/repo".into(),
                tag_prefix: "v".into(),
            },
            "sodeck.core",
            "0.1.0",
        )
        .is_err());
    assert_eq!(server.requests.lock().unwrap().len(), 3);
}

#[test]
fn core_and_plugin_discovery_read_only_their_own_tags() {
    let server = Server::new();
    let f = Fixture::new();
    server.release(f.package(""), "1", TARGET);
    server.publish("test/repo", "v", vec![json!({"tag_name":"v0.2.1","draft":false,"prerelease":false,"assets":[{"name":"core-update.json","browser_download_url":format!("{}/core.json",server.base)}]})]);
    server.put("/core.json",json!({"id":"sodeck.core","version":"0.2.1","platforms":{TARGET:{"url":format!("{}/core.zip",server.base)}}}).to_string());
    let client = server.client();
    for (id, prefix, old, new) in [
        ("test.fixture", "plugin-fixture-v", "0.1.0", "0.1.1"),
        ("sodeck.core", "v", "0.2.0", "0.2.1"),
    ] {
        assert_eq!(
            client
                .discover(
                    &UpdateSource {
                        repository: "test/repo".into(),
                        tag_prefix: prefix.into()
                    },
                    id,
                    old
                )
                .unwrap()
                .unwrap()
                .version,
            new
        );
    }
    assert_eq!(
        server
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|p| p.contains("/git/matching-refs/tags/"))
            .count(),
        2
    );
    assert!(!server
        .requests
        .lock()
        .unwrap()
        .iter()
        .any(|p| p == "/repos/test/repo/releases"));
}

#[test]
fn completely_disabled_plugin_updates_without_restart_status() {
    let f = Fixture::new();
    f.add();
    let core = f.open();
    core.set_enabled("test.fixture", false).unwrap();
    drop(core);
    let core = f.open();
    let server = Server::new();
    server.release(f.package(""), "1", TARGET);
    core.check_component("test.fixture", &server.client())
        .unwrap();
    assert!(!core
        .download_plugin("test.fixture", &server.client())
        .unwrap());
    assert_eq!(core.update_views()["test.fixture"].phase, "current");
    assert!(!core.snapshot().restart_required);
}

#[test]
fn changed_update_source_survives_version_switch_and_rollback() {
    for (enabled, mode, source) in [
        (true, "", None),
        (
            true,
            "init-error",
            Some(UpdateSource {
                repository: "test/another".into(),
                tag_prefix: "plugin-fixture-v".into(),
            }),
        ),
        (false, "init-error", None),
    ] {
        let f = Fixture::new();
        f.add();
        let server = Server::new();
        server.release(f.package(mode), "1", TARGET);
        let core = f.open();
        core.set_enabled("test.fixture", enabled).unwrap();
        core.start_enabled();
        core.check_component("test.fixture", &server.client())
            .unwrap();
        core.download_plugin("test.fixture", &server.client())
            .unwrap();
        core.set_update_source("test.fixture", source.clone())
            .unwrap();
        core.set_enabled("test.fixture", true).unwrap();
        core.shutdown().unwrap();
        drop(core);

        let core = f.open();
        core.start_enabled();
        let plugin = &core.snapshot().plugins[0];
        assert_eq!(plugin.status, PluginStatus::Running);
        assert_eq!(plugin.manifest.update_source, source);
        assert_eq!(
            plugin.manifest.version,
            if mode.is_empty() { "0.1.1" } else { "0.1.0" }
        );
        core.shutdown().unwrap();
    }
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
    assert_eq!(
        fs::read(plugin.directory.join("资料/说明.txt")).unwrap(),
        "更新包的中文文件".as_bytes()
    );
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
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    archive
        .add_symlink(
            "plugin/link",
            "../../outside",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
    server.release(archive.finish().unwrap().into_inner(), "1", TARGET);
    core.check_component("test.fixture", &server.client())
        .unwrap();
    assert!(core
        .download_plugin("test.fixture", &server.client())
        .unwrap_err()
        .contains("符号链接"));
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

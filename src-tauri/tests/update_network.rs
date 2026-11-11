#![cfg(feature = "test-fixtures")]
use serde_json::json;
use sha2::{Digest, Sha256};
use sodeck::update_network::{
    normalize_prefix, Error, Preferences, RequestKind, State, Transport, UpdateMirror,
};
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
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
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Server {
    fn new(
        answer: impl Fn(&str, &str) -> (String, Vec<u8>, Option<usize>) + Send + 'static,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (recorded, stopped) = (requests.clone(), stop.clone());
        let worker = thread::spawn(move || {
            while !stopped.load(Ordering::Acquire) {
                let Ok((mut socket, _)) = listener.accept() else {
                    thread::sleep(Duration::from_millis(2));
                    continue;
                };
                socket.set_nonblocking(false).unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                socket
                    .set_write_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut byte = [0];
                while !bytes.ends_with(b"\r\n\r\n") && bytes.len() < 8192 {
                    if socket.read(&mut byte).unwrap_or(0) == 0 {
                        break;
                    }
                    bytes.push(byte[0]);
                }
                let header = String::from_utf8_lossy(&bytes);
                let path = header.split_whitespace().nth(1).unwrap_or_default();
                recorded.lock().unwrap().push(path.into());
                let (status, body, length) = answer(path, &header);
                let _ = write!(
                    socket,
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    length.unwrap_or(body.len())
                );
                let _ = socket.write_all(&body);
            }
        });
        Self {
            base,
            requests,
            stop,
            worker: Some(worker),
        }
    }
    fn prefix(&self) -> String {
        format!("{}/", self.base)
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.worker.take().unwrap().join().unwrap();
    }
}

fn failed(_: &str, _: &str) -> (String, Vec<u8>, Option<usize>) {
    ("503 Unavailable".into(), Vec::new(), None)
}
fn partial() -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "sodeck-network-{}-{}.part",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ))
}

#[test]
fn only_origin_404_confirms_optional_metadata_is_absent() {
    let direct = Server::new(|_, _| ("404 Not Found".into(), Vec::new(), None));
    let mirror = Server::new(|_, _| ("200 OK".into(), b"{}".to_vec(), None));
    let t = Transport::test_server(
        &direct.base,
        vec![(mirror.prefix(), true)],
        Arc::new(State::default()),
    );
    let url = format!("{}/repos/test/repo/releases/tags/v1.0.0", direct.base);
    assert!(t
        .json_optional::<serde_json::Value>(&url, RequestKind::Api, "application/json")
        .unwrap()
        .is_none());
    assert!(mirror.requests.lock().unwrap().is_empty());
    assert!(t
        .only_endpoint(None)
        .unwrap()
        .json::<serde_json::Value>(&url, RequestKind::Api, "application/json")
        .unwrap_err()
        .contains("404"));
}

#[test]
fn a_preferred_mirror_404_still_checks_the_origin() {
    let direct = Server::new(|path, _| {
        if path.ends_with("warmup") {
            failed(path, "")
        } else {
            ("200 OK".into(), br#"{"version":"1.0.0"}"#.to_vec(), None)
        }
    });
    let mirror = Server::new(|path, _| {
        if path.ends_with("warmup") {
            ("200 OK".into(), b"{}".to_vec(), None)
        } else {
            ("404 Not Found".into(), Vec::new(), None)
        }
    });
    let t = Transport::test_server(
        &direct.base,
        vec![(mirror.prefix(), true)],
        Arc::new(State::default()),
    );
    t.json::<serde_json::Value>(
        &format!("{}/warmup", direct.base),
        RequestKind::Api,
        "application/json",
    )
    .unwrap();
    let value = t
        .json_optional::<serde_json::Value>(
            &format!("{}/repos/test/repo/releases/tags/v1.0.0", direct.base),
            RequestKind::Api,
            "application/json",
        )
        .unwrap()
        .unwrap();
    assert_eq!(value["version"], "1.0.0");
    assert_eq!(direct.requests.lock().unwrap().len(), 2);
    assert_eq!(mirror.requests.lock().unwrap().len(), 2);
}

#[test]
fn optional_metadata_does_not_hide_network_http_or_redirect_failures() {
    let direct = Server::new(failed);
    let mirror = Server::new(|_, _| ("404 Not Found".into(), Vec::new(), None));
    let t = Transport::test_server(
        &direct.base,
        vec![(mirror.prefix(), true)],
        Arc::new(State::default()),
    );
    assert!(t
        .json_optional::<serde_json::Value>(&direct.base, RequestKind::Api, "application/json")
        .is_err());

    let foreign = Server::new(|_, _| ("404 Not Found".into(), Vec::new(), None));
    let destination = foreign.base.clone();
    let redirected = Server::new(move |_, _| {
        (
            format!("302 Found\r\nLocation: {destination}"),
            Vec::new(),
            None,
        )
    });
    let t = Transport::test_server(&redirected.base, Vec::new(), Arc::new(State::default()));
    assert!(t
        .json_optional::<serde_json::Value>(&redirected.base, RequestKind::Api, "application/json")
        .is_err());

    for status in ["403 Forbidden", "429 Too Many Requests", "500 Error"] {
        let direct = Server::new(move |_, _| (status.into(), Vec::new(), None));
        let t = Transport::test_server(&direct.base, Vec::new(), Arc::new(State::default()));
        assert!(t
            .json_optional::<serde_json::Value>(&direct.base, RequestKind::Api, "application/json")
            .is_err());
    }
}

#[test]
fn optional_metadata_preserves_size_and_cancellation_limits() {
    let direct = Server::new(|_, _| ("200 OK".into(), vec![b' '; 2 * 1024 * 1024 + 1], None));
    let t = Transport::test_server(&direct.base, Vec::new(), Arc::new(State::default()));
    assert!(t
        .json_optional::<serde_json::Value>(&direct.base, RequestKind::Api, "application/json")
        .unwrap_err()
        .contains("过大"));
    let before = direct.requests.lock().unwrap().len();
    let t = t.observed(Arc::new(|_| {}), Arc::new(|| true));
    assert!(t
        .json_optional::<serde_json::Value>(&direct.base, RequestKind::Api, "application/json")
        .is_err());
    assert_eq!(direct.requests.lock().unwrap().len(), before);

    let cancelled = Arc::new(AtomicBool::new(false));
    let requested = cancelled.clone();
    let direct = Server::new(move |_, _| {
        requested.store(true, Ordering::Release);
        ("404 Not Found".into(), Vec::new(), None)
    });
    let t = Transport::test_server(&direct.base, Vec::new(), Arc::new(State::default())).observed(
        Arc::new(|_| {}),
        Arc::new(move || cancelled.load(Ordering::Acquire)),
    );
    assert!(t
        .json_optional::<serde_json::Value>(&direct.base, RequestKind::Api, "application/json")
        .unwrap_err()
        .contains("停止"));
}

#[test]
fn defaults_and_custom_address_validation() {
    let old: Preferences = serde_json::from_value(json!({})).unwrap();
    assert!(old.fallback_enabled);
    assert_eq!(old.revision, 0);
    assert_eq!(
        normalize_prefix(" https://EXAMPLE.com/proxy ").unwrap(),
        "https://example.com/proxy/"
    );
    for bad in [
        "http://example.com",
        "https://name:secret@example.com/",
        "https://example.com/?a=1",
        "https://example.com/#a",
        "",
        "../proxy",
    ] {
        assert!(normalize_prefix(bad).is_err(), "{bad}");
    }
    let mut p = old;
    p.custom_mirrors = vec![UpdateMirror {
        name: " test ".into(),
        prefix: "https://example.com".into(),
        enabled: true,
    }];
    assert_eq!(
        p.clone().validated().unwrap().custom_mirrors[0].name,
        "test"
    );
    p.custom_mirrors.push(p.custom_mirrors[0].clone());
    assert!(p.validated().is_err());
}

#[test]
fn settings_save_preserves_other_preferences_and_rejects_stale_or_blocked_writes() {
    let root = partial();
    fs::create_dir_all(&root).unwrap();
    let core = sodeck::AppCore::open(root.clone(), "0.1.0".into()).unwrap();
    core.set_theme("dark").unwrap();
    core.set_automatic_updates(false).unwrap();
    let p = Preferences {
        fallback_enabled: false,
        ..Default::default()
    };
    let saved = core.set_update_network(0, p.clone()).unwrap();
    assert_eq!(saved.revision, 1);
    assert!(core
        .set_update_network(0, p.clone())
        .unwrap_err()
        .contains("另一窗口"));
    let snapshot = core.snapshot();
    assert_eq!(snapshot.preferences.theme, "dark");
    assert!(!snapshot.preferences.automatic_updates);
    drop(core);
    let core = sodeck::AppCore::open(root.clone(), "0.1.0".into()).unwrap();
    assert_eq!(core.snapshot().preferences.update_network, saved);
    drop(core);
    fs::write(root.join("preferences.json"), b"broken saved file").unwrap();
    let core = sodeck::AppCore::open(root.clone(), "0.1.0".into()).unwrap();
    assert!(core.set_update_network(0, p).is_err());
    assert_eq!(
        fs::read(root.join("preferences.json")).unwrap(),
        b"broken saved file"
    );
    drop(core);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn metadata_fallback_preserves_original_query_and_separates_api_health() {
    let direct = Server::new(failed);
    let bad = Server::new(|_, _| ("200 OK".into(), b"<html>bad cache</html>".to_vec(), None));
    let good = Server::new(|_, _| ("200 OK".into(), br#"{"value":42}"#.to_vec(), None));
    let files = Server::new(|_, _| ("200 OK".into(), br#"{"value":1}"#.to_vec(), None));
    let t = Transport::test_server(
        &direct.base,
        vec![
            (files.prefix(), false),
            (bad.prefix(), true),
            (good.prefix(), true),
        ],
        Arc::new(State::default()),
    );
    let original = format!("{}/repos/test/repo?q=a%2Fb&page=2", direct.base);
    let value: serde_json::Value = t
        .json(&original, RequestKind::Api, "application/json")
        .unwrap();
    assert_eq!(value["value"], 42);
    assert_eq!(good.requests.lock().unwrap()[0], format!("/{original}"));
    assert!(
        files.requests.lock().unwrap().is_empty(),
        "File-only nodes must not receive API requests"
    );
    let _: serde_json::Value = t
        .json(&original, RequestKind::Api, "application/json")
        .unwrap();
    assert_eq!(
        direct.requests.lock().unwrap().len(),
        1,
        "Cooling direct address is skipped"
    );
    let value: serde_json::Value = t
        .json(&original, RequestKind::File, "application/json")
        .unwrap();
    assert_eq!(
        value["value"], 1,
        "API failure must not cool file capability"
    );
}

#[test]
fn interrupted_or_corrupt_download_restarts_and_verifies_before_success() {
    let direct = Server::new(|_, _| ("200 OK".into(), b"unfinished".to_vec(), Some(100)));
    let bad = Server::new(|_, _| ("200 OK".into(), b"corrupt".to_vec(), None));
    let good = Server::new(|_, _| ("200 OK".into(), b"complete package".to_vec(), None));
    let t = Transport::test_server(
        &direct.base,
        vec![(bad.prefix(), true), (good.prefix(), true)],
        Arc::new(State::default()),
    );
    let path = partial();
    let mut counts = Vec::new();
    t.download(
        &format!("{}/package.zip", direct.base),
        &path,
        |count, _| counts.push(count),
        |_, hash| {
            if hash == format!("{:x}", Sha256::digest(b"complete package")) {
                Ok(())
            } else {
                Err(Error::remote("hash mismatch"))
            }
        },
    )
    .unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"complete package");
    assert_eq!(counts.iter().filter(|count| **count == 0).count(), 3);
    fs::remove_file(&path).unwrap();
}

#[test]
fn manifest_failure_only_defers_a_node_when_other_package_routes_also_fail() {
    let direct = Server::new(failed);
    let package_node = Server::new(|path, _| {
        if path.ends_with("/manifest.json") {
            failed(path, "")
        } else {
            ("200 OK".into(), b"package".to_vec(), None)
        }
    });
    let metadata_node = Server::new(|path, _| {
        if path.ends_with("/manifest.json") {
            ("200 OK".into(), b"{}".to_vec(), None)
        } else {
            failed(path, "")
        }
    });
    let t = Transport::test_server(
        &direct.base,
        vec![
            (package_node.prefix(), true),
            (metadata_node.prefix(), true),
        ],
        Arc::new(State::default()),
    );
    t.json::<serde_json::Value>(
        &format!("{}/manifest.json", direct.base),
        RequestKind::File,
        "application/json",
    )
    .unwrap();
    let path = partial();
    t.download(
        &format!("{}/package.zip", direct.base),
        &path,
        |_, _| {},
        |_, hash| {
            if hash == format!("{:x}", Sha256::digest(b"package")) {
                Ok(())
            } else {
                Err(Error::remote("hash mismatch"))
            }
        },
    )
    .unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"package");
    assert_eq!(
        direct.requests.lock().unwrap().len(),
        1,
        "Cooling direct must remain behind cooling mirrors"
    );
    fs::remove_file(path).unwrap();
}

#[test]
fn an_unsafe_proxy_redirect_is_rejected_then_another_proxy_can_succeed() {
    let direct = Server::new(failed);
    let unsafe_node = Server::new(|_, _| {
        (
            "302 Found\r\nLocation: http://example.invalid/untrusted".into(),
            Vec::new(),
            None,
        )
    });
    let good = Server::new(|_, _| ("200 OK".into(), b"{}".to_vec(), None));
    let t = Transport::test_server(
        &direct.base,
        vec![(unsafe_node.prefix(), true), (good.prefix(), true)],
        Arc::new(State::default()),
    );
    t.json::<serde_json::Value>(&direct.base, RequestKind::Api, "application/json")
        .unwrap();
    assert_eq!(unsafe_node.requests.lock().unwrap().len(), 1);
    assert_eq!(good.requests.lock().unwrap().len(), 1);
}

#[test]
fn proxy_redirects_keep_using_the_selected_proxy_and_diagnostic_does_not_fallback() {
    let direct = Server::new(failed);
    let target = format!("{}/redirected", direct.base);
    let mirror = Server::new(move |path, _| {
        if path.ends_with("/start") {
            (format!("302 Found\r\nLocation: {target}"), Vec::new(), None)
        } else {
            ("200 OK".into(), b"{}".to_vec(), None)
        }
    });
    let other = Server::new(|_, _| ("200 OK".into(), b"{}".to_vec(), None));
    let t = Transport::test_server(
        &direct.base,
        vec![(mirror.prefix(), true), (other.prefix(), true)],
        Arc::new(State::default()),
    );
    let original = format!("{}/start", direct.base);
    t.json::<serde_json::Value>(&original, RequestKind::File, "application/json")
        .unwrap();
    assert_eq!(direct.requests.lock().unwrap().len(), 1);
    assert_eq!(mirror.requests.lock().unwrap().len(), 2);
    assert!(other.requests.lock().unwrap().is_empty());
    let chosen = t.only_endpoint(Some(&direct.prefix())).unwrap();
    assert!(chosen
        .json::<serde_json::Value>(&original, RequestKind::File, "application/json")
        .is_err());
    assert!(other.requests.lock().unwrap().is_empty());
}

#[test]
fn local_failure_and_cancellation_stop_without_trying_another_address() {
    let direct = Server::new(|_, _| ("200 OK".into(), b"payload".to_vec(), None));
    let alternate = Server::new(failed);
    let t = Transport::test_server(
        &direct.base,
        vec![(alternate.prefix(), true)],
        Arc::new(State::default()),
    );
    let path = partial();
    fs::write(&path, b"existing cache").unwrap();
    assert!(t
        .download(&direct.base, &path, |_, _| {}, |_, _| Ok(()))
        .unwrap_err()
        .contains("无法创建"));
    assert_eq!(fs::read(&path).unwrap(), b"existing cache");
    assert!(alternate.requests.lock().unwrap().is_empty());
    fs::remove_file(&path).unwrap();
    let cancelled = Arc::new(AtomicBool::new(false));
    let signal = cancelled.clone();
    let t = t.observed(
        Arc::new(|_| {}),
        Arc::new(move || signal.load(Ordering::Acquire)),
    );
    assert!(t
        .download(
            &direct.base,
            &path,
            |_, _| {},
            |_, _| {
                cancelled.store(true, Ordering::Release);
                Ok(())
            }
        )
        .is_err());
    assert!(!path.exists());
    assert!(alternate.requests.lock().unwrap().is_empty());
}

#[test]
fn disabling_fallback_and_single_endpoint_diagnostics_never_use_alternates() {
    let direct = Server::new(failed);
    let alternate = Server::new(|_, _| ("200 OK".into(), b"{}".to_vec(), None));
    let prefs = Preferences {
        fallback_enabled: false,
        ..Default::default()
    };
    let t = Transport::new(prefs, Arc::new(State::default())).unwrap();
    assert!(t
        .json::<serde_json::Value>("https://127.0.0.1:1/", RequestKind::Api, "application/json")
        .is_err());
    let t = Transport::test_server(
        &direct.base,
        vec![(alternate.prefix(), true)],
        Arc::new(State::default()),
    )
    .only_endpoint(None)
    .unwrap();
    assert!(t
        .json::<serde_json::Value>(&direct.base, RequestKind::Api, "application/json")
        .is_err());
    assert!(alternate.requests.lock().unwrap().is_empty());
}

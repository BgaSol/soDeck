use serde_json::{json, Value};
use sodeck::AppCore;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
static SEQUENCE: AtomicUsize = AtomicUsize::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "sodeck-servers-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&p).unwrap();
        Self(p)
    }
    fn open(&self) -> std::sync::Arc<AppCore> {
        AppCore::open(self.0.clone(), "test".into()).unwrap()
    }
    fn disk(&self) -> Value {
        serde_json::from_slice(&fs::read(self.0.join("servers.json")).unwrap()).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn change(core: &AppCore, v: Value) -> Result<Value, String> {
    core.change_catalog(
        core.server_catalog()["revision"].as_u64().unwrap(),
        serde_json::from_value(v).unwrap(),
    )
}
fn server(host: &str) -> Value {
    json!({"op":"saveServer","draft":{"id":null,"name":"","groupId":null,"host":host,"port":22,"username":"ops","authentication":{"type":"password","secret":{"action":"keep","remember":false}},"route":[]}})
}
fn proxy() -> Value {
    json!({"op":"saveProxy","draft":{"id":null,"name":"Local proxy","host":"127.0.0.1","port":1080,"authentication":{"type":"none","secret":{"action":"clear"}}}})
}
fn id(v: Value) -> String {
    v["selectedId"].as_str().unwrap().into()
}
fn edit(core: &AppCore, id: &str) -> Value {
    let v = core.server_catalog();
    let mut d = v["servers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == id)
        .unwrap()
        .clone();
    d["authentication"] = json!({"type":"password","secret":{"action":"keep","remember":true}});
    json!({"op":"saveServer","draft":d})
}
#[test]
fn catalog_dates_preserve_existing_creation_times_and_refresh_copies() {
    let f = Fixture::new();
    let core = f.open();
    let server_id = id(change(&core, server("date.invalid")).unwrap());
    let proxy_id = id(change(&core, proxy()).unwrap());
    drop(core);
    let mut catalog = f.disk();
    let original_time = "2000-02-29T23:59:59Z";
    for records in ["servers", "proxies"] {
        for record in catalog[records].as_array_mut().unwrap() {
            record["createdAt"] = json!(original_time);
            record["updatedAt"] = json!(original_time);
        }
    }
    fs::write(
        f.0.join("servers.json"),
        serde_json::to_vec(&catalog).unwrap(),
    )
    .unwrap();
    let core = f.open();
    change(&core, edit(&core, &server_id)).unwrap();
    let mut proxy_draft = proxy();
    proxy_draft["draft"]["id"] = json!(proxy_id);
    change(&core, proxy_draft).unwrap();
    change(&core, json!({"op":"copyServer","id":server_id})).unwrap();
    change(&core, json!({"op":"copyProxy","id":proxy_id})).unwrap();
    let updated = core.server_catalog();
    for records in ["servers", "proxies"] {
        let rows = updated[records].as_array().unwrap();
        assert_eq!(rows[0]["createdAt"], original_time);
        assert_ne!(rows[0]["updatedAt"], original_time);
        assert_eq!(rows[1]["createdAt"], rows[1]["updatedAt"]);
        assert_ne!(rows[1]["createdAt"], original_time);
        for row in rows {
            for field in ["createdAt", "updatedAt"] {
                let time = row[field].as_str().unwrap();
                assert_eq!(time.len(), 20);
                assert!(time.ends_with('Z'));
                assert_eq!(&time[10..11], "T");
            }
        }
    }
    drop(core);
    let reopened = f.open().server_catalog();
    for stored in [f.disk(), reopened] {
        for records in ["servers", "proxies"] {
            for (row, expected) in stored[records]
                .as_array()
                .unwrap()
                .iter()
                .zip(updated[records].as_array().unwrap())
            {
                assert_eq!(row["createdAt"], expected["createdAt"]);
                assert_eq!(row["updatedAt"], expected["updatedAt"]);
            }
        }
    }
}

#[test]
fn mixed_routes_reopen_and_crud() {
    let f = Fixture::new();
    let core = f.open();
    let g = id(change(&core, json!({"op":"saveGroup","id":null,"name":"生产"})).unwrap());
    let a = id(change(&core, server("gateway.invalid")).unwrap());
    let p = id(change(&core, proxy()).unwrap());
    let mut b = server("db.internal");
    b["draft"]["groupId"] = json!(g);
    b["draft"]["route"] = json!([{"id":"","kind":"ssh","serverId":a},{"id":"","kind":"socks5","proxyId":p},{"id":"","kind":"socks5","proxyId":p}]);
    let b = id(change(&core, b).unwrap());
    let mut rename = edit(&core, &a);
    rename["draft"]["name"] = json!("新名称");
    change(&core, rename).unwrap();
    assert!(change(&core, json!({"op":"deleteServer","id":a}))
        .unwrap_err()
        .contains("db.internal"));
    assert!(change(&core, json!({"op":"deleteProxy","id":p})).is_err());
    let copy = id(change(&core, json!({"op":"copyServer","id":b})).unwrap());
    let view = core.server_catalog();
    let original = view["servers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == b)
        .unwrap();
    let duplicate = view["servers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == copy)
        .unwrap();
    assert_ne!(original["route"][0]["id"], duplicate["route"][0]["id"]);
    change(&core, json!({"op":"deleteGroup","id":g})).unwrap();
    change(&core, json!({"op":"deleteServer","id":copy})).unwrap();
    let view = core.server_catalog();
    drop(core);
    let core = f.open();
    assert_eq!(view, core.server_catalog());
    let bview = core.server_catalog();
    let bview = bview["servers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == b)
        .unwrap();
    assert!(bview["groupId"].is_null());
    assert_eq!(bview["route"].as_array().unwrap().len(), 3);
    change(&core, json!({"op":"deleteServer","id":b})).unwrap();
    change(&core, json!({"op":"deleteServer","id":a})).unwrap();
    change(&core, json!({"op":"deleteProxy","id":p})).unwrap();
    assert!(core.server_catalog()["servers"]
        .as_array()
        .unwrap()
        .is_empty());
}
#[test]
fn secret_keep_replace_clear_session_and_copy() {
    let f = Fixture::new();
    let core = f.open();
    let mut s = server("host.invalid");
    s["draft"]["authentication"]["secret"] =
        json!({"action":"replace","value":"test-secret-1","remember":true});
    let a = id(change(&core, s).unwrap());
    assert!(f.disk().to_string().contains("test-secret-1"));
    assert!(!core.server_catalog().to_string().contains("test-secret-1"));
    assert!(!serde_json::to_string(&core.snapshot())
        .unwrap()
        .contains("test-secret-1"));
    change(&core, edit(&core, &a)).unwrap();
    assert!(f.disk().to_string().contains("test-secret-1"));
    let copy = id(change(&core, json!({"op":"copyServer","id":a})).unwrap());
    assert!(!f.disk()["servers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == copy)
        .unwrap()
        .to_string()
        .contains("test-secret-1"));
    let mut d = edit(&core, &a);
    d["draft"]["authentication"]["secret"] = json!({"action":"keep","remember":false});
    change(&core, d).unwrap();
    assert!(!f.disk().to_string().contains("test-secret-1"));
    assert_eq!(
        core.server_catalog()["servers"][1]["authentication"]["hasSessionSecret"],
        true
    );
    drop(core);
    let core = f.open();
    assert!(!core
        .server_catalog()
        .to_string()
        .contains("\"hasSessionSecret\":true"));
    let mut d = edit(&core, &a);
    d["draft"]["authentication"]["secret"] =
        json!({"action":"replace","value":"test-secret-2","remember":true});
    change(&core, d).unwrap();
    let mut d = edit(&core, &a);
    d["draft"]["host"] = json!("new.invalid");
    change(&core, d).unwrap();
    assert!(!f.disk().to_string().contains("test-secret-2"));
    let mut d = edit(&core, &a);
    d["draft"]["authentication"] = json!({"type":"privateKey","privateKeyPath":"/home/ops/.ssh/id_ed25519","secret":{"action":"replace","value":"test-passphrase","remember":true}});
    change(&core, d).unwrap();
    let mut d = edit(&core, &a);
    d["draft"]["authentication"] = json!({"type":"privateKey","privateKeyPath":"/home/ops/.ssh/id_ed25519","secret":{"action":"clear"}});
    change(&core, d).unwrap();
    assert!(!f.disk().to_string().contains("test-passphrase"));
}
#[test]
fn proxy_credentials_are_separate_and_not_copied() {
    let f = Fixture::new();
    let core = f.open();
    let mut p = proxy();
    p["draft"]["authentication"] = json!({"type":"usernamePassword","username":"proxy-user","secret":{"action":"replace","value":"proxy-test-secret","remember":true}});
    let p = id(change(&core, p).unwrap());
    let copy = id(change(&core, json!({"op":"copyProxy","id":p})).unwrap());
    assert!(!core
        .server_catalog()
        .to_string()
        .contains("proxy-test-secret"));
    assert!(!f.disk()["proxies"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == copy)
        .unwrap()
        .to_string()
        .contains("proxy-test-secret"));
    drop(core);
    let core = f.open();
    assert_eq!(
        core.server_catalog()["proxies"][0]["authentication"]["hasSavedSecret"],
        true
    );
}
#[test]
fn bad_routes_and_stale_revisions_do_not_mutate() {
    let f = Fixture::new();
    let core = f.open();
    let a = id(change(&core, server("same.internal")).unwrap());
    let b = id(change(&core, server("same.internal")).unwrap()); // same IP in different contexts is allowed
    let before = core.server_catalog();
    for route in [
        json!([{"id":"","kind":"ssh","serverId":a}]),
        json!([{"id":"","kind":"ssh","serverId":"missing"}]),
        json!([{"id":"","kind":"ssh","serverId":b},{"id":"","kind":"ssh","serverId":b}]),
    ] {
        let mut d = edit(&core, &a);
        d["draft"]["route"] = route;
        assert!(change(&core, d).is_err());
    }
    let mut d = edit(&core, &a);
    d["draft"]["port"] = json!(0);
    assert!(change(&core, d).is_err());
    assert!(core
        .change_catalog(0, serde_json::from_value(server("stale.invalid")).unwrap())
        .is_err());
    assert_eq!(before, core.server_catalog());
    let mut d = edit(&core, &a);
    d["draft"]["route"] = json!([{"id":"","kind":"ssh","serverId":b}]);
    change(&core, d).unwrap();
    let mut d = edit(&core, &b);
    d["draft"]["route"] = json!([{"id":"","kind":"ssh","serverId":a}]);
    change(&core, d).unwrap(); // references are endpoints, never recursive routes
}
#[test]
fn long_route_is_not_limited_and_step_ids_stay_stable() {
    let f = Fixture::new();
    let core = f.open();
    let p = id(change(&core, proxy()).unwrap());
    let mut s = server("[fd00::5]");
    s["draft"]["port"] = json!(2222);
    s["draft"]["route"] = Value::Array(
        (0..40)
            .map(|_| json!({"id":"","kind":"socks5","proxyId":p}))
            .collect(),
    );
    let a = id(change(&core, s).unwrap());
    let mut d = edit(&core, &a);
    let step = d["draft"]["route"][0]["id"].clone();
    d["draft"]["route"].as_array_mut().unwrap().reverse();
    change(&core, d).unwrap();
    let v = core.server_catalog();
    assert_eq!(v["servers"][0]["route"][39]["id"], step);
    assert_eq!(v["servers"][0]["name"], "[fd00::5]:2222");
}
#[test]
fn invalid_files_block_writes_and_backup_recovers() {
    let f = Fixture::new();
    let core = f.open();
    change(&core, server("first.invalid")).unwrap();
    change(&core, server("second.invalid")).unwrap();
    drop(core);
    for invalid in [
        "{broken",
        r#"{"schemaVersion":2,"revision":1,"groups":[],"servers":[],"proxies":[]}"#,
    ] {
        fs::write(f.0.join("servers.json"), invalid).unwrap();
        let core = f.open();
        assert!(core.server_catalog()["error"].is_string());
        assert!(change(&core, server("no-overwrite.invalid")).is_err());
        assert_eq!(
            fs::read_to_string(f.0.join("servers.json")).unwrap(),
            invalid
        );
        let v = core.recover_catalog().unwrap();
        assert!(v["error"].is_null());
        assert_eq!(v["servers"].as_array().unwrap().len(), 1);
        drop(core);
    }
    assert!(fs::read_dir(&f.0).unwrap().any(|p| p
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with("servers.corrupt-")));
    let mut bad = f.disk();
    bad["servers"][0]["route"] = json!([{"id":"bad","kind":"socks5","proxyId":"missing"}]);
    fs::write(f.0.join("servers.json"), bad.to_string()).unwrap();
    let core = f.open();
    assert!(core.server_catalog()["error"].is_string());
}
#[test]
fn failed_write_keeps_memory_disk_and_secret() {
    let f = Fixture::new();
    let core = f.open();
    let a = id(change(&core, server("original.invalid")).unwrap());
    let before = core.server_catalog();
    let disk = f.disk();
    fs::create_dir(f.0.join("servers.json.tmp")).unwrap();
    let mut d = edit(&core, &a);
    d["draft"]["name"] = json!("unsaved");
    d["draft"]["authentication"]["secret"] =
        json!({"action":"replace","value":"unsaved-secret","remember":false});
    assert!(change(&core, d).is_err());
    let mut after = core.server_catalog();
    after["canRecover"] = before["canRecover"].clone();
    assert_eq!(before, after);
    assert_eq!(disk, f.disk());
}

#[path = "support/write_failure.rs"]
mod write_failure;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use sodeck::AppCore;
use std::{fs, path::Path, sync::Arc};

fn open(path: &Path) -> Arc<AppCore> {
    AppCore::open(path.into(), "sharing-test".into()).unwrap()
}
fn change(core: &AppCore, value: Value) -> String {
    core.change_catalog(revision(core), serde_json::from_value(value).unwrap())
        .unwrap()["selectedId"]
        .as_str()
        .unwrap_or("")
        .into()
}
fn revision(core: &AppCore) -> u64 {
    core.server_catalog()["revision"].as_u64().unwrap()
}
fn server(core: &AppCore, name: &str, route: Value) -> String {
    change(
        core,
        json!({"op":"saveServer","draft":{"name":name,"host":"same.internal","port":22,"username":"ops","tagIds":[],"newTagNames":["生产"],"authentication":{"type":"password","secret":{"action":"replace","value":"secret-marker","remember":false}},"route":route}}),
    )
}
fn key(core: &AppCore, name: &str, path: &Path) -> String {
    change(
        core,
        json!({"op":"saveServer","draft":{"name":name,"host":"same.internal","port":22,"username":"ops","authentication":{"type":"privateKey","privateKeyPath":path,"secret":{"action":"keep","remember":true}},"route":[]}}),
    )
}
fn code(core: &AppCore, id: Option<&str>) -> String {
    core.export_server_share(revision(core), id.map(str::to_owned))
        .unwrap()["code"]
        .as_str()
        .unwrap()
        .into()
}
fn bundle(code: &str) -> Value {
    serde_json::from_slice(&STANDARD.decode(&code[8..]).unwrap()).unwrap()
}
fn encode(v: Value) -> String {
    format!(
        "SODECK1:{}",
        STANDARD.encode(serde_json::to_vec(&v).unwrap())
    )
}
fn import(core: &AppCore, code: &str) -> Value {
    let p = core.preview_server_share(code).unwrap();
    core.import_server_share(p["catalogRevision"].as_u64().unwrap(), code)
        .unwrap()
}
fn make_key(path: &Path) {
    let key =
        russh::keys::PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519).unwrap();
    fs::write(
        path,
        key.to_openssh(russh::keys::ssh_key::LineEnding::LF)
            .unwrap()
            .as_bytes(),
    )
    .unwrap();
}

#[test]
fn single_shares_only_explicit_route_and_all_preserves_saved_routes() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let source = open(a.path());
    let target = open(b.path());
    let p = change(
        &source,
        json!({"op":"saveProxy","draft":{"name":"proxy","host":"127.0.0.1","port":1080,"authentication":{"type":"usernamePassword","username":"proxy-user","secret":{"action":"replace","value":"proxy-secret","remember":true}}}}),
    );
    let unrelated = server(&source, "unrelated", json!([]));
    let hop = server(
        &source,
        "hop",
        json!([{"id":"","kind":"ssh","serverId":unrelated}]),
    );
    let route = json!([{"id":"","kind":"socks5","proxyId":p},{"id":"","kind":"ssh","serverId":hop},{"id":"","kind":"socks5","proxyId":p}]);
    let id = server(&source, "target", route);
    let share = code(&source, Some(&id));
    let content = bundle(&share);
    assert_eq!(content["servers"].as_array().unwrap().len(), 2);
    assert!(!content.to_string().contains("unrelated"));
    assert_eq!(content["servers"][0]["route"].as_array().unwrap().len(), 1);
    let before = revision(&target);
    let preview = target.preview_server_share(&share).unwrap();
    assert_eq!(revision(&target), before);
    assert!(!b.path().join("servers.json").exists());
    assert_eq!(preview["targetCount"], 1);
    let result = import(&target, &share);
    assert_eq!(revision(&target), before + 1);
    let imported = result["catalog"]["servers"].as_array().unwrap();
    assert_eq!(imported.len(), 2);
    assert_eq!(imported[1]["route"].as_array().unwrap().len(), 3);
    assert!(!result.to_string().contains("secret-marker"));
    assert!(fs::read_to_string(b.path().join("servers.json"))
        .unwrap()
        .contains("secret-marker"));
    let all = code(&source, None);
    let all_content = bundle(&all);
    assert_eq!(all_content["servers"].as_array().unwrap().len(), 3);
    assert!(all_content.to_string().contains("unrelated"));
    import(&target, &share);
    assert_eq!(
        target.server_catalog()["servers"][3]["name"],
        "target（导入）"
    );
    assert_eq!(target.server_catalog()["tags"].as_array().unwrap().len(), 1);
    let stale = target.preview_server_share(&share).unwrap();
    server(&target, "new", json!([]));
    let before = target.server_catalog();
    assert_eq!(
        target
            .import_server_share(stale["catalogRevision"].as_u64().unwrap(), &share)
            .unwrap_err()
            .code,
        "catalogChanged"
    );
    assert_eq!(before, target.server_catalog());
}

#[test]
fn managed_key_survives_source_removal_copy_backup_and_restart() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let source = open(a.path());
    let destination = open(b.path());
    let path = a.path().join("source-key");
    make_key(&path);
    let id = key(&source, "key-target", &path);
    key(&source, "second", &path);
    let share = code(&source, None);
    assert_eq!(bundle(&share)["keys"].as_array().unwrap().len(), 1);
    assert!(!bundle(&share).to_string().contains("source-key"));
    let imported = import(&destination, &share);
    let server = &imported["catalog"]["servers"][0];
    let saved = server["authentication"]["privateKeyPath"].as_str().unwrap();
    assert_ne!(saved, path.to_str().unwrap());
    assert!(Path::new(saved).is_file());
    assert_eq!(
        server["authentication"]["privateKeyPath"],
        imported["catalog"]["servers"][1]["authentication"]["privateKeyPath"]
    );
    fs::remove_file(path).unwrap();
    let saved = saved.to_owned();
    let mut draft = server.clone();
    draft["id"] = Value::Null;
    draft["copyFromId"] = Value::Null;
    draft["name"] = json!("copy");
    draft["authentication"]["secret"] = json!({"action":"keep","remember":true});
    change(&destination, json!({"op":"saveServer","draft":draft}));
    drop(destination);
    let destination = open(b.path());
    assert!(Path::new(&saved).is_file());
    assert_eq!(
        destination.server_catalog()["servers"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    // The backup must retain keys even after all current references have been removed in one import-free rewrite.
    let catalog_file = b.path().join("servers.json");
    let original = fs::read(&catalog_file).unwrap();
    fs::write(b.path().join("servers.json.bak"), &original).unwrap();
    drop(destination);
    let mut empty: Value = serde_json::from_slice(&original).unwrap();
    empty["servers"] = json!([]);
    fs::write(&catalog_file, serde_json::to_vec(&empty).unwrap()).unwrap();
    let destination = open(b.path());
    assert!(Path::new(&saved).is_file());
    drop(destination);
    fs::write(&catalog_file, b"{bad").unwrap();
    let destination = open(b.path());
    destination.recover_catalog().unwrap();
    assert!(Path::new(&saved).is_file());
    assert!(!id.is_empty());
}

#[test]
fn malformed_unreferenced_and_oversized_shares_never_write() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let source = open(a.path());
    let target = open(b.path());
    let id = server(&source, "target", json!([]));
    let valid = code(&source, Some(&id));
    for code in [
        "bad".into(),
        "SODECK2:AA==".into(),
        format!("{valid}x"),
        "x".repeat(sodeck::servers::SHARE_LIMIT + 1),
    ] {
        assert!(target.preview_server_share(&code).is_err());
    }
    let mut v = bundle(&valid);
    v["servers"][0]["route"] = json!([{"kind":"ssh","id":"missing"}]);
    assert!(target.preview_server_share(&encode(v)).is_err());
    let mut v = bundle(&valid);
    v["servers"][0]["authentication"] =
        json!({"type":"privateKey","privateKeyPath":"/etc/passwd","keyId":null,"passphrase":null});
    let e = target.preview_server_share(&encode(v)).unwrap_err();
    assert!(!e.message.contains("passwd"));
    let mut v = bundle(&valid);
    let duplicate = v["servers"][0].clone();
    v["servers"].as_array_mut().unwrap().push(duplicate);
    assert!(target.preview_server_share(&encode(v)).is_err());
    let wrapped = format!(" \r\n{}\n{}\t", &valid[..20], &valid[20..]);
    assert!(target.preview_server_share(&wrapped).is_ok());
    assert!(!b.path().join("servers.json").exists());
    assert!(!b.path().join("server-keys").exists());
}

#[test]
fn final_write_failure_rolls_back_key_files_and_catalog() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let source = open(a.path());
    let target = open(b.path());
    let path = a.path().join("key");
    make_key(&path);
    key(&source, "key", &path);
    let share = code(&source, None);
    server(&target, "existing", json!([]));
    server(&target, "second original", json!([]));
    let before = target.server_catalog();
    let Some(block) = write_failure::block(b.path().join("servers.json")) else {
        return;
    };
    assert_eq!(
        target
            .import_server_share(revision(&target), &share)
            .unwrap_err()
            .code,
        "persistenceFailed"
    );
    drop(block);
    assert_eq!(before, target.server_catalog());
    assert!(fs::read_dir(b.path().join("server-keys"))
        .unwrap()
        .next()
        .is_none());
    let result = import(&target, &share);
    assert_eq!(result["catalog"]["servers"].as_array().unwrap().len(), 3);
}

#[test]
fn orphan_cleanup_is_conservative_and_never_deletes_external_keys() {
    let a = tempfile::tempdir().unwrap();
    let source = open(a.path());
    server(&source, "existing", json!([]));
    drop(source);
    let root = a.path().join("server-keys");
    let orphan = root.join("import-AbCdEf");
    fs::create_dir_all(&orphan).unwrap();
    fs::write(orphan.join("key-0"), "synthetic").unwrap();
    let unknown = root.join("import-Unknown");
    fs::create_dir_all(&unknown).unwrap();
    fs::write(unknown.join("notes"), "keep").unwrap();
    let external = a.path().join("external");
    fs::write(&external, "keep").unwrap();
    let c = open(a.path());
    assert!(!orphan.exists());
    assert!(external.exists());
    assert!(unknown.exists());
    drop(c);
    fs::create_dir_all(&orphan).unwrap();
    fs::write(orphan.join("key-0"), "synthetic").unwrap();
    fs::write(a.path().join("servers.json.bak"), "bad").unwrap();
    let _c = open(a.path());
    assert!(orphan.exists());
}

#[test]
fn plain_and_encrypted_rsa_and_ed25519_roundtrip_without_reencoding() {
    for algorithm in [
        russh::keys::Algorithm::Ed25519,
        russh::keys::Algorithm::Rsa { hash: None },
    ] {
        let material = russh::keys::PrivateKey::random(&mut rand::rng(), algorithm).unwrap();
        for encrypted in [false, true] {
            let a = tempfile::tempdir().unwrap();
            let b = tempfile::tempdir().unwrap();
            let source = open(a.path());
            let destination = open(b.path());
            let material = if encrypted {
                material
                    .encrypt(&mut rand::rng(), "test-passphrase")
                    .unwrap()
            } else {
                material.clone()
            };
            let content = material
                .to_openssh(russh::keys::ssh_key::LineEnding::LF)
                .unwrap();
            let path = a.path().join("key");
            fs::write(&path, content.as_bytes()).unwrap();
            let id = key(&source, "key", &path);
            let share = code(&source, Some(&id));
            let preview = destination.preview_server_share(&share).unwrap();
            assert_eq!(preview["rows"][0]["needsCredentials"], encrypted);
            let result = import(&destination, &share);
            let path = Path::new(
                result["catalog"]["servers"][0]["authentication"]["privateKeyPath"]
                    .as_str()
                    .unwrap(),
            );
            assert_eq!(fs::read(path).unwrap(), content.as_bytes());
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(
                    fs::metadata(path).unwrap().permissions().mode() & 0o777,
                    0o600
                );
            }
        }
    }
}

#[test]
fn missing_auth_long_routes_and_large_text_keep_explicit_semantics() {
    let a = tempfile::tempdir().unwrap();
    let source = open(a.path());
    let p = change(
        &source,
        json!({"op":"saveProxy","draft":{"name":"proxy","host":"localhost","port":1080,"authentication":{"type":"none","secret":{"action":"clear"}}}}),
    );
    let id = server(
        &source,
        "long",
        json!((0..40)
            .map(|_| json!({"id":"","kind":"socks5","proxyId":p}))
            .collect::<Vec<_>>()),
    );
    let share = code(&source, Some(&id));
    let mut value = bundle(&share);
    value["servers"][0]["authentication"] = json!({"type":"password","password":null});
    let share = encode(value);
    let preview = source
        .preview_server_share(&format!("{}{share}", " ".repeat(300000)))
        .unwrap();
    assert_eq!(preview["rows"][0]["needsCredentials"], true);
    let result = import(&source, &share);
    assert_eq!(
        result["catalog"]["servers"][1]["route"]
            .as_array()
            .unwrap()
            .len(),
        40
    );
    let missing = a.path().join("missing-key");
    let id = key(&source, "unreadable", &missing);
    assert_eq!(
        source
            .export_server_share(revision(&source), Some(id))
            .unwrap_err()
            .code,
        "keyReadFailed"
    );
}

#[test]
fn key_directory_and_backup_write_failures_preserve_existing_directory() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let source = open(a.path());
    let target = open(b.path());
    let path = a.path().join("key");
    make_key(&path);
    key(&source, "key", &path);
    let share = code(&source, None);
    server(&target, "existing", json!([]));
    server(&target, "second", json!([]));
    let before = target.server_catalog();
    fs::write(b.path().join("server-keys"), "occupied").unwrap();
    assert!(target
        .import_server_share(revision(&target), &share)
        .is_err());
    assert_eq!(before, target.server_catalog());
    fs::remove_file(b.path().join("server-keys")).unwrap();
    let Some(block) = write_failure::block(b.path().join("servers.json.bak")) else {
        return;
    };
    assert!(target
        .import_server_share(revision(&target), &share)
        .is_err());
    drop(block);
    assert_eq!(before, target.server_catalog());
    assert!(fs::read_dir(b.path().join("server-keys"))
        .unwrap()
        .next()
        .is_none());
}

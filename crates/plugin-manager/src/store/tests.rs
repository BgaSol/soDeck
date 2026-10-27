use super::*;
use base64::Engine;
use ring::signature::KeyPair;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::io::Write;

struct Fixture {
    dir: tempfile::TempDir,
    key: ring::signature::Ed25519KeyPair,
    keys: TrustedKeys,
}
impl Fixture {
    fn new() -> Self {
        let doc = ring::signature::Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new())
            .unwrap();
        let key = ring::signature::Ed25519KeyPair::from_pkcs8(doc.as_ref()).unwrap();
        let keys = BTreeMap::from([("dev".into(), key.public_key().as_ref().to_vec())]);
        Self {
            dir: tempfile::tempdir().unwrap(),
            key,
            keys,
        }
    }
    fn package(&self, version: &str, text: &str) -> PathBuf {
        self.write_package(version, self.files(version, text))
    }
    fn files(&self, version: &str, text: &str) -> BTreeMap<String, Vec<u8>> {
        let manifest = json!({"pluginId":"sodeck.terminal","version":version,
            "requires":{"core":">=2.0.0, <3.0.0","pluginApi":"^4.0.0","capabilities":["remote.pty.v1"]},
            "entry":{"ui":"ui/index.html","worker":"worker/main.js"},"dataSchemaVersion":1,
            "contributes":{"views":[{"id":"terminal.session","title":"Terminal","scope":"server","supportedHosts":["tab","window","dock"],"dockEdges":["bottom"],"defaultLocation":{"host":"tab"}}]}});
        BTreeMap::from([
            (
                "manifest.json".to_owned(),
                serde_json::to_vec(&manifest).unwrap(),
            ),
            ("ui/index.html".to_owned(), text.as_bytes().to_vec()),
            (
                "worker/main.js".to_owned(),
                b"globalThis.sodeckPlugin={invoke(){return true}}".to_vec(),
            ),
        ])
    }
    fn write_package(&self, version: &str, mut files: BTreeMap<String, Vec<u8>>) -> PathBuf {
        let integrity: BTreeMap<_, _> = files
            .iter()
            .map(|(p, b)| (p.clone(), format!("{:x}", Sha256::digest(b))))
            .collect();
        let integrity = serde_json::to_vec(&integrity).unwrap();
        let signature = self.key.sign(&integrity);
        files.insert("signature.json".into(), serde_json::to_vec(&json!({"keyId":"dev","signature":base64::engine::general_purpose::STANDARD.encode(signature.as_ref())})).unwrap());
        files.insert("integrity.json".into(), integrity);
        let path = self
            .dir
            .path()
            .join(format!("{version}-{}.sodeck-plugin", uuid::Uuid::new_v4()));
        let mut zip = zip::ZipWriter::new(File::create(&path).unwrap());
        for (name, bytes) in files {
            zip.start_file(
                name,
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored),
            )
            .unwrap();
            zip.write_all(&bytes).unwrap();
        }
        zip.finish().unwrap();
        path
    }
    fn manager(&self) -> PluginManager {
        PluginManager::open(self.dir.path().join("store"), self.keys.clone()).unwrap()
    }
}

#[test]
fn update_is_pending_until_startup_and_data_is_copied() {
    let f = Fixture::new();
    let mut manager = f.manager();
    manager.prepare_package(f.package("0.1.0", "old")).unwrap();
    assert!(manager.active("sodeck.terminal").unwrap().is_none());
    manager.activate_pending(|_, _| Ok(())).unwrap();
    let old = manager.active("sodeck.terminal").unwrap().unwrap();
    fs::write(old.data_dir.join("settings.json"), "old-settings").unwrap();
    manager
        .prepare_package(f.package("0.1.1", "new-reset-button"))
        .unwrap();
    assert_eq!(
        manager
            .active("sodeck.terminal")
            .unwrap()
            .unwrap()
            .manifest
            .version,
        "0.1.0"
    );
    assert_eq!(
        manager.status()[0].pending_version.as_deref(),
        Some("0.1.1")
    );
    drop(manager);
    let mut manager = f.manager();
    manager
        .activate_pending(|_, data| {
            assert_eq!(
                fs::read_to_string(data.join("settings.json")).unwrap(),
                "old-settings"
            );
            fs::write(data.join("settings.json"), "new-settings").unwrap();
            Ok(())
        })
        .unwrap();
    let new = manager.active("sodeck.terminal").unwrap().unwrap();
    assert_eq!(new.manifest.version, "0.1.1");
    assert_ne!(new.data_dir, old.data_dir);
    assert_eq!(
        fs::read_to_string(old.data_dir.join("settings.json")).unwrap(),
        "old-settings"
    );
}

#[test]
fn verified_display_names_and_presentation_do_not_change_legacy_status() {
    let f=Fixture::new();let mut manager=f.manager();
    manager.prepare_package(f.package("0.1.0","old")).unwrap();manager.activate_pending(|_,_|Ok(())).unwrap();
    assert_eq!(manager.metadata()[0]["displayName"],"Terminal");
    let mut files=f.files("0.2.0","new");
    let mut manifest:serde_json::Value=serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest["displayName"]=json!("  中文插件  ");
    manifest["requires"]["capabilities"]=json!(["workspace.contributions.v2"]);
    manifest["contributes"]["commands"]=json!([{"id":"new","title":"新建页面","scope":"server","primary":true}]);
    manifest["contributes"]["presentation"]=json!({"tabCreate":[{"commandId":"new","icon":"plus"}]});
    files.insert("manifest.json".into(),serde_json::to_vec(&manifest).unwrap());
    manager.prepare_package(f.write_package("0.2.0",files)).unwrap();
    assert_eq!(manager.metadata()[0]["displayName"],"Terminal");
    assert_eq!(manager.metadata()[0]["pendingDisplayName"],"中文插件");
    assert!(serde_json::to_value(manager.status()).unwrap()[0].get("displayName").is_none());
    manager.activate_pending(|_,_|Ok(())).unwrap();
    assert_eq!(manager.metadata()[0]["displayName"],"中文插件");
    let active=manager.active("sodeck.terminal").unwrap().unwrap();
    assert!(serde_json::to_value(&active.manifest.contributes.commands[0]).unwrap().get("presentation").is_none());
    for bad in [json!({"tabCreate":[{"commandId":"missing","icon":"plus"}]}),json!({"tools":[{"viewType":"terminal.session","commandId":"new","edge":"right","icon":"folder","defaultWidth":360}]}),json!({"dialogs":[{"id":"edit","title":"编辑","width":560,"height":520}]})] {
        let mut invalid=manifest.clone();invalid["contributes"]["presentation"]=bad;
        let parsed:Manifest=serde_json::from_value(invalid).unwrap();assert!(package::validate_manifest(&parsed).is_err());
    }
}

#[test]
fn initialization_failure_preserves_active_data() {
    let f = Fixture::new();
    let mut manager = f.manager();
    manager.prepare_package(f.package("0.1.0", "old")).unwrap();
    manager.activate_pending(|_, _| Ok(())).unwrap();
    let old = manager.active("sodeck.terminal").unwrap().unwrap();
    fs::write(old.data_dir.join("state"), "original").unwrap();
    manager.prepare_package(f.package("0.1.1", "new")).unwrap();
    manager
        .activate_pending(|_, data| {
            fs::write(data.join("state"), "mutated-copy").unwrap();
            Err("fixture failure".into())
        })
        .unwrap();
    assert_eq!(
        manager
            .active("sodeck.terminal")
            .unwrap()
            .unwrap()
            .manifest
            .version,
        "0.1.0"
    );
    assert_eq!(
        fs::read_to_string(old.data_dir.join("state")).unwrap(),
        "original"
    );
    assert!(manager.status()[0]
        .error
        .as_ref()
        .unwrap()
        .contains("fixture failure"));
    assert!(manager.status()[0].pending_version.is_none());
}

#[test]
fn bundled_package_does_not_replace_an_independent_pending_update() {
    let f=Fixture::new();let mut manager=f.manager();
    let factory=f.package("0.1.0","factory");manager.prepare_bundled(&factory).unwrap();manager.activate_pending(|_,_|Ok(())).unwrap();
    manager.prepare_package(f.package("0.1.1","candidate")).unwrap();manager.prepare_bundled(&factory).unwrap();
    assert_eq!(manager.status()[0].pending_version.as_deref(),Some("0.1.1"));manager.activate_pending(|_,_|Ok(())).unwrap();manager.prepare_bundled(&factory).unwrap();
    assert_eq!(manager.status()[0].active_version.as_deref(),Some("0.1.1"));assert!(manager.status()[0].pending_version.is_none());
}

#[test]
fn package_versions_are_immutable_and_trust_is_required() {
    let f = Fixture::new();
    let mut manager = f.manager();
    let first = f.package("0.1.0", "old");
    manager.prepare_package(&first).unwrap();
    assert!(manager
        .prepare_package(f.package("0.1.0", "mutated"))
        .is_err());
    let other = Fixture::new();
    assert!(manager
        .prepare_package(other.package("0.1.1", "forged"))
        .is_err());
    manager.prepare_package(first).unwrap(); // Exact repeat is safe.
}

#[test]
fn active_files_are_revalidated_and_traversal_is_rejected() {
    let f = Fixture::new();
    let mut manager = f.manager();
    manager.prepare_package(f.package("0.1.0", "old")).unwrap();
    manager.activate_pending(|_, _| Ok(())).unwrap();
    let active = manager.active("sodeck.terminal").unwrap().unwrap();
    fs::write(active.ui_path, "tampered").unwrap();
    assert!(manager.active("sodeck.terminal").is_err());
    let bad = f.write_package(
        "escape",
        BTreeMap::from([("../escaped".into(), b"no".to_vec())]),
    );
    assert!(manager.prepare_package(bad).is_err());
    assert!(!f.dir.path().join("escaped").exists());
}

#[test]
fn writer_lock_prevents_simultaneous_activation() {
    let f = Fixture::new();
    let manager = f.manager();
    assert!(PluginManager::open(f.dir.path().join("store"), f.keys.clone()).is_err());
    drop(manager);
    assert!(PluginManager::open(f.dir.path().join("store"), f.keys).is_ok());
}

#[test]
fn recovery_distinguishes_precommit_and_postcommit_without_reverting_data() {
    let f = Fixture::new();
    let mut manager = f.manager();
    manager.prepare_package(f.package("0.1.0", "old")).unwrap();
    manager.activate_pending(|_, _| Ok(())).unwrap();
    let previous = manager.state.clone();
    let mut next = previous.clone();
    next.generation += 1;
    atomic_json(
        &manager.root.join("recovery.json"),
        &Journal {
            previous: previous.clone(),
            next: next.clone(),
        },
    )
    .unwrap();
    drop(manager);
    let mut manager = f.manager();
    assert_eq!(manager.state.generation, previous.generation);
    manager.persist(next.clone()).unwrap();
    let plugin = manager.active("sodeck.terminal").unwrap().unwrap();
    fs::write(plugin.data_dir.join("post-commit"), "new-data").unwrap();
    atomic_json(
        &manager.root.join("recovery.json"),
        &Journal {
            previous,
            next: next.clone(),
        },
    )
    .unwrap();
    drop(manager);
    let manager = f.manager();
    assert_eq!(manager.state.generation, next.generation);
    assert_eq!(
        fs::read_to_string(plugin.data_dir.join("post-commit")).unwrap(),
        "new-data"
    );
}

#[test]
fn windows_alias_paths_are_rejected() {
    for path in [
        "../escape",
        "C:/test",
        "x\\y",
        "ui/CON.html",
        "ui/a.",
        "ui/a ",
        "/root",
        "a//b",
    ] {
        assert!(!package::valid_relative(path), "{path}");
    }
    assert!(package::valid_relative("ui/assets/terminal.js"));
}

#[test]
fn signed_incompatible_candidates_preserve_active_and_existing_pending() {
    let f = Fixture::new();
    let mut manager = f.manager();
    manager.prepare_package(f.package("0.1.0", "old")).unwrap();
    manager.activate_pending(|_, _| Ok(())).unwrap();
    manager
        .prepare_package(f.package("0.1.1", "pending"))
        .unwrap();
    for (path, value) in [
        ("/requires/core", json!(">=9.0.0")),
        ("/requires/pluginApi", json!("^8.0.0")),
        ("/requires/pluginApi", json!("^1.0.0")),
        ("/requires/pluginApi", json!("^2.0.0")),
        ("/requires/pluginApi", json!("^3.0.0")),
        ("/requires/capabilities", json!(["workspace.v1"])),
        ("/requires/capabilities", json!(["unsupported.capability"])),
        (
            "/requires/platforms",
            json!(["fixture-unsupported-platform"]),
        ),
        ("/entry/worker", json!("worker/absent.js")),
        (
            "/contributes/views/0/defaultLocation/host",
            json!("unsupported-host"),
        ),
    ] {
        let mut files = f.files("0.2.0", "candidate");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&files["manifest.json"]).unwrap();
        if path == "/requires/platforms" {
            manifest["requires"]["platforms"] = value;
        } else {
            *manifest.pointer_mut(path).unwrap() = value;
        }
        files.insert(
            "manifest.json".into(),
            serde_json::to_vec(&manifest).unwrap(),
        );
        assert!(
            manager
                .prepare_package(f.write_package("incompatible", files))
                .is_err(),
            "{path}"
        );
        assert_eq!(
            manager
                .active("sodeck.terminal")
                .unwrap()
                .unwrap()
                .manifest
                .version,
            "0.1.0"
        );
        assert_eq!(
            manager.status()[0].pending_version.as_deref(),
            Some("0.1.1")
        );
    }
}

#[test]
fn schema_change_requires_migration_and_missing_candidate_falls_back() {
    let f = Fixture::new();
    let mut manager = f.manager();
    manager.prepare_package(f.package("0.1.0", "old")).unwrap();
    manager.activate_pending(|_, _| Ok(())).unwrap();
    let old = manager.active("sodeck.terminal").unwrap().unwrap();
    fs::write(old.data_dir.join("state"), "preserved").unwrap();
    let mut files = f.files("0.1.1", "new-schema");
    let mut manifest: serde_json::Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest["dataSchemaVersion"] = json!(2);
    files.insert(
        "manifest.json".into(),
        serde_json::to_vec(&manifest).unwrap(),
    );
    manager
        .prepare_package(f.write_package("schema", files))
        .unwrap();
    manager
        .activate_pending(|_, _| panic!("incompatible data must not run worker"))
        .unwrap();
    assert!(manager.status()[0]
        .error
        .as_ref()
        .unwrap()
        .contains("migration"));
    assert_eq!(
        fs::read_to_string(old.data_dir.join("state")).unwrap(),
        "preserved"
    );
    manager
        .prepare_package(f.package("0.1.2", "missing"))
        .unwrap();
    fs::remove_dir_all(manager.package_path("sodeck.terminal", "0.1.2")).unwrap();
    manager
        .activate_pending(|_, _| panic!("missing package must not run worker"))
        .unwrap();
    assert_eq!(
        manager
            .active("sodeck.terminal")
            .unwrap()
            .unwrap()
            .manifest
            .version,
        "0.1.0"
    );
    assert!(manager.status()[0].pending_version.is_none());
    assert!(manager.status()[0].error.is_some());
    fs::remove_file(old.ui_path).unwrap();
    assert!(manager.active("sodeck.terminal").is_err());
}

#[test]
fn extraction_rejects_archive_count_expansion_and_case_limits() {
    let f = Fixture::new();
    let mut manager = f.manager();
    let oversized = f.dir.path().join("oversize.zip");
    File::create(&oversized)
        .unwrap()
        .set_len(32 * 1024 * 1024 + 1)
        .unwrap();
    assert!(manager
        .prepare_package(oversized)
        .unwrap_err()
        .to_string()
        .contains("32 MiB"));
    let many = (0..2001)
        .map(|n| (format!("files/{n}.txt"), Vec::new()))
        .collect();
    assert!(manager
        .prepare_package(f.write_package("many", many))
        .unwrap_err()
        .to_string()
        .contains("2000"));
    let mut files = f.files("0.1.0", "case");
    files.insert("ui/INDEX.html".into(), b"collision".to_vec());
    assert!(manager
        .prepare_package(f.write_package("case", files))
        .unwrap_err()
        .to_string()
        .contains("case-colliding"));
    let expanded = f.dir.path().join("expanded.zip");
    let mut zip = zip::ZipWriter::new(File::create(&expanded).unwrap());
    zip.start_file(
        "large.txt",
        zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated),
    )
    .unwrap();
    let chunk = vec![0u8; 1024 * 1024];
    for _ in 0..129 {
        zip.write_all(&chunk).unwrap();
    }
    zip.finish().unwrap();
    assert!(manager
        .prepare_package(expanded)
        .unwrap_err()
        .to_string()
        .contains("128 MiB"));
    assert!(manager.status().is_empty());
}


#[test]
fn workspace_initializers_validate_explicit_generic_server_commands() {
    let f = Fixture::new();
    for (scope, input, initializers, valid) in [
        ("server", false, json!([{"id":"sample.initial","commandId":"sample.seed"}]), true),
        ("view", false, json!([{"id":"sample.initial","commandId":"sample.seed"}]), false),
        ("server", true, json!([{"id":"sample.initial","commandId":"sample.seed"}]), false),
        ("server", false, json!([{"id":"sample.initial","commandId":"absent"}]), false),
        ("server", false, json!([{"id":"a","commandId":"sample.seed"},{"id":"b","commandId":"sample.seed"}]), false),
    ] {
        let mut files=f.files("0.4.0","fixture");
        let mut m:serde_json::Value=serde_json::from_slice(&files["manifest.json"]).unwrap();
        m["contributes"]["commands"]=json!([{"id":"sample.seed","title":"Sample","scope":scope,"primary":false}]);
        if input {m["contributes"]["commands"][0]["input"]=json!({"kind":"text","key":"title","label":"Title","initial":"viewTitle","minLength":1,"maxLength":80});}
        m["contributes"]["workspaceInitializers"]=initializers;
        files.insert("manifest.json".into(),serde_json::to_vec(&m).unwrap());
        assert_eq!(f.manager().prepare_package(f.write_package("generic",files)).is_ok(),valid);
    }
}

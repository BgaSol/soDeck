use super::*;
use base64::{engine::general_purpose::STANDARD,Engine};
use ring::signature::{Ed25519KeyPair,KeyPair};
use std::io::Write;
#[path = "../../../../../crates/updater/tests/common/mod.rs"]
mod fixtures;

fn package(root:&Path,version:&str,api:&str)->PathBuf {
    let key=Ed25519KeyPair::from_seed_unchecked(&[7;32]).unwrap();
    let manifest=json!({"pluginId":"sodeck.terminal","version":version,"requires":{"core":">=2.0.0, <3.0.0","pluginApi":api,"capabilities":["remote.pty.v1"],"platforms":["windows"]},"entry":{"ui":"ui/index.html","worker":"worker/main.js"},"dataSchemaVersion":1,"contributes":{"views":[]}});
    let mut files=BTreeMap::from([("manifest.json",serde_json::to_vec(&manifest).unwrap()),("ui/index.html",b"<html></html>".to_vec()),("worker/main.js",b"globalThis.sodeckPlugin={invoke(){return true}}".to_vec())]);
    let hashes=files.iter().map(|(k,v)|(*k,domain::hash(v))).collect::<BTreeMap<_,_>>();let integrity=serde_json::to_vec(&hashes).unwrap();
    files.insert("signature.json",serde_json::to_vec(&json!({"keyId":"test","signature":STANDARD.encode(key.sign(&integrity).as_ref())})).unwrap());files.insert("integrity.json",integrity);
    let path=root.join(format!("{version}.sodeck-plugin"));let mut archive=zip::ZipWriter::new(fs::File::create(&path).unwrap());
    for (name,bytes) in files{archive.start_file(name,zip::write::SimpleFileOptions::default()).unwrap();archive.write_all(&bytes).unwrap();}archive.finish().unwrap();path
}
fn setup(future:bool)->(tempfile::TempDir,Updates,PluginManager,Target,String,PathBuf) {
    let root=tempfile::tempdir().unwrap();let key=Ed25519KeyPair::from_seed_unchecked(&[7;32]).unwrap();
    let keys=TrustedKeys::from([("test".into(),key.public_key().as_ref().to_vec().into())]);
    let mut manager=PluginManager::open(root.path().join("plugins"),keys.clone()).unwrap();
    let old=package(root.path(),"1.0.0",">=6.0.0, <7.0.0");manager.prepare_package(old).unwrap();manager.activate_pending(|_,_|Ok(())).unwrap();
    let path=package(root.path(),"1.1.0",if future{">=6.3.0, <7.0.0"}else{">=6.2.0, <7.0.0"});let staged=manager.stager().stage(&path).unwrap();
    let (mut catalog,snapshot)=fixtures::fixture();let release=&mut catalog.plugins.get_mut("sodeck.terminal").unwrap().current;
    release.requires=serde_json::from_value(serde_json::to_value(&staged.snapshot().manifest().requires).unwrap()).unwrap();release.package_digest=staged.snapshot().digest().into();
    let bytes=fs::read(&path).unwrap();release.asset.size=bytes.len()as u64;release.asset.archive_sha256=domain::hash(&bytes);let release=release.clone();drop(staged);
    let verified=fixtures::verified(&catalog);let digest=verified.digest().to_owned();let mut store=Store::open(root.path().join("updates")).unwrap();store.accept_catalog(&verified).unwrap();
    fs::write(store.root().join("cache").join(&release.asset.archive_sha256),bytes).unwrap();
    let mut build=identity::BuildIdentity::compiled();build.catalog_keys=BTreeMap::from([("test".into(),STANDARD.encode(key.public_key().as_ref()))]);build.channel="stable".into();
    let updates=Updates{storage:Mutex::new(Ok(Registry{store,catalog:Some(verified),cancellations:BTreeMap::new(),reservations:BTreeMap::new(),asset_locks:BTreeMap::new(),reviews:BTreeMap::new()})),runtime:snapshot.runtime,build,downloads:tokio::sync::Semaphore::new(2),check_lock:tokio::sync::Mutex::new(()),cancel:CancellationToken::new(),install_lock:Mutex::new(()),work:Arc::new(work::Work::default())};
    let target=Target::Plugin{plugin_id:"sodeck.terminal".into(),release,requires_core:future.then(||catalog.core.current.app_version)};
    (root,updates,manager,target,digest,path)
}
#[test]
fn local_import_of_identical_official_bytes_stops_following_and_bundled_skip_preserves_it(){
    let (_root,u,mut manager,target,digest,path)=setup(false);u.verify_target(&target,&digest).unwrap();
    let staged=manager.stager().stage(&path).unwrap();u.accept_package(&mut manager,staged,Origin::Official,"official",false,None).unwrap();
    let selected=manager.selection_snapshot();let hash=&selected.plugins[0].pending.as_ref().unwrap().digest;
    assert_eq!(u.with(|r|Ok(r.store.source_for("sodeck.terminal",hash))).unwrap(),(Origin::Official,true));
    let staged=manager.stager().stage(&path).unwrap();u.accept_package(&mut manager,staged,Origin::Local,"manual",false,None).unwrap();
    let staged=manager.stager().stage(&path).unwrap();u.accept_package(&mut manager,staged,Origin::Bundled,"bundled",true,None).unwrap();
    assert_eq!(u.with(|r|Ok(r.store.source_for("sodeck.terminal",hash))).unwrap(),(Origin::Local,false));
}
#[test]
fn source_write_failure_prevents_candidate_publication(){
    let (root,u,mut manager,_,_,path)=setup(false);fs::create_dir(root.path().join("updates/sources.json")).unwrap();
    let staged=manager.stager().stage(&path).unwrap();assert!(u.accept_package(&mut manager,staged,Origin::Official,"update",false,None).is_err());
    assert!(manager.selection_snapshot().plugins[0].pending.is_none());
}
#[test]
fn interrupted_activation_is_not_retried_and_keeps_active_data(){
    let (_root,u,mut manager,target,digest,path)=setup(false);
    let data=manager.active("sodeck.terminal").unwrap().unwrap().data_dir;fs::write(data.join("settings"),b"preserve").unwrap();
    u.install_plugin(&mut manager,&target,&digest,"install",&path).unwrap();u.before_activation(&mut manager,"boot-a").unwrap();
    u.before_activation(&mut manager,"boot-b").unwrap();assert!(manager.selection_snapshot().plugins[0].pending.is_none());
    assert_eq!(u.with(|r|Ok(r.store.state.installs["sodeck.terminal"].stage.clone())).unwrap(),Stage::Failed);
    assert_eq!(fs::read(data.join("settings")).unwrap(),b"preserve");
}
#[test]
fn future_intent_waits_for_exact_core_handoff_and_only_then_activates(){
    let (_root,mut u,mut manager,target,digest,path)=setup(true);u.verify_target(&target,&digest).unwrap();
    assert_eq!(u.install_plugin(&mut manager,&target,&digest,"install",&path).unwrap(),Stage::WaitingForCore);
    u.before_activation(&mut manager,"old-core").unwrap();assert!(manager.selection_snapshot().plugins[0].pending.is_none());
    let mut h=u.with(|r|Ok(r.store.state.installs["sodeck.terminal"].required_core.clone().unwrap())).unwrap();
    u.runtime.app_version=h.target_version.clone();u.runtime.build_id=h.build_id.clone();u.runtime.compatibility.plugin_api_version="6.3.0".into();h.stage=Stage::Installed;
    u.with(|r|{let mut next=r.store.state.clone();next.handoff=Some(h);r.store.save_state(next)}).unwrap();
    u.before_activation(&mut manager,"new-core").unwrap();assert!(manager.selection_snapshot().plugins[0].pending.is_some());
    manager.activate_pending(|_,_|Ok(())).unwrap();u.after_activation(&manager).unwrap();
    assert_eq!(manager.active("sodeck.terminal").unwrap().unwrap().manifest.version,"1.1.0");
    assert_eq!(u.with(|r|Ok(r.store.state.installs["sodeck.terminal"].stage.clone())).unwrap(),Stage::Installed);
}
#[test]
fn tampered_target_metadata_never_uses_a_valid_cached_archive(){
    let (_root,u,_manager,mut target,digest,_)=setup(false);
    if let Target::Plugin{release,..}=&mut target{release.data_schema_version=9;}
    assert!(u.verify_target(&target,&digest).is_err());
}
#[test]
fn damaged_journal_preserves_candidate_until_explicit_preparation(){
    let (root,u,mut manager,target,digest,path)=setup(false);
    u.install_plugin(&mut manager,&target,&digest,"install",&path).unwrap();
    let pending=manager.selection_snapshot().plugins[0].pending.as_ref().unwrap().digest.clone();
    fs::write(root.path().join("updates/state.json"),b"{broken").unwrap();
    u.with(|r|{r.store=Store::open(root.path().join("updates"))?;r.catalog=None;Ok(())}).unwrap();
    for boot in ["first-recovery","second-recovery"] {
        assert!(u.before_activation(&mut manager,boot).is_err());
        assert_eq!(manager.selection_snapshot().plugins[0].pending.as_ref().unwrap().digest,pending);
        assert_eq!(manager.active("sodeck.terminal").unwrap().unwrap().manifest.version,"1.0.0");
    }
    assert!(fs::read_dir(root.path().join("updates/diagnostics")).unwrap().any(|e|fs::read(e.unwrap().path()).unwrap()==b"{broken"));
    let staged=manager.stager().stage(&path).unwrap();
    u.accept_package(&mut manager,staged,Origin::Local,"explicit-reprepare",false,None).unwrap();
    u.before_activation(&mut manager,"confirmed").unwrap();
    manager.activate_pending(|_,_|Ok(())).unwrap();
    assert_eq!(manager.active("sodeck.terminal").unwrap().unwrap().manifest.version,"1.1.0");
}
#[test]
fn disabled_candidate_survives_restarts_without_activation_attempt(){
    let (root,u,mut manager,target,digest,path)=setup(false);
    u.install_plugin(&mut manager,&target,&digest,"install",&path).unwrap();
    let preference=manager.enablement_snapshot();
    manager.commit_enablement(preference["generation"].as_str().unwrap(),preference["revision"].as_str().unwrap(),&[sodeck_plugin_manager::enablement::Change{plugin_id:"sodeck.terminal".into(),enabled:false}],&Uuid::new_v4().to_string()).unwrap();
    for boot in ["disabled-one","disabled-two"] {
        drop(manager);manager=PluginManager::open(root.path().join("plugins"),u.package_keys()).unwrap();
        u.before_activation(&mut manager,boot).unwrap();
        manager.activate_pending(|_,_|panic!("disabled candidate must not initialize")).unwrap();
        u.after_activation(&manager).unwrap();
        assert!(manager.selection_snapshot().plugins[0].pending.is_some());
        assert!(u.with(|r|Ok(r.store.state.installs["sodeck.terminal"].activation_attempt.is_none())).unwrap());
    }
    let preference=manager.enablement_snapshot();
    manager.commit_enablement(preference["generation"].as_str().unwrap(),preference["revision"].as_str().unwrap(),&[sodeck_plugin_manager::enablement::Change{plugin_id:"sodeck.terminal".into(),enabled:true}],&Uuid::new_v4().to_string()).unwrap();
    drop(manager);let mut manager=PluginManager::open(root.path().join("plugins"),u.package_keys()).unwrap();
    u.before_activation(&mut manager,"enabled").unwrap();manager.activate_pending(|_,_|Ok(())).unwrap();u.after_activation(&manager).unwrap();
    assert_eq!(u.with(|r|Ok(r.store.state.installs["sodeck.terminal"].stage.clone())).unwrap(),Stage::Installed);
}

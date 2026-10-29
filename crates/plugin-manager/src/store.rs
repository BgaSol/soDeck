use crate::{
    package::{self, invalid},
    Manifest, Result, TrustedKeys, VerifiedPackageSnapshot, enablement::{EnablementStore, BootEnablement, Change},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap,BTreeSet},
    sync::{Arc,Mutex},
    fs::{self, File},
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledPlugin {
    #[serde(skip)]
    pub snapshot: Arc<VerifiedPackageSnapshot>,
    pub manifest: Manifest,
    pub root: PathBuf,
    pub ui_path: PathBuf,
    pub worker_path: PathBuf,
    pub data_dir: PathBuf,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginStatus {
    pub plugin_id: String,
    pub active_version: Option<String>,
    pub pending_version: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct State {
    generation: u64,
    active: BTreeMap<String, Selection>,
    pending: BTreeMap<String, String>,
    failures: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Selection {
    version: String,
    data_generation: String,
    data_schema_version: u32,
}

#[derive(Deserialize, Serialize)]
struct Journal {
    previous: State,
    next: State,
}

#[derive(Clone,Debug)]
pub struct PackageMetadata { pub manifest:Manifest, pub signature:crate::SignatureStatus, pub digest:String, pub settings:std::result::Result<Option<crate::settings_descriptor::Descriptor>,String> }
impl From<&VerifiedPackageSnapshot> for PackageMetadata {fn from(s:&VerifiedPackageSnapshot)->Self{Self{manifest:s.manifest().clone(),signature:s.signature().clone(),digest:s.digest().into(),settings:s.manifest().contributes.settings.as_ref().map(|r|crate::settings_descriptor::Descriptor::parse(s.file(&r.path).unwrap(),&s.manifest().plugin_id)).transpose()}}}
#[derive(Clone)]
pub struct PackageStager {root:PathBuf,keys:TrustedKeys}
pub struct StagedPackage {path:PathBuf,snapshot:VerifiedPackageSnapshot}
impl StagedPackage {
    pub fn snapshot(&self) -> &VerifiedPackageSnapshot { &self.snapshot }
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionMetadata {
    pub plugin_id: String,
    pub selected: Option<SelectedPackageMetadata>,
    pub pending: Option<SelectedPackageMetadata>,
    pub enabled: bool,
    pub error: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectedPackageMetadata {
    pub manifest: Manifest,
    pub digest: String,
    pub signature: crate::SignatureStatus,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionSnapshot {
    pub revision: String,
    pub enablement_revision: String,
    pub plugins: Vec<SelectionMetadata>,
}
impl Drop for StagedPackage {fn drop(&mut self){if self.path.exists(){let _=fs::remove_dir_all(&self.path);}}}
impl PackageStager {
    pub fn stage(&self,archive:&Path)->Result<StagedPackage>{
        let path=self.root.join("staging").join(uuid::Uuid::new_v4().to_string());
        let result=(||{package::extract(archive,&path)?;let snapshot=package::read_snapshot(&path,&self.keys)?;Ok(StagedPackage{path:path.clone(),snapshot})})();
        if result.is_err()&&path.exists(){let _=fs::remove_dir_all(&path);}result
    }
}
pub struct PluginManager {
    root: PathBuf,
    keys: TrustedKeys,
    state: State,
    enablement: EnablementStore,
    boot: BootEnablement,
    metadata_cache: Mutex<BTreeMap<(String,String), std::result::Result<PackageMetadata,String>>>,
    // Kept open for the lifetime of the manager, including package staging.
    _lock: File,
}

impl PluginManager {
    pub fn open(root: impl AsRef<Path>, trusted_keys: TrustedKeys) -> Result<Self> {
        fs::create_dir_all(root.as_ref())?;
        let root = fs::canonicalize(root)?;
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join("writer.lock"))?;
        lock.try_lock()
            .map_err(|_| invalid("Another App already owns the plugin data directory"))?;
        let state_path = root.join("activation.json");
        let mut state: State = if state_path.exists() {
            serde_json::from_slice(&fs::read(state_path)?)?
        } else {
            State::default()
        };
        if root.join("recovery.json").exists() {
            let journal: Journal = serde_json::from_slice(&fs::read(root.join("recovery.json"))?)?;
            // An atomic activation record is the commit point. Never overwrite data
            // after commit: normal plugin execution may already have modified it.
            if state.generation != journal.next.generation {
                state = journal.previous;
                atomic_json(&root.join("activation.json"), &state)?;
            }
            fs::remove_file(root.join("recovery.json"))?;
        }
        let enablement=EnablementStore::open(&root);let boot=enablement.boot();
        let manager=Self {
            enablement,boot,metadata_cache:Mutex::new(BTreeMap::new()),
            root,
            keys: trusted_keys,
            state,
            _lock: lock,
        };
        manager.refresh_metadata();
        Ok(manager)
    }

    pub fn prepare_package(&mut self, archive: impl AsRef<Path>) -> Result<Manifest> {
        self.prepare(archive.as_ref(),false,None)
    }

    /// Bundled packages seed an installation; they never replace a pending
    /// independent update or downgrade an already installed plugin on restart.
    pub fn prepare_bundled(&mut self, archive: impl AsRef<Path>) -> Result<Manifest> {
        self.prepare(archive.as_ref(),true,None)
    }

    pub fn prepare_bundled_for(&mut self,archive:impl AsRef<Path>,expected_id:Option<&str>)->Result<Manifest>{self.prepare(archive.as_ref(),true,expected_id)}

    pub fn stager(&self)->PackageStager {PackageStager{root:self.root.clone(),keys:self.keys.clone()}}
    fn prepare(&mut self, archive:&Path,only_missing:bool,expected_id:Option<&str>)->Result<Manifest>{
        let staged=self.stager().stage(archive)?;self.accept_staged(staged,only_missing,expected_id)
    }
    pub fn accept_staged(&mut self, staged:StagedPackage, only_missing:bool, expected_id:Option<&str>)->Result<Manifest>{
        let manifest=staged.snapshot.manifest().clone();
        if expected_id.is_some_and(|id|id!=manifest.plugin_id){return Err(invalid("随附插件身份与资源索引不符"));}
        if only_missing&&(self.state.active.contains_key(&manifest.plugin_id)||self.state.pending.contains_key(&manifest.plugin_id)){return Ok(manifest);}
        if self.status().len()>=1024&&!self.known_ids().contains(&manifest.plugin_id){return Err(crate::Error::Fault("CAPACITY","插件数量达到上限".into()));}
        let destination=self.package_path(&manifest.plugin_id,&manifest.version);
        if destination.exists(){
            let existing=package::read_snapshot(&destination,&self.keys)?;
            if existing.digest()!=staged.snapshot.digest(){return Err(invalid("A plugin version is immutable; publish a new version"));}
        } else {fs::create_dir_all(destination.parent().ok_or_else(||invalid("Invalid destination"))?)?;fs::rename(&staged.path,&destination)?;}
        self.metadata_cache.lock().unwrap().insert((manifest.plugin_id.clone(),manifest.version.clone()),Ok(PackageMetadata::from(&staged.snapshot)));
        let mut next=self.state.clone();next.pending.insert(manifest.plugin_id.clone(),manifest.version.clone());next.failures.remove(&manifest.plugin_id);self.persist(next)?;Ok(manifest)
    }

    /// Call only during startup, before publishing plugins to the workbench.
    /// The initializer sees a data copy; failed initialization never mutates active data.
    pub fn activate_pending<F>(&mut self, mut initialize: F) -> Result<Vec<PluginStatus>>
    where
        F: FnMut(&InstalledPlugin, &Path) -> std::result::Result<(), String>,
    {
        if !self.state.pending.keys().any(|id|self.boot.enabled(id)) {
            return Ok(self.status());
        }
        let previous = self.state.clone();
        let mut next = previous.clone();
        next.generation += 1;
        let pending = previous.pending.clone();
        // Write before copying data or calling any plugin code, so interrupted
        // startup always has an unambiguous old selection to return to.
        atomic_json(
            &self.root.join("recovery.json"),
            &Journal {
                previous: previous.clone(),
                next: next.clone(),
            },
        )?;
        for (id, version) in pending {
            if !self.boot.enabled(&id) {continue;}
            let candidate = (|| -> Result<InstalledPlugin> {
                let root = self.package_path(&id, &version);
                let snapshot = self.verify_snapshot(&id,&version)?;
                let manifest = snapshot.manifest().clone();
                let generation = uuid::Uuid::new_v4().to_string();
                let data_dir = self.root.join("data").join(&id).join(&generation);
                if let Some(old) = previous.active.get(&id) {
                    // First versions intentionally share schema 1. A different
                    // schema needs an explicit migration contract, not blind reuse.
                    if old.data_schema_version != manifest.data_schema_version {
                        return Err(invalid("Plugin data migration is required"));
                    }
                    let source = self.root.join("data").join(&id).join(&old.data_generation);
                    if source.exists() {
                        copy_data(&source, &data_dir)?;
                    }
                }
                fs::create_dir_all(&data_dir)?;
                let plugin = installed(snapshot, root, data_dir.clone());
                if let Err(message) = initialize(&plugin, &data_dir) {
                    return Err(invalid(format!("Plugin initialization failed: {message}")));
                }
                next.active.insert(
                    id.clone(),
                    Selection {
                        version: version.clone(),
                        data_generation: generation,
                        data_schema_version: plugin.manifest.data_schema_version,
                    },
                );
                Ok(plugin)
            })();
            next.pending.remove(&id);
            match candidate {
                Ok(_) => {
                    next.failures.remove(&id);
                }
                Err(error) => {
                    next.failures.insert(id.clone(), error.to_string());
                }
            }
        }
        atomic_json(
            &self.root.join("recovery.json"),
            &Journal {
                previous,
                next: next.clone(),
            },
        )?;
        self.persist(next)?;
        fs::remove_file(self.root.join("recovery.json"))?;
        Ok(self.status())
    }

    fn verify_snapshot(&self,id:&str,version:&str)->Result<VerifiedPackageSnapshot>{
        let result=package::read_snapshot(&self.package_path(id,version),&self.keys);
        self.metadata_cache.lock().unwrap().insert((id.into(),version.into()),result.as_ref().map(PackageMetadata::from).map_err(|e|e.to_string()));result
    }
    pub fn active(&self, plugin_id:&str)->Result<Option<InstalledPlugin>> {
        let Some(selection)=self.state.active.get(plugin_id)else{return Ok(None)};
        let snapshot=self.verify_snapshot(plugin_id,&selection.version)?;
        Ok(Some(installed(snapshot,self.package_path(plugin_id,&selection.version),self.root.join("data").join(plugin_id).join(&selection.data_generation))))
    }
    pub fn load_for_run(&self,id:&str)->Result<Option<InstalledPlugin>> {
        if !self.boot.enabled(id){return Err(crate::Error::Fault("PLUGIN_DISABLED","插件在本次运行中已停用，启用后需重启".into()));}self.active(id)
    }
    pub fn boot_enabled(&self,id:&str)->bool {self.boot.enabled(id)}
    pub fn enabled(&self,id:&str)->bool {self.enablement.enabled(id)}
    pub fn enablement_snapshot(&self)->serde_json::Value {self.enablement.snapshot()}
    pub fn known_ids(&self)->BTreeSet<String>{self.status().into_iter().map(|s|s.plugin_id).collect()}
    pub fn validate_enablement(&self,g:&str,r:&str,c:&[Change])->Result<()>{self.enablement.validate(g,r,c,&self.known_ids())}
    pub fn commit_enablement(&mut self,g:&str,r:&str,c:&[Change],op:&str)->Result<serde_json::Value>{let known=self.known_ids();self.enablement.commit(g,r,c,&known,op,&self.boot)}
    pub fn enablement_operation(&self,g:&str,op:&str)->serde_json::Value{self.enablement.operation(g,op)}
    pub fn repair_enablement(&mut self,token:&str,op:&str)->Result<serde_json::Value>{let known=self.known_ids();self.enablement.repair(token,op,&known)}
    pub fn refresh_metadata(&self){for status in self.status(){for version in [status.active_version,status.pending_version].into_iter().flatten(){let _=self.verify_snapshot(&status.plugin_id,&version);}}}
    pub fn package_manifest(&self,id:&str,version:&str)->Option<Manifest>{self.metadata_cache.lock().unwrap().get(&(id.into(),version.into()))?.as_ref().ok().map(|m|m.manifest.clone())}
    pub fn selected_metadata(&self,id:&str)->Option<PackageMetadata>{let version=&self.state.active.get(id)?.version;self.metadata_cache.lock().unwrap().get(&(id.into(),version.clone()))?.as_ref().ok().cloned()}
    pub fn selection_snapshot(&self) -> SelectionSnapshot {
        use sha2::{Digest, Sha256};
        let cache = self.metadata_cache.lock().unwrap();
        let plugins: Vec<_> = self.status().into_iter().map(|s| {
            let metadata = |version: Option<&str>| version.and_then(|v| cache.get(&(s.plugin_id.clone(), v.into())))
                .and_then(|v| v.as_ref().ok()).map(|m| SelectedPackageMetadata {
                    manifest: m.manifest.clone(), digest: m.digest.clone(), signature: m.signature.clone(),
                });
            SelectionMetadata { plugin_id: s.plugin_id.clone(), selected: metadata(s.active_version.as_deref()),
                pending: metadata(s.pending_version.as_deref()), enabled: self.enabled(&s.plugin_id), error: s.error }
        }).collect();
        // Preparing a candidate does not increment activation.generation. Bind the
        // full saved selection and verified metadata so every prepare invalidates plans.
        let revision = format!("{:x}", Sha256::digest(serde_json::to_vec(&(&self.state, &plugins)).unwrap()));
        let enablement_revision = format!("{:x}", Sha256::digest(serde_json::to_vec(&self.enablement.snapshot()).unwrap()));
        SelectionSnapshot { revision, enablement_revision, plugins }
    }

    /// Recovery may remove only the precise candidate it previously published.
    /// A later manual import must survive recovery of an interrupted online update.
    pub fn cancel_pending(&mut self, id: &str, version: &str, digest: &str) -> Result<bool> {
        if self.state.pending.get(id).map(String::as_str) != Some(version) { return Ok(false); }
        let snapshot = self.verify_snapshot(id, version)?;
        if snapshot.digest() != digest { return Ok(false); }
        let mut next = self.state.clone();
        next.pending.remove(id);
        self.persist(next)?;
        Ok(true)
    }
    pub fn management_rows(&self)->Vec<serde_json::Value>{
        let cache=self.metadata_cache.lock().unwrap();
        self.status().into_iter().map(|s|{
            let info=|version:Option<&str>|version.map(|v|match cache.get(&(s.plugin_id.clone(),v.into())){
                Some(Ok(m))=>serde_json::json!({"version":v,"displayName":m.manifest.display_name(),"signature":m.signature,"digest":m.digest,"valid":true,"error":null}),
                other=>serde_json::json!({"version":v,"displayName":s.plugin_id,"signature":{"status":"invalid"},"digest":null,"valid":false,"error":other.and_then(|r|r.as_ref().err()).cloned().unwrap_or_else(||"包尚未验证".into())})
            });
            let selected=info(s.active_version.as_deref());let pending=info(s.pending_version.as_deref());
            let name=selected.as_ref().or(pending.as_ref()).and_then(|v|v["displayName"].as_str()).unwrap_or(&s.plugin_id).to_owned();
            serde_json::json!({"pluginId":s.plugin_id,"displayName":name,"enabled":self.enabled(&s.plugin_id),"bootEnabled":self.boot_enabled(&s.plugin_id),"selected":selected,"pending":pending,"error":s.error,"restartRequired":self.enabled(&s.plugin_id)!=self.boot_enabled(&s.plugin_id)||self.enabled(&s.plugin_id)&&s.pending_version.is_some()})
        }).collect()
    }

    pub fn status(&self) -> Vec<PluginStatus> {
        let mut ids: Vec<_> = self
            .state
            .active
            .keys()
            .chain(self.state.pending.keys())
            .chain(self.state.failures.keys())
            .cloned()
            .collect();
        ids.sort();
        ids.dedup();
        ids.into_iter()
            .map(|id| PluginStatus {
                active_version: self.state.active.get(&id).map(|s| s.version.clone()),
                pending_version: self.state.pending.get(&id).cloned(),
                error: self.state.failures.get(&id).cloned(),
                plugin_id: id,
            })
            .collect()
    }

    fn package_path(&self, id: &str, version: &str) -> PathBuf {
        self.root.join("packages").join(id).join(version)
    }
    /// Names are taken only from verified immutable packages; pending metadata
    /// cannot replace the active package's identity in the running workbench.
    pub fn metadata(&self) -> Vec<serde_json::Value> {
        self.status().into_iter().map(|status| {
            let cache=self.metadata_cache.lock().unwrap();
            let name=|version:Option<&str>|version.and_then(|v|cache.get(&(status.plugin_id.clone(),v.into()))).and_then(|m|m.as_ref().ok()).map(|m|m.manifest.display_name());
            serde_json::json!({
                "displayName": name(status.active_version.as_deref()).unwrap_or_else(|| status.plugin_id.clone()),
                "pendingDisplayName": name(status.pending_version.as_deref()),
                "pluginId": status.plugin_id, "activeVersion": status.active_version,
                "pendingVersion": status.pending_version, "error": status.error
            })
        }).collect()
    }
    fn persist(&mut self, next: State) -> Result<()> {
        atomic_json(&self.root.join("activation.json"), &next)?;
        self.state = next;
        Ok(())
    }
}

fn installed(snapshot: VerifiedPackageSnapshot, root: PathBuf, data_dir: PathBuf) -> InstalledPlugin {
    let manifest=snapshot.manifest().clone();
    InstalledPlugin {
        snapshot: Arc::new(snapshot),
        ui_path: root.join(&manifest.entry.ui),
        worker_path: root.join(&manifest.entry.worker),
        manifest,
        root,
        data_dir,
    }
}

fn atomic_json(path:&Path,value:&impl Serialize)->Result<()>{crate::atomic_file::write(path,&serde_json::to_vec(value)?)}

fn copy_data(source: &Path, target: &Path) -> Result<()> {
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_symlink() {
            return Err(invalid("Plugin data contains a symbolic link"));
        }
        let destination = target.join(entry.file_name());
        if ty.is_dir() {
            copy_data(&entry.path(), &destination)?;
        } else if ty.is_file() {
            fs::copy(entry.path(), destination)?;
        } else {
            return Err(invalid("Plugin data contains a special file"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;

use crate::{
    package::{self, invalid},
    Manifest, Result, TrustedKeys,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledPlugin {
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

pub struct PluginManager {
    root: PathBuf,
    keys: TrustedKeys,
    state: State,
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
        Ok(Self {
            root,
            keys: trusted_keys,
            state,
            _lock: lock,
        })
    }

    pub fn prepare_package(&mut self, archive: impl AsRef<Path>) -> Result<Manifest> {
        let staging = self
            .root
            .join("staging")
            .join(uuid::Uuid::new_v4().to_string());
        let result = (|| {
            package::extract(archive.as_ref(), &staging)?;
            let manifest = package::verify(&staging, &self.keys)?;
            let destination = self.package_path(&manifest.plugin_id, &manifest.version);
            if destination.exists() {
                let existing = package::verify(&destination, &self.keys)?;
                if existing.version != manifest.version
                    || fs::read(destination.join("integrity.json"))?
                        != fs::read(staging.join("integrity.json"))?
                {
                    return Err(invalid(
                        "A plugin version is immutable; publish a new version",
                    ));
                }
            } else {
                fs::create_dir_all(
                    destination
                        .parent()
                        .ok_or_else(|| invalid("Invalid package destination"))?,
                )?;
                fs::rename(&staging, &destination)?;
            }
            let mut next = self.state.clone();
            next.pending
                .insert(manifest.plugin_id.clone(), manifest.version.clone());
            next.failures.remove(&manifest.plugin_id);
            self.persist(next)?;
            Ok(manifest)
        })();
        if staging.exists() {
            let _ = fs::remove_dir_all(&staging);
        }
        result
    }

    /// Call only during startup, before publishing plugins to the workbench.
    /// The initializer sees a data copy; failed initialization never mutates active data.
    pub fn activate_pending<F>(&mut self, mut initialize: F) -> Result<Vec<PluginStatus>>
    where
        F: FnMut(&InstalledPlugin, &Path) -> std::result::Result<(), String>,
    {
        if self.state.pending.is_empty() {
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
            let candidate = (|| -> Result<InstalledPlugin> {
                let root = self.package_path(&id, &version);
                let manifest = package::verify(&root, &self.keys)?;
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
                let plugin = installed(manifest, root, data_dir.clone());
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

    pub fn active(&self, plugin_id: &str) -> Result<Option<InstalledPlugin>> {
        let Some(selection) = self.state.active.get(plugin_id) else {
            return Ok(None);
        };
        let root = self.package_path(plugin_id, &selection.version);
        let manifest = package::verify(&root, &self.keys)?;
        let data_dir = self
            .root
            .join("data")
            .join(plugin_id)
            .join(&selection.data_generation);
        Ok(Some(installed(manifest, root, data_dir)))
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
    fn persist(&mut self, next: State) -> Result<()> {
        atomic_json(&self.root.join("activation.json"), &next)?;
        self.state = next;
        Ok(())
    }
}

fn installed(manifest: Manifest, root: PathBuf, data_dir: PathBuf) -> InstalledPlugin {
    InstalledPlugin {
        ui_path: root.join(&manifest.entry.ui),
        worker_path: root.join(&manifest.entry.worker),
        manifest,
        root,
        data_dir,
    }
}

fn atomic_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let temp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp)?;
    file.write_all(&serde_json::to_vec(value)?)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temp, path)?;
    Ok(())
}

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

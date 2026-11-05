use super::Error;
use crate::storage;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::PathBuf, sync::Mutex};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Key {
    pub algorithm: String,
    pub public_key: String,
    pub fingerprint: String,
}
#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct File {
    schema_version: u32,
    hosts: BTreeMap<String, Key>,
}
pub(super) struct HostKeys {
    path: PathBuf,
    state: Mutex<Result<File, String>>,
}
impl HostKeys {
    pub fn open(root: &std::path::Path) -> Self {
        let path = root.join("ssh-hosts.json");
        let state = if path.exists() {
            fs::read(&path)
                .ok()
                .and_then(|b| serde_json::from_slice::<File>(&b).ok())
                .filter(|f| f.schema_version == 1)
                .ok_or_else(|| "SSH 主机信任文件无法读取，原文件已保留".into())
        } else {
            Ok(File {
                schema_version: 1,
                ..File::default()
            })
        };
        Self {
            path,
            state: Mutex::new(state),
        }
    }
    pub fn get(&self, scope: &str) -> Result<Option<Key>, Error> {
        self.state
            .lock()
            .unwrap()
            .as_ref()
            .map(|s| s.hosts.get(scope).cloned())
            .map_err(|e| Error::new("hostStorage", e.clone()))
    }
    pub fn save(&self, scope: &str, key: Key, expected: Option<&str>) -> Result<(), Error> {
        let mut state = self.state.lock().unwrap();
        let s = state
            .as_mut()
            .map_err(|e| Error::new("hostStorage", e.clone()))?;
        if s.hosts
            .get(scope)
            .is_some_and(|current| current.public_key == key.public_key)
        {
            return Ok(());
        }
        // A parallel confirmation cannot silently overwrite another user's decision.
        if s.hosts.get(scope).map(|k| k.public_key.as_str()) != expected {
            return Err(Error::new("hostChanged", "主机信任已变更，请重新连接确认"));
        }
        let mut hosts = s.hosts.clone();
        hosts.insert(scope.into(), key);
        let next = File {
            schema_version: 1,
            hosts,
        };
        storage::write_json(&self.path, &next)
            .map_err(|_| Error::new("hostStorage", "无法保存 SSH 主机信任"))?;
        *s = next;
        Ok(())
    }
}

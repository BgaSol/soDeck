//! Official plugins are discovered from the repository's own release configurations.
use crate::{
    model::UpdateSource,
    updates::{validate_source, ReleaseClient, CORE_ID, REPOSITORY},
    AppCore,
};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fs};

const CACHE_TTL: u64 = 60 * 60 * 1000;
const MAX_PLUGINS: usize = 128;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OfficialPlugin {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub tag_prefix: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub order: i32,
}

impl OfficialPlugin {
    pub fn source(&self) -> UpdateSource {
        UpdateSource {
            repository: REPOSITORY.into(),
            tag_prefix: self.tag_prefix.clone(),
        }
    }

    fn validate(&self) -> Result<(), String> {
        if self.id == CORE_ID
            || self.id.is_empty()
            || self.id.len() > 128
            || !self.id.starts_with(|c: char| c.is_ascii_alphanumeric())
            || !self
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        {
            return Err("官方插件 ID 无效".into());
        }
        if self.name.trim().is_empty() || self.name.len() > 256 || self.description.len() > 4096 {
            return Err(format!("官方插件 {} 的名称或说明无效", self.id));
        }
        if self.icon.len() > 64
            || !self
                .icon
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(format!("官方插件 {} 的图标名称无效", self.id));
        }
        validate_source(&self.source())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OfficialCatalog {
    pub plugins: Vec<OfficialPlugin>,
    pub fetched_at: u64,
    #[serde(default)]
    pub error: Option<String>,
}

fn validate_plugins(plugins: &[OfficialPlugin]) -> Result<(), String> {
    if plugins.len() > MAX_PLUGINS {
        return Err("官方插件列表过大".into());
    }
    let mut ids = HashSet::new();
    let mut prefixes = HashSet::new();
    for plugin in plugins {
        plugin.validate()?;
        if !ids.insert(&plugin.id) || !prefixes.insert(&plugin.tag_prefix) {
            return Err("官方插件列表包含重复 ID 或发行标签前缀".into());
        }
    }
    Ok(())
}

#[derive(Deserialize)]
struct Tree {
    tree: Vec<TreeEntry>,
    truncated: bool,
}

#[derive(Deserialize)]
struct TreeEntry {
    path: String,
    #[serde(rename = "type")]
    kind: String,
    sha: String,
}

impl ReleaseClient {
    fn official_plugins(&self) -> Result<Vec<OfficialPlugin>, String> {
        let tree: Tree = self.official_repository_json("git/trees/main?recursive=1", false)?;
        if tree.truncated {
            return Err("仓库目录不完整，无法读取官方插件列表".into());
        }
        let mut plugins = Vec::new();
        for entry in tree.tree {
            let parts: Vec<_> = entry.path.split('/').collect();
            if entry.kind != "blob"
                || parts.len() != 3
                || parts[0] != "plugins"
                || parts[2] != "release.json"
            {
                continue;
            }
            if plugins.len() >= MAX_PLUGINS
                || entry.sha.len() != 40
                || !entry.sha.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err("仓库插件目录无效或过大".into());
            }
            // Blob IDs keep all metadata pinned to the tree that was just discovered.
            let plugin: OfficialPlugin = self
                .official_repository_json(&format!("git/blobs/{}", entry.sha), true)
                .map_err(|error| format!("无法读取 {}：{error}", entry.path))?;
            plugins.push(plugin);
        }
        validate_plugins(&plugins)?;
        plugins.sort_by(|a, b| a.order.cmp(&b.order).then_with(|| a.id.cmp(&b.id)));
        Ok(plugins)
    }
}

impl AppCore {
    pub fn official_plugins(
        &self,
        client: &ReleaseClient,
        refresh: bool,
    ) -> Result<OfficialCatalog, String> {
        let path = self
            .store
            .lock()
            .unwrap()
            .root
            .join("official-plugins.json");
        // Only catalog requests share this lock; server work and plugin updates stay independent.
        let mut cached = self.official_catalog.lock().unwrap();
        if cached.is_none() {
            if let Ok(bytes) = fs::read(&path) {
                if bytes.len() <= 1024 * 1024 {
                    if let Ok(value) = serde_json::from_slice::<OfficialCatalog>(&bytes) {
                        if validate_plugins(&value.plugins).is_ok() {
                            *cached = Some(value);
                        }
                    }
                }
            }
        }
        let now = crate::core::now();
        if !refresh {
            if let Some(value) = cached
                .as_ref()
                .filter(|v| v.fetched_at <= now && now - v.fetched_at < CACHE_TTL)
            {
                return Ok(value.clone());
            }
        }
        match client.official_plugins() {
            Ok(plugins) => {
                let mut catalog = OfficialCatalog {
                    plugins,
                    fetched_at: crate::core::now(),
                    error: None,
                };
                if let Err(error) = crate::storage::write_json(&path, &catalog) {
                    catalog.error = Some(format!("列表已刷新，但未能保存离线缓存：{error}"));
                }
                *cached = Some(catalog.clone());
                Ok(catalog)
            }
            Err(error) => match cached.as_mut() {
                Some(value) => {
                    value.error = Some(format!(
                        "无法刷新官方插件，正在显示上次成功获取的列表：{error}"
                    ));
                    Ok(value.clone())
                }
                None => Err(format!("无法获取官方插件列表：{error}")),
            },
        }
    }
}

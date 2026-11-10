//! Independent GitHub releases and staged plugin programs. No live process replacement.
use crate::{model::*, plugins, AppCore, API_VERSION, TARGET};
use semver::Version;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File, OpenOptions},
    io::Read,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

pub const CORE_ID: &str = "sodeck.core";
pub const REPOSITORY: &str = "BgaSol/soDeck";
const MAX_EXPANDED: u64 = 2 * 1024 * 1024 * 1024;

pub async fn open_release_page(handle: tauri::AppHandle) -> Result<(), String> {
    open_web_page(handle, "https://github.com/BgaSol/soDeck/releases").await
}

fn web_page_url(value: &str) -> Result<reqwest::Url, String> {
    let url = reqwest::Url::parse(value).map_err(|_| "更新说明链接无效".to_string())?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err("更新说明仅支持 HTTP/HTTPS 链接".into());
    }
    Ok(url)
}

pub async fn open_web_page(handle: tauri::AppHandle, value: &str) -> Result<(), String> {
    let url = web_page_url(value)?;
    crate::desktop_open::web_page(handle, url).await
}

#[cfg(test)]
mod web_page_tests {
    use super::web_page_url;

    #[test]
    fn update_links_only_accept_absolute_web_urls() {
        for value in [
            "https://github.com/BgaSol/soDeck/releases",
            "http://example.com/notes?q=中文&version=1#changes",
        ] {
            assert!(web_page_url(value).is_ok(), "{value}");
        }
        for value in [
            "javascript:alert(1)",
            "data:text/html,<script>alert(1)</script>",
            "file:///etc/passwd",
            "releases/v0.9.0.md",
            "--help",
            "https://",
        ] {
            assert!(web_page_url(value).is_err(), "{value}");
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UpdateView {
    pub id: String,
    pub phase: String,
    pub version: Option<String>,
    pub notes: String,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub error: Option<String>,
    pub error_stage: Option<String>,
    pub checked_at: Option<u64>,
    #[serde(default)]
    pub network: Option<crate::update_network::Attempt>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "test-fixtures")]
    #[test]
    fn pinned_upgrade_uses_exact_component_and_rejects_incompatible_inputs() {
        let source = UpdateSource {
            repository: REPOSITORY.into(),
            tag_prefix: "plugin-docker-v".into(),
        };
        let manifest: UpdateManifest = serde_json::from_value(serde_json::json!({
            "id": "sodeck.docker", "version": "0.1.3", "apiVersion": "1",
            "source": source, "manifestUrl": "https://github.com/BgaSol/soDeck/releases/download/plugin-docker-v0.1.3/plugin-update.json",
            "platforms": { (TARGET): { "url": "https://github.com/BgaSol/soDeck/releases/download/plugin-docker-v0.1.3/package.zip", "sha256": "a".repeat(64) } }
        })).unwrap();
        let client = ReleaseClient::github()
            .unwrap()
            .test_pin(vec![manifest.clone()]);
        assert_eq!(
            client
                .discover(&source, "sodeck.docker", "0.1.2")
                .unwrap()
                .unwrap()
                .version,
            "0.1.3"
        );
        assert!(client
            .discover(&source, "sodeck.docker", "0.1.3")
            .unwrap()
            .is_none());
        assert!(client.discover(&source, CORE_ID, "0.1.2").is_err());
        for invalid in 0..4 {
            let mut changed = manifest.clone();
            match invalid {
                0 => changed.source.as_mut().unwrap().repository = "other/repo".into(),
                1 => changed.api_version = "2".into(),
                2 => changed.platforms.clear(),
                _ => {
                    changed.platforms.get_mut(TARGET).unwrap().url =
                        "file:///tmp/package.zip".into()
                }
            }
            assert!(ReleaseClient::github()
                .unwrap()
                .test_pin(vec![changed])
                .discover(&source, "sodeck.docker", "0.1.2")
                .is_err());
        }
    }
    #[test]
    fn repository_blob_accepts_both_cached_formats_and_rejects_invalid_envelopes() {
        use russh::keys::ssh_encoding::base64::{Base64, Encoding};
        let raw = br#"{"id":"test.plugin","name":"test"}"#;
        let encoded = Base64::encode_string(raw);
        let envelope = serde_json::json!({"encoding":"base64","size":raw.len(),"content":format!("{}\n{}", &encoded[..8], &encoded[8..])});
        let a: serde_json::Value = decode_repository_blob(raw).unwrap();
        let b: serde_json::Value =
            decode_repository_blob(&serde_json::to_vec(&envelope).unwrap()).unwrap();
        assert_eq!(a, b);
        for invalid in [
            serde_json::json!({"encoding":"base64","size":1,"content":encoded}),
            serde_json::json!({"encoding":"base64","size":1,"content":"!"}),
            serde_json::json!({"encoding":"utf8","content":"{}"}),
        ] {
            assert!(decode_repository_blob::<serde_json::Value>(
                &serde_json::to_vec(&invalid).unwrap()
            )
            .is_err());
        }
    }
    #[test]
    fn tar_gzip_extracts_regular_files_and_rejects_links() {
        let root = std::env::temp_dir().join(format!(
            "sodeck-tar-{}-{}",
            std::process::id(),
            crate::core::now()
        ));
        fs::create_dir_all(&root).unwrap();
        let archive = root.join("plugin.tar.gz");
        for link in [false, true] {
            let file = File::create(&archive).unwrap();
            let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
                file,
                flate2::Compression::default(),
            ));
            let manifest = serde_json::json!({"id":"test.tar","name":"Tar","version":"0.1.0","apiVersion":"1","target":TARGET,"entry":"bin/tool"}).to_string();
            for (name, bytes) in [
                ("plugin/plugin.json", manifest.as_bytes()),
                ("plugin/bin/tool", b"tool".as_slice()),
            ] {
                let mut header = tar::Header::new_gnu();
                header.set_size(bytes.len() as u64);
                header.set_mode(0o755);
                header.set_cksum();
                builder.append_data(&mut header, name, bytes).unwrap();
            }
            if link {
                let mut header = tar::Header::new_gnu();
                header.set_entry_type(tar::EntryType::Symlink);
                header.set_size(0);
                header.set_mode(0o777);
                header.set_link_name("../../outside").unwrap();
                header.set_cksum();
                builder
                    .append_data(&mut header, "plugin/link", std::io::empty())
                    .unwrap();
            }
            builder.into_inner().unwrap().finish().unwrap();
            let result = unpack(
                &archive,
                &root.join(if link { "linked" } else { "normal" }),
                false,
            );
            if link {
                assert!(result.unwrap_err().contains("普通文件"));
            } else {
                assert_eq!(fs::read(result.unwrap().join("bin/tool")).unwrap(), b"tool");
            }
        }
        fs::remove_dir_all(root).unwrap();
    }
}

#[derive(Default)]
pub struct UpdateState {
    pub items: HashMap<String, UpdateView>,
    candidates: HashMap<String, UpdateManifest>,
    pub(crate) busy: HashSet<String>,
    pub(crate) checking_all: bool,
    last_checked_at: u64,
    next_check_at: u64,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(default)]
struct UpdateCache {
    items: HashMap<String, UpdateView>,
    candidates: HashMap<String, UpdateManifest>,
    last_checked_at: u64,
    next_check_at: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateOverview {
    pub checking: bool,
    pub busy: bool,
    pub last_checked_at: u64,
    pub next_check_at: u64,
}

pub struct CheckBatch {
    core: Arc<AppCore>,
    successes: usize,
}
impl CheckBatch {
    pub fn succeeded(&mut self) {
        self.successes += 1;
    }
}
impl Drop for CheckBatch {
    fn drop(&mut self) {
        let mut state = self.core.updates.lock().unwrap();
        state.checking_all = false;
        let time = crate::core::now();
        if self.successes > 0 {
            state.last_checked_at = time;
        }
        let failed = state
            .items
            .values()
            .any(|v| v.error_stage.as_deref() == Some("check"));
        state.next_check_at = time
            + if failed {
                15 * 60 * 1000
            } else {
                24 * 60 * 60 * 1000
            };
        drop(state);
        self.core.persist_update_cache();
        self.core.changed();
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PackageAsset {
    pub url: String,
    #[serde(default)]
    pub sha256: String,
    #[serde(default)]
    pub signature: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateManifest {
    pub id: String,
    pub version: String,
    #[serde(default)]
    pub api_version: String,
    #[serde(default)]
    pub notes: String,
    pub platforms: HashMap<String, PackageAsset>,
    #[serde(default)]
    pub manifest_url: String,
    #[serde(default)]
    pub source: Option<UpdateSource>,
}

#[derive(Clone, Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<ReleaseAsset>,
}
#[derive(Clone, Deserialize)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
}

pub fn validate_source(source: &UpdateSource) -> Result<(), String> {
    let parts: Vec<_> = source.repository.split('/').collect();
    if parts.len() != 2
        || parts.iter().any(|s| {
            s.is_empty()
                || *s == "."
                || *s == ".."
                || !s
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        })
    {
        return Err("发行仓库应为 owner/repository".into());
    }
    if source.tag_prefix.is_empty()
        || source.tag_prefix.len() > 100
        || !source
            .tag_prefix
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
    {
        return Err("标签前缀只能包含英文字母、数字、点、横线和下划线".into());
    }
    Ok(())
}

fn stable_version(value: &str) -> Result<Version, String> {
    let version = Version::parse(value).map_err(|_| "版本应为 major.minor.patch".to_string())?;
    if !version.pre.is_empty() || !version.build.is_empty() {
        return Err("当前只发布正式版本".into());
    }
    Ok(version)
}

pub struct ReleaseClient {
    pub(crate) network: crate::update_network::Transport,
    api_base: String,
    #[cfg(feature = "test-fixtures")]
    pinned: Option<Vec<UpdateManifest>>,
    releases: Arc<std::sync::Mutex<HashMap<String, Result<Vec<Release>, String>>>>,
}

fn decode_repository_blob<T: serde::de::DeserializeOwned>(
    bytes: &[u8],
) -> Result<T, crate::update_network::Error> {
    use crate::update_network::Error;
    use russh::keys::ssh_encoding::base64::{Base64, Encoding};
    let value: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|e| Error::remote(format!("插件配置格式错误：{e}")))?;
    // Public proxies may cache either GitHub's raw JSON or its base64 envelope for this URL.
    if value.get("encoding").is_some() || value.get("content").is_some() {
        if value["encoding"] != "base64" {
            return Err(Error::remote("插件配置编码不受支持"));
        }
        let content = value["content"]
            .as_str()
            .ok_or_else(|| Error::remote("插件配置内容缺失"))?;
        let content: String = content
            .chars()
            .filter(|c| !c.is_ascii_whitespace())
            .collect();
        let decoded =
            Base64::decode_vec(&content).map_err(|_| Error::remote("插件配置 Base64 无效"))?;
        if value["size"].as_u64() != Some(decoded.len() as u64) {
            return Err(Error::remote("插件配置长度不一致"));
        }
        serde_json::from_slice(&decoded)
            .map_err(|e| Error::remote(format!("插件配置格式错误：{e}")))
    } else {
        serde_json::from_value(value).map_err(|e| Error::remote(format!("插件配置格式错误：{e}")))
    }
}

impl ReleaseClient {
    pub fn diagnose(
        &self,
        prefix: Option<&str>,
    ) -> Result<Vec<crate::update_network::Diagnostic>, String> {
        use crate::update_network::{diagnostic, BUILTIN_MIRRORS};
        let network = self.network.clone().only_endpoint(prefix)?;
        let file_only = prefix.is_some_and(|p| {
            BUILTIN_MIRRORS
                .iter()
                .any(|m| m.prefix == p && !m.supports_api)
        });
        let client = Self::with_network(self.api_base.clone(), network);
        let mut results = Vec::new();
        if !file_only {
            diagnostic(&mut results, "发行版本 API", || {
                client.repository_releases(REPOSITORY)
            });
            diagnostic(&mut results, "插件目录与配置", || {
                let tree: serde_json::Value =
                    client.official_repository_json("git/trees/main?recursive=1", false)?;
                if tree["truncated"] != false {
                    return Err("仓库目录不完整".into());
                }
                let blob = tree["tree"]
                    .as_array()
                    .and_then(|entries| {
                        entries.iter().find(|entry| {
                            entry["type"] == "blob"
                                && entry["path"].as_str().is_some_and(|p| {
                                    p.starts_with("plugins/") && p.ends_with("/release.json")
                                })
                        })
                    })
                    .and_then(|entry| entry["sha"].as_str())
                    .ok_or("未找到插件配置")?;
                if blob.len() != 40 || !blob.bytes().all(|c| c.is_ascii_hexdigit()) {
                    return Err("插件配置标识无效".into());
                }
                let _: crate::official_plugins::OfficialPlugin =
                    client.official_repository_json(&format!("git/blobs/{blob}"), true)?;
                Ok(())
            });
        }
        let manifest = diagnostic(&mut results, "更新清单", || {
            let url = format!(
                "https://github.com/{REPOSITORY}/releases/latest/download/core-update.json"
            );
            #[cfg(feature = "test-fixtures")]
            let url = if client.api_base.starts_with("http://127.0.0.1:") {
                format!("{}/core-update.json", client.api_base)
            } else {
                url
            };
            let manifest: UpdateManifest = client.json(&url)?;
            if manifest.id != CORE_ID {
                return Err("核心更新清单 ID 无效".into());
            }
            stable_version(&manifest.version)?;
            let asset = manifest.platforms.get(TARGET).ok_or("当前平台没有更新包")?;
            client.url(&asset.url)?;
            if asset.signature.is_empty() {
                return Err("核心更新清单缺少签名".into());
            }
            Ok(manifest)
        });
        if let Some(manifest) = manifest {
            diagnostic(&mut results, "安装包首段（最多 64 KiB）", || {
                client
                    .network
                    .sample_package(&manifest.platforms[TARGET].url)
            });
        }
        Ok(results)
    }

    pub fn github() -> Result<Self, String> {
        let preferences = crate::update_network::Preferences {
            fallback_enabled: false,
            ..Default::default()
        };
        let network =
            crate::update_network::Transport::new(preferences, Arc::new(Default::default()))?;
        Ok(Self::with_network("https://api.github.com".into(), network))
    }
    pub(crate) fn with_network(
        api_base: String,
        network: crate::update_network::Transport,
    ) -> Self {
        Self {
            network,
            api_base,
            #[cfg(feature = "test-fixtures")]
            pinned: None,
            releases: Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }
    #[cfg(feature = "test-fixtures")]
    pub fn test_github(token: String) -> Result<Self, String> {
        let mut client = Self::github()?;
        client.network = client.network.test_token(token);
        Ok(client)
    }
    #[cfg(feature = "test-fixtures")]
    pub fn test_pin(mut self, manifests: Vec<UpdateManifest>) -> Self {
        self.pinned = Some(manifests);
        self
    }
    #[cfg(feature = "test-fixtures")]
    pub fn test_proxy(prefix: &str) -> Result<Self, String> {
        let mut client = Self::github()?;
        client.network = client.network.only_endpoint(Some(prefix))?;
        Ok(client)
    }
    #[cfg(feature = "test-fixtures")]
    pub fn test_server(base: &str) -> Self {
        Self::test_mirrors(base, Vec::new(), Arc::new(Default::default()))
    }

    #[cfg(feature = "test-fixtures")]
    pub fn test_mirrors(
        base: &str,
        mirrors: Vec<(String, bool)>,
        state: Arc<crate::update_network::State>,
    ) -> Self {
        Self::with_network(
            base.into(),
            crate::update_network::Transport::test_server(base, mirrors, state),
        )
    }

    pub fn for_component(&self, core: &Arc<AppCore>, id: &str) -> Self {
        Self {
            network: self
                .network
                .clone()
                .observed(core.update_network_observer(Some(id)), {
                    let weak = Arc::downgrade(core);
                    Arc::new(move || weak.upgrade().is_none_or(|core| core.is_closing()))
                }),
            api_base: self.api_base.clone(),
            #[cfg(feature = "test-fixtures")]
            pinned: self.pinned.clone(),
            releases: self.releases.clone(),
        }
    }
    fn url(&self, value: &str) -> Result<reqwest::Url, String> {
        self.network.validate_url(value)
    }
    fn json<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T, String> {
        self.json_with_accept(url, "application/json")
    }

    pub(crate) fn official_repository_json<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        raw: bool,
    ) -> Result<T, String> {
        let url = format!("{}/repos/{REPOSITORY}/{path}", self.api_base);
        if raw {
            self.network.read(
                &url,
                crate::update_network::RequestKind::Api,
                "application/vnd.github+json",
                decode_repository_blob,
            )
        } else {
            self.json_with_accept(&url, "application/vnd.github+json")
        }
    }

    fn json_with_accept<T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
        accept: &str,
    ) -> Result<T, String> {
        let parsed = self.url(url)?;
        let kind = if parsed.host_str() == Some("api.github.com")
            || parsed.path().starts_with("/repos/")
        {
            crate::update_network::RequestKind::Api
        } else {
            crate::update_network::RequestKind::File
        };
        self.network.json(url, kind, accept)
    }

    pub fn discover(
        &self,
        source: &UpdateSource,
        id: &str,
        current: &str,
    ) -> Result<Option<UpdateManifest>, String> {
        validate_source(source)?;
        let current = stable_version(current)?;
        #[cfg(feature = "test-fixtures")]
        if let Some(pinned) = &self.pinned {
            let manifest = pinned
                .iter()
                .find(|m| m.id == id)
                .ok_or("固定输入缺少组件")?;
            let binding = manifest.source.as_ref().ok_or("固定输入缺少来源")?;
            if binding.repository != source.repository || binding.tag_prefix != source.tag_prefix {
                return Err("固定输入来源不匹配".into());
            }
            if stable_version(&manifest.version)? <= current {
                return Ok(None);
            }
            if !manifest.platforms.contains_key(TARGET)
                || (id != CORE_ID && manifest.api_version.split('.').next() != Some(API_VERSION))
            {
                return Err("固定输入平台或 API 不兼容".into());
            }
            self.url(&manifest.manifest_url)?;
            self.url(&manifest.platforms[TARGET].url)?;
            return Ok(Some(manifest.clone()));
        }
        let all = self.repository_releases(&source.repository)?;
        let mut releases = Vec::new();
        for release in all {
            if release.draft || release.prerelease {
                continue;
            }
            if let Some(version) = release
                .tag_name
                .strip_prefix(&source.tag_prefix)
                .and_then(|v| stable_version(v).ok())
            {
                if version > current {
                    releases.push((version, release));
                }
            }
        }
        releases.sort_by(|a, b| b.0.cmp(&a.0));
        let mut incompatible = None;
        for (version, release) in releases {
            let name = if id == CORE_ID {
                "core-update.json"
            } else {
                "plugin-update.json"
            };
            let Some(asset) = release.assets.iter().find(|a| a.name == name) else {
                continue;
            };
            let mut manifest: UpdateManifest = self.json(&asset.browser_download_url)?;
            if manifest.id != id || stable_version(&manifest.version)? != version {
                return Err("发行标签与更新清单的组件或版本不一致".into());
            }
            if id != CORE_ID && manifest.api_version.split('.').next() != Some(API_VERSION) {
                incompatible = Some(format!(
                    "新版 {} 要求核心 API {}，当前为 {API_VERSION}",
                    manifest.version, manifest.api_version
                ));
                continue;
            }
            if !manifest.platforms.contains_key(TARGET) {
                incompatible = Some(format!(
                    "新版 {} 尚未提供本机平台 {TARGET} 的包",
                    manifest.version
                ));
                continue;
            }
            self.url(&manifest.platforms[TARGET].url)?;
            manifest.manifest_url = asset.browser_download_url.clone();
            manifest.source = Some(source.clone());
            return Ok(Some(manifest));
        }
        if let Some(error) = incompatible {
            return Err(error);
        }
        Ok(None)
    }

    fn repository_releases(&self, repository: &str) -> Result<Vec<Release>, String> {
        let mut cache = self.releases.lock().unwrap();
        if let Some(releases) = cache.get(repository) {
            return releases.clone();
        }
        let result = (|| {
            let mut releases = Vec::new();
            for page in 1..=10 {
                let batch: Vec<Release> = self.json(&format!(
                    "{}/repos/{}/releases?per_page=100&page={page}",
                    self.api_base, repository
                ))?;
                let done = batch.len() < 100;
                releases.extend(batch);
                if done {
                    break;
                }
                if page == 10 {
                    return Err("发行记录超过 1000 条，请收窄更新仓库".into());
                }
            }
            Ok(releases)
        })();
        cache.insert(repository.into(), result.clone());
        result
    }

    fn validate_candidate(&self, cached: &UpdateManifest) -> Result<(), String> {
        let fresh: UpdateManifest = self.json(&cached.manifest_url)?;
        if fresh.id != cached.id
            || fresh.version != cached.version
            || fresh.api_version != cached.api_version
            || serde_json::to_value(&fresh.platforms).ok()
                != serde_json::to_value(&cached.platforms).ok()
        {
            return Err("发行信息已改变，请重新检查更新".into());
        }
        Ok(())
    }

    fn download(
        &self,
        asset: &PackageAsset,
        path: &Path,
        progress: impl FnMut(u64, Option<u64>),
    ) -> Result<(), String> {
        if asset.sha256.len() != 64 || !asset.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("更新包缺少有效的 SHA-256".into());
        }
        self.network
            .download(&asset.url, path, progress, |_, hash| {
                if hash != asset.sha256.to_lowercase() {
                    Err(crate::update_network::Error::remote(
                        "下载包不完整或 SHA-256 不匹配，旧插件未改变",
                    ))
                } else {
                    Ok(())
                }
            })
    }

    pub(crate) fn download_core(
        &self,
        update: &tauri_plugin_updater::Update,
        path: &Path,
        progress: impl FnMut(u64, Option<u64>),
    ) -> Result<(), String> {
        let fallback = format!(
            "https://github.com/{REPOSITORY}/releases/download/v{}/core-update.json",
            update.version
        );
        let source = update
            .raw_json
            .get("manifestUrl")
            .and_then(|value| value.as_str())
            .unwrap_or(&fallback);
        let fresh: UpdateManifest = self.json(source)?;
        let asset = fresh
            .platforms
            .get(TARGET)
            .ok_or("核心更新不支持本机平台")?;
        if fresh.id != CORE_ID
            || fresh.version != update.version
            || asset.url != update.download_url.as_str()
            || asset.signature != update.signature
        {
            return Err("核心发行信息已改变，请重新检查更新".into());
        }
        self.network
            .download(update.download_url.as_str(), path, progress, |path, _| {
                let bytes = crate::desktop_updates::read_package(path)
                    .map_err(crate::update_network::Error::Local)?;
                update.verify(&bytes).map_err(|e| {
                    crate::update_network::Error::remote(format!("核心包签名校验失败：{e}"))
                })
            })
    }
}

fn safe_path(path: &Path) -> Result<(), String> {
    if path.as_os_str().is_empty()
        || path.to_string_lossy().contains(['\\', ':'])
        || path
            .components()
            .any(|p| !matches!(p, Component::Normal(_)))
    {
        return Err("插件包包含不安全的路径".into());
    }
    Ok(())
}

fn unpack(archive: &Path, output: &Path, zip_format: bool) -> Result<PathBuf, String> {
    fs::create_dir_all(output).map_err(|e| e.to_string())?;
    let mut expanded = 0u64;
    let mut entries = 0usize;
    let mut write =
        |path: &Path, directory: bool, size: u64, reader: &mut dyn Read| -> Result<(), String> {
            safe_path(path)?;
            entries += 1;
            expanded = expanded.checked_add(size).ok_or("解压大小无效")?;
            if expanded > MAX_EXPANDED || entries > 20_000 {
                return Err("插件包解压大小或文件数超限".into());
            }
            let target = output.join(path);
            if directory {
                return fs::create_dir_all(target).map_err(|e| e.to_string());
            }
            fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(target)
                .map_err(|e| e.to_string())?;
            let copied =
                std::io::copy(&mut reader.take(size + 1), &mut file).map_err(|e| e.to_string())?;
            if copied != size {
                return Err("插件包文件大小不一致".into());
            }
            Ok(())
        };
    if zip_format {
        let mut archive = zip::ZipArchive::new(File::open(archive).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).map_err(|e| e.to_string())?;
            if entry.is_symlink() {
                return Err("插件包不支持符号链接".into());
            }
            let name = entry.name().map_err(|e| e.to_string())?;
            let path = PathBuf::from(name.trim_end_matches('/'));
            write(&path, entry.is_dir(), entry.size(), &mut entry)?;
        }
    } else {
        let decoded = flate2::read::GzDecoder::new(File::open(archive).map_err(|e| e.to_string())?);
        let mut archive = tar::Archive::new(decoded);
        for entry in archive.entries().map_err(|e| e.to_string())? {
            let mut entry = entry.map_err(|e| e.to_string())?;
            let kind = entry.header().entry_type();
            if !kind.is_file() && !kind.is_dir() {
                return Err("插件包只支持普通文件和目录".into());
            }
            let path = entry.path().map_err(|e| e.to_string())?.into_owned();
            write(&path, kind.is_dir(), entry.size(), &mut entry)?;
        }
    }
    let root = if output.join("plugin.json").is_file() {
        output.to_owned()
    } else {
        let children = fs::read_dir(output)
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        if children.len() != 1 || !children[0].path().is_dir() {
            return Err("插件包应包含一个带 plugin.json 的目录".into());
        }
        children[0].path()
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let manifest: Manifest =
            serde_json::from_slice(&fs::read(root.join("plugin.json")).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        safe_path(Path::new(&manifest.entry))?;
        fs::set_permissions(root.join(manifest.entry), fs::Permissions::from_mode(0o755))
            .map_err(|e| e.to_string())?;
    }
    Ok(root)
}

pub struct Operation {
    core: Arc<AppCore>,
    id: String,
}
impl Drop for Operation {
    fn drop(&mut self) {
        let mut state = self.core.updates.lock().unwrap();
        state.busy.remove(&self.id);
        if let Some(view) = state.items.get_mut(&self.id) {
            if ["waiting", "downloading", "preparing", "checking"].contains(&view.phase.as_str()) {
                view.error_stage = Some(
                    if view.phase == "checking" {
                        "check"
                    } else {
                        "download"
                    }
                    .into(),
                );
                view.phase = "error".into();
                view.error = Some("更新任务已中断，请重试".into());
            }
        }
        drop(state);
        self.core.changed();
    }
}

impl AppCore {
    pub(crate) fn invalidate_update(&self, id: &str) {
        let mut state = self.updates.lock().unwrap();
        state.items.remove(id);
        state.candidates.remove(id);
        state.next_check_at = 0;
        drop(state);
        self.persist_update_cache();
    }
    pub fn fail_update(&self, id: &str, error: String) {
        self.update_phase(id, "error", Some(error));
        self.changed();
    }
    pub fn failed_core_install(&self, error: String) {
        if let Some(view) = self.updates.lock().unwrap().items.get_mut(CORE_ID) {
            view.phase = "ready".into();
            view.error_stage = Some("install".into());
            view.error = Some(error);
        }
        self.changed();
    }
    pub fn updates_busy(&self) -> bool {
        let state = self.updates.lock().unwrap();
        state.checking_all || !state.busy.is_empty()
    }
    pub fn update_views(&self) -> HashMap<String, UpdateView> {
        let snapshot = self.snapshot();
        let mut items = self.updates.lock().unwrap().items.clone();
        for p in snapshot.plugins {
            if let Some(version) = p.pending_version {
                let view = items
                    .entry(p.manifest.id.clone())
                    .or_insert_with(|| UpdateView {
                        id: p.manifest.id,
                        ..Default::default()
                    });
                view.phase = "ready".into();
                view.version = Some(version);
                view.error = None;
                view.error_stage = None;
            }
        }
        items
    }

    pub fn update_overview(&self) -> UpdateOverview {
        let state = self.updates.lock().unwrap();
        UpdateOverview {
            checking: state.checking_all,
            busy: !state.busy.is_empty(),
            last_checked_at: state.last_checked_at,
            next_check_at: state.next_check_at,
        }
    }

    pub fn begin_update_check(self: &Arc<Self>, automatic: bool) -> Option<CheckBatch> {
        let enabled = self.store.lock().unwrap().preferences.automatic_updates;
        let mut state = self.updates.lock().unwrap();
        if self.is_closing()
            || state.checking_all
            || !state.busy.is_empty()
            || (automatic && (!enabled || state.next_check_at > crate::core::now()))
        {
            return None;
        }
        state.checking_all = true;
        drop(state);
        self.changed();
        Some(CheckBatch {
            core: self.clone(),
            successes: 0,
        })
    }

    pub fn update_check_ids(&self) -> Vec<String> {
        let mut ids = vec![CORE_ID.to_string()];
        ids.extend(
            self.snapshot()
                .plugins
                .into_iter()
                .filter(|p| p.manifest.update_source.is_some() && p.pending_version.is_none())
                .map(|p| p.manifest.id),
        );
        ids
    }

    pub fn reserve_update_downloads(
        self: &Arc<Self>,
        ids: &[String],
    ) -> Result<Vec<(String, Operation)>, String> {
        if ids.is_empty() || ids.len() > 200 {
            return Err("请选择要下载的更新".into());
        }
        let snapshot = self.snapshot();
        let mut state = self.updates.lock().unwrap();
        if state.checking_all || !state.busy.is_empty() {
            return Err("已有更新任务在处理中，请完成后继续".into());
        }
        let mut unique = HashSet::new();
        for id in ids {
            if self.is_closing() || !unique.insert(id) || state.busy.contains(id) {
                return Err("更新任务已在处理中".into());
            }
            let view = state.items.get(id).ok_or("请先检查更新")?;
            if !["available", "error"].contains(&view.phase.as_str())
                || view.version.is_none()
                || view.error_stage.as_deref() == Some("check")
                || snapshot
                    .plugins
                    .iter()
                    .any(|p| p.manifest.id == *id && p.pending_version.is_some())
            {
                return Err("所选更新的状态已改变，请刷新后重试".into());
            }
        }
        let mut tasks = Vec::new();
        for id in ids {
            state.busy.insert(id.clone());
            let view = state.items.get_mut(id).unwrap();
            view.phase = "waiting".into();
            view.error = None;
            view.error_stage = None;
            tasks.push((
                id.clone(),
                Operation {
                    core: self.clone(),
                    id: id.clone(),
                },
            ));
        }
        drop(state);
        self.changed();
        Ok(tasks)
    }

    pub(crate) fn persist_update_cache(&self) {
        let _io = self.update_cache_io.lock().unwrap();
        let root = self.store.lock().unwrap().root.clone();
        let state = self.updates.lock().unwrap();
        let cache = UpdateCache {
            items: state
                .items
                .iter()
                .filter(|(_, v)| {
                    !["checking", "downloading", "preparing", "waiting"].contains(&v.phase.as_str())
                })
                .map(|(k, v)| {
                    let mut view = v.clone();
                    view.network = None;
                    (k.clone(), view)
                })
                .collect(),
            candidates: state.candidates.clone(),
            last_checked_at: state.last_checked_at,
            next_check_at: state.next_check_at,
        };
        drop(state);
        if let Err(error) = crate::storage::write_json(&root.join("updates-cache.json"), &cache) {
            self.log(None, "warn", &error, None);
        }
    }

    pub(crate) fn restore_update_cache(&self) {
        let snapshot = self.snapshot();
        let path = snapshot.data_directory.join("updates-cache.json");
        let Ok(bytes) = fs::read(path) else {
            return;
        };
        let Ok(mut cache) = serde_json::from_slice::<UpdateCache>(&bytes) else {
            return;
        };
        cache.candidates.retain(|id, m| {
            id != CORE_ID
                && snapshot.plugins.iter().any(|p| {
                    p.manifest.id == *id
                        && p.manifest.update_source == m.source
                        && p.pending_version.is_none()
                        && stable_version(&p.manifest.version)
                            .ok()
                            .zip(stable_version(&m.version).ok())
                            .is_some_and(|(old, new)| new > old)
                        && m.platforms.contains_key(TARGET)
                        && m.api_version.split('.').next() == Some(API_VERSION)
                })
        });
        cache.items.retain(|id, v| {
            v.network = None;
            id != CORE_ID
                && snapshot
                    .plugins
                    .iter()
                    .any(|p| p.manifest.id == *id && p.manifest.update_source.is_some())
                && (v.phase == "current"
                    || (v.error_stage.as_deref() == Some("check")
                        && ["error", "incompatible"].contains(&v.phase.as_str()))
                    || cache.candidates.contains_key(id))
        });
        let mut state = self.updates.lock().unwrap();
        state.items = cache.items;
        state.candidates = cache.candidates;
        state.last_checked_at = cache.last_checked_at;
        state.next_check_at = cache
            .next_check_at
            .min(crate::core::now() + 24 * 60 * 60 * 1000);
    }

    pub(crate) fn update_progress(&self, id: &str, downloaded: u64, total: Option<u64>) {
        if let Some(view) = self.updates.lock().unwrap().items.get_mut(id) {
            view.downloaded_bytes = downloaded;
            view.total_bytes = total;
        }
        self.changed();
    }
    pub(crate) fn update_phase(&self, id: &str, phase: &str, error: Option<String>) {
        let mut updates = self.updates.lock().unwrap();
        let view = updates
            .items
            .entry(id.into())
            .or_insert_with(|| UpdateView {
                id: id.into(),
                ..Default::default()
            });
        view.error_stage = error.as_ref().map(|_| {
            if view.phase == "checking" {
                "check"
            } else {
                "download"
            }
            .into()
        });
        view.phase = if phase == "error"
            && error
                .as_ref()
                .is_some_and(|e| e.contains("要求核心 API") || e.contains("尚未提供本机平台"))
        {
            "incompatible"
        } else {
            phase
        }
        .into();
        view.error = error.clone();
        drop(updates);
        if let Some(error) = error {
            self.log(None, "error", &format!("更新 {id}：{error}"), None);
        }
        self.changed();
    }
    pub(crate) fn update_operation(self: &Arc<Self>, id: &str) -> Result<Operation, String> {
        let mut updates = self.updates.lock().unwrap();
        if self.is_closing() {
            return Err("应用正在退出或重启".into());
        }
        if !updates.busy.insert(id.into()) {
            return Err("该组件正在检查或下载更新".into());
        }
        Ok(Operation {
            core: self.clone(),
            id: id.into(),
        })
    }
    pub fn check_component(
        self: &Arc<Self>,
        id: &str,
        client: &ReleaseClient,
    ) -> Result<Option<UpdateManifest>, String> {
        let client = client.for_component(self, id);
        let _operation = self.update_operation(id)?;
        if self
            .snapshot()
            .plugins
            .iter()
            .any(|p| p.manifest.id == id && p.pending_version.is_some())
        {
            return Ok(None);
        }
        self.update_phase(id, "checking", None);
        let previous_version = self
            .updates
            .lock()
            .unwrap()
            .items
            .get(id)
            .and_then(|v| v.version.clone());
        let result = (|| {
            let (source, current) = if id == CORE_ID {
                (
                    UpdateSource {
                        repository: REPOSITORY.into(),
                        tag_prefix: "v".into(),
                    },
                    self.snapshot().version,
                )
            } else {
                let store = self.store.lock().unwrap();
                let record = store
                    .plugins
                    .iter()
                    .find(|p| p.manifest.id == id)
                    .ok_or("插件记录不存在")?;
                if record.pending_update.is_some() {
                    return Err("该插件已准备好更新，请先重启应用".into());
                }
                (
                    record
                        .manifest
                        .update_source
                        .clone()
                        .ok_or("本地插件尚未配置更新来源")?,
                    record.manifest.version.clone(),
                )
            };
            let result = client.discover(&source, id, &current);
            if self.is_closing() {
                return Err("应用正在退出或重启".into());
            }
            result
        })();
        match &result {
            Ok(candidate) => {
                let mut updates = self.updates.lock().unwrap();
                updates.items.insert(
                    id.into(),
                    UpdateView {
                        id: id.into(),
                        phase: if candidate.is_some() {
                            "available"
                        } else {
                            "current"
                        }
                        .into(),
                        version: candidate.as_ref().map(|m| m.version.clone()),
                        notes: candidate
                            .as_ref()
                            .map(|m| m.notes.clone())
                            .unwrap_or_default(),
                        checked_at: Some(crate::core::now()),
                        ..Default::default()
                    },
                );
                updates.candidates.remove(id);
                if let Some(candidate) = candidate {
                    updates.candidates.insert(id.into(), candidate.clone());
                }
                drop(updates);
                if let Some(candidate) = candidate
                    .as_ref()
                    .filter(|c| previous_version.as_ref() != Some(&c.version))
                {
                    let _ = self.publish(
                        None,
                        PublishMessage {
                            title: format!("发现更新：{id} {}", candidate.version),
                            body: candidate.notes.clone(),
                            level: "info".into(),
                            notify: false,
                            dedupe_key: Some(format!("update:{id}:{}", candidate.version)),
                        },
                    );
                }
                self.changed();
            }
            Err(error) => self.update_phase(id, "error", Some(error.clone())),
        }
        self.persist_update_cache();
        result
    }
    pub fn download_plugin(
        self: &Arc<Self>,
        id: &str,
        client: &ReleaseClient,
    ) -> Result<bool, String> {
        if id == CORE_ID {
            return Err("核心更新请使用应用更新入口".into());
        }
        let _operation = self.update_operation(id)?;
        self.download_plugin_reserved(id, client, _operation)
    }

    pub fn download_plugin_reserved(
        self: &Arc<Self>,
        id: &str,
        client: &ReleaseClient,
        _operation: Operation,
    ) -> Result<bool, String> {
        let client = client.for_component(self, id);
        let candidate = self
            .updates
            .lock()
            .unwrap()
            .candidates
            .get(id)
            .cloned()
            .ok_or("请先检查插件更新")?;
        self.update_phase(id, "downloading", None);
        let result = self.prepare_plugin(id, &candidate, &client, false);
        match &result {
            Ok(restart) => self.update_phase(id, if *restart { "ready" } else { "current" }, None),
            Err(error) => self.update_phase(id, "error", Some(error.clone())),
        }
        self.persist_update_cache();
        result
    }
    fn prepare_plugin(
        &self,
        id: &str,
        candidate: &UpdateManifest,
        client: &ReleaseClient,
        initial_install: bool,
    ) -> Result<bool, String> {
        let root = self.store.lock().unwrap().root.clone();
        let archive = root.join(format!("plugin-download-{}.part", self.next_id()));
        let output =
            root.join("plugins")
                .join(id)
                .join(format!("{}-{}", candidate.version, self.next_id()));
        let result = (|| {
            client.validate_candidate(candidate)?;
            let asset = candidate
                .platforms
                .get(TARGET)
                .ok_or("更新不支持本机平台")?;
            client.download(asset, &archive, |n, total| {
                self.update_progress(id, n, total)
            })?;
            self.update_phase(id, "preparing", None);
            let zip_format = reqwest::Url::parse(&asset.url)
                .map_err(|e| e.to_string())?
                .path()
                .ends_with(".zip");
            let directory = unpack(&archive, &output, zip_format)?;
            let mut package = plugins::read_manifest(&directory)?;
            if package.manifest.id != id
                || package.manifest.version != candidate.version
                || package.manifest.api_version != candidate.api_version
            {
                return Err("下载包的 ID、版本或 API 与更新清单不一致".into());
            }
            let _mutation = self.mutations.lock().unwrap();
            if self.is_closing() {
                return Err("应用正在退出或重启".into());
            }
            let active = self.plugin_needs_restart(id);
            let mut store = self.store.lock().unwrap();
            if initial_install {
                if store.plugins.iter().any(|p| p.manifest.id == id) {
                    return Err("该插件已有记录，请在详情中检查更新".into());
                }
                if self.is_closing() {
                    return Err("应用正在退出或重启".into());
                }
                package.manifest.update_source = candidate.source.clone();
                package.enabled = true;
                store.register(package)?;
                drop(store);
                self.log(
                    None,
                    "info",
                    &format!("官方插件 {id} {} 已安装，重启后启用", candidate.version),
                    None,
                );
                return Ok(true);
            }
            let mut record = store
                .plugins
                .iter()
                .find(|p| p.manifest.id == id)
                .cloned()
                .ok_or("插件记录已移除")?;
            if record.manifest.update_source != candidate.source {
                return Err("更新来源已改变，请重新检查更新".into());
            }
            if stable_version(&record.manifest.version)? >= stable_version(&candidate.version)?
                || record.pending_update.is_some()
            {
                return Err("插件版本已改变，请重新检查更新".into());
            }
            let mut manifest = package.manifest;
            manifest.update_source = record.manifest.update_source.clone();
            let package = PluginPackage {
                directory,
                manifest,
            };
            let restart = record.enabled || active;
            record.update_error = None;
            if restart {
                record.pending_update = Some(package);
            } else {
                record.previous = Some(PluginPackage {
                    directory: record.directory.clone(),
                    manifest: record.manifest.clone(),
                });
                record.directory = package.directory;
                record.manifest = package.manifest;
            }
            store.register(record)?;
            drop(store);
            self.log(
                None,
                "info",
                &format!(
                    "插件 {id} {} 已下载{}",
                    candidate.version,
                    if restart {
                        "，重启后生效"
                    } else {
                        "，保持关闭"
                    }
                ),
                None,
            );
            Ok(restart)
        })();
        let _ = fs::remove_file(&archive);
        if result.is_err() && output.starts_with(root.join("plugins")) {
            let _ = fs::remove_dir_all(&output);
        }
        result
    }

    pub fn install_official_plugin(
        self: &Arc<Self>,
        id: &str,
        client: &ReleaseClient,
    ) -> Result<String, String> {
        let catalog = self.official_plugins(client, false)?;
        let source = catalog
            .plugins
            .iter()
            .find(|p| p.id == id)
            .ok_or("该插件不在官方列表中，请刷新列表后重试")?
            .source();
        let _operation = self.update_operation(id)?;
        self.update_phase(id, "checking", None);
        let result: Result<String, String> = (|| {
            if self
                .store
                .lock()
                .unwrap()
                .plugins
                .iter()
                .any(|p| p.manifest.id == id)
            {
                return Err("该插件已有记录，请在详情中检查更新".into());
            }
            let candidate = client
                .discover(&source, id, "0.0.0")?
                .ok_or("该官方插件尚未发布适用的正式版本")?;
            self.updates.lock().unwrap().items.insert(
                id.into(),
                UpdateView {
                    id: id.into(),
                    phase: "downloading".into(),
                    version: Some(candidate.version.clone()),
                    notes: candidate.notes.clone(),
                    ..Default::default()
                },
            );
            self.changed();
            self.prepare_plugin(id, &candidate, client, true)?;
            Ok(id.to_string())
        })();
        match &result {
            Ok(_) => self.update_phase(id, "ready", None),
            Err(error) => self.update_phase(id, "error", Some(error.clone())),
        }
        result
    }
}

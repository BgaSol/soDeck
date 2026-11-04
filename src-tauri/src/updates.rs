//! Independent GitHub releases and staged plugin programs. No live process replacement.
use crate::{model::*, plugins, AppCore, API_VERSION, TARGET};
use reqwest::blocking::Client;
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

pub const CORE_ID: &str = "sodeck.core";
pub const REPOSITORY: &str = "BgaSol/soDeck";
const MAX_PACKAGE: u64 = 512 * 1024 * 1024;
const MAX_EXPANDED: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateView {
    pub id: String,
    pub phase: String,
    pub version: Option<String>,
    pub notes: String,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
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
    #[serde(skip)]
    pub manifest_url: String,
    #[serde(skip)]
    pub source: Option<UpdateSource>,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<ReleaseAsset>,
}
#[derive(Deserialize)]
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
    client: Client,
    api_base: String,
    test_http: bool,
}

impl ReleaseClient {
    pub fn github() -> Result<Self, String> {
        Self::new("https://api.github.com".into(), false)
    }
    fn new(api_base: String, test_http: bool) -> Result<Self, String> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let mut builder = Client::builder()
            .user_agent("SoDeck-updates")
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(600))
            .https_only(!test_http);
        if test_http {
            builder = builder.no_proxy();
        }
        let client = builder.build().map_err(|e| e.to_string())?;
        Ok(Self {
            client,
            api_base,
            test_http,
        })
    }
    #[cfg(feature = "test-fixtures")]
    pub fn test_server(base: &str) -> Self {
        let url = reqwest::Url::parse(base).unwrap();
        assert_eq!(url.host_str(), Some("127.0.0.1"));
        Self::new(base.into(), true).unwrap()
    }
    fn url(&self, value: &str) -> Result<reqwest::Url, String> {
        let url = reqwest::Url::parse(value).map_err(|_| "更新下载地址无效".to_string())?;
        if url.scheme() != "https"
            && !(self.test_http && url.scheme() == "http" && url.host_str() == Some("127.0.0.1"))
        {
            return Err("更新下载必须使用 HTTPS".into());
        }
        Ok(url)
    }
    fn json<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T, String> {
        let url = self.url(url)?;
        let mut response = None;
        for attempt in 0..3 {
            match self
                .client
                .get(url.clone())
                .timeout(Duration::from_secs(30))
                .send()
            {
                Ok(value) => {
                    response = Some(value);
                    break;
                }
                Err(error)
                    if attempt < 2
                        && !error.is_timeout()
                        && (error.is_connect() || error.is_request()) =>
                {
                    // GitHub and local proxy connections can be reset before any response arrives.
                    std::thread::sleep(Duration::from_millis(150 * (attempt + 1)));
                }
                Err(error) => return Err(format!("无法读取发行信息：{error}")),
            }
        }
        let response = response
            .ok_or("无法建立更新连接")?
            .error_for_status()
            .map_err(|e| format!("无法读取发行信息：{e}"))?;
        let mut bytes = Vec::new();
        response
            .take(2 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > 2 * 1024 * 1024 {
            return Err("发行信息过大".into());
        }
        serde_json::from_slice(&bytes).map_err(|e| format!("发行信息格式错误：{e}"))
    }

    pub fn discover(
        &self,
        source: &UpdateSource,
        id: &str,
        current: &str,
    ) -> Result<Option<UpdateManifest>, String> {
        validate_source(source)?;
        let current = stable_version(current)?;
        let mut releases = Vec::new();
        for page in 1..=10 {
            let batch: Vec<Release> = self.json(&format!(
                "{}/repos/{}/releases?per_page=100&page={page}",
                self.api_base, source.repository
            ))?;
            let done = batch.len() < 100;
            for release in batch {
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
            if done {
                break;
            }
            if page == 10 {
                return Err("发行记录超过 1000 条，请收窄更新仓库".into());
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

    fn download(
        &self,
        asset: &PackageAsset,
        path: &Path,
        mut progress: impl FnMut(u64, Option<u64>),
    ) -> Result<(), String> {
        if asset.sha256.len() != 64 || !asset.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("更新包缺少有效的 SHA-256".into());
        }
        let mut response = self
            .client
            .get(self.url(&asset.url)?)
            .send()
            .and_then(|r| r.error_for_status())
            .map_err(|e| format!("下载失败：{e}"))?;
        let total = response.content_length();
        if total.is_some_and(|n| n > MAX_PACKAGE) {
            return Err("插件包超过 512 MiB".into());
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|e| e.to_string())?;
        let mut hash = Sha256::new();
        let mut buffer = [0; 64 * 1024];
        let mut count = 0;
        let mut last = Instant::now();
        loop {
            let read = response
                .read(&mut buffer)
                .map_err(|e| format!("下载中断：{e}"))?;
            if read == 0 {
                break;
            }
            count += read as u64;
            if count > MAX_PACKAGE {
                return Err("插件包超过 512 MiB".into());
            }
            file.write_all(&buffer[..read]).map_err(|e| e.to_string())?;
            hash.update(&buffer[..read]);
            if last.elapsed() >= Duration::from_millis(200) {
                progress(count, total);
                last = Instant::now();
            }
        }
        file.sync_all().map_err(|e| e.to_string())?;
        if total.is_some_and(|n| n != count)
            || format!("{:x}", hash.finalize()) != asset.sha256.to_lowercase()
        {
            return Err("下载包不完整或 SHA-256 不匹配，旧插件未改变".into());
        }
        progress(count, total);
        Ok(())
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
            let path = PathBuf::from(entry.name().trim_end_matches('/'));
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

pub(crate) struct Operation {
    core: Arc<AppCore>,
    id: String,
}
impl Drop for Operation {
    fn drop(&mut self) {
        self.core.updates.lock().unwrap().busy.remove(&self.id);
    }
}

impl AppCore {
    pub fn updates_busy(&self) -> bool {
        !self.updates.lock().unwrap().busy.is_empty()
    }
    pub fn update_views(&self) -> HashMap<String, UpdateView> {
        self.updates.lock().unwrap().items.clone()
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
        view.phase = phase.into();
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
        let _operation = self.update_operation(id)?;
        self.update_phase(id, "checking", None);
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
            client.discover(&source, id, &current)
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
                        ..Default::default()
                    },
                );
                updates.candidates.remove(id);
                if let Some(candidate) = candidate {
                    updates.candidates.insert(id.into(), candidate.clone());
                }
                drop(updates);
                if let Some(candidate) = candidate {
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
        let candidate = self
            .updates
            .lock()
            .unwrap()
            .candidates
            .get(id)
            .cloned()
            .ok_or("请先检查插件更新")?;
        self.update_phase(id, "downloading", None);
        let result = self.prepare_plugin(id, &candidate, client);
        match &result {
            Ok(_) => self.update_phase(id, "ready", None),
            Err(error) => self.update_phase(id, "error", Some(error.clone())),
        }
        result
    }
    fn prepare_plugin(
        &self,
        id: &str,
        candidate: &UpdateManifest,
        client: &ReleaseClient,
    ) -> Result<bool, String> {
        let root = self.store.lock().unwrap().root.clone();
        let archive = root.join(format!("plugin-download-{}.part", self.next_id()));
        let output =
            root.join("plugins")
                .join(id)
                .join(format!("{}-{}", candidate.version, self.next_id()));
        let result = (|| {
            let asset = candidate
                .platforms
                .get(TARGET)
                .ok_or("更新不支持本机平台")?;
            client.download(asset, &archive, |n, total| {
                self.update_progress(id, n, total)
            })?;
            let zip_format = reqwest::Url::parse(&asset.url)
                .map_err(|e| e.to_string())?
                .path()
                .ends_with(".zip");
            let directory = unpack(&archive, &output, zip_format)?;
            let package = plugins::read_manifest(&directory)?;
            if package.manifest.id != id
                || package.manifest.version != candidate.version
                || package.manifest.api_version != candidate.api_version
            {
                return Err("下载包的 ID、版本或 API 与更新清单不一致".into());
            }
            let _mutation = self.mutations.lock().unwrap();
            let active = self.plugin_needs_restart(id);
            let mut store = self.store.lock().unwrap();
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
}

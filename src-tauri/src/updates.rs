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

pub fn open_release_page() -> Result<(), String> {
    let url = "https://github.com/BgaSol/soDeck/releases";
    #[cfg(target_os = "windows")]
    let mut command = {
        use std::os::windows::process::CommandExt;
        let mut c = std::process::Command::new("rundll32.exe");
        c.args(["url.dll,FileProtocolHandler", url])
            .creation_flags(0x08000000);
        c
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut c = std::process::Command::new("open");
        c.arg(url);
        c
    };
    #[cfg(target_os = "linux")]
    let mut command = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(url);
        c
    };
    let mut child = command
        .spawn()
        .map_err(|e| format!("无法打开发行页面：{e}"))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
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
    checking_all: bool,
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
    client: Client,
    api_base: String,
    test_http: bool,
    releases: std::sync::Mutex<HashMap<String, Result<Vec<Release>, String>>>,
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
            releases: std::sync::Mutex::new(HashMap::new()),
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
                .map(|(k, v)| (k.clone(), v.clone()))
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
        let candidate = self
            .updates
            .lock()
            .unwrap()
            .candidates
            .get(id)
            .cloned()
            .ok_or("请先检查插件更新")?;
        self.update_phase(id, "downloading", None);
        let result = self.prepare_plugin(id, &candidate, client, false);
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
        let source = crate::official_plugins::source(id)?;
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

use crate::{model::*, storage, updates::*, AppCore, TARGET};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
#[cfg(target_os = "linux")]
use tauri::Manager;
use tauri_plugin_updater::{Update, UpdaterExt};

#[derive(Default)]
pub struct CoreUpdater {
    update: Mutex<Option<Update>>,
    prepared: Mutex<Option<PathBuf>>,
}
#[derive(Deserialize, Serialize)]
struct CachedCore {
    target: String,
    current_version: String,
    raw_json: serde_json::Value,
}

impl CoreUpdater {
    pub fn restore<R: tauri::Runtime>(
        &self,
        handle: &tauri::AppHandle<R>,
        core: &Arc<AppCore>,
    ) -> Result<(), String> {
        #[cfg(target_os = "linux")]
        if handle.env().appimage.is_none() {
            core.update_phase(CORE_ID, "unsupported", None);
            return Ok(());
        }
        self.restore_cache(handle, core)
    }

    fn restore_cache<R: tauri::Runtime>(
        &self,
        handle: &tauri::AppHandle<R>,
        core: &Arc<AppCore>,
    ) -> Result<(), String> {
        let root = core.snapshot().data_directory.join("updates/core");
        let metadata = root.join("metadata.json");
        if !metadata.exists() {
            return Ok(());
        }
        let _operation = core.update_operation(CORE_ID)?;
        core.update_phase(CORE_ID, "checking", None);
        let result: Result<(), String> = (|| {
            let cached: CachedCore =
                serde_json::from_slice(&fs::read(&metadata).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
            if cached.target != TARGET || cached.current_version != core.snapshot().version {
                self.clear_cache(&root);
                core.update_phase(CORE_ID, "current", None);
                return Ok(());
            }
            let update = handle
                .updater_builder()
                .target(TARGET)
                // The builder requires an endpoint; restore_update does not request it.
                .endpoints(vec![format!(
                    "https://api.github.com/repos/{REPOSITORY}/releases"
                )
                .parse()
                .unwrap()])
                .map_err(|e| e.to_string())?
                .build()
                .map_err(|e| e.to_string())?
                .restore_update(cached.raw_json)
                .map_err(|e| e.to_string())?;
            let Some(update) = update else {
                self.clear_cache(&root);
                core.update_phase(CORE_ID, "current", None);
                return Ok(());
            };
            if update.download_url.scheme() != "https" {
                return Err("核心缓存的下载地址无效，请重新检查更新".into());
            }
            let path = root.join("package.bin");
            core.updates.lock().unwrap().items.insert(
                CORE_ID.into(),
                UpdateView {
                    id: CORE_ID.into(),
                    phase: "preparing".into(),
                    version: Some(update.version.clone()),
                    notes: update.body.clone().unwrap_or_default(),
                    ..Default::default()
                },
            );
            *self.update.lock().unwrap() = Some(update.clone());
            if !path.exists() {
                core.update_phase(CORE_ID, "available", None);
                return Ok(());
            }
            let bytes = read_package(&path)?;
            update
                .verify(&bytes)
                .map_err(|e| format!("缓存包校验失败，请重新下载：{e}"))?;
            core.update_progress(CORE_ID, bytes.len() as u64, Some(bytes.len() as u64));
            *self.prepared.lock().unwrap() = Some(path);
            core.update_phase(CORE_ID, "ready", None);
            Ok(())
        })();
        if let Err(error) = &result {
            core.update_phase(CORE_ID, "error", Some(error.clone()));
        }
        core.changed();
        result
    }
    fn clear_cache(&self, root: &std::path::Path) {
        for name in ["metadata.json", "package.bin", "package.part"] {
            let _ = fs::remove_file(root.join(name));
        }
    }
    pub async fn check(
        &self,
        handle: &tauri::AppHandle,
        core: &Arc<AppCore>,
    ) -> Result<(), String> {
        self.check_with_client(handle, core, Arc::new(ReleaseClient::github()?))
            .await
    }
    pub async fn check_with_client(
        &self,
        handle: &tauri::AppHandle,
        core: &Arc<AppCore>,
        client: Arc<ReleaseClient>,
    ) -> Result<(), String> {
        let _operation = core.update_operation(CORE_ID)?;
        if self.ready() {
            return Ok(());
        }
        core.update_phase(CORE_ID, "checking", None);
        let result = self.discover(handle, core, client).await;
        if let Err(error) = &result {
            core.update_phase(CORE_ID, "error", Some(error.clone()));
        }
        result
    }
    async fn discover(
        &self,
        handle: &tauri::AppHandle,
        core: &Arc<AppCore>,
        client: Arc<ReleaseClient>,
    ) -> Result<(), String> {
        #[cfg(target_os = "linux")]
        if handle.env().appimage.is_none() {
            core.update_phase(CORE_ID, "unsupported", None);
            return Ok(());
        }
        let current = core.snapshot().version;
        let candidate = tauri::async_runtime::spawn_blocking(move || {
            client.discover(
                &UpdateSource {
                    repository: REPOSITORY.into(),
                    tag_prefix: "v".into(),
                },
                CORE_ID,
                &current,
            )
        })
        .await
        .map_err(|e| e.to_string())??;
        let Some(candidate) = candidate else {
            *self.update.lock().unwrap() = None;
            self.clear_cache(&core.snapshot().data_directory.join("updates/core"));
            core.update_phase(CORE_ID, "current", None);
            return Ok(());
        };
        let update = handle
            .updater_builder()
            .target(TARGET)
            .endpoints(vec![candidate
                .manifest_url
                .parse()
                .map_err(|_| "核心更新地址无效")?])
            .map_err(|e| e.to_string())?
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| e.to_string())?
            .check()
            .await
            .map_err(|e| e.to_string())?
            .ok_or("核心发行信息在检查期间发生变化，请重试")?;
        if update.version != candidate.version || update.download_url.scheme() != "https" {
            return Err("核心更新信息不一致或未使用 HTTPS".into());
        }
        *self.update.lock().unwrap() = Some(update);
        let root = core.snapshot().data_directory.join("updates/core");
        self.clear_cache(&root);
        fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let raw_json = self
            .update
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .raw_json
            .clone();
        storage::write_json(
            &root.join("metadata.json"),
            &CachedCore {
                target: TARGET.into(),
                current_version: core.snapshot().version,
                raw_json,
            },
        )?;
        core.updates.lock().unwrap().items.insert(
            CORE_ID.into(),
            UpdateView {
                id: CORE_ID.into(),
                phase: "available".into(),
                version: Some(candidate.version),
                notes: candidate.notes,
                checked_at: Some(crate::core::now()),
                ..Default::default()
            },
        );
        core.changed();
        Ok(())
    }
    pub async fn download(&self, core: &Arc<AppCore>) -> Result<(), String> {
        let operation = core.update_operation(CORE_ID)?;
        self.download_reserved(core, operation).await
    }
    pub async fn download_reserved(
        &self,
        core: &Arc<AppCore>,
        _operation: Operation,
    ) -> Result<(), String> {
        let mut update = self
            .update
            .lock()
            .unwrap()
            .clone()
            .ok_or("请先检查核心更新")?;
        update.timeout = Some(Duration::from_secs(600));
        core.update_phase(CORE_ID, "downloading", None);
        let mut count = 0;
        let mut last = Instant::now();
        let result = update
            .download(
                |chunk, total| {
                    count += chunk as u64;
                    if last.elapsed() >= Duration::from_millis(200) {
                        core.update_progress(CORE_ID, count, total);
                        last = Instant::now();
                    }
                },
                || core.update_phase(CORE_ID, "preparing", None),
            )
            .await;
        let result = match result {
            Ok(bytes) => {
                core.update_progress(CORE_ID, bytes.len() as u64, Some(bytes.len() as u64));
                let root = core.snapshot().data_directory.join("updates/core");
                let cached = CachedCore {
                    target: TARGET.into(),
                    current_version: core.snapshot().version,
                    raw_json: update.raw_json.clone(),
                };
                let saved =
                    tauri::async_runtime::spawn_blocking(move || -> Result<PathBuf, String> {
                        fs::create_dir_all(&root).map_err(|e| e.to_string())?;
                        let path = root.join("package.bin");
                        let temporary = root.join("package.part");
                        let mut file = fs::File::create(&temporary).map_err(|e| e.to_string())?;
                        file.write_all(&bytes)
                            .and_then(|_| file.sync_all())
                            .map_err(|e| e.to_string())?;
                        fs::rename(&temporary, &path).map_err(|e| e.to_string())?;
                        storage::write_json(&root.join("metadata.json"), &cached)?;
                        Ok(path)
                    })
                    .await
                    .map_err(|e| e.to_string())?;
                match saved {
                    Ok(path) => {
                        *self.prepared.lock().unwrap() = Some(path);
                        core.update_phase(CORE_ID, "ready", None);
                        Ok(())
                    }
                    Err(error) => Err(error),
                }
            }
            Err(error) => Err(error.to_string()),
        };
        if let Err(error) = &result {
            core.update_phase(CORE_ID, "error", Some(error.clone()));
        }
        result
    }
    pub fn verify_ready(&self) -> Result<(), String> {
        let update = self
            .update
            .lock()
            .unwrap()
            .clone()
            .ok_or("请先检查核心更新")?;
        let path = self
            .prepared
            .lock()
            .unwrap()
            .clone()
            .ok_or("请先下载核心更新")?;
        update
            .verify(&read_package(&path)?)
            .map_err(|e| format!("缓存包校验失败，请重新下载：{e}"))
    }
    /// Called only after the existing normal shutdown checks and plugin cleanup.
    pub fn install(&self) -> Result<(), String> {
        let update = self
            .update
            .lock()
            .unwrap()
            .clone()
            .ok_or("请先检查核心更新")?;
        let path = self
            .prepared
            .lock()
            .unwrap()
            .clone()
            .ok_or("请先下载核心更新")?;
        update
            .install(read_package(&path)?)
            .map_err(|e| e.to_string())?;
        *self.prepared.lock().unwrap() = None;
        // Keep the disk cache until the next launch confirms the running version changed.
        Ok(())
    }
    pub fn ready(&self) -> bool {
        self.prepared.lock().unwrap().is_some()
    }
    pub fn discard_prepared(&self) {
        *self.prepared.lock().unwrap() = None;
    }
}
fn read_package(path: &std::path::Path) -> Result<Vec<u8>, String> {
    if fs::metadata(path)
        .map_err(|e| format!("无法读取核心缓存：{e}"))?
        .len()
        > 512 * 1024 * 1024
    {
        return Err("核心缓存包过大，请重新下载".into());
    }
    fs::read(path).map_err(|e| format!("无法读取核心缓存：{e}"))
}

#[cfg(all(test, feature = "test-fixtures"))]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static SEQUENCE: AtomicUsize = AtomicUsize::new(0);
    // Public signed "test" vector from tauri-plugin-updater's offline tests, bound to version 2.0.0.
    const PUBLIC_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEE0OUVGNDc0NkZBNkU2RDcKUldUWDVxWnZkUFNlcERHV2cyYno4bjZBclFuQk13Z1FLMWdlVm9zb3JjYkx6Y3VuRCtLSitjWWU=";
    const SIGNATURE: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVUWDVxWnZkUFNlcFBGMnpzanZQL0wvVFF0QkxSbldSdmFxKzRvbUsxVUV1Z3RRcC9VU3JHaUkxdjc2MnNEeTNpZDNhb3I3YlJDOUVUeWVvdDZsTUhIcldvWjVoalVNMVFjPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzkxNjQ5MDUxCWZpbGU6cGF5bG9hZAl2ZXJzaW9uOjIuMC4wCkREcTJDTE9FbDVjUW1DMmVKUkhsblpQVlZpT1pzdTFDanBQS1FvZWk0QS9YQmZrRHFHTzBCK1FNcmNySGdFUTVxVjNtbmU3NXRkRldoSnBxVVhoOUFBPT0=";
    fn app() -> tauri::App<tauri::test::MockRuntime> {
        let mut context = tauri::test::mock_context(tauri::test::noop_assets());
        context
            .config_mut()
            .plugins
            .0
            .insert("updater".into(), json!({"pubkey":PUBLIC_KEY}));
        tauri::test::mock_builder()
            .plugin(tauri_plugin_updater::Builder::new().build())
            .build(context)
            .unwrap()
    }
    #[test]
    fn core_cache_restores_offline_and_rejects_corruption_and_version_tampering() {
        for (payload, version, valid) in [
            (b"test".as_slice(), "2.0.0", true),
            (b"changed".as_slice(), "2.0.0", false),
            (b"test".as_slice(), "3.0.0", false),
        ] {
            let root = std::env::temp_dir().join(format!(
                "sodeck-core-cache-{}-{}",
                std::process::id(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            let core = AppCore::open(root.clone(), "0.2.0".into()).unwrap();
            let cache = root.join("updates/core");
            fs::create_dir_all(&cache).unwrap();
            let metadata = CachedCore {
                target: TARGET.into(),
                current_version: "0.2.0".into(),
                raw_json: json!({"version":version,"notes":"cached notes","platforms":{TARGET:{"url":"https://example.invalid/package","signature":SIGNATURE}}}),
            };
            storage::write_json(&cache.join("metadata.json"), &metadata).unwrap();
            fs::write(cache.join("package.bin"), payload).unwrap();
            drop(core);
            let core = AppCore::open(root.clone(), "0.2.0".into()).unwrap();
            let app = app();
            let updater = CoreUpdater::default();
            let restored = updater.restore_cache(app.handle(), &core);
            assert_eq!(restored.is_ok(), valid, "{restored:?}");
            assert_eq!(updater.ready(), valid);
            if valid {
                assert_eq!(core.update_views()[CORE_ID].phase, "ready");
                updater.verify_ready().unwrap();
                fs::write(cache.join("package.bin"), b"corrupt after restore").unwrap();
                assert!(updater.verify_ready().is_err());
                drop(core);
                let upgraded = AppCore::open(root.clone(), "2.0.0".into()).unwrap();
                let updater = CoreUpdater::default();
                updater.restore_cache(app.handle(), &upgraded).unwrap();
                assert!(!updater.ready());
                assert!(!cache.join("metadata.json").exists());
                assert!(!cache.join("package.bin").exists());
                drop(upgraded);
            } else {
                assert_eq!(
                    core.update_views()[CORE_ID].error_stage.as_deref(),
                    Some("download")
                );
                drop(core);
            }
            fs::remove_dir_all(root).unwrap();
        }
    }
}

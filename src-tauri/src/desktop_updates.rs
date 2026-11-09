use crate::{model::*, storage, updates::*, AppCore, TARGET};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
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
        let client = core.release_client()?.for_component(core, CORE_ID);
        self.check_with_client(handle, core, Arc::new(client)).await
    }
    pub async fn check_with_client(
        &self,
        handle: &tauri::AppHandle,
        core: &Arc<AppCore>,
        client: Arc<ReleaseClient>,
    ) -> Result<(), String> {
        let client = Arc::new(client.for_component(core, CORE_ID));
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
        if core.is_closing() {
            return Err("应用正在退出或重启".into());
        }
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
            .build()
            .map_err(|e| e.to_string())?
            .restore_update(serde_json::to_value(&candidate).map_err(|e| e.to_string())?)
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
        let client = Arc::new(core.release_client()?);
        let operation = core.update_operation(CORE_ID)?;
        self.download_reserved_with_client(core, operation, client)
            .await
    }
    pub async fn download_reserved_with_client(
        &self,
        core: &Arc<AppCore>,
        _operation: Operation,
        client: Arc<ReleaseClient>,
    ) -> Result<(), String> {
        if self.ready() {
            return Ok(());
        }
        let update = self
            .update
            .lock()
            .unwrap()
            .clone()
            .ok_or("请先检查核心更新")?;
        let client = client.for_component(core, CORE_ID);
        core.update_phase(CORE_ID, "downloading", None);
        let copy = core.clone();
        let result = tauri::async_runtime::spawn_blocking(move || -> Result<PathBuf, String> {
            let root = copy.snapshot().data_directory.join("updates/core");
            fs::create_dir_all(&root).map_err(|e| e.to_string())?;
            let temporary = root.join("package.part");
            if temporary.exists() {
                fs::remove_file(&temporary).map_err(|e| e.to_string())?;
            }
            client.download_core(&update, &temporary, |count, total| {
                copy.update_progress(CORE_ID, count, total)
            })?;
            if copy.is_closing() {
                let _ = fs::remove_file(&temporary);
                return Err("应用正在退出或重启".into());
            }
            copy.update_phase(CORE_ID, "preparing", None);
            let cached = CachedCore {
                target: TARGET.into(),
                current_version: copy.snapshot().version,
                raw_json: update.raw_json.clone(),
            };
            let path = root.join("package.bin");
            fs::rename(&temporary, &path).map_err(|e| e.to_string())?;
            if let Err(error) = storage::write_json(&root.join("metadata.json"), &cached) {
                let _ = fs::remove_file(&path);
                return Err(error);
            }
            Ok(path)
        })
        .await
        .map_err(|e| e.to_string())?;
        let result = match result {
            Ok(path) if !core.is_closing() => {
                *self.prepared.lock().unwrap() = Some(path);
                core.update_phase(CORE_ID, "ready", None);
                Ok(())
            }
            Ok(_) => Err("应用正在退出或重启".into()),
            Err(error) => Err(error),
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
    pub fn verify_for_close(&self, core: &AppCore) -> Result<(), String> {
        let result = self.verify_ready();
        if let Err(error) = &result {
            self.discard_prepared();
            core.fail_update(CORE_ID, error.clone());
        }
        result
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
pub(crate) fn read_package(path: &std::path::Path) -> Result<Vec<u8>, String> {
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
        app_with_key(PUBLIC_KEY)
    }
    fn app_with_key(public_key: &str) -> tauri::App<tauri::test::MockRuntime> {
        let mut context = tauri::test::mock_context(tauri::test::noop_assets());
        context
            .config_mut()
            .plugins
            .0
            .insert("updater".into(), json!({"pubkey":public_key}));
        tauri::test::mock_builder()
            .plugin(tauri_plugin_updater::Builder::new().build())
            .build(context)
            .unwrap()
    }
    #[test]
    #[ignore = "Explicit public test downloads and verifies the real core update in isolated data; does not install"]
    fn core_download_live_mirror() {
        let root = std::env::temp_dir().join(format!(
            "sodeck-core-live-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let core = AppCore::open(root.clone(), "0.0.1".into()).unwrap();
        let prefix = std::env::var("SODECK_TEST_MIRROR")
            .expect("Set SODECK_TEST_MIRROR to auto or a single HTTPS prefix");
        let client = Arc::new(if prefix == "auto" {
            core.release_client().unwrap()
        } else {
            ReleaseClient::test_proxy(&prefix).unwrap()
        });
        let candidate = client
            .discover(
                &UpdateSource {
                    repository: REPOSITORY.into(),
                    tag_prefix: "v".into(),
                },
                CORE_ID,
                "0.0.1",
            )
            .unwrap()
            .unwrap();
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let app = app_with_key(config["plugins"]["updater"]["pubkey"].as_str().unwrap());
        let update = app
            .handle()
            .updater_builder()
            .target(TARGET)
            .endpoints(vec![candidate.manifest_url.parse().unwrap()])
            .unwrap()
            .build()
            .unwrap()
            .restore_update(serde_json::to_value(&candidate).unwrap())
            .unwrap()
            .unwrap();
        let updater = CoreUpdater::default();
        *updater.update.lock().unwrap() = Some(update);
        let operation = core.update_operation(CORE_ID).unwrap();
        tauri::async_runtime::block_on(
            updater.download_reserved_with_client(&core, operation, client),
        )
        .unwrap();
        updater.verify_ready().unwrap();
        assert_eq!(core.update_views()[CORE_ID].phase, "ready");
        println!("Core {} {} real package downloaded, verified with configured public key and prepared; installer not run",candidate.version,TARGET);
        for log in core
            .logs(&Default::default())
            .into_iter()
            .filter(|log| log.message == "更新访问线路已切换或失败")
            .take(20)
        {
            if let Some(network) = log.fields.and_then(|fields| fields.get("network").cloned()) {
                println!("Network attempt: {network}");
            }
        }
        drop(core);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn shared_transport_retries_bad_core_signature_and_prepares_only_verified_package() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
            sync::atomic::{AtomicBool, Ordering},
            thread,
            time::Duration,
        };
        let direct = TcpListener::bind("127.0.0.1:0").unwrap();
        let mirror = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", direct.local_addr().unwrap());
        let prefix = format!("http://{}/", mirror.local_addr().unwrap());
        let raw = json!({"id":CORE_ID,"version":"2.0.0","manifestUrl":format!("{base}/core-update.json"),"platforms":{TARGET:{"url":format!("{base}/package.zip"),"signature":SIGNATURE}}});
        let stopped = Arc::new(AtomicBool::new(false));
        let mut workers = Vec::new();
        for (listener, proxy) in [(direct, false), (mirror, true)] {
            listener.set_nonblocking(true).unwrap();
            let stopped = stopped.clone();
            let metadata = raw.to_string();
            workers.push(thread::spawn(move || {
                while !stopped.load(Ordering::Acquire) {
                    let Ok((mut socket, _)) = listener.accept() else {
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    };
                    socket.set_nonblocking(false).unwrap();
                    socket
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let mut bytes = Vec::new();
                    let mut byte = [0];
                    while !bytes.ends_with(b"\r\n\r\n") && bytes.len() < 8192 {
                        if socket.read(&mut byte).unwrap_or(0) == 0 {
                            break;
                        }
                        bytes.push(byte[0]);
                    }
                    let header = String::from_utf8_lossy(&bytes);
                    let body: &[u8] = if header
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or_default()
                        .ends_with("core-update.json")
                    {
                        metadata.as_bytes()
                    } else if proxy {
                        b"test"
                    } else {
                        b"corrupt"
                    };
                    let _ = write!(
                        socket,
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = socket.write_all(body);
                }
            }));
        }
        let root = std::env::temp_dir().join(format!(
            "sodeck-core-network-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let core = AppCore::open(root.clone(), "0.2.0".into()).unwrap();
        let app = app();
        let update = app
            .handle()
            .updater_builder()
            .target(TARGET)
            .endpoints(vec![format!("{base}/core-update.json").parse().unwrap()])
            .unwrap()
            .build()
            .unwrap()
            .restore_update(raw.clone())
            .unwrap()
            .unwrap();
        let updater = CoreUpdater::default();
        *updater.update.lock().unwrap() = Some(update);
        let client = Arc::new(ReleaseClient::test_mirrors(
            &base,
            vec![(prefix, true)],
            Arc::new(Default::default()),
        ));
        let operation = core.update_operation(CORE_ID).unwrap();
        tauri::async_runtime::block_on(
            updater.download_reserved_with_client(&core, operation, client),
        )
        .unwrap();
        updater.verify_ready().unwrap();
        assert_eq!(core.update_views()[CORE_ID].phase, "ready");
        assert_eq!(
            fs::read(root.join("updates/core/package.bin")).unwrap(),
            b"test"
        );
        assert!(!root.join("updates/core/package.part").exists());
        let cached: CachedCore =
            serde_json::from_slice(&fs::read(root.join("updates/core/metadata.json")).unwrap())
                .unwrap();
        assert_eq!(
            cached.raw_json["platforms"][TARGET]["url"],
            raw["platforms"][TARGET]["url"]
        );
        stopped.store(true, Ordering::Release);
        for worker in workers {
            worker.join().unwrap();
        }
        drop(core);
        fs::remove_dir_all(root).unwrap();
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

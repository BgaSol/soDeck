use super::CoreUpdater;
use crate::{updates::*, AppCore};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::sync::OnceCell;
use tokio_util::sync::CancellationToken;

/// Application-owned orchestration; update state and operation reservations stay in AppCore.
pub struct UpdateService {
    core: Arc<AppCore>,
    pub updater: Arc<CoreUpdater>,
    initialized: OnceCell<()>,
    started: AtomicBool,
    stop: CancellationToken,
    #[cfg(feature = "test-fixtures")]
    test_client: Option<Arc<ReleaseClient>>,
}

impl UpdateService {
    pub fn new(core: Arc<AppCore>) -> Self {
        Self {
            core,
            updater: Arc::new(CoreUpdater::default()),
            initialized: OnceCell::new(),
            started: AtomicBool::new(false),
            stop: CancellationToken::new(),
            #[cfg(feature = "test-fixtures")]
            test_client: None,
        }
    }

    #[cfg(feature = "test-fixtures")]
    pub fn with_test_client(core: Arc<AppCore>, client: ReleaseClient) -> Self {
        let mut service = Self::new(core);
        service.test_client = Some(Arc::new(client));
        service
    }

    fn client(&self) -> Result<Arc<ReleaseClient>, String> {
        #[cfg(feature = "test-fixtures")]
        if let Some(client) = &self.test_client {
            return Ok(client.clone());
        }
        self.core.release_client().map(Arc::new)
    }

    async fn initialize<R: tauri::Runtime>(&self, handle: &tauri::AppHandle<R>) {
        self.initialized
            .get_or_init(|| async {
                let updater = self.updater.clone();
                let core = self.core.clone();
                let handle = handle.clone();
                let result =
                    tauri::async_runtime::spawn_blocking(move || updater.restore(&handle, &core))
                        .await
                        .map_err(|e| e.to_string())
                        .and_then(|v| v);
                if let Err(error) = result {
                    self.core.fail_check(CORE_ID, error);
                }
            })
            .await;
    }

    pub fn start<R: tauri::Runtime>(self: &Arc<Self>, handle: tauri::AppHandle<R>) {
        self.start_with_timing(handle, Duration::from_secs(10), Duration::from_secs(60));
    }

    pub(crate) fn start_with_timing<R: tauri::Runtime>(
        self: &Arc<Self>,
        handle: tauri::AppHandle<R>,
        delay: Duration,
        interval: Duration,
    ) {
        if self.started.swap(true, Ordering::AcqRel) {
            return;
        }
        let weak = Arc::downgrade(self);
        let stop = self.stop.clone();
        tauri::async_runtime::spawn(async move {
            if let Some(service) = weak.upgrade() {
                service.initialize(&handle).await;
            }
            let mut wait = delay;
            loop {
                tokio::select! {
                    _ = stop.cancelled() => break,
                    _ = tokio::time::sleep(wait) => {}
                }
                let Some(service) = weak.upgrade() else {
                    break;
                };
                // Closing is reversible until the native finalizer actually exits.
                let _ = service.check(&handle, None, true).await;
                wait = interval;
            }
        });
    }

    pub fn stop(&self) {
        self.stop.cancel();
    }

    pub fn initialized(&self) -> bool {
        self.started.load(Ordering::Acquire) && self.initialized.get().is_some()
    }

    pub async fn check<R: tauri::Runtime>(
        &self,
        handle: &tauri::AppHandle<R>,
        id: Option<String>,
        automatic: bool,
    ) -> Result<(), String> {
        self.initialize(handle).await;
        let batch = if id.is_some() {
            self.core.begin_component_check()
        } else {
            self.core.begin_update_check(automatic)
        };
        let Some(mut batch) = batch else {
            return if automatic {
                Ok(())
            } else {
                Err("已有更新任务在处理中，或应用正在退出，请稍后重试".into())
            };
        };
        let client = match self.client() {
            Ok(client) => client,
            Err(error) => {
                self.core
                    .log(None, "error", &format!("无法开始更新检查：{error}"), None);
                batch.finish(Some(error.clone()));
                return Err(error);
            }
        };
        let single = id.is_some();
        let ids = id
            .map(|id| vec![id])
            .unwrap_or_else(|| self.core.update_check_ids());
        let mut failure = None;
        for id in ids {
            let result = if id == CORE_ID {
                self.updater
                    .check_with_client(handle, &self.core, client.clone())
                    .await
            } else {
                let core = self.core.clone();
                let client = client.clone();
                let id = id.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    core.check_component(&id, &client).map(|_| ())
                })
                .await
                .map_err(|e| e.to_string())
                .and_then(|v| v)
            };
            match result {
                Ok(()) => {
                    if self
                        .core
                        .update_views()
                        .get(&id)
                        .is_some_and(|v| ["current", "available"].contains(&v.phase.as_str()))
                    {
                        batch.succeeded();
                    }
                }
                Err(error) => {
                    self.core.fail_check(&id, error.clone());
                    batch.failed();
                    failure = Some(error);
                }
            }
        }
        batch.finish(None);
        if single {
            failure.map_or(Ok(()), Err)
        } else {
            Ok(())
        }
    }

    pub async fn download<R: tauri::Runtime>(
        &self,
        handle: &tauri::AppHandle<R>,
        ids: Vec<String>,
    ) -> Result<(), String> {
        self.initialize(handle).await;
        let client = self.client()?;
        let mut tasks = std::collections::VecDeque::from(self.core.reserve_update_downloads(&ids)?);
        while let Some(first) = tasks.pop_front() {
            let second = tasks.pop_front();
            tokio::join!(self.download_reserved(first, client.clone()), async {
                if let Some(task) = second {
                    self.download_reserved(task, client.clone()).await;
                }
            });
        }
        Ok(())
    }

    async fn download_reserved(
        &self,
        (id, operation): (String, Operation),
        client: Arc<ReleaseClient>,
    ) {
        let result = if id == CORE_ID {
            self.updater
                .download_reserved_with_client(&self.core, operation, client)
                .await
        } else {
            let core = self.core.clone();
            let plugin_id = id.clone();
            tauri::async_runtime::spawn_blocking(move || {
                core.download_plugin_reserved(&plugin_id, &client, operation)
                    .map(|_| ())
            })
            .await
            .map_err(|e| e.to_string())
            .and_then(|v| v)
        };
        if let Err(error) = result {
            self.core.fail_update(&id, error);
        }
    }
}

impl Drop for UpdateService {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

#[cfg(all(test, feature = "test-fixtures"))]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{fs, path::PathBuf, sync::atomic::AtomicUsize};
    static SEQUENCE: AtomicUsize = AtomicUsize::new(0);

    fn fixture() -> (PathBuf, Arc<AppCore>, Arc<UpdateService>) {
        let root = std::env::temp_dir().join(format!(
            "sodeck-auto-update-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let plugin = root.join("plugin");
        fs::create_dir_all(&plugin).unwrap();
        fs::write(plugin.join("entry"), b"not executed").unwrap();
        fs::write(plugin.join("plugin.json"), json!({"id":"test.auto","name":"Auto fixture","version":"0.1.0","apiVersion":"2.0","target":crate::TARGET,"entry":"entry","updateSource":{"repository":"test/repo","tagPrefix":"plugin-auto-v"}}).to_string()).unwrap();
        let core = AppCore::open(root.join("data"), "2.0.0".into()).unwrap();
        core.add_plugin(&plugin).unwrap();
        core.set_enabled("test.auto", false).unwrap();
        let manifests = [
            json!({"id":CORE_ID,"version":"2.0.0","source":{"repository":REPOSITORY,"tagPrefix":"v"}}),
            json!({"id":"test.auto","version":"0.1.1","source":{"repository":"test/repo","tagPrefix":"plugin-auto-v"}}),
        ].into_iter().map(|mut value| {
            value["apiVersion"] = json!("2.0");
            value["manifestUrl"] = json!("https://example.com/update.json");
            value["platforms"] = json!({(crate::TARGET):{"url":"https://example.com/package.zip","sha256":"a".repeat(64)}});
            serde_json::from_value(value).unwrap()
        }).collect();
        let client = ReleaseClient::test_server("http://127.0.0.1:1").test_pin(manifests);
        let service = Arc::new(UpdateService::with_test_client(core.clone(), client));
        (root, core, service)
    }

    fn app() -> tauri::App<tauri::test::MockRuntime> {
        let mut context = tauri::test::mock_context(tauri::test::noop_assets());
        context
            .config_mut()
            .plugins
            .0
            .insert("updater".into(), json!({"pubkey":"test-discovery-only"}));
        tauri::test::mock_builder()
            .plugin(tauri_plugin_updater::Builder::new().build())
            .build(context)
            .unwrap()
    }

    #[test]
    fn automatic_service_starts_without_frontend_and_resumes_after_close_is_cancelled() {
        let (root, core, service) = fixture();
        let app = app();
        core.set_closing(true);
        service.start_with_timing(
            app.handle().clone(),
            Duration::from_millis(5),
            Duration::from_millis(10),
        );
        service.start_with_timing(
            app.handle().clone(),
            Duration::from_millis(5),
            Duration::from_millis(10),
        );
        tauri::async_runtime::block_on(async {
            tokio::time::sleep(Duration::from_millis(40)).await;
            assert_eq!(core.update_views()["test.auto"].phase, "idle");
            core.set_closing(false);
            tokio::time::timeout(Duration::from_secs(3), async {
                while {
                    let state = core.update_snapshot();
                    state.items["test.auto"].phase != "available" || state.overview.checking
                } {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
            assert!(!core.update_overview().checking);
        });
        assert_eq!(core.messages().len(), 1);
        assert!(!core.snapshot().plugins[0].enabled);
        service.stop();
        drop(service);
        drop(core);
        drop(app);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn disabled_automatic_checks_allow_manual_checks_and_initialization_failure_retries() {
        let (root, core, service) = fixture();
        let app = app();
        core.set_automatic_updates(false).unwrap();
        tauri::async_runtime::block_on(async {
            service.check(app.handle(), None, true).await.unwrap();
            assert_eq!(core.update_views()["test.auto"].phase, "idle");
            service
                .check(app.handle(), Some("test.auto".into()), false)
                .await
                .unwrap();
            assert_eq!(core.update_views()["test.auto"].phase, "available");
            assert_eq!(core.update_overview().next_check_at, 0);
            core.set_automatic_updates(true).unwrap();
            let mut invalid = UpdateService::new(core.clone());
            invalid.test_client = None;
            core.store
                .lock()
                .unwrap()
                .preferences
                .update_network
                .custom_mirrors
                .push(crate::update_network::UpdateMirror {
                    name: "invalid".into(),
                    prefix: "http://invalid.example/".into(),
                    enabled: true,
                });
            assert!(invalid.check(app.handle(), None, true).await.is_err());
            let state = core.update_overview();
            assert!(!state.checking && state.error.is_some());
            assert!(state.next_check_at < crate::core::now() + 16 * 60 * 1000);
            core.store
                .lock()
                .unwrap()
                .preferences
                .update_network
                .custom_mirrors
                .clear();
            service.check(app.handle(), None, false).await.unwrap();
            assert!(core.update_overview().error.is_none());
        });
        drop(service);
        drop(core);
        drop(app);
        let _ = fs::remove_dir_all(root);
    }
}

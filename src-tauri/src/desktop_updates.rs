use crate::{model::*, updates::*, AppCore, TARGET};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
#[cfg(target_os = "linux")]
use tauri::Manager;
use tauri_plugin_updater::{Update, UpdaterExt};

#[derive(Default)]
pub struct CoreUpdater {
    update: Mutex<Option<Update>>,
    downloaded: Mutex<Option<Vec<u8>>>,
}

impl CoreUpdater {
    pub async fn check(
        &self,
        handle: &tauri::AppHandle,
        core: &Arc<AppCore>,
    ) -> Result<(), String> {
        let _operation = core.update_operation(CORE_ID)?;
        core.update_phase(CORE_ID, "checking", None);
        let result = self.discover(handle, core).await;
        if let Err(error) = &result {
            core.update_phase(CORE_ID, "error", Some(error.clone()));
        }
        result
    }

    async fn discover(&self, handle: &tauri::AppHandle, core: &Arc<AppCore>) -> Result<(), String> {
        #[cfg(target_os = "linux")]
        if handle.env().appimage.is_none() {
            core.update_phase(
                CORE_ID,
                "unsupported",
                Some(
                    "DEB 安装请使用系统包管理器升级。需要应用内升级时，请使用同架构的 AppImage。"
                        .into(),
                ),
            );
            return Ok(());
        }
        let current = core.snapshot().version;
        let candidate = tauri::async_runtime::spawn_blocking(move || {
            ReleaseClient::github()?.discover(
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
        // Do not discard a downloaded update if a later check fails.
        let Some(candidate) = candidate else {
            *self.update.lock().unwrap() = None;
            *self.downloaded.lock().unwrap() = None;
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
        *self.downloaded.lock().unwrap() = None;
        core.updates.lock().unwrap().items.insert(
            CORE_ID.into(),
            UpdateView {
                id: CORE_ID.into(),
                phase: "available".into(),
                version: Some(candidate.version.clone()),
                notes: candidate.notes.clone(),
                ..Default::default()
            },
        );
        let _ = core.publish(
            None,
            PublishMessage {
                title: format!("SoDeck {} 可更新", candidate.version),
                body: candidate.notes,
                level: "info".into(),
                notify: false,
                dedupe_key: Some(format!("update:core:{}", candidate.version)),
            },
        );
        core.changed();
        Ok(())
    }

    pub async fn download(&self, core: &Arc<AppCore>) -> Result<(), String> {
        let _operation = core.update_operation(CORE_ID)?;
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
                || {},
            )
            .await;
        match result {
            Ok(bytes) => {
                core.update_progress(CORE_ID, bytes.len() as u64, Some(bytes.len() as u64));
                *self.downloaded.lock().unwrap() = Some(bytes);
                core.update_phase(CORE_ID, "ready", None);
                Ok(())
            }
            Err(error) => {
                core.update_phase(CORE_ID, "error", Some(error.to_string()));
                Err(error.to_string())
            }
        }
    }

    /// The caller must stop plugins and prevent another exit/restart before calling this.
    pub fn install(&self) -> Result<(), String> {
        let update = self
            .update
            .lock()
            .unwrap()
            .clone()
            .ok_or("请先检查并下载核心更新")?;
        let bytes = self
            .downloaded
            .lock()
            .unwrap()
            .take()
            .ok_or("请先下载核心更新")?;
        if let Err(error) = update.install(&bytes) {
            *self.downloaded.lock().unwrap() = Some(bytes);
            return Err(error.to_string());
        }
        Ok(())
    }

    pub fn ready(&self) -> bool {
        self.downloaded.lock().unwrap().is_some()
    }
}

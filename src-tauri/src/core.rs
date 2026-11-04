use crate::{
    model::*,
    plugins::{self, Instance},
    storage::{self, Store},
    TARGET,
};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

type Listener = Arc<dyn Fn(&str, Value) + Send + Sync>;

pub struct AppCore {
    pub(crate) store: Mutex<Store>,
    pub(crate) updates: Mutex<crate::updates::UpdateState>,
    pub(crate) mutations: Mutex<()>,
    startup_enabled: HashSet<String>,
    instances: Mutex<HashMap<String, Arc<Instance>>>,
    listener: Mutex<Option<Listener>>,
    version: String,
    started_at: u64,
    started: Instant,
    sequence: AtomicU64,
    closing: AtomicBool,
    restart_after_error: AtomicBool,
    log_rate: Mutex<(Instant, usize, u64)>,
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

impl AppCore {
    pub fn open(directory: PathBuf, version: String) -> Result<Arc<Self>, String> {
        let store = Store::open(directory)?;
        let startup_enabled = store
            .plugins
            .iter()
            .filter(|p| p.enabled)
            .map(|p| p.manifest.id.clone())
            .collect();
        let core = Arc::new(Self {
            store: Mutex::new(store),
            updates: Mutex::new(crate::updates::UpdateState::default()),
            mutations: Mutex::new(()),
            startup_enabled,
            instances: Mutex::new(HashMap::new()),
            listener: Mutex::new(None),
            version,
            started_at: now(),
            started: Instant::now(),
            sequence: AtomicU64::new(0),
            closing: AtomicBool::new(false),
            restart_after_error: AtomicBool::new(false),
            log_rate: Mutex::new((Instant::now(), 0, 0)),
        });
        core.log(None, "info", "SoDeck 已启动", None);
        Ok(core)
    }

    pub fn set_listener(&self, listener: Listener) {
        *self.listener.lock().unwrap() = Some(listener);
    }

    pub(crate) fn event(&self, name: &str, payload: Value) {
        let listener = self.listener.lock().unwrap().clone();
        if let Some(listener) = listener {
            listener(name, payload);
        }
    }

    pub(crate) fn changed(&self) {
        self.event("core-changed", json!(null));
    }

    pub(crate) fn next_id(&self) -> String {
        format!(
            "{}-{}-{}",
            self.started_at,
            std::process::id(),
            self.sequence.fetch_add(1, Ordering::Relaxed)
        )
    }

    pub fn snapshot(&self) -> Snapshot {
        let store = self.store.lock().unwrap();
        let records = store.plugins.clone();
        let mut snapshot = Snapshot {
            version: self.version.clone(),
            target: TARGET.into(),
            os: std::env::consts::OS.into(),
            arch: std::env::consts::ARCH.into(),
            started_at: self.started_at,
            uptime_ms: self.started.elapsed().as_millis() as u64,
            data_directory: store.root.clone(),
            log_directory: store.root.join("logs"),
            preferences: store.preferences.clone(),
            plugins: Vec::new(),
            unread_count: store.messages.iter().filter(|m| !m.read).count(),
            storage_errors: store.errors.iter().flatten().cloned().collect(),
            restart_required: self.restart_after_error.load(Ordering::Acquire),
        };
        drop(store);
        let instances = self.instances.lock().unwrap();
        for record in records {
            let update_source = record.manifest.update_source.clone();
            let mut view = instances
                .get(&record.manifest.id)
                .map(|i| i.view())
                .unwrap_or_else(|| {
                    let missing = !record.directory.is_dir();
                    PluginView {
                        directory: record.directory,
                        manifest: record.manifest,
                        status: if missing {
                            PluginStatus::Failed
                        } else {
                            PluginStatus::Unloaded
                        },
                        instance_id: None,
                        pid: None,
                        started_at: None,
                        error: missing.then(|| "插件目录不存在，请重新选择目录".into()),
                        cleanup_complete: true,
                        enabled: record.enabled,
                        pending_restart: false,
                        pending_version: None,
                        update_error: record.update_error.clone(),
                    }
                });
            view.enabled = record.enabled;
            view.manifest.update_source = update_source;
            view.pending_restart = record.enabled
                != self.startup_enabled.contains(&view.manifest.id)
                || record.pending_update.is_some();
            view.pending_version = record
                .pending_update
                .as_ref()
                .map(|p| p.manifest.version.clone());
            view.update_error = record.update_error;
            snapshot.restart_required |= view.pending_restart;
            snapshot.plugins.push(view);
        }
        snapshot
            .plugins
            .sort_by(|a, b| a.manifest.name.cmp(&b.manifest.name));
        snapshot
    }

    pub fn load(self: &Arc<Self>, directory: &Path) -> Result<PluginView, String> {
        if self.closing.load(Ordering::Acquire) {
            return Err("应用正在退出".into());
        }
        let mut record = plugins::read_manifest(directory)?;
        if let Some(saved) = self
            .store
            .lock()
            .unwrap()
            .plugins
            .iter()
            .find(|p| p.manifest.id == record.manifest.id)
        {
            record.enabled = saved.enabled;
            record.pending_update = saved.pending_update.clone();
            record.previous = saved.previous.clone();
            record.update_error = saved.update_error.clone();
            record.manifest.update_source = saved
                .manifest
                .update_source
                .clone()
                .or(record.manifest.update_source);
        }
        let id = record.manifest.id.clone();
        let (instance, control, ready, ready_rx) = Instance::new(record.clone(), self.next_id());
        {
            let mut instances = self.instances.lock().unwrap();
            if self.closing.load(Ordering::Acquire) {
                return Err("应用正在退出".into());
            }
            if let Some(existing) = instances.get(&id) {
                let view = existing.view();
                if !view.cleanup_complete {
                    return Err(format!(
                        "插件已在加载、运行或停止中：{}",
                        view.directory.display()
                    ));
                }
            }
            self.store.lock().unwrap().register(record)?;
            instances.insert(id, instance.clone());
        }
        self.changed();
        let core = self.clone();
        let running = instance.clone();
        std::thread::spawn(move || plugins::supervise(core, running, control, ready));
        ready_rx
            .recv_timeout(Duration::from_secs(7))
            .map_err(|_| "等待插件初始化结果超时，请查看插件状态与日志".to_string())??;
        Ok(instance.view())
    }

    pub fn unload(&self, id: &str) -> Result<PluginView, String> {
        let instance = self
            .instances
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or("插件未运行")?;
        if instance.view().status == PluginStatus::Stopping {
            return Err("插件正在卸载，请等待完成".into());
        }
        let receiver = instance.stop()?;
        self.changed();
        receiver
            .recv_timeout(Duration::from_millis(6000))
            .map_err(|_| "停止尚未完成，请查看实际运行状态并重试停止".to_string())??;
        Ok(instance.view())
    }

    pub fn forget(&self, id: &str) -> Result<(), String> {
        let _operation = self.mutations.lock().unwrap();
        if self
            .store
            .lock()
            .unwrap()
            .plugins
            .iter()
            .any(|p| p.manifest.id == id && p.enabled)
        {
            return Err("请先关闭插件并重启，再移除记录".into());
        }
        let mut instances = self.instances.lock().unwrap();
        if instances
            .get(id)
            .is_some_and(|i| !i.view().cleanup_complete)
        {
            return Err("请先卸载插件再移除记录".into());
        }
        self.store.lock().unwrap().forget(id)?;
        instances.remove(id);
        drop(instances);
        self.changed();
        Ok(())
    }

    pub fn shutdown(&self) -> Result<(), String> {
        self.closing.store(true, Ordering::Release);
        let instances: Vec<_> = self.instances.lock().unwrap().values().cloned().collect();
        let pending: Vec<_> = instances
            .iter()
            .filter(|i| !i.view().cleanup_complete)
            .map(|i| i.stop())
            .collect();
        let deadline = Instant::now() + Duration::from_millis(6000);
        let mut errors = Vec::new();
        for pending in pending {
            match pending {
                Ok(receiver) => match receiver
                    .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                {
                    Ok(Ok(())) => (),
                    Ok(Err(error)) => errors.push(error),
                    Err(_) => errors.push("插件退出超时，请重试关闭".into()),
                },
                Err(error) => errors.push(error),
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            self.closing.store(false, Ordering::Release);
            Err(errors.join("；"))
        }
    }

    pub fn add_plugin(&self, directory: &Path) -> Result<String, String> {
        let _operation = self.mutations.lock().unwrap();
        let mut record = plugins::read_manifest(directory)?;
        let id = record.manifest.id.clone();
        let mut store = self.store.lock().unwrap();
        if store.plugins.iter().any(|p| p.manifest.id == id) {
            return Err("该插件已有记录，请在详情中管理或移除旧记录后再添加".into());
        }
        record.enabled = true;
        store.register(record)?;
        drop(store);
        self.changed();
        Ok(id)
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), String> {
        let _operation = self.mutations.lock().unwrap();
        let mut store = self.store.lock().unwrap();
        let mut record = store
            .plugins
            .iter()
            .find(|p| p.manifest.id == id)
            .cloned()
            .ok_or("插件记录不存在")?;
        record.enabled = enabled;
        store.register(record)?;
        drop(store);
        self.changed();
        Ok(())
    }

    pub fn set_update_source(&self, id: &str, source: Option<UpdateSource>) -> Result<(), String> {
        if let Some(source) = &source {
            crate::updates::validate_source(source)?;
        }
        let _operation = self.mutations.lock().unwrap();
        let mut store = self.store.lock().unwrap();
        let mut record = store
            .plugins
            .iter()
            .find(|p| p.manifest.id == id)
            .cloned()
            .ok_or("插件记录不存在")?;
        record.manifest.update_source = source;
        store.register(record)?;
        drop(store);
        self.updates.lock().unwrap().items.remove(id);
        self.changed();
        Ok(())
    }

    /// Called once during application startup, before any UI-triggered plugin operations.
    pub fn start_enabled(self: &Arc<Self>) {
        let _operation = self.mutations.lock().unwrap();
        let records = self.store.lock().unwrap().plugins.clone();
        for mut record in records {
            if let Some(pending) = record.pending_update.take() {
                let checked = plugins::read_manifest(&pending.directory).and_then(|candidate| {
                    if candidate.manifest.id != record.manifest.id
                        || candidate.manifest.version != pending.manifest.version
                    {
                        return Err("待更新插件的 ID 或版本已改变".into());
                    }
                    Ok(())
                });
                match checked {
                    Ok(()) => {
                        record.previous = Some(PluginPackage {
                            directory: record.directory.clone(),
                            manifest: record.manifest.clone(),
                        });
                        record.directory = pending.directory;
                        record.manifest = pending.manifest;
                        record.update_error = None;
                    }
                    Err(error) => {
                        record.update_error = Some(error);
                    }
                }
                let saved = self.store.lock().unwrap().register(record.clone());
                if let Err(error) = saved {
                    self.log(None, "error", &error, None);
                    continue;
                }
            }
            if !record.enabled {
                continue;
            }
            match self.load(&record.directory) {
                Ok(_) => {
                    if record.previous.take().is_some() {
                        let saved = self.store.lock().unwrap().register(record.clone());
                        if let Err(error) = saved {
                            self.log(None, "error", &error, None);
                        }
                    }
                }
                Err(error) => {
                    if let Some(previous) = record.previous.take() {
                        // Only restore binaries; plugin data belongs to the plugin and is kept.
                        record.directory = previous.directory;
                        record.manifest = previous.manifest;
                        record.update_error = Some(format!("新版启动失败，已恢复旧程序：{error}"));
                        let saved = self.store.lock().unwrap().register(record.clone());
                        if saved.is_ok() {
                            let _ = self.load(&record.directory);
                        }
                    } else {
                        record.update_error = Some(error.clone());
                        let _ = self.store.lock().unwrap().register(record.clone());
                        self.log(
                            None,
                            "error",
                            &format!("插件 {} 启动失败：{error}", record.manifest.id),
                            None,
                        );
                    }
                }
            }
        }
        self.changed();
    }

    pub(crate) fn plugin_needs_restart(&self, id: &str) -> bool {
        self.startup_enabled.contains(id)
            || self
                .instances
                .lock()
                .unwrap()
                .get(id)
                .is_some_and(|p| !p.view().cleanup_complete)
    }

    pub fn resume_after_failed_install(&self) {
        self.closing.store(false, Ordering::Release);
        self.restart_after_error.store(true, Ordering::Release);
        self.changed();
    }

    pub fn set_theme(&self, theme: &str) -> Result<(), String> {
        let result = self.store.lock().unwrap().set_theme(theme);
        self.changed();
        result
    }

    pub fn logs(&self, filter: &LogFilter) -> Vec<LogRecord> {
        let store = self.store.lock().unwrap();
        let root = store.root.clone();
        let fallback: Vec<_> = store.recent.iter().cloned().collect();
        drop(store);
        storage::query_logs(&root, &fallback, filter, 500)
    }

    pub fn messages(&self) -> Vec<Message> {
        let mut messages = self.store.lock().unwrap().messages.clone();
        messages.reverse();
        messages
    }

    pub fn mark_read(&self, id: Option<&str>) -> Result<(), String> {
        let result = self.store.lock().unwrap().mark_read(id);
        self.changed();
        result
    }

    pub(crate) fn plugin_data(&self, id: &str) -> Result<PathBuf, String> {
        let path = self.store.lock().unwrap().root.join("plugin-data").join(id);
        std::fs::create_dir_all(&path).map_err(|error| format!("无法创建插件数据目录：{error}"))?;
        Ok(path)
    }

    pub(crate) fn log(
        &self,
        source: Option<(&str, &str)>,
        level: &str,
        message: &str,
        fields: Option<Value>,
    ) {
        self.write_log(source, "core", level, message, fields);
    }

    fn write_log(
        &self,
        source: Option<(&str, &str)>,
        origin: &str,
        level: &str,
        message: &str,
        fields: Option<Value>,
    ) {
        let record = LogRecord {
            id: self.next_id(),
            received_at: now(),
            source: origin.into(),
            plugin_id: source.map(|s| s.0.into()),
            instance_id: source.map(|s| s.1.into()),
            level: level.into(),
            message: message.into(),
            fields,
        };
        self.store.lock().unwrap().append_log(record);
        self.changed();
    }

    pub(crate) fn plugin_log(
        &self,
        id: &str,
        instance: &str,
        level: &str,
        message: &str,
        fields: Option<Value>,
    ) -> Result<bool, String> {
        if !["debug", "info", "warn", "error"].contains(&level) {
            return Err("无效日志级别".into());
        }
        let mut rate = self.log_rate.lock().unwrap();
        let mut dropped = 0;
        if rate.0.elapsed() >= Duration::from_secs(1) {
            dropped = rate.2;
            *rate = (Instant::now(), 0, 0);
        }
        let accepted = rate.1 < 200;
        if accepted {
            rate.1 += 1;
        } else {
            rate.2 += 1;
        }
        drop(rate);
        if dropped > 0 {
            self.log(
                None,
                "warn",
                "插件日志输出过快，已合并丢弃记录",
                Some(json!({"droppedCount":dropped})),
            );
        }
        if accepted {
            self.write_log(Some((id, instance)), "plugin", level, message, fields);
        }
        Ok(accepted)
    }

    pub(crate) fn publish(
        &self,
        source: Option<(&str, &str)>,
        input: PublishMessage,
    ) -> Result<Message, String> {
        if input.title.trim().is_empty() || input.title.len() > 1024 || input.body.len() > 16 * 1024
        {
            return Err("消息标题不能为空，标题或正文过长".into());
        }
        if !["info", "success", "warning", "error"].contains(&input.level.as_str()) {
            return Err("无效消息级别".into());
        }
        let message = Message {
            id: self.next_id(),
            plugin_id: source.map(|s| s.0.into()),
            instance_id: source.map(|s| s.1.into()),
            source: if source.is_some() { "plugin" } else { "core" }.into(),
            received_at: now(),
            title: input.title,
            body: input.body,
            level: input.level,
            read: false,
            count: 1,
            dedupe_key: input.dedupe_key,
        };
        let result = self.store.lock().unwrap().publish(message);
        self.changed();
        let (saved, fresh) = result?;
        if input.notify && fresh {
            self.event("core-message", serde_json::to_value(&saved).unwrap());
        }
        Ok(saved)
    }

    pub(crate) fn plugin_failure(&self, id: &str, instance: &str, reason: &str) {
        self.log(Some((id, instance)), "error", reason, None);
        let input = PublishMessage {
            title: format!("插件运行失败：{id}"),
            body: reason.into(),
            level: "error".into(),
            notify: true,
            dedupe_key: Some(format!("plugin-failed:{id}")),
        };
        // Lifecycle failures originate in the core, with the plugin association attached by it.
        let message = Message {
            id: self.next_id(),
            plugin_id: Some(id.into()),
            instance_id: Some(instance.into()),
            source: "core".into(),
            received_at: now(),
            title: input.title,
            body: input.body,
            level: input.level,
            read: false,
            count: 1,
            dedupe_key: input.dedupe_key,
        };
        if let Ok((message, fresh)) = self.store.lock().unwrap().publish(message) {
            if fresh {
                self.event("core-message", serde_json::to_value(message).unwrap());
            }
        }
        self.changed();
    }
}

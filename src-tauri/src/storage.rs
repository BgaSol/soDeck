use crate::model::*;
use serde::{de::DeserializeOwned, Serialize};
use std::{
    collections::{HashSet, VecDeque},
    fs::{self, File, OpenOptions},
    io::{BufReader, Write},
    path::{Path, PathBuf},
};

const LOG_BYTES: u64 = 10 * 1024 * 1024;
const LOG_FILES: usize = 5;
const MESSAGE_LIMIT: usize = 1000;

pub struct Store {
    pub root: PathBuf,
    pub plugins: Vec<PluginRecord>,
    pub messages: Vec<Message>,
    pub preferences: Preferences,
    pub errors: [Option<String>; 4],
    pub recent: VecDeque<LogRecord>,
    _lock: File,
    blocked: [bool; 3],
}

fn read_json<T: DeserializeOwned + Default>(path: &Path) -> (T, Option<String>) {
    match fs::read(path) {
        Ok(bytes) => match serde_json::from_slice(&bytes) {
            Ok(value) => (value, None),
            Err(error) => (
                T::default(),
                Some(format!(
                    "配置无法读取，原文件已保留：{}：{error}",
                    path.display()
                )),
            ),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (T::default(), None),
        Err(error) => (
            T::default(),
            Some(format!("无法读取 {}：{error}", path.display())),
        ),
    }
}

pub fn write_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let temporary = path.with_extension("json.tmp");
    let result = (|| -> std::io::Result<()> {
        let mut file = File::create(&temporary)?;
        serde_json::to_writer_pretty(&mut file, value)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::rename(&temporary, path)
    })();
    result.map_err(|error| format!("无法保存 {}：{error}", path.display()))
}

impl Store {
    pub fn open(root: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&root).map_err(|error| format!("无法创建应用数据目录：{error}"))?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join("app.lock"))
            .map_err(|error| format!("无法打开应用数据锁：{error}"))?;
        // Tauri spawns the replacement just before the old process exits during restart.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while lock.try_lock().is_err() {
            if std::time::Instant::now() >= deadline {
                return Err("应用数据正在被另一个 SoDeck 实例使用，请先关闭它。".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        let (plugins, plugin_error) = read_json(&root.join("plugins.json"));
        let (messages, message_error) = read_json(&root.join("messages.json"));
        let (preferences, preference_error) = read_json(&root.join("preferences.json"));
        let blocked = [
            plugin_error.is_some(),
            message_error.is_some(),
            preference_error.is_some(),
        ];
        let mut store = Self {
            root,
            plugins,
            messages,
            preferences,
            errors: [plugin_error, message_error, preference_error, None],
            recent: VecDeque::new(),
            _lock: lock,
            blocked,
        };
        if let Err(error) = fs::create_dir_all(store.root.join("logs")) {
            store.errors[3] = Some(format!("无法创建日志目录：{error}"));
        }
        // Retire the two former demonstration packages before startup/update selection.
        // As with removing a plugin record, leave external programs and user data alone.
        let mut plugins = store.plugins.clone();
        plugins.retain(|p| !matches!(p.manifest.id.as_str(), "sodeck.hello" | "sodeck.ui-example"));
        if plugins.len() != store.plugins.len() {
            store.save(0, "plugins.json", &plugins)?;
            store.plugins = plugins;
        }
        store.recent = query_logs(&store.root, &[], &LogFilter::default(), 5000).into();
        Ok(store)
    }

    pub fn register(&mut self, record: PluginRecord) -> Result<(), String> {
        if self.blocked[0] {
            return Err(self.errors[0].clone().unwrap());
        }
        let mut records = self.plugins.clone();
        records.retain(|item| item.manifest.id != record.manifest.id);
        records.push(record);
        self.save(0, "plugins.json", &records)?;
        self.plugins = records;
        Ok(())
    }

    pub fn forget(&mut self, id: &str) -> Result<(), String> {
        if self.blocked[0] {
            return Err(self.errors[0].clone().unwrap());
        }
        let records: Vec<_> = self
            .plugins
            .iter()
            .filter(|r| r.manifest.id != id)
            .cloned()
            .collect();
        self.save(0, "plugins.json", &records)?;
        self.plugins = records;
        Ok(())
    }

    pub fn set_theme(&mut self, theme: &str) -> Result<(), String> {
        if !["system", "light", "dark", "dopamine"].contains(&theme) {
            return Err("未知的主题设置".into());
        }
        if self.blocked[2] {
            return Err(self.errors[2].clone().unwrap());
        }
        let mut preferences = self.preferences.clone();
        preferences.theme = theme.into();
        self.save(2, "preferences.json", &preferences)?;
        self.preferences = preferences;
        Ok(())
    }

    pub fn set_automatic_updates(&mut self, enabled: bool) -> Result<(), String> {
        if self.blocked[2] {
            return Err(self.errors[2].clone().unwrap());
        }
        let mut preferences = self.preferences.clone();
        preferences.automatic_updates = enabled;
        self.save(2, "preferences.json", &preferences)?;
        self.preferences = preferences;
        Ok(())
    }

    pub fn set_update_network(
        &mut self,
        expected_revision: u64,
        input: crate::update_network::Preferences,
    ) -> Result<crate::update_network::Preferences, String> {
        if self.blocked[2] {
            return Err(self.errors[2].clone().unwrap());
        }
        if self.preferences.update_network.revision != expected_revision {
            return Err("网络设置已在另一窗口修改，请重新读取后再保存。当前输入仍保留。".into());
        }
        let mut input = input.validated()?;
        input.revision = expected_revision
            .checked_add(1)
            .ok_or("网络设置修订号无效")?;
        let mut preferences = self.preferences.clone();
        preferences.update_network = input.clone();
        self.save(2, "preferences.json", &preferences)?;
        self.preferences = preferences;
        Ok(input)
    }

    pub fn append_log(&mut self, mut record: LogRecord) {
        if record.message.len() > 6000 {
            let mut end = 6000;
            while !record.message.is_char_boundary(end) {
                end -= 1;
            }
            record.message.truncate(end);
            record.message.push_str("…[已截断]");
        }
        if serde_json::to_vec(&record).is_ok_and(|v| v.len() > 8192) {
            record.fields = None;
        }
        while serde_json::to_vec(&record).is_ok_and(|v| v.len() > 8192) {
            let mut end = record.message.len() / 2;
            while !record.message.is_char_boundary(end) {
                end -= 1;
            }
            record.message.truncate(end);
            record.message.push_str("…[已截断]");
        }
        if let Err(error) = self.write_log(&record) {
            self.errors[3] = Some(format!("日志保存失败，近期记录暂存于内存：{error}"));
        } else {
            self.errors[3] = None;
        }
        self.recent.push_back(record);
        while self.recent.len() > 5000 {
            self.recent.pop_front();
        }
    }

    fn write_log(&self, record: &LogRecord) -> std::io::Result<()> {
        let directory = self.root.join("logs");
        fs::create_dir_all(&directory)?;
        let path = directory.join("current.jsonl");
        let mut encoded = serde_json::to_vec(record)?;
        encoded.push(b'\n');
        if fs::metadata(&path).is_ok_and(|m| m.len() + encoded.len() as u64 > LOG_BYTES) {
            let last = directory.join(format!("{}.jsonl", LOG_FILES - 1));
            if last.exists() {
                fs::remove_file(last)?;
            }
            for index in (1..LOG_FILES - 1).rev() {
                let old = directory.join(format!("{index}.jsonl"));
                if old.exists() {
                    fs::rename(old, directory.join(format!("{}.jsonl", index + 1)))?;
                }
            }
            fs::rename(&path, directory.join("1.jsonl"))?;
        }
        let mut file = OpenOptions::new().append(true).create(true).open(path)?;
        file.write_all(&encoded)
    }

    pub fn publish(&mut self, message: Message) -> Result<(Message, bool), String> {
        if self.blocked[1] {
            return Err(self.errors[1].clone().unwrap());
        }
        let mut messages = self.messages.clone();
        let duplicate = message
            .dedupe_key
            .as_ref()
            .filter(|key| !key.is_empty())
            .and_then(|key| {
                messages.iter().position(|old| {
                    old.source == message.source
                        && old.plugin_id == message.plugin_id
                        && old.dedupe_key.as_ref() == Some(key)
                        && message.received_at.saturating_sub(old.received_at) <= 300_000
                })
            });
        let mut saved = message;
        if let Some(index) = duplicate {
            let old = messages.remove(index);
            saved.id = old.id;
            saved.count = old.count.saturating_add(1);
        }
        messages.push(saved.clone());
        if messages.len() > MESSAGE_LIMIT {
            messages.drain(..messages.len() - MESSAGE_LIMIT);
        }
        self.save(1, "messages.json", &messages)?;
        self.messages = messages;
        Ok((saved, duplicate.is_none()))
    }

    pub fn mark_read(&mut self, id: Option<&str>) -> Result<(), String> {
        if self.blocked[1] {
            return Err(self.errors[1].clone().unwrap());
        }
        let mut messages = self.messages.clone();
        for message in &mut messages {
            if id.is_none_or(|id| message.id == id) {
                message.read = true;
            }
        }
        self.save(1, "messages.json", &messages)?;
        self.messages = messages;
        Ok(())
    }

    fn save(&mut self, index: usize, file: &str, value: &impl Serialize) -> Result<(), String> {
        let result = write_json(&self.root.join(file), value);
        self.errors[index] = result.as_ref().err().cloned();
        result
    }
}

pub fn query_logs(
    root: &Path,
    fallback: &[LogRecord],
    filter: &LogFilter,
    limit: usize,
) -> Vec<LogRecord> {
    let mut found = VecDeque::new();
    let mut seen = HashSet::new();
    let mut paths: Vec<_> = (1..LOG_FILES)
        .rev()
        .map(|i| root.join(format!("logs/{i}.jsonl")))
        .collect();
    paths.push(root.join("logs/current.jsonl"));
    for path in paths {
        let Ok(file) = File::open(path) else { continue };
        let mut reader = BufReader::new(file);
        loop {
            let Ok(Some(bytes)) = crate::plugins::read_frame(&mut reader, 16 * 1024) else {
                break;
            };
            let Ok(record) = serde_json::from_slice::<LogRecord>(&bytes) else {
                continue;
            };
            if filter.matches(&record) {
                seen.insert(record.id.clone());
                found.push_back(record);
                while found.len() > limit {
                    found.pop_front();
                }
            }
        }
    }
    for record in fallback {
        if !seen.contains(&record.id) && filter.matches(record) {
            found.push_back(record.clone());
        }
    }
    let mut result: Vec<_> = found.into();
    result.sort_by_key(|r| r.received_at);
    if result.len() > limit {
        result.drain(..result.len() - limit);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_query_stops_only_the_oversized_file_and_skips_short_invalid_json() {
        let root =
            std::env::temp_dir().join(format!("sodeck-log-query-test-{}", std::process::id()));
        fs::create_dir_all(root.join("logs")).unwrap();
        let record = |id: &str, received_at| LogRecord {
            id: id.into(),
            received_at,
            source: "core".into(),
            plugin_id: None,
            instance_id: None,
            level: "info".into(),
            message: id.into(),
            fields: None,
        };
        let before = record("before", 1);
        let after = record("after", 2);
        let current = record("current", 3);
        let fallback = record("fallback", 4);
        let mut rotated = serde_json::to_vec(&before).unwrap();
        rotated.push(b'\n');
        rotated.extend(vec![b'x'; 16 * 1024]);
        rotated.push(b'\n');
        rotated.extend(serde_json::to_vec(&after).unwrap());
        rotated.push(b'\n');
        fs::write(root.join("logs/1.jsonl"), rotated).unwrap();
        let mut active = b"invalid json\n".to_vec();
        active.extend(serde_json::to_vec(&current).unwrap());
        active.push(b'\n');
        fs::write(root.join("logs/current.jsonl"), active).unwrap();

        let found = query_logs(&root, &[before, fallback], &LogFilter::default(), 10);
        assert_eq!(
            found
                .iter()
                .map(|record| record.id.as_str())
                .collect::<Vec<_>>(),
            ["before", "current", "fallback"]
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn logs_bound_escaped_messages_and_rotate_files() {
        let root = std::env::temp_dir().join(format!("sodeck-log-test-{}", std::process::id()));
        let mut store = Store::open(root.clone()).unwrap();
        let record = LogRecord {
            id: "escaped".into(),
            received_at: 1,
            source: "core".into(),
            plugin_id: None,
            instance_id: None,
            level: "info".into(),
            message: "\0".repeat(6000),
            fields: None,
        };
        store.append_log(record);
        assert!(fs::metadata(root.join("logs/current.jsonl")).unwrap().len() <= 8193);
        for index in 0..6 {
            let file = OpenOptions::new()
                .write(true)
                .open(root.join("logs/current.jsonl"))
                .unwrap();
            file.set_len(LOG_BYTES).unwrap();
            drop(file);
            let mut record = store.recent[0].clone();
            record.id = index.to_string();
            record.message = "next".into();
            store.append_log(record);
        }
        assert_eq!(fs::read_dir(root.join("logs")).unwrap().count(), 5);
        assert!(store.errors[3].is_none());
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_save_reports_error_and_can_recover() {
        let root = std::env::temp_dir().join(format!("sodeck-save-test-{}", std::process::id()));
        let mut store = Store::open(root.clone()).unwrap();
        fs::create_dir(root.join("preferences.json")).unwrap();
        assert!(store.set_theme("dark").is_err());
        assert!(store.errors[2].is_some());
        assert_eq!(store.preferences.theme, "system");
        fs::remove_dir(root.join("preferences.json")).unwrap();
        store.set_theme("dark").unwrap();
        assert!(store.errors[2].is_none());
        assert!(
            Store::open(root.clone()).is_err(),
            "second writer must not open the same data"
        );
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }
}

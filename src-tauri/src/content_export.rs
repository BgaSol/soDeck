//! User-chosen local byte exports; content interpretation belongs to the plugin.
use crate::{resource_owner::Owner, resource_tasks::Tasks, AppCore};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Instant, SystemTime},
};
struct Output {
    state: &'static str,
    error: Option<String>,
    offset: u64,
    file: Option<File>,
    temporary: Option<PathBuf>,
    target: Option<PathBuf>,
    before: Option<(u64, Option<SystemTime>)>,
    last: Option<(u64, Vec<u8>)>,
    touched: Instant,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn waiting() -> Output {
        Output {
            state: "waitingInput",
            error: None,
            offset: 0,
            file: None,
            temporary: None,
            target: None,
            before: None,
            last: None,
            touched: Instant::now(),
        }
    }
    #[test]
    fn late_picker_does_not_restart_an_expired_export() {
        let directory = std::env::temp_dir().join(format!(
            "sodeck-late-picker-{}-{}",
            std::process::id(),
            crate::core::now()
        ));
        let core = AppCore::open(directory.clone(), "test".into()).unwrap();
        let mut output = waiting();
        output.touched -= std::time::Duration::from_secs(301);
        let export = Arc::new(Export {
            owner: Owner {
                plugin: "test".into(),
                instance: "test".into(),
                view: "test".into(),
                context: crate::workspaces::Context {
                    workspace_id: "test".into(),
                    server_id: "test".into(),
                    connection_id: "test".into(),
                    generation: 1,
                },
            },
            surface: "test".into(),
            request: "test".into(),
            length: 1,
            name: "output.txt".into(),
            output: Mutex::new(output),
            tasks: Arc::new(Tasks::default()),
        });
        core.content_exports
            .0
            .lock()
            .unwrap()
            .insert("test".into(), export.clone());
        let target = directory.join("output.txt");
        core.content_export_choose("test", Ok(Some(target.clone())));
        let output = export.output.lock().unwrap();
        assert_eq!(output.state, "failed");
        assert!(output.error.as_ref().unwrap().contains("超时"));
        assert!(output.temporary.is_none());
        assert!(!target.exists());
        drop(output);
        core.shutdown().unwrap();
        drop(core);
        std::fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn write_failure_keeps_cleanup_error_and_path_for_drop_retry() {
        let directory = std::env::temp_dir().join(format!(
            "sodeck-cleanup-failure-{}-{}",
            std::process::id(),
            crate::core::now()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let temporary = directory.join("temporary.part");
        // A directory at the registered file path forces remove_file to fail on every target.
        std::fs::create_dir(&temporary).unwrap();
        let mut output = waiting();
        output.state = "receiving";
        output.temporary = Some(temporary.clone());
        fail(&mut output, "写入失败".into());
        assert_eq!(output.state, "failed");
        assert!(output
            .error
            .as_ref()
            .unwrap()
            .contains("写入失败；临时导出文件清理失败"));
        assert_eq!(output.temporary.as_ref(), Some(&temporary));
        std::fs::remove_dir(&temporary).unwrap();
        std::fs::write(&temporary, b"partial").unwrap();
        drop(output);
        assert!(!temporary.exists());
        std::fs::remove_dir(directory).unwrap();
    }
    #[test]
    fn idle_exports_expire_without_touching_the_destination_or_completed_result() {
        let directory = std::env::temp_dir().join(format!(
            "sodeck-export-expiry-{}-{}",
            std::process::id(),
            crate::core::now()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let target = directory.join("original.txt");
        std::fs::write(&target, b"original").unwrap();
        for state in ["waitingInput", "receiving", "completed"] {
            let temporary = directory.join(format!("{state}.part"));
            let file = File::create(&temporary).unwrap();
            let mut output = Output {
                state,
                error: None,
                offset: 0,
                file: Some(file),
                temporary: Some(temporary.clone()),
                target: Some(target.clone()),
                before: metadata(&target),
                last: None,
                touched: Instant::now() - std::time::Duration::from_secs(301),
            };
            expire(&mut output);
            if state == "completed" {
                assert_eq!(output.state, "completed");
                assert!(output.error.is_none());
            } else {
                assert_eq!(output.state, "failed");
                assert!(!temporary.exists());
            }
            assert_eq!(std::fs::read(&target).unwrap(), b"original");
        }
        std::fs::remove_dir_all(directory).unwrap();
    }
}
struct Export {
    owner: Owner,
    surface: String,
    request: String,
    length: u64,
    name: String,
    output: Mutex<Output>,
    tasks: Arc<Tasks>,
}
impl Drop for Output {
    fn drop(&mut self) {
        self.file.take();
        if let Some(p) = self.temporary.take() {
            let _ = std::fs::remove_file(p);
        }
    }
}
#[derive(Default)]
pub(crate) struct Exports(Mutex<HashMap<String, Arc<Export>>>);
fn active(o: &Output) -> bool {
    matches!(o.state, "waitingInput" | "receiving" | "committing")
}
fn metadata(p: &PathBuf) -> Option<(u64, Option<SystemTime>)> {
    std::fs::metadata(p)
        .ok()
        .map(|m| (m.len(), m.modified().ok()))
}
fn cancel(o: &mut Output) {
    if !active(o) {
        return;
    }
    o.file.take();
    if let Some(p) = o.temporary.as_ref() {
        if let Err(e) = std::fs::remove_file(&p) {
            o.error = Some(format!("临时导出文件清理失败：{e}"));
            o.state = "failed";
            return;
        }
        o.temporary = None;
    }
    o.state = "cancelled";
}
fn fail(o: &mut Output, error: String) {
    cancel(o);
    let cleanup = o.error.take();
    o.state = "failed";
    o.error = Some(match cleanup {
        Some(cleanup) => format!("{error}；{cleanup}"),
        None => error,
    });
}
fn value(o: &Output) -> Value {
    json!({"state":o.state,"error":o.error,"receivedBytes":o.offset.to_string()})
}
fn expire(o: &mut Output) {
    let deadline = if o.state == "waitingInput" { 300 } else { 60 };
    if active(o) && o.touched.elapsed().as_secs() > deadline {
        fail(o, "导出等待超时，请重新导出".into());
    }
}
impl Exports {
    pub(crate) fn cancel(&self, matches: impl Fn(&Owner) -> bool) {
        for e in self
            .0
            .lock()
            .unwrap()
            .values()
            .filter(|e| matches(&e.owner))
        {
            cancel(&mut e.output.lock().unwrap());
            e.tasks.close();
        }
    }
    pub(crate) fn pending(&self, matches: impl Fn(&Owner) -> bool) -> usize {
        self.0
            .lock()
            .unwrap()
            .values()
            .filter(|e| matches(&e.owner) && e.tasks.active() > 0)
            .count()
    }
}
impl AppCore {
    pub fn content_export_begin(
        &self,
        window: &str,
        token: &str,
        p: &Value,
    ) -> Result<Value, String> {
        let view = self.ui_data_writable(window, token)?;
        let _admission = self.surface_write_admission(window, token)?;
        let request = p["requestId"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 128)
            .ok_or("导出请求ID无效")?;
        let length = p["byteLength"]
            .as_str()
            .ok_or("缺少byteLength")?
            .parse::<u64>()
            .map_err(|_| "导出长度无效")?;
        let name = p["name"]
            .as_str()
            .filter(|s| {
                !s.is_empty()
                    && s.len() <= 256
                    && !s.contains(['/', '\\'])
                    && !s.chars().any(char::is_control)
            })
            .ok_or("文件名无效")?;
        if length > 128 * 1024 * 1024 {
            return Err("导出超过128MiB，请缩小范围".into());
        }
        let mut entries = self.content_exports.0.lock().unwrap();
        entries.retain(|_, e| {
            let mut o = e.output.lock().unwrap();
            expire(&mut o);
            active(&o) || o.touched.elapsed().as_secs() < 300
        });
        for (id, e) in entries.iter() {
            if e.owner.view == view.id && e.surface == token && e.request == request {
                if e.length != length || e.name != name {
                    return Err("同一导出请求参数不同".into());
                }
                return Ok(
                    json!({"id":id,"started":false,"status":value(&e.output.lock().unwrap())}),
                );
            }
        }
        if entries.len() >= 64
            || entries
                .values()
                .filter(|e| active(&e.output.lock().unwrap()))
                .count()
                >= 4
            || entries
                .values()
                .any(|e| e.owner.view == view.id && active(&e.output.lock().unwrap()))
        {
            return Err("导出资源已满".into());
        }
        let id = self.next_id();
        let e = Arc::new(Export {
            owner: Owner {
                view: view.id,
                plugin: view.plugin_id,
                instance: view.plugin_instance_id,
                context: view.context,
            },
            surface: token.into(),
            request: request.into(),
            length,
            name: name.into(),
            tasks: Arc::new(Tasks::default()),
            output: Mutex::new(Output {
                state: "waitingInput",
                error: None,
                offset: 0,
                file: None,
                temporary: None,
                target: None,
                before: None,
                last: None,
                touched: Instant::now(),
            }),
        });
        entries.insert(id.clone(), e);
        Ok(json!({"id":id,"started":true,"status":{"state":"waitingInput"}}))
    }
    pub fn content_export_choose(&self, id: &str, chosen: Result<Option<PathBuf>, String>) {
        let Some(e) = self.content_exports.0.lock().unwrap().get(id).cloned() else {
            return;
        };
        let mut o = e.output.lock().unwrap();
        expire(&mut o);
        if o.state != "waitingInput" {
            return;
        }
        let result = (|| {
            let Some(target) = chosen? else {
                cancel(&mut o);
                return Ok(());
            };
            let temp = target
                .parent()
                .ok_or("目标没有父目录")?
                .join(format!(".sodeck-{}.part", self.next_id()));
            let file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temp)
                .map_err(|e| e.to_string())?;
            o.before = metadata(&target);
            o.target = Some(target);
            o.temporary = Some(temp);
            o.file = Some(file);
            o.state = "receiving";
            o.touched = Instant::now();
            Ok::<_, String>(())
        })();
        if let Err(error) = result {
            o.state = "failed";
            o.error = Some(error)
        }
    }
    pub async fn content_export_data(
        self: &Arc<Self>,
        window: &str,
        token: &str,
        p: Value,
    ) -> Result<Value, String> {
        let view = self.ui_data_view(window, token)?;
        let id = p["id"].as_str().ok_or("缺少导出ID")?;
        let e = self
            .content_exports
            .0
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or("导出已释放")?;
        if e.owner.view != view.id
            || e.owner.instance != view.plugin_instance_id
            || e.surface != token
        {
            return Err("导出不属于此页面".into());
        }
        if p["action"] == "status" {
            let mut o = e.output.lock().unwrap();
            expire(&mut o);
            return Ok(value(&o));
        }
        drop(self.surface_write_admission(window, token)?);
        let task = e.tasks.enter()?;
        let core = self.clone();
        let window = window.to_owned();
        let token = token.to_owned();
        tauri::async_runtime::spawn_blocking(move || {
            let _task = task;
            let _admission = core.surface_write_admission(&window, &token)?;
            let mut o = e.output.lock().unwrap();
            expire(&mut o);
            if p["action"] == "cancel" {
                cancel(&mut o);
                return Ok(value(&o));
            }
            let result = (|| {
                if p["action"] == "append" {
                    if o.state != "receiving" {
                        return Err("导出尚不可写入".into());
                    }
                    let offset = p["offset"]
                        .as_str()
                        .ok_or("缺少offset")?
                        .parse::<u64>()
                        .map_err(|_| "偏移无效")?;
                    if !p["data"]
                        .as_array()
                        .is_some_and(|a| !a.is_empty() && a.len() <= 16 * 1024)
                    {
                        return Err("导出块必须为1–16KiB".into());
                    }
                    let bytes: Vec<u8> =
                        serde_json::from_value(p["data"].clone()).map_err(|_| "字节无效")?;
                    if bytes.len() > 16 * 1024 || bytes.is_empty() {
                        return Err("导出块必须为1–16KiB".into());
                    }
                    if offset < o.offset {
                        if o.last.as_ref() == Some(&(offset, bytes.clone())) {
                            return Ok(());
                        }
                        return Err("导出块冲突".into());
                    }
                    if offset != o.offset || offset + bytes.len() as u64 > e.length {
                        return Err("导出偏移或长度无效".into());
                    }
                    o.file
                        .as_mut()
                        .ok_or("临时文件已关闭")?
                        .write_all(&bytes)
                        .map_err(|e| e.to_string())?;
                    o.offset += bytes.len() as u64;
                    o.last = Some((offset, bytes));
                    o.touched = Instant::now();
                } else if p["action"] == "finish" {
                    if o.state == "completed" {
                        return Ok(());
                    }
                    if o.state != "receiving" || o.offset != e.length {
                        return Err("导出内容尚未完整接收".into());
                    }
                    o.state = "committing";
                    o.file
                        .as_mut()
                        .ok_or("临时文件已关闭")?
                        .sync_all()
                        .map_err(|e| e.to_string())?;
                    o.file.take();
                    let target = o.target.as_ref().unwrap();
                    if metadata(target) != o.before {
                        return Err("目标文件已变化，未覆盖".into());
                    }
                    std::fs::rename(o.temporary.as_ref().unwrap(), target)
                        .map_err(|e| e.to_string())?;
                    o.temporary = None;
                    o.state = "completed";
                    o.last = None;
                } else {
                    return Err("未知导出操作".into());
                }
                Ok::<_, String>(())
            })();
            if let Err(error) = result {
                fail(&mut o, error);
            }
            Ok(value(&o))
        })
        .await
        .map_err(|e| e.to_string())?
    }
}

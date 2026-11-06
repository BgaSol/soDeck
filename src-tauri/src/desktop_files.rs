//! Native local-file inputs, distinct from the terminal's text clipboard.
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::Mutex,
    time::{Duration, Instant},
};
use tauri::Emitter;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

#[derive(Default)]
pub struct DesktopFiles {
    drops: Mutex<HashMap<String, (String, Vec<PathBuf>, Instant)>>,
}
impl DesktopFiles {
    pub fn event(&self, window: &tauri::Webview, event: &tauri::DragDropEvent) {
        let (kind, paths, position) = match event {
            tauri::DragDropEvent::Enter { paths, position } => {
                ("enter", Some(paths), Some(position))
            }
            tauri::DragDropEvent::Over { position } => ("over", None, Some(position)),
            tauri::DragDropEvent::Drop { paths, position } => ("drop", Some(paths), Some(position)),
            tauri::DragDropEvent::Leave => ("leave", None, None),
            _ => return,
        };
        let mut drops = self.drops.lock().unwrap();
        drops.retain(|_, (_, _, at)| at.elapsed() < Duration::from_secs(60));
        if kind == "leave" {
            drops.remove(window.label());
        }
        if let Some(paths) = paths {
            if paths.len() > 4096 {
                return;
            }
            let id = format!("{}-{}", std::process::id(), crate::core::now());
            drops.insert(window.label().into(), (id, paths.clone(), Instant::now()));
        }
        let id = drops.get(window.label()).map(|v| v.0.clone());
        let payload =
            json!({"type":kind,"dropId":id,"position":position.map(|p|json!({"x":p.x,"y":p.y}))});
        drop(drops);
        let _ = window.emit("core-file-drop", payload);
    }
    pub fn claim(&self, window: &str, id: &str) -> Result<Vec<PathBuf>, String> {
        let mut drops = self.drops.lock().unwrap();
        if !drops
            .get(window)
            .is_some_and(|v| v.0 == id && v.2.elapsed() < Duration::from_secs(60))
        {
            return Err("拖入来源已失效，请重试".into());
        }
        Ok(drops.remove(window).unwrap().1)
    }
}
pub struct Clipboard {
    pub paths: Vec<PathBuf>,
    pub revision: String,
    pub has_files: bool,
}
pub async fn reveal_directory(handle: tauri::AppHandle, path: PathBuf) -> Result<(), String> {
    #[cfg(windows)]
    {
        let _ = handle;
        tauri::async_runtime::spawn_blocking(move || windows::reveal(path))
            .await
            .map_err(|e| e.to_string())?
    }
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        handle
            .run_on_main_thread(move || {
                #[cfg(target_os = "macos")]
                let result = macos::reveal(path);
                #[cfg(target_os = "linux")]
                let result = linux::reveal(path);
                let _ = sender.send(result);
            })
            .map_err(|e| e.to_string())?;
        receiver.await.map_err(|e| e.to_string())?
    }
}
pub async fn clipboard(handle: tauri::AppHandle, files: bool) -> Result<Clipboard, String> {
    #[cfg(windows)]
    {
        let _ = handle;
        tauri::async_runtime::spawn_blocking(move || windows::read(files))
            .await
            .map_err(|e| e.to_string())?
    }
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        handle
            .run_on_main_thread(move || {
                #[cfg(target_os = "macos")]
                {
                    let _ = sender.send(macos::read(files));
                }
                #[cfg(target_os = "linux")]
                linux::read(files, sender);
            })
            .map_err(|e| e.to_string())?;
        tokio::time::timeout(Duration::from_secs(5), receiver)
            .await
            .map_err(|_| "读取文件剪贴板超时".to_string())?
            .map_err(|_| "文件剪贴板读取已结束".to_string())?
    }
}
/// Strictly parse actual URI-list clipboard formats, never arbitrary plain text paths.
#[cfg(any(target_os = "linux", test))]
pub(crate) fn uri_paths(data: &[u8]) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::new();
    for raw in data.split(|b| *b == b'\n') {
        let line = raw.strip_suffix(b"\r").unwrap_or(raw);
        if line.is_empty() || line.starts_with(b"#") || line == b"copy" || line == b"cut" {
            continue;
        }
        let rest = line
            .strip_prefix(b"file://")
            .ok_or("剪贴板包含非本地文件 URL")?;
        let path = if rest.starts_with(b"/") {
            rest
        } else {
            rest.strip_prefix(b"localhost")
                .filter(|s| s.starts_with(b"/"))
                .ok_or("剪贴板包含非本地文件来源")?
        };
        if path.contains(&0) {
            return Err("文件 URL 含 NUL".into());
        }
        let mut decoded = Vec::new();
        let mut i = 0;
        while i < path.len() {
            if path[i] == b'%' {
                if i + 2 >= path.len() {
                    return Err("文件 URL 编码无效".into());
                }
                let digits =
                    std::str::from_utf8(&path[i + 1..i + 3]).map_err(|_| "文件 URL 编码无效")?;
                let b = u8::from_str_radix(digits, 16).map_err(|_| "文件 URL 编码无效")?;
                if b == 0 {
                    return Err("文件 URL 含 NUL".into());
                }
                decoded.push(b);
                i += 3;
            } else {
                decoded.push(path[i]);
                i += 1;
            }
        }
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            paths.push(PathBuf::from(std::ffi::OsString::from_vec(decoded)));
        }
        #[cfg(windows)]
        {
            let text = String::from_utf8(decoded).map_err(|_| "文件 URL 编码无效")?;
            paths.push(PathBuf::from(text.trim_start_matches('/')));
        }
        if paths.len() > 4096 {
            return Err("复制条目超过 4096 项".into());
        }
    }
    Ok(paths)
}
pub fn clipboard_info(value: &Clipboard) -> Value {
    json!({"revision":value.revision,"count":value.paths.len(),"hasFiles":value.has_files})
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn uri_list_is_file_only_and_preserves_encoded_separators() {
        let paths = uri_paths(
            b"copy\n#ignore\nfile:///tmp/a%20b%25.txt\r\nfile:///tmp/%E4%B8%AD%E6%96%87\n",
        )
        .unwrap();
        assert_eq!(paths.len(), 2);
        assert!(paths[0].to_string_lossy().ends_with("a b%.txt"));
        assert!(uri_paths(b"https://example.com/file").is_err());
        assert!(uri_paths(b"file://other/tmp/a").is_err());
        assert!(uri_paths(b"file:///tmp/%00").is_err());
        assert!(uri_paths(b"file:///tmp/%xx").is_err());
    }
}

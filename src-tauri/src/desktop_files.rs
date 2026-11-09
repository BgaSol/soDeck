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
#[cfg(any(target_os = "linux", test))]
mod requests;
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
    crate::desktop_open::directory(handle, path).await
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
        #[cfg(target_os = "linux")]
        let id = requests::next_id();
        #[cfg(target_os = "linux")]
        let _cancel = CancelRead {
            handle: handle.clone(),
            id,
        };
        handle
            .run_on_main_thread(move || {
                if sender.is_closed() {
                    return;
                }
                #[cfg(target_os = "macos")]
                {
                    let _ = sender.send(macos::read(files));
                }
                #[cfg(target_os = "linux")]
                linux::read(id, files, sender);
            })
            .map_err(|e| e.to_string())?;
        tokio::time::timeout(Duration::from_secs(5), receiver)
            .await
            .map_err(|_| "读取文件剪贴板超时".to_string())?
            .map_err(|_| "文件剪贴板读取已结束".to_string())?
    }
}

#[cfg(target_os = "linux")]
struct CancelRead {
    handle: tauri::AppHandle,
    id: u64,
}
#[cfg(target_os = "linux")]
impl Drop for CancelRead {
    fn drop(&mut self) {
        let id = self.id;
        let _ = self.handle.run_on_main_thread(move || linux::cancel(id));
    }
}

/// Called by the application finalizer before the UI event loop ends.
pub fn shutdown(handle: &tauri::AppHandle) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let (sender, receiver) = std::sync::mpsc::channel();
        handle
            .run_on_main_thread(move || {
                linux::shutdown();
                let _ = sender.send(());
            })
            .map_err(|e| e.to_string())?;
        receiver
            .recv_timeout(Duration::from_secs(5))
            .map_err(|e| format!("文件剪贴板收尾失败：{e}"))?;
    }
    #[cfg(not(target_os = "linux"))]
    let _ = handle;
    Ok(())
}
/// Strictly parse actual URI-list clipboard formats, never arbitrary plain text paths.
#[cfg(any(target_os = "linux", target_os = "macos", test))]
pub(crate) fn uri_paths(data: &[u8]) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::new();
    for raw in data.split(|b| *b == b'\n') {
        let line = raw.strip_suffix(b"\r").unwrap_or(raw);
        if line.is_empty() || line.starts_with(b"#") || line == b"copy" || line == b"cut" {
            continue;
        }
        let line = std::str::from_utf8(line).map_err(|_| "文件 URL 编码无效")?;
        // Check before URL normalization can discard an invalid path segment.
        if line.contains('\0') || line.contains("%00") {
            return Err("文件 URL 含 NUL".into());
        }
        // URL parsing is forgiving by default. Keep malformed escapes and
        // ignored whitespace invalid without maintaining our own decoder.
        let invalid = std::cell::Cell::new(false);
        let url = reqwest::Url::options()
            .syntax_violation_callback(Some(&|_| invalid.set(true)))
            .parse(line)
            .map_err(|_| "文件 URL 无效")?;
        if invalid.get() {
            return Err("文件 URL 编码无效".into());
        }
        if url.scheme() != "file" {
            return Err("剪贴板包含非本地文件 URL".into());
        }
        if url.host_str().is_some_and(|host| host != "localhost") {
            return Err("剪贴板包含非本地文件来源".into());
        }
        if url.query().is_some() || url.fragment().is_some() {
            return Err("文件 URL 不应包含查询或片段".into());
        }
        let path = url.to_file_path().map_err(|_| "无法读取本地文件 URL")?;
        paths.push(path);
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
    #[cfg(windows)]
    const FILE_ROOT: &str = "file:///C:/tmp";
    #[cfg(unix)]
    const FILE_ROOT: &str = "file:///tmp";

    #[test]
    fn file_uri_scheme_and_localhost_are_case_insensitive() {
        let local = format!("{FILE_ROOT}/a%20b%25.txt");
        let expected = uri_paths(local.as_bytes()).unwrap();
        let upper_scheme = local.replacen("file:", "FILE:", 1);
        let upper_host = local.replacen("file:///", "file://LOCALHOST/", 1);
        assert_eq!(uri_paths(upper_scheme.as_bytes()).unwrap(), expected);
        assert_eq!(uri_paths(upper_host.as_bytes()).unwrap(), expected);
    }

    #[test]
    fn uri_list_is_file_only_and_preserves_encoded_separators() {
        let input =
            format!("copy\n#ignore\n{FILE_ROOT}/a%20b%25.txt\r\n{FILE_ROOT}/%E4%B8%AD%E6%96%87\n");
        let paths = uri_paths(input.as_bytes()).unwrap();
        assert_eq!(paths.len(), 2);
        assert!(paths[0].to_string_lossy().ends_with("a b%.txt"));
        assert!(paths[1].to_string_lossy().ends_with("中文"));
        assert!(uri_paths(b"https://example.com/file").is_err());
        assert!(uri_paths(b"file://other/tmp/a").is_err());
        assert!(uri_paths(b"file:///tmp/%00").is_err());
        assert!(uri_paths(b"file:///tmp/%xx").is_err());
    }

    #[test]
    fn file_uri_encoding_does_not_lose_or_double_decode_file_names() {
        let input = format!(
            "{FILE_ROOT}/a%2520%23%3F.txt\n{FILE_ROOT}/中文\n{FILE_ROOT}/%E4%B8%AD%E6%96%87"
        );
        let paths = uri_paths(input.as_bytes()).unwrap();
        assert!(paths[0].to_string_lossy().ends_with("a%20#?.txt"));
        assert_eq!(paths[1], paths[2]);
        let mut invalid_utf8 = format!("{FILE_ROOT}/").into_bytes();
        invalid_utf8.push(0xff);
        assert!(uri_paths(&invalid_utf8).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn file_uri_preserves_non_utf8_unix_path_bytes() {
        use std::os::unix::ffi::OsStrExt;
        let paths = uri_paths(b"file:///tmp/%FF%FE%25").unwrap();
        assert_eq!(paths[0].as_os_str().as_bytes(), b"/tmp/\xff\xfe%");
    }

    #[test]
    fn file_uri_rejects_invalid_input_before_url_normalization() {
        for suffix in [
            "%",
            "%0",
            "%xx",
            "%00",
            "%00/..",
            "%xx/..",
            "a\0b",
            "a\tb",
            "a ",
            "a?query",
            "a#fragment",
        ] {
            let input = format!("{FILE_ROOT}/{suffix}");
            assert!(uri_paths(input.as_bytes()).is_err(), "{input:?}");
        }
        for input in [
            "/tmp/a",
            "file://other/tmp/a",
            "file://127.0.0.1/tmp/a",
            "https://localhost/a",
        ] {
            assert!(uri_paths(input.as_bytes()).is_err(), "{input}");
        }
        assert!(uri_paths(format!("{FILE_ROOT}/valid\nfile://other/tmp/a").as_bytes()).is_err());
    }

    #[test]
    fn uri_list_keeps_clipboard_markers_and_the_entry_limit() {
        assert!(uri_paths(b"copy\ncut\n#comment\r\n\n").unwrap().is_empty());
        let input = format!("{FILE_ROOT}/a\r\n").repeat(4096);
        assert_eq!(uri_paths(input.as_bytes()).unwrap().len(), 4096);
        assert!(uri_paths(format!("{input}{FILE_ROOT}/extra").as_bytes()).is_err());
    }
}

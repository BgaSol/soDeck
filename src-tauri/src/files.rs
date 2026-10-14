use crate::connection::{self, SharedContext};
use crate::ssh_transport::Cancellation;
use crate::{
    blocking,
    model::{AppError, AppResult},
    State,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use ssh2::{FileStat, OpenFlags, OpenType, Sftp};
use std::{
    collections::HashMap,
    io::{Read, Write},
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use tauri::{Manager, State as TauriState, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

const TEXT_LIMIT: usize = 1024 * 1024;

#[derive(Default)]
pub struct FileState {
    editors: Mutex<HashMap<String, EditorHandle>>,
}

#[derive(Clone)]
struct EditorHandle {
    spec: SharedContext,
    path: String,
    gate: Arc<Mutex<()>>,
    dirty: Arc<AtomicBool>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorDocument {
    server_name: String,
    path: String,
    text: String,
    version: String,
    size: usize,
}

fn remote_path(path: &str) -> AppResult<&Path> {
    if path.is_empty() || path.contains('\0') {
        return Err(AppError::new("path", "远端文件路径无效"));
    }
    Ok(Path::new(path))
}

fn text_bytes(sftp: &Sftp, path: &str) -> AppResult<(Vec<u8>, FileStat)> {
    let path = remote_path(path)?;
    let stat = sftp.lstat(path)?;
    if !stat.is_file() {
        return Err(AppError::new("fileType", "只能在线编辑普通文本文件"));
    }
    if stat.size.unwrap_or(0) > TEXT_LIMIT as u64 {
        return Err(AppError::new(
            "tooLarge",
            "在线编辑支持不超过 1 MB 的文本文件，请下载后编辑",
        ));
    }
    let mut bytes = Vec::new();
    sftp.open(path)?
        .take((TEXT_LIMIT + 1) as u64)
        .read_to_end(&mut bytes)?;
    validate_text(&bytes)?;
    Ok((bytes, stat))
}

fn validate_text(bytes: &[u8]) -> AppResult<&str> {
    if bytes.len() > TEXT_LIMIT {
        return Err(AppError::new(
            "tooLarge",
            "在线编辑支持不超过 1 MB 的文本文件，请下载后编辑",
        ));
    }
    let content = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    let text = std::str::from_utf8(content)
        .map_err(|_| AppError::new("encoding", "此文件不是 UTF-8 文本，请下载后编辑"))?;
    if text
        .chars()
        .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\r' | '\t' | '\u{c}'))
    {
        return Err(AppError::new(
            "binary",
            "此文件包含二进制内容，请下载后查看",
        ));
    }
    Ok(text)
}

fn revision(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn document(editor: &EditorHandle, bytes: &[u8]) -> AppResult<EditorDocument> {
    Ok(EditorDocument {
        server_name: editor.spec.target().name.clone(),
        path: editor.path.clone(),
        text: validate_text(bytes)?.to_owned(),
        version: revision(bytes),
        size: bytes.len(),
    })
}

fn edited_bytes(original: &[u8], text: &str) -> AppResult<Vec<u8>> {
    let source = validate_text(original)?;
    // Code editors normalize to LF; preserve the existing file's CRLF convention.
    let normalized = text.replace("\r\n", "\n");
    if normalized == source.replace("\r\n", "\n") {
        return Ok(original.to_vec());
    }
    let crlf = source.contains("\r\n") && !source.replace("\r\n", "").contains('\n');
    let encoded = if crlf {
        normalized.replace('\n', "\r\n")
    } else {
        normalized
    };
    let mut bytes = Vec::with_capacity(encoded.len() + 3);
    if original.starts_with(&[0xef, 0xbb, 0xbf]) {
        bytes.extend_from_slice(&[0xef, 0xbb, 0xbf]);
    }
    bytes.extend_from_slice(encoded.as_bytes());
    validate_text(&bytes)?;
    Ok(bytes)
}

#[tauri::command]
pub async fn open_editor(
    window: WebviewWindow,
    app: tauri::AppHandle,
    state: TauriState<'_, Arc<State>>,
    session_id: String,
    path: String,
) -> AppResult<String> {
    let spec = state.spec(&session_id, window.label())?;
    let editor = blocking(move || {
        let connection = connection::connect_verified(&spec, &Cancellation::default())?;
        let session = connection.session();
        let sftp = session.sftp()?;
        let canonical = sftp
            .realpath(remote_path(&path)?)?
            .to_string_lossy()
            .to_string();
        text_bytes(&sftp, &canonical)?;
        Ok(EditorHandle {
            spec,
            path: canonical,
            gate: Arc::new(Mutex::new(())),
            dirty: Arc::new(AtomicBool::new(false)),
        })
    })
    .await?;
    let (label, create) = {
        let _gate = state.configuration.lock().unwrap();
        state.check_context(&editor.spec)?;
        let mut editors = state.files.editors.lock().unwrap();
        if let Some((label, _)) = editors.iter().find(|(_, item)| {
            item.spec.identity() == editor.spec.identity() && item.path == editor.path
        }) {
            (label.clone(), false)
        } else {
            let label = format!("editor-{}", uuid::Uuid::new_v4());
            editors.insert(label.clone(), editor.clone());
            (label, true)
        }
    };
    if !create {
        if let Some(existing) = app.get_webview_window(&label) {
            let _ = existing.unminimize();
            existing
                .set_focus()
                .map_err(|e| AppError::new("window", e))?;
        }
        return Ok(label);
    }
    let result =
        WebviewWindowBuilder::new(&app, &label, WebviewUrl::App("index.html?editor=1".into()))
            .title(format!("{} — {}", editor.path, editor.spec.target().name))
            .inner_size(960.0, 720.0)
            .min_inner_size(560.0, 360.0)
            .build();
    match result {
        Ok(editor_window) => {
            let state = state.inner().clone();
            let cleanup_label = label.clone();
            editor_window.on_window_event(move |event| {
                if matches!(event, tauri::WindowEvent::Destroyed) {
                    state.files.editors.lock().unwrap().remove(&cleanup_label);
                }
            });
            Ok(label)
        }
        Err(error) => {
            state.files.editors.lock().unwrap().remove(&label);
            Err(AppError::new("window", error))
        }
    }
}

fn editor_for(state: &State, window: &WebviewWindow) -> AppResult<EditorHandle> {
    state
        .files
        .editors
        .lock()
        .unwrap()
        .get(window.label())
        .cloned()
        .ok_or_else(|| AppError::new("permission", "此窗口没有可编辑的远程文件"))
}

impl FileState {
    pub fn has_activity(&self) -> bool {
        !self.editors.lock().unwrap().is_empty()
    }
    pub fn has_server_activity(&self, id: &str) -> bool {
        self.editors
            .lock()
            .unwrap()
            .values()
            .any(|e| e.spec.uses_server(id))
    }
    pub fn unsaved_count(&self) -> usize {
        self.editors
            .lock()
            .unwrap()
            .values()
            .filter(|editor| editor.dirty.load(Ordering::SeqCst))
            .count()
    }
}

#[tauri::command]
pub fn editor_set_dirty(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
    dirty: bool,
) -> AppResult<()> {
    editor_for(&state, &window)?
        .dirty
        .store(dirty, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
pub fn unsaved_editor_count(
    window: WebviewWindow,
    state: TauriState<Arc<State>>,
) -> AppResult<usize> {
    crate::main_only(&window)?;
    Ok(state.files.unsaved_count())
}

#[tauri::command]
pub async fn editor_load(
    window: WebviewWindow,
    state: TauriState<'_, Arc<State>>,
) -> AppResult<EditorDocument> {
    let editor = editor_for(&state, &window)?;
    blocking(move || {
        let _gate = editor.gate.lock().unwrap();
        let connection = connection::connect_verified(&editor.spec, &Cancellation::default())?;
        let session = connection.session();
        let (bytes, _) = text_bytes(&session.sftp()?, &editor.path)?;
        document(&editor, &bytes)
    })
    .await
}

#[tauri::command]
pub async fn editor_save(
    window: WebviewWindow,
    state: TauriState<'_, Arc<State>>,
    text: String,
    version: String,
    force: bool,
) -> AppResult<EditorDocument> {
    let editor = editor_for(&state, &window)?;
    validate_text(text.as_bytes())?;
    blocking(move || {
        let _gate = editor.gate.lock().unwrap();
        let connection = connection::connect_verified(&editor.spec, &Cancellation::default())?;
        let session = connection.session();
        let sftp = session.sftp()?;
        let (before, stat) = text_bytes(&sftp, &editor.path)?;
        if !force && revision(&before) != version {
            return Err(AppError::new(
                "conflict",
                "远端文件已被修改，请重新加载或确认覆盖",
            ));
        }
        let bytes = edited_bytes(&before, &text)?;
        let temp = format!("{}.ssh-workspace-{}.tmp", editor.path, uuid::Uuid::new_v4());
        let result = (|| -> AppResult<()> {
            let mut file = sftp.open_mode(
                Path::new(&temp),
                OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::EXCLUSIVE,
                0o600,
                OpenType::File,
            )?;
            file.write_all(&bytes)?;
            file.flush()?;
            file.setstat(FileStat {
                size: None,
                uid: None,
                gid: None,
                perm: stat.perm.map(|mode| mode & 0o7777),
                atime: None,
                mtime: None,
            })?;
            file.close()?;
            // Check once more immediately before publishing the staged file.
            let (current, _) = text_bytes(&sftp, &editor.path)?;
            if revision(&current) != revision(&before) {
                return Err(AppError::new(
                    "conflict",
                    "保存期间远端文件发生变化，请重试",
                ));
            }
            crate::remote_write::atomic_remote_replace(session, &sftp, &temp, &editor.path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = sftp.unlink(Path::new(&temp));
        }
        result?;
        document(&editor, &bytes)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn text_validation_and_encoding_preserve_bom_and_crlf() {
        assert!(validate_text(b"a\0b").is_err());
        assert!(validate_text(&[0xff, 0xfe]).is_err());
        assert!(validate_text(&vec![b'a'; TEXT_LIMIT + 1]).is_err());
        let original = b"\xef\xbb\xbfa\r\nb\r\n";
        assert_eq!(
            edited_bytes(original, "new\nline\n").unwrap(),
            b"\xef\xbb\xbfnew\r\nline\r\n"
        );
        assert_ne!(revision(original), revision(b"a\r\nb\r\n"));
    }
}

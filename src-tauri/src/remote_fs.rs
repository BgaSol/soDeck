use crate::connection;
use crate::ssh_transport::Cancellation;
use crate::{
    blocking,
    model::{AppError, AppResult},
    moves, State,
};
use serde::Serialize;
use ssh2::Sftp;
use std::{collections::HashSet, path::Path, sync::Arc};
use tauri::{State as TauriState, WebviewWindow};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    name: String,
    path: String,
    is_dir: bool,
    is_symlink: bool,
    size: u64,
    modified: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Directory {
    path: String,
    resolved_path: String,
    entries: Vec<FileEntry>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryResult {
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    directory: Option<Directory>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryBatch {
    home_path: String,
    path: String,
    directories: Vec<DirectoryResult>,
}

fn validate_path(path: &str) -> AppResult<()> {
    if path.is_empty() || path.len() > 16384 || path.contains('\0') {
        return Err(AppError::new("path", "远程目录路径无效"));
    }
    Ok(())
}

// Preserve logical link names. `..` must be resolved by SFTP, not by local
// PathBuf (Windows separators and symlink traversal have different semantics).
fn absolute_path(sftp: &Sftp, path: &str, home: &str) -> AppResult<String> {
    validate_path(path)?;
    let expanded = if path == "~" || path == "." {
        home.to_owned()
    } else if let Some(rest) = path.strip_prefix("~/") {
        format!("{}/{}", home.trim_end_matches('/'), rest)
    } else if path.starts_with('/') {
        path.to_owned()
    } else {
        format!("{}/{}", home.trim_end_matches('/'), path)
    };
    if expanded.split('/').any(|part| part == "..") {
        return Ok(sftp
            .realpath(Path::new(&expanded))?
            .to_string_lossy()
            .into_owned());
    }
    let parts: Vec<_> = expanded
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .collect();
    Ok(format!("/{}", parts.join("/")))
}

fn path_chain(path: &str) -> AppResult<Vec<String>> {
    if !path.starts_with('/') {
        return Err(AppError::new("path", "远程目录必须是绝对路径"));
    }
    let mut result = vec!["/".to_owned()];
    let mut parent = String::new();
    for part in path.split('/').filter(|part| !part.is_empty()) {
        if result.len() >= 256 {
            return Err(AppError::new("path", "远程目录层级过深"));
        }
        parent.push('/');
        parent.push_str(part);
        result.push(parent.clone());
    }
    Ok(result)
}

fn read_directory(sftp: &Sftp, path: &str) -> AppResult<Directory> {
    validate_path(path)?;
    let canonical = sftp.realpath(Path::new(path))?;
    let resolved_path = canonical.to_string_lossy().into_owned();
    let mut entries = Vec::new();
    let mut handle = sftp.opendir(&canonical)?;
    loop {
        // Read the raw remote name. Sftp::readdir joins it with a desktop
        // PathBuf, which interprets legal POSIX backslashes as Windows folders.
        let (name, stat) = match handle.readdir() {
            Ok(entry) => entry,
            // ssh2::File::readdir uses LIBSSH2_ERROR_FILE for end-of-list.
            Err(error) if error.code() == ssh2::ErrorCode::Session(-16) => break,
            Err(error) => return Err(error.into()),
        };
        let name = name.to_string_lossy().into_owned();
        if name == "." || name == ".." {
            continue;
        }
        let remote_path = format!("{}/{}", path.trim_end_matches('/'), name);
        let is_symlink = stat.file_type().is_symlink();
        // Resolve only links, avoiding an extra round trip per ordinary entry.
        let is_dir = stat.is_dir()
            || (is_symlink
                && sftp
                    .stat(Path::new(&remote_path))
                    .map(|target| target.is_dir())
                    .unwrap_or(false));
        entries.push(FileEntry {
            name,
            path: remote_path,
            is_dir,
            is_symlink,
            size: stat.size.unwrap_or(0),
            modified: stat.mtime.unwrap_or(0),
        });
    }
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(Directory {
        path: path.to_owned(),
        resolved_path,
        entries,
    })
}

#[tauri::command]
pub async fn list_directory(
    window: WebviewWindow,
    state: TauriState<'_, Arc<State>>,
    session_id: String,
    path: Option<String>,
) -> AppResult<Directory> {
    moves::workspace_only(&window)?;
    let spec = state.spec(&session_id, window.label())?;
    blocking(move || {
        let connection = connection::connect_verified(&spec, &Cancellation::default())?;
        let session = connection.session();
        let sftp = session.sftp()?;
        let home = sftp
            .realpath(Path::new("."))?
            .to_string_lossy()
            .into_owned();
        let path = absolute_path(&sftp, path.as_deref().unwrap_or(&home), &home)?;
        read_directory(&sftp, &path)
    })
    .await
}

#[tauri::command]
pub async fn list_directory_chain(
    window: WebviewWindow,
    state: TauriState<'_, Arc<State>>,
    session_id: String,
    path: Option<String>,
    known_paths: Option<Vec<String>>,
) -> AppResult<DirectoryBatch> {
    moves::workspace_only(&window)?;
    let spec = state.spec(&session_id, window.label())?;
    let known = known_paths.unwrap_or_default();
    if known.len() > 4096 {
        return Err(AppError::new("path", "缓存目录数量过多"));
    }
    blocking(move || {
        let connection = connection::connect_verified(&spec, &Cancellation::default())?;
        let session = connection.session();
        let sftp = session.sftp()?;
        let home_path = sftp
            .realpath(Path::new("."))?
            .to_string_lossy()
            .into_owned();
        let path = absolute_path(&sftp, path.as_deref().unwrap_or(&home_path), &home_path)?;
        // Validate the target first, so a mistyped address does not create an
        // apparent directory. An unreadable ancestor may still be traversable.
        if !sftp.stat(Path::new(&path))?.is_dir() {
            return Err(AppError::new("fileType", "目标不是远程目录"));
        }
        let known: HashSet<_> = known.into_iter().collect();
        let mut directories = Vec::new();
        for current in path_chain(&path)? {
            if known.contains(&current) {
                continue;
            }
            let result = match read_directory(&sftp, &current) {
                Ok(directory) => DirectoryResult {
                    path: current,
                    directory: Some(directory),
                    error: None,
                },
                Err(error) => DirectoryResult {
                    path: current,
                    directory: None,
                    error: Some(error.message),
                },
            };
            directories.push(result);
        }
        Ok(DirectoryBatch {
            home_path,
            path,
            directories,
        })
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_reads_only_ancestors_and_preserves_posix_names() {
        assert_eq!(path_chain("/").unwrap(), ["/"]);
        assert_eq!(
            path_chain("/home/alice/中文 folder").unwrap(),
            ["/", "/home", "/home/alice", "/home/alice/中文 folder"]
        );
        assert_eq!(path_chain("/a\\b/c").unwrap(), ["/", "/a\\b", "/a\\b/c"]);
        assert!(path_chain("relative").is_err());
        assert!(path_chain(&format!("/{}", vec!["a"; 256].join("/"))).is_err());
        assert!(validate_path("/a\0b").is_err());
    }
}

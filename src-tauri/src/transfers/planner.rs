use super::model::{Source, WriteHandle};
use crate::{
    connection,
    model::{AppError, AppResult},
    remote_path,
};
use serde::{Deserialize, Serialize};
use ssh2::Sftp;
use std::{collections::HashSet, fs, path::Path, time::UNIX_EPOCH};

#[derive(Clone, Serialize, Deserialize)]
pub struct Entry {
    pub source: String,
    pub parts: Vec<String>,
    pub directory: bool,
    pub size: u64,
    pub modified: Option<u64>,
    pub mode: Option<u32>,
    pub created_directory: bool,
    pub target: Option<String>,
    pub outcome: String,
}
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Plan {
    pub entries: Vec<Entry>,
    pub skipped: Vec<String>,
}
fn push(plan: &mut Plan, entry: Entry) -> AppResult<()> {
    if plan.entries.len() + plan.skipped.len() >= 100_000 || entry.parts.len() > 128 {
        return Err(AppError::new("limit", "目录层级或项目数量过多，请分批传输"));
    }
    plan.entries.push(entry);
    Ok(())
}
fn local(item: &WriteHandle, plan: &mut Plan, path: &Path, parts: Vec<String>) -> AppResult<()> {
    item.check()?;
    check_scan_limit(plan, parts.len())?;
    let stat = fs::symlink_metadata(path)?;
    if super::download::is_link(&stat) || !(stat.is_file() || stat.is_dir()) {
        plan.skipped.push(path.display().to_string());
        return Ok(());
    }
    let source = path
        .to_str()
        .ok_or_else(|| AppError::new("path", "本地文件名不是有效 UTF-8"))?
        .to_owned();
    push(
        plan,
        Entry {
            source,
            parts: parts.clone(),
            directory: stat.is_dir(),
            size: if stat.is_file() { stat.len() } else { 0 },
            modified: stat
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs()),
            mode: local_mode(&stat),
            created_directory: false,
            target: None,
            outcome: "pending".into(),
        },
    )?;
    if stat.is_dir() {
        let mut children = Vec::new();
        for child in fs::read_dir(path)? {
            item.check()?;
            if children.len() + plan.entries.len() + plan.skipped.len() >= 100_000 {
                return Err(AppError::new("limit", "目录项目过多，请分批传输"));
            }
            children.push(child?);
        }
        children.sort_by_key(|e| e.file_name());
        for child in children {
            let name = child
                .file_name()
                .into_string()
                .map_err(|_| AppError::new("path", "本地文件名不是有效 UTF-8"))?;
            remote_path::component(&name)?;
            let mut parts = parts.clone();
            parts.push(name);
            local(item, plan, &child.path(), parts)?;
        }
    }
    Ok(())
}
fn remote(
    item: &WriteHandle,
    plan: &mut Plan,
    sftp: &Sftp,
    path: &str,
    parts: Vec<String>,
) -> AppResult<()> {
    item.check()?;
    // Copying the user's home must not enumerate the live control directory
    // that this transfer just deployed there (its heartbeat keeps changing).
    if item
        .receipt
        .lock()
        .unwrap()
        .direct
        .as_ref()
        .is_some_and(|d| d.source.root == path)
    {
        return Ok(());
    }
    check_scan_limit(plan, parts.len())?;
    let stat = sftp.lstat(Path::new(path))?;
    if !(stat.is_file() || stat.is_dir()) {
        plan.skipped.push(path.to_owned());
        return Ok(());
    }
    push(
        plan,
        Entry {
            source: path.to_owned(),
            parts: parts.clone(),
            directory: stat.is_dir(),
            size: if stat.is_file() {
                stat.size.unwrap_or(0)
            } else {
                0
            },
            modified: stat.mtime,
            mode: stat.perm.map(|p| p & 0o777),
            created_directory: false,
            target: None,
            outcome: "pending".into(),
        },
    )?;
    if stat.is_dir() {
        let mut directory = sftp.opendir(Path::new(path))?;
        let mut names = Vec::new();
        loop {
            match directory.readdir() {
                Ok((name, _)) => {
                    let name = name
                        .to_str()
                        .ok_or_else(|| AppError::new("path", "远程文件名不是有效 UTF-8"))?;
                    if name == "." || name == ".." {
                        continue;
                    }
                    remote_path::component(name)?;
                    names.push(name.to_owned());
                }
                Err(e) if e.code() == ssh2::ErrorCode::Session(-16) => break,
                Err(e) => return Err(e.into()),
            }
            if names.len() > 100_000 {
                return Err(AppError::new("limit", "目录项目过多"));
            }
        }
        names.sort();
        for name in names {
            let mut parts = parts.clone();
            parts.push(name.clone());
            remote(item, plan, sftp, &remote_path::join(path, &name)?, parts)?;
        }
    }
    Ok(())
}
fn check_scan_limit(plan: &Plan, depth: usize) -> AppResult<()> {
    if plan.entries.len() + plan.skipped.len() >= 100_000 || depth > 128 {
        return Err(AppError::new("limit", "目录层级或项目数量过多，请分批传输"));
    }
    Ok(())
}
pub fn normalize_selection(paths: &[String]) -> AppResult<Vec<String>> {
    if paths.is_empty() || paths.len() > 10_000 {
        return Err(AppError::new("validation", "请选择 1–10000 个项目"));
    }
    let mut values = Vec::new();
    for path in paths {
        remote_path::absolute(path)?;
        if path == "/" {
            return Err(AppError::new("path", "不能把整个根目录作为传输项目"));
        }
        let value = path
            .split('/')
            .filter(|p| !p.is_empty() && *p != ".")
            .collect::<Vec<_>>()
            .join("/");
        if value.is_empty() {
            return Err(AppError::new("path", "不能把整个根目录作为传输项目"));
        }
        values.push(format!("/{value}"));
    }
    values.sort();
    values.dedup();
    let selected: HashSet<_> = values.iter().map(String::as_str).collect();
    Ok(values
        .iter()
        .filter(|p| {
            !p.match_indices('/')
                .any(|(index, _)| index > 0 && selected.contains(&p[..index]))
        })
        .cloned()
        .collect())
}
pub fn scan(item: &WriteHandle) -> AppResult<Plan> {
    let mut plan = Plan::default();
    match &item.source {
        Source::Local(paths) => {
            // Normalize only ancestors: canonicalizing the file itself follows a
            // selected symlink, which would silently change the transfer source.
            let mut normalized = Vec::new();
            for path in paths {
                let parent = fs::canonicalize(
                    path.parent()
                        .ok_or_else(|| AppError::new("path", "本地项目缺少父目录"))?,
                )?;
                normalized.push(
                    parent.join(
                        path.file_name()
                            .ok_or_else(|| AppError::new("path", "不能上传本地根目录"))?,
                    ),
                );
            }
            normalized.sort();
            normalized.dedup();
            for path in &normalized {
                if normalized
                    .iter()
                    .any(|other| other != path && path.starts_with(other))
                {
                    continue;
                }
                let name = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .ok_or_else(|| AppError::new("path", "文件名无效"))?;
                local(item, &mut plan, path, vec![name.into()])?;
            }
        }
        Source::Remote { context, paths } => {
            let connection = connection::connect_verified(context, &item.cancel)?;
            let sftp = connection.session().sftp()?;
            for path in normalize_selection(paths)? {
                let name = remote_path::name(&path)?.to_owned();
                remote(item, &mut plan, &sftp, &path, vec![name])?;
            }
        }
    }
    Ok(plan)
}
#[cfg(unix)]
fn local_mode(stat: &fs::Metadata) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    Some(stat.permissions().mode() & 0o777)
}
#[cfg(not(unix))]
fn local_mode(_: &fs::Metadata) -> Option<u32> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selection_deduplicates_only_path_components() {
        assert_eq!(
            normalize_selection(&["/a/b".into(), "/ab".into(), "/a".into(), "/a/".into()]).unwrap(),
            vec!["/a", "/ab"]
        );
        assert!(normalize_selection(&["/a/../b".into()]).is_err());
        assert!(normalize_selection(&["/".into()]).is_err());
        assert!(normalize_selection(&["//./".into()]).is_err());
        assert_eq!(
            normalize_selection(&["/a".into(), "/a-foo".into(), "/a/z".into()]).unwrap(),
            vec!["/a", "/a-foo"]
        );
    }
}

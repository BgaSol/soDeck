//! Same-host operations use the existing verified SSH route, never A-to-B auth.
use super::{model::WriteHandle, planner::Plan};
use crate::{
    model::{AppError, AppResult},
    remote_path, remote_write,
    ssh::quote,
    ssh_transport::{Cancellation, SshConnection},
    State,
};
use std::{path::Path, time::Duration};

pub fn validate(item: &WriteHandle, s: &ssh2::Sftp, root: &str) -> AppResult<()> {
    let super::Source::Remote { paths, .. } = &item.source else {
        return Ok(());
    };
    for path in paths {
        remote_write::safe_ancestors(s, remote_path::parent(path))?;
        // Canonicalize the parent even when a previous attempt already moved the file.
        let parent = s
            .realpath(Path::new(remote_path::parent(path)))?
            .to_string_lossy()
            .into_owned();
        let source = remote_path::join(&parent, remote_path::name(path)?)?;
        if root == source || root.starts_with(&format!("{source}/")) {
            return Err(AppError::new("path", "不能复制或移动到来源自身或其子目录"));
        }
        if item.moving() && root == parent {
            return Err(AppError::new("noChange", "项目已经在该目录中"));
        }
        if root == parent && item.task.lock().unwrap().conflict != super::ConflictPolicy::Rename {
            return Err(AppError::new("conflict", "复制到原目录时请选择自动重命名"));
        }
    }
    Ok(())
}
pub fn stamp(c: &SshConnection, item: &WriteHandle, path: &str) -> AppResult<String> {
    let (code, out, err) = super::direct::capabilities::exec_checked(
        c,
        &item.cancel,
        &format!("LC_ALL=C stat -c '%d:%i:%s:%y:%z' -- {}", quote(path)),
        Duration::from_secs(30),
        || item.check(),
    )?;
    if code != 0 {
        return Err(AppError::new(
            "changed",
            format!("无法核对来源 {path}：{err}"),
        ));
    }
    Ok(out.trim_end().into())
}
pub fn same_filesystem(
    c: &SshConnection,
    item: &WriteHandle,
    s: &ssh2::Sftp,
    source: &str,
    destination: &str,
) -> AppResult<bool> {
    let mut parent = remote_path::parent(destination);
    while remote_write::metadata(s, parent)?.is_none() && parent != "/" {
        parent = remote_path::parent(parent);
    }
    // OpenSSH often rejects fstatvfs on directory handles. st_dev is the
    // relevant identity for rename and also detects a selected mount point.
    let (code, out, _) = super::direct::capabilities::exec_checked(
        c,
        &item.cancel,
        &format!("stat -c '%d' -- {} {}", quote(source), quote(parent)),
        Duration::from_secs(30),
        || item.check(),
    )?;
    let values: Vec<_> = out.lines().collect();
    Ok(code == 0 && values.len() == 2 && !values[0].is_empty() && values[0] == values[1])
}
pub fn move_directories(
    state: &State,
    app: Option<&tauri::AppHandle>,
    item: &WriteHandle,
    c: &SshConnection,
    plan: &mut Plan,
) -> AppResult<()> {
    // Use the fast path only on a fresh complete plan; partial merges stay per-item.
    if plan.entries.iter().any(|e| e.committed()) {
        return Ok(());
    }
    let current = super::planner::scan(item)?;
    let s = c.session().sftp()?;
    for i in 0..plan.entries.len() {
        let root = plan.entries[i].clone();
        if !root.directory || root.parts.len() != 1 || root.committed() || root.outcome == "skipped"
        {
            continue;
        }
        let destination = root.target.as_deref().unwrap();
        let contains = |p: &str| p == root.source || p.starts_with(&format!("{}/", root.source));
        let indices: Vec<usize> = plan
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| contains(&e.source))
            .map(|(i, _)| i)
            .collect();
        if indices
            .iter()
            .any(|&i| plan.entries[i].outcome == "skipped")
            || current.skipped.iter().any(|p| contains(p))
            || plan.skipped.iter().any(|p| contains(p))
            || remote_write::metadata(&s, destination)?.is_some()
            || !same_filesystem(c, item, &s, &root.source, destination)?
        {
            continue;
        }
        let fresh: Vec<_> = current
            .entries
            .iter()
            .filter(|e| contains(&e.source))
            .collect();
        if fresh.len() != indices.len()
            || fresh.iter().any(|e| {
                !indices.iter().any(|&i| {
                    let old = &plan.entries[i];
                    old.source == e.source
                        && old.directory == e.directory
                        && old.size == e.size
                        && old.modified == e.modified
                })
            })
        {
            return Err(AppError::new(
                "changed",
                "来源目录内容在扫描后变化，请重新选择",
            ));
        }
        let check = super::verify::SourceCheck::new(item)?;
        for &index in &indices {
            let e = &plan.entries[index];
            check.unchanged(e)?;
            if e.source_stamp.as_ref() != Some(&stamp(c, item, &e.source)?) {
                return Err(AppError::new("changed", "来源在移动前变化"));
            }
        }
        if let Some(task) = state.transfers.conflicting_commit(item, destination) {
            return Err(AppError::new(
                "conflictPending",
                format!("目标存在待核对任务：{task}"),
            ));
        }
        item.receipt.lock().unwrap().pending_commit = Some(super::model::PendingCommit {
            rename_directory: true,
            destination: destination.into(),
            stage_path: Some(root.source.clone()),
            expected_size: None,
            run_id: item.task.lock().unwrap().run_id.clone(),
            entry: i,
            digest: None,
            directory: true,
        });
        state.transfers.persist(item)?;
        item.check()?;
        if let Err(e) = s.rename(
            Path::new(&root.source),
            Path::new(destination),
            Some(ssh2::RenameFlags::empty()),
        ) {
            if matches!(e.code(), ssh2::ErrorCode::SFTP(code) if code != 6 && code != 7) {
                item.receipt.lock().unwrap().pending_commit = None;
                state.transfers.persist(item)?;
                // Unsupported rename, mount boundary, or a racing destination:
                // the ordinary merge/publish path will recheck the conflict.
                continue;
            }
            return Err(AppError::new("unknown", format!("目录移动结果待核对：{e}")));
        }
        for &index in &indices {
            plan.entries[index].outcome = "completed".into();
            plan.entries[index].error = None;
            if !plan.entries[index].directory {
                let mut t = item.task.lock().unwrap();
                t.files_done += 1;
                t.committed_bytes += plan.entries[index].size;
            }
        }
        state.transfers.save_plan(item, plan)?;
        item.receipt.lock().unwrap().pending_commit = None;
        state.transfers.persist(item)?;
        if let Some(app) = app {
            super::directory_changed(
                app,
                item.target.identity(),
                remote_path::parent(&root.source),
            );
            super::directory_changed(
                app,
                item.target.identity(),
                remote_path::parent(destination),
            );
        }
    }
    Ok(())
}
pub fn directory_rename_completed(
    c: &SshConnection,
    s: &ssh2::Sftp,
    source: &str,
    target: &str,
    original: &str,
) -> AppResult<bool> {
    if remote_write::metadata(s, source)?.is_some() {
        return Ok(false);
    }
    if !remote_write::metadata(s, target)?.is_some_and(|s| s.is_dir()) {
        return Err(AppError::new("unknown", "来源和目标目录状态无法确认"));
    }
    let (code, out, _) = super::direct::capabilities::exec(
        c,
        &Cancellation::default(),
        &format!("LC_ALL=C stat -c '%d:%i' -- {}", quote(target)),
    )?;
    if code != 0 || inode(original) != inode(out.trim()) {
        return Err(AppError::new(
            "unknown",
            "目标目录身份与移动前不符，保留记录",
        ));
    }
    Ok(true)
}
pub fn transfer(
    state: &State,
    item: &WriteHandle,
    c: &SshConnection,
    plan: &mut Plan,
    index: usize,
    entries: &[usize],
) -> AppResult<()> {
    let root = item.receipt.lock().unwrap().attempts[index]
        .target_root
        .clone();
    let s = c.session().sftp()?;
    // A short script contains quoted paths only. Payload remains on the server.
    let mut script = String::from("#!/bin/sh\nset -eu\n");
    for &i in entries {
        let e = &plan.entries[i];
        let destination = e.staged.as_deref().unwrap();
        script.push_str(&format!(
            "mkdir -p -- {}\ncp -p -- {} {}\n",
            quote(remote_path::parent(destination)),
            quote(&e.source),
            quote(destination)
        ));
    }
    let script_path = format!("{root}/copy.sh");
    super::direct::write_new(&s, &script_path, script.as_bytes())?;
    item.receipt.lock().unwrap().attempts[index].dispatched = true;
    state.transfers.persist(item)?;
    let marker = format!("{root}/copy.exit");
    let next = format!("{marker}.next");
    let command = format!("trap ':' HUP INT TERM; sh {}; code=$?; printf '%s\\n' \"$code\" > {} && mv -- {} {}; exit \"$code\"", quote(&script_path), quote(&next), quote(&next), quote(&marker));
    let result = super::direct::execute_remote(c, None, item, &command, None, false);
    let code = super::direct::exit_code(&s, &root, "copy.exit")
        .ok()
        .flatten();
    {
        let mut r = item.receipt.lock().unwrap();
        r.attempts[index].source_finished = code.is_some();
        r.attempts[index].receiver_finished = code.is_some();
        r.attempts[index].successful = code == Some(0);
    }
    state.transfers.persist(item)?;
    item.check()?;
    let (exit, _, error) = result?;
    if exit != 0 || code != Some(0) {
        return Err(AppError::new("copy", format!("服务器内复制失败：{error}")));
    }
    for &i in entries {
        plan.entries[i].error = None;
        state.transfers.save_entry(item, i, &plan.entries[i])?;
        item.task.lock().unwrap().bytes_done += plan.entries[i].size;
    }
    Ok(())
}
pub fn refresh(item: &WriteHandle, a: &mut super::model::Attempt) -> AppResult<()> {
    if !a.dispatched || (a.source_finished && a.receiver_finished) {
        return Ok(());
    }
    let c = crate::connection::connect_verified(&item.target, &Cancellation::default(), crate::traffic::TrafficPurpose::Transfer)?;
    let exit = super::direct::exit_code(&c.session().sftp()?, &a.target_root, "copy.exit")?;
    a.source_finished = exit.is_some();
    a.receiver_finished = exit.is_some();
    a.successful = exit == Some(0);
    Ok(())
}

// Target commit is durable before removing any original. Retrying only this stage
// never recopies a committed file. rmdir cannot delete skipped or newly added data.
pub fn finish_move(
    state: &State,
    app: Option<&tauri::AppHandle>,
    item: &WriteHandle,
    c: &SshConnection,
    plan: &mut Plan,
) -> AppResult<()> {
    let s = c.session().sftp()?;
    let mut failed = false;
    for i in (0..plan.entries.len()).rev() {
        if plan.entries[i].outcome != "copied" {
            continue;
        }
        let mut e = plan.entries[i].clone();
        let result: AppResult<()> = (|| {
            item.check()?;
            remote_write::safe_ancestors(&s, &e.source)?;
            let source_missing = remote_write::metadata(&s, &e.source)?.is_none();
            let target = e
                .target
                .as_deref()
                .ok_or_else(|| AppError::new("verification", "缺少目标记录"))?;
            remote_write::safe_ancestors(&s, target)?;
            if e.directory {
                if !remote_write::metadata(&s, target)?.is_some_and(|m| m.is_dir()) {
                    return Err(AppError::new("changed", "目标目录已变化"));
                }
            } else {
                let digest = e
                    .digest
                    .as_ref()
                    .ok_or_else(|| AppError::new("verification", "缺少移动校验值"))?;
                if super::verify::remote_digest_checked(c, target, &item.cancel, || item.check())?
                    != *digest
                {
                    return Err(AppError::new("changed", "目标内容已变化，保留来源"));
                }
            }
            if source_missing {
                if e.removal_pending {
                    return Ok(());
                }
                return Err(AppError::new("changed", "来源已被外部移除，保留记录供核对"));
            }
            if e.directory {
                if !remote_write::metadata(&s, target)?.is_some_and(|m| m.is_dir()) {
                    return Err(AppError::new("changed", "目标目录已变化"));
                }
                if plan.entries.iter().any(|child| {
                    child.source.starts_with(&format!("{}/", e.source))
                        && child.outcome != "completed"
                }) || plan
                    .skipped
                    .iter()
                    .any(|p| p.starts_with(&format!("{}/", e.source)))
                {
                    return Err(AppError::new(
                        "sourceRetained",
                        "目录中仍有跳过或未完成项目，来源目录保留",
                    ));
                }
                // Directory contents change its mtime; inode identity must remain.
                let now = stamp(c, item, &e.source)?;
                if e.source_stamp
                    .as_ref()
                    .is_none_or(|v| inode(v) != inode(&now))
                {
                    return Err(AppError::new("changed", "来源目录身份已变化"));
                }
            } else {
                super::verify::SourceCheck::new(item)?.unchanged(&e)?;
                let current = stamp(c, item, &e.source)?;
                if e.source_stamp.as_ref() != Some(&current) {
                    return Err(AppError::new("changed", "来源在复制后已变化，保留来源"));
                }
                let digest = e
                    .digest
                    .as_ref()
                    .ok_or_else(|| AppError::new("verification", "缺少移动校验值"))?;
                for p in [&e.source] {
                    if super::verify::remote_digest_checked(c, p, &item.cancel, || item.check())?
                        != *digest
                    {
                        return Err(AppError::new("changed", "来源或目标内容已变化，保留来源"));
                    }
                }
            }
            e.removal_pending = true;
            state.transfers.save_entry(item, i, &e)?;
            state.transfers.persist(item)?;
            // Check again after hashing and journal fsync; external changes abort removal.
            if !e.directory && e.source_stamp.as_ref() != Some(&stamp(c, item, &e.source)?) {
                return Err(AppError::new("changed", "来源在删除前变化，保留来源"));
            }
            if e.directory {
                s.rmdir(Path::new(&e.source))?;
            } else {
                s.unlink(Path::new(&e.source))?;
            }
            Ok(())
        })();
        match result {
            Ok(()) => {
                e.outcome = "completed".into();
                if !e.directory {
                    item.task.lock().unwrap().files_done += 1;
                }
                e.removal_pending = false;
                e.error = None;
            }
            Err(err) => {
                failed = true;
                e.error = Some(format!("已复制，来源未删除：{}", err.message));
            }
        }
        plan.entries[i] = e.clone();
        state.transfers.save_entry(item, i, &e)?;
        state.transfers.persist(item)?;
        if let Some(app) = app {
            super::directory_changed(app, item.target.identity(), remote_path::parent(&e.source));
        }
        item.check()?;
    }
    if failed {
        Err(AppError::new(
            "sourceRetained",
            "部分项目已复制，来源未删除；重试将只处理尚未完成的步骤",
        ))
    } else {
        Ok(())
    }
}
fn inode(stamp: &str) -> (&str, &str) {
    let mut parts = stamp.split(':');
    (parts.next().unwrap_or(""), parts.next().unwrap_or(""))
}

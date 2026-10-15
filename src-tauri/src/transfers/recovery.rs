use super::model::{Source, WriteHandle};
use crate::{
    connection,
    model::{AppError, AppResult},
    remote_path, remote_write,
    ssh_transport::Cancellation,
    State,
};
use std::path::Path;
fn valid_root(path: &str) -> AppResult<()> {
    if remote_path::name(path)?
        .strip_prefix(".ssh-workspace-transfer-")
        .is_none_or(|s| uuid::Uuid::parse_str(s).is_err())
    {
        return Err(AppError::new("path", "临时目录归属无效，拒绝清理"));
    }
    Ok(())
}
fn remove_tree(sftp: &ssh2::Sftp, path: &str, depth: usize) -> AppResult<()> {
    if depth > 256 {
        return Err(AppError::new("path", "临时目录过深"));
    }
    let Some(stat) = remote_write::metadata(sftp, path)? else {
        return Ok(());
    };
    if stat.is_dir() {
        for (p, _) in sftp.readdir(Path::new(path))? {
            let name = p
                .file_name()
                .and_then(|s| s.to_str())
                .ok_or_else(|| AppError::new("path", "临时文件名不是 UTF-8"))?;
            if name == "." || name == ".." {
                continue;
            }
            remove_tree(sftp, &remote_path::join(path, name)?, depth + 1)?;
        }
        sftp.rmdir(Path::new(path))?;
    } else {
        sftp.unlink(Path::new(path))?;
    }
    Ok(())
}
pub fn cleanup(item: &WriteHandle) -> AppResult<()> {
    if item.receipt.lock().unwrap().pending_commit.is_some() {
        return Err(AppError::new(
            "conflictPending",
            "先核对提交结果，暂存内容用于核对",
        ));
    }
    let count = item.receipt.lock().unwrap().attempts.len();
    let mut unresolved = false;
    for i in 0..count {
        let mut a = item.receipt.lock().unwrap().attempts[i].clone();
        if a.target_root.is_empty() && a.source_root.is_none() {
            continue;
        }
        if a.dispatched && (!a.source_finished || !a.receiver_finished) {
            let _ = super::direct::refresh_attempt(item, &mut a);
        }
        if a.dispatched && (!a.source_finished || !a.receiver_finished) {
            unresolved = true;
            item.receipt.lock().unwrap().attempts[i] = a;
            continue;
        }
        if !a.target_root.is_empty() {
            valid_root(&a.target_root)?;
            let c = connection::connect_verified(&item.target, &Cancellation::default())?;
            let s = c.session().sftp()?;
            remote_write::safe_ancestors(&s, &a.target_root)?;
            remove_tree(&s, &a.target_root, 0)?;
            a.target_root.clear();
            item.receipt.lock().unwrap().attempts[i] = a.clone();
        }
        if let (Some(root), Source::Remote { context, .. }) = (&a.source_root, &item.source) {
            valid_root(root)?;
            let c = connection::connect_verified(context, &Cancellation::default())?;
            let s = c.session().sftp()?;
            remote_write::safe_ancestors(&s, root)?;
            remove_tree(&s, root, 0)?;
            a.source_root = None;
        }
        item.receipt.lock().unwrap().attempts[i] = a;
    }
    if unresolved {
        return Err(AppError::new(
            "interrupted",
            "部分远端执行尚未确认退出，保留其临时目录；其他传输不受影响",
        ));
    }
    Ok(())
}
pub fn reconcile(state: &State, item: &WriteHandle) -> AppResult<()> {
    let count = item.receipt.lock().unwrap().attempts.len();
    for i in 0..count {
        let mut a = item.receipt.lock().unwrap().attempts[i].clone();
        if !a.target_root.is_empty() {
            super::direct::refresh_attempt(item, &mut a)?;
            item.receipt.lock().unwrap().attempts[i] = a;
        }
    }
    state.transfers.persist(item)?;
    let pending = item.receipt.lock().unwrap().pending_commit.clone();
    if let Some(commit) = pending {
        let task_id = item.task.lock().unwrap().id.clone();
        let _guard = state.transfers.paths.acquire(
            &item.write_key(),
            &commit.destination,
            &task_id,
            |_| Ok(()),
        )?;
        let c = connection::connect_verified(&item.target, &Cancellation::default())?;
        let s = c.session().sftp()?;
        let destination = remote_write::metadata(&s, &commit.destination)?;
        let recorded = item
            .plan
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|p| p.entries.get(commit.entry))
            .is_some_and(|e| {
                e.outcome == "completed" && e.target.as_deref() == Some(&commit.destination)
            });
        let completed = if recorded
            || (commit.directory && destination.as_ref().is_some_and(|s| s.is_dir()))
        {
            true
        } else if let (Some(digest), Some(stat)) = (&commit.digest, &destination) {
            stat.is_file()
                && super::verify::remote_digest(&c, &commit.destination, &Cancellation::default())?
                    == *digest
        } else {
            false
        };
        let not_submitted = if let Some(stage) = &commit.stage_path {
            remote_write::metadata(&s, stage)?.is_some()
        } else {
            destination.is_none()
        };
        if !completed && !not_submitted {
            return Err(AppError::new(
                "conflictPending",
                format!(
                    "无法可靠确认 {} 的提交结果；保留记录，可改名或换目录创建新任务",
                    commit.destination
                ),
            ));
        }
        let mut plan = item.plan.lock().unwrap().clone().unwrap_or_default();
        if commit.directory && completed {
            let old = plan
                .entries
                .get(commit.entry)
                .and_then(|e| e.target.clone());
            if let Some(old) = old {
                for e in plan.entries.iter_mut().skip(commit.entry + 1) {
                    if let Some(suffix) = e
                        .target
                        .as_ref()
                        .and_then(|p| p.strip_prefix(&format!("{old}/")))
                    {
                        e.target = Some(format!("{}/{suffix}", commit.destination));
                    }
                }
            }
        }
        if let Some(entry) = plan.entries.get_mut(commit.entry) {
            entry.outcome = if completed { "completed" } else { "pending" }.into();
            entry.target = Some(commit.destination.clone());
            if commit.directory {
                entry.created_directory = false;
            }
            entry.error = None;
        }
        state.transfers.save_plan(item, &plan)?;
        item.receipt.lock().unwrap().pending_commit = None;
    }
    let stopped = item
        .receipt
        .lock()
        .unwrap()
        .attempts
        .iter()
        .all(|a| !a.dispatched || (a.source_finished && a.receiver_finished));
    {
        let mut t = item.task.lock().unwrap();
        let plan = item.plan.lock().unwrap();
        if let Some(plan) = plan.as_ref() {
            t.files_done = plan
                .entries
                .iter()
                .filter(|e| !e.directory && e.outcome == "completed")
                .count() as u64;
            t.committed_bytes = plan
                .entries
                .iter()
                .filter(|e| e.outcome == "completed")
                .map(|e| e.size)
                .sum();
            t.skipped = plan
                .entries
                .iter()
                .filter(|e| e.outcome == "skipped")
                .count() as u64
                + plan.skipped.len() as u64;
            if plan
                .entries
                .iter()
                .all(|e| e.outcome == "completed" || e.outcome == "skipped")
            {
                t.status = "completed".into();
                t.error = None;
            }
        }
        if t.status != "completed" {
            t.status = "interrupted".into();
            t.error = Some(
                if stopped {
                    "执行已停止，可重试未完成项或清理临时数据"
                } else {
                    "远端执行尚未确认退出；可使用新目录重新执行，旧目录暂不清理"
                }
                .into(),
            );
        }
    }
    state.transfers.persist(item)
}

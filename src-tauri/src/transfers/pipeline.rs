use super::{
    direct::Controller,
    download::ConflictPolicy,
    model::{PendingCommit, Source, WriteHandle},
    planner::{self, Entry},
};
use crate::{
    connection,
    model::{AppError, AppResult},
    remote_path,
    remote_write::{self, Stage},
    State,
};
use std::{collections::HashMap, path::Path};

pub fn run(state: &State, app: Option<&tauri::AppHandle>, item: &WriteHandle) -> AppResult<()> {
    item.check()?;
    item.task.lock().unwrap().status = "checking".into();
    let target = connection::connect_verified(&item.target, &item.cancel)?;
    let sftp = target.session().sftp()?;
    let requested = item.task.lock().unwrap().target_path.clone();
    let root = sftp
        .realpath(Path::new(&requested))?
        .to_str()
        .ok_or_else(|| AppError::new("path", "目标目录不是 UTF-8"))?
        .to_owned();
    if !sftp.stat(Path::new(&root))?.is_dir() {
        return Err(AppError::new("fileType", "目标不是目录"));
    }
    item.task.lock().unwrap().target_path = root.clone();
    // Hold the target's write lane across planning and commit. Other target
    // servers are independent, and registry locks are never held over I/O.
    let lane = state.transfers.write_lane(&item.write_key());
    let _writer = loop {
        item.check()?;
        match lane.try_lock() {
            Ok(writer) => break writer,
            Err(std::sync::TryLockError::WouldBlock) => {
                std::thread::sleep(std::time::Duration::from_millis(50))
            }
            Err(std::sync::TryLockError::Poisoned(error)) => {
                return Err(AppError::new("internal", error))
            }
        }
    };
    if state.transfers.requires_reconciliation(item) {
        return Err(AppError::new(
            "quarantined",
            "目标已有待核对任务，请先完成核对再重试",
        ));
    }
    let mut controller = if matches!(item.source, Source::Remote { .. }) {
        Some(Controller::start(state, item)?)
    } else {
        None
    };
    let result = (|| {
        if let Some(controller) = controller.as_mut() {
            let stage = create_stage(state, item, &sftp, &root)?;
            let probe = Entry {
                source: controller.probe_source(),
                parts: vec![],
                directory: false,
                size: 0,
                modified: None,
                mode: None,
                created_directory: false,
                target: None,
                outcome: "pending".into(),
            };
            controller.copy(item, usize::MAX, &probe, &stage.path)?;
            stage.cleanup(&sftp)?;
            item.receipt
                .lock()
                .unwrap()
                .stage_dirs
                .retain(|p| p != &stage.directory);
        }
        item.task.lock().unwrap().status = "scanning".into();
        let saved = item.plan.lock().unwrap().take();
        let mut plan = match saved {
            Some(plan) => plan,
            None => planner::scan(item)?,
        };
        {
            let mut task = item.task.lock().unwrap();
            task.files_total = plan.entries.iter().filter(|e| !e.directory).count() as u64;
            task.bytes_total = plan.entries.iter().map(|e| e.size).sum();
            task.files_done = plan
                .entries
                .iter()
                .filter(|e| !e.directory && e.outcome == "completed")
                .count() as u64;
            task.bytes_done = plan
                .entries
                .iter()
                .filter(|e| e.outcome == "completed")
                .map(|e| e.size)
                .sum();
            task.committed_bytes = task.bytes_done;
            task.current_file = None;
            task.current_bytes = 0;
            task.skipped = plan.skipped.len() as u64
                + plan
                    .entries
                    .iter()
                    .filter(|e| e.outcome == "skipped")
                    .count() as u64;
            task.status = "running".into();
            task.warnings = plan
                .skipped
                .iter()
                .take(100)
                .map(|p| format!("跳过符号链接或特殊文件：{p}"))
                .collect();
        }
        let policy = item.task.lock().unwrap().conflict;
        let mut directories: HashMap<Vec<String>, Option<String>> = HashMap::new();
        directories.insert(vec![], Some(root.clone()));
        let mut completed_bytes = item.task.lock().unwrap().bytes_done;
        let transfer_result = (|| {
            for index in 0..plan.entries.len() {
                item.check()?;
                let entry = plan.entries[index].clone();
                let parent_parts = &entry.parts[..entry.parts.len() - 1];
                let parent = directories.get(parent_parts).cloned().flatten();
                if parent.is_none() {
                    if entry.outcome != "skipped" {
                        plan.entries[index].outcome = "skipped".into();
                        item.task.lock().unwrap().skipped += 1;
                    }
                    if entry.directory {
                        directories.insert(entry.parts.clone(), None);
                    }
                    continue;
                }
                let requested = entry.target.clone().unwrap_or(remote_path::join(
                    parent.as_ref().unwrap(),
                    entry.parts.last().unwrap(),
                )?);
                if entry.directory {
                    let destination = if entry.outcome == "completed" {
                        if !remote_write::metadata(&sftp, &requested)?.is_some_and(|s| s.is_dir()) {
                            return Err(AppError::new(
                                "changed",
                                format!("重试时目标目录已变化：{requested}"),
                            ));
                        }
                        Some(requested)
                    } else if entry.outcome == "skipped" {
                        None
                    } else {
                        let result = remote_write::directory_destination(
                            &sftp,
                            &requested,
                            policy,
                            |path| record_commit(state, item, path, None, None),
                        );
                        if !result.as_ref().is_err_and(|e| e.kind == "unknown") {
                            item.receipt.lock().unwrap().pending_commit = None;
                        }
                        let result = result?;
                        plan.entries[index].created_directory =
                            result.as_ref().is_some_and(|d| d.created);
                        result.map(|d| d.path)
                    };
                    plan.entries[index].outcome = if destination.is_some() {
                        "completed"
                    } else {
                        "skipped"
                    }
                    .into();
                    plan.entries[index].target = destination.clone();
                    if destination.is_none() && entry.outcome != "skipped" {
                        item.task.lock().unwrap().skipped += 1;
                    }
                    directories.insert(entry.parts.clone(), destination);
                    state.transfers.persist(item)?;
                    continue;
                }
                if entry.outcome == "completed" {
                    if !remote_write::metadata(&sftp, &requested)?
                        .is_some_and(|s| s.is_file() && s.size == Some(entry.size))
                    {
                        return Err(AppError::new(
                            "changed",
                            format!("已完成的目标文件发生变化：{requested}"),
                        ));
                    }
                    continue;
                }
                if entry.outcome == "skipped" {
                    continue;
                }
                if policy == ConflictPolicy::Skip
                    && remote_write::metadata(&sftp, &requested)?.is_some()
                {
                    plan.entries[index].outcome = "skipped".into();
                    let mut task = item.task.lock().unwrap();
                    task.skipped += 1;
                    continue;
                }
                plan.entries[index].target = Some(requested.clone());
                let stage = create_stage(state, item, &sftp, remote_path::parent(&requested))?;
                item.task.lock().unwrap().status = "running".into();
                {
                    let mut task = item.task.lock().unwrap();
                    task.bytes_done = completed_bytes;
                    task.committed_bytes = completed_bytes;
                    task.current_bytes = 0;
                    task.current_file = Some(entry.source.clone());
                }
                let result = (|| {
                    if let Some(controller) = controller.as_mut() {
                        controller.copy(item, index, &entry, &stage.path)?;
                    } else {
                        super::upload::upload_file(item, &sftp, &entry, &stage.path)?;
                    }
                    item.check()?;
                    let verified = VerifiedStage::verify(&sftp, &stage, entry.size)?;
                    apply_attributes(item, &sftp, &stage.path, &entry);
                    item.task.lock().unwrap().status = "committing".into();
                    let destination =
                        verified.publish(target.session(), &sftp, &requested, policy, |path| {
                            record_commit(state, item, path, Some(&stage.path), Some(entry.size))
                        });
                    if !destination.as_ref().is_err_and(|e| e.kind == "unknown") {
                        item.receipt.lock().unwrap().pending_commit = None;
                    }
                    let destination = destination?;
                    plan.entries[index].outcome = if destination.is_some() {
                        "completed"
                    } else {
                        "skipped"
                    }
                    .into();
                    plan.entries[index].target = destination;
                    {
                        let mut task = item.task.lock().unwrap();
                        if destination_is_completed(&plan.entries[index]) {
                            task.files_done += 1;
                        }
                        task.bytes_done = completed_bytes
                            + if plan.entries[index].outcome == "completed" {
                                entry.size
                            } else {
                                0
                            };
                        if plan.entries[index].outcome == "skipped" {
                            task.skipped += 1;
                        }
                        completed_bytes = task.bytes_done;
                        task.committed_bytes = completed_bytes;
                        task.current_bytes = 0;
                        task.current_file = None;
                    }
                    Ok(())
                })();
                if result
                    .as_ref()
                    .is_err_and(|e: &AppError| e.kind == "unknown")
                {
                    return result;
                }
                match stage.cleanup(&sftp) {
                    Ok(()) => item
                        .receipt
                        .lock()
                        .unwrap()
                        .stage_dirs
                        .retain(|p| p != &stage.directory),
                    Err(e) => {
                        item.warn(format!(
                            "临时文件清理待完成：{}（{}）",
                            stage.directory, e.message
                        ));
                        state.transfers.persist(item)?;
                        result?;
                        return Err(AppError::new(
                            "cleanup",
                            "临时资源清理失败，任务已暂停写入；请先核对并清理后重试",
                        ));
                    }
                }
                state.transfers.persist(item)?;
                result?;
                if let Some(app) = app {
                    super::directory_changed(
                        app,
                        item.target.identity(),
                        remote_path::parent(&requested),
                    );
                }
            }
            // Finalize newly created directories deepest first; applying a
            // read-only mode before children would break the remaining copy.
            for entry in plan
                .entries
                .iter()
                .rev()
                .filter(|e| e.directory && e.created_directory && e.outcome == "completed")
            {
                if let Some(path) = &entry.target {
                    apply_attributes(item, &sftp, path, entry);
                }
            }
            Ok(())
        })();
        *item.plan.lock().unwrap() = Some(plan);
        transfer_result
    })();
    let mut result = result;
    if let Some(controller) = controller.as_mut() {
        let stopping = if result.is_ok() {
            controller.finish(item)
        } else {
            controller.stop(item)
        };
        if let Err(error) = stopping {
            if error.kind == "unknown" || result.is_ok() {
                result = Err(error);
            }
        }
        if controller.confirmed_stopped() {
            if let Err(error) = state.transfers.persist(item) {
                item.warn(format!("停止确认保存失败：{}", error.message));
            }
        }
    }
    drop(controller);
    // Do not clean files while the source may still have them open.
    if !result.as_ref().is_err_and(|e| e.kind == "unknown") {
        if let Err(e) = super::cleanup(item) {
            item.warn(format!("临时资源清理失败：{}", e.message));
        }
    }
    if let Some(app) = app {
        super::directory_changed(app, item.target.identity(), &root);
    }
    result
}

fn apply_attributes(item: &WriteHandle, sftp: &ssh2::Sftp, path: &str, entry: &Entry) {
    let result = (|| {
        let before = sftp.lstat(Path::new(path))?;
        if before.file_type().is_symlink() {
            return Err(AppError::new("changed", "目标路径变为符号链接"));
        }
        sftp.setstat(
            Path::new(path),
            ssh2::FileStat {
                size: None,
                uid: None,
                gid: None,
                perm: entry.mode.map(|m| m & 0o777),
                atime: entry.modified.map(|_| before.atime.unwrap_or(0)),
                mtime: entry.modified,
            },
        )?;
        Ok::<_, AppError>(())
    })();
    if let Err(e) = result {
        item.warn(format!(
            "内容已复制，但属性未完整保留：{path}（{}）",
            e.message
        ));
    }
}

fn destination_is_completed(entry: &Entry) -> bool {
    entry.outcome == "completed"
}

// Only a validated, fully received stage can enter the publication path.
struct VerifiedStage<'a> {
    stage: &'a Stage,
}
impl<'a> VerifiedStage<'a> {
    fn verify(sftp: &ssh2::Sftp, stage: &'a Stage, expected_size: u64) -> AppResult<Self> {
        remote_write::safe_ancestors(sftp, &stage.path)?;
        if !remote_write::metadata(sftp, &stage.path)?
            .is_some_and(|s| s.is_file() && s.size == Some(expected_size))
        {
            return Err(AppError::new("changed", "临时文件大小或类型不符合传输计划"));
        }
        Ok(Self { stage })
    }
    fn publish(
        &self,
        session: &ssh2::Session,
        sftp: &ssh2::Sftp,
        requested: &str,
        policy: ConflictPolicy,
        before_commit: impl FnMut(&str) -> AppResult<()>,
    ) -> AppResult<Option<String>> {
        remote_write::publish(
            session,
            sftp,
            &self.stage.path,
            requested,
            policy,
            before_commit,
        )
    }
}
fn create_stage(
    state: &State,
    item: &WriteHandle,
    sftp: &ssh2::Sftp,
    parent: &str,
) -> AppResult<Stage> {
    Stage::create(sftp, parent, |directory| {
        item.receipt
            .lock()
            .unwrap()
            .stage_dirs
            .push(directory.into());
        state.transfers.persist(item)
    })
}
fn record_commit(
    state: &State,
    item: &WriteHandle,
    destination: &str,
    stage_path: Option<&str>,
    expected_size: Option<u64>,
) -> AppResult<()> {
    let run_id = item.task.lock().unwrap().run_id.clone();
    item.receipt.lock().unwrap().pending_commit = Some(PendingCommit {
        destination: destination.into(),
        stage_path: stage_path.map(str::to_owned),
        expected_size,
        run_id,
    });
    state.transfers.persist(item)
}

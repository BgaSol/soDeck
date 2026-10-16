use super::{
    model::{Attempt, PendingCommit, Source, WriteHandle},
    planner::{self, Entry, Plan},
    ConflictPolicy,
};
use crate::{
    connection,
    model::{AppError, AppResult},
    remote_path, remote_write, State,
};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

pub fn run(state: &State, app: Option<&tauri::AppHandle>, item: &WriteHandle) -> AppResult<()> {
    item.check()?;
    item.task.lock().unwrap().status = "checking".into();
    let requested = item.task.lock().unwrap().target_path.clone();
    let target = connection::connect_verified(&item.target, &item.cancel, crate::traffic::TrafficPurpose::Transfer)?;
    target.session().set_timeout(15000);
    let sftp = target.session().sftp()?;
    let root = sftp
        .realpath(Path::new(&requested))?
        .to_str()
        .ok_or_else(|| AppError::new("path", "目标不是 UTF-8"))?
        .to_owned();
    if !sftp.stat(Path::new(&root))?.is_dir() {
        return Err(AppError::new("fileType", "目标不是目录"));
    }
    remote_write::safe_ancestors(&sftp, &requested)?;
    if root != requested {
        return Err(AppError::new("path", "目标路径已变化，请刷新目录后重试"));
    }
    if item.same_server() {
        super::same_server::validate(item, &sftp, &root)?;
    }
    item.task.lock().unwrap().target_path = root.clone();
    item.task.lock().unwrap().status = "scanning".into();
    let previous = item.plan.lock().unwrap().clone();
    let mut plan = match previous {
        Some(p) if !p.entries.is_empty() => p,
        _ => planner::scan(item)?,
    };
    plan_targets(item, &sftp, &root, &mut plan)?;
    let task_id = item.task.lock().unwrap().id.clone();
    let mut claims: Vec<_> = plan
        .entries
        .iter()
        .filter(|e| e.parts.len() == 1)
        .filter_map(|e| e.target.as_ref())
        .map(|p| super::scheduler::Claim {
            key: item.write_key(),
            path: p.clone(),
            write: true,
        })
        .collect();
    let source_claim_start = claims.len();
    if let Source::Remote { context, paths } = &item.source {
        claims.extend(paths.iter().map(|p| super::scheduler::Claim {
            key: context.target_public_key().into(),
            path: p.clone(),
            write: item.moving(),
        }));
    }
    let _paths = state
        .transfers
        .paths
        .acquire_many(&claims, &task_id, |conflict| {
            item.check()?;
            item.task.lock().unwrap().waiting_reason =
                conflict.map(|(path, task)| format!("等待任务 {task} 释放路径：{path}"));
            Ok(())
        })?;
    for claim in claims.iter().skip(source_claim_start) {
        if let Some(task) = state
            .transfers
            .pending_path(&claim.key, &claim.path, Some(&task_id))
        {
            return Err(AppError::new(
                "conflictPending",
                format!("来源涉及待核对任务 {task}，请先核对结果"),
            ));
        }
    }
    let _editors = if item.moving() {
        let Source::Remote { context, paths } = &item.source else {
            unreachable!()
        };
        Some(state.files.begin_move(context.target_public_key(), paths)?)
    } else {
        None
    };
    remote_write::safe_ancestors(&sftp, &root)?;
    {
        let mut t = item.task.lock().unwrap();
        t.files_total = plan.entries.iter().filter(|e| !e.directory).count() as u64;
        t.bytes_total = plan.entries.iter().map(|e| e.size).sum();
        t.committed_bytes = plan
            .entries
            .iter()
            .filter(|e| e.committed())
            .map(|e| e.size)
            .sum();
        t.bytes_done = t.committed_bytes;
        t.files_done = plan
            .entries
            .iter()
            .filter(|e| !e.directory && e.outcome == "completed")
            .count() as u64;
        t.skipped = plan.skipped.len() as u64
            + plan
                .entries
                .iter()
                .filter(|e| e.outcome == "skipped")
                .count() as u64;
        t.warnings = plan
            .skipped
            .iter()
            .take(100)
            .map(|p| format!("跳过链接或特殊文件：{p}"))
            .collect();
    }
    state.transfers.save_plan(item, &plan)?;
    state.transfers.persist(item)?;
    if plan
        .entries
        .iter()
        .all(|e| e.committed() || e.outcome == "skipped")
    {
        if item.moving() {
            super::same_server::finish_move(state, app, item, &target, &mut plan)?;
        }
        return Ok(());
    }
    let source_check = super::verify::SourceCheck::new(item)?;
    let strong = item.receipt.lock().unwrap().verify_sha256;
    if strong {
        source_check.check_hash_tool()?;
        super::verify::check_hash_tool(&target, &item.cancel)?;
    }
    for entry in plan
        .entries
        .iter_mut()
        .filter(|e| !e.committed() && e.outcome != "skipped")
    {
        source_check.unchanged(entry)?;
        if item.moving() {
            let now = super::same_server::stamp(&target, item, &entry.source)?;
            if entry.source_stamp.as_ref().is_some_and(|old| old != &now) {
                return Err(AppError::new("changed", "来源身份或内容已变化，请重新选择"));
            }
            entry.source_stamp = Some(now);
        }
        if strong && !entry.directory {
            entry.digest = Some(source_check.digest(entry)?);
        }
        if item.moving()
            && !entry.directory
            && super::same_server::same_filesystem(
                &target,
                item,
                &sftp,
                &entry.source,
                entry.target.as_deref().unwrap(),
            )?
        {
            entry.staged = Some(entry.source.clone());
        }
    }
    state.transfers.save_plan(item, &plan)?;
    if item.moving() {
        super::same_server::move_directories(state, app, item, &target, &mut plan)?;
    }
    let groups = staging_groups(state, item, &sftp, &mut plan)?;
    item.task.lock().unwrap().status = "running".into();
    let mut transferred = item.task.lock().unwrap().committed_bytes;
    for (index, entries) in groups {
        if item.same_server() {
            super::same_server::transfer(state, item, &target, &mut plan, index, &entries)?;
        } else if matches!(item.source, Source::Remote { .. }) {
            let mut executor = super::direct::Executor::prepare(state, item, index)?;
            executor.transfer(state, item, &mut plan, index, &entries)?;
        } else {
            for i in entries {
                item.check()?;
                let entry = &mut plan.entries[i];
                let path = entry.staged.clone().unwrap();
                entry.error = None;
                {
                    let mut t = item.task.lock().unwrap();
                    t.current_file = Some(entry.source.clone());
                    t.bytes_done = transferred;
                }
                if let Err(e) = super::upload::upload_file(item, &sftp, entry, &path) {
                    entry.error = Some(e.message.clone());
                    state.transfers.save_entry(item, i, entry)?;
                    return Err(e);
                }
                transferred += entry.size;
                state.transfers.save_entry(item, i, entry)?;
            }
            let mut r = item.receipt.lock().unwrap();
            r.attempts[index].source_finished = true;
            r.attempts[index].receiver_finished = true;
            r.attempts[index].successful = true;
        }
        state.transfers.persist(item)?;
    }
    item.task.lock().unwrap().status = "verifying".into();
    let mut invalid = false;
    for (i, entry) in plan
        .entries
        .iter_mut()
        .enumerate()
        .filter(|(_, e)| !e.directory && !e.committed() && e.outcome != "skipped")
    {
        let result = (|| {
            item.check()?;
            source_check.unchanged(entry)?;
            let staged = entry
                .staged
                .as_deref()
                .ok_or_else(|| AppError::new("verification", "缺少暂存文件"))?;
            if !remote_write::metadata(&sftp, staged)?
                .is_some_and(|m| m.is_file() && m.size == Some(entry.size))
            {
                return Err(AppError::new(
                    "verification",
                    "暂存文件类型或大小与计划不一致",
                ));
            }
            if strong {
                let source = source_check.digest(entry)?;
                let received =
                    super::verify::remote_digest_checked(&target, staged, &item.cancel, || {
                        item.check()
                    })?;
                if source != received || entry.digest.as_ref() != Some(&source) {
                    return Err(AppError::new("verification", "SHA-256 不一致"));
                }
                entry.digest = Some(source);
            }
            if staged != entry.source {
                attributes(item, &sftp, staged, entry);
            }
            Ok(())
        })();
        if let Err(e) = result {
            item.check()?;
            entry.error = Some(e.message.clone());
            invalid = true;
        }
        state.transfers.save_entry(item, i, entry)?;
    }
    state.transfers.persist(item)?;
    if invalid {
        return Err(AppError::new(
            "verification",
            "部分源文件发生变化或校验失败，本次内容尚未提交。请查看项目详情",
        ));
    }
    item.task.lock().unwrap().status = "committing".into();
    let policy = item.task.lock().unwrap().conflict;
    let extra_guards = std::cell::RefCell::new(Vec::new());
    for i in 0..plan.entries.len() {
        item.check()?;
        if plan.entries[i].committed() || plan.entries[i].outcome == "skipped" {
            continue;
        }
        let mut entry = plan.entries[i].clone();
        let requested = entry.target.clone().unwrap();
        let task_id = item.task.lock().unwrap().id.clone();
        let _guard =
            state
                .transfers
                .paths
                .acquire(&item.write_key(), &requested, &task_id, |conflict| {
                    item.check()?;
                    if let Some((p, t)) = conflict {
                        return Err(AppError::new(
                            "busy",
                            format!("改名后的目标路径 {p} 正由任务 {t} 使用，请重试"),
                        ));
                    }
                    Ok(())
                })?;
        let merge_only = entry.directory
            && policy != ConflictPolicy::Rename
            && remote_write::metadata(&sftp, &requested)?.is_some_and(|s| s.is_dir());
        if let Some(task) = (!merge_only)
            .then(|| state.transfers.conflicting_commit(item, &requested))
            .flatten()
        {
            return Err(AppError::new(
                "conflictPending",
                format!(
                    "目标 {requested} 的提交结果需核对（任务 {task}），可改名或更换目录创建任务"
                ),
            ));
        }
        item.task.lock().unwrap().waiting_reason = None;
        let before = |path: &str| {
            item.check()?;
            if item.same_server() {
                source_check.unchanged(&entry)?;
            }
            if item.moving()
                && entry.staged.as_deref() == Some(entry.source.as_str())
                && entry.source_stamp.as_ref()
                    != Some(&super::same_server::stamp(&target, item, &entry.source)?)
            {
                return Err(AppError::new("changed", "来源在移动前变化，请重新选择"));
            }
            if path != requested {
                extra_guards
                    .borrow_mut()
                    .push(state.transfers.paths.acquire(
                        &item.write_key(),
                        path,
                        &task_id,
                        |conflict| {
                            item.check()?;
                            if conflict.is_some() {
                                return Err(AppError::new(
                                    "busy",
                                    "自动改名目标正被其他任务使用，请重试",
                                ));
                            }
                            Ok(())
                        },
                    )?);
            }
            if let Some(task) = state.transfers.conflicting_commit(item, path) {
                return Err(AppError::new(
                    "conflictPending",
                    format!("目标 {path} 的提交结果需核对（任务 {task}）"),
                ));
            }
            if policy == ConflictPolicy::Overwrite && !entry.directory {
                let now = remote_write::metadata(&sftp, path)?
                    .as_ref()
                    .map(planner::Version::from);
                if now != entry.target_version {
                    return Err(AppError::new(
                        "conflict",
                        format!("目标自扫描后已变化，保留原文件；请重新创建任务确认覆盖：{path}"),
                    ));
                }
            }
            item.receipt.lock().unwrap().pending_commit = Some(PendingCommit {
                rename_directory: false,
                destination: path.into(),
                stage_path: entry.staged.clone(),
                expected_size: (!entry.directory).then_some(entry.size),
                run_id: item.task.lock().unwrap().run_id.clone(),
                entry: i,
                digest: entry.digest.clone(),
                directory: entry.directory,
            });
            state.transfers.persist(item)
        };
        let mut created = false;
        let result = if entry.directory {
            remote_write::directory_destination(&sftp, &requested, policy, before).map(|d| {
                d.map(|d| {
                    created = d.created;
                    d.path
                })
            })
        } else {
            remote_write::publish(
                target.session(),
                &sftp,
                entry.staged.as_ref().unwrap(),
                &requested,
                policy,
                before,
            )
        };
        match result {
            Ok(destination) => {
                let mut rebased = false;
                if entry.directory {
                    entry.created_directory = created;
                    if let Some(new) = &destination {
                        if new != &requested {
                            for child in plan.entries.iter_mut().skip(i + 1) {
                                if let Some(p) = &child.target {
                                    if let Some(suffix) = p.strip_prefix(&format!("{requested}/")) {
                                        child.target = Some(format!("{new}/{suffix}"));
                                        rebased = true;
                                    }
                                }
                            }
                        }
                    }
                    if destination.is_none() {
                        for child in plan.entries.iter_mut().skip(i + 1) {
                            if child
                                .target
                                .as_ref()
                                .is_some_and(|p| p.starts_with(&format!("{requested}/")))
                            {
                                child.outcome = "skipped".into();
                                rebased = true;
                            }
                        }
                    }
                }
                entry.outcome = if destination.is_some() {
                    if item.moving()
                        && (entry.directory
                            || entry.staged.as_deref() != Some(entry.source.as_str()))
                    {
                        "copied"
                    } else {
                        "completed"
                    }
                } else {
                    "skipped"
                }
                .into();
                entry.target = destination;
                entry.error = None;
                plan.entries[i] = entry.clone();
                if rebased {
                    state.transfers.save_plan(item, &plan)?;
                } else {
                    state.transfers.save_entry(item, i, &entry)?;
                }
                item.receipt.lock().unwrap().pending_commit = None;
                {
                    let mut t = item.task.lock().unwrap();
                    if !entry.directory && entry.committed() {
                        if entry.outcome == "completed" {
                            t.files_done += 1;
                        }
                        t.committed_bytes += entry.size;
                    }
                    if entry.outcome == "skipped" {
                        t.skipped += 1;
                    }
                }
                state.transfers.persist(item)?;
            }
            Err(e) => {
                entry.error = Some(e.message.clone());
                state.transfers.save_entry(item, i, &entry)?;
                if e.kind != "unknown" {
                    item.receipt.lock().unwrap().pending_commit = None;
                }
                state.transfers.persist(item)?;
                return Err(e);
            }
        }
        if let Some(app) = app {
            super::directory_changed(app, item.target.identity(), remote_path::parent(&requested));
            if item.moving() {
                super::directory_changed(
                    app,
                    item.target.identity(),
                    remote_path::parent(&entry.source),
                );
            }
        }
    }
    for entry in plan
        .entries
        .iter()
        .rev()
        .filter(|e| e.directory && e.created_directory)
    {
        if let Some(path) = &entry.target {
            attributes(item, &sftp, path, entry);
        }
    }
    {
        let mut t = item.task.lock().unwrap();
        t.bytes_done = t.bytes_total;
        t.committed_bytes = plan
            .entries
            .iter()
            .filter(|e| e.committed())
            .map(|e| e.size)
            .sum();
        t.current_file = None;
        t.current_bytes = 0;
    }
    if item.moving() {
        super::same_server::finish_move(state, app, item, &target, &mut plan)?;
    }
    Ok(())
}
fn plan_targets(
    item: &WriteHandle,
    sftp: &ssh2::Sftp,
    root: &str,
    plan: &mut Plan,
) -> AppResult<()> {
    let policy = item.task.lock().unwrap().conflict;
    let mut dirs = HashMap::<String, String>::new();
    let mut skipped_dirs = HashSet::new();
    let mut reserved = HashSet::new();
    for entry in &mut plan.entries {
        item.check()?;
        let source_parent = match &item.source {
            Source::Local(_) => Path::new(&entry.source)
                .parent()
                .and_then(|p| p.to_str())
                .unwrap_or(""),
            Source::Remote { .. } => remote_path::parent(&entry.source),
        };
        if skipped_dirs.contains(source_parent) {
            entry.outcome = "skipped".into();
            if entry.directory {
                skipped_dirs.insert(entry.source.clone());
            }
            continue;
        }
        let parent = dirs.get(source_parent).map(String::as_str).unwrap_or(root);
        let requested = remote_path::join(parent, entry.parts.last().unwrap())?;
        if !entry.committed() && entry.outcome != "skipped" && entry.target.is_none() {
            let target = if policy == ConflictPolicy::Rename {
                let mut suffix = 0;
                loop {
                    let p = if suffix == 0 {
                        requested.clone()
                    } else {
                        remote_path::join(
                            parent,
                            &remote_path::renamed(
                                entry.parts.last().unwrap(),
                                entry.directory,
                                suffix,
                            ),
                        )?
                    };
                    if !reserved.contains(&p) && remote_write::metadata(sftp, &p)?.is_none() {
                        break p;
                    }
                    suffix += 1;
                    if suffix > 100_000 {
                        return Err(AppError::new("conflict", "无法分配名称"));
                    }
                }
            } else {
                requested
            };
            let stat = remote_write::metadata(sftp, &target)?;
            if reserved.contains(&target) {
                if policy == ConflictPolicy::Skip {
                    entry.outcome = "skipped".into();
                } else {
                    return Err(AppError::new(
                        "conflict",
                        format!("多个来源映射到同一目标，请使用自动改名：{target}"),
                    ));
                }
            }
            entry.target_version = stat.as_ref().map(planner::Version::from);
            if policy == ConflictPolicy::Skip
                && stat
                    .as_ref()
                    .is_some_and(|m| !(entry.directory && m.is_dir()))
            {
                entry.outcome = "skipped".into();
                if entry.directory {
                    skipped_dirs.insert(entry.source.clone());
                }
            }
            if policy == ConflictPolicy::Overwrite
                && stat.as_ref().is_some_and(|m| {
                    if entry.directory {
                        !m.is_dir()
                    } else {
                        !m.is_file()
                    }
                })
            {
                return Err(AppError::new("conflict", format!("目标类型冲突：{target}")));
            }
            entry.created_directory = entry.directory && stat.is_none();
            entry.target = Some(target);
            entry.error = None;
        }
        if let Some(target) = &entry.target {
            reserved.insert(target.clone());
            if entry.directory {
                dirs.insert(entry.source.clone(), target.clone());
            }
        }
    }
    Ok(())
}
fn attributes(item: &WriteHandle, sftp: &ssh2::Sftp, path: &str, entry: &Entry) {
    if let Err(e) = sftp.setstat(
        Path::new(path),
        ssh2::FileStat {
            size: None,
            uid: None,
            gid: None,
            perm: entry.mode.map(|m| m & 0o777),
            atime: entry.modified,
            mtime: entry.modified,
        },
    ) {
        item.warn(format!("内容已接收，部分属性未保存：{path}（{e}）"));
    }
}

// Place a separate private root on each destination filesystem. This also
// handles directory merges that traverse mount points; final rename stays local.
fn staging_groups(
    state: &State,
    item: &WriteHandle,
    sftp: &ssh2::Sftp,
    plan: &mut Plan,
) -> AppResult<Vec<(usize, Vec<usize>)>> {
    let mut groups = HashMap::<String, (String, Vec<usize>)>::new();
    let mut parents = HashMap::<String, (String, String)>::new();
    for (i, e) in plan.entries.iter().enumerate().filter(|(_, e)| {
        !e.directory
            && !e.committed()
            && e.outcome != "skipped"
            && e.staged.as_deref() != Some(e.source.as_str())
    }) {
        item.check()?;
        let parent = remote_path::parent(e.target.as_ref().unwrap()).to_owned();
        let (key, existing) = if let Some(cached) = parents.get(&parent) {
            cached.clone()
        } else {
            let mut existing = parent.clone();
            loop {
                match remote_write::metadata(sftp, &existing)? {
                    Some(stat) if stat.is_dir() => break,
                    Some(_) => {
                        return Err(AppError::new(
                            "conflict",
                            format!("目标父路径不是目录：{existing}"),
                        ))
                    }
                    None => existing = remote_path::parent(&existing).into(),
                }
            }
            remote_write::safe_ancestors(sftp, &existing)?;
            // If the server lacks statvfs, grouping by existing parent is safe.
            let key = sftp
                .opendir(Path::new(&existing))
                .and_then(|mut f| f.statvfs())
                .ok()
                .filter(|v| v.f_fsid != 0)
                .map(|v| format!("fs:{}", v.f_fsid))
                .unwrap_or_else(|| existing.clone());
            parents.insert(parent, (key.clone(), existing.clone()));
            (key, existing)
        };
        let group = groups.entry(key).or_insert_with(|| (existing, vec![]));
        group.1.push(i);
    }
    let mut groups = groups.into_values().collect::<Vec<_>>();
    groups.sort_by(|a, b| a.0.cmp(&b.0));
    let mut result = vec![];
    for (parent, entries) in groups {
        if let Ok(vfs) = sftp
            .opendir(Path::new(&parent))
            .and_then(|mut f| f.statvfs())
        {
            let required: u64 = entries.iter().map(|&i| plan.entries[i].size).sum();
            if vfs.f_bavail.saturating_mul(vfs.f_frsize) < required {
                return Err(AppError::new(
                    "space",
                    format!("目标可用空间不足以暂存本次内容：{parent}"),
                ));
            }
        }
        let reusable = if matches!(item.source, Source::Remote { .. }) {
            plan.entries[entries[0]].staged.as_ref().and_then(|p| {
                item.receipt.lock().unwrap().attempts.iter().position(|a| {
                    a.source_finished
                        && a.receiver_finished
                        && !a.target_root.is_empty()
                        && p.starts_with(&format!("{}/", a.target_root))
                })
            })
        } else {
            None
        };
        let target_root = if let Some(index) = reusable {
            let old = item.receipt.lock().unwrap().attempts[index]
                .target_root
                .clone();
            for name in [
                "probe",
                "receiver.exit",
                "receiver.exit.next",
                "copy.sh",
                "copy.exit",
                "copy.exit.next",
            ] {
                let p = format!("{old}/{name}");
                if remote_write::metadata(sftp, &p)?.is_some() {
                    sftp.unlink(Path::new(&p))?;
                }
            }
            item.receipt.lock().unwrap().attempts[index]
                .target_root
                .clear();
            old
        } else {
            remote_path::join(
                &parent,
                &format!(".ssh-workspace-transfer-{}", uuid::Uuid::new_v4()),
            )?
        };
        let index = {
            let mut r = item.receipt.lock().unwrap();
            r.attempts.push(Attempt {
                id: uuid::Uuid::new_v4().to_string(),
                target_root: target_root.clone(),
                ..Attempt::default()
            });
            r.attempts.len() - 1
        };
        state.transfers.persist(item)?;
        if reusable.is_none() {
            sftp.mkdir(Path::new(&target_root), 0o700)?;
            sftp.mkdir(Path::new(&format!("{target_root}/data")), 0o700)?;
        }
        for &i in &entries {
            let name = match &item.source {
                Source::Local(_) => i.to_string(),
                Source::Remote { .. } => plan.entries[i].source.trim_start_matches('/').into(),
            };
            plan.entries[i].staged = Some(format!("{target_root}/data/{name}"));
        }
        result.push((index, entries));
    }
    state.transfers.save_plan(item, plan)?;
    Ok(result)
}

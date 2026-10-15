use super::{
    direct::remote_job,
    model::{Source, WriteHandle},
};
use crate::{
    connection,
    model::{AppError, AppResult},
    remote_path, remote_write,
    ssh_transport::Cancellation,
};

pub(super) fn valid_task_directory(path: &str, prefix: &str) -> AppResult<()> {
    let name = remote_path::name(path)?;
    if name
        .strip_prefix(prefix)
        .is_none_or(|id| uuid::Uuid::parse_str(id).is_err())
    {
        return Err(AppError::new("path", "任务临时目录标识无效，拒绝清理"));
    }
    Ok(())
}
pub(super) fn verify_stopped(item: &WriteHandle) -> AppResult<()> {
    let _proof = remote_job::stop_and_verify(item)?;
    Ok(())
}

pub(super) fn cleanup(item: &WriteHandle) -> AppResult<()> {
    // The cleanup entry point itself requires a proof, regardless of caller.
    let _proof = remote_job::stop_and_verify(item)?;
    let receipt = item.receipt.lock().unwrap().clone();
    if !receipt.stage_dirs.is_empty() {
        let connection = connection::connect_verified(&item.target, &Cancellation::default())?;
        let sftp = connection.session().sftp()?;
        for directory in receipt.stage_dirs {
            valid_task_directory(&directory, ".ssh-workspace-")?;
            remote_write::safe_ancestors(&sftp, &directory)?;
            if remote_write::metadata(&sftp, &directory)?.is_some() {
                remote_write::Stage {
                    path: format!("{directory}/content"),
                    directory: directory.clone(),
                }
                .cleanup(&sftp)?;
            }
            item.receipt
                .lock()
                .unwrap()
                .stage_dirs
                .retain(|d| d != &directory);
        }
    }
    if let Some(direct) = receipt.direct {
        let Source::Remote { context, .. } = &item.source else {
            return Err(AppError::new("unknown", "直传来源记录缺失"));
        };
        for (source, spec, job) in [
            (true, context, &direct.source),
            (false, &item.target, &direct.target),
        ] {
            if job.root.is_empty() {
                continue;
            }
            let connection = connection::connect_verified(spec, &Cancellation::default())?;
            remote_job::remove_root(
                &connection.session().sftp()?,
                job,
                direct.attempt.as_deref(),
            )?;
            if let Some(d) = item.receipt.lock().unwrap().direct.as_mut() {
                if source {
                    d.source.root.clear();
                } else {
                    d.target.root.clear();
                }
            }
        }
        item.receipt.lock().unwrap().direct = None;
    }
    Ok(())
}

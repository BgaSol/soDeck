use super::{executor, model::*};
use crate::{
    connection::SharedContext,
    model::{AppError, AppResult},
    ssh, State,
};
use std::{
    io::Write,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

pub struct PrivateConfig {
    pub prefix: String,
    pub content: String,
    pub suffix: String,
}
pub struct Prepared {
    pub name: String,
    pub fingerprint: String,
    pub consequences: Vec<String>,
    pub command: String,
    pub batch: Option<Vec<super::batch::Step>>,
    pub private_config: Option<PrivateConfig>,
    pub timeout: u64,
}
pub fn prepare(
    state: &State,
    spec: &SharedContext,
    session: &ssh2::Session,
    engine: &EngineBinding,
    id: &str,
    action: &Action,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<Prepared> {
    match action {
        Action::Batch { entries } => {
            super::batch::prepare(session, &engine.endpoint, entries, check)
        }
        Action::VolumeCreate
        | Action::VolumeRemove
        | Action::NetworkCreate
        | Action::NetworkRemove
        | Action::NetworkConnect { .. }
        | Action::NetworkDisconnect { .. }
        | Action::CachePrune => {
            super::resources::prepare(session, &engine.endpoint, id, action, check)
        }
        Action::Compose {
            mode,
            association_id,
            revision,
        } => super::compose::prepare(
            state,
            spec,
            session,
            engine,
            id,
            mode,
            association_id,
            *revision,
            check,
        ),
        _ => {
            let (name, fingerprint, consequences) =
                super::images::prepare(session, &engine.endpoint, id, action, check)?;
            Ok(Prepared {
                name,
                fingerprint,
                consequences,
                command: action.command(&engine.endpoint, id)?,
                private_config: None,
                batch: None,
                timeout: 1800,
            })
        }
    }
}
pub fn execute(
    session: &ssh2::Session,
    plan: Prepared,
    attempted: &AtomicBool,
    check: &impl Fn() -> AppResult<()>,
    progress: &impl Fn(&str),
    item: &impl Fn(usize, super::batch::Item),
) -> AppResult<ssh::ExecResult> {
    if let Some(steps) = plan.batch {
        return super::batch::execute(session, steps, attempted, check, item);
    }
    if let Some(config) = plan.private_config {
        let path = format!(
            "/tmp/.sodeck-compose-{}.json",
            uuid::Uuid::new_v4().simple()
        );
        check()?;
        let sftp = session.sftp()?;
        let mut file = sftp.open_mode(
            Path::new(&path),
            ssh2::OpenFlags::WRITE | ssh2::OpenFlags::CREATE | ssh2::OpenFlags::EXCLUSIVE,
            0o600,
            ssh2::OpenType::File,
        )?;
        let written = file.write_all(config.content.as_bytes());
        drop(file);
        if let Err(e) = written {
            let _ = sftp.unlink(Path::new(&path));
            return Err(e.into());
        }
        let cleanup = format!(
            "rm -f -- {} || printf '__SODECK_TEMP_CLEANUP_FAILED__\\n' >&2",
            ssh::quote(&path)
        );
        let inner = format!(
            "trap {} EXIT; {} --file {} {}",
            ssh::quote(&cleanup),
            config.prefix,
            ssh::quote(&path),
            config.suffix
        );
        let result = executor::execute_progress(
            session,
            &format!("sh -c {}", ssh::quote(&inner)),
            Duration::from_secs(plan.timeout),
            attempted,
            check,
            &|_| {},
        );
        match result {
            Ok(mut result) => {
                if result.stderr.contains("__SODECK_TEMP_CLEANUP_FAILED__") {
                    result.stderr = format!("临时配置清理未完成：{path}，请在远端操作结束后清理。");
                } else if result.code != 0 {
                    // Compose can echo interpolated environment values on errors.
                    result.stderr = format!("Compose 返回退出码 {}。请检查项目配置和服务日志；已完成的阶段不会自动回滚。", result.code);
                } else {
                    result.stderr.clear();
                }
                result.stdout.clear();
                Ok(result)
            }
            Err(error) => {
                if !attempted.load(Ordering::SeqCst) {
                    let _ = sftp.unlink(Path::new(&path));
                }
                Err(AppError::new(&error.kind, format!("{}；临时配置 {path} 由远端命令结束时清理。若连接中断使清理未完成，请在核对远端操作后清理。", error.message)))
            }
        }
    } else {
        executor::execute_progress(
            session,
            &plan.command,
            Duration::from_secs(plan.timeout),
            attempted,
            check,
            progress,
        )
    }
}
pub fn observe(
    session: &ssh2::Session,
    endpoint: &str,
    id: &str,
    action: &Action,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<String> {
    match action {
        Action::Batch { entries } => super::batch::observe(session, endpoint, entries, check),
        Action::VolumeCreate
        | Action::VolumeRemove
        | Action::NetworkCreate
        | Action::NetworkRemove
        | Action::NetworkConnect { .. }
        | Action::NetworkDisconnect { .. }
        | Action::CachePrune => super::resources::observe(session, endpoint, id, action, check),
        Action::Compose { .. } => super::compose::observe(session, endpoint, id, check),
        _ => super::images::observe(session, endpoint, id, action, check),
    }
}

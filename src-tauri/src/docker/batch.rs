use super::{executor, inspect, model::*, operations::Prepared};
use crate::{
    model::{AppError, AppResult},
    ssh,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Start,
    Stop,
    Restart,
    RemoveContainer,
    RemoveImage,
    RemoveVolume,
    RemoveNetwork,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub target_id: String,
    pub action: Kind,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub target_id: String,
    pub name: String,
    pub phase: String,
    pub message: String,
}
pub struct Step {
    entry: Entry,
    fingerprint: String,
    endpoint: String,
    name: String,
}
impl Entry {
    fn action(&self) -> Action {
        match self.action {
            Kind::Start => Action::Start,
            Kind::Stop => Action::Stop,
            Kind::Restart => Action::Restart,
            Kind::RemoveContainer => Action::Remove,
            Kind::RemoveImage => Action::ImageRemove { reference: None },
            Kind::RemoveVolume => Action::VolumeRemove,
            Kind::RemoveNetwork => Action::NetworkRemove,
        }
    }
}
pub fn validate(entries: &[Entry]) -> AppResult<()> {
    if entries.is_empty() || entries.len() > 100 {
        return Err(AppError::new("validation", "每次选择 1–100 个对象"));
    }
    let mut keys = std::collections::HashSet::new();
    for e in entries {
        e.action().validate_target(&e.target_id)?;
        if !keys.insert(&e.target_id) {
            return Err(AppError::new("validation", "批量操作包含重复对象"));
        }
    }
    Ok(())
}
fn step(
    session: &ssh2::Session,
    endpoint: &str,
    entry: &Entry,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<(String, String, String)> {
    let action = entry.action();
    if action.is_container() {
        let d = inspect::container(session, endpoint, &entry.target_id, check)?;
        let needed = action.check(&d)?;
        if d.auto_remove && matches!(action, Action::Stop | Action::Restart) {
            return Err(AppError::new(
                "confirmationRequired",
                format!("{} 启用了自动删除，请单独操作并确认影响", d.name),
            ));
        }
        Ok((
            d.name.clone(),
            inspect::fingerprint(&d),
            if needed {
                action.command(endpoint, &entry.target_id)?
            } else {
                String::new()
            },
        ))
    } else if matches!(action, Action::ImageRemove { .. }) {
        let (name, fp, _) =
            super::images::prepare(session, endpoint, &entry.target_id, &action, check)?;
        Ok((name, fp, action.command(endpoint, &entry.target_id)?))
    } else {
        let p = super::resources::prepare(session, endpoint, &entry.target_id, &action, check)?;
        Ok((p.name, p.fingerprint, p.command))
    }
}
pub fn prepare(
    session: &ssh2::Session,
    endpoint: &str,
    entries: &[Entry],
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<Prepared> {
    validate(entries)?;
    let mut steps = vec![];
    let mut messages=vec![format!("仅处理下列 {} 个已选对象，不自动加入之后出现的对象。每项单独反馈，允许部分失败；失败后不升级为强制操作。",entries.len())];
    for e in entries {
        let (name, fp, _) = step(session, endpoint, e, check)?;
        messages.push(format!(
            "{} · {} · {}",
            e.action().label(),
            name,
            e.target_id
        ));
        steps.push(Step {
            entry: e.clone(),
            fingerprint: fp,
            endpoint: endpoint.into(),
            name,
        });
    }
    if entries
        .iter()
        .any(|e| matches!(e.action, Kind::RemoveVolume))
    {
        messages.push("选中的数据卷将永久删除，其中数据无法恢复。".into());
    }
    if entries
        .iter()
        .any(|e| matches!(e.action, Kind::RemoveContainer))
    {
        messages.push("删除容器及可写层，保留其挂载卷和镜像。".into());
    }
    messages.push("共享镜像层不按逐项大小累加，释放空间以刷新后的引擎报告为准。".into());
    let fingerprint = format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(
                &steps
                    .iter()
                    .map(|s| (&s.entry, &s.fingerprint))
                    .collect::<Vec<_>>()
            )
            .unwrap()
        )
    );
    Ok(Prepared {
        name: format!("{} 个对象", entries.len()),
        fingerprint,
        consequences: messages,
        command: String::new(),
        private_config: None,
        batch: Some(steps),
        timeout: 600,
    })
}
pub fn execute(
    session: &ssh2::Session,
    steps: Vec<Step>,
    attempted: &AtomicBool,
    check: &impl Fn() -> AppResult<()>,
    update: &impl Fn(usize, Item),
) -> AppResult<ssh::ExecResult> {
    let mut failed = 0;
    let total = steps.len();
    for (index, s) in steps.into_iter().enumerate() {
        check()?;
        let mut item = Item {
            target_id: s.entry.target_id.clone(),
            name: s.name.clone(),
            phase: "checking".into(),
            message: "重新核对原对象".into(),
        };
        update(index, item.clone());
        let command = match step(session, &s.endpoint, &s.entry, check) {
            Ok((_, fp, c)) if fp == s.fingerprint => c,
            result => {
                failed += 1;
                item.phase = "failed".into();
                item.message = match result {
                    Err(e) => e.message,
                    _ => "对象或使用关系已变化，已跳过".into(),
                };
                update(index, item);
                continue;
            }
        };
        if command.is_empty() {
            item.phase = "completed".into();
            item.message = "已是目标状态，无需执行".into();
            update(index, item);
            continue;
        }
        item.phase = "running".into();
        item.message = "正在执行".into();
        update(index, item.clone());
        match executor::execute_progress(
            session,
            &command,
            Duration::from_secs(600),
            attempted,
            check,
            &|_| {},
        ) {
            Ok(result) => {
                attempted.store(false, Ordering::SeqCst);
                item.phase = if result.code == 0 {
                    "completed"
                } else {
                    failed += 1;
                    "failed"
                }
                .into();
                item.message = if result.code == 0 {
                    "命令已完成".into()
                } else {
                    inspect::clean(&result.stderr, 2000)
                };
                update(index, item);
            }
            Err(e) => {
                item.phase = if attempted.load(Ordering::SeqCst) {
                    "unknown"
                } else {
                    "failed"
                }
                .into();
                item.message = e.message.clone();
                update(index, item);
                return Err(e);
            }
        }
    }
    Ok(ssh::ExecResult {
        code: if failed == 0 { 0 } else { 1 },
        stdout: format!("{} 项完成，{} 项未完成。", total - failed, failed),
        stderr: if failed > 0 {
            format!("{} 项未完成，请查看逐项结果。", failed)
        } else {
            String::new()
        },
    })
}
pub fn observe(
    session: &ssh2::Session,
    endpoint: &str,
    entries: &[Entry],
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<String> {
    let mut facts = vec![];
    for e in entries {
        let action = e.action();
        let result = if action.is_container() {
            inspect::container(session, endpoint, &e.target_id, check)
                .map(|d| format!("{}: {}", d.name, d.state))
        } else if matches!(action, Action::ImageRemove { .. }) {
            super::images::observe(session, endpoint, &e.target_id, &action, check)
        } else {
            super::resources::observe(session, endpoint, &e.target_id, &action, check)
        };
        facts.push(match result {
            Ok(v) => v,
            Err(error) if error.kind == "notFound" => format!("{} 已移除", e.target_id),
            Err(e) => return Err(e),
        });
    }
    Ok(inspect::clean(&facts.join("；"), 12000))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_selection_rejects_duplicate_objects_and_limits_scope() {
        let e = Entry {
            target_id: "a".repeat(64),
            action: Kind::RemoveContainer,
        };
        assert!(validate(&[]).is_err());
        assert!(validate(&[e.clone()]).is_ok());
        assert!(validate(&[e.clone(), e]).is_err());
        assert!(validate(&[Entry {
            target_id: "--all".into(),
            action: Kind::RemoveVolume
        }])
        .is_err());
    }
}

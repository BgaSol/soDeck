use super::{inspect, model::Action, query};
use crate::{
    model::{AppError, AppResult},
    remote_read, ssh,
};
use serde::Serialize;
use serde_json::Value;

// Variant is omitted for many images. Docker's strict raw JSON fallback needs
// index so an absent optional field becomes null instead of aborting inspect.
pub(super) const FORMAT: &str = r#"{"Id":{{json .Id}},"RepoTags":{{json .RepoTags}},"RepoDigests":{{json .RepoDigests}},"Created":{{json .Created}},"Os":{{json .Os}},"Architecture":{{json .Architecture}},"Variant":{{json (index . "Variant")}},"Size":{{json .Size}}}"#;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageDetail {
    pub id: String,
    pub tags: Vec<String>,
    pub digests: Vec<String>,
    pub created: String,
    pub platform: String,
    pub size: u64,
    pub containers: Vec<ImageContainer>,
}
#[derive(Clone, Debug, Serialize)]
pub struct ImageContainer {
    pub id: String,
    pub name: String,
    pub state: String,
}

pub fn validate_id(id: &str) -> AppResult<()> {
    if id
        .strip_prefix("sha256:")
        .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        Ok(())
    } else {
        Err(AppError::new("validation", "请使用完整的镜像 SHA256 ID"))
    }
}
pub fn validate_reference(value: &str) -> AppResult<()> {
    if value.is_empty()
        || value.len() > 512
        || !value.as_bytes()[0].is_ascii_alphanumeric()
        || !value.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b':' | b'/' | b'@')
        })
        || value.contains("://")
        || value.ends_with('/')
    {
        return Err(AppError::new(
            "validation",
            "请填写镜像名称、Tag 或 digest，不包含空格、协议或认证信息",
        ));
    }
    if let Some((name, digest)) = value.split_once('@') {
        if name.is_empty() || validate_id(digest).is_err() {
            return Err(AppError::new(
                "validation",
                "digest 必须为 sha256 与完整内容 ID",
            ));
        }
    }
    Ok(())
}
pub fn validate_tag(value: &str) -> AppResult<()> {
    validate_reference(value)?;
    if value.contains('@') || value.starts_with("sha256:") {
        return Err(AppError::new("validation", "Tag 不能使用内容 ID 或 digest"));
    }
    Ok(())
}
pub fn command(action: &Action, prefix: &str, id: &str) -> AppResult<String> {
    Ok(match action {
        Action::ImagePull => format!("{prefix} image pull {}", ssh::quote(id)),
        Action::ImageTag { reference } => format!(
            "{prefix} image tag {} {}",
            ssh::quote(id),
            ssh::quote(reference)
        ),
        Action::ImageRemove { reference } => format!(
            "{prefix} image rm --no-prune {}",
            ssh::quote(reference.as_deref().unwrap_or(id))
        ),
        _ => return Err(AppError::new("validation", "不支持的镜像操作")),
    })
}
fn inspect_value(
    session: &ssh2::Session,
    endpoint: &str,
    target: &str,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<Value> {
    validate_reference(target)?;
    let result = remote_read::exec(
        session,
        &format!(
            "{} image inspect --format {} {}",
            query::docker(endpoint),
            ssh::quote(FORMAT),
            ssh::quote(target)
        ),
        check,
    )?;
    if result.code != 0 {
        return Err(AppError::new(
            if result.stderr.to_lowercase().contains("no such") {
                "notFound"
            } else {
                "docker"
            },
            inspect::clean(&result.stderr, 2000),
        ));
    }
    let value: Value = serde_json::from_str(result.stdout.trim())
        .map_err(|_| AppError::new("format", "镜像详情格式不可用"))?;
    validate_id(value["Id"].as_str().unwrap_or_default())?;
    Ok(value)
}
pub fn resolve_local(
    session: &ssh2::Session,
    endpoint: &str,
    reference: &str,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<Option<String>> {
    match inspect_value(session, endpoint, reference, check) {
        Ok(v) => Ok(v["Id"].as_str().map(str::to_owned)),
        Err(e) if e.kind == "notFound" => Ok(None),
        Err(e) => Err(e),
    }
}
pub fn detail(
    session: &ssh2::Session,
    endpoint: &str,
    id: &str,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<ImageDetail> {
    validate_id(id)?;
    let v = inspect_value(session, endpoint, id, check)?;
    if v["Id"] != id {
        return Err(AppError::new("identity", "镜像身份已变化"));
    }
    let text = |k: &str| v[k].as_str().unwrap_or_default().to_owned();
    let strings = |k: &str| {
        let mut values: Vec<_> = v[k]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| s.as_str().map(str::to_owned))
            .collect();
        values.sort();
        values
    };
    let prefix = query::docker(endpoint);
    let result = remote_read::exec(
        session,
        &format!("{prefix} container ls -a -q --no-trunc"),
        check,
    )?;
    if result.code != 0 {
        return Err(AppError::new(
            "docker",
            inspect::clean(&result.stderr, 2000),
        ));
    }
    let ids: Vec<_> = result
        .stdout
        .lines()
        .filter(|s| !s.trim().is_empty())
        .collect();
    if ids.len() > 4000 {
        return Err(AppError::new(
            "limit",
            "容器数量过多，暂不能完整核对镜像使用关系",
        ));
    }
    for id in &ids {
        super::model::validate_id(id)?;
    }
    let mut containers = Vec::new();
    for chunk in ids.chunks(64) {
        let template = r#"{"Id":{{json .Id}},"Image":{{json .Image}},"Name":{{json .Name}},"State":{{json .State.Status}}}"#;
        let result = remote_read::exec(
            session,
            &format!(
                "{prefix} container inspect --format {} {}",
                ssh::quote(template),
                chunk
                    .iter()
                    .map(|id| ssh::quote(id))
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
            check,
        )?;
        if result.code != 0 {
            return Err(AppError::new(
                "conflict",
                "容器使用关系发生变化，请刷新镜像详情",
            ));
        }
        for line in result.stdout.lines() {
            let c: Value = serde_json::from_str(line)
                .map_err(|_| AppError::new("format", "镜像使用关系格式不可用"))?;
            if c["Image"] == id {
                containers.push(ImageContainer {
                    id: c["Id"].as_str().unwrap_or_default().into(),
                    name: c["Name"]
                        .as_str()
                        .unwrap_or_default()
                        .trim_start_matches('/')
                        .into(),
                    state: c["State"].as_str().unwrap_or_default().into(),
                });
            }
        }
    }
    containers.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(ImageDetail {
        id: id.into(),
        tags: strings("RepoTags"),
        digests: strings("RepoDigests"),
        created: text("Created"),
        platform: [text("Os"), text("Architecture"), text("Variant")]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("/"),
        size: v["Size"].as_u64().unwrap_or(0),
        containers,
    })
}
/// Includes stopped containers and the exact destination reference; a tag changing
/// between confirmation and execution invalidates the preview.
pub fn prepare(
    session: &ssh2::Session,
    endpoint: &str,
    id: &str,
    action: &Action,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<(String, String, Vec<String>)> {
    action.validate()?;
    action.validate_target(id)?;
    if matches!(action, Action::ImagePull) {
        return Ok((id.into(), String::new(), vec![]));
    }
    let d = detail(session, endpoint, id, check)?;
    let mut messages = vec![];
    let reference_id = match action {
        Action::ImageTag { reference } => {
            let existing = resolve_local(session, endpoint, reference, check)?;
            messages.push(format!("将 {} 指向此镜像。", reference));
            if let Some(previous) = &existing {
                messages.push(format!(
                    "该 Tag 已存在，当前指向 {previous}。确认后将替换此引用。"
                ));
            }
            existing
        }
        Action::ImageRemove { reference } => {
            if let Some(reference) = reference {
                let existing = resolve_local(session, endpoint, reference, check)?;
                if existing.as_deref() != Some(id) {
                    return Err(AppError::new("conflict", "此引用已不再指向选中的镜像"));
                }
                messages.push(format!(
                    "移除引用 {reference}；如果它是最后一个引用，Docker 也可能删除镜像内容。"
                ));
            } else {
                messages.push(
                    "按内容 ID 删除本地镜像；存在其他 Tag 或容器引用时可能被引擎拒绝。".into(),
                );
            }
            messages.push(format!("当前有 {} 个容器使用此内容（包含已停止容器）。不强制删除，不删除容器，不删除远端仓库内容。", d.containers.len()));
            None
        }
        _ => return Err(AppError::new("validation", "不支持的镜像操作")),
    };
    let name = d.tags.first().cloned().unwrap_or_else(|| id.into());
    Ok((
        name,
        serde_json::json!([d, reference_id]).to_string(),
        messages,
    ))
}
pub fn observe(
    session: &ssh2::Session,
    endpoint: &str,
    id: &str,
    action: &Action,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<String> {
    let target = match action {
        Action::ImageTag { reference } => reference.as_str(),
        Action::ImageRemove {
            reference: Some(reference),
        } => reference.as_str(),
        _ => id,
    };
    Ok(match resolve_local(session, endpoint, target, check)? {
        Some(content) => format!("当前引用指向 {content}"),
        None => "removed".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn references_cannot_inject_shell_or_flags() {
        for s in [
            "--all-tags",
            "repo;touch /tmp/x",
            "repo\nother",
            "https://host/repo",
            "user:password@host/repo",
            "$(whoami)",
        ] {
            assert!(validate_reference(s).is_err(), "{s}");
        }
        assert!(validate_reference("registry.example:5000/team/app:v1").is_ok());
        let action = Action::ImageRemove {
            reference: Some("app:v1".into()),
        };
        let command = action
            .command(
                "unix:///var/run/docker.sock",
                &format!("sha256:{}", "a".repeat(64)),
            )
            .unwrap();
        assert!(command.contains("image rm --no-prune 'app:v1'"));
        assert!(!command.contains("--force"));
    }
}

use super::{inspect, model::*, operations::Prepared, query};
use crate::{
    model::{AppError, AppResult},
    remote_read, ssh,
};
use serde::Serialize;
use serde_json::{json, Value};
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Resource {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub driver: String,
    pub created: String,
    pub protected: bool,
    pub mountpoint: String,
    pub containers: Vec<Reference>,
    pub configuration: Value,
}
#[derive(Clone, Serialize)]
pub struct Reference {
    pub id: String,
    pub name: String,
    pub state: String,
}
#[derive(Serialize)]
pub struct Overview {
    pub usage: Vec<Value>,
    pub volumes: Vec<Resource>,
    pub networks: Vec<Resource>,
    pub candidates: Vec<Candidate>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub id: String,
    pub name: String,
    pub kind: String,
}
pub fn name(value: &str) -> AppResult<()> {
    if value.is_empty()
        || value.len() > 255
        || !value.as_bytes()[0].is_ascii_alphanumeric()
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
    {
        return Err(AppError::new("validation", "资源名称无效"));
    }
    Ok(())
}
fn read(
    session: &ssh2::Session,
    command: &str,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<String> {
    let r = remote_read::exec(session, command, check)?;
    if r.code != 0 {
        return Err(AppError::new(
            if r.stderr.to_lowercase().contains("no such") {
                "notFound"
            } else {
                "docker"
            },
            inspect::clean(&r.stderr, 2000),
        ));
    }
    Ok(r.stdout)
}
pub fn detail(
    session: &ssh2::Session,
    endpoint: &str,
    kind: &str,
    id: &str,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<Resource> {
    if kind == "network" {
        validate_id(id)?;
    } else if kind == "volume" {
        name(id)?;
    } else {
        return Err(AppError::new("validation", "无效的资源类型"));
    }
    let template = if kind == "volume" {
        r#"{"Name":{{json .Name}},"Driver":{{json .Driver}},"CreatedAt":{{json .CreatedAt}},"Mountpoint":{{json .Mountpoint}},"Scope":{{json .Scope}}}"#
    } else {
        r#"{"Id":{{json .Id}},"Name":{{json .Name}},"Driver":{{json .Driver}},"CreatedAt":{{json .Created}},"Scope":{{json .Scope}},"Internal":{{json .Internal}},"Ingress":{{json .Ingress}},"IPAM":{{json .IPAM.Config}}}"#
    };
    let prefix = query::docker(endpoint);
    let raw = read(
        session,
        &format!(
            "{prefix} {kind} inspect --format {} {}",
            ssh::quote(template),
            ssh::quote(id)
        ),
        check,
    )?;
    let v: Value =
        serde_json::from_str(&raw).map_err(|_| AppError::new("format", "资源详情格式无效"))?;
    let text = |k: &str| v[k].as_str().unwrap_or_default().to_owned();
    if (kind == "volume" && text("Name") != id) || (kind == "network" && text("Id") != id) {
        return Err(AppError::new("identity", "资源身份已变化"));
    }
    let filter = format!("{kind}={id}");
    let raw = read(
        session,
        &format!(
            "{prefix} container ls -a --no-trunc --filter {} --format '{{{{json .}}}}'",
            ssh::quote(&filter)
        ),
        check,
    )?;
    let mut containers = vec![];
    for line in raw.lines().filter(|s| !s.is_empty()) {
        let c: Value = serde_json::from_str(line)
            .map_err(|_| AppError::new("format", "资源使用关系格式无效"))?;
        let cid = c["ID"].as_str().unwrap_or_default();
        validate_id(cid)?;
        containers.push(Reference {
            id: cid.into(),
            name: c["Names"].as_str().unwrap_or_default().into(),
            state: c["State"].as_str().unwrap_or_default().into(),
        });
    }
    containers.sort_by(|a, b| a.id.cmp(&b.id));
    let protected = kind == "network"
        && (["bridge", "host", "none"].contains(&text("Name").as_str()) || v["Ingress"] == true);
    Ok(Resource {
        id: id.into(),
        name: text("Name"),
        kind: kind.into(),
        driver: text("Driver"),
        created: text("CreatedAt"),
        protected,
        mountpoint: text("Mountpoint"),
        containers,
        configuration: v,
    })
}
pub fn overview(
    session: &ssh2::Session,
    endpoint: &str,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<Overview> {
    let prefix = query::docker(endpoint);
    let usage = read(
        session,
        &format!("{prefix} system df --format '{{{{json .}}}}'"),
        check,
    )?
    .lines()
    .filter(|s| !s.is_empty())
    .map(|s| serde_json::from_str(s).map_err(|_| AppError::new("format", "磁盘用量格式无效")))
    .collect::<AppResult<Vec<_>>>()?;
    let mut volumes = vec![];
    let mut networks = vec![];
    for kind in ["volume", "network"] {
        let raw = read(
            session,
            &format!(
                "{prefix} {kind} ls {} --format '{{{{.{} }}}}'",
                if kind == "network" { "--no-trunc" } else { "" },
                if kind == "network" { "ID" } else { "Name" }
            ),
            check,
        )?;
        let ids: Vec<_> = raw.lines().filter(|s| !s.is_empty()).collect();
        if ids.len() > 300 {
            return Err(AppError::new(
                "limit",
                "资源数量超过 300，暂不能完整核对引用关系",
            ));
        }
        for id in ids {
            let resource = detail(session, endpoint, kind, id, check)?;
            if kind == "volume" {
                volumes.push(resource)
            } else {
                networks.push(resource)
            }
        }
    }
    let mut candidates = vec![];
    for c in query::containers_on(session, endpoint, check)? {
        if ["exited", "created", "dead"].contains(&c.state.as_str()) {
            candidates.push(Candidate {
                id: c.id,
                name: c.name,
                kind: "container".into(),
            });
        }
    }
    let images = read(
        session,
        &format!("{prefix} image ls --filter dangling=true --no-trunc --format '{{{{.ID}}}}'"),
        check,
    )?;
    let mut seen = std::collections::HashSet::new();
    for id in images.lines().filter(|s| !s.is_empty()) {
        super::images::validate_id(id)?;
        if seen.insert(id) {
            candidates.push(Candidate {
                id: id.into(),
                name: id.into(),
                kind: "image".into(),
            });
        }
    }
    for v in &volumes {
        if v.containers.is_empty() {
            candidates.push(Candidate {
                id: v.id.clone(),
                name: v.name.clone(),
                kind: "volume".into(),
            });
        }
    }
    for n in &networks {
        if !n.protected && n.containers.is_empty() {
            candidates.push(Candidate {
                id: n.id.clone(),
                name: n.name.clone(),
                kind: "network".into(),
            });
        }
    }
    Ok(Overview {
        usage,
        volumes,
        networks,
        candidates,
    })
}
pub fn prepare(
    session: &ssh2::Session,
    endpoint: &str,
    id: &str,
    action: &Action,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<Prepared> {
    let prefix = query::docker(endpoint);
    let mut messages = vec![];
    let (title, fingerprint, command) = match action {
        Action::VolumeCreate | Action::NetworkCreate => {
            name(id)?;
            let kind = if matches!(action, Action::VolumeCreate) {
                "volume"
            } else {
                "network"
            };
            let existing = remote_read::exec(
                session,
                &format!("{prefix} {kind} inspect {}", ssh::quote(id)),
                check,
            )?;
            if existing.code == 0 {
                return Err(AppError::new("conflict", "同名资源已存在"));
            }
            if !existing.stderr.to_lowercase().contains("no such") {
                return Err(AppError::new("docker", "无法确认资源名称是否可用"));
            }
            messages.push(if kind == "volume" {
                "创建本地驱动的数据卷。".into()
            } else {
                "创建 bridge 驱动的网络，不修改已有网络。".into()
            });
            (
                id.into(),
                json!([id, kind]).to_string(),
                format!(
                    "{prefix} {kind} create {} {}",
                    if kind == "network" {
                        "--driver bridge"
                    } else {
                        "--driver local"
                    },
                    ssh::quote(id)
                ),
            )
        }
        Action::VolumeRemove
        | Action::NetworkRemove
        | Action::NetworkConnect { .. }
        | Action::NetworkDisconnect { .. } => {
            let kind = if matches!(action, Action::VolumeRemove) {
                "volume"
            } else {
                "network"
            };
            let d = detail(session, endpoint, kind, id, check)?;
            if matches!(action, Action::VolumeRemove | Action::NetworkRemove) {
                if d.protected {
                    return Err(AppError::new("validation", "内置或 Ingress 网络不能删除"));
                }
                if !d.containers.is_empty() {
                    return Err(AppError::new(
                        "conflict",
                        "资源仍被容器引用（包含已停止容器），不能删除",
                    ));
                }
                if d.created.is_empty() {
                    return Err(AppError::new(
                        "unavailable",
                        "无法取得资源创建标识，暂不能确认删除原对象",
                    ));
                }
                messages.push(if kind == "volume" {
                    "永久删除此数据卷及其中的数据，无法撤销。不强制删除，不扩大到其他卷。".into()
                } else {
                    "删除此未被容器使用的网络。不使用强制删除。".into()
                });
                let command = format!("{prefix} {kind} rm {}", ssh::quote(id));
                (d.name.clone(), json!(d).to_string(), command)
            } else {
                let container = match action {
                    Action::NetworkConnect { container_id }
                    | Action::NetworkDisconnect { container_id } => container_id,
                    _ => unreachable!(),
                };
                let c = inspect::container(session, endpoint, container, check)?;
                let disconnect = matches!(action, Action::NetworkDisconnect { .. });
                let attached = c.networks.iter().any(|n| n.id == id);
                if attached != disconnect {
                    return Err(AppError::new(
                        "conflict",
                        if disconnect {
                            "容器已不在此网络"
                        } else {
                            "容器已连接此网络"
                        },
                    ));
                }
                if ["host", "none"].contains(&d.name.as_str()) {
                    return Err(AppError::new("validation", "此网络不支持手动连接管理"));
                }
                messages.push(format!(
                    "将容器 {} {}网络 {}{}。",
                    c.name,
                    if disconnect { "断开" } else { "连接到" },
                    d.name,
                    if disconnect {
                        "，可能中断容器通信"
                    } else {
                        ""
                    }
                ));
                let command = format!(
                    "{prefix} network {} {} {}",
                    if disconnect { "disconnect" } else { "connect" },
                    ssh::quote(id),
                    ssh::quote(container)
                );
                (
                    d.name.clone(),
                    json!([d, inspect::fingerprint(&c), c.networks]).to_string(),
                    command,
                )
            }
        }
        Action::CachePrune => {
            messages.push("清理 Docker 构建器当前判定的悬空缓存。候选由引擎在执行时选择，无法提供精确对象预览或承诺释放空间。后续构建可能需要重新下载或构建。".into());
            (
                "悬空构建缓存".into(),
                "dangling-build-cache".into(),
                format!("{prefix} builder prune --force"),
            )
        }
        _ => return Err(AppError::new("validation", "不支持的资源操作")),
    };
    Ok(Prepared {
        name: title,
        fingerprint,
        consequences: messages,
        command,
        private_config: None,
        batch: None,
        timeout: 600,
    })
}
pub fn observe(
    session: &ssh2::Session,
    endpoint: &str,
    id: &str,
    action: &Action,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<String> {
    let kind = if matches!(action, Action::VolumeCreate | Action::VolumeRemove) {
        "volume"
    } else {
        "network"
    };
    if matches!(action, Action::CachePrune) {
        let _ = read(
            session,
            &format!("{} system df", query::docker(endpoint)),
            check,
        )?;
        return Ok("已重新读取空间用量；无法由此证明先前缓存清理是否完成".into());
    }
    if matches!(action, Action::NetworkCreate) {
        let raw = read(
            session,
            &format!(
                "{} network inspect --format '{{{{.Id}}}}' {}",
                query::docker(endpoint),
                ssh::quote(id)
            ),
            check,
        )?;
        return Ok(format!("网络 {}", raw.trim()));
    }
    match detail(session, endpoint, kind, id, check) {
        Ok(d) => Ok(format!("{} · {} 个容器引用", d.name, d.containers.len())),
        Err(e) if e.kind == "notFound" => Ok("removed".into()),
        Err(e) => Err(e),
    }
}

use super::{inspect, model::EngineBinding, query};
use crate::{
    model::{AppError, AppResult},
    remote_read,
};
use serde::{Deserialize, Serialize};
use std::net::IpAddr;

#[derive(Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum LinkRequest {
    Port {
        container_port: String,
        host_ip: String,
        host_port: String,
    },
    Mount {
        destination: String,
    },
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostLink {
    pub path: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub name: String,
}

/// A remote TCP/SSH Engine (or Docker Desktop VM) must never be mistaken for
/// the SSH filesystem or its loopback interface. Fail closed on uncertain locality.
pub fn ensure_local(
    session: &ssh2::Session,
    engine: &EngineBinding,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<()> {
    if !engine.endpoint.starts_with("unix:///") {
        return Err(AppError::new(
            "unavailable",
            "此引擎通过外部地址连接，无法确认端口和路径属于当前 SSH 主机。请复制信息后手动配置。",
        ));
    }
    let result = remote_read::exec(
        session,
        &format!(
            "{} info --format '{{{{json .Name}}}}'",
            query::docker(&engine.endpoint)
        ),
        check,
    )?;
    let daemon: String = serde_json::from_str(result.stdout.trim()).unwrap_or_default();
    let host = remote_read::exec(session, "hostname", check)?;
    if result.code != 0 || host.code != 0 || daemon.is_empty() || daemon != host.stdout.trim() {
        return Err(AppError::new(
            "unavailable",
            "Docker 宿主与 SSH 主机的身份未能匹配，无法自动定位。请复制信息后手动配置。",
        ));
    }
    Ok(())
}
pub fn resolve(
    session: &ssh2::Session,
    engine: &EngineBinding,
    container: &str,
    request: &LinkRequest,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<HostLink> {
    ensure_local(session, engine, check)?;
    let detail = inspect::container(session, &engine.endpoint, container, check)?;
    match request {
        LinkRequest::Mount { destination } => {
            let mount = detail
                .mounts
                .iter()
                .find(|m| m.kind == "bind" && &m.destination == destination)
                .ok_or_else(|| AppError::new("conflict", "挂载已变化，请刷新详情"))?;
            if !mount.source.starts_with('/') || mount.source.chars().any(char::is_control) {
                return Err(AppError::new(
                    "unavailable",
                    "此挂载路径无法在当前文件面板定位",
                ));
            }
            Ok(HostLink {
                path: Some(mount.source.clone()),
                host: None,
                port: None,
                name: detail.name,
            })
        }
        LinkRequest::Port {
            container_port,
            host_ip,
            host_port,
        } => {
            if !container_port.ends_with("/tcp") {
                return Err(AppError::new(
                    "validation",
                    "当前转发只支持已发布的 TCP 端口",
                ));
            }
            if !detail.ports.iter().any(|p| {
                &p.container_port == container_port
                    && p.host_ip.as_deref().unwrap_or("") == host_ip
                    && p.host_port.as_ref() == Some(host_port)
            }) {
                return Err(AppError::new("conflict", "发布端口已变化，请刷新详情"));
            }
            let host: IpAddr = host_ip
                .parse()
                .map_err(|_| AppError::new("validation", "宿主发布地址无效"))?;
            let host = if host.is_unspecified() {
                if host.is_ipv4() {
                    "127.0.0.1".into()
                } else {
                    "::1".into()
                }
            } else {
                host.to_string()
            };
            let port: u16 = host_port
                .parse()
                .ok()
                .filter(|p| *p != 0)
                .ok_or_else(|| AppError::new("validation", "发布端口无效"))?;
            Ok(HostLink {
                path: None,
                host: Some(host),
                port: Some(port),
                name: format!("{} · {}", detail.name, container_port),
            })
        }
    }
}

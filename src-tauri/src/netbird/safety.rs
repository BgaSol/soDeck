use super::{model::*, parser, runner};
use crate::{connection::ConnectionContext, model::AppResult, ssh::quote, traffic::now};
use serde_json::Value;
use std::net::IpAddr;
fn result(state: &str, reason: &str) -> PathAssessment {
    PathAssessment {
        result: state.into(),
        checked_at: now(),
        reasons: vec![reason.into()],
    }
}
pub fn assess_data(
    snapshot: &Snapshot,
    configured: &str,
    jump: bool,
    tuple: &str,
    interfaces: &Value,
    routes: &Value,
) -> PathAssessment {
    let parts: Vec<_> = tuple.split_whitespace().collect();
    if parts.len() != 4
        || parts[0].parse::<IpAddr>().is_err()
        || parts[2].parse::<IpAddr>().is_err()
    {
        return result("unknown", "未获得完整的 SSH 地址证据");
    }
    let local: Vec<_> = snapshot
        .local_peer
        .iter()
        .flat_map(|p| [p.ipv4.as_deref(), p.ipv6.as_deref()])
        .flatten()
        .collect();
    let destination = parts[2].parse::<IpAddr>().unwrap().to_string();
    if local.contains(&destination.as_str()) {
        return result("dependent", "SSH 目标使用当前客户端的 NetBird 地址");
    }
    let Some(ifaces) = interfaces.as_array() else {
        return result("unknown", "无法读取系统网络接口");
    };
    let names: Vec<_> = ifaces
        .iter()
        .filter(|i| {
            i["addr_info"].as_array().is_some_and(|a| {
                a.iter()
                    .any(|v| v["local"].as_str().is_some_and(|s| local.contains(&s)))
            })
        })
        .filter_map(|i| i["ifname"].as_str())
        .collect();
    let Some(routes) = routes.as_array().filter(|r| r.len() == 1) else {
        return result("unknown", "无法确认 SSH 返回路由");
    };
    let route = &routes[0];
    if route["dev"].as_str().is_some_and(|d| names.contains(&d))
        || route["prefsrc"]
            .as_str()
            .is_some_and(|s| local.contains(&s))
    {
        return result("dependent", "SSH 返回路由经过 NetBird 接口");
    }
    if jump {
        return result("unknown", "当前路径含跳板，无法核对所有 SSH 链路");
    }
    if configured.parse::<IpAddr>().ok() != parts[2].parse::<IpAddr>().ok() {
        return result(
            "unknown",
            "配置地址与本次 SSH 地址无法对应，无法核对原会话路径",
        );
    }
    if local.is_empty()
        || (names.is_empty() && snapshot.service != "stopped")
        || route["dev"].as_str().is_none()
        || route.get("multipath").is_some()
    {
        return result("unknown", "缺少 NetBird 接口或路由证据");
    }
    result(
        "independent",
        "SSH 目标地址和返回路由均未使用已识别的 NetBird 接口",
    )
}
pub fn assess(
    session: &ssh2::Session,
    context: &ConnectionContext,
    snapshot: &Snapshot,
    check: &impl Fn() -> AppResult<()>,
) -> AppResult<PathAssessment> {
    let tuple = runner::read(session, "printf '%s' \"$SSH_CONNECTION\"", check)?;
    let parts: Vec<_> = tuple.stdout.split_whitespace().collect();
    if parts.len() != 4 {
        return Ok(result("unknown", "远端未提供 SSH_CONNECTION"));
    }
    let Some(source) = parser::address(parts[0]) else {
        return Ok(result("unknown", "无法识别 SSH 来源地址"));
    };
    let Some(destination) = parser::address(parts[2]) else {
        return Ok(result("unknown", "无法识别 SSH 目标地址"));
    };
    let interfaces = runner::read(session, "ip -j address show", check)?;
    let routes = runner::read(
        session,
        &format!(
            "ip -j {} route get {} from {}",
            if source.contains(':') { "-6" } else { "-4" },
            quote(&source),
            quote(&destination)
        ),
        check,
    )?;
    Ok(assess_data(
        snapshot,
        &context.target().host,
        context.view().hops.len() > 1,
        &tuple.stdout,
        &serde_json::from_str(&interfaces.stdout).unwrap_or(Value::Null),
        &serde_json::from_str(&routes.stdout).unwrap_or(Value::Null),
    ))
}

use super::{model::*, query, runner};
use crate::{
    model::{AppError, AppResult},
    ssh::quote,
    traffic::now,
};
use std::sync::atomic::AtomicBool;
pub fn raw(
    session: &ssh2::Session,
    snapshot: &Snapshot,
    check: &dyn Fn() -> AppResult<()>,
) -> AppResult<Diagnostic> {
    let binding = snapshot.binding.as_ref().unwrap();
    let command = query::cli(
        &binding.cli_path,
        if binding.capabilities.json {
            "status --json"
        } else {
            "status"
        },
    );
    let r = runner::read(session, &command, &|| check())?;
    if r.code != 0 {
        return Err(AppError::new(
            "unavailable",
            "原始状态读取失败，请检查 daemon 或权限",
        ));
    }
    Ok(Diagnostic {
        text: r.stdout,
        source: "远端 NetBird CLI".into(),
        checked_at: now(),
        truncated: false,
    })
}
pub fn logs(session: &ssh2::Session, check: &dyn Fn() -> AppResult<()>) -> AppResult<Diagnostic> {
    let r = runner::run(
        session,
        "env LC_ALL=C journalctl -u netbird.service -n 200 --no-pager -o short-iso",
        8,
        2 * 1024 * 1024,
        &AtomicBool::new(false),
        &|| check(),
    )?;
    if r.code != 0 || r.stderr.contains("not seeing messages") || r.stderr.contains("permission") {
        return Err(AppError::new(
            "permission",
            "无法读取 netbird.service 日志，请检查 journalctl 和读取权限",
        ));
    }
    Ok(Diagnostic {
        text: r.stdout,
        source: "systemd · netbird.service · 最近 200 行".into(),
        checked_at: now(),
        truncated: true,
    })
}
pub fn probe(
    session: &ssh2::Session,
    snapshot: &Snapshot,
    request: ProbeRequest,
    source: &str,
    check: &dyn Fn() -> AppResult<()>,
) -> AppResult<ProbeResult> {
    if !["icmp", "tcp"].contains(&request.kind.as_str())
        || request.kind == "tcp" && request.port.is_none_or(|p| p == 0)
    {
        return Err(AppError::new(
            "validation",
            "请选择 Ping 或一个有效 TCP 端口",
        ));
    }
    let peer = snapshot
        .peers
        .iter()
        .find(|p| p.id == request.peer_id)
        .ok_or_else(|| AppError::new("conflict", "节点已不在当前快照中"))?;
    let address = peer
        .ipv4
        .as_ref()
        .or(peer.ipv6.as_ref())
        .ok_or_else(|| AppError::new("unavailable", "节点没有可用地址"))?;
    let tool = if request.kind == "icmp" { "ping" } else { "nc" };
    let available = runner::read(
        session,
        &format!("command -v timeout && command -v {tool}"),
        &|| check(),
    )?;
    if available.code != 0 {
        return Err(AppError::new(
            "unsupported",
            format!("远端需要 timeout 和 {tool} 才能执行此检测"),
        ));
    }
    let command = if request.kind == "icmp" {
        format!(
            "timeout -k 1 5 ping {} -n -c 1 -W 3 -- {}",
            if address.contains(':') { "-6" } else { "-4" },
            quote(address)
        )
    } else {
        format!(
            "timeout -k 1 5 nc {} -z -w 3 -- {} {}",
            if address.contains(':') { "-6" } else { "-4" },
            quote(address),
            request.port.unwrap()
        )
    };
    let r = runner::run(
        session,
        &command,
        7,
        64 * 1024,
        &AtomicBool::new(false),
        &|| check(),
    )?;
    if r.stderr.to_lowercase().contains("invalid option")
        || r.stderr.to_lowercase().contains("unrecognized option")
    {
        return Err(AppError::new(
            "unsupported",
            "远端检测工具不支持所需的超时参数",
        ));
    }
    Ok(ProbeResult {
        state: if r.code == 0 {
            "reachable"
        } else {
            "noResponse"
        }
        .into(),
        message: if r.code == 0 {
            if request.kind == "icmp" {
                "收到 ICMP 响应"
            } else {
                "TCP 连接成功"
            }
        } else if request.kind == "icmp" {
            "未收到响应；这不代表节点离线"
        } else {
            "未能建立 TCP 连接；可能被拒绝、过滤或超时"
        }
        .into(),
        address: address.clone(),
        source: source.into(),
        checked_at: now(),
    })
}

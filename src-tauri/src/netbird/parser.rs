use super::model::*;
use crate::model::{AppError, AppResult};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::net::IpAddr;

pub fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key)?
        .as_str()
        .filter(|s| !s.is_empty())
        .map(|s| s.chars().filter(|c| !c.is_control()).take(2048).collect())
}
pub fn address(s: &str) -> Option<String> {
    s.split('/')
        .next()?
        .parse::<IpAddr>()
        .ok()
        .map(|a| a.to_string())
}
pub fn hash(s: &str) -> String {
    format!("{:x}", Sha256::digest(s.as_bytes()))
}
fn time(v: &Value, key: &str) -> Option<u64> {
    let s = v.get(key)?.as_str()?;
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|t| t.timestamp_millis())
        .filter(|n| *n > 0)
        .map(|n| n as u64)
}
fn bytes(v: &Value, key: &str) -> Option<String> {
    let n = v.get(key)?;
    n.as_u64()
        .map(|x| x.to_string())
        .or_else(|| n.as_str()?.parse::<u64>().ok().map(|x| x.to_string()))
}
fn component(v: &Value) -> Component {
    Component {
        state: match v.get("connected").and_then(Value::as_bool) {
            Some(true) => "connected",
            Some(false) => "disconnected",
            None => "unknown",
        }
        .into(),
        address: text(v, "url"),
        message: text(v, "error"),
    }
}
fn peer(v: &Value, now: u64) -> Peer {
    let ipv4 = text(v, "netbirdIp").and_then(|s| address(&s));
    let ipv6 = text(v, "netbirdIpv6").and_then(|s| address(&s));
    let fqdn = text(v, "fqdn");
    let raw_state = text(v, "status");
    let raw = raw_state.as_deref().unwrap_or("").to_ascii_lowercase();
    let mode = text(v, "connectionType")
        .unwrap_or_default()
        .to_ascii_lowercase();
    let id =
        hash(&text(v, "publicKey").unwrap_or_else(|| format!("{:?}|{:?}|{:?}", ipv4, ipv6, fqdn)));
    // Go time.Duration is serialized as nanoseconds, not milliseconds.
    let latency = v
        .get("latency")
        .and_then(Value::as_f64)
        .filter(|n| n.is_finite() && *n > 0.)
        .map(|n| n / 1_000_000.);
    Peer {
        id,
        name: fqdn
            .clone()
            .or_else(|| ipv4.clone())
            .or_else(|| ipv6.clone())
            .unwrap_or_else(|| "未命名节点".into()),
        fqdn,
        ipv4,
        ipv6,
        state: match raw.as_str() {
            "connected" => "connected",
            "connecting" => "connecting",
            "idle" | "disconnected" => "idle",
            _ => "unknown",
        }
        .into(),
        raw_state,
        connection_mode: match mode.as_str() {
            "p2p" | "direct" => "direct",
            "relayed" | "relay" => "relayed",
            _ => "unknown",
        }
        .into(),
        latency_ms: latency,
        latency_measured_at: latency.map(|_| now),
        last_handshake_at: time(v, "lastWireguardHandshake"),
        state_changed_at: time(v, "lastStatusUpdate"),
        received_bytes: bytes(v, "transferReceived"),
        sent_bytes: bytes(v, "transferSent"),
        networks: v
            .get("networks")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .take(256)
                    .map(|s| s.chars().take(256).collect())
                    .collect()
            })
            .unwrap_or_default(),
    }
}
pub fn parse(raw: &str, now: u64) -> AppResult<Snapshot> {
    let v: Value = serde_json::from_str(raw)
        .map_err(|_| AppError::new("format", "NetBird 状态不是有效 JSON"))?;
    if !v.is_object()
        || ![
            "peers",
            "daemonVersion",
            "daemonStatus",
            "netbirdIp",
            "management",
        ]
        .iter()
        .any(|k| v.get(k).is_some())
    {
        return Err(AppError::new("format", "无法识别 NetBird 状态结构"));
    }
    let raw_state = text(&v, "daemonStatus").unwrap_or_default();
    let state = raw_state.to_ascii_lowercase();
    let details = v.pointer("/peers/details").and_then(Value::as_array);
    let total = v.pointer("/peers/total").and_then(Value::as_u64);
    let mut seen = std::collections::HashSet::new();
    let peers: Vec<_> = details
        .into_iter()
        .flatten()
        .take(10_000)
        .filter(|p| {
            p.is_object()
                && ["publicKey", "fqdn", "netbirdIp", "netbirdIpv6"]
                    .iter()
                    .any(|key| text(p, key).is_some())
        })
        .map(|p| peer(p, now))
        .filter(|p| seen.insert(p.id.clone()))
        .collect();
    let complete = details.is_some_and(|a| {
        a.len() <= 10_000 && total.is_none_or(|n| n == a.len() as u64) && a.len() == peers.len()
    });
    let relays = v.get("relays").unwrap_or(&Value::Null);
    let dns = v.get("dnsServers").and_then(Value::as_array);
    let dns_state = match dns {
        Some(a) if a.is_empty() => "notConfigured",
        Some(a) if a.iter().any(|d| text(d, "error").is_some()) => "error",
        Some(a)
            if a.iter()
                .all(|d| d.get("enabled").and_then(Value::as_bool) == Some(true)) =>
        {
            "connected"
        }
        _ => "unknown",
    };
    Ok(Snapshot {
        collection_status: "ready".into(),
        updated_at: Some(now),
        installation: "installed".into(),
        service: "running".into(),
        authentication: match state.as_str() {
            "needslogin" | "loginfailed" | "sessionexpired" => "needsLogin",
            "connected" => "authenticated",
            _ => "unknown",
        }
        .into(),
        connection: match state.as_str() {
            "connected" => "connected",
            "connecting" => "connecting",
            "idle" | "disconnected" => "disconnected",
            "needslogin" | "loginfailed" | "sessionexpired" => "needsLogin",
            _ => "unknown",
        }
        .into(),
        local_peer: Some(peer(&v, now)),
        management: component(&v["management"]),
        signal: component(&v["signal"]),
        relay: Component {
            state: match relays.get("available").and_then(Value::as_u64) {
                Some(n) if n > 0 => "connected",
                Some(0) => "unavailable",
                _ => "unknown",
            }
            .into(),
            ..Default::default()
        },
        dns: Component {
            state: dns_state.into(),
            ..Default::default()
        },
        reported_total: total,
        peers,
        peers_complete: complete,
        ..Default::default()
    })
}

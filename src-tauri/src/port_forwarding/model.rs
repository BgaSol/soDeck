use crate::{
    connection::{ConnectionEvent, ConnectionView},
    model::{AppError, AppResult},
};
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub server_id: String,
    pub listen_address: String,
    pub listen_port: Option<u16>,
    pub target_host: String,
    pub target_port: u16,
    pub auto_reconnect: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    pub id: String,
    pub revision: u32,
    pub name: String,
    pub web_scheme: Option<String>,
    pub settings: Settings,
}
impl Rule {
    pub fn validate(&mut self) -> AppResult<()> {
        self.name = self.name.trim().into();
        self.settings.target_host = self.settings.target_host.trim().into();
        let host = &mut self.settings.target_host;
        if host.starts_with('[') && host.ends_with(']') {
            let raw = &host[1..host.len() - 1];
            if raw.parse::<Ipv6Addr>().is_ok() {
                *host = raw.into();
            }
        }
        let address: Option<IpAddr> = self.settings.listen_address.parse().ok();
        if !matches!(address, Some(IpAddr::V4(a)) if a == Ipv4Addr::LOCALHOST)
            && !matches!(address, Some(IpAddr::V6(a)) if a == Ipv6Addr::LOCALHOST)
        {
            return Err(AppError::new(
                "validation",
                "本地地址只支持 127.0.0.1 或 ::1",
            ));
        }
        if self.name.is_empty()
            || self.name.chars().count() > 128
            || self.name.chars().any(char::is_control)
            || host.is_empty()
            || host.len() > 253
            || host
                .chars()
                .any(|c| c.is_control() || c.is_whitespace() || "/\\@?#".contains(c))
            || (host.contains(':') && host.parse::<Ipv6Addr>().is_err())
            || self.settings.target_port == 0
            || self.settings.listen_port == Some(0)
            || self.settings.server_id.is_empty()
            || self
                .web_scheme
                .as_deref()
                .is_some_and(|s| !matches!(s, "http" | "https"))
        {
            return Err(AppError::new(
                "validation",
                "请检查名称、服务器、主机地址和端口（1–65535）",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenRequest {
    pub id: String,
    pub server_id: Option<String>,
    pub seed: Option<ForwardSeed>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetResult {
    pub at: u64,
    pub success: bool,
    pub source: TargetSource,
    pub message: String,
}
#[derive(Clone, PartialEq, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TargetSource {
    Probe,
    Connection,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub at: u64,
    pub message: String,
    pub count: u32,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Prompt {
    pub attempt_id: String,
    pub event: ConnectionEvent,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Runtime {
    pub run_id: String,
    pub phase: String,
    pub actual_address: Option<String>,
    pub settings: Settings,
    pub connection: Option<ConnectionView>,
    pub started_at: u64,
    pub active_connections: usize,
    pub sent_bytes: String,
    pub received_bytes: String,
    pub sent_per_second: f64,
    pub received_per_second: f64,
    pub attempts: u32,
    pub retry_at: Option<u64>,
    pub error: Option<AppError>,
    pub prompt: Option<Prompt>,
    pub target_result: Option<TargetResult>,
    pub probe_id: Option<String>,
    pub notes: Vec<Note>,
}
impl Runtime {
    pub fn note(&mut self, message: impl Into<String>) {
        let message = message.into();
        if let Some(last) = self.notes.last_mut().filter(|n| n.message == message) {
            last.count = last.count.saturating_add(1);
            last.at = crate::traffic::now();
            return;
        }
        self.notes.push(Note {
            at: crate::traffic::now(),
            message,
            count: 1,
        });
        if self.notes.len() > 200 {
            self.notes.remove(0);
        }
    }
    pub fn phase(&mut self, value: &str) {
        crate::diagnostics::state_event("port_forward", &self.run_id, value);
        self.phase = value.into();
        self.prompt = None;
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleView {
    pub rule: Rule,
    pub runtime: Option<Runtime>,
    pub pending_changes: bool,
    pub server_error: Option<String>,
}
#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub ready: usize,
    pub active: usize,
    pub connections: usize,
    pub attention: usize,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardSeed {
    pub name: String,
    pub target_host: String,
    pub target_port: u16,
}

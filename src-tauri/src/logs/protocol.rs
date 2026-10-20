use crate::model::AppError;
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerObservation {
    pub state: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub restart_count: u64,
    pub observed_at: i64,
}
impl ContainerObservation {
    pub fn parse(v: &serde_json::Value) -> Self {
        let time = |key: &str| {
            v["State"][key]
                .as_str()
                .filter(|s| !s.starts_with("0001-") && !s.is_empty())
                .map(str::to_owned)
        };
        Self {
            state: v["State"]["Status"]
                .as_str()
                .unwrap_or(if v["State"]["Running"].as_bool() == Some(true) {
                    "running"
                } else {
                    "stopped"
                })
                .into(),
            started_at: time("StartedAt"),
            finished_at: time("FinishedAt"),
            restart_count: v["RestartCount"].as_u64().unwrap_or(0),
            observed_at: crate::log_store::now(),
        }
    }
}
#[derive(Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum LogEvent {
    State {
        collector_id: String,
        event_revision: u64,
        phase: String,
        reason: Option<String>,
        retry_at: Option<i64>,
        error: Option<String>,
        container: Option<ContainerObservation>,
        tty: Option<bool>,
    },
    Saved {
        last_seq: i64,
        changed_from: Option<i64>,
    },
    Lifecycle {},
    Notice {
        error: String,
    },
    End {
        error: Option<String>,
    },
}
#[derive(Debug, PartialEq, Eq)]
pub enum Failure {
    Transport,
    Daemon,
    Missing,
    Permission,
    Driver,
    Storage,
    Unknown,
}
impl Failure {
    pub fn reason(&self) -> &'static str {
        match self {
            Self::Transport => "network",
            Self::Daemon => "daemonUnavailable",
            Self::Missing => "containerMissing",
            Self::Permission => "permission",
            Self::Driver => "unsupportedDriver",
            Self::Storage => "storage",
            Self::Unknown => "unknown",
        }
    }
    pub fn retry(&self, attempt: u32) -> bool {
        matches!(self, Self::Transport | Self::Daemon) || (*self == Self::Unknown && attempt < 2)
    }
}
pub fn classify(e: &AppError) -> Failure {
    let text = e.message.to_lowercase();
    if matches!(e.kind.as_str(), "hostKey" | "auth" | "authentication")
        || text.contains("permission denied")
        || text.contains("authentication failed")
    {
        return Failure::Permission;
    }
    if e.kind == "logs" || e.kind == "database" || text.contains("日志保存失败") {
        return Failure::Storage;
    }
    if text.contains("logging driver")
        && (text.contains("does not support") || text.contains("not support"))
    {
        return Failure::Driver;
    }
    if e.kind == "docker" {
        if text.contains("cannot connect to the docker daemon")
            || text.contains("is the docker daemon running")
            || text.contains("connection refused")
            || text.contains("connection reset")
        {
            return Failure::Daemon;
        }
        if text.contains("no such container") || text.contains("no such object") {
            return Failure::Missing;
        }
        return Failure::Unknown;
    }
    if matches!(e.kind.as_str(), "network" | "ssh" | "timeout" | "io") {
        Failure::Transport
    } else {
        Failure::Unknown
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn classification_does_not_retry_identity_or_disk_failures() {
        for e in [
            AppError::new("hostKey", "changed"),
            AppError::new("ssh", "Permission denied"),
            AppError::new("logs", "日志保存失败"),
        ] {
            assert!(!classify(&e).retry(0));
        }
        assert!(classify(&AppError::new(
            "docker",
            "Cannot connect to the Docker daemon"
        ))
        .retry(50));
        assert_eq!(
            classify(&AppError::new("docker", "No such container: a")),
            Failure::Missing
        );
        assert!(!Failure::Unknown.retry(2));
    }
}

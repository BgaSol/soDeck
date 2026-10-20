use crate::remote_mounts::{MountTable, MountUsage};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Metric<T> {
    pub value: Option<T>,
    pub status: String,
    pub updated_at: Option<u64>,
    pub error: Option<String>,
}
impl<T> Default for Metric<T> {
    fn default() -> Self {
        Self {
            value: None,
            status: "collecting".into(),
            updated_at: None,
            error: None,
        }
    }
}
impl<T> Metric<T> {
    pub fn success(value: T, now: u64) -> Self {
        Self {
            value: Some(value),
            status: "ready".into(),
            updated_at: Some(now),
            error: None,
        }
    }
    pub fn fail(&mut self, status: &str, message: &str) {
        self.status = status.into();
        self.error = Some(message.into());
    }
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CpuSample {
    pub percent: Option<f64>,
    pub cores: BTreeMap<String, Option<f64>>,
    pub load: Option<[f64; 3]>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemorySample {
    pub total_bytes: String,
    pub available_bytes: String,
    pub used_bytes: String,
    pub used_percent: f64,
    pub cached_bytes: Option<String>,
    pub swap_total_bytes: Option<String>,
    pub swap_used_bytes: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkSample {
    pub name: String,
    pub index: Option<u64>,
    pub is_default: bool,
    pub virtual_interface: bool,
    pub rx_bytes: String,
    pub tx_bytes: String,
    pub rx_per_second: Option<f64>,
    pub tx_per_second: Option<f64>,
    pub rx_errors: String,
    pub tx_errors: String,
    pub rx_dropped: String,
    pub tx_dropped: String,
}
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Metrics {
    pub cpu: Metric<CpuSample>,
    pub memory: Metric<MemorySample>,
    pub network: Metric<Vec<NetworkSample>>,
}
impl Metrics {
    pub fn fail(&mut self, status: &str, message: &str) {
        self.cpu.fail(status, message);
        self.memory.fail(status, message);
        self.network.fail(status, message);
    }
}
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CachedUsage {
    pub value: Option<MountUsage>,
    pub loading: bool,
    pub stale: bool,
    pub error: Option<String>,
}
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Mounts {
    pub table: Option<MountTable>,
    pub usage: BTreeMap<String, CachedUsage>,
    pub fetched_at: u64,
    pub loading: bool,
    pub error: String,
}
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Interest {
    #[serde(default)]
    pub visible: bool,
    #[serde(default)]
    pub mount_paths: Vec<String>,
    #[serde(default)]
    pub docker: DockerInterest,
    #[serde(default)]
    pub client_traffic: bool,
}
#[derive(Clone, Debug, Default, Deserialize)]
pub struct DockerInterest {
    #[serde(default)]
    pub summary: bool,
    #[serde(default)]
    pub containers: bool,
    #[serde(default)]
    pub images: bool,
}
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DockerSummary {
    pub total: usize,
    pub by_state: BTreeMap<String, usize>,
    pub unhealthy: Option<usize>,
    pub unhealthy_ids: Vec<String>,
    pub health_error: Option<String>,
}
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DockerState {
    pub engine_generation: u64,
    pub engine_error_kind: Option<String>,
    pub engine: Metric<crate::docker::DockerStatus>,
    pub containers: Metric<Vec<crate::docker::Container>>,
    pub summary: Metric<DockerSummary>,
    pub images: Metric<Vec<crate::docker::Image>>,
    pub refreshing: bool,
}
impl DockerState {
    /// A timed-out observation does not revoke a previously verified target.
    /// Callers must still verify its identity on their own connection before acting.
    pub fn can_attempt(&self) -> bool {
        self.engine
            .value
            .as_ref()
            .is_some_and(|e| e.state == "running")
            && (self.engine.status == "ready"
                || (self.engine.status == "stale"
                    && self.engine_error_kind.as_deref() == Some("timeout")))
    }
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPoint {
    pub sequence: u64,
    pub at: u64,
    pub cpu: Option<f64>,
    pub memory: Option<f64>,
    pub network: BTreeMap<String, [Option<f64>; 2]>,
    pub gap: bool,
}
#[derive(Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum MonitorEvent {
    Connection {
        generation: String,
        connected: bool,
    },
    Snapshot {
        generation: String,
        #[serde(rename = "metricsRevision")]
        metrics_revision: u64,
        #[serde(rename = "mountsRevision")]
        mounts_revision: u64,
        metrics: Metrics,
        mounts: Mounts,
        #[serde(rename = "dockerRevision")]
        docker_revision: u64,
        docker: DockerState,
        traffic: crate::traffic::Sample,
    },
    Metrics {
        generation: String,
        revision: u64,
        metrics: Metrics,
        point: HistoryPoint,
    },
    Mounts {
        generation: String,
        revision: u64,
        mounts: Mounts,
    },
    Docker {
        generation: String,
        revision: u64,
        docker: DockerState,
    },
    Traffic {
        generation: String,
        traffic: crate::traffic::Sample,
        point: Option<crate::traffic::Point>,
    },
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPage {
    pub generation: String,
    pub points: Vec<HistoryPoint>,
    pub truncated: bool,
    pub traffic: crate::traffic::HistoryPage,
}

#[cfg(test)]
mod availability_tests {
    use super::*;
    #[test]
    fn only_classified_timeout_retains_verified_target() {
        let mut docker = DockerState::default();
        assert!(!docker.can_attempt());
        docker.engine = Metric::success(
            crate::docker::DockerStatus {
                state: "running".into(),
                message: "ok".into(),
                endpoint: "unix:///docker.sock".into(),
                daemon_id: Some("engine".into()),
            },
            1,
        );
        assert!(docker.can_attempt());
        docker.engine.fail("stale", "failure");
        assert!(!docker.can_attempt());
        for kind in [
            "permission",
            "authentication",
            "hostKey",
            "network",
            "disconnected",
            "ssh",
        ] {
            docker.engine_error_kind = Some(kind.into());
            assert!(!docker.can_attempt(), "{kind}");
        }
        docker.engine_error_kind = Some("timeout".into());
        assert!(docker.can_attempt());
        docker.engine.value.as_mut().unwrap().state = "unreachable".into();
        assert!(!docker.can_attempt());
        docker.engine.value = None;
        assert!(!docker.can_attempt());
    }
}

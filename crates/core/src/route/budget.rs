use serde::Serialize;
use super::ResolvedStep;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cost {
    pub sessions: usize,
    pub bridges: usize,
    pub socket_endpoints: usize,
    pub bridge_bytes: usize,
    pub proxy_contexts: usize,
    pub snapshot_bytes: usize,
}
impl Cost {
    pub fn for_steps(steps: &[ResolvedStep]) -> Self {
        let sessions = steps.iter().filter(|step| matches!(step, ResolvedStep::Ssh(_))).count();
        let bridges = sessions.saturating_sub(1);
        Self { sessions, bridges, socket_endpoints: 1 + 2 * bridges, bridge_bytes: bridges * 512 * 1024,
            proxy_contexts: steps.len() - sessions, snapshot_bytes: serde_json::to_vec(steps).expect("resolved steps").len() }
    }
    pub fn attempt_timeout_ms(&self) -> u64 {
        (120_000 + 75_000 * self.sessions.saturating_sub(2) as u64 + 30_000 * self.proxy_contexts as u64).min(900_000)
    }
}

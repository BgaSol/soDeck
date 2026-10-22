use super::*;
fn url_fingerprint(parts: &[String]) -> String {
    use sha2::Digest;
    format!("{:x}", sha2::Sha256::digest(parts.join("\n").as_bytes()))
}
pub(super) fn preflight(
    state: &State,
    owner_generation: u64,
) -> AppResult<(InstallReview, String)> {
    let mut blockers = Vec::new();
    let mut interruptions = Vec::new();
    let mut identity = state.workspaces.impact_ids();
    let file_ids = state.files.update_impact_ids();
    if state.files.blocks_update() {
        blockers.push(Blocker {
            kind: "editors".into(),
            count: file_ids.len(),
            action: Some("saveAndCloseEditors".into()),
        });
        identity.extend(file_ids);
    }
    let task_ids = crate::file_tasks::update_blocking_ids(state);
    if !task_ids.is_empty() {
        blockers.push(Blocker {
            kind: "fileTasks".into(),
            count: task_ids.len(),
            action: Some("reviewFileTasks".into()),
        });
        identity.extend(task_ids.iter().map(|id| format!("fileTask:{id}")));
    }
    if task_ids.is_empty() && state.transfers.has_activity() {
        blockers.push(Blocker {
            kind: "transfersFinishing".into(),
            count: 1,
            action: None,
        });
        identity.push("transfers:finishing".into());
    }
    let docker = state.docker.summary();
    let docker_count = docker.active + docker.attention;
    if docker_count > 0 {
        blockers.push(Blocker {
            kind: "dockerOperations".into(),
            count: docker_count,
            action: Some("reviewDocker".into()),
        });
        identity.push(format!("docker:{}:{}", docker.active, docker.attention));
    }
    if state.netbird.blocks_update()? {
        blockers.push(Blocker {
            kind: "netbirdOperations".into(),
            count: 1,
            action: Some("reviewNetbird".into()),
        });
        identity.push("netbird:busy".into());
    }
    let terminals = state.terminals.lock().unwrap();
    let terminal_ids = terminals
        .iter()
        .filter(|(_, t)| !t.stream.lock().unwrap().closed)
        .map(|(id, _)| id.clone())
        .collect::<Vec<_>>();
    drop(terminals);
    if !terminal_ids.is_empty() {
        interruptions.push(Interruption {
            kind: "terminals".into(),
            count: terminal_ids.len(),
            connections: None,
        });
        identity.extend(terminal_ids.iter().map(|id| format!("terminal:{id}")));
    }
    let logs = state.logs.update_impact_ids();
    if !logs.is_empty() {
        interruptions.push(Interruption {
            kind: "logs".into(),
            count: logs.len(),
            connections: None,
        });
        identity.extend(logs.into_iter().map(|id| format!("log:{id}")));
    }
    let forwards = state.port_forwarding.update_impact_ids();
    if !forwards.is_empty() {
        interruptions.push(Interruption {
            kind: "portForwarding".into(),
            count: forwards.len(),
            connections: Some(state.port_forwarding.summary().connections),
        });
        identity.extend(forwards.into_iter().map(|id| format!("forward:{id}")));
    }
    let monitors = state.monitor.update_impact_ids();
    if !monitors.is_empty() {
        interruptions.push(Interruption {
            kind: "monitors".into(),
            count: monitors.len(),
            connections: None,
        });
        identity.extend(monitors.into_iter().map(|id| format!("monitor:{id}")));
    }
    let review = InstallReview {
        release_id: String::new(),
        review_token: None,
        expires_at: now() + 60,
        blockers,
        interruptions,
    };
    let mut identity = identity;
    identity.push(format!("mainGeneration:{owner_generation}"));
    identity.sort();
    Ok((review, url_fingerprint(&identity)))
}

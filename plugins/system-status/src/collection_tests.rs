use crate::{
    collectors::{self, DecodeContext, Family},
    metrics::{self, Baseline},
    model::*,
    shared_state::Publisher,
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

const DYNAMIC: &str = include_str!("collectors/fixtures/linux-dynamic.txt");
const TOPOLOGY: &str = include_str!("collectors/fixtures/linux-topology.txt");

fn sample(
    state: &mut StatusState,
    base: &mut Baseline,
    group: &str,
    raw: &str,
    at: u64,
    now: Instant,
) -> metrics::ApplySummary {
    let targets = state.mounts.targets();
    let paths: Vec<_> = targets.iter().map(|t| t.path.clone()).collect();
    let decoded = collectors::decode(
        Family::Linux,
        group,
        raw,
        "fixture",
        &DecodeContext {
            total: None,
            interfaces: state.interfaces.value.as_deref().unwrap_or(&[]),
            paths: &paths,
        },
    )
    .unwrap();
    metrics::apply(state, base, decoded, &targets, at, now, 5)
}

#[test]
fn collection_raw_linux_sources_keep_all_expected_metrics() {
    let mut state = StatusState::default();
    state.family = Some(Family::Linux);
    let mut base = Baseline::default();
    let now = Instant::now();
    for (group, raw) in [("topology", TOPOLOGY), ("dynamic", DYNAMIC)] {
        let summary = sample(&mut state, &mut base, group, raw, 1000, now);
        assert!(
            summary.failures.is_empty(),
            "{group}: {:?}",
            summary.failures
        );
    }
    assert_eq!(state.disk_source.state, "ok");
    assert_eq!(state.disks.value.as_ref().unwrap().len(), 2);
    let interfaces = state.interfaces.value.as_ref().unwrap();
    assert_eq!(interfaces.len(), 2);
    assert_eq!(
        interfaces[0].addresses,
        ["192.0.2.10/24", "2001:db8::10/64"]
    );
    assert_eq!(interfaces[0].state.as_deref(), Some("up"));
    assert_eq!(interfaces[1].id, "wt0:3:");
    assert!(state.mounts.complete, "{:?}", state.mounts.reason);
    assert_eq!(state.mounts.entries.len(), 3);
    assert!(state.data()["sourceErrors"]
        .as_object()
        .unwrap()
        .values()
        .all(|v| v.as_object().unwrap().is_empty()));
}

#[test]
fn collection_bad_records_still_fail_only_their_source() {
    let now = Instant::now();
    let mut state = StatusState::default();
    let mut base = Baseline::default();
    let raw = DYNAMIC.replace("8 0 sda 1 0 100 0 1 0 200 0 0 0 0", "8 0 sda");
    let summary = sample(&mut state, &mut base, "dynamic", &raw, 1000, now);
    assert_eq!(summary.failures, ["磁盘计数缺项"]);
    assert_eq!(state.memory.state, "ok");
    assert_eq!(state.network_source.state, "ok");
    let blank = DYNAMIC.replace("8 0 sda ", "\n8 0 sda ");
    let summary = sample(&mut state, &mut base, "dynamic", &blank, 1000, now);
    assert_eq!(summary.failures, ["磁盘计数缺项"]);
    let raw = TOPOLOGY.replace("eno1|2|02:00:00:00:00:01|up|", "eno1");
    let summary = sample(&mut state, &mut base, "topology", &raw, 1000, now);
    assert_eq!(summary.failures, ["接口记录无效"]);
    assert_eq!(state.load.state, "ok");
    let raw = TOPOLOGY.replace(
        "2 1 8:17 / /data rw,relatime - ext4 /dev/sdb1 rw",
        "2 1 8:17",
    );
    sample(&mut state, &mut base, "topology", &raw, 1000, now);
    assert!(!state.mounts.complete);
    let raw = DYNAMIC.replace(" 100 0 1 0 200 ", " 18446744073709551615 0 1 0 200 ");
    let summary = sample(&mut state, &mut base, "dynamic", &raw, 2000, now);
    assert!(summary.failures.iter().any(|e| e == "磁盘计数溢出"));
    let raw = DYNAMIC.replace("END disk 0", "END disk 1");
    let summary = sample(&mut state, &mut base, "dynamic", &raw, 3000, now);
    assert!(summary.failures.iter().any(|e| e == "disk 来源退出码 1"));
}

#[test]
fn collection_interface_recovery_restores_identity_and_preserves_explicit_selection() {
    let mut app = crate::scheduler::App::new(5);
    app.mount(
        "v",
        &json!({"workspaceId":"w","connectionId":"c","generation":1}),
    )
    .unwrap();
    let mut base = Baseline::default();
    let now = Instant::now();
    let state = &mut app.views.get_mut("v").unwrap().data;
    sample(state, &mut base, "dynamic", DYNAMIC, 1000, now);
    assert!(state.interfaces.value.as_ref().unwrap()[0]
        .addresses
        .is_empty());
    sample(state, &mut base, "topology", TOPOLOGY, 2000, now);
    assert_eq!(
        app.snapshot("v", None).unwrap()["network"]["device"]["id"],
        "eno1:2:02:00:00:00:00:01"
    );
    app.views.get_mut("v").unwrap().navigation = json!({"interfaceId":"wt0:3:"});
    let broken = TOPOLOGY.replace("END interfaces 0", "END interfaces 1");
    sample(
        &mut app.views.get_mut("v").unwrap().data,
        &mut base,
        "topology",
        &broken,
        3000,
        now,
    );
    let state = &app.views["v"].data;
    assert_eq!(state.interfaces.observed_at, Some(2000));
    assert_eq!(state.interfaces.state, "unavailable");
    sample(
        &mut app.views.get_mut("v").unwrap().data,
        &mut base,
        "topology",
        TOPOLOGY,
        4000,
        now,
    );
    assert_eq!(
        app.snapshot("v", None).unwrap()["network"]["device"]["id"],
        "wt0:3:"
    );
    assert_eq!(app.views["v"].data.interfaces.observed_at, Some(4000));
}

#[test]
fn collection_disk_failure_keeps_old_time_and_rewarms_before_new_rates() {
    let mut state = StatusState::default();
    let mut base = Baseline::default();
    let now = Instant::now();
    let raw = |step: u64| {
        DYNAMIC
            .replace("10.00 20.00", &format!("{}.00 20.00", 10 + step * 5))
            .replace(
                "8 0 sda 1 0 100 0 1 0 200",
                &format!(
                    "8 0 sda 1 0 {} 0 1 0 {}",
                    100 + step * 100,
                    200 + step * 100
                ),
            )
    };
    sample(&mut state, &mut base, "dynamic", &raw(0), 1000, now);
    sample(
        &mut state,
        &mut base,
        "dynamic",
        &raw(1),
        6000,
        now + Duration::from_secs(5),
    );
    assert_eq!(state.rate("disk:8:0")["first"], 10240.0);
    let broken = raw(2).replace("END disk 0", "END disk 1");
    let failed = sample(
        &mut state,
        &mut base,
        "dynamic",
        &broken,
        11000,
        now + Duration::from_secs(10),
    );
    assert_eq!(state.rate("disk:8:0")["observedAt"], 6000);
    assert_eq!(state.rate("disk:8:0")["state"], "unavailable");
    assert_eq!(state.memory.observed_at, Some(11000));
    assert_eq!(failed.points["disk:8:0:first"], None);
    let recovered = sample(
        &mut state,
        &mut base,
        "dynamic",
        &raw(3),
        16000,
        now + Duration::from_secs(15),
    );
    assert_eq!(recovered.points["disk:8:0:first"], None);
    assert_eq!(state.rate("disk:8:0")["state"], "warmingUp");
    let next = sample(
        &mut state,
        &mut base,
        "dynamic",
        &raw(4),
        21000,
        now + Duration::from_secs(20),
    );
    assert_eq!(next.points["disk:8:0:first"], Some(10240.0));
    assert_eq!(state.rate("disk:8:0")["observedAt"], 21000);
}

fn publish(
    p: &mut Publisher,
    state: &StatusState,
    at: u64,
    records: &mut BTreeMap<(String, String), Value>,
) {
    p.observe(&state.mounts, at, None, false);
    for revision in 1..100 {
        let Some(job) = p.next() else { return };
        let result = if job.revision.is_none() {
            json!({"record":{"revision":"0","present":records.contains_key(&job.key),"value":records.get(&job.key)}})
        } else {
            if let Some(value) = &job.value {
                records.insert(job.key.clone(), value.clone());
            } else {
                records.remove(&job.key);
            }
            json!({"revision":revision.to_string()})
        };
        p.complete(job, Ok(result));
    }
    panic!("publisher failed to settle");
}

#[test]
fn collection_partial_then_complete_scan_cleans_shared_mount_and_capacity() {
    let mut state = StatusState::default();
    let mut base = Baseline::default();
    let now = Instant::now();
    sample(&mut state, &mut base, "topology", TOPOLOGY, 1000, now);
    let targets = state.mounts.targets();
    state.mounts.capacity(
        crate::mounts::Capacity {
            path: "/data".into(),
            device: "/dev/sdb1".into(),
            total: 100,
            used: Some(20),
            available: 80,
        },
        &targets,
        1001,
    );
    let old = targets
        .iter()
        .find(|t| t.path == "/data")
        .unwrap()
        .key
        .clone();
    let mut p = Publisher::default();
    let mut records = BTreeMap::new();
    publish(&mut p, &state, 1001, &mut records);
    assert_eq!(
        records[&("filesystem.mounts.v1".into(), "scan-status".into())]["coverage"],
        "complete"
    );
    let partial = TOPOLOGY.replace(
        "2 1 8:17 / /data rw,relatime - ext4 /dev/sdb1 rw",
        "broken mount record",
    );
    sample(&mut state, &mut base, "topology", &partial, 2000, now);
    publish(&mut p, &state, 2000, &mut records);
    assert_eq!(
        records[&("filesystem.mounts.v1".into(), old.clone())]["observedAt"],
        1000
    );
    assert_eq!(
        records[&("filesystem.capacity.v1".into(), old.clone())]["observedAt"],
        1001
    );
    assert_eq!(
        records[&("filesystem.mounts.v1".into(), "scan-status".into())]["coverage"],
        "partial"
    );
    let complete: String = TOPOLOGY
        .split_inclusive('\n')
        .filter(|line| !line.starts_with("2 1 8:17 "))
        .collect();
    sample(&mut state, &mut base, "topology", &complete, 3000, now);
    publish(&mut p, &state, 3000, &mut records);
    assert!(!records.contains_key(&("filesystem.mounts.v1".into(), old.clone())));
    assert!(!records.contains_key(&("filesystem.capacity.v1".into(), old)));
    assert_eq!(
        records[&("filesystem.mounts.v1".into(), "scan-status".into())]["coverage"],
        "complete"
    );
}

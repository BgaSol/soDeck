//! Producer-owned observations and coalesced CAS work; remote sampling never waits on publishing.
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};
type Key = (String, String);
#[derive(Default, Clone)]
pub struct Publisher {
    desired: BTreeMap<Key, Option<Value>>,
    known: BTreeMap<Key, (String, Option<Value>)>,
    retry: Option<Instant>,
    scan: Value,
}
pub struct Job {
    pub key: Key,
    pub value: Option<Value>,
    pub revision: Option<String>,
}
impl Publisher {
    pub fn observe(
        &mut self,
        catalog: &crate::mounts::MountCatalog,
        observed: u64,
        error: Option<&str>,
        paused: bool,
    ) {
        let previous = self.clone();
        let mut live = std::collections::BTreeSet::new();
        for e in catalog.entries.values() {
            let m = &e.mount;
            let id = &e.key;
            live.insert(id.clone());
            self.desired.insert(("filesystem.mounts.v1".into(),id.clone()),Some(json!({"kind":"mount","mountId":id,"path":m.path,"pathStyle":if m.windows{"windows"}else{"posix"},"filesystemView":"session-default","device":m.device,"filesystemType":m.kind,"readOnly":m.read_only,"observedAt":e.observed_at,"staleAfterMs":60000})));
            let capacity=e.capacity.value.as_ref().map(|c|json!({"mountId":id,"totalBytes":c.total.to_string(),"availableBytes":c.available.to_string(),"observedAt":e.capacity.observed_at,"staleAfterMs":120000}));
            self.desired
                .insert(("filesystem.capacity.v1".into(), id.clone()), capacity);
        }
        for ((_, id), value) in &mut self.desired {
            if id != "scan-status" && !live.contains(id) {
                *value = None;
            }
        }
        let error = catalog.status.reason.as_deref().or(error);
        self.scan = json!({"kind":"scanStatus","coverage":if catalog.status.observed_at.is_none(){"unsupported"}else if catalog.complete{"complete"}else{"partial"},"reason":catalog.reason,"observedAt":catalog.status.observed_at,"staleAfterMs":60000,"state":if paused{"paused"}else if error.is_some(){"failed"}else{"ready"},"error":error,"publishState":"publishing"});
        self.desired.insert(
            ("filesystem.mounts.v1".into(), "scan-status".into()),
            Some(self.scan.clone()),
        );
        let pending_bytes: usize = self
            .desired
            .iter()
            .filter(|(key, value)| self.dirty(key, value))
            .map(|(key, value)| {
                serde_json::to_vec(&json!([key, value])).map_or(0, |b| b.len() + 128)
            })
            .sum();
        if self.bytes() + pending_bytes > 960 * 1024 {
            *self = previous;
            self.scan = json!({"kind":"scanStatus","coverage":"partial","reason":"共享发布缓存超过预算，保留旧观测","state":"failed","observedAt":observed,"staleAfterMs":60000,"publishState":"limited"});
            self.desired.insert(
                ("filesystem.mounts.v1".into(), "scan-status".into()),
                Some(self.scan.clone()),
            );
        }
    }
    fn dirty(&self, key: &Key, value: &Option<Value>) -> bool {
        !self
            .known
            .get(key)
            .is_some_and(|(_, current)| current == value)
    }
    pub fn next(&mut self) -> Option<Job> {
        if self.retry.is_some_and(|at| at > Instant::now()) || self.scan.is_null() {
            return None;
        }
        let obsolete: Vec<_> = self
            .desired
            .iter()
            .filter(|(k, v)| v.is_none() && !self.dirty(k, v))
            .map(|(k, _)| k.clone())
            .collect();
        for key in obsolete {
            self.desired.remove(&key);
            self.known.remove(&key);
        }
        let status = ("filesystem.mounts.v1".into(), "scan-status".into());
        let mut value = self.scan.clone();
        if self.scan["publishState"] != "limited"
            && !self
                .desired
                .iter()
                .any(|(k, v)| *k != status && self.dirty(k, v))
        {
            value["publishState"] = json!("synced");
        }
        self.desired.insert(status.clone(), Some(value));
        let key = if self.dirty(&status, &self.desired[&status]) {
            status
        } else {
            self.desired
                .iter()
                .find(|(k, v)| self.dirty(k, v))?
                .0
                .clone()
        };
        Some(Job {
            value: self.desired[&key].clone(),
            revision: self.known.get(&key).map(|(r, _)| r.clone()),
            key,
        })
    }
    pub fn complete(&mut self, job: Job, result: Result<Value, sodeck_plugin_client::RpcError>) {
        match result {
            Ok(r) => {
                let (revision, value) = if job.revision.is_none() {
                    let record = &r["record"];
                    (
                        record["revision"].as_str(),
                        (record["present"] == true).then(|| record["value"].clone()),
                    )
                } else {
                    (r["revision"].as_str(), job.value)
                };
                if let Some(revision) = revision {
                    self.known.insert(job.key, (revision.into(), value));
                    self.retry = None;
                } else {
                    self.known.remove(&job.key);
                    self.retry = Some(Instant::now() + Duration::from_secs(1));
                }
            }
            Err(_) => {
                self.known.remove(&job.key);
                self.retry = Some(Instant::now() + Duration::from_secs(1));
            }
        }
    }
    pub fn bytes(&self) -> usize {
        serde_json::to_vec(&json!({"desired":self.desired.iter().map(|(k,v)|json!([k,v])).collect::<Vec<_>>(),"known":self.known.iter().map(|(k,v)|json!([k,v])).collect::<Vec<_>>()})).map_or(0,|b|b.len())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model::Read,
        mounts::{Capacity, Mount, MountCatalog, MountScan},
    };
    fn scan(device: &str, path: &str, complete: bool) -> Read<MountScan> {
        Read::Value(MountScan {
            rows: vec![Mount {
                identity: device.into(),
                path: path.into(),
                device: device.into(),
                capacity_identity: device.into(),
                kind: Some("apfs".into()),
                read_only: Some(false),
                windows: false,
                eligible: true,
                system: false,
            }],
            complete,
            reason: None,
        })
    }
    #[test]
    fn shared_capacity_and_display_use_one_identity_and_preserve_observation_time() {
        let mut c = MountCatalog::default();
        c.scan(scan("a", "/data", true), 1);
        let target = c.targets();
        c.capacity(
            Capacity {
                path: "/data".into(),
                device: "a".into(),
                total: 100,
                used: Some(20),
                available: 70,
            },
            &target,
            2,
        );
        let id = target[0].key.clone();
        let mut p = Publisher::default();
        p.observe(&c, 2, None, false);
        assert_eq!(
            p.desired[&("filesystem.capacity.v1".into(), id.clone())]
                .as_ref()
                .unwrap()["observedAt"],
            2
        );
        c.scan(scan("a", "/data", true), 100);
        p.observe(&c, 100, None, false);
        assert_eq!(
            p.desired[&("filesystem.capacity.v1".into(), id.clone())]
                .as_ref()
                .unwrap()["observedAt"],
            2
        );
        assert_eq!(crate::filesystem::project(&c)[0]["observedAt"], 2);
        c.scan(scan("b", "/data", false), 101);
        p.observe(&c, 101, None, false);
        assert_eq!(
            p.desired[&("filesystem.capacity.v1".into(), id.clone())],
            None
        );
        assert_eq!(p.desired[&("filesystem.mounts.v1".into(), id)], None);
    }
    #[test]
    fn partial_scan_retains_old_mount_time_and_complete_scan_removes_it() {
        let mut c = MountCatalog::default();
        c.scan(scan("a", "/a", true), 1);
        c.scan(scan("b", "/b", false), 2);
        assert_eq!(c.entries.len(), 2);
        let mut p = Publisher::default();
        p.observe(&c, 2, None, false);
        assert!(p
            .desired
            .values()
            .flatten()
            .any(|v| v["path"] == "/a" && v["observedAt"] == 1));
        c.scan(scan("b", "/b", true), 3);
        p.observe(&c, 3, None, false);
        assert_eq!(c.entries.len(), 1);
        assert!(p.desired.values().any(Option::is_none));
    }
    #[test]
    fn publishing_budget_keeps_previous_copy_and_reports_limit() {
        let mut c = MountCatalog::default();
        for i in 0..512 {
            let mut scan = match scan(&i.to_string(), &format!("/{i}{}", "x".repeat(3990)), false) {
                Read::Value(s) => s,
                _ => unreachable!(),
            };
            scan.complete = false;
            c.scan(Read::Value(scan), 1);
        }
        let mut p = Publisher::default();
        p.observe(&c, 1, None, false);
        assert_eq!(p.scan["publishState"], "limited");
        assert!(p.bytes() < 960 * 1024);
    }
    #[test]
    fn late_write_and_conflict_reconcile_to_latest_observation() {
        let mut p = Publisher::default();
        let key = ("filesystem.capacity.v1".into(), "mount".into());
        p.scan = json!({"publishState":"publishing"});
        p.desired.insert(key.clone(), Some(json!({"observedAt":1})));
        p.known.insert(key.clone(), ("7".into(), None));
        let old = Job {
            key: key.clone(),
            revision: Some("7".into()),
            value: p.desired[&key].clone(),
        };
        p.desired.insert(key.clone(), Some(json!({"observedAt":2})));
        p.complete(old, Ok(json!({"revision":"8"})));
        assert!(p.dirty(&key, &p.desired[&key]));
        assert_eq!(p.known[&key].1.as_ref().unwrap()["observedAt"], 1);
        let retry = Job {
            key: key.clone(),
            revision: Some("8".into()),
            value: p.desired[&key].clone(),
        };
        p.complete(
            retry,
            Err(sodeck_plugin_client::RpcError {
                code: -32000,
                message: "conflict".into(),
                data: Value::Null,
            }),
        );
        p.desired.insert(key.clone(), Some(json!({"observedAt":3})));
        p.retry = None;
        let get = Job {
            key: key.clone(),
            revision: None,
            value: p.desired[&key].clone(),
        };
        p.complete(
            get,
            Ok(json!({"record":{"revision":"9","present":true,"value":{"observedAt":2}}})),
        );
        p.known.insert(
            ("filesystem.mounts.v1".into(), "scan-status".into()),
            ("10".into(), Some(p.scan.clone())),
        );
        let next = p.next().unwrap();
        assert_eq!(next.key, key);
        assert_eq!(next.revision.as_deref(), Some("9"));
        assert_eq!(next.value.unwrap()["observedAt"], 3);
    }
}

//! Producer-owned observations and coalesced CAS work; remote sampling never waits on publishing.
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    hash::{Hash, Hasher},
    time::{Duration, Instant},
};
type Key = (String, String);
#[derive(Default, Clone)]
pub struct Publisher {
    desired: BTreeMap<Key, Option<Value>>,
    known: BTreeMap<Key, (String, Option<Value>)>,
    identities: BTreeMap<String, (String, String, u64)>,
    sequence: u64,
    retry: Option<Instant>,
    scan: Value,
}
pub struct Job {
    pub key: Key,
    pub value: Option<Value>,
    pub revision: Option<String>,
}
impl Publisher {
    pub fn observe(&mut self, data: &Value, observed: u64, error: Option<&str>, paused: bool) {
        let previous = self.clone();
        let topology = &data["mountTopology"];
        let coverage = topology["coverage"].as_str().unwrap_or("unsupported");
        let rows = topology["mounts"].as_array().cloned().unwrap_or_default();
        let mut seen = std::collections::BTreeSet::new();
        for row in rows {
            let Some(path) = row["path"].as_str().filter(|p| p.len() <= 4096) else {
                continue;
            };
            let slot = row["sourceKey"].as_str().unwrap_or(path);
            let fingerprint = row["identity"].to_string();
            if !self
                .identities
                .get(slot)
                .is_some_and(|(fp, _, _)| fp == &fingerprint)
            {
                if self.identities.len() >= 512 && !self.identities.contains_key(slot) {
                    continue;
                }
                self.sequence += 1;
                let mut h = std::collections::hash_map::DefaultHasher::new();
                fingerprint.hash(&mut h);
                self.identities.insert(
                    slot.into(),
                    (
                        fingerprint,
                        format!("m{}-{:x}", self.sequence, h.finish()),
                        observed,
                    ),
                );
            }
            let (_, id, first) = self.identities.get(slot).unwrap();
            seen.insert(slot.to_string());
            self.desired.insert(("filesystem.mounts.v1".into(),id.clone()),Some(json!({"kind":"mount","mountId":id,"path":path,"pathStyle":row["pathStyle"],"filesystemView":"session-default","device":row["device"],"filesystemType":row["filesystemType"],"readOnly":row["readOnly"],"observedAt":topology["observedAt"],"staleAfterMs":60000})));
            let capacity = data["filesystems"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|c| {
                    c["path"] == path
                        && c["observedAt"].as_u64().is_some_and(|at| at >= *first)
                        && (!row["capacityIdentity"].is_string()
                            || c["id"] == row["capacityIdentity"])
                });
            if let Some(c) = capacity {
                self.desired.insert(("filesystem.capacity.v1".into(),id.clone()),Some(json!({"mountId":id,"totalBytes":c["total"],"availableBytes":c["available"],"observedAt":c["observedAt"],"staleAfterMs":120000})));
            }
        }
        if coverage == "complete" && error.is_none() {
            let removed: Vec<_> = self
                .identities
                .keys()
                .filter(|p| !seen.contains(*p))
                .cloned()
                .collect();
            for path in removed {
                let (_, id, _) = self.identities.remove(&path).unwrap();
                self.desired
                    .insert(("filesystem.mounts.v1".into(), id.clone()), None);
                self.desired
                    .insert(("filesystem.capacity.v1".into(), id), None);
            }
        }
        let live: std::collections::BTreeSet<_> = self
            .identities
            .values()
            .map(|(_, id, _)| id.clone())
            .collect();
        for ((topic, id), value) in &mut self.desired {
            if id != "scan-status"
                && !live.contains(id)
                && (topic == "filesystem.mounts.v1" || topic == "filesystem.capacity.v1")
            {
                *value = None;
            }
        }
        self.scan = json!({"kind":"scanStatus","coverage":coverage,"reason":topology["reason"],"observedAt":topology["observedAt"],"staleAfterMs":60000,"state":if paused{"paused"}else if error.is_some(){"failed"}else{"ready"},"error":error,"publishState":"publishing"});
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
        serde_json::to_vec(&json!({"desired":self.desired.iter().map(|(k,v)|json!([k,v])).collect::<Vec<_>>(),"known":self.known.iter().map(|(k,v)|json!([k,v])).collect::<Vec<_>>(),"identities":self.identities})).map_or(0,|b|b.len())
    }
}
pub fn linux_mounts(text: &str) -> Value {
    let mut complete = !text.is_empty();
    let mut rows = Vec::new();
    for line in text.lines() {
        let parsed = (|| {
            let (before, after) = line.split_once(" - ")?;
            let f: Vec<_> = before.split_whitespace().collect();
            let a: Vec<_> = after.split_whitespace().collect();
            if f.len() < 6 || a.len() < 3 {
                return None;
            }
            f[0].parse::<u64>().ok()?;
            let path = decode(f[4])?;
            let root = decode(f[3])?;
            let device = decode(a[1])?;
            if !path.starts_with('/') || path.len() > 4096 {
                return None;
            }
            Some(
                json!({"sourceKey":format!("{}:{}",f[0],path),"identity":[f[0],f[2],root,path,a[0],device],"path":path,"pathStyle":"posix","device":device,"filesystemType":a[0],"readOnly":f[5].split(',').any(|o|o=="ro")}),
            )
        })();
        if let Some(row) = parsed {
            if rows.len() < 512 {
                rows.push(row);
            } else {
                complete = false;
            }
        } else {
            complete = false;
        }
    }
    json!({"mounts":rows,"coverage":if complete{"complete"}else{"partial"},"reason":if complete{Value::Null}else{json!("mountinfo 缺项、截断或超过 512 项")},"observedAt":crate::scheduler::timestamp()})
}
fn decode(text: &str) -> Option<String> {
    let mut out = Vec::new();
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' {
            let s = b.get(i + 1..i + 4)?;
            if !s.iter().all(|c| (b'0'..=b'7').contains(c)) {
                return None;
            }
            let n = (s[0] - b'0') as u16 * 64 + (s[1] - b'0') as u16 * 8 + (s[2] - b'0') as u16;
            if n > 255 || n == 0 {
                return None;
            }
            out.push(n as u8);
            i += 4;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}
#[cfg(test)]
mod tests {
    use super::*;
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
    #[test]
    fn topology_refresh_does_not_refresh_unsampled_capacity() {
        let mut p = Publisher::default();
        let mut data = json!({"mountTopology":{"coverage":"complete","observedAt":1,"mounts":[{"path":"/","identity":"a","pathStyle":"posix"}]},"filesystems":[{"path":"/","observedAt":1,"total":"9","available":"4"}]});
        p.observe(&data, 1, None, false);
        let id = p.identities["/"].1.clone();
        data["mountTopology"]["observedAt"] = json!(100);
        p.observe(&data, 100, None, false);
        assert_eq!(
            p.desired[&("filesystem.mounts.v1".into(), id.clone())]
                .as_ref()
                .unwrap()["observedAt"],
            100
        );
        assert_eq!(
            p.desired[&("filesystem.capacity.v1".into(), id)]
                .as_ref()
                .unwrap()["observedAt"],
            1
        );
    }
    #[test]
    fn publishing_buffers_are_bounded_and_excess_keeps_last_observation() {
        let mut p = Publisher::default();
        let rows:Vec<_>=(0..512).map(|i|json!({"path":format!("/{}{}",i,"x".repeat(4000)),"identity":"x".repeat(4096),"pathStyle":"posix"})).collect();
        p.observe(
            &json!({"mountTopology":{"mounts":rows,"coverage":"complete","observedAt":1}}),
            1,
            None,
            false,
        );
        assert!(p.bytes() < 1024 * 1024);
        assert_eq!(p.scan["publishState"], "limited");
        assert!(p.identities.is_empty());
    }
    #[test]
    fn full_mountinfo_is_not_capacity_whitelist() {
        let data = linux_mounts(
            "1 0 0:1 / /proc rw - proc proc rw\n2 0 8:1 / /data\\040disk ro - ext4 /dev/sda1 ro\n",
        );
        assert_eq!(data["coverage"], "complete");
        assert_eq!(data["mounts"][1]["path"], "/data disk");
        assert_eq!(data["mounts"][1]["readOnly"], true);
        assert_eq!(linux_mounts("broken")["coverage"], "partial");
    }
    #[test]
    fn partial_scan_retains_absent_and_changed_identity_drops_capacity() {
        let mut p = Publisher::default();
        let mut d = json!({"mountTopology":{"coverage":"complete","observedAt":1,"mounts":[{"path":"/","identity":"a","pathStyle":"posix"}]},"filesystems":[{"path":"/","observedAt":1,"total":"9"}]});
        p.observe(&d, 1, None, false);
        let first = p.identities["/"].1.clone();
        d["mountTopology"]["mounts"] = json!([]);
        d["mountTopology"]["coverage"] = json!("partial");
        p.observe(&d, 2, None, false);
        assert_eq!(p.identities["/"].1, first);
        d["mountTopology"]["mounts"] = json!([{"path":"/","identity":"b","pathStyle":"posix"}]);
        p.observe(&d, 3, None, false);
        assert_ne!(p.identities["/"].1, first);
        assert_eq!(p.desired[&("filesystem.capacity.v1".into(), first)], None);
    }
}

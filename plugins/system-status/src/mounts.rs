use crate::model::{Observed, Read};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    hash::{Hash, Hasher},
};

#[derive(Clone, Debug)]
pub struct Mount {
    pub identity: String,
    pub path: String,
    pub device: String,
    pub capacity_identity: String,
    pub kind: Option<String>,
    pub read_only: Option<bool>,
    pub windows: bool,
    pub eligible: bool,
    pub system: bool,
}
#[derive(Clone, Debug)]
pub struct MountScan {
    pub rows: Vec<Mount>,
    pub complete: bool,
    pub reason: Option<String>,
}
#[derive(Clone, Debug)]
pub struct Capacity {
    pub path: String,
    pub device: String,
    pub total: u64,
    pub used: Option<u64>,
    pub available: u64,
}
#[derive(Clone, Debug)]
pub struct Target {
    pub key: String,
    pub path: String,
    pub device: String,
}
#[derive(Clone)]
pub struct Entry {
    pub key: String,
    pub mount: Mount,
    pub observed_at: u64,
    pub capacity: Observed<Capacity>,
}
#[derive(Clone, Default)]
pub struct MountCatalog {
    pub entries: BTreeMap<String, Entry>,
    pub status: Observed<()>,
    pub complete: bool,
    pub reason: Option<String>,
    sequence: u64,
}
impl MountCatalog {
    pub fn scan(&mut self, read: Read<MountScan>, at: u64) -> bool {
        let Read::Value(mut scan) = read else {
            return self.status.apply(
                match read {
                    Read::Failed(e) => Read::Failed(e),
                    _ => Read::Unsupported,
                },
                at,
            );
        };
        let mut seen = std::collections::BTreeSet::new();
        for mount in scan.rows.into_iter().take(512) {
            let slot = format!("{}\0{}", mount.path, mount.device);
            seen.insert(slot.clone());
            let changed = self
                .entries
                .get(&slot)
                .is_none_or(|e| e.mount.identity != mount.identity);
            if changed && self.entries.len() >= 512 && !self.entries.contains_key(&slot) {
                scan.complete = false;
                scan.reason = Some("挂载目录达到 512 项预算".into());
                continue;
            }
            if changed {
                self.sequence += 1;
                self.entries.insert(
                    slot.clone(),
                    Entry {
                        key: {
                            let mut hash = std::collections::hash_map::DefaultHasher::new();
                            mount.identity.hash(&mut hash);
                            format!("m{}-{:x}", self.sequence, hash.finish())
                        },
                        mount: mount.clone(),
                        observed_at: at,
                        capacity: Observed::default(),
                    },
                );
            }
            let entry = self.entries.get_mut(&slot).unwrap();
            entry.mount = mount;
            entry.observed_at = at;
        }
        // A confirmed new source at the same path invalidates the old association, even in a partial scan.
        let paths: std::collections::BTreeSet<_> = seen
            .iter()
            .filter_map(|s| s.split_once('\0').map(|(p, _)| p.to_string()))
            .collect();
        self.entries.retain(|slot, e| {
            seen.contains(slot) || (!scan.complete && !paths.contains(&e.mount.path))
        });
        self.complete = scan.complete;
        self.reason = scan.reason;
        self.status.apply(Read::Value(()), at)
    }
    pub fn targets(&self) -> Vec<Target> {
        self.entries
            .values()
            .filter(|e| e.mount.eligible)
            .map(|e| Target {
                key: e.key.clone(),
                path: e.mount.path.clone(),
                device: e.mount.capacity_identity.clone(),
            })
            .collect()
    }
    pub fn capacity(&mut self, row: Capacity, targets: &[Target], at: u64) -> bool {
        let Some(target) = targets
            .iter()
            .find(|t| t.path == row.path && t.device == row.device)
        else {
            return false;
        };
        let matches: Vec<_> = self
            .entries
            .iter()
            .filter(|(_, e)| e.mount.path == row.path)
            .map(|(k, _)| k.clone())
            .collect();
        if matches.len() != 1 {
            return false;
        }
        let entry = self.entries.get_mut(&matches[0]).unwrap();
        if entry.key != target.key || entry.mount.capacity_identity != row.device {
            return false;
        }
        if row.total == 0 && row.available != 0
            || row.available > row.total
            || row.used.is_some_and(|u| {
                u > row.total || u as u128 + row.available as u128 > row.total as u128
            })
        {
            entry
                .capacity
                .apply(Read::Failed("容量数据越界".into()), at);
            return false;
        }
        entry.capacity.apply(Read::Value(row), at)
    }
    pub fn fail_capacity(&mut self, path: &str, error: &str, at: u64) {
        for e in self
            .entries
            .values_mut()
            .filter(|e| e.mount.path == path || path.is_empty())
        {
            e.capacity.apply(Read::Failed(error.into()), at);
        }
    }
    pub fn rows(&self) -> Vec<Value> {
        self.entries.values().filter(|e|e.mount.eligible).map(|e|{
        let c=&e.capacity;let mut row=json!({"id":e.mount.capacity_identity,"path":e.mount.path,"mountKey":e.key,"mountIdentity":[e.key],"filesystemType":e.mount.kind,"readOnly":e.mount.read_only,"systemMount":e.mount.system,"pathStyle":if e.mount.windows{"windows"}else{"posix"},"observedAt":c.observed_at,"state":c.state,"reason":c.reason});
        if let Some(v)=&c.value{row["total"]=json!(v.total.to_string());row["used"]=json!(v.used.map(|v|v.to_string()));row["available"]=json!(v.available.to_string());}row
    }).collect()
    }
    pub fn bytes(&self) -> usize {
        self.entries
            .values()
            .map(|e| {
                e.key.capacity()
                    + e.mount.identity.capacity()
                    + e.mount.path.capacity() * 2
                    + e.mount.device.capacity() * 2
                    + 512
            })
            .sum()
    }
    pub fn trim(&mut self) -> bool {
        let len = self.entries.len();
        if len == 0 {
            return false;
        }
        let keys: Vec<_> = self.entries.keys().skip(len / 2).cloned().collect();
        for k in keys {
            self.entries.remove(&k);
        }
        self.complete = false;
        self.reason = Some("缓存达到预算，挂载未完整保留".into());
        true
    }
    #[cfg(test)]
    pub fn fixture(data: &Value) -> Self {
        let mut c = Self::default();
        let topology = data["mountTopology"]["mounts"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let capacities = data["filesystems"].as_array().cloned().unwrap_or_default();
        let mut rows = Vec::new();
        for r in &capacities {
            let path = r["path"].as_str().unwrap_or("").to_string();
            let m = topology.iter().find(|m| m["path"] == path).unwrap_or(r);
            let dev = r["id"].as_str().unwrap_or("").to_string();
            rows.push(Mount {
                identity: m["identity"].to_string() + &path,
                path,
                device: dev.clone(),
                capacity_identity: dev,
                kind: m["filesystemType"].as_str().map(String::from),
                read_only: m["readOnly"].as_bool(),
                windows: false,
                eligible: true,
                system: false,
            });
        }
        c.scan(
            Read::Value(MountScan {
                rows,
                complete: true,
                reason: None,
            }),
            1,
        );
        let targets = c.targets();
        for r in capacities {
            let num = |k: &str| r[k].as_str().and_then(|s| s.parse().ok());
            if let (Some(total), Some(available)) = (num("total"), num("available")) {
                if (!r["used"].is_null() && num("used").is_none())
                    || r["percent"]
                        .as_f64()
                        .is_some_and(|p| !(0.0..=100.0).contains(&p))
                {
                    continue;
                }
                c.capacity(
                    Capacity {
                        path: r["path"].as_str().unwrap_or("").into(),
                        device: r["id"].as_str().unwrap_or("").into(),
                        total,
                        used: num("used"),
                        available,
                    },
                    &targets,
                    r["observedAt"].as_u64().unwrap_or(1),
                );
            }
        }
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scan(device: &str, ro: bool) -> Read<MountScan> {
        Read::Value(MountScan {
            rows: vec![Mount {
                identity: device.into(),
                path: "/data".into(),
                device: device.into(),
                capacity_identity: device.into(),
                kind: Some("apfs".into()),
                read_only: Some(ro),
                windows: false,
                eligible: true,
                system: false,
            }],
            complete: true,
            reason: None,
        })
    }
    #[test]
    fn capacity_is_bound_to_captured_identity_and_attributes_do_not_retime_it() {
        let mut c = MountCatalog::default();
        c.scan(scan("a", false), 1);
        let targets = c.targets();
        let cap = Capacity {
            path: "/data".into(),
            device: "a".into(),
            total: 100,
            used: Some(20),
            available: 70,
        };
        assert!(c.capacity(cap.clone(), &targets, 2));
        c.scan(scan("a", true), 3);
        assert_eq!(c.rows()[0]["observedAt"], 2);
        let key = c.rows()[0]["mountKey"].clone();
        c.scan(scan("b", false), 4);
        assert_ne!(c.rows()[0]["mountKey"], key);
        assert!(!c.capacity(cap, &targets, 5));
        assert!(c.rows()[0]["total"].is_null());
    }
}

//! Private display projection; collection and shared mount publication remain complete.
use serde_json::{json, Value};
use std::hash::{Hash, Hasher};

fn mount<'a>(topology: &'a Value, capacity: &Value) -> Option<&'a Value> {
    let mut matches = topology["mounts"]
        .as_array()?
        .iter()
        .filter(|m| m["path"] == capacity["path"]);
    let first = matches.next()?;
    let same_source = if first["capacityIdentity"].is_string() {
        first["capacityIdentity"] == capacity["id"]
    } else {
        first["device"] == capacity["id"]
    };
    (matches.next().is_none() && same_source && first["identity"].is_array()).then_some(first)
}

pub fn associate(row: &mut Value, topology: &Value) {
    row["mountIdentity"] = mount(topology, row)
        .map(|m| m["identity"].clone())
        .unwrap_or(Value::Null);
}

pub fn reconcile(data: &mut Value) {
    let topology = data["mountTopology"].clone();
    for row in data["filesystems"].as_array_mut().into_iter().flatten() {
        // A path can be remounted between capacity rounds. Do not relabel its old sample.
        let at_path: Vec<_> = topology["mounts"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|m| m["path"] == row["path"])
            .collect();
        if row["mountIdentity"].is_array()
            && !at_path.is_empty()
            && !at_path
                .iter()
                .any(|m| m["identity"] == row["mountIdentity"])
        {
            for key in [
                "total",
                "used",
                "available",
                "percent",
                "observedAt",
                "mountIdentity",
            ] {
                row.as_object_mut().unwrap().remove(key);
            }
        }
    }
}

fn bytes(row: &Value, key: &str) -> Option<u64> {
    row[key].as_str()?.parse().ok()
}
fn runtime(path: &str) -> bool {
    path == "/run"
        || path.starts_with("/run/credentials/")
        || path == "/run/snapd/ns"
        || path == "/dev/shm"
        || path
            .strip_prefix("/run/user/")
            .is_some_and(|uid| !uid.is_empty() && uid.bytes().all(|b| b.is_ascii_digit()))
}
fn local_partition(kind: &str) -> bool {
    matches!(
        kind,
        "ext2"
            | "ext3"
            | "ext4"
            | "xfs"
            | "btrfs"
            | "vfat"
            | "fat"
            | "msdos"
            | "f2fs"
            | "jfs"
            | "reiserfs"
            | "zfs"
            | "ufs"
    )
}

pub fn project(data: &Value) -> Vec<Value> {
    let mut rows: Vec<_> = data["filesystems"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| {
            let m =
                mount(&data["mountTopology"], c).filter(|m| m["identity"] == c["mountIdentity"]);
            let kind = m.and_then(|m| m["filesystemType"].as_str());
            let read_only = m.and_then(|m| m["readOnly"].as_bool());
            let path = c["path"].as_str().unwrap_or("");
            let is_runtime = m.is_some_and(|m| m["pathStyle"] == "posix")
                && kind.is_some_and(|k| matches!(k, "tmpfs" | "ramfs"))
                && runtime(path);
            let image = kind == Some("squashfs") && read_only == Some(true);
            let (total, used, available) =
                (bytes(c, "total"), bytes(c, "used"), bytes(c, "available"));
            let used = if c["used"].is_null() {
                total.and_then(|t| t.checked_sub(available?))
            } else {
                used
            };
            let capacity = total.zip(used).zip(available).filter(|&((t, u), a)| {
                t > 0
                    && u <= t
                    && a <= t
                    && u as u128 + a as u128 <= t as u128
                    && (c["percent"].is_null()
                        || c["percent"]
                            .as_f64()
                            .is_some_and(|p| p.is_finite() && (0.0..=100.0).contains(&p)))
            });
            let state = if is_runtime && total == Some(0) && used == Some(0) && available == Some(0)
            {
                "notApplicable"
            } else if capacity.is_none() {
                "unknown"
            } else if image {
                "fixedImage"
            } else {
                "known"
            };
            let warning = state == "known"
                && capacity.is_some_and(|((t, u), _)| u as u128 * 100 >= t as u128 * 85);
            let group = if warning || path == "/" {
                "primary"
            } else if image || is_runtime {
                "system"
            } else if kind.is_some_and(local_partition) && matches!(path, "/boot" | "/boot/efi") {
                "auxiliary"
            } else {
                "primary"
            };
            let mut hash = std::collections::hash_map::DefaultHasher::new();
            if let Some(m) = m {
                m["identity"].to_string().hash(&mut hash);
            } else {
                (path, c["id"].to_string()).hash(&mut hash);
            }
            let mut row = c.clone();
            row.as_object_mut().unwrap().remove("mountIdentity");
            row["mountKey"] = json!(format!("fs-{:016x}", hash.finish()));
            row["filesystemType"] = json!(kind);
            row["readOnly"] = json!(read_only);
            row["displayGroup"] = json!(group);
            row["capacityState"] = json!(state);
            row["spaceWarning"] = json!(warning);
            row["percent"] =
                json!(capacity.map(|((t, u), _)| (u as u128 * 10000 / t as u128) as f64 / 100.0));
            row["used"] = json!(used.map(|u| u.to_string()));
            row
        })
        .collect();
    rows.sort_by(|a, b| {
        let rank = |r: &Value| {
            if r["path"] == "/" {
                0
            } else if r["spaceWarning"] == true {
                1
            } else {
                2
            }
        };
        rank(a)
            .cmp(&rank(b))
            .then_with(|| {
                if a["spaceWarning"] == true && b["spaceWarning"] == true {
                    b["percent"]
                        .as_f64()
                        .partial_cmp(&a["percent"].as_f64())
                        .unwrap_or(std::cmp::Ordering::Equal)
                } else {
                    std::cmp::Ordering::Equal
                }
            })
            .then_with(|| a["path"].as_str().cmp(&b["path"].as_str()))
            .then_with(|| a["mountKey"].as_str().cmp(&b["mountKey"].as_str()))
    });
    rows
}

pub fn summary(rows: &[Value]) -> Value {
    let count = |group| rows.iter().filter(|r| r["displayGroup"] == group).count();
    json!({"primary":count("primary"),"auxiliary":count("auxiliary"),"system":count("system"),
        "warnings":rows.iter().filter(|r| r["spaceWarning"] == true).count()})
}
pub fn revision(rows: &[Value]) -> String {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    for row in rows {
        (
            &row["mountKey"].to_string(),
            &row["displayGroup"].to_string(),
        )
            .hash(&mut hash);
    }
    format!("{:016x}", hash.finish())
}
pub fn select(rows: Vec<Value>, group: &str) -> Vec<Value> {
    if group != "overview" {
        return rows
            .into_iter()
            .filter(|r| r["displayGroup"] == group)
            .collect();
    }
    let required = rows
        .iter()
        .filter(|r| {
            r["displayGroup"] == "primary" && (r["path"] == "/" || r["spaceWarning"] == true)
        })
        .count();
    let mut remaining = 3usize.saturating_sub(required);
    rows.into_iter()
        .filter(|r| {
            if r["displayGroup"] != "primary" {
                false
            } else if r["path"] == "/" || r["spaceWarning"] == true {
                true
            } else if remaining > 0 {
                remaining -= 1;
                true
            } else {
                false
            }
        })
        .collect()
}

#[cfg(test)]
pub mod tests {
    use super::*;
    pub fn screenshot() -> Value {
        let mut data = json!({"filesystems":[],"mountTopology":{"mounts":[]}});
        for (i, (path, kind, ro, total, used)) in [
            ("/", "ext4", false, 1000, 200),
            ("/mnt/data", "ext4", false, 1000, 300),
            ("/snap/core20/2318", "squashfs", true, 1000, 1000),
            ("/snap/core20/2434", "squashfs", true, 1000, 1000),
            ("/snap/core22/100", "squashfs", true, 1000, 1000),
            ("/snap/core22/101", "squashfs", true, 1000, 1000),
            ("/snap/lxd/1", "squashfs", true, 1000, 1000),
            ("/snap/snapd/1", "squashfs", true, 1000, 1000),
            ("/snap/snapd/2", "squashfs", true, 1000, 1000),
            ("/run", "tmpfs", false, 1000, 10),
            (
                "/run/credentials/systemd-sysusers.service",
                "ramfs",
                true,
                0,
                0,
            ),
            ("/dev/shm", "tmpfs", false, 1000, 0),
            ("/boot", "ext4", false, 1000, 200),
            ("/boot/efi", "vfat", false, 1000, 50),
        ]
        .into_iter()
        .enumerate()
        {
            let m = json!({"identity":[i,path,kind],"path":path,"device":format!("dev{i}"),"pathStyle":"posix","filesystemType":kind,"readOnly":ro});
            let mut c = json!({"path":path,"id":m["device"],"total":total.to_string(),"used":used.to_string(),"available":(total-used).to_string(),"observedAt":10});
            data["mountTopology"]["mounts"]
                .as_array_mut()
                .unwrap()
                .push(m);
            associate(&mut c, &data["mountTopology"]);
            data["filesystems"].as_array_mut().unwrap().push(c);
        }
        data
    }
    #[test]
    fn screenshot_hides_twelve_mounts_and_promotes_real_risks() {
        let mut data = screenshot();
        assert_eq!(
            summary(&project(&data)),
            json!({"primary":2,"auxiliary":2,"system":10,"warnings":0})
        );
        assert_eq!(select(project(&data), "overview").len(), 2);
        for i in [1, 9, 12] {
            data["filesystems"][i]["used"] = json!("900");
            data["filesystems"][i]["available"] = json!("100");
        }
        assert_eq!(
            summary(&project(&data)),
            json!({"primary":4,"auxiliary":1,"system":9,"warnings":3})
        );
        assert_eq!(select(project(&data), "overview").len(), 4);
    }
    #[test]
    fn uncertain_metadata_and_business_volumes_stay_visible() {
        for (path, kind, ro) in [
            ("/snap/business-data", "ext4", false),
            ("/data", "ext4", true),
            ("/tmp", "tmpfs", false),
            ("/run/business", "tmpfs", false),
            ("/", "overlay", false),
            ("/boot", "unknown", false),
        ] {
            let mut d = screenshot();
            d["filesystems"] =
                json!([{"path":path,"id":"dev","total":"1000","used":"900","available":"100"}]);
            d["mountTopology"]["mounts"] = json!([{"path":path,"device":"dev","identity":[path,kind],"pathStyle":"posix","filesystemType":kind,"readOnly":ro}]);
            let topology = d["mountTopology"].clone();
            associate(&mut d["filesystems"][0], &topology);
            assert_eq!(project(&d)[0]["spaceWarning"], true);
            d["mountTopology"]["mounts"][0]["readOnly"] = Value::Null;
            assert_eq!(project(&d)[0]["displayGroup"], "primary");
        }
    }
    #[test]
    fn changed_identity_clears_capacity_but_attribute_changes_keep_time() {
        let mut d = screenshot();
        let old = project(&d)[0]["mountKey"].clone();
        d["mountTopology"]["mounts"][0]["readOnly"] = json!(true);
        reconcile(&mut d);
        assert_eq!(project(&d)[0]["observedAt"], 10);
        d["mountTopology"]["mounts"][0]["identity"] = json!(["replacement"]);
        reconcile(&mut d);
        assert_eq!(project(&d)[0]["capacityState"], "unknown");
        assert_ne!(project(&d)[0]["mountKey"], old);
        assert!(project(&d)[0]["observedAt"].is_null());
        d["mountTopology"]["mounts"] = json!([]);
        assert_eq!(project(&d).len(), 14);
        assert!(project(&d).iter().all(|r| r["displayGroup"] == "primary"));
    }
    #[test]
    fn byte_validation_is_exact_and_unknown_is_not_full() {
        let mut d = json!({"filesystems":[{"id":"win","path":"D:\\","total":"9007199254740993","available":"1"}]});
        assert_eq!(project(&d)[0]["used"], "9007199254740992");
        assert_eq!(project(&d)[0]["spaceWarning"], true);
        for changes in [
            json!({"total":"0"}),
            json!({"available":"9007199254740994"}),
            json!({"used":"invalid"}),
            json!({"percent":101}),
            json!({"used":"9007199254740993"}),
            json!({"total":null}),
        ] {
            let mut sample = d.clone();
            sample["filesystems"][0]
                .as_object_mut()
                .unwrap()
                .extend(changes.as_object().unwrap().clone());
            assert_eq!(project(&sample)[0]["capacityState"], "unknown", "{changes}");
            assert_eq!(project(&sample)[0]["spaceWarning"], false);
        }
        d["filesystems"][0]["available"] = json!("1351079888211150");
        assert_eq!(project(&d)[0]["spaceWarning"], false);
    }
}

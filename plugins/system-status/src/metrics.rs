use crate::collectors::{self, Family};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap},
    time::Instant,
};
#[derive(Default)]
pub struct Baseline {
    pub previous: Option<Raw>,
}
pub struct Raw {
    time: f64,
    boot: String,
    cpu: Option<(u64, u64)>,
    counters: HashMap<String, (u64, u64)>,
    counter_clocks: HashMap<String, (u64, u64)>,
    local: Instant,
}
impl Baseline {
    pub fn memory_usage(&self) -> usize {
        self.previous
            .as_ref()
            .map(|r| {
                r.boot.capacity()
                    + r.counters.capacity() * 96
                    + r.counters.keys().map(String::capacity).sum::<usize>()
                    + r.counter_clocks.capacity() * 96
                    + r.counter_clocks.keys().map(String::capacity).sum::<usize>()
            })
            .unwrap_or(0)
    }
}
fn number(v: &Value) -> Option<u64> {
    v.as_str()?.parse().ok()
}
fn metric(value: Option<f64>, source: &str) -> Value {
    json!({"value":value.filter(|v|v.is_finite()),"state":if value.is_some(){"ok"}else{"warmingUp"},"source":source})
}
fn count(text: &str) -> Option<u64> {
    text.trim().parse().ok()
}
pub fn apply(
    family: Family,
    group: &str,
    text: &str,
    nonce: &str,
    paths: &[String],
    snapshot: &mut Value,
    base: &mut Baseline,
    interval: u64,
) -> Result<(), String> {
    let sections = if family == Family::Windows {
        BTreeMap::new()
    } else {
        collectors::sections(text, nonce)?
    };
    let win = if family == Family::Windows {
        collectors::windows_json(text, nonce)?
    } else {
        Value::Null
    };
    if family == Family::Windows {
        snapshot["sourceErrors"][group] = Value::Object(
            win["errors"]
                .as_object()
                .into_iter()
                .flatten()
                .take(16)
                .map(|(key, value)| {
                    (
                        limited(key, 64),
                        json!(limited(value.as_str().unwrap_or("数据源不可用"), 256)),
                    )
                })
                .collect(),
        );
        if win.as_object().is_none_or(|o| {
            o.keys()
                .all(|k| ["schemaVersion", "nonce", "errors"].contains(&k.as_str()))
        }) {
            return Err("此数据组没有可用的数据源，详情见采集详情".into());
        }
    }
    if family != Family::Windows && sections.is_empty() {
        return Err("此数据组没有可用的数据源".into());
    }
    let get = |key: &str| sections.get(key).map(String::as_str).unwrap_or("");
    match group {
        "identity" => {
            let (system, hostname, model, cores, total) = if family == Family::Windows {
                let cores = win["cpu"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|v| v["cores"].as_u64()).sum::<u64>());
                (
                    win["os"]["system"].as_str().unwrap_or("").into(),
                    win["os"]["hostname"].as_str().unwrap_or("").into(),
                    win["cpu"][0]["model"]
                        .as_str()
                        .or(win["device"]["model"].as_str())
                        .unwrap_or("")
                        .into(),
                    cores,
                    number(&win["os"]["totalBytes"]),
                )
            } else if family == Family::Linux {
                let os = get("os")
                    .lines()
                    .find_map(|l| l.strip_prefix("PRETTY_NAME="))
                    .map(|s| s.trim_matches(['"', '\'']).to_string())
                    .unwrap_or_else(|| get("system").trim().into());
                let model = get("cpuinfo")
                    .lines()
                    .filter_map(|l| l.split_once(':'))
                    .find(|(k, _)| ["model name", "Hardware", "Processor"].contains(&k.trim()))
                    .map(|(_, v)| v.trim().to_string())
                    .unwrap_or_else(|| get("model").trim_matches(['\0', '\n']).into());
                let cores = get("cpuinfo")
                    .lines()
                    .filter(|l| {
                        l.split_once(':')
                            .is_some_and(|(k, _)| k.trim() == "processor")
                    })
                    .count();
                (
                    os,
                    get("hostname").trim().into(),
                    model,
                    (cores > 0).then_some(cores as u64),
                    None,
                )
            } else {
                (
                    (if get("os").is_empty() {
                        get("system")
                    } else {
                        get("os")
                    })
                    .trim()
                    .to_string(),
                    get("hostname").trim().to_string(),
                    get("model").trim().to_string(),
                    count(get("cores")),
                    count(get("total")),
                )
            };
            snapshot["identity"] = json!({"system":limited(&system,256),"hostname":limited(&hostname,256),"cpuModel":limited(&model,256),"cores":cores});
            if let Some(total) = total {
                snapshot["memoryTotal"] = json!(total.to_string());
            }
        }
        "dynamic" => {
            let now = Instant::now();
            let uptime = if family == Family::Linux {
                get("uptime")
                    .split_whitespace()
                    .next()
                    .and_then(|s| s.parse::<f64>().ok())
            } else {
                None
            };
            let mut raw = Raw {
                time: uptime
                    .unwrap_or_else(|| snapshot["originMs"].as_f64().unwrap_or(0.0) / 1000.0),
                boot: get("boot").trim().into(),
                cpu: None,
                counters: HashMap::new(),
                counter_clocks: HashMap::new(),
                local: now,
            };
            if family == Family::Linux {
                let values: Vec<u64> = get("cpu")
                    .lines()
                    .find(|l| l.starts_with("cpu "))
                    .unwrap_or("")
                    .split_whitespace()
                    .skip(1)
                    .take(8)
                    .map(str::parse)
                    .collect::<Result<_, _>>()
                    .map_err(|_| "CPU 计数无效")?;
                if values.len() >= 4 {
                    raw.cpu = values
                        .iter()
                        .try_fold(0u64, |n, &v| n.checked_add(v))
                        .map(|total| {
                            (
                                total,
                                values[3].saturating_add(values.get(4).copied().unwrap_or(0)),
                            )
                        });
                }
                let m = meminfo(get("memory"));
                let total = m.get("MemTotal").copied();
                let available = m.get("MemAvailable").copied().or_else(|| {
                    let free = *m.get("MemFree")?;
                    Some(
                        free.saturating_add(*m.get("Buffers")?)
                            .saturating_add(*m.get("Cached")?)
                            .saturating_add(m.get("SReclaimable").copied().unwrap_or(0))
                            .saturating_sub(m.get("Shmem").copied().unwrap_or(0)),
                    )
                });
                if let Some(total) = total {
                    snapshot["memoryTotal"] = json!(total.to_string());
                    snapshot["memory"] = json!({"value":available.filter(|&a|a<=total && total>0).map(|a|(total-a) as f64/total as f64*100.0),"state":if available.is_some(){"ok"}else{"unavailable"},"total":total.to_string(),"available":available.map(|n|n.to_string()),"estimated":!m.contains_key("MemAvailable"),"source":"procfs"});
                } else {
                    snapshot["memory"] =
                        json!({"value":null,"state":"unavailable","reason":"内存数据源不可读"});
                }
                snapshot["swap"] = json!({"total":m.get("SwapTotal").map(u64::to_string),"available":m.get("SwapFree").map(u64::to_string)});
                for line in get("network").lines().skip(2) {
                    if let Some((name, values)) = line.split_once(':') {
                        let n: Vec<_> = values.split_whitespace().collect();
                        if let (Some(a), Some(b)) = (
                            n.first().and_then(|n| count(n)),
                            n.get(8).and_then(|n| count(n)),
                        ) {
                            let name = name.trim();
                            let identity = snapshot["interfaces"]
                                .as_array()
                                .and_then(|a| a.iter().find(|v| v["name"] == name))
                                .and_then(|v| v["id"].as_str())
                                .unwrap_or(name);
                            raw.counters.insert(format!("net:{identity}"), (a, b));
                        }
                    }
                }
                let mut disks = Vec::new();
                for line in get("disk").lines() {
                    let n: Vec<_> = line.split_whitespace().collect();
                    if n.len() < 14 {
                        continue;
                    }
                    let name = n[2];
                    if name.starts_with("loop") || name.starts_with("ram") {
                        continue;
                    }
                    if let (Some(a), Some(b)) = (
                        count(n[5]).and_then(|n| n.checked_mul(512)),
                        count(n[9]).and_then(|n| n.checked_mul(512)),
                    ) {
                        let id = format!("{}:{}", n[0], n[1]);
                        raw.counters.insert(format!("disk:{id}"), (a, b));
                        disks.push(json!({"id":id,"name":name}));
                    }
                }
                disks.truncate(256);
                snapshot["disks"] = json!(disks);
                if let Some(up) = uptime {
                    snapshot["uptime"] = json!({"seconds":up,"estimated":false});
                }
            } else if family == Family::Windows {
                if let (Some(t), Some(idle)) =
                    (number(&win["cpu"]["time"]), number(&win["cpu"]["idle"]))
                {
                    raw.cpu = Some((t, idle));
                    raw.time = t as f64 / 10_000_000.0;
                    raw.boot = "windows-perf".into();
                }
                if let (Some(total), Some(available)) = (
                    number(&snapshot["memoryTotal"]),
                    number(&win["memory"]["availableBytes"]),
                ) {
                    snapshot["memory"] = json!({"value":(available<=total && total>0).then(||(total-available) as f64/total as f64*100.0),"state":"ok","total":total.to_string(),"available":available.to_string(),"source":"Windows 可见内存"});
                } else {
                    snapshot["memory"] = json!({"value":null,"state":"unavailable","reason":"Windows 内存数据源不可读"});
                }
                for (kind, field, a, b) in [
                    ("net", "network", "received", "sent"),
                    ("disk", "disks", "read", "write"),
                ] {
                    let mut rows = Vec::new();
                    for v in win[field].as_array().into_iter().flatten().take(256) {
                        if let (Some(id), Some(x), Some(y)) =
                            (v["id"].as_str(), number(&v[a]), number(&v[b]))
                        {
                            let key = format!("{kind}:{id}");
                            raw.counters.insert(key.clone(), (x, y));
                            if let (Some(time), Some(frequency)) = (
                                number(&v["time"]),
                                number(&v["frequency"]).filter(|n| *n > 0),
                            ) {
                                raw.counter_clocks.insert(key, (time, frequency));
                            }
                            rows.push(json!({"id":limited(id,256),"name":limited(v["name"].as_str().unwrap_or(id),256),"counterInstance":kind=="net"}));
                        }
                    }
                    snapshot[if kind == "net" {
                        "counterInterfaces"
                    } else {
                        "disks"
                    }] = json!(rows);
                }
            } else {
                if family == Family::Darwin {
                    let cpu = get("cpu")
                        .lines()
                        .filter(|l| l.starts_with("CPU usage:"))
                        .last()
                        .and_then(|l| l.split(',').last())
                        .and_then(|l| l.split('%').next())
                        .and_then(|s| s.trim().parse::<f64>().ok())
                        .filter(|v| (0.0..=100.0).contains(v));
                    snapshot["cpu"] = metric(cpu.map(|v| 100.0 - v), "top · 1 秒观察窗口");
                    snapshot["memoryDetails"] = json!(limited(get("memory"), 4096));
                } else {
                    let n: Vec<u64> = get("cpu")
                        .split_whitespace()
                        .map(str::parse)
                        .collect::<Result<_, _>>()
                        .unwrap_or_default();
                    if n.len() == 5 {
                        raw.cpu = n
                            .iter()
                            .try_fold(0u64, |a, &b| a.checked_add(b))
                            .map(|t| (t, n[4]));
                    }
                }
                for (id, a, b) in netstat(get("network")) {
                    raw.counters.insert(format!("net:{id}"), (a, b));
                }
            }
            let compatible = base.previous.as_ref().filter(|p| {
                p.boot == raw.boot
                    && now.duration_since(p.local).as_secs_f64() <= interval as f64 * 3.0
                    && (uptime.is_none() || raw.time > p.time)
            });
            let elapsed = compatible
                .map(|p| {
                    if family == Family::Windows {
                        raw.cpu
                            .zip(p.cpu)
                            .and_then(|(n, p)| n.0.checked_sub(p.0))
                            .map(|delta| delta as f64 / 10_000_000.0)
                            .unwrap_or(0.0)
                    } else if family == Family::Linux {
                        raw.time - p.time
                    } else {
                        now.duration_since(p.local).as_secs_f64()
                    }
                })
                .filter(|n| *n > 0.0 && n.is_finite());
            if family != Family::Darwin {
                let cpu = compatible.and_then(|p| {
                    let (t, i) = raw.cpu?;
                    let (pt, pi) = p.cpu?;
                    let total = t.checked_sub(pt)?;
                    let idle = i.checked_sub(pi)?;
                    (total > 0 && idle <= total)
                        .then(|| (total - idle) as f64 / total as f64 * 100.0)
                });
                snapshot["cpu"] = metric(
                    cpu,
                    if family == Family::Windows {
                        "Windows inverse timer"
                    } else {
                        "累计 CPU 时间"
                    },
                );
            }
            let mut rates = serde_json::Map::new();
            for (key, &(a, b)) in raw.counters.iter().take(512) {
                let previous = compatible.and_then(|p| p.counters.get(key));
                let window = if family == Family::Windows {
                    compatible
                        .and_then(|p| raw.counter_clocks.get(key).zip(p.counter_clocks.get(key)))
                        .and_then(|(&(t, f), &(pt, pf))| {
                            (f == pf)
                                .then(|| t.checked_sub(pt).map(|n| n as f64 / f as f64))
                                .flatten()
                        })
                        .filter(|n| *n > 0.0)
                } else {
                    elapsed
                };
                let rate = previous.and_then(|&(x, y)| {
                    Some((
                        a.checked_sub(x)? as f64 / window?,
                        b.checked_sub(y)? as f64 / window?,
                    ))
                });
                rates.insert(key.clone(),json!({"first":rate.map(|v|v.0),"second":rate.map(|v|v.1),"state":if rate.is_some(){"ok"}else{"warmingUp"},"estimated":!matches!(family,Family::Linux|Family::Windows)}));
            }
            snapshot["rates"] = Value::Object(rates);
            snapshot["cpu"]["windowMs"] = if family == Family::Darwin {
                json!(1000)
            } else {
                json!(elapsed.map(|n| n * 1000.0))
            };
            base.previous = Some(raw);
        }
        "topology" => {
            if family == Family::Linux {
                let rows:Vec<_>=get("interfaces").lines().filter_map(|l|{let n:Vec<_>=l.split('|').collect();if n.len()<4 {return None;}let addresses:Vec<_>=get("addresses").lines().filter(|l|l.split_whitespace().nth(1).is_some_and(|s|s.trim_end_matches(':')==n[0])).filter_map(|l|l.split_whitespace().nth(3)).map(|s|limited(s,128)).collect();Some(json!({"id":format!("{}:{}:{}",n[0],n[1],n[2]),"name":limited(n[0],256),"state":n[3],"addresses":addresses}))}).take(256).collect();
                snapshot["interfaces"] = json!(rows);
                let mounts = collectors::mount_paths(get("mounts"));
                if let Some(rows) = snapshot["filesystems"].as_array_mut() {
                    rows.retain(|r| {
                        r["path"]
                            .as_str()
                            .is_some_and(|p| mounts.iter().any(|m| m == p))
                    });
                }
                snapshot["mountPaths"] = json!(mounts);
                snapshot["load"] = json!(get("load")
                    .split_whitespace()
                    .take(3)
                    .map(str::to_owned)
                    .collect::<Vec<_>>());
            } else if family == Family::Windows {
                snapshot["interfaces"] = json!(win["interfaces"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .take(256)
                    .cloned()
                    .collect::<Vec<_>>());
            } else {
                snapshot["loadText"] = json!(limited(get("load").trim(), 512));
                let names: Vec<_> = get("interfaces")
                    .lines()
                    .filter(|l| !l.starts_with(char::is_whitespace))
                    .filter_map(|l| l.split_once(':'))
                    .map(|(name, _)| json!({"id":name,"name":name}))
                    .take(256)
                    .collect();
                snapshot["interfaces"] = json!(names);
            }
        }
        "capacity" => {
            let observed = crate::scheduler::timestamp();
            if family == Family::Windows {
                snapshot["filesystems"] = json!(win["filesystems"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .take(512)
                    .cloned()
                    .map(|mut row| {
                        row["observedAt"] = json!(observed);
                        row
                    })
                    .collect::<Vec<_>>());
            } else {
                let mut rows = if family == Family::Linux {
                    snapshot["filesystems"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default()
                } else {
                    vec![]
                };
                for (i, path) in paths.iter().enumerate().take(16) {
                    if let Some(mut fs) = df(get(&format!("fs{i}")), Some(path)) {
                        fs["observedAt"] = json!(observed);
                        rows.retain(|v| v["path"] != *path);
                        rows.push(fs);
                    }
                }
                if family != Family::Linux {
                    rows = df_all(get("filesystems"));
                    for row in &mut rows {
                        row["observedAt"] = json!(observed);
                    }
                }
                rows.truncate(512);
                snapshot["filesystems"] = json!(rows);
            }
        }
        "uptime" => {
            let seconds = if family == Family::Windows {
                number(&win["uptime"]["seconds"]).map(|n| n as f64)
            } else if family == Family::Linux {
                get("uptime")
                    .split_whitespace()
                    .next()
                    .and_then(|s| s.parse::<f64>().ok())
            } else {
                let boot = get("boottime")
                    .split("sec =")
                    .nth(1)
                    .and_then(|s| s.split(',').next())
                    .and_then(|s| s.trim().parse::<u64>().ok())
                    .or_else(|| get("boottime").split_whitespace().next().and_then(count));
                count(get("now"))
                    .zip(boot)
                    .and_then(|(now, boot)| now.checked_sub(boot))
                    .map(|n| n as f64)
            };
            if let Some(seconds) = seconds.filter(|n| *n >= 0.0 && n.is_finite()) {
                snapshot["uptime"] = json!({"seconds":seconds,"estimated":!matches!(family,Family::Linux|Family::Windows)});
            }
        }
        _ => {}
    }
    Ok(())
}
pub fn limited(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}
fn meminfo(text: &str) -> HashMap<String, u64> {
    text.lines()
        .filter_map(|l| {
            let (k, v) = l.split_once(':')?;
            let n = v
                .split_whitespace()
                .next()?
                .parse::<u64>()
                .ok()?
                .checked_mul(1024)?;
            Some((k.into(), n))
        })
        .collect()
}
fn netstat(text: &str) -> Vec<(String, u64, u64)> {
    let mut lines = text.lines();
    let header: Vec<_> = lines.next().unwrap_or("").split_whitespace().collect();
    let (Some(a), Some(b)) = (
        header.iter().position(|&s| s == "Ibytes"),
        header.iter().position(|&s| s == "Obytes"),
    ) else {
        return vec![];
    };
    let mut seen = std::collections::HashSet::new();
    lines
        .filter_map(|l| {
            let f: Vec<_> = l.split_whitespace().collect();
            if !f.iter().any(|s| s.starts_with("<Link")) || !seen.insert(*f.first()?) {
                return None;
            }
            Some((f[0].to_string(), count(f.get(a)?)?, count(f.get(b)?)?))
        })
        .take(256)
        .collect()
}
fn df(text: &str, path: Option<&String>) -> Option<Value> {
    df_all(text).into_iter().next().map(|mut v| {
        if let Some(path) = path {
            v["path"] = json!(path);
        }
        v
    })
}
fn df_all(text: &str) -> Vec<Value> {
    text.lines().skip(1).filter_map(|l|{let f:Vec<_>=l.split_whitespace().collect();if f.len()<6 {return None;}let(total,used,available)=(count(f[1])?.checked_mul(1024)?,count(f[2])?.checked_mul(1024)?,count(f[3])?.checked_mul(1024)?);Some(json!({"id":f[0],"path":limited(&f[5..].join(" "),4096),"total":total.to_string(),"used":used.to_string(),"available":available.to_string(),"percent":(total>0).then(||used as f64/total as f64*100.0)}))}).take(512).collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn windows_source_failures_remain_visible_and_all_failed_sources_retry() {
        let mut data = json!({});
        let sample =
            json!({"schemaVersion":1,"nonce":"n","errors":{"cpu":"数据源不可用：AccessDenied"}})
                .to_string();
        assert!(apply(
            Family::Windows,
            "dynamic",
            &sample,
            "n",
            &[],
            &mut data,
            &mut Baseline::default(),
            5
        )
        .is_err());
        assert_eq!(
            data["sourceErrors"]["dynamic"]["cpu"],
            "数据源不可用：AccessDenied"
        );
    }
    #[test]
    fn capacity_batches_do_not_refresh_other_mount_timestamps() {
        let mut data =
            json!({"filesystems":[{"path":"/a","observedAt":1},{"path":"/b","observedAt":2}]});
        let sample = "SD1 n BEGIN fs0\nFilesystem 1024-blocks Used Available Capacity Mounted on\n/dev/root 100 40 50 40% /a\nSD1 n END fs0 0\nSD1 n DONE\n";
        apply(
            Family::Linux,
            "capacity",
            sample,
            "n",
            &["/a".into()],
            &mut data,
            &mut Baseline::default(),
            5,
        )
        .unwrap();
        let rows = data["filesystems"].as_array().unwrap();
        assert_eq!(
            rows.iter().find(|r| r["path"] == "/b").unwrap()["observedAt"],
            2
        );
        assert!(
            rows.iter().find(|r| r["path"] == "/a").unwrap()["observedAt"]
                .as_u64()
                .unwrap()
                > 2
        );
    }
    fn output(cpu: &str, uptime: &str) -> String {
        format!("SD1 n BEGIN cpu\n{cpu}\nSD1 n END cpu 0\nSD1 n BEGIN uptime\n{uptime}\nSD1 n END uptime 0\nSD1 n DONE\n")
    }
    #[test]
    fn guest_is_not_counted_twice_and_large_counts_subtract_before_float() {
        let mut snapshot = json!({});
        let mut base = Baseline::default();
        apply(
            Family::Linux,
            "dynamic",
            &output("cpu 9007199254740993 0 0 10 0 0 0 0 500 0", "10"),
            "n",
            &[],
            &mut snapshot,
            &mut base,
            5,
        )
        .unwrap();
        assert!(snapshot["cpu"]["value"].is_null());
        apply(
            Family::Linux,
            "dynamic",
            &output("cpu 9007199254741003 0 0 20 0 0 0 0 510 0", "15"),
            "n",
            &[],
            &mut snapshot,
            &mut base,
            5,
        )
        .unwrap();
        assert_eq!(snapshot["cpu"]["value"], 50.0);
        apply(
            Family::Linux,
            "dynamic",
            &output("cpu 1 0 0 1 0 0 0 0", "1"),
            "n",
            &[],
            &mut snapshot,
            &mut base,
            5,
        )
        .unwrap();
        assert!(snapshot["cpu"]["value"].is_null());
    }
    #[test]
    fn memory_and_reserved_space_are_not_zeroed_or_relabelled() {
        assert_eq!(meminfo("MemTotal: 100 kB")["MemTotal"], 102400);
        assert_eq!(df("Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/a 100 40 50 40% /data disk\n",None).unwrap()["available"],"51200");
    }
    #[test]
    #[ignore = "requires a local Linux Docker image; SODECK_STATUS_TEST_IMAGE"]
    fn linux_procfs_collects_on_gnu_and_busybox_shells() {
        let image = std::env::var("SODECK_STATUS_TEST_IMAGE").unwrap();
        let mut snapshot = json!({});
        let mut base = Baseline::default();
        for group in [
            "identity", "dynamic", "topology", "capacity", "uptime", "dynamic",
        ] {
            let paths: Vec<String> = snapshot["mountPaths"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_str().map(str::to_string))
                .take(16)
                .collect();
            let command = collectors::command(Family::Linux, group, "sample", &paths);
            let output = std::process::Command::new("docker")
                .args(["run", "--rm", "--entrypoint", "sh", &image, "-c", &command])
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let text = String::from_utf8(output.stdout).unwrap();
            apply(
                Family::Linux,
                group,
                &text,
                "sample",
                &paths,
                &mut snapshot,
                &mut base,
                5,
            )
            .unwrap();
        }
        assert!(snapshot["identity"]["hostname"].is_string());
        assert!(snapshot["memory"]["value"].is_number(), "{snapshot}");
        assert!(snapshot["cpu"]["value"].is_number(), "{snapshot}");
        assert!(!snapshot["filesystems"].as_array().unwrap().is_empty());
    }
}

use super::{common::*, *};
pub fn command(group: &str, paths: &[String]) -> String {
    match group{
 "identity"=>"s system uname -srm; s architecture uname -m; s hostname cat /proc/sys/kernel/hostname; s os cat /etc/os-release; s cpuinfo cat /proc/cpuinfo; s model cat /sys/firmware/devicetree/base/model".into(),
 "dynamic"=>"s cpu cat /proc/stat; s memory cat /proc/meminfo; s network cat /proc/net/dev; s disk cat /proc/diskstats; s uptime cat /proc/uptime; s boot cat /proc/sys/kernel/random/boot_id".into(),
 "topology"=>"s load cat /proc/loadavg; s mounts cat /proc/self/mountinfo; s interfaces sh -c 'for p in /sys/class/net/*; do [ -d \"$p\" ] || continue; printf \"%s|\" \"${p##*/}\"; for f in ifindex address operstate; do tr -d \"\\n\" < \"$p/$f\"; printf \"|\"; done; printf \"\\n\"; done'; s addresses ip -o addr show".into(),
 "uptime"=>"s uptime cat /proc/uptime; s boot cat /proc/sys/kernel/random/boot_id".into(),
 "capacity"=>paths.iter().take(16).enumerate().map(|(i,p)|format!("s fs{i} df -Pk {}",quote(p))).collect::<Vec<_>>().join("; "),_=>unreachable!()}
}
pub fn meminfo(text: &str) -> std::collections::HashMap<String, u64> {
    text.lines()
        .filter_map(|s| {
            let (k, v) = s.split_once(':')?;
            Some((
                k.into(),
                number(v.split_whitespace().next()?)
                    .ok()?
                    .checked_mul(1024)?,
            ))
        })
        .collect()
}
pub fn mounts(text: &str) -> MountScan {
    let mut complete = !text.is_empty();
    let mut rows = Vec::new();
    for line in text.lines() {
        let parsed = (|| {
            let (b, a) = line.split_once(" - ")?;
            let f: Vec<_> = b.split_whitespace().collect();
            let a: Vec<_> = a.split_whitespace().collect();
            if f.len() < 6 || a.len() < 3 {
                return None;
            }
            number(f[0]).ok()?;
            let path = decode_escape(f[4])?;
            let root = decode_escape(f[3])?;
            let device = decode_escape(a[1])?;
            if !path.starts_with('/') || path.len() > 4096 {
                return None;
            }
            Some(Mount {
                identity: format!("{}:{}:{root}:{path}:{}:{device}", f[0], f[2], a[0]),
                path,
                device: device.clone(),
                capacity_identity: device,
                kind: Some(a[0].into()),
                read_only: Some(f[5].split(',').any(|o| o == "ro")),
                windows: false,
                eligible: eligible(a[0]),
                system: false,
            })
        })();
        if let Some(v) = parsed {
            if rows.len() < 512 {
                rows.push(v);
            } else {
                complete = false;
            }
        } else {
            complete = false;
        }
    }
    MountScan {
        rows,
        complete,
        reason: (!complete).then(|| "mountinfo 缺项、截断或超过预算".into()),
    }
}
fn interfaces(s: &Sections) -> Result<Vec<Device>, String> {
    let text = required(s, "interfaces")?;
    let addresses = required(s, "addresses").unwrap_or("");
    let mut rows = Vec::new();
    for line in text.lines() {
        let f: Vec<_> = line.split('|').collect();
        if f.len() < 4 {
            return Err("接口记录无效".into());
        }
        let ips = addresses
            .lines()
            .filter(|l| {
                l.split_whitespace()
                    .nth(1)
                    .is_some_and(|s| s.trim_end_matches(':') == f[0])
            })
            .filter_map(|l| l.split_whitespace().nth(3))
            .map(|s| limited(s, 128))
            .take(64)
            .collect();
        rows.push(Device {
            id: format!("{}:{}:{}", f[0], f[1], f[2]),
            name: limited(f[0], 128),
            state: Some(f[3].into()),
            addresses: ips,
            counter_instance: false,
        });
        if rows.len() > 256 {
            return Err("接口超过预算".into());
        }
    }
    Ok(rows)
}
pub fn decode(group: &str, s: &Sections, ctx: &DecodeContext) -> Sample {
    match group {
        "identity" => {
            let mut v = identity(s, false);
            if let Ok(v) = &mut v {
                let cpu = required(s, "cpuinfo").unwrap_or("");
                v.system = required(s, "os")
                    .ok()
                    .and_then(|s| s.lines().find_map(|s| s.strip_prefix("PRETTY_NAME=")))
                    .map(|s| s.trim_matches(['\"', '\'']).to_string())
                    .unwrap_or_else(|| v.system.clone());
                v.cpu_model = cpu
                    .lines()
                    .filter_map(|s| s.split_once(':'))
                    .find(|(k, _)| ["model name", "Hardware", "Processor"].contains(&k.trim()))
                    .map(|(_, s)| limited(s.trim(), 256))
                    .unwrap_or_else(|| v.cpu_model.trim_matches('\0').into());
                let n = cpu
                    .lines()
                    .filter(|s| {
                        s.split_once(':')
                            .is_some_and(|(k, _)| k.trim() == "processor")
                    })
                    .count();
                v.cores = (n > 0).then_some(n as u64);
            }
            Sample::Identity(v.into())
        }
        "dynamic" => {
            let cpu = required(s, "cpu").and_then(|s| {
                let l = s
                    .lines()
                    .find(|s| s.starts_with("cpu "))
                    .ok_or("缺少 CPU 总计")?;
                let n = l
                    .split_whitespace()
                    .skip(1)
                    .take(8)
                    .map(number)
                    .collect::<Result<Vec<_>, _>>()?;
                if n.len() < 4 {
                    return Err("CPU 计数缺项".into());
                }
                let total = n
                    .iter()
                    .try_fold(0u64, |a, b| a.checked_add(*b))
                    .ok_or("CPU 计数溢出")?;
                let idle = n[3]
                    .checked_add(n.get(4).copied().unwrap_or(0))
                    .ok_or("CPU idle 溢出")?;
                Ok(Cpu::Counter(total, idle))
            });
            let m = required(s, "memory").map(meminfo);
            let memory = m.as_ref().map_err(Clone::clone).and_then(|m| {
                let total = *m.get("MemTotal").ok_or("缺少内存总量")?;
                let available = m
                    .get("MemAvailable")
                    .copied()
                    .or_else(|| {
                        Some(
                            m.get("MemFree")?
                                .saturating_add(*m.get("Buffers")?)
                                .saturating_add(*m.get("Cached")?)
                                .saturating_add(m.get("SReclaimable").copied().unwrap_or(0))
                                .saturating_sub(m.get("Shmem").copied().unwrap_or(0)),
                        )
                    })
                    .ok_or("缺少可用内存")?;
                memory(
                    total,
                    available,
                    !m.contains_key("MemAvailable"),
                    "procfs",
                    String::new(),
                )
            });
            let network = required(s, "network").and_then(|s| {
                let mut rows = Vec::new();
                for line in s.lines().skip(2) {
                    let Some((name, data)) = line.split_once(':') else {
                        continue;
                    };
                    let name = name.trim();
                    let f: Vec<_> = data.split_whitespace().collect();
                    if f.len() < 9 {
                        return Err("网卡字节计数缺项".into());
                    }
                    let device = ctx
                        .interfaces
                        .iter()
                        .find(|v| v.name == name)
                        .cloned()
                        .unwrap_or_else(|| Device {
                            id: name.into(),
                            name: name.into(),
                            ..Device::default()
                        });
                    rows.push(Counter {
                        device,
                        first: number(f[0])?,
                        second: number(f[8])?,
                        clock: None,
                    });
                    if rows.len() > 256 {
                        return Err("网络计数超过预算".into());
                    }
                }
                Ok(rows)
            });
            let disks = required(s, "disk").and_then(|s| {
                let mut rows = Vec::new();
                for l in s.lines() {
                    let f: Vec<_> = l.split_whitespace().collect();
                    if f.len() < 14 {
                        return Err("磁盘计数缺项".into());
                    }
                    if f[2].starts_with("loop") || f[2].starts_with("ram") {
                        continue;
                    }
                    rows.push(Counter {
                        device: Device {
                            id: format!("{}:{}", f[0], f[1]),
                            name: limited(f[2], 128),
                            ..Device::default()
                        },
                        first: number(f[5])?.checked_mul(512).ok_or("磁盘计数溢出")?,
                        second: number(f[9])?.checked_mul(512).ok_or("磁盘计数溢出")?,
                        clock: None,
                    });
                    if rows.len() > 256 {
                        return Err("磁盘设备超过预算".into());
                    }
                }
                Ok(rows)
            });
            let up = required(s, "uptime")
                .ok()
                .and_then(|s| s.split_whitespace().next())
                .and_then(|s| finite(s).ok());
            let swap = m
                .ok()
                .map(|m| (m.get("SwapTotal").copied(), m.get("SwapFree").copied()));
            Sample::Dynamic(Dynamic {
                cpu: cpu.into(),
                memory: memory.into(),
                network: network.into(),
                disks: disks.into(),
                boot: required(s, "boot").unwrap_or("").trim().into(),
                remote_seconds: up,
                uptime: up.map(|seconds| Uptime {
                    seconds,
                    estimated: false,
                }),
                estimated: false,
                swap,
            })
        }
        "topology" => Sample::Topology(Topology {
            interfaces: interfaces(s).into(),
            mounts: required(s, "mounts").map(mounts).into(),
            load: required(s, "load")
                .and_then(|s| {
                    s.split_whitespace()
                        .take(3)
                        .map(finite)
                        .collect::<Result<Vec<_>, _>>()
                })
                .and_then(|v| {
                    v.try_into()
                        .map(Load::Values)
                        .map_err(|_| "负载缺项".into())
                })
                .into(),
        }),
        "capacity" => {
            let mut rows = Vec::new();
            let mut failures = Vec::new();
            for (i, path) in ctx.paths.iter().take(16).enumerate() {
                match required(s, &format!("fs{i}")).and_then(capacities) {
                    Ok(v) => {
                        if v.len() == 1 && v[0].path == *path {
                            rows.extend(v);
                        } else {
                            failures.push((path.clone(), "容量路径与目标不一致".into()));
                        }
                    }
                    Err(e) => failures.push((path.clone(), e)),
                }
            }
            Sample::Capacity(Read::Value(CapacitySample { rows, failures }))
        }
        "uptime" => Sample::Uptime(
            required(s, "uptime")
                .and_then(|s| finite(s.split_whitespace().next().unwrap_or("")))
                .map(|seconds| Uptime {
                    seconds,
                    estimated: false,
                })
                .into(),
        ),
        _ => unreachable!(),
    }
}

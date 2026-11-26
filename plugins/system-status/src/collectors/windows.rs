use super::{common::*, *};
fn source(v: &Value, key: &str) -> Result<(), String> {
    if let Some(e) = v["errors"][key].as_str() {
        return Err(limited(e, 256));
    }
    if v[key].is_null() {
        return Err(format!("缺少 {key} 数据源"));
    }
    Ok(())
}
fn num(v: &Value) -> Result<u64, String> {
    v.as_str()
        .ok_or("字节计数不是字符串".into())
        .and_then(number)
}
fn array<'a>(v: &'a Value, key: &str) -> Result<&'a Vec<Value>, String> {
    source(v, key)?;
    v[key]
        .as_array()
        .filter(|v| v.len() <= 512)
        .ok_or("设备列表无效或超过预算".into())
}
fn counters(v: &Value, key: &str, a: &str, b: &str) -> Result<Vec<Counter>, String> {
    let rows = array(v, key)?;
    if rows.len() > 256 {
        return Err("设备列表超过预算".into());
    }
    rows.iter()
        .map(|r| {
            Ok(Counter {
                device: Device {
                    id: limited(r["id"].as_str().ok_or("缺少设备 ID")?, 256),
                    name: limited(r["name"].as_str().unwrap_or("设备"), 256),
                    counter_instance: key == "network",
                    ..Device::default()
                },
                first: num(&r[a])?,
                second: num(&r[b])?,
                clock: Some((num(&r["time"])?, num(&r["frequency"])?)),
            })
        })
        .collect()
}
pub fn decode(group: &str, v: &Value, ctx: &DecodeContext) -> Sample {
    match group {
        "identity" => {
            let good = !v["os"].is_null() || !v["cpu"].is_null();
            let cpu = v["cpu"]
                .as_array()
                .and_then(|a| a.first())
                .unwrap_or(&Value::Null);
            let arch = match cpu["architecture"].as_u64() {
                Some(0) => Some("x86"),
                Some(1) => Some("MIPS"),
                Some(2) => Some("Alpha"),
                Some(3) => Some("PowerPC"),
                Some(5) => Some("ARM"),
                Some(6) => Some("IA64"),
                Some(9) => Some("x86_64"),
                Some(12) => Some("ARM64"),
                _ => None,
            };
            Sample::Identity(if good {
                Read::Value(Identity {
                    system: limited(v["os"]["system"].as_str().unwrap_or(""), 256),
                    hostname: limited(v["os"]["hostname"].as_str().unwrap_or(""), 256),
                    cpu_model: limited(
                        cpu["model"]
                            .as_str()
                            .or(v["device"]["model"].as_str())
                            .unwrap_or(""),
                        256,
                    ),
                    cores: v["cpu"]
                        .as_array()
                        .map(|a| a.iter().filter_map(|v| v["cores"].as_u64()).sum()),
                    architecture: arch.map(String::from),
                    total: num(&v["os"]["totalBytes"]).ok(),
                })
            } else {
                Read::Failed("此数据组没有可用的数据源".into())
            })
        }
        "dynamic" => Sample::Dynamic(Dynamic {
            cpu: source(v, "cpu")
                .and_then(|_| {
                    Ok(Cpu::Counter(
                        num(&v["cpu"]["time"])?,
                        num(&v["cpu"]["idle"])?,
                    ))
                })
                .into(),
            memory: source(v, "memory")
                .and_then(|_| {
                    memory(
                        ctx.total.ok_or("缺少物理内存总量")?,
                        num(&v["memory"]["availableBytes"])?,
                        false,
                        "Windows 可见内存",
                        String::new(),
                    )
                })
                .into(),
            network: counters(v, "network", "received", "sent").into(),
            disks: counters(v, "disks", "read", "write").into(),
            boot: "windows-perf".into(),
            remote_seconds: None,
            uptime: None,
            estimated: false,
            swap: None,
        }),
        "topology" => Sample::Topology(Topology {
            interfaces: array(v, "interfaces")
                .and_then(|rows| {
                    rows.iter()
                        .map(|r| {
                            Ok(Device {
                                id: limited(r["id"].as_str().ok_or("接口 ID 缺失")?, 256),
                                name: limited(r["name"].as_str().unwrap_or(""), 256),
                                addresses: r["addresses"]
                                    .as_array()
                                    .into_iter()
                                    .flatten()
                                    .filter_map(Value::as_str)
                                    .take(64)
                                    .map(|s| limited(s, 128))
                                    .collect(),
                                state: Some(
                                    if r["enabled"] == true { "up" } else { "down" }.into(),
                                ),
                                counter_instance: false,
                            })
                        })
                        .collect()
                })
                .into(),
            mounts: array(v, "mounts")
                .and_then(|rows| {
                    rows.iter()
                        .map(|r| {
                            let id = r["id"].as_str().ok_or("卷 ID 缺失")?;
                            let path = r["path"].as_str().ok_or("卷路径缺失")?;
                            Ok(Mount {
                                identity: format!("{id}\0{path}"),
                                path: limited(path, 4096),
                                device: id.into(),
                                capacity_identity: id.into(),
                                kind: r["kind"].as_str().map(String::from),
                                read_only: None,
                                windows: true,
                                eligible: true,
                                system: false,
                            })
                        })
                        .collect::<Result<Vec<_>, String>>()
                })
                .map(|rows| MountScan {
                    rows,
                    complete: false,
                    reason: Some("本地固定卷的主挂载路径，不含所有附加挂载".into()),
                })
                .into(),
            load: Read::Unsupported,
        }),
        "capacity" => Sample::Capacity(
            array(v, "filesystems")
                .and_then(|rows| {
                    rows.iter()
                        .map(|r| {
                            Ok(Capacity {
                                path: r["path"].as_str().ok_or("容量路径缺失")?.into(),
                                device: r["id"].as_str().ok_or("容量来源缺失")?.into(),
                                total: num(&r["total"])?,
                                used: None,
                                available: num(&r["available"])?,
                            })
                        })
                        .collect::<Result<Vec<_>, String>>()
                })
                .map(|rows| CapacitySample {
                    rows,
                    failures: Vec::new(),
                })
                .into(),
        ),
        "uptime" => Sample::Uptime(
            source(v, "uptime")
                .and_then(|_| num(&v["uptime"]["seconds"]))
                .map(|n| Uptime {
                    seconds: n as f64,
                    estimated: false,
                })
                .into(),
        ),
        _ => unreachable!(),
    }
}

pub fn command(group: &str, nonce: &str) -> String {
    // OpenSSH may invoke cmd.exe, whose command line is limited to 8191 characters.
    let mut script = String::new();
    let mut in_switch = false;
    let mut selected = false;
    for line in include_str!("../../scripts/windows.ps1").lines() {
        if line.starts_with("switch (") {
            in_switch = true;
            continue;
        }
        if in_switch {
            if line == " }" {
                selected = false;
                continue;
            }
            if line == "}" {
                in_switch = false;
                continue;
            }
            if line.starts_with(" '") {
                selected = line.trim() == format!("'{group}' {{");
                continue;
            }
            if !selected {
                continue;
            }
        }
        script.push_str(line);
        script.push('\n');
    }
    let script = script.replace("__NONCE__", nonce);
    let utf16: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    format!(
        "powershell.exe -NoProfile -NonInteractive -EncodedCommand {}",
        sodeck_plugin_client::encoding::base64(&utf16)
    )
}

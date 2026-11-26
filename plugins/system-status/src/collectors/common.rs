use super::*;
pub fn required<'a>(s: &'a Sections, key: &str) -> Result<&'a str, String> {
    s.get(key)
        .ok_or_else(|| format!("缺少 {key} 数据源"))?
        .as_ref()
        .map(String::as_str)
        .map_err(Clone::clone)
}
pub fn number(s: &str) -> Result<u64, String> {
    s.trim().parse().map_err(|_| "计数无效".into())
}
pub fn finite(s: &str) -> Result<f64, String> {
    s.trim()
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite() && *v >= 0.0)
        .ok_or("数值无效".into())
}
pub fn limited(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}
pub fn architecture(s: &str) -> Option<String> {
    let s = s.trim();
    if s.is_empty()
        || s.eq_ignore_ascii_case("unknown")
        || s.len() > 64
        || !s
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_- .".contains(&c) && c != b' ')
    {
        return None;
    }
    Some(
        match s.to_ascii_lowercase().as_str() {
            "amd64" | "x64" | "x86_64" => "x86_64",
            "aarch64" | "arm64" => "ARM64",
            "i386" | "i486" | "i586" | "i686" | "x86" => "x86",
            "armv7" | "armv7l" => "ARMv7",
            "armv6" | "armv6l" => "ARMv6",
            "arm" => "ARM",
            _ => s,
        }
        .into(),
    )
}
pub fn identity(s: &Sections, arm: bool) -> Result<Identity, String> {
    let get = |key| {
        s.get(key)
            .and_then(|v| v.as_ref().ok())
            .map(String::as_str)
            .unwrap_or("")
            .trim()
    };
    if get("system").is_empty() && get("hostname").is_empty() {
        return Err("设备信息没有可用来源".into());
    }
    Ok(Identity {
        system: limited(
            if get("os").is_empty() {
                get("system")
            } else {
                get("os")
            },
            256,
        ),
        hostname: limited(get("hostname"), 256),
        cpu_model: limited(get("model"), 256),
        cores: number(get("cores")).ok(),
        architecture: if arm && get("arm64") == "1" {
            Some("ARM64".into())
        } else {
            architecture(get("architecture"))
        },
        total: number(get("total")).ok(),
    })
}
pub fn memory(
    total: u64,
    available: u64,
    estimated: bool,
    source: &str,
    details: String,
) -> Result<Memory, String> {
    if total == 0 || available > total {
        return Err("内存容量数据越界".into());
    }
    Ok(Memory {
        total,
        available,
        estimated,
        source: source.into(),
        details,
    })
}
pub fn uptime(s: &Sections, estimated: bool) -> Result<Uptime, String> {
    let boot = required(s, "boottime")?;
    let boot = boot
        .split("sec =")
        .nth(1)
        .and_then(|s| s.split(',').next())
        .or_else(|| boot.split_whitespace().next())
        .ok_or("启动时间无效")?;
    let seconds = number(required(s, "now")?)?
        .checked_sub(number(boot)?)
        .ok_or("启动时间晚于当前时间")?;
    Ok(Uptime {
        seconds: seconds as f64,
        estimated,
    })
}
pub fn capacities(s: &str) -> Result<Vec<Capacity>, String> {
    let mut rows = Vec::new();
    if !s
        .lines()
        .next()
        .is_some_and(|s| s.starts_with("Filesystem"))
    {
        return Err("容量表头无效".into());
    }
    for line in s.lines().skip(1).filter(|s| !s.trim().is_empty()) {
        let f: Vec<_> = line.split_whitespace().collect();
        if f.len() < 6 {
            return Err("容量行不完整".into());
        }
        let bytes = |i: usize| -> Result<u64, String> {
            number(f[i])?.checked_mul(1024).ok_or("容量溢出".into())
        };
        rows.push(Capacity {
            device: limited(f[0], 256),
            path: limited(&f[5..].join(" "), 4096),
            total: bytes(1)?,
            used: Some(bytes(2)?),
            available: bytes(3)?,
        });
        if rows.len() > 512 {
            return Err("容量条目超过预算".into());
        }
    }
    Ok(rows)
}
#[derive(Debug)]
pub struct Link {
    pub name: String,
    pub index: u64,
    pub first: u64,
    pub second: u64,
}
pub fn links(text: &str) -> Result<Vec<Link>, String> {
    let mut lines = text.lines();
    let head: Vec<_> = lines
        .next()
        .ok_or("缺少网络表头")?
        .split_whitespace()
        .collect();
    let address = head
        .iter()
        .position(|s| *s == "Address")
        .ok_or("缺少 Address 列")?;
    let stats = &head[address + 1..];
    let a = stats
        .iter()
        .position(|s| *s == "Ibytes")
        .ok_or("缺少 Ibytes 列")?;
    let b = stats
        .iter()
        .position(|s| *s == "Obytes")
        .ok_or("缺少 Obytes 列")?;
    let mut rows = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for line in lines {
        let f: Vec<_> = line.split_whitespace().collect();
        let Some(link) = f.iter().position(|s| s.starts_with("<Link#")) else {
            continue;
        };
        if link != 2 || f.len() < 3 + stats.len() || f.len() > 4 + stats.len() {
            return Err("链路计数列数无效".into());
        }
        let start = f.len() - stats.len();
        if start == 4 && !f[3].contains(':') {
            return Err("链路硬件地址无效".into());
        }
        let name = f[0].trim_end_matches('*');
        if !seen.insert(name.to_string()) {
            return Err("链路计数重复".into());
        }
        let index = f[link]
            .strip_prefix("<Link#")
            .and_then(|s| s.strip_suffix('>'))
            .ok_or("链路编号无效")?;
        rows.push(Link {
            name: limited(name, 128),
            index: number(index)?,
            first: number(f[start + a])?,
            second: number(f[start + b])?,
        });
        if rows.len() > 256 {
            return Err("网络设备超过预算".into());
        }
    }
    Ok(rows)
}
pub fn ifconfig(text: &str, links: &[Link], mac: bool) -> Result<Vec<Device>, String> {
    let mut rows: Vec<Device> = Vec::new();
    let mut flags_up = false;
    for line in text.lines() {
        if !line.starts_with(char::is_whitespace) {
            let Some((name, rest)) = line.split_once(':') else {
                if line.trim().is_empty() {
                    continue;
                }
                return Err("接口记录无效".into());
            };
            if rows.len() >= 256 {
                return Err("接口超过预算".into());
            }
            let name = name.trim_end_matches('*');
            let link = links.iter().find(|l| l.name == name);
            if mac && link.is_none() {
                return Err(format!("接口 {name} 缺少链路身份"));
            }
            let flags = rest
                .split('<')
                .nth(1)
                .and_then(|s| s.split('>').next())
                .unwrap_or("");
            flags_up = flags.split(',').any(|s| s == "UP");
            let flags_running = flags.split(',').any(|s| s == "RUNNING");
            rows.push(Device {
                id: if mac {
                    format!("if:{}:{name}", link.unwrap().index)
                } else {
                    name.into()
                },
                name: limited(name, 128),
                state: Some(
                    if flags_up && flags_running {
                        "up"
                    } else {
                        "down"
                    }
                    .into(),
                ),
                ..Device::default()
            });
            continue;
        }
        let Some(row) = rows.last_mut() else {
            continue;
        };
        let f: Vec<_> = line.split_whitespace().collect();
        match f.as_slice() {
            ["status:", status, ..] => {
                row.state = Some(
                    if *status == "active" && flags_up {
                        "up"
                    } else {
                        "down"
                    }
                    .into(),
                )
            }
            ["inet", ip, ..] => {
                let addr: std::net::Ipv4Addr = ip.parse().map_err(|_| "IPv4 无效")?;
                let prefix = f
                    .iter()
                    .position(|s| *s == "netmask")
                    .and_then(|i| f.get(i + 1))
                    .and_then(|s| u32::from_str_radix(s.trim_start_matches("0x"), 16).ok())
                    .map(|v| v.count_ones());
                row.addresses
                    .push(prefix.map_or_else(|| addr.to_string(), |n| format!("{addr}/{n}")));
            }
            ["inet6", ip, ..] => {
                let raw = ip.split('%').next().unwrap_or(ip);
                raw.parse::<std::net::Ipv6Addr>().map_err(|_| "IPv6 无效")?;
                let prefix = f
                    .iter()
                    .position(|s| *s == "prefixlen")
                    .and_then(|i| f.get(i + 1))
                    .and_then(|s| s.parse::<u8>().ok())
                    .filter(|n| *n <= 128);
                row.addresses
                    .push(prefix.map_or_else(|| ip.to_string(), |n| format!("{ip}/{n}")));
            }
            _ => {}
        }
        if row.addresses.len() > 64 {
            return Err("接口地址超过展示预算".into());
        }
    }
    for r in &mut rows {
        r.addresses.sort();
        r.addresses.dedup();
    }
    Ok(rows)
}
pub fn decode_escape(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' {
            if i + 3 >= b.len() {
                return None;
            }
            let n = u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 4]).ok()?, 8).ok()?;
            out.push(n);
            i += 4;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}
pub fn eligible(kind: &str) -> bool {
    matches!(
        kind,
        "ext2"
            | "ext3"
            | "ext4"
            | "xfs"
            | "btrfs"
            | "f2fs"
            | "jffs2"
            | "ubifs"
            | "overlay"
            | "squashfs"
            | "tmpfs"
            | "vfat"
            | "exfat"
            | "ntfs"
            | "ntfs3"
            | "zfs"
            | "ufs"
            | "ramfs"
    )
}

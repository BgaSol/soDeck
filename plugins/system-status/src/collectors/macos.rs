use super::{common::*, *};
pub fn command(group: &str) -> String {
    match group{
 "identity"=>"s system /usr/bin/uname -srm; s architecture /usr/bin/uname -m; s arm64 /usr/sbin/sysctl -n hw.optional.arm64; s hostname /bin/hostname; s os /usr/bin/sw_vers; s model /usr/sbin/sysctl -n machdep.cpu.brand_string; s cores /usr/sbin/sysctl -n hw.logicalcpu; s total /usr/sbin/sysctl -n hw.memsize".into(),
 "dynamic"=>format!("s cpu /usr/bin/top -l 2 -s 1 -n 0; s memory /usr/bin/vm_stat; s total /usr/sbin/sysctl -n hw.memsize; s network /usr/sbin/netstat -ibn; s boot /usr/sbin/sysctl -n kern.boottime; s disk /bin/sh -c {}",quote("r=$(/usr/sbin/ioreg -r -c IOBlockStorageDriver -d 2 -a) || exit $?; printf '%s' \"$r\" | /usr/bin/plutil -convert json -o - -")),
 "topology"=>"s interfaces /sbin/ifconfig -a; s network /usr/sbin/netstat -ibn; s mounts /sbin/mount; s load /usr/sbin/sysctl -n vm.loadavg".into(),
 "capacity"=>"s filesystems /bin/df -Pkl".into(),
 "uptime"=>"s boottime /usr/sbin/sysctl -n kern.boottime; s now /bin/date +%s".into(),_=>unreachable!()}
}
pub fn memory_vm(text: &str, total: u64) -> Result<Memory, String> {
    let page = text
        .lines()
        .next()
        .and_then(|s| s.split("page size of ").nth(1))
        .and_then(|s| s.split_whitespace().next())
        .ok_or("缺少页大小")?;
    let page = number(page)?;
    if !matches!(page, 4096 | 16384) {
        return Err("不支持的内存页大小".into());
    }
    let mut pages = std::collections::BTreeMap::new();
    for line in text.lines().skip(1) {
        if let Some((name, n)) = line.split_once(':') {
            pages.insert(name.trim(), number(n.trim().trim_end_matches('.'))?);
        }
    }
    let get = |k| pages.get(k).copied().ok_or_else(|| format!("缺少 {k}"));
    let anonymous = get("Anonymous pages")?;
    let purgeable = get("Pages purgeable")?;
    let wired = get("Pages wired down")?;
    let compressor = get("Pages occupied by compressor")?;
    let used = anonymous
        .checked_sub(purgeable)
        .and_then(|v| v.checked_add(wired))
        .and_then(|v| v.checked_add(compressor))
        .and_then(|v| v.checked_mul(page))
        .ok_or("内存计数越界")?;
    let available = total.checked_sub(used).ok_or("已用内存超过物理容量")?;
    memory(total,available,true,"macOS 物理占用估算",format!("已用 =（匿名页 - 可清理页 + wired 页 + 压缩器实际占用页）× 页大小。\n页大小 {page} B；匿名 {anonymous}、可清理 {purgeable}、wired {wired}、压缩器 {compressor} 页。\n可用为物理总量减已用的可回收余量估算，包含文件缓存等余量；不等于立即空闲内存。"))
}
fn cpu(text: &str) -> Result<Cpu, String> {
    let idle = text
        .lines()
        .filter(|l| l.starts_with("CPU usage:"))
        .last()
        .and_then(|l| l.split(',').find(|s| s.contains(" idle")))
        .and_then(|s| s.split('%').next())
        .ok_or("CPU idle 字段无效")?;
    let idle = finite(idle)?;
    if idle > 100.0 {
        return Err("CPU 百分比越界".into());
    }
    Ok(Cpu::Direct(100.0 - idle))
}
fn disks(text: &str, boot: &str) -> Result<Vec<Counter>, String> {
    if boot.is_empty() {
        return Err("缺少启动身份".into());
    }
    let json: Value = serde_json::from_str(text).map_err(|_| "磁盘统计 JSON 无效")?;
    let rows = json.as_array().ok_or("磁盘统计必须为数组")?;
    if rows.len() > 256 {
        return Err("磁盘设备超过预算".into());
    }
    rows.iter()
        .map(|v| {
            let id = v["IORegistryEntryID"].as_u64().ok_or("缺少磁盘身份")?;
            let stats = &v["Statistics"];
            let first = stats["Bytes (Read)"].as_u64().ok_or("缺少累计读取字节")?;
            let second = stats["Bytes (Write)"].as_u64().ok_or("缺少累计写入字节")?;
            let name = v["IORegistryEntryChildren"]
                .as_array()
                .and_then(|r| r.first())
                .and_then(|v| v["IORegistryEntryName"].as_str())
                .unwrap_or("存储设备");
            Ok(Counter {
                device: Device {
                    id: format!("io:{boot}:{id}"),
                    name: limited(name, 128),
                    ..Device::default()
                },
                first,
                second,
                clock: None,
            })
        })
        .collect()
}
pub fn mounts(text: &str) -> MountScan {
    let mut rows = Vec::new();
    let mut complete = !text.trim().is_empty();
    for line in text.lines().filter(|s| !s.is_empty()) {
        let parsed = (|| {
            let (left, opts) = line.rsplit_once(" (")?;
            let opts = opts.strip_suffix(')')?;
            let (device, path) = left.split_once(" on ")?;
            let opts: Vec<_> = opts.split(',').map(str::trim).collect();
            let kind = *opts.first()?;
            if !path.starts_with('/') || path.len() > 4096 {
                return None;
            }
            let local = opts.contains(&"local");
            let system = local
                && kind == "apfs"
                && opts.contains(&"nobrowse")
                && matches!(
                    path,
                    "/System/Volumes/VM"
                        | "/System/Volumes/Preboot"
                        | "/System/Volumes/Update"
                        | "/System/Volumes/xarts"
                        | "/System/Volumes/iSCPreboot"
                        | "/System/Volumes/Hardware"
                        | "/System/Volumes/Recovery"
                );
            Some(Mount {
                identity: format!("{device}\0{path}"),
                path: path.into(),
                device: limited(device, 256),
                capacity_identity: limited(device, 256),
                kind: Some(kind.into()),
                read_only: Some(opts.contains(&"read-only")),
                windows: false,
                eligible: local && device.starts_with("/dev/disk"),
                system,
            })
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
    MountScan {
        rows,
        complete,
        reason: (!complete).then(|| "挂载输出缺项、格式无效或超过预算".into()),
    }
}
pub fn decode(group: &str, s: &Sections) -> Sample {
    match group {
        "identity" => Sample::Identity(identity(s, true).into()),
        "dynamic" => {
            let boot = required(s, "boot")
                .ok()
                .and_then(|s| s.split("sec =").nth(1))
                .and_then(|s| s.split(',').next())
                .and_then(|s| number(s).ok())
                .map(|n| n.to_string())
                .unwrap_or_default();
            Sample::Dynamic(Dynamic {
                cpu: required(s, "cpu").and_then(cpu).into(),
                memory: required(s, "total")
                    .and_then(number)
                    .and_then(|total| required(s, "memory").and_then(|v| memory_vm(v, total)))
                    .into(),
                network: required(s, "network")
                    .and_then(links)
                    .map(|rows| {
                        rows.into_iter()
                            .map(|l| Counter {
                                device: Device {
                                    id: format!("if:{}:{}", l.index, l.name),
                                    name: l.name,
                                    ..Device::default()
                                },
                                first: l.first,
                                second: l.second,
                                clock: None,
                            })
                            .collect()
                    })
                    .into(),
                disks: required(s, "disk").and_then(|v| disks(v, &boot)).into(),
                boot,
                remote_seconds: None,
                uptime: None,
                estimated: true,
                swap: None,
            })
        }
        "topology" => Sample::Topology(Topology {
            interfaces: required(s, "network")
                .and_then(links)
                .and_then(|links| required(s, "interfaces").and_then(|v| ifconfig(v, &links, true)))
                .into(),
            mounts: required(s, "mounts").map(mounts).into(),
            load: required(s, "load")
                .and_then(|s| {
                    let v = s
                        .trim()
                        .trim_matches(['{', '}'])
                        .split_whitespace()
                        .map(finite)
                        .collect::<Result<Vec<_>, _>>()?;
                    let v: [f64; 3] = v.try_into().map_err(|_| "需要三项负载".to_string())?;
                    Ok(Load::Values(v))
                })
                .into(),
        }),
        "capacity" => Sample::Capacity(
            required(s, "filesystems")
                .and_then(capacities)
                .map(|rows| CapacitySample {
                    rows,
                    failures: Vec::new(),
                })
                .into(),
        ),
        "uptime" => Sample::Uptime(uptime(s, true).into()),
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const VM:&str="Mach Virtual Memory Statistics: (page size of 16384 bytes)\nPages free: 18775.\nAnonymous pages: 493214.\nPages purgeable: 44184.\nPages wired down: 121986.\nPages occupied by compressor: 44630.\nPages stored in compressor: 132404.\n";
    #[test]
    fn physical_memory_uses_actual_page_size_and_compressor_physical_pages() {
        let m = memory_vm(VM, 17179869184).unwrap();
        assert_eq!(m.available, 7093125120);
        let m4 = memory_vm(&VM.replace("16384 bytes", "4096 bytes"), 17179869184).unwrap();
        assert_eq!(17179869184 - m4.available, (17179869184 - m.available) / 4);
        assert!(memory_vm(VM, 1).is_err());
        assert!(memory_vm(&VM.replace("493214.", "1."), 17179869184).is_err());
        assert!(memory_vm(&VM.replace("44184.", "18446744073709551615."), 17179869184).is_err());
    }
    #[test]
    fn link_rows_do_not_shift_bytes_when_address_is_blank() {
        let v=links("Name Mtu Network Address Ipkts Ierrs Ibytes Opkts Oerrs Obytes Coll\nlo0 16384 <Link#1> 26067 0 7035943 26067 0 7035943 0\nen0 1500 <Link#9> 02:00:00:00:00:01 20 0 12000 10 0 8000 0\nutun223* 1400 <Link#25> 15836 0 4788974 7414 0 2143775 0\n").unwrap();
        assert_eq!((v[0].first, v[0].second), (7035943, 7035943));
        assert_eq!(
            (v[2].name.as_str(), v[2].first, v[2].second),
            ("utun223", 4788974, 2143775)
        );
    }
    #[test]
    fn apfs_metadata_preserves_data_and_folds_only_known_system_mounts() {
        let scan=mounts("/dev/disk3s3s1 on / (apfs, sealed, local, read-only, journaled)\n/dev/disk3s1 on /System/Volumes/Data (apfs, local, journaled, nobrowse)\n/dev/disk3s6 on /System/Volumes/VM (apfs, local, journaled, nobrowse)\n/dev/disk4 on /custom (apfs, local, read-only, nobrowse)\n");
        assert!(scan.complete);
        assert_eq!(scan.rows[0].read_only, Some(true));
        assert!(!scan.rows[1].system);
        assert!(scan.rows[2].system);
        assert!(!scan.rows[3].system);
    }
}

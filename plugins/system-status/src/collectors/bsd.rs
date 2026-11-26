use super::{common::*, *};
pub fn command(family: Family, group: &str) -> String {
    match group{
 "identity"=>match family{Family::FreeBsd=>"s system uname -srm; s architecture uname -m; s hostname hostname; s model sysctl -n hw.model; s cores sysctl -n hw.ncpu; s total sysctl -n hw.physmem",Family::OpenBsd=>"s system uname -srm; s architecture uname -m; s hostname hostname; s model sysctl -n hw.model; s cores sysctl -n hw.ncpu; s total sysctl -n hw.physmem",Family::NetBsd=>"s system uname -srm; s architecture uname -m; s hostname hostname; s model sysctl -n machdep.cpu_brand; s cores sysctl -n hw.ncpu; s total sysctl -n hw.physmem64",_=>"s system uname -srm; s architecture uname -m; s hostname hostname"}.into(),
 "dynamic"=>"s cpu sysctl -n kern.cp_time; s network netstat -ibn".into(),
 "topology"=>"s load uptime; s interfaces ifconfig -a; s filesystems df -Pkl".into(),
 "capacity"=>"s filesystems df -Pkl".into(),"uptime"=>"s boottime sysctl -n kern.boottime; s now date +%s".into(),_=>unreachable!()}
}
pub fn decode(group: &str, s: &Sections) -> Sample {
    match group {
        "identity" => Sample::Identity(identity(s, false).into()),
        "dynamic" => Sample::Dynamic(Dynamic {
            cpu: required(s, "cpu")
                .and_then(|s| {
                    let n = s
                        .split_whitespace()
                        .map(number)
                        .collect::<Result<Vec<_>, _>>()?;
                    if n.len() != 5 {
                        return Err("CPU 计数缺项".into());
                    }
                    Ok(Cpu::Counter(
                        n.iter()
                            .try_fold(0u64, |a, b| a.checked_add(*b))
                            .ok_or("CPU 计数溢出")?,
                        n[4],
                    ))
                })
                .into(),
            memory: Read::Unsupported,
            network: required(s, "network")
                .and_then(links)
                .map(|r| {
                    r.into_iter()
                        .map(|l| Counter {
                            device: Device {
                                id: l.name.clone(),
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
            disks: Read::Unsupported,
            boot: String::new(),
            remote_seconds: None,
            uptime: None,
            estimated: true,
            swap: None,
        }),
        "topology" => Sample::Topology(Topology {
            interfaces: required(s, "interfaces")
                .and_then(|s| ifconfig(s, &[], false))
                .into(),
            mounts: required(s, "filesystems")
                .and_then(capacities)
                .map(|rows| MountScan {
                    rows: rows
                        .into_iter()
                        .map(|r| Mount {
                            identity: format!("{}\0{}", r.device, r.path),
                            path: r.path,
                            device: r.device.clone(),
                            capacity_identity: r.device,
                            kind: None,
                            read_only: None,
                            windows: false,
                            eligible: true,
                            system: false,
                        })
                        .collect(),
                    complete: false,
                    reason: Some("df 仅提供部分本地挂载拓扑".into()),
                })
                .into(),
            load: required(s, "load")
                .map(|v| Load::Text(limited(v.trim(), 512)))
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

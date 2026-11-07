//! Fixed read-only scripts. Remote text is parsed as data, never sourced or evaluated.
use serde_json::{json, Value};
use std::collections::BTreeMap;
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Family {
    Linux,
    Windows,
    Darwin,
    FreeBsd,
    OpenBsd,
    NetBsd,
    Unix,
}
impl Family {
    pub fn name(self) -> &'static str {
        match self {
            Self::Linux => "Linux",
            Self::Windows => "Windows",
            Self::Darwin => "macOS",
            Self::FreeBsd => "FreeBSD",
            Self::OpenBsd => "OpenBSD",
            Self::NetBsd => "NetBSD",
            Self::Unix => "Unix",
        }
    }
}
pub fn detect(text: &str) -> Option<Family> {
    text.lines().find_map(|line| match line.trim() {
        "Linux" => Some(Family::Linux),
        "Darwin" => Some(Family::Darwin),
        "FreeBSD" => Some(Family::FreeBsd),
        "OpenBSD" => Some(Family::OpenBsd),
        "NetBSD" => Some(Family::NetBsd),
        s if s.starts_with("CYGWIN") || s.starts_with("MSYS") || s.starts_with("MINGW") => {
            Some(Family::Windows)
        }
        "SunOS" | "AIX" | "HP-UX" | "DragonFly" => Some(Family::Unix),
        _ => None,
    })
}
pub fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}
pub fn command(family: Family, group: &str, nonce: &str, paths: &[String]) -> String {
    if family == Family::Windows {
        return windows(group, nonce);
    }
    let body = match (family, group) {
        (Family::Linux,"identity")=>"s system uname -srm; s hostname cat /proc/sys/kernel/hostname; s os cat /etc/os-release; s cpuinfo cat /proc/cpuinfo; s model cat /sys/firmware/devicetree/base/model".into(),
        (Family::Linux,"dynamic")=>"s cpu cat /proc/stat; s memory cat /proc/meminfo; s network cat /proc/net/dev; s disk cat /proc/diskstats; s uptime cat /proc/uptime; s boot cat /proc/sys/kernel/random/boot_id".into(),
        (Family::Linux,"topology")=>"s load cat /proc/loadavg; s mounts cat /proc/self/mountinfo; s interfaces sh -c 'for p in /sys/class/net/*; do [ -d \"$p\" ] || continue; printf \"%s|\" \"${p##*/}\"; for f in ifindex address operstate; do tr -d \"\\n\" < \"$p/$f\"; printf \"|\"; done; printf \"\\n\"; done'; s addresses ip -o addr show".into(),
        (Family::Linux,"uptime")=>"s uptime cat /proc/uptime; s boot cat /proc/sys/kernel/random/boot_id".into(),
        (Family::Linux,"capacity")=>paths.iter().take(16).enumerate().map(|(i,p)|format!("s fs{i} df -Pk {}",quote(p))).collect::<Vec<_>>().join("; "),
        (Family::Darwin,"identity")=>"s system uname -srm; s hostname hostname; s os sw_vers; s model sysctl -n machdep.cpu.brand_string; s cores sysctl -n hw.logicalcpu; s total sysctl -n hw.memsize".into(),
        (Family::Darwin,"dynamic")=>"s cpu top -l 2 -s 1 -n 0; s memory vm_stat; s network netstat -ibn".into(),
        (Family::FreeBsd,"identity")=>"s system uname -srm; s hostname hostname; s model sysctl -n hw.model; s cores sysctl -n hw.ncpu; s total sysctl -n hw.physmem".into(),
        (Family::FreeBsd,"dynamic")=>"s cpu sysctl -n kern.cp_time; s memory sysctl vm.stats.vm; s network netstat -ibn".into(),
        (Family::OpenBsd,"identity")=>"s system uname -srm; s hostname hostname; s model sysctl -n hw.model; s cores sysctl -n hw.ncpu; s total sysctl -n hw.physmem".into(),
        (Family::NetBsd,"identity")=>"s system uname -srm; s hostname hostname; s model sysctl -n machdep.cpu_brand; s cores sysctl -n hw.ncpu; s total sysctl -n hw.physmem64".into(),
        (Family::OpenBsd|Family::NetBsd,"dynamic")=>"s cpu sysctl -n kern.cp_time; s network netstat -ibn".into(),
        (_,"identity")=>"s system uname -srm; s hostname hostname".into(),
        (_,"topology")=>"s load uptime; s interfaces ifconfig -a".into(),
        (_,"uptime")=>"s boottime sysctl -n kern.boottime; s now date +%s; s uptime uptime".into(),
        (Family::Darwin|Family::FreeBsd|Family::OpenBsd|Family::NetBsd,"capacity")=>"s filesystems df -Pkl".into(),
        _=>"s unsupported false".into(),
    };
    let script=format!("LC_ALL=C; export LC_ALL\nn={}\ns() {{ k=$1; shift; printf '\\nSD1 %s BEGIN %s\\n' \"$n\" \"$k\"; \"$@\"; r=$?; printf '\\nSD1 %s END %s %s\\n' \"$n\" \"$k\" \"$r\"; }}\n{}\nprintf 'SD1 %s DONE\\n' \"$n\"",quote(nonce),body);
    format!("sh -c {}", quote(&script))
}
fn windows(group: &str, nonce: &str) -> String {
    let script = include_str!("../scripts/windows.ps1")
        .replace("__GROUP__", group)
        .replace("__NONCE__", nonce);
    let utf16: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    format!(
        "powershell.exe -NoProfile -NonInteractive -EncodedCommand {}",
        sodeck_plugin_client::encoding::base64(&utf16)
    )
}
pub fn windows_probe() -> &'static str {
    "powershell.exe -NoProfile -NonInteractive -Command \"[Console]::WriteLine('SODECK_WINDOWS_1')\""
}
pub fn sections(text: &str, nonce: &str) -> Result<BTreeMap<String, String>, String> {
    if text.len() > 1024 * 1024 || nonce.len() > 128 {
        return Err("采集输出过大".into());
    }
    let prefix = format!("SD1 {nonce} ");
    let mut out = BTreeMap::new();
    let mut active = None::<(String, String)>;
    let mut done = false;
    let mut seen = std::collections::HashSet::new();
    for line in text.lines() {
        if line.len() > 32768 {
            return Err("采集输出单行过长".into());
        }
        if let Some(marker) = line.strip_prefix(&prefix) {
            if marker == "DONE" {
                if active.is_some() {
                    return Err("采集段未结束".into());
                }
                done = true;
                continue;
            }
            let parts: Vec<_> = marker.split_whitespace().collect();
            match parts.as_slice() {
                ["BEGIN", name]
                    if active.is_none()
                        && !done
                        && seen.insert(name.to_string())
                        && seen.len() <= 64 =>
                {
                    active = Some((name.to_string(), String::new()))
                }
                ["END", name, code] => {
                    let (key, data) = active.take().ok_or("采集段缺少起点")?;
                    if key != *name {
                        return Err("采集段不匹配".into());
                    }
                    let code = code.parse::<u32>().map_err(|_| "采集退出码无效")?;
                    if code == 0 {
                        out.insert(key, data);
                    }
                }
                _ => return Err("采集协议无效".into()),
            }
        } else if let Some((_, data)) = &mut active {
            data.push_str(line);
            data.push('\n');
        } else if line.starts_with("SD1 ") {
            return Err("采集票据不匹配".into());
        }
    }
    if !done || active.is_some() {
        return Err("采集输出不完整".into());
    }
    Ok(out)
}
pub fn windows_json(text: &str, nonce: &str) -> Result<Value, String> {
    let start = text.find('{').ok_or("Windows 没有返回数据")?;
    let v: Value =
        serde_json::from_str(text[start..].trim()).map_err(|_| "Windows 采集数据无效")?;
    if v["schemaVersion"] != 1 || v["nonce"] != nonce {
        return Err("Windows 采集票据不匹配".into());
    }
    Ok(v)
}
pub fn mount_paths(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let (before, after) = line.split_once(" - ")?;
            let fields: Vec<_> = before.split_whitespace().collect();
            let kind = after.split_whitespace().next()?;
            if ![
                "ext2", "ext3", "ext4", "xfs", "btrfs", "f2fs", "jffs2", "ubifs", "overlay",
                "squashfs", "tmpfs", "vfat", "exfat", "ntfs", "ntfs3", "zfs", "ufs", "ramfs",
            ]
            .contains(&kind)
            {
                return None;
            }
            let p = fields.get(4)?;
            Some(
                p.replace("\\040", " ")
                    .replace("\\011", "\t")
                    .replace("\\012", "\n")
                    .replace("\\134", "\\"),
            )
        })
        .take(512)
        .collect()
}
pub fn initial_capabilities(family: Family) -> Value {
    json!({"family":family.name(),"diskIo":family==Family::Linux || family==Family::Windows,"memoryPercent":matches!(family,Family::Linux|Family::Windows|Family::FreeBsd),"load":family!=Family::Windows})
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn protocol_requires_complete_unique_matching_sections() {
        assert_eq!(
            sections(
                "banner\nSD1 n BEGIN cpu\n1\nSD1 n END cpu 0\nSD1 n DONE\n",
                "n"
            )
            .unwrap()["cpu"],
            "1\n"
        );
        assert!(sections("SD1 n BEGIN cpu\n1", "n").is_err());
        assert!(sections("SD1 x DONE", "n").is_err());
        assert!(sections(
            "SD1 n BEGIN a\nSD1 n END a 0\nSD1 n BEGIN a\nSD1 n END a 0\nSD1 n DONE",
            "n"
        )
        .is_err());
        assert!(sections("SD1 n BEGIN a\nSD1 n END a 1\nSD1 n DONE", "n")
            .unwrap()
            .is_empty());
    }
    #[test]
    fn mount_metadata_excludes_remote_and_decodes_paths() {
        let m =
            "1 0 8:1 / /data\\040disk rw - ext4 /dev/a rw\n2 0 0:1 / /remote rw - nfs host:/x rw";
        assert_eq!(mount_paths(m), ["/data disk"]);
        assert_eq!(quote("a'b"), "'a'\\''b'");
    }
}

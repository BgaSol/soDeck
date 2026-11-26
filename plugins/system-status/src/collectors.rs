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

mod bsd;
pub(crate) mod common;
mod linux;
mod macos;
mod windows;
use crate::model::*;
use crate::mounts::{Capacity, Mount, MountScan};
type Sections = BTreeMap<String, Result<String, String>>;
pub struct DecodeContext<'a> {
    pub total: Option<u64>,
    pub interfaces: &'a [Device],
    pub paths: &'a [String],
}
pub fn command(family: Family, group: &str, nonce: &str, paths: &[String]) -> String {
    if family == Family::Windows {
        return windows::command(group, nonce);
    }
    let body = match family {
        Family::Linux => linux::command(group, paths),
        Family::Darwin => macos::command(group),
        _ => bsd::command(family, group),
    };
    let script=format!("LC_ALL=C; export LC_ALL\nn={}\ns() {{ k=$1; shift; printf '\\nSD1 %s BEGIN %s\\n' \"$n\" \"$k\"; \"$@\"; r=$?; printf '\\nSD1 %s END %s %s\\n' \"$n\" \"$k\" \"$r\"; }}\n{}\nprintf 'SD1 %s DONE\\n' \"$n\"",quote(nonce),body);
    format!("sh -c {}", quote(&script))
}
pub fn decode(
    family: Family,
    group: &str,
    text: &str,
    nonce: &str,
    ctx: &DecodeContext,
) -> Result<Sample, String> {
    if family == Family::Windows {
        return Ok(windows::decode(group, &windows_json(text, nonce)?, ctx));
    }
    let sections = envelope(text, nonce)?;
    Ok(match family {
        Family::Linux => linux::decode(group, &sections, ctx),
        Family::Darwin => macos::decode(group, &sections),
        _ => bsd::decode(group, &sections),
    })
}
pub fn windows_probe() -> &'static str {
    "powershell.exe -NoProfile -NonInteractive -Command \"[Console]::WriteLine('SODECK_WINDOWS_1')\""
}
fn envelope(text: &str, nonce: &str) -> Result<Sections, String> {
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
                    out.insert(
                        key.clone(),
                        if code == 0 {
                            Ok(data)
                        } else {
                            Err(format!("{key} 来源退出码 {code}"))
                        },
                    );
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
pub fn initial_capabilities(family: Family) -> Value {
    json!({"family":family.name(),"diskIo":matches!(family,Family::Linux|Family::Windows|Family::Darwin),"memoryPercent":matches!(family,Family::Linux|Family::Windows|Family::Darwin),"load":family!=Family::Windows})
}
#[cfg(test)]
pub fn sections(text: &str, nonce: &str) -> Result<BTreeMap<String, String>, String> {
    Ok(envelope(text, nonce)?
        .into_iter()
        .filter_map(|(k, v)| v.ok().map(|v| (k, v)))
        .collect())
}
#[cfg(test)]
pub fn mount_paths(text: &str) -> Vec<String> {
    linux::mounts(text)
        .rows
        .into_iter()
        .filter(|m| m.eligible)
        .map(|m| m.path)
        .collect()
}
#[cfg(test)]
mod command_tests {
    use super::*;
    #[test]
    fn every_windows_group_fits_cmd_exe_command_limit() {
        for group in ["identity", "dynamic", "topology", "capacity", "uptime"] {
            assert!(
                command(Family::Windows, group, &"n".repeat(128), &[]).len() < 8191,
                "{group}"
            );
        }
    }
    #[test]
    fn every_posix_command_fits_core_budget_and_reports_failed_sections() {
        for family in [
            Family::Linux,
            Family::Darwin,
            Family::FreeBsd,
            Family::OpenBsd,
            Family::NetBsd,
        ] {
            for group in ["identity", "dynamic", "topology", "capacity", "uptime"] {
                assert!(
                    command(
                        family,
                        group,
                        &"n".repeat(128),
                        &["/path with ' spaces".into()]
                    )
                    .len()
                        <= 16 * 1024
                );
            }
        }
        let source = envelope("SD1 n BEGIN memory\nSD1 n END memory 7\nSD1 n DONE\n", "n").unwrap();
        assert!(source["memory"].as_ref().unwrap_err().contains('7'));
    }
    #[test]
    #[cfg(target_os = "macos")]
    fn native_macos_sources_cover_real_dynamic_and_topology() {
        let mut state = StatusState::default();
        state.family = Some(Family::Darwin);
        let mut base = crate::metrics::Baseline::default();
        for group in ["identity", "topology", "dynamic", "capacity", "dynamic"] {
            let targets = state.mounts.targets();
            let paths: Vec<_> = targets.iter().map(|t| t.path.clone()).collect();
            let output = std::process::Command::new("/bin/sh")
                .args(["-c", &command(Family::Darwin, group, "native", &paths)])
                .output()
                .unwrap();
            assert!(output.status.success());
            let sample = decode(
                Family::Darwin,
                group,
                &String::from_utf8(output.stdout).unwrap(),
                "native",
                &DecodeContext {
                    total: state.identity.value.as_ref().and_then(|v| v.total),
                    interfaces: state.interfaces.value.as_deref().unwrap_or(&[]),
                    paths: &paths,
                },
            )
            .unwrap();
            let applied = crate::metrics::apply(
                &mut state,
                &mut base,
                sample,
                &targets,
                crate::scheduler::timestamp(),
                std::time::Instant::now(),
                5,
            );
            assert!(applied.successes > 0, "{group}: {:?}", applied.failures);
        }
        assert_eq!(state.memory.state, "ok");
        assert_eq!(state.cpu.state, "ok");
        assert_eq!(state.disk_source.state, "ok");
        assert!(!state.mounts.rows().is_empty());
    }
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

#[cfg(test)]
pub fn test_mounts(text: &str) -> Value {
    let scan = linux::mounts(text);
    json!({"coverage":if scan.complete{"complete"}else{"partial"},"mounts":scan.rows.iter().map(|m|json!({"identity":[m.identity],"path":m.path,"device":m.device,"filesystemType":m.kind,"readOnly":m.read_only})).collect::<Vec<_>>()})
}

#[cfg(test)]
pub fn test_meminfo(s: &str) -> std::collections::HashMap<String, u64> {
    linux::meminfo(s)
}

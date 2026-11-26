use crate::{collectors, model::*, mounts::Target};
use std::{collections::BTreeMap, time::Instant};

struct Previous {
    first: u64,
    second: u64,
    local: Instant,
    boot: String,
    remote: Option<f64>,
    clock: Option<(u64, u64)>,
}
#[derive(Default)]
pub struct Baseline {
    streams: BTreeMap<String, Previous>,
}
impl Baseline {
    pub fn clear(&mut self) {
        self.streams.clear();
    }
    pub fn memory_usage(&self) -> usize {
        self.streams
            .iter()
            .map(|(k, v)| k.capacity() + v.boot.capacity() + 160)
            .sum()
    }
    fn delta(
        &mut self,
        key: &str,
        first: u64,
        second: u64,
        d: &Clock,
        device_clock: Option<(u64, u64)>,
    ) -> Option<(u64, u64, f64)> {
        let current = Previous {
            first,
            second,
            local: d.local,
            boot: d.boot.clone(),
            remote: d.remote,
            clock: device_clock,
        };
        let previous = self.streams.insert(key.into(), current)?;
        if previous.boot != d.boot
            || d.local.duration_since(previous.local).as_secs_f64() > d.interval as f64 * 3.0
        {
            return None;
        }
        let seconds = match (device_clock, previous.clock) {
            (Some((t, hz)), Some((pt, phz))) if hz > 0 && hz == phz => {
                t.checked_sub(pt)? as f64 / hz as f64
            }
            (None, None) => match (d.remote, previous.remote) {
                (Some(t), Some(p)) => t - p,
                (None, None) => d.local.duration_since(previous.local).as_secs_f64(),
                _ => return None,
            },
            _ => return None,
        };
        if !seconds.is_finite() || seconds <= 0.0 {
            return None;
        }
        Some((
            first.checked_sub(previous.first)?,
            second.checked_sub(previous.second)?,
            seconds,
        ))
    }
}
struct Clock {
    local: Instant,
    boot: String,
    remote: Option<f64>,
    interval: u64,
}
#[derive(Default)]
pub struct ApplySummary {
    pub points: BTreeMap<String, Option<f64>>,
    pub successes: usize,
    pub failures: Vec<String>,
}
fn warm<T>(v: &mut Observed<T>, at: u64) {
    v.state = "warmingUp";
    v.attempted_at = Some(at);
    v.reason = None;
}
fn record<T>(v: &mut Observed<T>, read: Read<T>, at: u64, out: &mut ApplySummary) {
    if let Read::Failed(e) = &read {
        out.failures.push(e.clone());
    }
    if v.apply(read, at) {
        out.successes += 1;
    }
}
pub fn apply(
    state: &mut StatusState,
    base: &mut Baseline,
    sample: Sample,
    targets: &[Target],
    at: u64,
    now: Instant,
    interval: u64,
) -> ApplySummary {
    let mut out = ApplySummary::default();
    match sample {
        Sample::Identity(v) => record(&mut state.identity, v, at, &mut out),
        Sample::Uptime(v) => record(&mut state.uptime, v, at, &mut out),
        Sample::Topology(v) => {
            record(&mut state.interfaces, v.interfaces, at, &mut out);
            record(&mut state.load, v.load, at, &mut out);
            if let Read::Failed(e) = &v.mounts {
                out.failures.push(e.clone());
            }
            if state.mounts.scan(v.mounts, at) {
                out.successes += 1;
            }
        }
        Sample::Capacity(v) => match v {
            Read::Value(v) => {
                for r in v.rows {
                    if state.mounts.capacity(r, targets, at) {
                        out.successes += 1;
                    }
                }
                for (path, error) in v.failures {
                    state.mounts.fail_capacity(&path, &error, at);
                    out.failures.push(error);
                }
            }
            Read::Failed(e) => {
                state.mounts.fail_capacity("", &e, at);
                out.failures.push(e);
            }
            Read::Unsupported => {}
        },
        Sample::Dynamic(v) => {
            let clock = Clock {
                local: now,
                boot: v.boot,
                remote: v.remote_seconds,
                interval,
            };
            match v.cpu {
                Read::Value(Cpu::Direct(value)) => {
                    record(
                        &mut state.cpu,
                        Read::Value(Gauge {
                            value,
                            source: "top · 1 秒观察窗口".into(),
                            window_ms: Some(1000.0),
                        }),
                        at,
                        &mut out,
                    );
                    out.points.insert("cpu".into(), Some(value));
                }
                Read::Value(Cpu::Counter(total, idle)) => {
                    out.successes += 1;
                    let cpu_clock = (state.family == Some(collectors::Family::Windows))
                        .then_some((total, 10_000_000));
                    let delta = base.delta("cpu", total, idle, &clock, cpu_clock);
                    let value = delta.and_then(|(t, i, sec)| {
                        (t > 0 && i <= t).then_some(((t - i) as f64 / t as f64 * 100.0, sec))
                    });
                    if let Some((value, seconds)) = value {
                        state.cpu.apply(
                            Read::Value(Gauge {
                                value,
                                source: "累计 CPU 时间".into(),
                                window_ms: Some(seconds * 1000.0),
                            }),
                            at,
                        );
                    } else {
                        warm(&mut state.cpu, at);
                    }
                    out.points.insert("cpu".into(), value.map(|v| v.0));
                }
                other => {
                    base.streams.remove("cpu");
                    record(
                        &mut state.cpu,
                        match other {
                            Read::Failed(e) => Read::Failed(e),
                            _ => Read::Unsupported,
                        },
                        at,
                        &mut out,
                    );
                    out.points.insert("cpu".into(), None);
                }
            }
            let memory_value = match &v.memory {
                Read::Value(m) => Some((m.total - m.available) as f64 / m.total as f64 * 100.0),
                _ => None,
            };
            record(&mut state.memory, v.memory, at, &mut out);
            out.points.insert("memory".into(), memory_value);
            counters(
                state,
                base,
                v.network,
                "net",
                &clock,
                at,
                v.estimated,
                &mut out,
            );
            counters(
                state,
                base,
                v.disks,
                "disk",
                &clock,
                at,
                v.estimated,
                &mut out,
            );
            if let Some(up) = v.uptime {
                state.uptime.apply(Read::Value(up), at);
            }
            state.swap = v.swap;
        }
    }
    out
}
fn counters(
    state: &mut StatusState,
    base: &mut Baseline,
    read: Read<Vec<Counter>>,
    kind: &str,
    clock: &Clock,
    at: u64,
    estimated: bool,
    out: &mut ApplySummary,
) {
    let source = if kind == "net" {
        &mut state.network_source
    } else {
        &mut state.disk_source
    };
    match read {
        Read::Value(rows) => {
            source.apply(Read::Value(()), at);
            out.successes += 1;
            let prefix = format!("{kind}:");
            let keys: std::collections::BTreeSet<_> = rows
                .iter()
                .map(|r| format!("{kind}:{}", r.device.id))
                .collect();
            state
                .rates
                .retain(|k, _| !k.starts_with(&prefix) || keys.contains(k));
            base.streams
                .retain(|k, _| !k.starts_with(&prefix) || keys.contains(k));
            let devices: Vec<_> = rows.iter().map(|r| r.device.clone()).collect();
            if kind == "disk" {
                state.disks.apply(Read::Value(devices), at);
            } else if state.family == Some(collectors::Family::Windows) {
                state.counter_interfaces.apply(Read::Value(devices), at);
            } else {
                let list = state.interfaces.value.get_or_insert_with(Vec::new);
                for device in devices {
                    if !list.iter().any(|d| d.id == device.id) && list.len() < 256 {
                        list.push(device);
                    }
                }
            }
            for r in rows {
                let key = format!("{kind}:{}", r.device.id);
                let delta = base.delta(&key, r.first, r.second, clock, r.clock);
                let slot = state.rates.entry(key.clone()).or_default();
                let rates = delta.map(|(a, b, seconds)| Rates {
                    first: a as f64 / seconds,
                    second: b as f64 / seconds,
                    estimated,
                });
                out.points
                    .insert(format!("{key}:first"), rates.as_ref().map(|r| r.first));
                out.points
                    .insert(format!("{key}:second"), rates.as_ref().map(|r| r.second));
                if let Some(v) = rates {
                    slot.apply(Read::Value(v), at);
                } else {
                    warm(slot, at);
                }
            }
        }
        other => {
            let reason = match other {
                Read::Failed(e) => {
                    out.failures.push(e.clone());
                    Some(e)
                }
                _ => None,
            };
            source.apply(reason.clone().map_or(Read::Unsupported, Read::Failed), at);
            let prefix = format!("{kind}:");
            base.streams.retain(|k, _| !k.starts_with(&prefix));
            for (k, r) in state
                .rates
                .iter_mut()
                .filter(|(k, _)| k.starts_with(&prefix))
            {
                r.apply(reason.clone().map_or(Read::Unsupported, Read::Failed), at);
                out.points.insert(format!("{k}:first"), None);
                out.points.insert(format!("{k}:second"), None);
            }
        }
    }
}
pub fn failed(
    state: &mut StatusState,
    base: &mut Baseline,
    group: &str,
    error: &str,
    at: u64,
) -> ApplySummary {
    let mut out = ApplySummary::default();
    match group {
        "dynamic" => {
            base.clear();
            state.cpu.apply(Read::Failed(error.to_string()), at);
            state.memory.apply(Read::Failed(error.to_string()), at);
            state
                .network_source
                .apply(Read::Failed(error.to_string()), at);
            state.disk_source.apply(Read::Failed(error.to_string()), at);
            out.points.insert("cpu".into(), None);
            out.points.insert("memory".into(), None);
            for (k, r) in &mut state.rates {
                r.apply(Read::Failed(error.to_string()), at);
                out.points.insert(format!("{k}:first"), None);
                out.points.insert(format!("{k}:second"), None);
            }
        }
        "identity" => {
            state.identity.apply(Read::Failed(error.to_string()), at);
        }
        "topology" => {
            state.interfaces.apply(Read::Failed(error.to_string()), at);
            state.load.apply(Read::Failed(error.to_string()), at);
            state
                .mounts
                .status
                .apply(Read::Failed(error.to_string()), at);
        }
        "capacity" => state.mounts.fail_capacity("", error, at),
        "uptime" => {
            state.uptime.apply(Read::Failed(error.to_string()), at);
        }
        _ => {}
    }
    out
}
pub fn limited(s: &str, n: usize) -> String {
    collectors::common::limited(s, n)
}

#[cfg(test)]
mod reducer_tests {
    use super::*;
    fn dynamic(cpu: Read<Cpu>, network: Read<Vec<Counter>>) -> Sample {
        Sample::Dynamic(Dynamic {
            cpu,
            memory: Read::Unsupported,
            network,
            disks: Read::Unsupported,
            boot: "boot".into(),
            remote_seconds: None,
            uptime: None,
            estimated: true,
            swap: None,
        })
    }
    fn net(a: u64, t: u64) -> Read<Vec<Counter>> {
        Read::Value(vec![Counter {
            device: Device {
                id: "en0".into(),
                name: "en0".into(),
                ..Device::default()
            },
            first: a,
            second: a,
            clock: Some((t, 10)),
        }])
    }
    #[test]
    fn independent_clock_survives_cpu_failure_and_old_values_are_not_new_history() {
        let mut s = StatusState::default();
        let mut b = Baseline::default();
        let now = Instant::now();
        apply(
            &mut s,
            &mut b,
            dynamic(Read::Value(Cpu::Direct(20.0)), net(9007199254740993, 100)),
            &[],
            1,
            now,
            5,
        );
        let out = apply(
            &mut s,
            &mut b,
            dynamic(
                Read::Failed("cpu failed".into()),
                net(9007199254741043, 150),
            ),
            &[],
            2,
            now + std::time::Duration::from_secs(5),
            5,
        );
        assert_eq!(s.rates["net:en0"].value.as_ref().unwrap().first, 10.0);
        assert_eq!(s.cpu.observed_at, Some(1));
        assert_eq!(s.cpu.value.as_ref().unwrap().value, 20.0);
        assert_eq!(out.points["cpu"], None);
        assert_eq!(out.points["net:en0:first"], Some(10.0));
        let out = failed(&mut s, &mut b, "dynamic", "failed", 3);
        assert_eq!(out.points["net:en0:first"], None);
        let out = apply(
            &mut s,
            &mut b,
            dynamic(Read::Value(Cpu::Direct(10.0)), net(9007199254741093, 200)),
            &[],
            4,
            now + std::time::Duration::from_secs(10),
            5,
        );
        assert_eq!(out.points["net:en0:first"], None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collectors::Family;
    use serde_json::{json, Value};
    fn normalize_architecture(s: &str) -> Option<String> {
        collectors::common::architecture(s)
    }
    fn meminfo(s: &str) -> std::collections::HashMap<String, u64> {
        collectors::test_meminfo(s)
    }
    fn netstat(s: &str) -> Vec<(String, u64, u64)> {
        collectors::common::links(s)
            .unwrap()
            .into_iter()
            .map(|r| (r.name, r.first, r.second))
            .collect()
    }
    fn df(s: &str, _: Option<&String>) -> Option<Value> {
        collectors::common::capacities(s).ok()?.first().map(|r|json!({"id":r.device,"path":r.path,"total":r.total.to_string(),"used":r.used.map(|v|v.to_string()),"available":r.available.to_string()}))
    }
    fn apply(
        family: Family,
        group: &str,
        text: &str,
        nonce: &str,
        paths: &[String],
        data: &mut Value,
        base: &mut Baseline,
        interval: u64,
    ) -> Result<(), String> {
        let mut state = StatusState::fixture(data);
        state.family = Some(family);
        let targets = state.mounts.targets();
        let sample = collectors::decode(
            family,
            group,
            text,
            nonce,
            &collectors::DecodeContext {
                total: state.identity.value.as_ref().and_then(|v| v.total),
                interfaces: state.interfaces.value.as_deref().unwrap_or(&[]),
                paths,
            },
        )?;
        let result = super::apply(
            &mut state,
            base,
            sample,
            &targets,
            crate::scheduler::timestamp(),
            Instant::now(),
            interval,
        );
        *data = state.data();
        data["filesystems"] = json!(state.mounts.rows());
        data["mountTopology"] = json!({"mounts":state.mounts.entries.values().map(|e|json!({"identity":[e.mount.identity],"path":e.mount.path,"device":e.mount.device,"filesystemType":e.mount.kind,"readOnly":e.mount.read_only})).collect::<Vec<_>>()});
        data["mountPaths"] = json!(state
            .mounts
            .targets()
            .iter()
            .map(|t| t.path.clone())
            .collect::<Vec<_>>());
        if result.successes == 0 {
            Err(result
                .failures
                .first()
                .cloned()
                .unwrap_or("no sources".into()))
        } else {
            Ok(())
        }
    }
    #[test]
    fn macos_link_rows_without_hardware_address_use_byte_columns() {
        let sample = "Name Mtu Network Address Ipkts Ierrs Ibytes Opkts Oerrs Obytes Coll\nlo0 16384 <Link#1> 26067 0 7035943 26067 0 7035943 0\nen0 1500 <Link#9> 02:00:00:00:00:01 20 0 12000 10 0 8000 0\nutun223 1400 <Link#25> 15836 0 4788974 7414 0 2143775 0\n";
        assert_eq!(
            netstat(sample),
            vec![
                ("lo0".into(), 7035943, 7035943),
                ("en0".into(), 12000, 8000),
                ("utun223".into(), 4788974, 2143775)
            ]
        );
    }
    #[test]
    fn unix_identity_reports_architecture_without_guessing_from_model() {
        for (family, raw, arm64, expected) in [
            (Family::Linux, "x86_64", "", Some("x86_64")),
            (Family::Linux, "AMD64", "", Some("x86_64")),
            (Family::Linux, "aarch64", "", Some("ARM64")),
            (Family::Linux, "armv7l", "", Some("ARMv7")),
            (Family::FreeBsd, "amd64", "", Some("x86_64")),
            (Family::OpenBsd, "arm64", "", Some("ARM64")),
            (Family::NetBsd, "i386", "", Some("x86")),
            (Family::Unix, "riscv64", "", Some("riscv64")),
            (Family::Unix, "aarch64_be", "", Some("aarch64_be")),
            (Family::Darwin, "x86_64", "1", Some("ARM64")),
            (Family::Darwin, "x86_64", "0", Some("x86_64")),
            (Family::Linux, "", "", None),
            (Family::Linux, "unknown", "", None),
            (Family::Linux, "x86_64\nunexpected banner", "", None),
        ] {
            let payload = format!("SD1 n BEGIN system\nLinux 6.8 x86_64\nSD1 n END system 0\nSD1 n BEGIN model\nApple M2\nSD1 n END model 0\nSD1 n BEGIN architecture\n{raw}\nSD1 n END architecture 0\nSD1 n BEGIN arm64\n{arm64}\nSD1 n END arm64 0\nSD1 n DONE\n");
            let mut snapshot = json!({});
            apply(
                family,
                "identity",
                &payload,
                "n",
                &[],
                &mut snapshot,
                &mut Baseline::default(),
                5,
            )
            .unwrap();
            assert_eq!(
                snapshot["identity"]["architecture"].as_str(),
                expected,
                "{family:?}: {raw}"
            );
        }
    }
    #[test]
    fn windows_identity_uses_processor_architecture_and_keeps_unknown_null() {
        for (code, expected) in [
            (Some(9), Some("x86_64")),
            (Some(12), Some("ARM64")),
            (Some(0), Some("x86")),
            (Some(5), Some("ARM")),
            (Some(99), None),
            (None, None),
        ] {
            let payload = json!({"schemaVersion":1,"nonce":"n","cpu":[{"model":"Intel compatible processor","cores":8,"architecture":code}]}).to_string();
            let mut snapshot = json!({});
            apply(
                Family::Windows,
                "identity",
                &payload,
                "n",
                &[],
                &mut snapshot,
                &mut Baseline::default(),
                5,
            )
            .unwrap();
            assert_eq!(
                snapshot["identity"]["architecture"].as_str(),
                expected,
                "{code:?}"
            );
        }
    }
    #[test]
    fn native_identity_collects_architecture_from_the_system() {
        let family = match std::env::consts::OS {
            "windows" => Family::Windows,
            "macos" => Family::Darwin,
            _ => Family::Linux,
        };
        let command = collectors::command(family, "identity", "native", &[]);
        let output = if family == Family::Windows {
            std::process::Command::new("powershell.exe")
                .args(["-NoProfile", "-NonInteractive", "-Command", &command])
                .output()
        } else {
            std::process::Command::new("sh")
                .args(["-c", &command])
                .output()
        }
        .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let mut snapshot = json!({});
        apply(
            family,
            "identity",
            &String::from_utf8(output.stdout).unwrap(),
            "native",
            &[],
            &mut snapshot,
            &mut Baseline::default(),
            5,
        )
        .unwrap();
        assert_eq!(
            snapshot["identity"]["architecture"].as_str(),
            normalize_architecture(std::env::consts::ARCH).as_deref(),
            "{snapshot}"
        );
    }
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
        let mut data = json!({"filesystems":[{"id":"/dev/root","path":"/a","total":"102400","used":"40960","available":"51200","observedAt":1},{"id":"/dev/b","path":"/b","total":"102400","used":"40960","available":"51200","observedAt":2}]});
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
        assert_eq!(snapshot["cpu"]["state"], "warmingUp");
        assert_eq!(snapshot["cpu"]["value"], 50.0);
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
        assert!(
            snapshot["identity"]["architecture"].is_string(),
            "{snapshot}"
        );
        assert!(snapshot["memory"]["value"].is_number(), "{snapshot}");
        assert!(snapshot["cpu"]["value"].is_number(), "{snapshot}");
        assert!(!snapshot["filesystems"].as_array().unwrap().is_empty());
    }
    #[test]
    #[ignore = "requires isolated Ubuntu container with real Snap and ext4 mounts; SODECK_STATUS_TEST_CONTAINER"]
    fn linux_filesystem_rules_match_real_mounts() {
        let container = std::env::var("SODECK_STATUS_TEST_CONTAINER").unwrap();
        let snap_path = std::env::var("SODECK_STATUS_TEST_SNAP_PATH").unwrap();
        let exec = |command: &str| {
            let out = std::process::Command::new("docker")
                .args(["exec", &container, "sh", "-c", command])
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8(out.stdout).unwrap()
        };
        let mut data = json!({});
        let mut baseline = Baseline::default();
        let paths = vec![
            "/".into(),
            "/mnt/status-data".into(),
            "/boot".into(),
            "/run".into(),
            "/run/credentials/status-test".into(),
            snap_path.clone(),
        ];
        for group in ["topology", "capacity"] {
            let text = exec(&collectors::command(Family::Linux, group, "real", &paths));
            apply(
                Family::Linux,
                group,
                &text,
                "real",
                &paths,
                &mut data,
                &mut baseline,
                5,
            )
            .unwrap();
        }
        let projected = crate::filesystem::project(&crate::mounts::MountCatalog::fixture(&data));
        let row = |path: &str| projected.iter().find(|r| r["path"] == path).unwrap();
        assert_eq!(row(&snap_path)["capacityState"], "fixedImage");
        assert_eq!(row(&snap_path)["percent"], 100.0);
        assert_eq!(row(&snap_path)["spaceWarning"], false);
        assert_eq!(row("/boot")["displayGroup"], "auxiliary");
        assert_eq!(row("/mnt/status-data")["displayGroup"], "primary");
        assert_eq!(row("/run")["displayGroup"], "system");
        assert_eq!(
            row("/run/credentials/status-test")["capacityState"],
            "notApplicable"
        );
        exec("dd if=/dev/zero of=/boot/status-fill bs=1M count=26 2>/dev/null; sync");
        let text = exec(&collectors::command(
            Family::Linux,
            "capacity",
            "real",
            &paths,
        ));
        apply(
            Family::Linux,
            "capacity",
            &text,
            "real",
            &paths,
            &mut data,
            &mut baseline,
            5,
        )
        .unwrap();
        assert!(
            crate::filesystem::project(&crate::mounts::MountCatalog::fixture(&data))
                .iter()
                .any(|r| r["path"] == "/boot"
                    && r["spaceWarning"] == true
                    && r["displayGroup"] == "primary")
        );
        assert_eq!(data["filesystems"].as_array().unwrap().len(), 6);
        assert!(data["mountTopology"]["mounts"].as_array().unwrap().len() >= 6);
    }
}

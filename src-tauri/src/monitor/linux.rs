use super::model::*;
use crate::model::{AppError, AppResult};
use std::{collections::BTreeMap, time::Instant};

// Fixed commands only: no host data or user-selected paths are interpolated.
pub const COMMAND: &str = r#"LC_ALL=C; export LC_ALL
test "$(uname -s)" = Linux || exit 42
printf '\n__SODECK_METRICS_V1__clock\n'; cat /proc/uptime
printf '\n__SODECK_METRICS_V1__boot\n'; cat /proc/sys/kernel/random/boot_id
printf '\n__SODECK_METRICS_V1__cpu\n'; cat /proc/stat
printf '\n__SODECK_METRICS_V1__load\n'; cat /proc/loadavg
printf '\n__SODECK_METRICS_V1__memory\n'; cat /proc/meminfo
printf '\n__SODECK_METRICS_V1__network\n'; cat /proc/net/dev
printf '\n__SODECK_METRICS_V1__route\n'; cat /proc/net/route
printf '\n__SODECK_METRICS_V1__route6\n'; cat /proc/net/ipv6_route
printf '\n__SODECK_METRICS_V1__interfaces\n'
for d in /sys/class/net/*; do
  test -r "$d/ifindex" || continue
  IFS= read -r idx < "$d/ifindex"
  virtual=1; test -e "$d/device" && virtual=0
  printf '%s\t%s\t%s\n' "${d##*/}" "$idx" "$virtual"
done
printf '\n__SODECK_METRICS_V1__end\n'
"#;

fn invalid(message: &str) -> AppError {
    AppError::new("format", message)
}

#[derive(Clone, Debug)]
struct Cpu([u64; 8]);
fn cpus(text: &str) -> AppResult<BTreeMap<String, Cpu>> {
    let mut result = BTreeMap::new();
    for line in text.lines() {
        let mut fields = line.split_whitespace();
        let Some(name) = fields.next() else { continue };
        if name != "cpu"
            && !name
                .strip_prefix("cpu")
                .is_some_and(|s| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()))
        {
            continue;
        }
        let values = fields
            .take(8)
            .map(str::parse::<u64>)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| invalid("CPU 时间计数无效"))?;
        if values.len() < 4 || result.len() >= 4097 {
            return Err(invalid("CPU 数据不完整或核心数量超出限制"));
        }
        let mut counters = [0; 8];
        counters[..values.len()].copy_from_slice(&values);
        if result.insert(name.into(), Cpu(counters)).is_some() {
            return Err(invalid("CPU 编号重复"));
        }
    }
    if !result.contains_key("cpu") {
        return Err(invalid("无法读取 /proc/stat 的 CPU 计数"));
    }
    Ok(result)
}
fn cpu_percent(old: &Cpu, new: &Cpu) -> Option<f64> {
    let mut deltas = [0u128; 8];
    for (i, delta) in deltas.iter_mut().enumerate() {
        // Linux permits iowait to decrease. Guest time is already included in user/nice.
        *delta = if i == 4 {
            new.0[i].saturating_sub(old.0[i])
        } else {
            new.0[i].checked_sub(old.0[i])?
        } as u128;
    }
    let total: u128 = deltas.iter().sum();
    (total > 0).then(|| (total - deltas[3] - deltas[4]) as f64 * 100.0 / total as f64)
}
fn memory(text: &str) -> AppResult<MemorySample> {
    let mut values = BTreeMap::new();
    for line in text.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() == 3 && fields[2] == "kB" {
            if let Ok(n) = fields[1].parse::<u64>() {
                if let Some(bytes) = n.checked_mul(1024) {
                    values.insert(fields[0].trim_end_matches(':'), bytes);
                }
            }
        }
    }
    let total = *values
        .get("MemTotal")
        .ok_or_else(|| invalid("缺少 MemTotal"))?;
    let available = *values.get("MemAvailable").ok_or_else(|| {
        AppError::new(
            "unsupported",
            "内核未提供 MemAvailable，无法计算可靠的内存使用率",
        )
    })?;
    if total == 0 || available > total {
        return Err(invalid("内存容量数据无效"));
    }
    let swap = values
        .get("SwapTotal")
        .zip(values.get("SwapFree"))
        .filter(|(t, f)| t >= f);
    let cached = values.get("Cached").map(|c| {
        c.saturating_add(*values.get("SReclaimable").unwrap_or(&0))
            .saturating_sub(*values.get("Shmem").unwrap_or(&0))
    });
    Ok(MemorySample {
        total_bytes: total.to_string(),
        available_bytes: available.to_string(),
        used_bytes: (total - available).to_string(),
        used_percent: (total - available) as f64 * 100.0 / total as f64,
        cached_bytes: cached.map(|v| v.to_string()),
        swap_total_bytes: swap.map(|(t, _)| t.to_string()),
        swap_used_bytes: swap.map(|(t, f)| (t - f).to_string()),
    })
}
#[derive(Clone, Debug)]
struct Net {
    counts: [u64; 16],
    index: Option<u64>,
    virtual_interface: bool,
}
fn network(text: &str, interfaces: &str) -> AppResult<BTreeMap<String, Net>> {
    let metadata: BTreeMap<_, _> = interfaces
        .lines()
        .filter_map(|line| {
            let f: Vec<_> = line.split_whitespace().collect();
            (f.len() == 3).then(|| (f[0], (f[1].parse::<u64>().ok(), f[2] == "1")))
        })
        .collect();
    let mut result = BTreeMap::new();
    for line in text.lines() {
        let Some((name, fields)) = line.rsplit_once(':') else {
            continue;
        };
        let name = name.trim();
        if name.is_empty() || name.len() > 64 || result.len() >= 512 {
            return Err(invalid("网卡数据超出限制"));
        }
        let counters: Vec<u64> = fields
            .split_whitespace()
            .map(str::parse)
            .collect::<Result<_, _>>()
            .map_err(|_| invalid("网卡累计计数无效"))?;
        let counts: [u64; 16] = counters
            .try_into()
            .map_err(|_| invalid("网卡统计字段不完整"))?;
        let (index, virtual_interface) = metadata.get(name).copied().unwrap_or((None, true));
        if result
            .insert(
                name.into(),
                Net {
                    counts,
                    index,
                    virtual_interface,
                },
            )
            .is_some()
        {
            return Err(invalid("网卡名称重复"));
        }
    }
    if result.is_empty() {
        return Err(invalid("无法读取 /proc/net/dev"));
    }
    Ok(result)
}
fn default_interface(route: &str, route6: &str) -> Option<String> {
    let ipv4 = route
        .lines()
        .filter_map(|line| {
            let f: Vec<_> = line.split_whitespace().collect();
            if f.len() < 8
                || f[1] != "00000000"
                || f[7] != "00000000"
                || u32::from_str_radix(f[3], 16).ok()? & 1 == 0
            {
                return None;
            }
            Some((f[6].parse::<u64>().ok()?, f[0]))
        })
        .min();
    ipv4.map(|(_, name)| name.to_owned()).or_else(|| {
        route6
            .lines()
            .filter_map(|line| {
                let f: Vec<_> = line.split_whitespace().collect();
                if f.len() != 10
                    || f[0] != "00000000000000000000000000000000"
                    || f[1] != "00"
                    || f[9] == "lo"
                    || u32::from_str_radix(f[8], 16).ok()? & 1 == 0
                {
                    return None;
                }
                Some((u64::from_str_radix(f[5], 16).ok()?, f[9]))
            })
            .min()
            .map(|(_, name)| name.to_owned())
    })
}
#[derive(Debug)]
pub struct Raw {
    cpu: AppResult<BTreeMap<String, Cpu>>,
    memory: AppResult<MemorySample>,
    network: AppResult<BTreeMap<String, Net>>,
    default_interface: Option<String>,
    load: Option<[f64; 3]>,
    boot: String,
    uptime: Option<f64>,
    received: Instant,
}
pub fn parse(text: &str) -> AppResult<Raw> {
    let mut sections = BTreeMap::new();
    let mut key = "";
    for line in text.lines() {
        if let Some(name) = line.strip_prefix("__SODECK_METRICS_V1__") {
            key = name;
            if sections.insert(key, String::new()).is_some() {
                return Err(invalid("资源采样分段重复"));
            }
        } else if let Some(value) = sections.get_mut(key) {
            value.push_str(line);
            value.push('\n');
        }
    }
    if !sections.contains_key("end") {
        return Err(invalid("资源采样输出不完整"));
    }
    let get = |name| sections.get(name).map(String::as_str).unwrap_or("");
    let load = get("load")
        .split_whitespace()
        .take(3)
        .map(str::parse::<f64>)
        .collect::<Result<Vec<_>, _>>()
        .ok()
        .filter(|v| v.iter().all(|n| n.is_finite() && *n >= 0.0))
        .and_then(|v| v.try_into().ok());
    let boot = if get("boot").trim().is_empty() {
        get("cpu")
            .lines()
            .find(|s| s.starts_with("btime "))
            .unwrap_or("")
            .into()
    } else {
        get("boot").trim().into()
    };
    Ok(Raw {
        cpu: cpus(get("cpu")),
        memory: memory(get("memory")),
        network: network(get("network"), get("interfaces")),
        default_interface: default_interface(get("route"), get("route6")),
        load,
        boot,
        uptime: get("clock")
            .split_whitespace()
            .next()
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|n| n.is_finite() && *n >= 0.0),
        received: Instant::now(),
    })
}
fn apply<T>(metric: &mut Metric<T>, result: AppResult<T>, now: u64) {
    match result {
        Ok(value) => *metric = Metric::success(value, now),
        Err(e) => metric.fail(
            if e.kind == "unsupported" {
                "unsupported"
            } else {
                "unavailable"
            },
            &e.message,
        ),
    }
}
pub fn sample(
    raw: &Raw,
    previous: Option<&Raw>,
    metrics: &mut Metrics,
    now: u64,
    sequence: u64,
) -> HistoryPoint {
    let previous = previous.filter(|p| {
        p.boot == raw.boot
            && match (p.uptime, raw.uptime) {
                (Some(a), Some(b)) => b > a,
                _ => true,
            }
    });
    let elapsed = previous
        .map(|p| match (p.uptime, raw.uptime) {
            (Some(a), Some(b)) => b - a,
            _ => raw.received.duration_since(p.received).as_secs_f64(),
        })
        .filter(|n| *n > 0.0 && *n <= 30.0);
    let previous = previous.filter(|_| elapsed.is_some());
    apply(
        &mut metrics.cpu,
        raw.cpu
            .as_ref()
            .map(|cpu| {
                let old = previous.and_then(|p| p.cpu.as_ref().ok());
                CpuSample {
                    percent: old
                        .filter(|p| p.keys().eq(cpu.keys()))
                        .and_then(|p| cpu_percent(&p["cpu"], &cpu["cpu"])),
                    cores: cpu
                        .iter()
                        .filter(|(n, _)| n.as_str() != "cpu")
                        .map(|(n, c)| {
                            (
                                n.clone(),
                                old.and_then(|p| p.get(n)).and_then(|p| cpu_percent(p, c)),
                            )
                        })
                        .collect(),
                    load: raw.load,
                }
            })
            .map_err(Clone::clone),
        now,
    );
    apply(&mut metrics.memory, raw.memory.clone(), now);
    apply(
        &mut metrics.network,
        raw.network
            .as_ref()
            .map(|net| {
                net.iter()
                    .map(|(name, n)| {
                        let old = previous
                            .and_then(|p| p.network.as_ref().ok())
                            .and_then(|p| p.get(name))
                            .filter(|p| p.index == n.index);
                        let rates = old.and_then(|p| {
                            Some((
                                n.counts[0].checked_sub(p.counts[0])? as f64 / elapsed?,
                                n.counts[8].checked_sub(p.counts[8])? as f64 / elapsed?,
                            ))
                        });
                        NetworkSample {
                            name: name.clone(),
                            index: n.index,
                            is_default: raw.default_interface.as_ref() == Some(name),
                            virtual_interface: n.virtual_interface,
                            rx_bytes: n.counts[0].to_string(),
                            tx_bytes: n.counts[8].to_string(),
                            rx_per_second: rates.map(|r| r.0),
                            tx_per_second: rates.map(|r| r.1),
                            rx_errors: n.counts[2].to_string(),
                            tx_errors: n.counts[10].to_string(),
                            rx_dropped: n.counts[3].to_string(),
                            tx_dropped: n.counts[11].to_string(),
                        }
                    })
                    .collect()
            })
            .map_err(Clone::clone),
        now,
    );
    HistoryPoint {
        sequence,
        at: now,
        gap: previous.is_none(),
        cpu: (metrics.cpu.status == "ready")
            .then(|| metrics.cpu.value.as_ref().and_then(|v| v.percent))
            .flatten(),
        memory: (metrics.memory.status == "ready")
            .then(|| metrics.memory.value.as_ref().map(|v| v.used_percent))
            .flatten(),
        network: if metrics.network.status == "ready" {
            metrics
                .network
                .value
                .as_ref()
                .into_iter()
                .flatten()
                .map(|n| (n.name.clone(), [n.rx_per_second, n.tx_per_second]))
                .collect()
        } else {
            BTreeMap::new()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(t: u64, cpu: &str, rx: u64, idx: u64, boot: &str) -> Raw {
        parse(&format!("__SODECK_METRICS_V1__clock\n{t} 0\n__SODECK_METRICS_V1__boot\n{boot}\n__SODECK_METRICS_V1__cpu\ncpu {cpu}\ncpu0 {cpu}\n__SODECK_METRICS_V1__memory\nMemTotal: 1000 kB\nMemAvailable: 600 kB\nSwapTotal: 20 kB\nSwapFree: 10 kB\n__SODECK_METRICS_V1__network\neth0: {rx} 1 2 3 0 0 0 0 100 1 4 5 0 0 0 0\n__SODECK_METRICS_V1__interfaces\neth0 {idx} 0\n__SODECK_METRICS_V1__end\n")).unwrap()
    }
    #[test]
    fn rates_use_remote_elapsed_time_and_exclude_guest_double_counting() {
        let a = fixture(10, "100 0 0 900 0 0 0 0 50 0", 9007199254740993, 2, "a");
        let b = fixture(12, "150 0 0 950 0 0 0 0 100 0", 9007199254741193, 2, "a");
        let mut metrics = Metrics::default();
        assert!(sample(&a, None, &mut metrics, 1000, 1).cpu.is_none());
        let point = sample(&b, Some(&a), &mut metrics, 3000, 2);
        assert_eq!(point.cpu, Some(50.0));
        assert_eq!(point.memory, Some(40.0));
        assert_eq!(point.network["eth0"], [Some(100.0), Some(0.0)]);
        assert_eq!(
            metrics.network.value.unwrap()[0].rx_bytes,
            "9007199254741193"
        );
    }
    #[test]
    fn reboot_interface_replacement_and_counter_regression_reset_baselines() {
        let a = fixture(10, "100 0 0 900", 1000, 2, "a");
        for b in [
            fixture(12, "150 0 0 950", 2000, 2, "b"),
            fixture(12, "150 0 0 950", 2000, 3, "a"),
            fixture(12, "150 0 0 950", 999, 2, "a"),
        ] {
            let p = sample(&b, Some(&a), &mut Metrics::default(), 0, 1);
            assert_eq!(p.network["eth0"], [None, None]);
        }
        assert_eq!(
            cpu_percent(
                &Cpu([100, 0, 0, 100, 20, 0, 0, 0]),
                &Cpu([110, 0, 0, 110, 10, 0, 0, 0])
            ),
            Some(50.0)
        );
    }
    #[test]
    fn bad_memory_does_not_hide_cpu_or_network() {
        let mut a = fixture(10, "100 0 0 900", 100, 2, "a");
        a.memory = memory("MemTotal: 100 kB\nMemFree: 20 kB\n");
        let mut metrics = Metrics::default();
        sample(&a, None, &mut metrics, 0, 1);
        assert_eq!(metrics.memory.status, "unsupported");
        assert_eq!(metrics.network.status, "ready");
        assert!(parse("__SODECK_METRICS_V1__cpu\ncpu 1 2 3 4").is_err());
    }
    #[test]
    fn default_route_uses_lowest_metric_and_supports_ipv6() {
        assert_eq!(
            default_interface(
                "eth0 00000000 0 0003 0 0 100 00000000\nens3 00000000 0 0003 0 0 10 00000000",
                ""
            ),
            Some("ens3".into())
        );
        assert_eq!(default_interface("", "00000000000000000000000000000000 00 00000000000000000000000000000000 00 fe800000000000000000000000000001 00000064 0 0 00000003 eth0"), Some("eth0".into()));
    }
}

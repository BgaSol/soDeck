use super::*;
use crate::remote_mounts;
use std::collections::{HashMap, HashSet};

pub(super) struct Schedule {
    epoch: u64,
    table_at: Instant,
    table_running: bool,
    running: HashSet<String>,
    next: HashMap<String, Instant>,
    failures: HashMap<String, u32>,
}
impl Default for Schedule {
    fn default() -> Self {
        Self {
            epoch: 0,
            table_at: Instant::now(),
            table_running: false,
            running: HashSet::new(),
            next: HashMap::new(),
            failures: HashMap::new(),
        }
    }
}
impl Schedule {
    pub fn refresh(&mut self) {
        self.epoch += 1;
        self.table_at = Instant::now();
        self.next.clear();
        self.failures.clear();
    }
}

pub(super) fn run(state: Weak<State>, entry: Arc<Entry>) {
    loop {
        let Some(state) = state.upgrade() else { break };
        if entry.check(&state).is_err() {
            break;
        }
        let mut table_job = None;
        let mut usage_jobs = Vec::new();
        let mut stale_changed = false;
        {
            let mut c = entry.core.lock().unwrap();
            let visible = c.subscribers.values().any(|s| s.interest.visible);
            if visible {
                for cached in c.mounts.usage.values_mut() {
                    if !cached.stale
                        && cached
                            .value
                            .as_ref()
                            .and_then(|v| v.updated_at)
                            .is_some_and(|at| now().saturating_sub(at) >= 30_000)
                    {
                        cached.stale = true;
                        stale_changed = true;
                    }
                }
                if !c.schedule.table_running && Instant::now() >= c.schedule.table_at {
                    c.schedule.table_running = true;
                    c.mounts.loading = true;
                    table_job = Some(c.schedule.epoch);
                } else if !c.schedule.table_running && c.mounts.error.is_empty() {
                    if let Some(table) = c.mounts.table.clone().filter(|t| t.status == "ready") {
                        let paths: HashSet<_> = c
                            .subscribers
                            .values()
                            .filter(|s| s.interest.visible)
                            .flat_map(|s| s.interest.mount_paths.iter().cloned())
                            .collect();
                        let mut mounts: Vec<_> = table
                            .mounts
                            .iter()
                            .filter(|m| paths.contains(&m.target))
                            .collect();
                        mounts.sort_by_key(|m| c.schedule.next.get(&m.key).copied());
                        for mount in mounts {
                            if c.schedule.running.len() >= 2 {
                                break;
                            }
                            if c.schedule.running.contains(&mount.key)
                                || c.schedule
                                    .next
                                    .get(&mount.key)
                                    .is_some_and(|at| *at > Instant::now())
                            {
                                continue;
                            }
                            if !remote_mounts::has_capacity(&mount.fs_type) {
                                // Still supply a useful state to file-tree consumers of pseudo filesystems.
                                if c.mounts.usage.contains_key(&mount.key) {
                                    continue;
                                }
                            }
                            c.schedule.running.insert(mount.key.clone());
                            let cached = c.mounts.usage.entry(mount.key.clone()).or_default();
                            cached.loading = true;
                            cached.stale = cached.value.is_some();
                            usage_jobs.push((
                                c.schedule.epoch,
                                table.version.clone(),
                                mount.key.clone(),
                            ));
                        }
                    }
                }
            }
        }
        if stale_changed || table_job.is_some() || !usage_jobs.is_empty() {
            entry.publish_mounts(&state);
        }
        if let Some(epoch) = table_job {
            let e = entry.clone();
            let s = state.clone();
            spawn(&entry, move || {
                let result = e
                    .spec()
                    .and_then(|spec| remote_mounts::query_table(&spec, &e.cancel, &|| e.check(&s)));
                {
                    let mut c = e.core.lock().unwrap();
                    c.schedule.table_running = false;
                    c.mounts.loading = false;
                    if c.schedule.epoch == epoch {
                        c.schedule.table_at = Instant::now() + Duration::from_secs(30);
                        match result {
                            Ok(table) => {
                                if c.mounts.table.as_ref().map(|t| &t.version)
                                    != Some(&table.version)
                                {
                                    c.mounts.usage.clear();
                                    c.schedule.next.clear();
                                    c.schedule.failures.clear();
                                }
                                if table.status == "unsupported" {
                                    c.schedule.table_at = Instant::now() + Duration::from_secs(300);
                                }
                                c.mounts.table = Some(table);
                                c.mounts.fetched_at = now();
                                c.mounts.error.clear();
                            }
                            Err(error) => {
                                if error.kind == "mountChanged" {
                                    c.mounts.table = None;
                                    c.mounts.usage.clear();
                                }
                                for usage in c.mounts.usage.values_mut() {
                                    usage.stale = true;
                                }
                                if matches!(
                                    error.kind.as_str(),
                                    "hostKey" | "authentication" | "permission"
                                ) {
                                    c.schedule.table_at = Instant::now() + Duration::from_secs(300);
                                }
                                c.mounts.error = error.message;
                            }
                        }
                    }
                }
                if e.check(&s).is_ok() {
                    e.publish_mounts(&s);
                }
            });
        }
        for (epoch, version, key) in usage_jobs {
            let e = entry.clone();
            let s = state.clone();
            spawn(&entry, move || {
                let result = e
                    .spec()
                    .and_then(|spec| {
                        remote_mounts::query_usage(
                            &spec,
                            &version,
                            vec![key.clone()],
                            &e.cancel,
                            &|| e.check(&s),
                        )
                    })
                    .and_then(|v| {
                        v.into_iter()
                            .next()
                            .ok_or_else(|| AppError::new("format", "未返回挂载容量"))
                    })
                    .and_then(|v| {
                        if v.status == "unavailable" {
                            Err(AppError::new(
                                "mountUsage",
                                v.error.as_deref().unwrap_or("容量不可用"),
                            ))
                        } else {
                            Ok(v)
                        }
                    });
                complete_usage(&mut e.core.lock().unwrap(), epoch, &version, key, result);
                if e.check(&s).is_ok() {
                    e.publish_mounts(&s);
                }
            });
        }
        drop(state);
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn complete_usage(
    c: &mut Core,
    epoch: u64,
    version: &str,
    key: String,
    result: AppResult<remote_mounts::MountUsage>,
) {
    c.schedule.running.remove(&key);
    if let Some(v) = c.mounts.usage.get_mut(&key) {
        v.loading = false;
    }

    if c.schedule.epoch == epoch
        && c.mounts
            .table
            .as_ref()
            .is_some_and(|t| t.version == version)
    {
        match result {
            Ok(value) => {
                c.schedule.failures.remove(&key);
                c.schedule.next.insert(
                    key.clone(),
                    Instant::now()
                        + Duration::from_secs(if value.status == "unsupported" {
                            300
                        } else {
                            30
                        }),
                );
                c.mounts.usage.insert(
                    key,
                    CachedUsage {
                        value: Some(value),
                        loading: false,
                        stale: false,
                        error: None,
                    },
                );
            }
            Err(error) => {
                let failures = c.schedule.failures.entry(key.clone()).or_default();
                *failures = failures.saturating_add(1);
                let delay = 30 * 2u64.pow((*failures - 1).min(4));
                c.schedule.next.insert(
                    key.clone(),
                    Instant::now() + Duration::from_secs(delay.min(300)),
                );
                let value = c.mounts.usage.entry(key).or_default();
                value.stale = true;
                value.error = Some(error.message.clone());
                if error.kind == "mountChanged" {
                    c.mounts.table = None;
                    c.mounts.usage.clear();
                    c.mounts.error = error.message;
                    c.schedule.refresh();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ready() -> Core {
        let mut core = Core::default();
        core.mounts.table = Some(remote_mounts::MountTable {
            version: "v1".into(),
            status: "ready",
            mounts: vec![],
            issues: vec![],
            message: None,
        });
        core
    }
    fn value(percent: u32) -> remote_mounts::MountUsage {
        remote_mounts::MountUsage {
            mount_key: "root".into(),
            status: "ready",
            total_bytes: Some("1000".into()),
            used_bytes: Some("500".into()),
            available_bytes: Some("500".into()),
            used_percent: Some(percent),
            updated_at: Some(now()),
            error: None,
        }
    }
    #[test]
    fn late_results_cannot_cross_manual_refresh_or_replacement() {
        let mut c = ready();
        complete_usage(&mut c, 0, "v1", "root".into(), Ok(value(50)));
        c.schedule.refresh();
        complete_usage(&mut c, 0, "v1", "root".into(), Ok(value(99)));
        assert_eq!(
            c.mounts.usage["root"].value.as_ref().unwrap().used_percent,
            Some(50)
        );
        c.mounts.table.as_mut().unwrap().version = "v2".into();
        c.mounts.usage.clear();
        complete_usage(&mut c, 1, "v1", "root".into(), Ok(value(99)));
        assert!(c.mounts.usage.is_empty());
    }
    #[test]
    fn failure_retains_values_with_backoff_and_changed_mounts_clear_attribution() {
        let mut c = ready();
        complete_usage(&mut c, 0, "v1", "root".into(), Ok(value(50)));
        for expected in [30, 60, 120, 240, 300, 300] {
            let before = Instant::now();
            complete_usage(
                &mut c,
                0,
                "v1",
                "root".into(),
                Err(AppError::new("timeout", "slow mount")),
            );
            assert!(c.mounts.usage["root"].stale);
            assert_eq!(
                c.mounts.usage["root"].value.as_ref().unwrap().used_percent,
                Some(50)
            );
            assert!(c.schedule.next["root"] >= before + Duration::from_secs(expected));
            assert!(c.schedule.next["root"] < before + Duration::from_secs(expected + 1));
        }
        c.schedule.refresh();
        assert!(!c.schedule.next.contains_key("root"));
        complete_usage(
            &mut c,
            1,
            "v1",
            "root".into(),
            Err(AppError::new("mountChanged", "unmounted")),
        );
        assert!(c.mounts.table.is_none());
        assert!(c.mounts.usage.is_empty());
    }
}

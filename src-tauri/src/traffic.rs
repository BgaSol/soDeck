//! App-lifetime SSH byte accounting; no remote polling and no credential ownership.
use serde::Serialize;
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Copy, Debug)]
pub enum TrafficPurpose {
    ResourceMonitor,
    MountMonitor,
    DockerMonitor,
    Terminal,
    ContainerTerminal,
    Logs,
    Files,
    Transfer,
    #[allow(dead_code)] // Reserved for connections with more than one business purpose.
    Shared,
}
impl TrafficPurpose {
    fn bucket(self) -> usize {
        match self {
            Self::ResourceMonitor | Self::MountMonitor | Self::DockerMonitor => 0,
            Self::Terminal | Self::ContainerTerminal | Self::Logs => 1,
            Self::Files | Self::Transfer => 2,
            Self::Shared => 3,
        }
    }
}
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub struct ConnectionTraffic {
    started_at: u64,
    sent: AtomicU64,
    received: AtomicU64,
    closed: AtomicBool,
}
impl Default for ConnectionTraffic {
    fn default() -> Self {
        Self {
            started_at: now(),
            sent: AtomicU64::new(0),
            received: AtomicU64::new(0),
            closed: AtomicBool::new(false),
        }
    }
}
impl ConnectionTraffic {
    pub fn record(&self, sent: bool, count: isize) {
        if count > 0 {
            let counter = if sent { &self.sent } else { &self.received };
            let mut n = counter.load(Ordering::Relaxed);
            loop {
                match counter.compare_exchange_weak(
                    n,
                    n.saturating_add(count as u64),
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => break,
                    Err(current) => n = current,
                }
            }
        }
    }
    pub fn close(&self) {
        self.closed.store(true, Ordering::Release);
    }
    pub(crate) fn counts(&self) -> [u64; 2] {
        [
            self.sent.load(Ordering::Relaxed),
            self.received.load(Ordering::Relaxed),
        ]
    }
}
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Values {
    pub sent_bytes: String,
    pub received_bytes: String,
    pub sent_per_second: Option<f64>,
    pub received_per_second: Option<f64>,
}
impl Values {
    fn new(total: [u64; 2], delta: [u64; 2], elapsed: Option<f64>) -> Self {
        Self {
            sent_bytes: total[0].to_string(),
            received_bytes: total[1].to_string(),
            sent_per_second: elapsed.map(|s| delta[0] as f64 / s),
            received_per_second: elapsed.map(|s| delta[1] as f64 / s),
        }
    }
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sample {
    pub epoch: String,
    pub revision: u64,
    pub started_at: u64,
    pub sampled_at: u64,
    pub active_connections: usize,
    pub measurement: &'static str,
    pub status: &'static str,
    pub total: Values,
    pub monitoring: Values,
    pub terminal_and_logs: Values,
    pub files: Values,
    pub other: Values,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Point {
    pub sequence: u64,
    pub at: u64,
    pub sent: Option<f64>,
    pub received: Option<f64>,
    pub monitor_sent: Option<f64>,
    pub monitor_received: Option<f64>,
    pub gap: bool,
}
#[derive(Clone, Serialize)]
pub struct HistoryPage {
    pub epoch: String,
    pub points: Vec<Point>,
    pub truncated: bool,
}
struct Active {
    counter: Arc<ConnectionTraffic>,
    bucket: usize,
    previous: [u64; 2],
}
struct Core {
    active: Vec<Active>,
    totals: [[u64; 2]; 4],
    last: Instant,
    sample: Sample,
    history: VecDeque<Point>,
    truncated: bool,
}
pub struct TrafficAccount {
    core: Mutex<Core>,
}
impl TrafficAccount {
    fn new() -> Self {
        let at = now();
        let zero = Values::new([0; 2], [0; 2], None);
        Self {
            core: Mutex::new(Core {
                active: Vec::new(),
                totals: [[0; 2]; 4],
                last: Instant::now(),
                sample: Sample {
                    epoch: uuid::Uuid::new_v4().to_string(),
                    revision: 0,
                    started_at: at,
                    sampled_at: at,
                    active_connections: 0,
                    measurement: "sshStream",
                    status: "collecting",
                    total: zero.clone(),
                    monitoring: zero.clone(),
                    terminal_and_logs: zero.clone(),
                    files: zero.clone(),
                    other: zero,
                },
                history: VecDeque::new(),
                truncated: false,
            }),
        }
    }
    pub fn attach(&self, counter: Arc<ConnectionTraffic>, purpose: TrafficPurpose) {
        let mut c = self.core.lock().unwrap();
        if c.active.iter().any(|a| Arc::ptr_eq(&a.counter, &counter)) {
            return;
        }
        let bucket = purpose.bucket();
        c.sample.started_at = c.sample.started_at.min(counter.started_at);
        let previous = counter.counts();
        for i in 0..2 {
            c.totals[bucket][i] = c.totals[bucket][i].saturating_add(previous[i]);
        }
        c.active.push(Active {
            counter,
            bucket,
            previous,
        });
        // Handshake bytes enter cumulative totals, never a later interval's rate.
        Self::update_values(&mut c, [[0; 2]; 4], None);
        c.sample.active_connections = c.active.len();
        c.sample.revision += 1;
        c.sample.status = "collecting";
    }
    fn update_values(c: &mut Core, delta: [[u64; 2]; 4], elapsed: Option<f64>) {
        let mut total = [0u64; 2];
        let mut change = [0u64; 2];
        for b in 0..4 {
            for i in 0..2 {
                total[i] = total[i].saturating_add(c.totals[b][i]);
                change[i] = change[i].saturating_add(delta[b][i]);
            }
        }
        c.sample.total = Values::new(total, change, elapsed);
        c.sample.monitoring = Values::new(c.totals[0], delta[0], elapsed);
        c.sample.terminal_and_logs = Values::new(c.totals[1], delta[1], elapsed);
        c.sample.files = Values::new(c.totals[2], delta[2], elapsed);
        c.sample.other = Values::new(c.totals[3], delta[3], elapsed);
    }
    fn sample_at(&self, instant: Instant, at: u64) {
        let mut c = self.core.lock().unwrap();
        let elapsed = instant.saturating_duration_since(c.last).as_secs_f64();
        if elapsed < 0.001 {
            return;
        }
        let mut delta = [[0u64; 2]; 4];
        let mut active = std::mem::take(&mut c.active);
        active.retain_mut(|a| {
            let closed = a.counter.closed.load(Ordering::Acquire);
            let current = a.counter.counts();
            for i in 0..2 {
                let n = current[i].saturating_sub(a.previous[i]);
                c.totals[a.bucket][i] = c.totals[a.bucket][i].saturating_add(n);
                delta[a.bucket][i] = delta[a.bucket][i].saturating_add(n);
            }
            a.previous = current;
            !closed
        });
        c.active = active;
        c.last = instant;
        c.sample.revision += 1;
        c.sample.sampled_at = at;
        c.sample.active_connections = c.active.len();
        c.sample.status = if c.active.is_empty() { "idle" } else { "ready" };
        // Long local sleeps create an explicit gap instead of an apparent instant burst.
        let duration = (elapsed <= 30.0).then_some(elapsed);
        Self::update_values(&mut c, delta, duration);
        let s = &c.sample;
        let point = Point {
            sequence: s.revision,
            at,
            sent: s.total.sent_per_second,
            received: s.total.received_per_second,
            monitor_sent: s.monitoring.sent_per_second,
            monitor_received: s.monitoring.received_per_second,
            gap: duration.is_none(),
        };
        c.history.push_back(point);
        while c.history.len() > 1801
            || c.history
                .front()
                .is_some_and(|p| p.at < at.saturating_sub(3_600_000))
        {
            c.history.pop_front();
        }
    }
    pub fn snapshot(&self) -> Sample {
        self.core.lock().unwrap().sample.clone()
    }
    pub fn update(&self) -> (Sample, Option<Point>) {
        let c = self.core.lock().unwrap();
        (c.sample.clone(), c.history.back().cloned())
    }
    #[cfg(test)]
    pub fn latest_point(&self) -> Option<Point> {
        self.core.lock().unwrap().history.back().cloned()
    }
    pub fn history(&self, since: u64) -> HistoryPage {
        let c = self.core.lock().unwrap();
        HistoryPage {
            epoch: c.sample.epoch.clone(),
            points: c
                .history
                .iter()
                .filter(|p| p.at >= since)
                .cloned()
                .collect(),
            truncated: c.truncated,
        }
    }
}

#[derive(Default)]
pub struct TrafficManager {
    accounts: Mutex<HashMap<String, Arc<TrafficAccount>>>,
}
impl TrafficManager {
    pub fn new() -> Arc<Self> {
        let manager = Arc::new(Self::default());
        let weak = Arc::downgrade(&manager);
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(2));
            let Some(manager) = weak.upgrade() else { break };
            manager.flush();
        });
        manager
    }
    pub fn account(&self, identity: &str) -> Arc<TrafficAccount> {
        self.accounts
            .lock()
            .unwrap()
            .entry(identity.into())
            .or_insert_with(|| Arc::new(TrafficAccount::new()))
            .clone()
    }
    pub fn flush(&self) {
        let accounts: Vec<_> = self.accounts.lock().unwrap().values().cloned().collect();
        let instant = Instant::now();
        let at = now();
        for account in &accounts {
            account.sample_at(instant, at);
        }
        // A global budget bounds history even with many server profiles.
        let budget =
            (16 * 1024 * 1024 / std::mem::size_of::<Point>() / accounts.len().max(1)).min(1801);
        for account in accounts {
            let mut c = account.core.lock().unwrap();
            let trimmed = c.history.len() > budget;
            while c.history.len() > budget {
                c.history.pop_front();
                c.truncated = true;
            }
            if trimmed {
                c.history.shrink_to_fit();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn counts_successes_once_and_preserves_closed_connections() {
        let a = TrafficAccount::new();
        let c = Arc::new(ConnectionTraffic::default());
        c.record(true, 100); // Before attribution: cumulative only.
        a.attach(c.clone(), TrafficPurpose::ResourceMonitor);
        a.attach(c.clone(), TrafficPurpose::Files);
        c.record(true, -37);
        c.record(false, 0);
        c.record(true, 40);
        c.record(false, 80);
        c.close();
        let start = a.core.lock().unwrap().last;
        a.sample_at(start + Duration::from_secs(2), now());
        let s = a.snapshot();
        assert_eq!(s.total.sent_bytes, "140");
        assert_eq!(s.monitoring.sent_bytes, "140");
        assert_eq!(s.files.sent_bytes, "0");
        assert_eq!(s.total.sent_per_second, Some(20.0));
        assert_eq!(s.total.received_per_second, Some(40.0));
        assert_eq!(s.active_connections, 0);
        a.sample_at(start + Duration::from_secs(4), now());
        assert_eq!(a.snapshot().total.sent_bytes, "140");
        assert_eq!(a.snapshot().total.sent_per_second, Some(0.0));
    }
    #[test]
    fn independent_accounts_and_sleep_gaps_keep_precise_totals() {
        let manager = TrafficManager::default();
        let a = manager.account("a");
        assert!(Arc::ptr_eq(&a, &manager.account("a")));
        let c = Arc::new(ConnectionTraffic::default());
        a.attach(c.clone(), TrafficPurpose::Transfer);
        c.sent.store(9_007_199_254_740_993, Ordering::Relaxed);
        let start = a.core.lock().unwrap().last;
        a.sample_at(start + Duration::from_secs(60), now());
        assert_eq!(a.snapshot().files.sent_bytes, "9007199254740993");
        assert!(a.snapshot().total.sent_per_second.is_none());
        assert!(a.latest_point().unwrap().gap);
        assert_eq!(manager.account("b").snapshot().total.sent_bytes, "0");
    }
}

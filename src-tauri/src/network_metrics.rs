//! In-memory plugin payload totals. Local view replay never passes this boundary.
use crate::{terminal::Owner, AppCore};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{atomic::Ordering, Arc, Mutex},
    time::Instant,
};
#[derive(Default)]
pub struct Metrics {
    entries: Mutex<HashMap<(String, String), Total>>,
    transport: Mutex<HashMap<String, Physical>>,
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retired_transports_remain_in_run_totals_and_idle_rates_expire() {
        let root = std::env::temp_dir().join(format!("sodeck-metrics-{}", std::process::id()));
        let core = AppCore::open(root.clone(), "test".into()).unwrap();
        let first = Arc::new(crate::connections::io::Counters::default());
        core.network_metrics
            .register_transport("server", first.clone());
        first.sent.store(100, Ordering::Relaxed);
        first.received.store(200, Ordering::Relaxed);
        let next = Arc::new(crate::connections::io::Counters::default());
        core.network_metrics
            .register_transport("server", next.clone());
        next.sent.store(30, Ordering::Relaxed);
        let owner = Owner {
            plugin: "files".into(),
            instance: "one".into(),
            view: "view".into(),
            context: crate::workspaces::Context {
                workspace_id: "w".into(),
                server_id: "server".into(),
                connection_id: "c".into(),
                generation: 1,
            },
        };
        core.record_business(&owner, 25, 75);
        let mut older = owner.clone();
        older.instance = "two".into();
        core.record_business(&older, 5, 10);
        for t in core.network_metrics.entries.lock().unwrap().values_mut() {
            for s in &mut t.samples {
                s.0 = Instant::now() - std::time::Duration::from_secs(4);
            }
        }
        let snapshot = core.network_snapshot();
        assert_eq!(snapshot["servers"][0]["sent"], "130");
        assert_eq!(snapshot["servers"][0]["received"], "200");
        assert_eq!(snapshot["plugins"][0]["sent"], "30");
        assert_eq!(snapshot["plugins"][0]["received"], "85");
        assert_eq!(snapshot["plugins"][0]["sentPerSecond"], 0.0);
        drop(core);
        std::fs::remove_dir_all(root).unwrap();
    }
}
struct Physical {
    current: Arc<crate::connections::io::Counters>,
    retired: (u64, u64),
    samples: std::collections::VecDeque<(Instant, u64, u64)>,
}
impl Metrics {
    pub(crate) fn register_transport(
        &self,
        server: &str,
        counter: Arc<crate::connections::io::Counters>,
    ) {
        let mut all = self.transport.lock().unwrap();
        if let Some(t) = all.get_mut(server) {
            t.retired.0 = t
                .retired
                .0
                .saturating_add(t.current.sent.load(Ordering::Relaxed));
            t.retired.1 = t
                .retired
                .1
                .saturating_add(t.current.received.load(Ordering::Relaxed));
            t.current = counter;
        } else {
            all.insert(
                server.into(),
                Physical {
                    current: counter,
                    retired: (0, 0),
                    samples: std::collections::VecDeque::from([(Instant::now(), 0, 0)]),
                },
            );
        }
    }
}
#[derive(Default)]
struct Total {
    sent: u64,
    received: u64,
    samples: std::collections::VecDeque<(Instant, u64, u64)>,
}
impl AppCore {
    pub(crate) fn record_business(&self, owner: &Owner, sent: u64, received: u64) {
        let mut entries = self.network_metrics.entries.lock().unwrap();
        let total = entries
            .entry((owner.context.server_id.clone(), owner.plugin.clone()))
            .or_default();
        let now = Instant::now();
        if total
            .samples
            .back()
            .is_none_or(|s| now.duration_since(s.0).as_millis() >= 250)
        {
            total.samples.push_back((now, total.sent, total.received));
        }
        while total
            .samples
            .front()
            .is_some_and(|s| now.duration_since(s.0).as_secs_f64() > 3.0)
        {
            total.samples.pop_front();
        }
        total.sent = total.sent.saturating_add(sent);
        total.received = total.received.saturating_add(received);
    }
    pub fn network_snapshot(&self) -> Value {
        let now = Instant::now();
        let entries = self.network_metrics.entries.lock().unwrap();
        let plugins:Vec<_>=entries.iter().map(|((server,plugin),t)|{
            let sample=t.samples.iter().find(|s|now.duration_since(s.0).as_secs_f64()<=3.0);
            let (up,down)=sample.map(|s|{let seconds=now.duration_since(s.0).as_secs_f64().max(0.25);((t.sent-s.1)as f64/seconds,(t.received-s.2)as f64/seconds)}).unwrap_or((0.0,0.0));
            json!({"serverId":server,"pluginId":plugin,"sent":t.sent.to_string(),"received":t.received.to_string(),"sentPerSecond":up,"receivedPerSecond":down,"quality":"payload"})
        }).collect();
        drop(entries);
        let mut physical = self.network_metrics.transport.lock().unwrap();
        let mut servers:Vec<_>=physical.iter_mut().map(|(server,t)|{
            let sent=t.retired.0.saturating_add(t.current.sent.load(Ordering::Relaxed));let received=t.retired.1.saturating_add(t.current.received.load(Ordering::Relaxed));
            let sample=t.samples.iter().find(|s|now.duration_since(s.0).as_secs_f64()<=3.0);
            let (up,down)=sample.map(|s|{let seconds=now.duration_since(s.0).as_secs_f64().max(0.25);((sent.saturating_sub(s.1))as f64/seconds,(received.saturating_sub(s.2))as f64/seconds)}).unwrap_or((0.0,0.0));
            while t.samples.front().is_some_and(|s|now.duration_since(s.0).as_secs_f64()>3.0){t.samples.pop_front();}
            if t.samples.back().is_none_or(|s|now.duration_since(s.0).as_millis()>=500){t.samples.push_back((now,sent,received));}
            json!({"serverId":server,"sent":sent.to_string(),"received":received.to_string(),"sentPerSecond":up,"receivedPerSecond":down,"quality":"socket"})
        }).collect();
        drop(physical);
        let catalog = self.server_catalog();
        for server in &mut servers {
            if let Some(name) = catalog["servers"]
                .as_array()
                .and_then(|s| s.iter().find(|s| s["id"] == server["serverId"]))
                .and_then(|s| s["name"].as_str())
            {
                server["name"] = json!(name);
            }
        }
        json!({"servers":servers,"plugins":plugins,"connections":self.connection_views(),"windowSeconds":3,"allocationQuality":"unavailable","note":"连接传输包含 SSH/SOCKS 封装；插件明细仅计算文件内容和 PTY 有效载荷。目录与协议流量保留为核心/未归属，不按 JSON 大小推算。"})
    }
}

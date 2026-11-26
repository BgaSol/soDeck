use crate::mounts::{Capacity, MountCatalog, MountScan};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub enum Read<T> {
    Value(T),
    Failed(String),
    Unsupported,
}
impl<T> From<Result<T, String>> for Read<T> {
    fn from(value: Result<T, String>) -> Self {
        match value {
            Ok(v) => Self::Value(v),
            Err(e) => Self::Failed(e),
        }
    }
}
#[derive(Clone, Debug)]
pub struct Observed<T> {
    pub value: Option<T>,
    pub observed_at: Option<u64>,
    pub attempted_at: Option<u64>,
    pub state: &'static str,
    pub reason: Option<String>,
}
impl<T> Default for Observed<T> {
    fn default() -> Self {
        Self {
            value: None,
            observed_at: None,
            attempted_at: None,
            state: "pending",
            reason: None,
        }
    }
}
impl<T> Observed<T> {
    pub fn apply(&mut self, read: Read<T>, at: u64) -> bool {
        self.attempted_at = Some(at);
        match read {
            Read::Value(value) => {
                self.value = Some(value);
                self.observed_at = Some(at);
                self.state = "ok";
                self.reason = None;
                true
            }
            Read::Failed(reason) => {
                self.state = "unavailable";
                self.reason = Some(reason.chars().take(256).collect());
                false
            }
            Read::Unsupported => {
                self.state = "unsupported";
                self.reason = None;
                false
            }
        }
    }
    pub fn meta(&self) -> Value {
        json!({"state":self.state,"observedAt":self.observed_at,"attemptedAt":self.attempted_at,"reason":self.reason})
    }
}
#[derive(Clone, Default, Debug, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Identity {
    pub system: String,
    pub hostname: String,
    pub cpu_model: String,
    pub cores: Option<u64>,
    pub architecture: Option<String>,
    #[serde(skip)]
    pub total: Option<u64>,
}
#[derive(Clone, Default, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub id: String,
    pub name: String,
    pub addresses: Vec<String>,
    pub state: Option<String>,
    pub counter_instance: bool,
}
#[derive(Clone, Debug)]
pub struct Memory {
    pub total: u64,
    pub available: u64,
    pub estimated: bool,
    pub source: String,
    pub details: String,
}
#[derive(Clone, Debug)]
pub struct Gauge {
    pub value: f64,
    pub source: String,
    pub window_ms: Option<f64>,
}
#[derive(Clone, Debug)]
pub enum Cpu {
    Direct(f64),
    Counter(u64, u64),
}
#[derive(Clone, Debug)]
pub struct Counter {
    pub device: Device,
    pub first: u64,
    pub second: u64,
    pub clock: Option<(u64, u64)>,
}
#[derive(Clone, Debug)]
pub struct Rates {
    pub first: f64,
    pub second: f64,
    pub estimated: bool,
}
#[derive(Clone, Debug)]
pub struct Uptime {
    pub seconds: f64,
    pub estimated: bool,
}
#[derive(Clone, Debug)]
pub enum Load {
    Values([f64; 3]),
    Text(String),
}
#[derive(Clone, Debug)]
pub struct Dynamic {
    pub cpu: Read<Cpu>,
    pub memory: Read<Memory>,
    pub network: Read<Vec<Counter>>,
    pub disks: Read<Vec<Counter>>,
    pub boot: String,
    pub remote_seconds: Option<f64>,
    pub uptime: Option<Uptime>,
    pub estimated: bool,
    pub swap: Option<(Option<u64>, Option<u64>)>,
}
#[derive(Clone, Debug)]
pub struct Topology {
    pub interfaces: Read<Vec<Device>>,
    pub mounts: Read<MountScan>,
    pub load: Read<Load>,
}
#[derive(Clone, Debug)]
pub struct CapacitySample {
    pub rows: Vec<Capacity>,
    pub failures: Vec<(String, String)>,
}
#[derive(Clone, Debug)]
pub enum Sample {
    Identity(Read<Identity>),
    Dynamic(Dynamic),
    Topology(Topology),
    Capacity(Read<CapacitySample>),
    Uptime(Read<Uptime>),
}

#[derive(Clone, Default)]
pub struct StatusState {
    pub identity: Observed<Identity>,
    pub cpu: Observed<Gauge>,
    pub memory: Observed<Memory>,
    pub interfaces: Observed<Vec<Device>>,
    pub counter_interfaces: Observed<Vec<Device>>,
    pub disks: Observed<Vec<Device>>,
    pub network_source: Observed<()>,
    pub disk_source: Observed<()>,
    pub rates: BTreeMap<String, Observed<Rates>>,
    pub mounts: MountCatalog,
    pub load: Observed<Load>,
    pub uptime: Observed<Uptime>,
    pub cache_limited: bool,
    pub collection_error: Option<String>,
    pub family: Option<crate::collectors::Family>,
    pub swap: Option<(Option<u64>, Option<u64>)>,
}
impl StatusState {
    pub fn data(&self) -> Value {
        let mut cpu = self.cpu.meta();
        if let Some(v) = &self.cpu.value {
            cpu["value"] = json!(v.value);
            cpu["source"] = json!(v.source);
            cpu["windowMs"] = json!(v.window_ms);
        }
        let mut memory = self.memory.meta();
        if let Some(v) = &self.memory.value {
            memory["value"] = json!((v.total - v.available) as f64 / v.total as f64 * 100.0);
            memory["total"] = json!(v.total.to_string());
            memory["available"] = json!(v.available.to_string());
            memory["estimated"] = json!(v.estimated);
            memory["source"] = json!(v.source);
        }
        let total = self
            .memory
            .value
            .as_ref()
            .map(|m| m.total)
            .or_else(|| self.identity.value.as_ref().and_then(|v| v.total));
        let (load, load_text) = match &self.load.value {
            Some(Load::Values(v)) => (json!(v), Value::Null),
            Some(Load::Text(v)) => (Value::Null, json!(v)),
            None => (Value::Null, Value::Null),
        };
        let mut uptime = self.uptime.meta();
        if let Some(v) = &self.uptime.value {
            uptime["seconds"] = json!(v.seconds);
            uptime["estimated"] = json!(v.estimated);
        }
        let mut errors = json!({"dynamic":{},"topology":{},"identity":{},"uptime":{}});
        for (group, name, meta) in [
            ("dynamic", "cpu", self.cpu.meta()),
            ("dynamic", "memory", self.memory.meta()),
            ("dynamic", "network", self.network_source.meta()),
            ("dynamic", "disk", self.disk_source.meta()),
            ("topology", "interfaces", self.interfaces.meta()),
            ("topology", "mounts", self.mounts.status.meta()),
            ("topology", "load", self.load.meta()),
            ("identity", "identity", self.identity.meta()),
            ("uptime", "uptime", self.uptime.meta()),
        ] {
            if meta["state"] == "unavailable" {
                errors[group][name] = meta["reason"].clone();
            }
        }
        json!({"identity":self.identity.value,"cpu":cpu,"memory":memory,"memoryTotal":total.map(|v|v.to_string()),"memoryDetails":self.memory.value.as_ref().map(|v|v.details.as_str()).unwrap_or(""),"load":load,"loadText":load_text,"uptime":uptime,"capabilities":self.family.map(crate::collectors::initial_capabilities),"sourceErrors":errors,"sources":{"interfaces":self.interfaces.meta(),"disks":self.disks.meta(),"network":self.network_source.meta(),"disk":self.disk_source.meta(),"load":self.load.meta()},"collectionError":self.collection_error,"cacheLimited":self.cache_limited,"swap":self.swap.map(|(a,b)|json!({"total":a.map(|v|v.to_string()),"available":b.map(|v|v.to_string())}))})
    }
    pub fn rate(&self, key: &str) -> Value {
        let Some(v) = self.rates.get(key) else {
            let source = if key.starts_with("net:") {
                &self.network_source
            } else {
                &self.disk_source
            };
            return if source.state == "ok" {
                json!({"state":"warmingUp"})
            } else {
                source.meta()
            };
        };
        let mut out = v.meta();
        if let Some(r) = &v.value {
            out["first"] = json!(r.first);
            out["second"] = json!(r.second);
            out["estimated"] = json!(r.estimated);
        }
        out
    }
    pub fn bytes(&self) -> usize {
        self.mounts.bytes()
            + serde_json::to_vec(&self.data()).map_or(0, |v| v.len() * 2)
            + self
                .interfaces
                .value
                .iter()
                .chain(self.disks.value.iter())
                .flatten()
                .map(|v| {
                    v.id.capacity()
                        + v.name.capacity()
                        + v.addresses.iter().map(String::capacity).sum::<usize>()
                        + 256
                })
                .sum::<usize>()
            + self
                .rates
                .iter()
                .map(|(k, v)| k.capacity() + v.reason.as_ref().map_or(0, String::capacity) + 256)
                .sum::<usize>()
    }
    #[cfg(test)]
    pub fn fixture(data: &Value) -> Self {
        let mut s = Self::default();
        s.mounts = MountCatalog::fixture(data);
        if let Some(v) = data["cpu"]["value"].as_f64() {
            s.cpu.apply(
                Read::Value(Gauge {
                    value: v,
                    source: String::new(),
                    window_ms: None,
                }),
                1,
            );
        }
        if let Ok(mut v) = serde_json::from_value::<Identity>(data["identity"].clone()) {
            v.total = data["memoryTotal"].as_str().and_then(|s| s.parse().ok());
            s.identity.apply(Read::Value(v), 1);
        }
        s
    }
}

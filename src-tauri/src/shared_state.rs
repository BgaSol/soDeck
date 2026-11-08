//! In-memory shared facts. Admission and delivery belong to the component host.
use crate::workspaces::Context;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    sync::{mpsc, Arc, Mutex},
    time::Instant,
};
pub(crate) mod delivery;
mod host;

pub mod limits {
    use std::time::Duration;
    pub const RECORD_BYTES: usize = 16 * 1024;
    pub const PROCESS_FRAME_BYTES: usize = 32 * 1024;
    pub const SURFACE_FRAME_BYTES: usize = 24 * 1024;
    pub const SCOPE_BYTES: usize = 4 * 1024 * 1024;
    pub const RETAINED_BYTES: usize = 32 * 1024 * 1024;
    pub const TRANSIENT_BYTES: usize = 32 * 1024 * 1024;
    pub const SUBSCRIPTION_BYTES: usize = 1024 * 1024;
    pub const RECORDS: usize = 4096;
    pub const INSTANCE_SUBSCRIPTIONS: usize = 32;
    pub const LEASE: Duration = Duration::from_secs(60);
    pub const SNAPSHOT_IDLE: Duration = Duration::from_secs(30);
    // Reserve space for the RPC/iframe routing envelope and bounded identities.
    pub const ENVELOPE_BYTES: usize = 1024;
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Producer {
    Core,
    Plugin {
        #[serde(rename = "pluginId")]
        plugin_id: String,
        #[serde(rename = "pluginInstanceId")]
        plugin_instance_id: String,
        #[serde(rename = "viewId")]
        view_id: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeliveryTarget {
    Process {
        instance_id: String,
        view_id: String,
    },
    Surface {
        window_id: String,
        surface_id: String,
    },
}
impl DeliveryTarget {
    fn frame_bytes(&self) -> usize {
        match self {
            Self::Process { .. } => limits::PROCESS_FRAME_BYTES,
            Self::Surface { .. } => limits::SURFACE_FRAME_BYTES,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Owner {
    pub context: Context,
    pub producer: Producer,
    pub target: DeliveryTarget,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Filter {
    pub topics: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub producer: Option<Producer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
}
impl Filter {
    fn validate(&self) -> Result<(), Error> {
        if self.topics.is_empty() || self.topics.len() > 8 {
            return Err(Error::invalid("过滤器需要 1–8 个明确主题"));
        }
        for topic in &self.topics {
            validate_topic(topic)?;
        }
        if let Some(key) = &self.key {
            validate_key(key)?;
        }
        if let Some(producer) = &self.producer {
            validate_producer(producer)?;
        }
        Ok(())
    }
    fn matches(&self, key: &RecordKey) -> bool {
        self.topics.contains(&key.topic)
            && self.producer.as_ref().is_none_or(|p| p == &key.producer)
            && self.key.as_ref().is_none_or(|k| k == &key.key)
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Error {
    pub code: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}
impl Error {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }
    fn invalid(message: impl Into<String>) -> Self {
        Self::new("invalidParams", message)
    }
    fn limit() -> Self {
        Self::new("resourceLimit", "共享状态资源额度不足")
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for Error {}

pub fn parse_revision(value: &Value) -> Result<u64, Error> {
    let text = value
        .as_str()
        .ok_or_else(|| Error::invalid("修订号必须是十进制字符串"))?;
    if text.is_empty()
        || (text.len() > 1 && text.starts_with('0'))
        || !text.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(Error::invalid("修订号格式无效"));
    }
    text.parse()
        .map_err(|_| Error::invalid("修订号超出 u64 范围"))
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct RecordKey {
    topic: String,
    producer: Producer,
    key: String,
}
struct Record {
    key: RecordKey,
    revision: u64,
    received_at: u64,
    value: Option<Value>,
    bytes: usize,
}
impl Record {
    fn wire(&self) -> Value {
        let mut value = json!({"producer":self.key.producer,"topic":self.key.topic,"key":self.key.key,
            "revision":self.revision.to_string(),"present":self.value.is_some(),"receivedAt":self.received_at});
        if let Some(data) = &self.value {
            value["value"] = data.clone();
        }
        value
    }
}

struct Scope {
    context: Context,
    connected: bool,
    frozen: bool,
    revision: u64,
    records: BTreeMap<RecordKey, Arc<Record>>,
    bytes: usize,
}
impl Scope {
    fn wire(&self) -> Value {
        json!({"context":self.context,"connected":self.connected,"frozen":self.frozen,
            "state":if !self.connected {"disconnected"} else if self.frozen {"frozen"} else {"active"}})
    }
    fn next_revision(&self) -> Result<u64, Error> {
        self.revision.checked_add(1).ok_or_else(Error::limit)
    }
}

#[derive(Clone)]
enum Change {
    Record(Arc<Record>, &'static str),
    Scope(Value),
}
impl Change {
    fn wire(&self) -> Value {
        match self {
            Self::Record(record, reason) if record.value.is_none() => {
                json!({"op":"remove","record":record.wire(),"reason":reason})
            }
            Self::Record(record, _) => json!({"op":"put","record":record.wire()}),
            Self::Scope(scope) => json!({"op":"scope","scope":scope}),
        }
    }
    fn bytes(&self) -> usize {
        match self {
            Self::Record(record, _) => record.bytes + 128,
            Self::Scope(scope) => encoded_size(scope) + 128,
        }
    }
}

struct Snapshot {
    id: String,
    owner: Owner,
    scope: Value,
    revision: u64,
    records: Vec<Arc<Record>>,
    // Each cursor identifies a fixed page, including the last page.
    starts: Vec<usize>,
    cursors: Vec<String>,
    served: HashSet<usize>,
    deadline: Instant,
    subscription: Option<String>,
    bytes: usize,
}
impl Snapshot {
    fn page(&self, page: usize, subscription: Option<&str>) -> Value {
        let end = self
            .starts
            .get(page + 1)
            .copied()
            .unwrap_or(self.records.len());
        let records: Vec<_> = self.records[self.starts[page]..end]
            .iter()
            .map(|r| r.wire())
            .collect();
        let mut result = json!({"snapshotId":self.id,"scope":self.scope,"revision":self.revision.to_string(),
            "records":records,"nextCursor":self.cursors.get(page + 1)});
        if let Some(id) = subscription {
            result["subscriptionId"] = json!(id);
            result["leaseMs"] = json!(limits::LEASE.as_millis() as u64);
        }
        result
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Initializing,
    Live,
    ResetRequired,
}
struct Batch {
    id: String,
    to_revision: u64,
    frames: Vec<Value>,
    part: usize,
    bytes: usize,
    // Keep all referenced versions charged until the final ACK.
    _changes: Vec<Change>,
}
struct Subscription {
    id: String,
    owner: Owner,
    client_id: String,
    filter: Filter,
    phase: Phase,
    confirmed_revision: u64,
    frontier: u64,
    deadline: Instant,
    snapshot: Option<String>,
    pending: BTreeMap<RecordKey, Change>,
    pending_scope: Option<Change>,
    batch: Option<Batch>,
    last_ack: Option<(String, usize, u64)>,
    metadata_bytes: usize,
}
impl Subscription {
    fn pending_bytes(&self) -> usize {
        self.pending.values().map(Change::bytes).sum::<usize>()
            + self.pending_scope.as_ref().map_or(0, Change::bytes)
    }
    fn bytes(&self) -> usize {
        self.metadata_bytes + self.pending_bytes() + self.batch.as_ref().map_or(0, |b| b.bytes)
    }
    fn reset(&mut self) {
        self.phase = Phase::ResetRequired;
        self.pending.clear();
        self.pending_scope = None;
        self.batch = None;
        self.last_ack = None;
        self.snapshot = None;
    }
}
struct RetiredSubscription {
    owner: Owner,
    client_id: String,
    code: &'static str,
    bytes: usize,
}

#[derive(Default)]
struct Store {
    scopes: HashMap<String, Scope>,
    instances: HashSet<(String, String)>,
    sources: BTreeMap<Producer, Context>,
    snapshots: HashMap<String, Snapshot>,
    subscriptions: BTreeMap<String, Subscription>,
    // Small identity tombstones prevent an expired initial retry creating a new snapshot.
    retired: Vec<RetiredSubscription>,
    sequence: u64,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Usage {
    pub scopes: usize,
    pub records: usize,
    pub snapshots: usize,
    pub subscriptions: usize,
    pub retired_subscription_ids: usize,
    pub retained_bytes: usize,
    pub transient_bytes: usize,
}

#[derive(Clone, Debug)]
pub struct Delivery {
    pub subscription_id: String,
    pub target: DeliveryTarget,
    pub frame: Value,
}

pub struct StateHub {
    inner: Mutex<Store>,
    wake: mpsc::SyncSender<()>,
}
impl StateHub {
    pub fn new() -> (Self, mpsc::Receiver<()>) {
        let (wake, receiver) = mpsc::sync_channel(1);
        (
            Self {
                inner: Mutex::new(Store::default()),
                wake,
            },
            receiver,
        )
    }
    fn update<T>(
        &self,
        now: Instant,
        operation: impl FnOnce(&mut Store) -> Result<T, Error>,
    ) -> Result<T, Error> {
        let result = {
            let mut store = self.inner.lock().unwrap();
            store.expire(now);
            operation(&mut store)
        };
        let _ = self.wake.try_send(());
        result
    }
    pub fn usage(&self) -> Usage {
        self.inner.lock().unwrap().usage()
    }
    pub fn expire(&self, now: Instant) {
        self.inner.lock().unwrap().expire(now);
    }

    /// Called only at the connection admission commit, never on a state request.
    pub fn adopt_scope(&self, context: Context, now: Instant) -> Result<(), Error> {
        validate_context(&context)?;
        self.update(now, |store| {
            if store
                .scopes
                .get(&context.workspace_id)
                .is_some_and(|s| s.context == context)
            {
                return Ok(());
            }
            let bytes = encoded_size(&json!({"context":context})) + 2304;
            let old = store
                .scopes
                .get(&context.workspace_id)
                .map_or(0, |s| s.bytes);
            if store.usage().retained_bytes - old + bytes > limits::RETAINED_BYTES {
                return Err(Error::limit());
            }
            store.remove_scope(&context.workspace_id);
            store.scopes.insert(
                context.workspace_id.clone(),
                Scope {
                    context,
                    connected: true,
                    frozen: false,
                    revision: 0,
                    records: BTreeMap::new(),
                    bytes,
                },
            );
            Ok(())
        })
    }
    pub fn remove_scope(&self, workspace_id: &str, now: Instant) {
        let _ = self.update(now, |store| {
            store.remove_scope(workspace_id);
            Ok(())
        });
    }
    pub fn activate_instance(&self, plugin_id: &str, instance_id: &str) -> Result<(), Error> {
        validate_identifier(plugin_id)?;
        validate_identifier(instance_id)?;
        let mut store = self.inner.lock().unwrap();
        let key = (plugin_id.into(), instance_id.into());
        if !store.instances.contains(&key)
            && store.usage().retained_bytes + plugin_id.len() + instance_id.len() + 256
                > limits::RETAINED_BYTES
        {
            return Err(Error::limit());
        }
        store.instances.insert(key);
        Ok(())
    }
    /// Registered by the logical-view lifecycle; requests cannot register producers.
    pub fn activate_view(&self, producer: Producer, context: &Context) -> Result<(), Error> {
        validate_producer(&producer)?;
        let mut store = self.inner.lock().unwrap();
        store.scope(context)?;
        store.instance(&producer)?;
        if producer == Producer::Core {
            return Err(Error::invalid("核心来源没有插件 view"));
        }
        if let Some(existing) = store.sources.get(&producer) {
            return if existing == context {
                Ok(())
            } else {
                Err(Error::new("staleContext", "view 已绑定另一个 Context"))
            };
        }
        if store.usage().retained_bytes + source_bytes(&producer, context) > limits::RETAINED_BYTES
        {
            return Err(Error::limit());
        }
        store.sources.insert(producer, context.clone());
        Ok(())
    }
    pub fn stop_instance(
        &self,
        plugin_id: &str,
        instance_id: &str,
        received_at: u64,
        now: Instant,
    ) {
        let _ = self.update(now, |store| {
            store.instances.remove(&(plugin_id.into(), instance_id.into()));
            let producers: Vec<_> = store.sources.keys().filter(|p| matches!(p,
                Producer::Plugin { plugin_id: p, plugin_instance_id: i, .. } if p == plugin_id && i == instance_id)).cloned().collect();
            for producer in producers { store.remove_source(&producer, "sourceStopped", received_at); }
            Ok(())
        });
    }
    pub fn remove_view(&self, producer: &Producer, received_at: u64, now: Instant) {
        let _ = self.update(now, |store| {
            store.remove_source(producer, "sourceStopped", received_at);
            Ok(())
        });
    }
    pub fn remove_surface(&self, window_id: &str, surface_id: &str, now: Instant) {
        let _ = self.update(now, |store| {
            store.remove_owner(|owner| matches!(&owner.target, DeliveryTarget::Surface { window_id: w, surface_id: s } if w == window_id && s == surface_id));
            Ok(())
        });
    }
    pub fn set_scope_status(
        &self,
        context: &Context,
        connected: Option<bool>,
        frozen: Option<bool>,
        now: Instant,
    ) -> Result<(), Error> {
        self.update(now, |store| {
            let scope = store.scope_mut(context)?;
            let next_connected = connected.unwrap_or(scope.connected);
            let next_frozen = frozen.unwrap_or(scope.frozen);
            if (next_connected, next_frozen) == (scope.connected, scope.frozen) {
                return Ok(());
            }
            scope.revision = scope.next_revision()?;
            scope.connected = next_connected;
            scope.frozen = next_frozen;
            let revision = scope.revision;
            let mut wire = scope.wire();
            wire["reason"] = json!(if !next_connected {
                "disconnected"
            } else if next_frozen {
                "workspaceClosing"
            } else {
                "resumed"
            });
            store.publish(context, revision, None, Change::Scope(wire));
            Ok(())
        })
    }

    pub fn get(
        &self,
        owner: &Owner,
        topic: &str,
        key: &str,
        producer: Option<&Producer>,
        now: Instant,
    ) -> Result<Value, Error> {
        validate_topic(topic)?;
        validate_key(key)?;
        if let Some(producer) = producer {
            validate_producer(producer)?;
        }
        let mut store = self.inner.lock().unwrap();
        store.expire(now);
        store.read_admission(owner)?;
        let scope = store.scope(&owner.context)?;
        let identity = RecordKey {
            topic: topic.into(),
            key: key.into(),
            producer: producer.unwrap_or(&owner.producer).clone(),
        };
        let record = scope.records.get(&identity).map(|r| r.wire()).unwrap_or_else(||
            json!({"producer":identity.producer,"topic":topic,"key":key,"revision":"0","present":false}));
        Ok(json!({"scope":scope.wire(),"record":record}))
    }
    pub fn put(
        &self,
        owner: &Owner,
        topic: &str,
        key: &str,
        expected_revision: u64,
        value: Value,
        received_at: u64,
        now: Instant,
    ) -> Result<Value, Error> {
        self.write(
            owner,
            topic,
            key,
            expected_revision,
            Some(value),
            received_at,
            now,
        )
    }
    pub fn remove(
        &self,
        owner: &Owner,
        topic: &str,
        key: &str,
        expected_revision: u64,
        received_at: u64,
        now: Instant,
    ) -> Result<Value, Error> {
        self.write(owner, topic, key, expected_revision, None, received_at, now)
    }
    fn write(
        &self,
        owner: &Owner,
        topic: &str,
        key: &str,
        expected_revision: u64,
        value: Option<Value>,
        received_at: u64,
        now: Instant,
    ) -> Result<Value, Error> {
        validate_topic(topic)?;
        validate_key(key)?;
        if let Some(value) = &value {
            validate_value(value, 0)?;
        }
        self.update(now, |store| {
            store.read_admission(owner)?;
            let scope = store.scope(&owner.context)?;
            if scope.frozen {
                return Err(Error::new("workspaceClosing", "工作区正在关闭"));
            }
            if !scope.connected {
                return Err(Error::new("staleContext", "连接已断开"));
            }
            let identity = RecordKey {
                topic: topic.into(),
                key: key.into(),
                producer: owner.producer.clone(),
            };
            let previous = scope.records.get(&identity);
            let revision = previous.map_or(0, |r| r.revision);
            if revision != expected_revision {
                return Err(Error {
                    code: "revisionConflict",
                    message: "记录已改变".into(),
                    data: Some(json!({"revision":revision.to_string()})),
                });
            }
            if previous.and_then(|r| r.value.as_ref()) == value.as_ref() {
                return Ok(json!({"revision":revision.to_string(),"changed":false}));
            }
            let mut record = Record {
                key: identity.clone(),
                revision: scope.next_revision()?,
                received_at,
                value,
                bytes: 0,
            };
            record.bytes = encoded_size(&record.wire()) + 128;
            if encoded_size(&record.wire()) > limits::RECORD_BYTES {
                return Err(Error::limit());
            }
            let old_bytes = previous.map_or(0, |r| r.bytes);
            if (previous.is_none() && scope.records.len() >= limits::RECORDS)
                || scope.bytes - old_bytes + record.bytes > limits::SCOPE_BYTES
                || store.usage().retained_bytes - old_bytes + record.bytes > limits::RETAINED_BYTES
            {
                return Err(Error::limit());
            }
            let record = Arc::new(record);
            let scope = store.scope_mut(&owner.context)?;
            scope.bytes = scope.bytes - old_bytes + record.bytes;
            scope.revision = record.revision;
            scope.records.insert(identity.clone(), record.clone());
            store.publish(
                &owner.context,
                record.revision,
                Some(identity),
                Change::Record(record.clone(), "removed"),
            );
            Ok(json!({"revision":record.revision.to_string(),"changed":true}))
        })
    }

    pub fn list(&self, owner: &Owner, filter: Filter, now: Instant) -> Result<Value, Error> {
        filter.validate()?;
        self.update(now, |store| {
            store.read_admission(owner)?;
            let snapshot = store.make_snapshot(owner, &filter, None, now)?;
            let result = snapshot.page(0, None);
            store.snapshots.insert(snapshot.id.clone(), snapshot);
            Ok(result)
        })
    }
    pub fn page(
        &self,
        owner: &Owner,
        cursor: &str,
        release: bool,
        now: Instant,
    ) -> Result<Value, Error> {
        validate_identifier(cursor)?;
        self.update(now, |store| {
            store.read_admission(owner)?;
            let found = store.snapshots.values().find_map(|s| {
                s.cursors
                    .iter()
                    .position(|c| c == cursor)
                    .map(|p| (s.id.clone(), p))
            });
            let Some((id, page)) = found else {
                return if release {
                    Ok(json!({"released":true}))
                } else {
                    Err(Error::new("snapshotExpired", "快照已失效"))
                };
            };
            let snapshot = store.snapshots.get(&id).unwrap();
            if &snapshot.owner != owner {
                return Err(Error::invalid("游标不属于此调用来源"));
            }
            if release {
                if let Some(subscription) = snapshot.subscription.clone() {
                    store.reset_subscription(&subscription);
                }
                store.snapshots.remove(&id);
                return Ok(json!({"released":true}));
            }
            let snapshot = store.snapshots.get_mut(&id).unwrap();
            snapshot.deadline = now + limits::SNAPSHOT_IDLE;
            snapshot.served.insert(page);
            Ok(snapshot.page(page, None))
        })
    }
    pub fn subscribe(
        &self,
        owner: &Owner,
        client_id: &str,
        filter: Filter,
        acknowledged_revision: Option<u64>,
        now: Instant,
    ) -> Result<Value, Error> {
        validate_identifier(client_id)?;
        filter.validate()?;
        self.update(now, |store| {
            store.read_admission(owner)?;
            if let Some(retired) = store.retired.iter().find(|s| s.owner == *owner && s.client_id == client_id) {
                return Err(Error::new(retired.code, "订阅已结束，请使用新的客户端订阅 ID"));
            }
            if let Some(id) = store.subscriptions.values().find(|s| s.owner == *owner && s.client_id == client_id).map(|s| s.id.clone()) {
                let sub = store.subscriptions.get(&id).unwrap();
                if sub.filter != filter { return Err(Error::invalid("重试订阅不能更改过滤器")); }
                if sub.phase == Phase::ResetRequired { return Err(Error::new("resetRequired", "订阅需要重新同步")); }
                match acknowledged_revision {
                    None if sub.phase == Phase::Live => return Err(Error::new("subscriptionInitialized", "初始快照已确认")),
                    None => {
                        let snapshot_id = sub.snapshot.as_ref().unwrap().clone();
                        let snapshot = store.snapshots.get_mut(&snapshot_id).ok_or_else(|| Error::new("snapshotExpired", "快照已失效"))?;
                        snapshot.deadline = now + limits::SNAPSHOT_IDLE;
                        let result = snapshot.page(0, Some(&id));
                        store.subscriptions.get_mut(&id).unwrap().deadline = now + limits::LEASE;
                        return Ok(result);
                    }
                    Some(revision) if sub.phase == Phase::Initializing => {
                        let snapshot_id = sub.snapshot.as_ref().unwrap().clone();
                        let snapshot = store.snapshots.get(&snapshot_id).unwrap();
                        if revision != snapshot.revision || snapshot.served.len() != snapshot.starts.len() {
                            return Err(Error::invalid("初始快照尚未完整读取或修订号不符"));
                        }
                        store.snapshots.remove(&snapshot_id);
                        let sub = store.subscriptions.get_mut(&id).unwrap();
                        sub.phase = Phase::Live;
                        sub.snapshot = None;
                    }
                    Some(revision) if revision != sub.confirmed_revision => return Err(Error::new("revisionConflict", "续订游标与确认游标不符")),
                    Some(_) => {}
                }
                let sub = store.subscriptions.get_mut(&id).unwrap();
                sub.deadline = now + limits::LEASE;
                return Ok(json!({"subscriptionId":id,"confirmedRevision":sub.confirmed_revision.to_string(),"leaseMs":limits::LEASE.as_millis() as u64}));
            }
            if acknowledged_revision.is_some() { return Err(Error::new("subscriptionExpired", "订阅不存在或已过期")); }
            if store.scope(&owner.context)?.frozen { return Err(Error::new("workspaceClosing", "关闭准备后不能新建订阅")); }
            if store.subscriptions.values().filter(|s| same_instance(&s.owner.producer, &owner.producer)).count() >= limits::INSTANCE_SUBSCRIPTIONS { return Err(Error::limit()); }
            let id = store.id("sub")?;
            let snapshot = store.make_snapshot(owner, &filter, Some(id.clone()), now)?;
            let metadata_bytes = owner_bytes(owner) + client_id.len() + encoded_size(&json!(filter)) + 512;
            if store.usage().transient_bytes + snapshot.bytes + metadata_bytes > limits::TRANSIENT_BYTES { return Err(Error::limit()); }
            let result = snapshot.page(0, Some(&id));
            store.subscriptions.insert(id.clone(), Subscription {
                id, owner: owner.clone(), client_id: client_id.into(), filter, phase: Phase::Initializing,
                confirmed_revision: snapshot.revision, frontier: snapshot.revision, deadline: now + limits::LEASE,
                snapshot: Some(snapshot.id.clone()), pending: BTreeMap::new(), pending_scope: None, batch: None, last_ack: None, metadata_bytes,
            });
            store.snapshots.insert(snapshot.id.clone(), snapshot);
            Ok(result)
        })
    }
    pub fn unsubscribe(
        &self,
        owner: &Owner,
        subscription_id: Option<&str>,
        client_id: Option<&str>,
        now: Instant,
    ) -> Result<(), Error> {
        if subscription_id.is_none() && client_id.is_none() {
            return Err(Error::invalid("缺少订阅 ID"));
        }
        if let Some(id) = subscription_id {
            validate_identifier(id)?;
        }
        if let Some(id) = client_id {
            validate_identifier(id)?;
        }
        self.update(now, |store| {
            store.read_admission(owner)?;
            let ids: Vec<_> = store
                .subscriptions
                .values()
                .filter(|s| {
                    s.owner == *owner
                        && subscription_id.is_none_or(|id| id == s.id)
                        && client_id.is_none_or(|id| id == s.client_id)
                })
                .map(|s| s.id.clone())
                .collect();
            if let Some(id) = subscription_id {
                if store
                    .subscriptions
                    .get(id)
                    .is_some_and(|s| s.owner != *owner)
                {
                    return Err(Error::invalid("订阅不属于此调用来源"));
                }
            }
            if ids.is_empty() {
                if let Some(client_id) = client_id {
                    if !store
                        .retired
                        .iter()
                        .any(|r| r.owner == *owner && r.client_id == client_id)
                    {
                        let bytes = owner_bytes(owner) + client_id.len() + 128;
                        if store.usage().transient_bytes + bytes > limits::TRANSIENT_BYTES {
                            return Err(Error::limit());
                        }
                        store.retired.push(RetiredSubscription {
                            owner: owner.clone(),
                            client_id: client_id.into(),
                            code: "subscriptionExpired",
                            bytes,
                        });
                    }
                }
            }
            for id in ids {
                store.retire(&id, "subscriptionExpired");
            }
            Ok(())
        })
    }
    /// Returns the current part again until acknowledged; sending it is not an ACK.
    pub fn deliveries(&self, now: Instant) -> Vec<Delivery> {
        let mut store = self.inner.lock().unwrap();
        store.expire(now);
        let ids: Vec<_> = store.subscriptions.keys().cloned().collect();
        let mut deliveries = Vec::new();
        for id in ids {
            let sub = store.subscriptions.get(&id).unwrap();
            if sub.phase == Phase::Initializing {
                continue;
            }
            if sub.phase == Phase::Live
                && sub.batch.is_none()
                && (!sub.pending.is_empty() || sub.pending_scope.is_some())
            {
                if store.make_batch(&id).is_err() {
                    store.reset_subscription(&id);
                }
            }
            let sub = store.subscriptions.get(&id).unwrap();
            let frame = if sub.phase == Phase::ResetRequired {
                json!({"subscriptionId":id,"clientSubscriptionId":sub.client_id,"context":sub.owner.context,"resetRequired":true})
            } else if let Some(batch) = &sub.batch {
                batch.frames[batch.part].clone()
            } else {
                continue;
            };
            deliveries.push(Delivery {
                subscription_id: id,
                target: sub.owner.target.clone(),
                frame,
            });
        }
        deliveries
    }
    pub fn acknowledge(
        &self,
        owner: &Owner,
        subscription_id: &str,
        batch_id: &str,
        part: usize,
        applied_revision: Option<u64>,
        now: Instant,
    ) -> Result<Value, Error> {
        validate_identifier(subscription_id)?;
        validate_identifier(batch_id)?;
        self.update(now, |store| {
            store.read_admission(owner)?;
            let sub = store
                .subscriptions
                .get_mut(subscription_id)
                .ok_or_else(|| Error::new("subscriptionExpired", "订阅已过期"))?;
            if sub.owner != *owner {
                return Err(Error::invalid("订阅不属于此调用来源"));
            }
            if sub.phase == Phase::ResetRequired {
                return Err(Error::new("resetRequired", "订阅需要重新同步"));
            }
            if sub
                .last_ack
                .as_ref()
                .is_some_and(|(id, last_part, revision)| {
                    id == batch_id && *last_part == part && Some(*revision) == applied_revision
                })
            {
                return Ok(
                    json!({"accepted":true,"confirmedRevision":sub.confirmed_revision.to_string()}),
                );
            }
            let Some(batch) = &mut sub.batch else {
                return Err(Error::invalid("没有待确认的批次"));
            };
            if batch.id != batch_id || part > batch.part || part >= batch.frames.len() {
                return Err(Error::invalid("批次或帧顺序不符"));
            }
            let final_part = part + 1 == batch.frames.len();
            if (final_part && applied_revision != Some(batch.to_revision))
                || (!final_part && applied_revision.is_some())
            {
                return Err(Error::invalid("只能在收齐批次后确认最终修订号"));
            }
            if part == batch.part {
                if final_part {
                    sub.confirmed_revision = batch.to_revision;
                    sub.last_ack = Some((batch.id.clone(), part, batch.to_revision));
                    sub.batch = None;
                } else {
                    batch.part += 1;
                }
            }
            Ok(json!({"accepted":true,"confirmedRevision":sub.confirmed_revision.to_string()}))
        })
    }
    pub fn clear(&self) {
        let mut store = self.inner.lock().unwrap();
        let sequence = store.sequence;
        *store = Store {
            sequence,
            ..Store::default()
        };
    }

    pub(crate) fn set_terminal_runtime(
        &self,
        context: &Context,
        id: &str,
        value: Value,
        now: Instant,
    ) -> Result<(), Error> {
        if encoded_size(&value) > 4 * 1024 {
            return Err(Error::limit());
        }
        self.update(now, |store| {
            let key = RecordKey {
                topic: "terminal.runtime.v1".into(),
                producer: Producer::Core,
                key: id.into(),
            };
            let scope = store.scope(context)?;
            let previous = scope.records.get(&key);
            if previous.and_then(|r| r.value.as_ref()) == Some(&value) {
                return Ok(());
            }
            let old = previous.map_or(0, |r| r.bytes);
            let mut record = Record {
                key: key.clone(),
                revision: scope.next_revision()?,
                received_at: crate::core::now(),
                value: Some(value),
                bytes: 0,
            };
            record.bytes = encoded_size(&record.wire()) + 128;
            if (previous.is_none() && scope.records.len() >= limits::RECORDS)
                || scope.bytes - old + record.bytes > limits::SCOPE_BYTES
                || store.usage().retained_bytes - old + record.bytes > limits::RETAINED_BYTES
            {
                return Err(Error::limit());
            }
            let record = Arc::new(record);
            let scope = store.scope_mut(context)?;
            scope.bytes = scope.bytes - old + record.bytes;
            scope.revision = record.revision;
            scope.records.insert(key.clone(), record.clone());
            store.publish(
                context,
                record.revision,
                Some(key),
                Change::Record(record, "updated"),
            );
            Ok(())
        })
    }

    pub(crate) fn sync_ui(
        &self,
        ui: &crate::component_ui::Runtime,
        now: Instant,
    ) -> Result<(), Error> {
        let mut store = self.inner.lock().unwrap();
        store.expire(now);
        let operation = |store: &mut Store| {
            let live: BTreeMap<_, _> = ui
                .workspaces
                .values()
                .flat_map(|w| &w.views)
                .map(|v| {
                    (
                        Producer::Plugin {
                            plugin_id: v.plugin_id.clone(),
                            plugin_instance_id: v.plugin_instance_id.clone(),
                            view_id: v.id.clone(),
                        },
                        v.context.clone(),
                    )
                })
                .collect();
            let gone: Vec<_> = store
                .sources
                .iter()
                .filter(|(p, c)| live.get(*p) != Some(*c))
                .map(|(p, _)| p.clone())
                .collect();
            let gone_runtime: Vec<_> = store
                .scopes
                .values()
                .flat_map(|scope| {
                    scope.records.iter().filter_map(|(key, record)| {
                        if key.topic != "terminal.runtime.v1" || key.producer != Producer::Core {
                            return None;
                        }
                        let value = record.value.as_ref()?;
                        let producer = Producer::Plugin {
                            plugin_id: value["pluginId"].as_str().unwrap_or("").into(),
                            plugin_instance_id: value["pluginInstanceId"]
                                .as_str()
                                .unwrap_or("")
                                .into(),
                            view_id: value["viewId"].as_str().unwrap_or("").into(),
                        };
                        (live.get(&producer) != Some(&scope.context))
                            .then(|| (scope.context.clone(), key.clone()))
                    })
                })
                .collect();
            for (context, key) in gone_runtime {
                let scope = store.scope_mut(&context)?;
                let record = scope.records.remove(&key).unwrap();
                scope.bytes -= record.bytes;
                let revision = scope.next_revision()?;
                scope.revision = revision;
                let removed = Arc::new(Record {
                    key: key.clone(),
                    revision,
                    received_at: crate::core::now(),
                    value: None,
                    bytes: 0,
                });
                store.publish(
                    &context,
                    revision,
                    Some(key),
                    Change::Record(removed, "sourceStopped"),
                );
            }
            for producer in gone {
                store.remove_source(&producer, "sourceStopped", crate::core::now());
            }
            for (producer, context) in live {
                if store.instance(&producer).is_ok()
                    && store.scope(&context).is_ok()
                    && !store.sources.contains_key(&producer)
                {
                    if store.usage().retained_bytes + source_bytes(&producer, &context)
                        > limits::RETAINED_BYTES
                    {
                        return Err(Error::limit());
                    }
                    store.sources.insert(producer, context);
                }
            }
            store.remove_owner(|owner| match &owner.target {
                DeliveryTarget::Surface {
                    window_id,
                    surface_id,
                } => !ui.surfaces.get(surface_id).is_some_and(|s| {
                    s.window == *window_id
                        && match &owner.producer {
                            Producer::Plugin { view_id, .. } => s.view == *view_id,
                            _ => false,
                        }
                }),
                _ => false,
            });
            let contexts: Vec<_> = store.scopes.values().map(|s| s.context.clone()).collect();
            for context in contexts {
                let scope = store.scope_mut(&context)?;
                let frozen = ui.frozen(&context.workspace_id);
                if scope.frozen != frozen {
                    scope.revision = scope.next_revision()?;
                    scope.frozen = frozen;
                    let revision = scope.revision;
                    let mut status = scope.wire();
                    status["reason"] = json!(if frozen {
                        "workspaceClosing"
                    } else {
                        "resumed"
                    });
                    store.publish(&context, revision, None, Change::Scope(status));
                }
                let active = ui
                    .workspaces
                    .get(&context.workspace_id)
                    .and_then(|w| w.editor.clone());
                let plugin = active
                    .as_ref()
                    .and_then(|id| ui.resource_view(id).ok())
                    .map(|v| v.plugin_id.clone());
                let value = json!({"activeEditorViewId":active,"activeEditorPluginId":plugin});
                let key = RecordKey {
                    topic: "workspace.presentation.v1".into(),
                    producer: Producer::Core,
                    key: "current".into(),
                };
                let scope = store.scope_mut(&context)?;
                if scope.records.get(&key).and_then(|r| r.value.as_ref()) == Some(&value) {
                    continue;
                }
                let revision = scope.next_revision()?;
                let mut record = Record {
                    key: key.clone(),
                    revision,
                    received_at: crate::core::now(),
                    value: Some(value),
                    bytes: 0,
                };
                record.bytes = encoded_size(&record.wire()) + 128;
                // The scope reserves 2 KiB for this bounded host fact before business writes.
                scope.revision = revision;
                let record = Arc::new(record);
                scope.records.insert(key.clone(), record.clone());
                store.publish(
                    &context,
                    revision,
                    Some(key),
                    Change::Record(record, "updated"),
                );
            }
            Ok(())
        };
        operation(&mut store)
    }

    pub(crate) fn reset_delivery(&self, id: &str, now: Instant) {
        let _ = self.update(now, |s| {
            s.reset_subscription(id);
            Ok(())
        });
    }
}

impl Store {
    fn id(&mut self, prefix: &str) -> Result<String, Error> {
        self.sequence = self.sequence.checked_add(1).ok_or_else(Error::limit)?;
        Ok(format!("{prefix}-{}", self.sequence))
    }
    fn scope(&self, context: &Context) -> Result<&Scope, Error> {
        let scope = self
            .scopes
            .get(&context.workspace_id)
            .ok_or_else(|| Error::new("scopeClosed", "工作区范围已关闭"))?;
        if &scope.context != context {
            return Err(Error::new("staleContext", "工作区连接 Context 已改变"));
        }
        Ok(scope)
    }
    fn scope_mut(&mut self, context: &Context) -> Result<&mut Scope, Error> {
        self.scope(context)?;
        Ok(self.scopes.get_mut(&context.workspace_id).unwrap())
    }
    fn instance(&self, producer: &Producer) -> Result<(), Error> {
        if let Producer::Plugin {
            plugin_id,
            plugin_instance_id,
            ..
        } = producer
        {
            if !self
                .instances
                .contains(&(plugin_id.clone(), plugin_instance_id.clone()))
            {
                return Err(Error::new("sourceStopped", "插件实例已停止"));
            }
        }
        Ok(())
    }
    fn read_admission(&self, owner: &Owner) -> Result<(), Error> {
        validate_producer(&owner.producer)?;
        validate_target(owner)?;
        self.scope(&owner.context)?;
        self.instance(&owner.producer)?;
        if owner.producer != Producer::Core
            && self.sources.get(&owner.producer) != Some(&owner.context)
        {
            return Err(Error::new("sourceStopped", "逻辑 view 未接纳或已释放"));
        }
        Ok(())
    }
    fn usage(&self) -> Usage {
        Usage {
            scopes: self.scopes.len(),
            records: self.scopes.values().map(|s| s.records.len()).sum(),
            snapshots: self.snapshots.len(),
            subscriptions: self.subscriptions.len(),
            retired_subscription_ids: self.retired.len(),
            retained_bytes: self.scopes.values().map(|s| s.bytes).sum::<usize>()
                + self
                    .sources
                    .iter()
                    .map(|(p, c)| source_bytes(p, c))
                    .sum::<usize>()
                + self
                    .instances
                    .iter()
                    .map(|(p, i)| p.len() + i.len() + 256)
                    .sum::<usize>(),
            transient_bytes: self.snapshots.values().map(|s| s.bytes).sum::<usize>()
                + self
                    .subscriptions
                    .values()
                    .map(Subscription::bytes)
                    .sum::<usize>()
                + self.retired.iter().map(|s| s.bytes).sum::<usize>(),
        }
    }
    fn expire(&mut self, now: Instant) {
        let expired: Vec<_> = self
            .subscriptions
            .values()
            .filter(|s| now >= s.deadline)
            .map(|s| s.id.clone())
            .collect();
        for id in expired {
            self.retire(&id, "subscriptionExpired");
        }
        let expired: Vec<_> = self
            .snapshots
            .values()
            .filter(|s| now >= s.deadline)
            .map(|s| (s.id.clone(), s.subscription.clone()))
            .collect();
        for (id, subscription) in expired {
            if let Some(subscription) = subscription {
                self.reset_subscription(&subscription);
            }
            self.snapshots.remove(&id);
        }
    }
    fn retire(&mut self, id: &str, code: &'static str) {
        if let Some(sub) = self.subscriptions.remove(id) {
            if let Some(snapshot) = sub.snapshot {
                self.snapshots.remove(&snapshot);
            }
            self.retired.push(RetiredSubscription {
                owner: sub.owner,
                client_id: sub.client_id,
                code,
                bytes: sub.metadata_bytes,
            });
        }
    }
    fn reset_subscription(&mut self, id: &str) {
        if let Some(sub) = self.subscriptions.get_mut(id) {
            if let Some(snapshot) = sub.snapshot.take() {
                self.snapshots.remove(&snapshot);
            }
            sub.reset();
        }
    }
    fn remove_owner(&mut self, matches: impl Fn(&Owner) -> bool) {
        let ids: Vec<_> = self
            .subscriptions
            .values()
            .filter(|s| matches(&s.owner))
            .map(|s| s.id.clone())
            .collect();
        for id in ids {
            if let Some(sub) = self.subscriptions.remove(&id) {
                if let Some(snapshot) = sub.snapshot {
                    self.snapshots.remove(&snapshot);
                }
            }
        }
        self.snapshots.retain(|_, s| !matches(&s.owner));
        self.retired.retain(|s| !matches(&s.owner));
    }
    fn remove_scope(&mut self, workspace_id: &str) {
        self.remove_owner(|owner| owner.context.workspace_id == workspace_id);
        self.sources.retain(|_, c| c.workspace_id != workspace_id);
        self.scopes.remove(workspace_id);
    }
    fn remove_source(&mut self, producer: &Producer, reason: &'static str, received_at: u64) {
        if *producer == Producer::Core {
            return;
        }
        self.sources.remove(producer);
        self.remove_owner(|owner| &owner.producer == producer);
        let contexts: Vec<_> = self.scopes.values().map(|s| s.context.clone()).collect();
        for context in contexts {
            let keys: Vec<_> = self
                .scope(&context)
                .unwrap()
                .records
                .keys()
                .filter(|k| &k.producer == producer)
                .cloned()
                .collect();
            for key in keys {
                let scope = self.scope_mut(&context).unwrap();
                let old = scope.records.remove(&key).unwrap();
                scope.bytes -= old.bytes;
                let Some(revision) = scope.revision.checked_add(1) else {
                    let ids: Vec<_> = self
                        .subscriptions
                        .values()
                        .filter(|s| s.owner.context == context)
                        .map(|s| s.id.clone())
                        .collect();
                    for id in ids {
                        self.reset_subscription(&id);
                    }
                    continue;
                };
                scope.revision = revision;
                let mut record = Record {
                    key: key.clone(),
                    revision,
                    received_at,
                    value: None,
                    bytes: 0,
                };
                record.bytes = encoded_size(&record.wire()) + 128;
                let record = Arc::new(record);
                let revision = scope.revision;
                self.publish(
                    &context,
                    revision,
                    Some(key),
                    Change::Record(record, reason),
                );
            }
        }
    }
    fn publish(
        &mut self,
        context: &Context,
        revision: u64,
        key: Option<RecordKey>,
        change: Change,
    ) {
        let ids: Vec<_> = self.subscriptions.keys().cloned().collect();
        for id in ids {
            let sub = self.subscriptions.get_mut(&id).unwrap();
            if &sub.owner.context != context
                || sub.phase == Phase::ResetRequired
                || key.as_ref().is_some_and(|k| !sub.filter.matches(k))
            {
                continue;
            }
            sub.frontier = revision;
            if let Some(key) = &key {
                sub.pending.insert(key.clone(), change.clone());
            } else {
                sub.pending_scope = Some(change.clone());
            }
            if sub.pending_bytes() + sub.batch.as_ref().map_or(0, |b| b.bytes)
                > limits::SUBSCRIPTION_BYTES
                || self.usage().transient_bytes > limits::TRANSIENT_BYTES
            {
                self.reset_subscription(&id);
            }
        }
    }
    fn make_snapshot(
        &mut self,
        owner: &Owner,
        filter: &Filter,
        subscription: Option<String>,
        now: Instant,
    ) -> Result<Snapshot, Error> {
        let scope = self.scope(&owner.context)?;
        let records: Vec<_> = scope
            .records
            .values()
            .filter(|r| r.value.is_some() && filter.matches(&r.key))
            .cloned()
            .collect();
        let scope_wire = scope.wire();
        let revision = scope.revision;
        let id = self.id("snapshot")?;
        let mut snapshot = Snapshot {
            id: id.clone(),
            owner: owner.clone(),
            scope: scope_wire,
            revision,
            records,
            starts: vec![0],
            cursors: vec![id],
            served: HashSet::from([0]),
            deadline: now + limits::SNAPSHOT_IDLE,
            subscription,
            bytes: 0,
        };
        let base = encoded_size(&json!({"snapshotId":snapshot.id,"scope":snapshot.scope,
            "revision":snapshot.revision.to_string(),"records":[],"nextCursor":"cursor-18446744073709551615",
            "subscriptionId":"sub-18446744073709551615","leaseMs":60000}));
        let mut page_bytes = base;
        for index in 0..snapshot.records.len() {
            let bytes = encoded_size(&snapshot.records[index].wire()) + 1;
            if page_bytes + bytes + limits::ENVELOPE_BYTES > owner.target.frame_bytes() {
                if snapshot.starts.last() == Some(&index) {
                    return Err(Error::limit());
                }
                snapshot.starts.push(index);
                snapshot.cursors.push(self.id("cursor")?);
                page_bytes = base;
            }
            page_bytes += bytes;
        }
        snapshot.bytes = snapshot.records.iter().map(|r| r.bytes + 16).sum::<usize>()
            + owner_bytes(owner)
            + encoded_size(&snapshot.scope)
            + snapshot
                .cursors
                .iter()
                .map(|c| c.len() + 128)
                .sum::<usize>()
            + 512;
        if self.usage().transient_bytes + snapshot.bytes > limits::TRANSIENT_BYTES {
            return Err(Error::limit());
        }
        Ok(snapshot)
    }
    fn make_batch(&mut self, id: &str) -> Result<(), Error> {
        let batch_id = self.id("batch")?;
        let sub = self.subscriptions.get(id).unwrap();
        let mut changes: Vec<_> = sub.pending.values().cloned().collect();
        if let Some(scope) = &sub.pending_scope {
            changes.push(scope.clone());
        }
        let base = json!({"subscriptionId":id,"clientSubscriptionId":sub.client_id,"context":sub.owner.context,
            "batchId":batch_id,"fromRevision":sub.confirmed_revision.to_string(),"toRevision":sub.frontier.to_string(),
            "part":4096,"parts":4096,"changes":[]});
        let base_bytes = encoded_size(&base);
        let mut pages: Vec<Vec<Value>> = vec![vec![]];
        let mut bytes = base_bytes;
        for change in &changes {
            let wire = change.wire();
            let size = encoded_size(&wire) + 1;
            if bytes + size + limits::ENVELOPE_BYTES > sub.owner.target.frame_bytes() {
                if pages.last().unwrap().is_empty() {
                    return Err(Error::limit());
                }
                pages.push(vec![]);
                bytes = base_bytes;
            }
            bytes += size;
            pages.last_mut().unwrap().push(wire);
        }
        let parts = pages.len();
        let frames: Vec<_> = pages
            .into_iter()
            .enumerate()
            .map(|(part, changes)| {
                let mut frame = base.clone();
                frame["part"] = json!(part);
                frame["parts"] = json!(parts);
                frame["changes"] = json!(changes);
                frame
            })
            .collect();
        let bytes = frames.iter().map(|f| encoded_size(f) + 128).sum::<usize>()
            + changes.iter().map(Change::bytes).sum::<usize>();
        if bytes > limits::SUBSCRIPTION_BYTES
            || self.usage().transient_bytes - sub.pending_bytes() + bytes > limits::TRANSIENT_BYTES
        {
            return Err(Error::limit());
        }
        let sub = self.subscriptions.get_mut(id).unwrap();
        sub.batch = Some(Batch {
            id: batch_id,
            to_revision: sub.frontier,
            frames,
            part: 0,
            bytes,
            _changes: changes,
        });
        sub.pending.clear();
        sub.pending_scope = None;
        Ok(())
    }
}

fn encoded_size(value: &Value) -> usize {
    serde_json::to_vec(value).expect("JSON value encodes").len()
}
fn source_bytes(producer: &Producer, context: &Context) -> usize {
    encoded_size(&json!({"producer":producer,"context":context})) + 256
}
fn owner_bytes(owner: &Owner) -> usize {
    source_bytes(&owner.producer, &owner.context)
        + match &owner.target {
            DeliveryTarget::Process {
                instance_id,
                view_id,
            } => instance_id.len() + view_id.len(),
            DeliveryTarget::Surface {
                window_id,
                surface_id,
            } => window_id.len() + surface_id.len(),
        }
        + 256
}
fn same_instance(a: &Producer, b: &Producer) -> bool {
    match (a, b) {
        (
            Producer::Plugin {
                plugin_id: ap,
                plugin_instance_id: ai,
                ..
            },
            Producer::Plugin {
                plugin_id: bp,
                plugin_instance_id: bi,
                ..
            },
        ) => (ap, ai) == (bp, bi),
        (Producer::Core, Producer::Core) => true,
        _ => false,
    }
}
fn validate_identifier(value: &str) -> Result<(), Error> {
    if value.is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
        return Err(Error::invalid("标识必须为 1–128 字节且不含控制字符"));
    }
    Ok(())
}
fn validate_topic(value: &str) -> Result<(), Error> {
    if value.is_empty() || value.len() > 128 || !value.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(Error::invalid("主题必须为 1–128 个可见 ASCII 字节"));
    }
    Ok(())
}
fn validate_key(value: &str) -> Result<(), Error> {
    if value.is_empty() || value.len() > 512 || value.chars().any(char::is_control) {
        return Err(Error::invalid("记录 key 必须为 1–512 字节且不含控制字符"));
    }
    Ok(())
}
fn validate_producer(producer: &Producer) -> Result<(), Error> {
    if let Producer::Plugin {
        plugin_id,
        plugin_instance_id,
        view_id,
    } = producer
    {
        for value in [plugin_id, plugin_instance_id, view_id] {
            validate_identifier(value)?;
        }
    }
    Ok(())
}
fn validate_context(context: &Context) -> Result<(), Error> {
    for value in [
        &context.workspace_id,
        &context.server_id,
        &context.connection_id,
    ] {
        validate_identifier(value)?;
    }
    Ok(())
}
fn validate_target(owner: &Owner) -> Result<(), Error> {
    match &owner.target {
        DeliveryTarget::Process {
            instance_id,
            view_id,
        } => {
            validate_identifier(instance_id)?;
            validate_identifier(view_id)?;
            if let Producer::Plugin {
                plugin_instance_id,
                view_id: own_view,
                ..
            } = &owner.producer
            {
                if instance_id != plugin_instance_id || view_id != own_view {
                    return Err(Error::invalid("进程投递目标与来源不符"));
                }
            }
        }
        DeliveryTarget::Surface {
            window_id,
            surface_id,
        } => {
            validate_identifier(window_id)?;
            validate_identifier(surface_id)?;
        }
    }
    Ok(())
}
fn validate_value(value: &Value, depth: usize) -> Result<(), Error> {
    if depth > 32 {
        return Err(Error::invalid("JSON 嵌套超过 32 层"));
    }
    match value {
        Value::Array(values) => {
            for value in values {
                validate_value(value, depth + 1)?;
            }
        }
        Value::Object(values) => {
            for value in values.values() {
                validate_value(value, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests;

//! Optional sharedState.v1 client. Delivery commits a projection before acknowledging.
use super::*;
use std::time::Instant;
#[derive(Default)]
pub(crate) struct Inbox(Mutex<HashMap<String, Arc<Mutex<Projection>>>>);
pub struct Projection {
    context: Value,
    subscription: String,
    revision: String,
    scope: Value,
    records: HashMap<String, Value>,
    valid: bool,
    deadline: Instant,
    batch: Option<Batch>,
    last_batch: String,
}
struct Batch {
    id: String,
    to: String,
    parts: usize,
    frames: Vec<Value>,
    bytes: usize,
}
impl Inbox {
    pub(crate) fn receive(&self, frame: &Value) -> Result<Value, String> {
        let id = frame["clientSubscriptionId"]
            .as_str()
            .ok_or("缺少客户端订阅 ID")?;
        let state = self
            .0
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or("订阅已释放")?;
        let mut state = state.lock().unwrap();
        let result = state.apply(frame);
        if result.is_err() {
            state.valid = false;
            state.batch = None;
        }
        result
    }
}
impl Projection {
    fn apply(&mut self, f: &Value) -> Result<Value, String> {
        if Instant::now() >= self.deadline {
            self.valid = false;
            return Err("共享状态租约已到期".into());
        }
        if f["resetRequired"] == true {
            self.valid = false;
            self.batch = None;
            return Ok(json!({"accepted":true}));
        }
        if !self.valid || f["context"] != self.context || f["subscriptionId"] != self.subscription {
            return Err("共享批次归属已失效".into());
        }
        let part = f["part"].as_u64().ok_or("part 无效")? as usize;
        let parts = f["parts"]
            .as_u64()
            .filter(|n| *n > 0 && *n <= 4096)
            .ok_or("parts 无效")? as usize;
        if part >= parts {
            return Err("part 超限".into());
        }
        let id = f["batchId"].as_str().ok_or("缺少 batchId")?;
        let from = decimal(&f["fromRevision"])?;
        let to = decimal(&f["toRevision"])?;
        let final_part = part + 1 == parts;
        let mut applied = false;
        if self.last_batch == id && to.to_string() == self.revision {
            applied = final_part;
        } else {
            if from.to_string() != self.revision || to <= from {
                return Err("共享游标失步".into());
            }
            let batch = self.batch.get_or_insert_with(|| Batch {
                id: id.into(),
                to: to.to_string(),
                parts,
                frames: Vec::new(),
                bytes: 0,
            });
            if batch.id != id
                || batch.to != to.to_string()
                || batch.parts != parts
                || part > batch.frames.len()
            {
                return Err("共享分帧失步".into());
            }
            if part < batch.frames.len() {
                if batch.frames[part] != f["changes"] {
                    return Err("重试帧内容改变".into());
                }
            } else {
                batch.bytes += serde_json::to_vec(f).map_err(|e| e.to_string())?.len();
                if batch.bytes > 1024 * 1024 {
                    return Err("共享积压超限".into());
                }
                batch.frames.push(f["changes"].clone());
            }
            if final_part {
                let mut records = self.records.clone();
                let mut scope = self.scope.clone();
                for frame in &batch.frames {
                    for change in frame.as_array().ok_or("changes 无效")? {
                        if change["op"] == "scope" {
                            scope = change["scope"].clone();
                        } else {
                            let record = &change["record"];
                            validate_record(record)?;
                            let key = identity(record);
                            if change["op"] == "put" && record["present"] == true {
                                records.insert(key, record.clone());
                            } else if change["op"] == "remove" && record["present"] == false {
                                records.remove(&key);
                            } else {
                                return Err("共享变化无效".into());
                            }
                        }
                    }
                }
                self.records = records;
                self.scope = scope;
                self.revision = to.to_string();
                self.last_batch = id.into();
                self.batch = None;
                applied = true;
            }
        }
        Ok(if applied {
            json!({"accepted":true,"batchId":id,"part":part,"appliedRevision":self.revision})
        } else {
            json!({"accepted":true,"batchId":id,"part":part})
        })
    }
}
pub fn request(
    client: &Client,
    view: &str,
    context: &Value,
    op: &str,
    mut params: Value,
) -> Result<Value, RpcError> {
    let params = params
        .as_object_mut()
        .ok_or_else(|| RpcError::local("共享参数必须为对象"))?;
    params.insert("viewId".into(), json!(view));
    params.insert("expectedContext".into(), context.clone());
    client.call_detailed(&format!("state.{op}"), json!(params))
}
pub struct Watch {
    client: Client,
    view: String,
    context: Value,
    filter: Value,
    client_id: String,
    state: Arc<Mutex<Projection>>,
}
impl Watch {
    pub fn open(
        client: &Client,
        view: &str,
        context: Value,
        filter: Value,
    ) -> Result<Self, RpcError> {
        let id = format!(
            "watch-{}",
            client.inner.sequence.fetch_add(1, Ordering::Relaxed)
        );
        let state = Arc::new(Mutex::new(Projection {
            context: context.clone(),
            subscription: String::new(),
            revision: "0".into(),
            scope: Value::Null,
            records: HashMap::new(),
            valid: false,
            deadline: Instant::now(),
            batch: None,
            last_batch: String::new(),
        }));
        client
            .inner
            .state
            .0
            .lock()
            .unwrap()
            .insert(id.clone(), state.clone());
        let watch = Self {
            client: client.clone(),
            view: view.into(),
            context,
            filter,
            client_id: id,
            state,
        };
        let initial = watch.call(
            "subscribe",
            json!({"clientSubscriptionId":watch.client_id,"filter":watch.filter}),
        )?;
        let mut records = initial["records"]
            .as_array()
            .ok_or_else(|| RpcError::local("共享快照无效"))?
            .clone();
        let revision = decimal(&initial["revision"])
            .map_err(RpcError::local)?
            .to_string();
        let mut cursor = initial["nextCursor"].as_str().map(String::from);
        while let Some(value) = cursor {
            let page = watch.call("list", json!({"cursor":value}))?;
            if page["revision"] != revision {
                return Err(RpcError::local("共享快照游标失步"));
            }
            records.extend(
                page["records"]
                    .as_array()
                    .ok_or_else(|| RpcError::local("共享页无效"))?
                    .clone(),
            );
            if serde_json::to_vec(&records)
                .map_err(|e| RpcError::local(e.to_string()))?
                .len()
                > 4 * 1024 * 1024
            {
                return Err(RpcError::local("共享快照超限"));
            }
            cursor = page["nextCursor"].as_str().map(String::from);
        }
        for record in &records {
            validate_record(record).map_err(RpcError::local)?;
        }
        {
            let mut s = watch.state.lock().unwrap();
            s.subscription = initial["subscriptionId"]
                .as_str()
                .ok_or_else(|| RpcError::local("缺少订阅 ID"))?
                .into();
            s.revision = revision.clone();
            s.scope = initial["scope"].clone();
            s.records = records.into_iter().map(|r| (identity(&r), r)).collect();
            s.valid = true;
            s.deadline = Instant::now()
                + Duration::from_millis(initial["leaseMs"].as_u64().unwrap_or(60000));
        }
        watch.call("subscribe",json!({"clientSubscriptionId":watch.client_id,"filter":watch.filter,"acknowledgedRevision":revision}))?;
        Ok(watch)
    }
    fn call(&self, op: &str, p: Value) -> Result<Value, RpcError> {
        request(&self.client, &self.view, &self.context, op, p)
    }
    pub fn snapshot(&self) -> Option<Value> {
        let s = self.state.lock().unwrap();
        (s.valid&&Instant::now()<s.deadline).then(||json!({"scope":s.scope,"revision":s.revision,"records":s.records.values().collect::<Vec<_>>()}))
    }
    pub fn renew(&self) -> Result<(), RpcError> {
        let revision = self.state.lock().unwrap().revision.clone();
        let result=self.call("subscribe",json!({"clientSubscriptionId":self.client_id,"filter":self.filter,"acknowledgedRevision":revision}));
        let mut state = self.state.lock().unwrap();
        match result {
            Ok(v) => {
                state.deadline =
                    Instant::now() + Duration::from_millis(v["leaseMs"].as_u64().unwrap_or(60000));
                Ok(())
            }
            Err(e) => {
                state.valid = false;
                Err(e)
            }
        }
    }
}
impl Drop for Watch {
    fn drop(&mut self) {
        self.client
            .inner
            .state
            .0
            .lock()
            .unwrap()
            .remove(&self.client_id);
        if !self.client.is_closed() {
            let _ = self.call(
                "unsubscribe",
                json!({"clientSubscriptionId":self.client_id}),
            );
        }
    }
}
fn decimal(v: &Value) -> Result<u64, String> {
    let s = v.as_str().ok_or("修订号必须为字符串")?;
    if s.is_empty() || (s.len() > 1 && s.starts_with('0')) || !s.bytes().all(|b| b.is_ascii_digit())
    {
        return Err("修订号格式无效".into());
    }
    s.parse().map_err(|_| "修订号超限".into())
}
fn identity(r: &Value) -> String {
    json!([r["topic"], r["producer"], r["key"]]).to_string()
}
fn validate_record(r: &Value) -> Result<(), String> {
    decimal(&r["revision"])?;
    if !r["topic"].is_string()
        || !r["key"].is_string()
        || !r["present"].is_boolean()
        || r["present"].as_bool() != Some(r.get("value").is_some())
        || !matches!(r["producer"]["kind"].as_str(), Some("core" | "plugin"))
    {
        return Err("共享记录无效".into());
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rust_and_js_share_atomic_decimal_wire_contract() {
        let f: Value =
            serde_json::from_str(include_str!("../../../scripts/fixtures/shared-state.json"))
                .unwrap();
        let mut p = Projection {
            context: f["context"].clone(),
            subscription: "sub".into(),
            revision: f["initialRevision"].as_str().unwrap().into(),
            scope: json!({}),
            records: HashMap::new(),
            valid: true,
            deadline: Instant::now() + Duration::from_secs(60),
            batch: None,
            last_batch: String::new(),
        };
        validate_record(&f["record"]).unwrap();
        for (i, raw) in f["frames"].as_array().unwrap().iter().enumerate() {
            let mut frame = raw.clone();
            frame["subscriptionId"] = json!("sub");
            frame["context"] = f["context"].clone();
            let ack = p.apply(&frame).unwrap();
            assert_eq!(
                p.revision,
                if i == 0 {
                    f["initialRevision"].as_str().unwrap()
                } else {
                    "9007199254740995"
                }
            );
            assert_eq!(ack.get("appliedRevision").is_some(), i == 1);
            assert!(p.records.is_empty());
        }
        assert!(decimal(&json!("01")).is_err());
        assert!(decimal(&json!("18446744073709551616")).is_err());
    }
}

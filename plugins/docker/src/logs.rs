//! Docker log capture metadata. Output lives in the core, never in this process.
use crate::{commands, runtime};
use serde_json::{json, Value};
use sodeck_plugin_client::Client;
use std::{
    collections::BTreeMap,
    sync::{mpsc, Arc, Mutex},
    time::{Duration, Instant},
};
struct Resource {
    id: String,
    start: u64,
    end: Option<u64>,
    tail: bool,
    created: Instant,
    status: Value,
    ticket: Option<Value>,
    stopped: bool,
}
struct Capture {
    generation: u64,
    context: Value,
    endpoint: String,
    engine: String,
    container: String,
    resources: Vec<Resource>,
    navigation: Value,
    error: Option<String>,
    loading: bool,
}
enum Job {
    Start {
        view: String,
        target: Value,
        input: Value,
    },
    Stop(String),
    Earlier {
        view: String,
        end: u64,
    },
    Dispose(String),
}
pub struct Logs {
    views: Mutex<BTreeMap<String, Capture>>,
    sender: mpsc::SyncSender<Job>,
    requests: Mutex<BTreeMap<String, Value>>,
    app: runtime::Shared,
    pending: Mutex<std::collections::BTreeSet<String>>,
    intents: Mutex<BTreeMap<String, u64>>,
}
impl Logs {
    pub fn new(client: Client, app: runtime::Shared) -> Arc<Self> {
        let (tx, rx) = mpsc::sync_channel(16);
        let logs = Arc::new(Self {
            views: Mutex::new(BTreeMap::new()),
            sender: tx,
            requests: Mutex::new(BTreeMap::new()),
            app,
            pending: Mutex::new(std::collections::BTreeSet::new()),
            intents: Mutex::new(BTreeMap::new()),
        });
        let weak = Arc::downgrade(&logs);
        std::thread::spawn(move || {
            while !client.is_closed() {
                let Some(logs) = weak.upgrade() else { break };
                match rx.recv_timeout(Duration::from_millis(500)) {
                    Ok(Job::Start {
                        view,
                        target,
                        input,
                    }) => logs.start(&client, &view, target, input),
                    Ok(Job::Stop(view)) => logs.stop(&client, &view, false),
                    Ok(Job::Dispose(view)) => {
                        logs.stop(&client, &view, true);
                    }
                    Ok(Job::Earlier { view, end }) => logs.earlier(&client, &view, end),
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    _ => {}
                }
                logs.poll(&client);
            }
        });
        logs
    }
    pub fn event(&self, view: &str, target: Value, input: &Value) -> Result<Value, String> {
        match input["action"].as_str().unwrap_or("") {
            "logs.start" => {
                let request = input["requestId"]
                    .as_str()
                    .filter(|s| !s.is_empty() && s.len() <= 128)
                    .ok_or("日志请求ID无效")?;
                let mut requests = self.requests.lock().unwrap();
                if let Some(old) = requests.get(view) {
                    if old["requestId"] == request {
                        if old != input {
                            return Err("相同日志请求参数不同".into());
                        }
                        return Ok(json!({"accepted":true,"replayed":true}));
                    }
                }
                let sequence = input["intentSequence"]
                    .as_str()
                    .and_then(|s| s.parse::<u64>().ok())
                    .ok_or("日志请求序号无效")?;
                if sequence <= *self.intents.lock().unwrap().get(view).unwrap_or(&0) {
                    return Err("旧日志请求已过期，不能重新执行".into());
                }
                if self.pending.lock().unwrap().contains(view) {
                    return Err("日志请求已在队列中，请等待回执".into());
                }
                self.pending.lock().unwrap().insert(view.into());
                if self
                    .sender
                    .try_send(Job::Start {
                        view: view.into(),
                        target,
                        input: input.clone(),
                    })
                    .is_err()
                {
                    self.pending.lock().unwrap().remove(view);
                    return Err("日志请求队列已满".into());
                }
                requests.insert(view.into(), input.clone());
                self.intents.lock().unwrap().insert(view.into(), sequence);
                Ok(json!({"accepted":true}))
            }
            "logs.stop" => {
                self.sender
                    .try_send(Job::Stop(view.into()))
                    .map_err(|_| "日志请求队列已满")?;
                Ok(json!({"accepted":true}))
            }
            "logs.loadEarlier" => {
                let end = input["end"].as_u64().ok_or("更早时间无效")?;
                self.sender
                    .try_send(Job::Earlier {
                        view: view.into(),
                        end,
                    })
                    .map_err(|_| "日志请求队列已满")?;
                Ok(json!({"accepted":true}))
            }
            "logs.navigation" => {
                if input["state"].to_string().len() > 8192 {
                    return Err("日志阅读状态超过上限".into());
                }
                if let Some(c) = self.views.lock().unwrap().get_mut(view) {
                    c.navigation = input["state"].clone()
                }
                Ok(json!({}))
            }
            "logs.status" => self.snapshot(view),
            _ => Err("未知日志动作".into()),
        }
    }
    pub fn dispose(&self, view: &str) {
        self.requests.lock().unwrap().remove(view);
        self.pending.lock().unwrap().remove(view);
        self.intents.lock().unwrap().remove(view);
        let _ = self.sender.try_send(Job::Dispose(view.into()));
    }
    pub fn snapshot(&self, view: &str) -> Result<Value, String> {
        let next = (*self.intents.lock().unwrap().get(view).unwrap_or(&0) + 1).to_string();
        let pending = self.pending.lock().unwrap().contains(view);
        let views = self.views.lock().unwrap();
        let Some(c) = views.get(view) else {
            return Ok(json!({"generation":0,"resources":[],"loading":pending,"nextIntent":next}));
        };
        Ok(
            json!({"generation":c.generation,"context":c.context,"loading":pending || c.loading,"nextIntent":next,"error":c.error,"navigation":c.navigation,"resources":c.resources.iter().map(|r|json!({"id":r.id,"start":r.start,"end":r.end,"tail":r.tail,"status":r.status})).collect::<Vec<_>>()}),
        )
    }
    fn stop(&self, client: &Client, view: &str, release: bool) {
        let ids = {
            let mut all = self.views.lock().unwrap();
            if release {
                all.remove(view)
                    .map(|c| c.resources.into_iter().map(|r| r.id).collect::<Vec<_>>())
                    .unwrap_or_default()
            } else {
                all.get(view)
                    .map(|c| c.resources.iter().map(|r| r.id.clone()).collect())
                    .unwrap_or_default()
            }
        };
        if !release {
            if let Some(c) = self.views.lock().unwrap().get_mut(view) {
                for r in &mut c.resources {
                    r.stopped = true;
                }
            }
        }
        for id in ids {
            let _ = client.call(
                if release {
                    "session.stream.release"
                } else {
                    "session.stream.close"
                },
                json!({"streamId":id}),
            );
        }
    }
    fn start(&self, client: &Client, view: &str, target: Value, input: Value) {
        if !self.app.lock().unwrap().views.contains_key(view) {
            return;
        }
        if let Some(c) = self.views.lock().unwrap().get_mut(view) {
            if c.resources.iter().any(|r| r.ticket.is_some()) {
                c.error = Some("原日志回执仍待确认，请稍后重新读取".into());
                self.pending.lock().unwrap().remove(view);
                return;
            }
        }
        let checked_target = runtime::log_target(&self.app, client, view, &target);
        if let Err(error) = &checked_target {
            if let Some(c) = self.views.lock().unwrap().get_mut(view) {
                c.error = Some(error.clone());
                self.pending.lock().unwrap().remove(view);
                return;
            }
        }
        let old = self
            .views
            .lock()
            .unwrap()
            .get(view)
            .map(|c| c.generation)
            .unwrap_or(0);
        self.stop(client, view, true);
        let mut c = Capture {
            generation: old + 1,
            context: target["context"].clone(),
            endpoint: target["endpoint"].as_str().unwrap_or("").into(),
            engine: target["engine"]["id"].as_str().unwrap_or("").into(),
            container: target["containerId"].as_str().unwrap_or("").into(),
            resources: vec![],
            navigation: json!({}),
            error: None,
            loading: true,
        };
        let result = (|| {
            let target = checked_target?;
            let now = clock_seconds(
                target["engine"]["systemTime"]
                    .as_str()
                    .ok_or("Engine未提供源时间，请刷新后重试")?,
            )?;
            let duration = input["windowSeconds"].as_u64();
            let mut start = duration
                .map(|d| now.saturating_sub(d))
                .unwrap_or_else(|| input["start"].as_u64().unwrap_or(0));
            let end = if duration.is_some() {
                now
            } else {
                input["end"].as_u64().unwrap_or(0)
            };
            if now == 0 || start >= end || end > now {
                return Err("日志时间范围无效".into());
            }
            if input["live"] == true && target["status"] != "running" && input["complete"] != true {
                start = 0;
            }
            commands::endpoint(&c.endpoint)?;
            commands::id(&c.container)?;
            if c.engine.is_empty() {
                return Err("请先刷新Docker身份".into());
            }
            if input["live"] == true && target["status"] == "running" {
                c.resources
                    .push(open(client, view, &c, end, None, false, 32 * 1024 * 1024)?);
            }
            c.resources.push(open(
                client,
                view,
                &c,
                start,
                Some(end),
                input["complete"] != true,
                if input["complete"] == true {
                    48 * 1024 * 1024
                } else {
                    4 * 1024 * 1024
                },
            )?);
            Ok::<_, String>(())
        })();
        if let Err(error) = result {
            c.error = Some(error)
        }
        c.loading = false;
        self.pending.lock().unwrap().remove(view);
        if !self.app.lock().unwrap().views.contains_key(view) {
            for r in &c.resources {
                let _ = client.call("session.stream.release", json!({"streamId":r.id}));
            }
            return;
        }
        self.views.lock().unwrap().insert(view.into(), c);
    }
    fn earlier(&self, client: &Client, view: &str, end: u64) {
        let target = {
            let all = self.views.lock().unwrap();
            let Some(c) = all.get(view) else { return };
            if c.resources.len() >= 12
                || c.resources
                    .iter()
                    .any(|r| r.end.is_some() && r.status["finished"] != true)
            {
                drop(all);
                if let Some(c) = self.views.lock().unwrap().get_mut(view) {
                    c.error = Some(
                        "历史读取尚未完成或窗口已满，请等待完成后补读，或选择其他时间范围".into(),
                    )
                }
                return;
            }
            Capture {
                generation: c.generation,
                context: c.context.clone(),
                endpoint: c.endpoint.clone(),
                engine: c.engine.clone(),
                container: c.container.clone(),
                resources: vec![],
                navigation: Value::Null,
                error: None,
                loading: false,
            }
        };
        let expected = json!({"context":target.context,"endpoint":target.endpoint,"engine":{"id":target.engine},"containerId":target.container});
        if let Err(error) = runtime::log_target(&self.app, client, view, &expected) {
            if let Some(c) = self.views.lock().unwrap().get_mut(view) {
                c.error = Some(error);
            }
            return;
        }
        let result = open(
            client,
            view,
            &target,
            end.saturating_sub(900),
            Some(end),
            true,
            4 * 1024 * 1024,
        );
        if let Some(c) = self.views.lock().unwrap().get_mut(view) {
            match result {
                Ok(r) => c.resources.push(r),
                Err(e) => c.error = Some(e),
            }
        }
    }
    fn poll(&self, client: &Client) {
        let alive = self
            .app
            .lock()
            .unwrap()
            .views
            .keys()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        let closed = self
            .views
            .lock()
            .unwrap()
            .keys()
            .filter(|view| !alive.contains(*view))
            .cloned()
            .collect::<Vec<_>>();
        for view in closed {
            self.stop(client, &view, true);
            self.requests.lock().unwrap().remove(&view);
            self.intents.lock().unwrap().remove(&view);
            self.pending.lock().unwrap().remove(&view);
        }
        let reads = {
            let all = self.views.lock().unwrap();
            all.iter()
                .flat_map(|(view, c)| {
                    c.resources
                        .iter()
                        .filter(|r| r.status["finished"] != true)
                        .map(|r| {
                            (
                                view.clone(),
                                r.id.clone(),
                                r.created,
                                r.end,
                                r.tail,
                                r.ticket.clone(),
                                r.stopped,
                            )
                        })
                })
                .collect::<Vec<_>>()
        };
        for (view, mut id, created, end, tail, ticket, stopped) in reads {
            if let Some(ticket) = ticket {
                match client.call("session.stream.lookup", ticket.clone()) {
                    Ok(found) if found["known"] == true => {
                        id = found["streamId"].as_str().unwrap_or("").into();
                        if let Some(c) = self.views.lock().unwrap().get_mut(&view) {
                            if let Some(r) = c
                                .resources
                                .iter_mut()
                                .find(|r| r.ticket.as_ref() == Some(&ticket))
                            {
                                r.id = id.clone();
                                r.ticket = None;
                            }
                        }
                        if stopped {
                            let _ = client.call("session.stream.close", json!({"streamId":id}));
                        }
                    }
                    _ => continue,
                }
            }
            if end.is_some() && created.elapsed() > Duration::from_secs(if tail { 30 } else { 120 })
            {
                let _ = client.call("session.stream.close", json!({"streamId":id}));
            }
            let result = client.call("session.stream.status", json!({"streamId":id}));
            if let Some(c) = self.views.lock().unwrap().get_mut(&view) {
                if let Some(r) = c.resources.iter_mut().find(|r| r.id == id) {
                    r.status = result.unwrap_or_else(
                        |e| json!({"state":"failed","finished":true,"error":{"message":e}}),
                    );
                }
            }
        }
    }
}
fn open(
    client: &Client,
    view: &str,
    c: &Capture,
    start: u64,
    end: Option<u64>,
    tail: bool,
    budget: usize,
) -> Result<Resource, String> {
    let mut args = vec![
        "container".into(),
        "logs".into(),
        "--timestamps".into(),
        "--since".into(),
        start.to_string(),
    ];
    if let Some(end) = end {
        args.extend(["--until".into(), end.to_string()]);
    } else {
        args.push("--follow".into())
    }
    if tail {
        args.extend(["--tail".into(), "2001".into()]);
    }
    args.push(c.container.clone());
    let check = commands::docker(&c.endpoint, &["info", "--format", "{{.ID}}"]);
    let command = commands::docker(
        &c.endpoint,
        &args.iter().map(String::as_str).collect::<Vec<_>>(),
    );
    let inspect = commands::docker(
        &c.endpoint,
        &["container", "inspect", "--format", "{{.Id}}", &c.container],
    );
    let command=format!("actual=$({check}) || exit; [ \"$actual\" = {} ] || {{ printf '%s\\n' 'Docker Engine changed; logs not started' >&2; exit 1; }}; actual=$({inspect}) || exit; [ \"$actual\" = {} ] || exit 1; exec {command}",commands::quote(&c.engine),commands::quote(&c.container));
    static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let sequence = SEQUENCE
        .try_update(
            std::sync::atomic::Ordering::Relaxed,
            std::sync::atomic::Ordering::Relaxed,
            |n| n.checked_add(1),
        )
        .map_err(|_| "流式命令序号已耗尽")?;
    let params = json!({"viewId":view,"expectedContext":c.context,"sequence":sequence.to_string(),"operationId":format!("logs-{}-{}",c.generation,sequence),"command":command,"maxOutputBytes":budget,"retention":if end.is_some(){"fixed"}else{"rolling"}});
    let mut response = None;
    for _ in 0..3 {
        match client.call_detailed("session.stream.open", params.clone()) {
            Ok(r) => {
                response = Some(r);
                break;
            }
            Err(e) if e.code == -32003 => continue,
            Err(e) => return Err(e.message),
        }
    }
    let id = response
        .as_ref()
        .map(|r| r["streamId"].as_str().ok_or("核心没有返回日志资源"))
        .transpose()?
        .unwrap_or("")
        .into();
    let ticket = if response.is_none() {
        Some(params)
    } else {
        None
    };
    Ok(Resource {
        id,
        start,
        end,
        tail,
        created: Instant::now(),
        status: json!({"state":"opening","finished":false}),
        ticket,
        stopped: false,
    })
}
fn clock_seconds(value: &str) -> Result<u64, String> {
    let invalid = || "Engine源时间不是有效RFC3339时间".to_string();
    if !value.is_ascii()
        || value.len() < 20
        || &value[4..5] != "-"
        || &value[7..8] != "-"
        || &value[10..11] != "T"
        || &value[13..14] != ":"
        || &value[16..17] != ":"
    {
        return Err(invalid());
    }
    let number = |a, b| {
        if !value[a..b].bytes().all(|b| b.is_ascii_digit()) {
            return Err(invalid());
        }
        value[a..b].parse::<i64>().map_err(|_| invalid())
    };
    let (mut year, month, day, hour, minute, second) = (
        number(0, 4)?,
        number(5, 7)?,
        number(8, 10)?,
        number(11, 13)?,
        number(14, 16)?,
        number(17, 19)?,
    );
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if !(1970..=9999).contains(&year)
        || !(1..=12).contains(&month)
        || !(1..=days[(month - 1) as usize]).contains(&day)
        || !(0..=23).contains(&hour)
        || !(0..=59).contains(&minute)
        || !(0..=59).contains(&second)
    {
        return Err(invalid());
    }
    let suffix = &value[19..];
    let zone = if let Some(fraction) = suffix.strip_prefix('.') {
        let at = fraction.find(['Z', '+', '-']).ok_or_else(invalid)?;
        if at == 0 || at > 9 || !fraction[..at].bytes().all(|b| b.is_ascii_digit()) {
            return Err(invalid());
        }
        &fraction[at..]
    } else {
        suffix
    };
    let offset = if zone == "Z" {
        0
    } else {
        if zone.len() != 6 || !zone.starts_with(['+', '-']) || &zone[3..4] != ":" {
            return Err(invalid());
        }
        let h = zone[1..3].parse::<i64>().map_err(|_| invalid())?;
        let m = zone[4..6].parse::<i64>().map_err(|_| invalid())?;
        if !(0..=23).contains(&h)
            || !(0..=59).contains(&m)
            || !zone[1..3]
                .bytes()
                .chain(zone[4..6].bytes())
                .all(|b| b.is_ascii_digit())
        {
            return Err(invalid());
        }
        (h * 3600 + m * 60) * if zone.starts_with('+') { 1 } else { -1 }
    };
    year -= if month <= 2 { 1 } else { 0 };
    let era = year / 400;
    let y = year - era * 400;
    let m = month + if month > 2 { -3 } else { 9 };
    let d = (153 * m + 2) / 5 + day - 1;
    let days = era * 146097 + y * 365 + y / 4 - y / 100 + d - 719468;
    u64::try_from(days * 86400 + hour * 3600 + minute * 60 + second - offset).map_err(|_| invalid())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn engine_clock_keeps_timezone_and_validates_calendar() {
        assert_eq!(
            clock_seconds("2026-11-15T08:00:00.123456789+08:00").unwrap(),
            clock_seconds("2026-11-15T00:00:00Z").unwrap()
        );
        assert_eq!(clock_seconds("1970-01-01T00:00:00Z").unwrap(), 0);
        assert!(clock_seconds("2026-02-29T00:00:00Z").is_err());
        assert!(clock_seconds("2026-11-15T00:00:00.helloZ").is_err());
        assert!(clock_seconds("2026-11-15T00:00:00--1:00").is_err());
        assert!(clock_seconds("2026-11-15T+1:00:00Z").is_err());
        assert_eq!(
            clock_seconds("2026-11-15T00:00:00-08:30").unwrap(),
            clock_seconds("2026-11-15T08:30:00Z").unwrap()
        );
    }
}

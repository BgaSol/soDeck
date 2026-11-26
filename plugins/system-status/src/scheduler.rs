use crate::{
    collectors::{self, Family},
    metrics::{self, Baseline},
};
use serde::Serialize;
use serde_json::{json, Value};
use sodeck_plugin_client::protocol::requests as api;
use sodeck_plugin_client::{Client, RpcError};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{mpsc, Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
pub const GROUPS: [&str; 5] = ["identity", "dynamic", "topology", "capacity", "uptime"];
pub struct Group {
    due: Option<Instant>,
    manual_pending: bool,
    pub observed: Option<u64>,
    pub success: Option<Instant>,
    pub error: Option<String>,
    failures: u8,
    quarantined: bool,
}
pub struct View {
    publisher: crate::shared_state::Publisher,
    pub context: Value,
    pub paused: bool,
    pub connected: bool,
    pub revision: u64,
    pub data: crate::model::StatusState,
    pub navigation: Value,
    pub groups: Vec<Group>,
    pub history: BTreeMap<String, VecDeque<Point>>,
    family: Option<Family>,
    epoch: u64,
    sequence: u64,
    state_due: Instant,
    pending: bool,
    ticket: Option<Ticket>,
    probe: u8,
    baseline: Baseline,
    manual: bool,
    capacity_page: usize,
}
#[derive(Clone, Serialize)]
pub struct Point {
    at: u64,
    value: Option<f64>,
}
struct Ticket {
    id: Option<String>,
    params: Value,
    nonce: String,
    group: usize,
    stdout: Vec<u8>,
    stderr: usize,
    a: String,
    b: String,
    paths: Vec<String>,
    targets: Vec<crate::mounts::Target>,
    poll_due: Instant,
}
pub struct App {
    pub views: BTreeMap<String, View>,
    pub interval: u64,
    pub settings_revision: u64,
    round: usize,
    last_tick: Instant,
    last_wall: SystemTime,
}
struct Work {
    view: String,
    epoch: u64,
    kind: Kind,
    method: &'static str,
    params: Value,
}
#[derive(Clone, Copy)]
enum Kind {
    SharedGet,
    SharedPut,
    SharedRemove,
    State,
    Start,
    Poll,
    Cancel,
    Release,
}
struct Result {
    work: Work,
    result: std::result::Result<Value, RpcError>,
}
pub fn timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
impl App {
    pub fn new(interval: u64) -> Self {
        Self {
            views: BTreeMap::new(),
            interval,
            settings_revision: 0,
            round: 0,
            last_tick: Instant::now(),
            last_wall: SystemTime::now(),
        }
    }
    fn enforce_budget(&mut self) {
        for v in self.views.values_mut() {
            while cache_size(v) > 2 * 1024 * 1024 {
                if !shrink(v) {
                    break;
                }
            }
        }
        while self.views.values().map(cache_size).sum::<usize>() > 64 * 1024 * 1024 {
            let Some(id) = self
                .views
                .iter()
                .max_by_key(|(_, v)| cache_size(v))
                .map(|(id, _)| id.clone())
            else {
                break;
            };
            if !shrink(self.views.get_mut(&id).unwrap()) {
                break;
            }
        }
    }
    pub fn mount(&mut self, id: &str, context: &Value) -> std::result::Result<(), String> {
        if !self.views.contains_key(id) {
            if self.views.len() >= 128 {
                return Err("系统状态工作区已达上限".into());
            }
            self.views.insert(
                id.into(),
                View {
                    publisher: crate::shared_state::Publisher::default(),
                    context: context.clone(),
                    paused: false,
                    connected: false,
                    revision: 1,
                    data: crate::model::StatusState::default(),
                    navigation: json!({}),
                    groups: groups(),
                    history: BTreeMap::new(),
                    family: None,
                    epoch: 1,
                    sequence: 0,
                    state_due: Instant::now(),
                    pending: false,
                    ticket: None,
                    probe: 0,
                    baseline: Baseline::default(),
                    manual: false,
                    capacity_page: 0,
                },
            );
        }
        Ok(())
    }
    pub fn snapshot(&self, id: &str, known: Option<u64>) -> std::result::Result<Value, String> {
        let v = self.views.get(id).ok_or("视图已关闭")?;
        // Age changes independently of data revision; a visible page derives fresh/stale locally.
        if known == Some(v.revision) {
            return Ok(json!({"revision":v.revision,"unchanged":true}));
        }
        let group:Vec<_>=v.groups.iter().enumerate().map(|(i,g)|json!({"name":GROUPS[i],"observedAt":g.observed,"error":g.error,"quarantined":g.quarantined,"staleAfterMs":match i {1=>self.interval*2000,2=>60000,_=>120000}})).collect();
        let mut data = v.data.data();
        let interfaces = if v.family == Some(Family::Windows) {
            &v.data.counter_interfaces
        } else {
            &v.data.interfaces
        };
        data["sources"]["interfaces"] = interfaces.meta();
        let network = selected_device(
            interfaces.value.as_deref().unwrap_or(&[]),
            v.navigation["interfaceId"].as_str(),
            true,
        );
        let disk = selected_device(
            v.data.disks.value.as_deref().unwrap_or(&[]),
            v.navigation["diskId"].as_str(),
            false,
        );
        let rates = |kind: &str, row: &Value| {
            let key = format!("{kind}:{}", row["id"].as_str().unwrap_or(""));
            v.data.rate(&key)
        };
        Ok(
            json!({"revision":v.revision,"context":v.context,"paused":v.paused,"connected":v.connected,"collecting":v.ticket.is_some(),"family":v.family.map(Family::name),"intervalSeconds":self.interval,"data":data,"groups":group,"navigation":v.navigation,"network":{"device":network,"rates":rates("net",&network)},"disk":{"device":disk,"rates":rates("disk",&disk)},"filesystemTotal":v.data.mounts.rows().len(),"filesystemSummary":crate::filesystem::summary(&crate::filesystem::project(&v.data.mounts))}),
        )
    }

    pub fn control(
        &mut self,
        id: &str,
        action: &str,
        p: &Value,
    ) -> std::result::Result<Value, String> {
        let v = self.views.get_mut(id).ok_or("视图已关闭")?;
        match action {
            "refresh" => {
                v.manual = true;
                for (i, g) in v.groups.iter_mut().enumerate() {
                    g.manual_pending = v.ticket.as_ref().is_none_or(|t| t.group != i);
                    g.quarantined = false;
                    g.failures = 0;
                }
                if v.family.is_none() {
                    v.probe = 0;
                }
            }
            "pause" => {
                v.paused = true;
                v.manual = false;
                v.epoch += 1;
                v.baseline.clear();
                for g in &mut v.groups {
                    g.manual_pending = false;
                }
            }
            "resume" => {
                v.paused = false;
                v.epoch += 1;
                v.baseline.clear();
                for g in &mut v.groups {
                    g.due = Some(Instant::now());
                    g.quarantined = false;
                }
            }
            "navigation" => {
                let n = &p["state"];
                if !n.is_object() || serde_json::to_vec(n).unwrap().len() > 2048 {
                    return Err("导航状态无效".into());
                }
                for key in ["interfaceId", "diskId"] {
                    if let Some(id) = n[key].as_str() {
                        if id.len() > 256 {
                            return Err("设备标识过长".into());
                        }
                        v.navigation[key] = json!(id);
                    }
                }
                if let Some(top) = n["scrollTop"]
                    .as_f64()
                    .filter(|n| n.is_finite() && *n >= 0.0 && *n <= 1000000.0)
                {
                    v.navigation["scrollTop"] = json!(top);
                }
                if let Some(a) = n["expandedGroups"].as_array().filter(|a| a.len() <= 8) {
                    v.navigation["expandedGroups"] = json!(a
                        .iter()
                        .filter(|v| v.as_str().is_some_and(|s| [
                            "device",
                            "collection",
                            "disks",
                            "memory",
                            "other-mounts"
                        ]
                        .contains(&s)))
                        .collect::<Vec<_>>());
                }
            }
            _ => return Err("未知系统状态操作".into()),
        }
        v.revision += 1;
        if ["pause", "resume"].contains(&action) {
            v.publisher.observe(
                &v.data.mounts,
                timestamp(),
                v.groups[2].error.as_deref(),
                v.paused,
            );
        }
        Ok(json!({"accepted":true,"revision":v.revision}))
    }
    pub fn list(&self, id: &str, p: &Value) -> std::result::Result<Value, String> {
        let v = self.views.get(id).ok_or("视图已关闭")?;
        let kind = p["kind"]
            .as_str()
            .filter(|s| ["interfaces", "disks", "filesystems"].contains(s))
            .ok_or("未知列表")?;
        if p["expectedRevision"]
            .as_u64()
            .is_some_and(|r| r != v.revision)
        {
            return Err("列表修订已过期".into());
        }
        let rows = if kind == "filesystems" && !p["group"].is_null() {
            let group = p["group"]
                .as_str()
                .filter(|g| ["overview", "primary", "auxiliary", "system"].contains(g))
                .ok_or("未知文件系统分组")?;
            crate::filesystem::select(crate::filesystem::project(&v.data.mounts), group)
        } else {
            match kind {
                "interfaces" => serde_json::to_value(if v.family == Some(Family::Windows) {
                    &v.data.counter_interfaces.value
                } else {
                    &v.data.interfaces.value
                })
                .unwrap()
                .as_array()
                .cloned()
                .unwrap_or_default(),
                "disks" => serde_json::to_value(&v.data.disks.value)
                    .unwrap()
                    .as_array()
                    .cloned()
                    .unwrap_or_default(),
                _ => {
                    let mut rows = v.data.mounts.rows();
                    for row in &mut rows {
                        row.as_object_mut().unwrap().remove("mountIdentity");
                    }
                    rows
                }
            }
        };
        let start = p["offset"].as_u64().unwrap_or(0) as usize;
        if start > rows.len() {
            return Err("列表游标已过期".into());
        }
        let mut page = Vec::new();
        let mut bytes = 0;
        for row in rows.iter().skip(start).take(32) {
            let n = serde_json::to_vec(row).unwrap().len();
            if bytes + n > 20000 {
                if page.is_empty() {
                    return Err("单项列表超过响应预算".into());
                }
                break;
            }
            bytes += n;
            page.push(row.clone());
        }
        Ok(
            json!({"revision":v.revision,"listRevision":crate::filesystem::revision(&rows),"items":page,"total":rows.len(),"offset":start,"nextOffset":(start+page.len()<rows.len()).then_some(start+page.len())}),
        )
    }
    fn take_work(&mut self) -> Option<Work> {
        let now = Instant::now();
        let wall = SystemTime::now();
        let mono = now.duration_since(self.last_tick).as_secs_f64();
        let clock = wall
            .duration_since(self.last_wall)
            .map(|d| d.as_secs_f64())
            .unwrap_or(-1.0);
        if mono > 3.0 || (mono - clock).abs() > 2.0 {
            for v in self.views.values_mut() {
                v.baseline.clear();
                v.groups[4].due = Some(now);
                v.revision += 1;
            }
        }
        self.last_tick = now;
        self.last_wall = wall;
        let ids: Vec<_> = self.views.keys().cloned().collect();
        for _ in 0..ids.len() {
            let id = ids[self.round % ids.len()].clone();
            self.round = self.round.wrapping_add(1);
            let v = self.views.get_mut(&id).unwrap();
            if v.pending {
                continue;
            }
            let make = |v: &mut View, kind, method, params| {
                v.pending = true;
                Work {
                    view: id.clone(),
                    epoch: v.epoch,
                    kind,
                    method,
                    params,
                }
            };
            if v.paused && !v.manual && v.ticket.as_ref().is_some_and(|t| t.id.is_some()) {
                let t = v.ticket.take().unwrap();
                if let Some(task) = t.id {
                    return Some(make(
                        v,
                        Kind::Cancel,
                        "session.exec.release",
                        json!({"taskId":task}),
                    ));
                }
            }
            if v.state_due <= now {
                v.state_due = now + Duration::from_secs(5);
                return Some(make(
                    v,
                    Kind::State,
                    "session.target.describe",
                    json!({"viewId":id,"expectedContext":v.context}),
                ));
            }
            if let Some(t) = &v.ticket {
                if t.poll_due > now {
                    continue;
                }
                return Some(if let Some(task) = &t.id {
                    make(
                        v,
                        Kind::Poll,
                        "session.exec.poll",
                        json!({"taskId":task,"stdoutOffset":t.a,"stderrOffset":t.b}),
                    )
                } else {
                    make(v, Kind::Start, "session.exec.start", t.params.clone())
                });
            }
            if v.connected
                && (v.paused
                    || !v
                        .groups
                        .iter()
                        .any(|g| g.due.is_some_and(|at| at <= now) || g.manual_pending))
            {
                if let Some(job) = v.publisher.next() {
                    let (kind, method) = if job.revision.is_none() {
                        (Kind::SharedGet, "state.get")
                    } else if job.value.is_some() {
                        (Kind::SharedPut, "state.put")
                    } else {
                        (Kind::SharedRemove, "state.remove")
                    };
                    let params = json!({"viewId":id,"expectedContext":v.context,"topic":job.key.0,"key":job.key.1,"expectedRevision":job.revision,"value":job.value});
                    return Some(make(v, kind, method, params));
                }
            }
            if !v.connected || (v.paused && !v.manual) {
                continue;
            }
            let active = self.views.values().filter(|v| v.ticket.is_some()).count();
            if active >= 4 {
                continue;
            }
            let v = self.views.get_mut(&id).unwrap();
            let group = if v.family.is_none() {
                if v.probe >= 3 {
                    continue;
                }
                5
            } else {
                let next = v
                    .groups
                    .iter()
                    .enumerate()
                    .filter(|(_, g)| {
                        !g.quarantined
                            && (g.manual_pending || (!v.paused && g.due.is_some_and(|t| t <= now)))
                    })
                    .min_by_key(|(_, g)| if g.manual_pending { Some(now) } else { g.due })
                    .map(|(i, _)| i);
                let Some(i) = next else {
                    v.manual = false;
                    continue;
                };
                i
            };
            if v.family == Some(Family::Unix) && matches!(group, 1 | 3) {
                v.groups[group].due = None;
                v.groups[group].manual_pending = false;
                v.groups[group].error = Some("此系统暂未提供可靠的数据源".into());
                v.revision += 1;
                continue;
            }
            v.sequence += 1;
            if group < GROUPS.len() {
                v.groups[group].manual_pending = false;
            }
            let nonce = format!("n{}e{}", v.sequence, v.epoch);
            let all = v.data.mounts.targets();
            let targets: Vec<_> = if v.family == Some(Family::Linux) {
                all.iter().skip(v.capacity_page).take(16).cloned().collect()
            } else {
                all.clone()
            };
            let paths: Vec<_> = targets.iter().map(|t| t.path.clone()).collect();
            if group == 3 {
                v.capacity_page = if v.capacity_page + 16 >= all.len() {
                    0
                } else {
                    v.capacity_page + 16
                };
            }
            let command = if group == 5 {
                match v.probe {
                    0 => "uname -s".into(),
                    1 => collectors::windows_probe().into(),
                    _ => "sh -c 'printf SODECK_UNIX_1; uname -s'".into(),
                }
            } else {
                collectors::command(v.family.unwrap(), GROUPS[group], &nonce, &paths)
            };
            let params = json!({"viewId":id,"expectedContext":v.context,"operationId":nonce,"sequence":v.sequence.to_string(),"command":command,"timeoutMs":if group==3 {20000}else if group==5 || group==4 {5000}else{10000},"maxOutputBytes":262144});
            v.ticket = Some(Ticket {
                id: None,
                params: params.clone(),
                nonce,
                group,
                stdout: vec![],
                stderr: 0,
                a: "0".into(),
                b: "0".into(),
                paths,
                targets,
                poll_due: now,
            });
            return Some(make(v, Kind::Start, "session.exec.start", params));
        }
        None
    }
    fn complete(&mut self, result: Result, cleanup: &mut VecDeque<Work>) {
        let Result { work, result } = result;
        if matches!(work.kind, Kind::Release) {
            return;
        }
        let Some(v) = self.views.get_mut(&work.view) else {
            return;
        };
        v.pending = false;
        if matches!(work.kind, Kind::Cancel) {
            return;
        }
        if work.epoch != v.epoch {
            if let Ok(r) = &result {
                if let Some(id) = r["taskId"].as_str() {
                    cleanup.push_back(release(&work.view, v.epoch, id));
                }
            }
            // Resolve an ambiguous start with its original ticket before releasing it.
            if v.ticket.as_ref().is_some_and(|t| t.id.is_some())
                || (matches!(work.kind, Kind::Start) && result.is_ok())
            {
                if let Some(t) = v.ticket.take() {
                    if let Some(id) = t.id {
                        cleanup.push_back(release(&work.view, v.epoch, &id));
                    }
                }
            }
            return;
        }
        match work.kind {
            Kind::SharedGet | Kind::SharedPut | Kind::SharedRemove => {
                let p = &work.params;
                v.publisher.complete(
                    crate::shared_state::Job {
                        key: (
                            p["topic"].as_str().unwrap().into(),
                            p["key"].as_str().unwrap().into(),
                        ),
                        revision: p["expectedRevision"].as_str().map(str::to_string),
                        value: if matches!(work.kind, Kind::SharedRemove) {
                            None
                        } else {
                            Some(p["value"].clone())
                        },
                    },
                    result,
                );
            }
            Kind::Release | Kind::Cancel => {}
            Kind::State => {
                if let Ok(r) = result {
                    let connected = r["state"] == "connected" && r["closing"] != true;
                    if r["context"] != v.context {
                        if let Some(t) = v.ticket.take() {
                            if let Some(id) = t.id {
                                cleanup.push_back(release(&work.view, v.epoch, &id));
                            }
                        }
                        v.context = r["context"].clone();
                        v.publisher = crate::shared_state::Publisher::default();
                        v.data = crate::model::StatusState::default();
                        v.epoch += 1;
                        v.family = None;
                        v.probe = 0;
                        v.groups = groups();
                        v.baseline.clear();
                        v.history.clear();
                        v.revision += 1;
                    }
                    if connected != v.connected {
                        v.connected = connected;
                        v.baseline.clear();
                        v.revision += 1;
                        if !connected {
                            if let Some(t) = v.ticket.take() {
                                if let Some(id) = t.id {
                                    cleanup.push_back(release(&work.view, v.epoch, &id));
                                }
                            }
                        }
                    }
                }
            }
            Kind::Start => match result {
                Ok(r) => {
                    if let Some(t) = &mut v.ticket {
                        t.id = r["taskId"].as_str().map(str::to_string);
                        t.poll_due = Instant::now() + Duration::from_millis(100);
                    }
                }
                Err(e) => {
                    let code = e.data["code"].as_str().unwrap_or("");
                    if ["staleContext", "workspaceClosing"].contains(&code) {
                        v.ticket = None;
                        v.connected = false;
                        v.state_due = Instant::now();
                    } else if ["resourceBusy", ""].contains(&code) {
                        if let Some(t) = &mut v.ticket {
                            t.poll_due = Instant::now() + Duration::from_secs(1);
                        }
                    } else {
                        fail_ticket(v, &e.message, code == "operationExpired");
                    }
                }
            },
            Kind::Poll => match result {
                Err(e) => {
                    if e.data["code"] == "taskExpired" {
                        fail_ticket(v, "采集任务已回收，刷新可重试", true);
                    } else if let Some(t) = &mut v.ticket {
                        t.poll_due = Instant::now() + Duration::from_millis(500);
                    }
                }
                Ok(r) => {
                    let Some(t) = &mut v.ticket else { return };
                    let bytes = sodeck_plugin_client::encoding::unbase64(
                        r["stdout"]["dataBase64"].as_str().unwrap_or(""),
                    );
                    if let Ok(bytes) = bytes {
                        if t.stdout.len() + bytes.len() > 262144 {
                            fail_ticket(v, "采集输出超过限制", true);
                            return;
                        }
                        t.stdout.extend(bytes);
                    } else {
                        fail_ticket(v, "采集输出编码无效", true);
                        return;
                    }
                    t.stderr += r["stderr"]["nextOffset"]
                        .as_str()
                        .and_then(|s| s.parse::<usize>().ok())
                        .unwrap_or(0)
                        .saturating_sub(t.b.parse::<usize>().unwrap_or(0));
                    t.a = r["stdout"]["nextOffset"].as_str().unwrap_or("0").into();
                    t.b = r["stderr"]["nextOffset"].as_str().unwrap_or("0").into();
                    t.poll_due = Instant::now() + Duration::from_millis(250);
                    if r["outputComplete"] != true {
                        return;
                    }
                    let t = v.ticket.take().unwrap();
                    if let Some(id) = &t.id {
                        cleanup.push_back(release(&work.view, v.epoch, id));
                    }
                    let text =
                        String::from_utf8(t.stdout).map_err(|_| "采集输出不是 UTF-8".to_string());
                    if t.group == 5 {
                        let family = text
                            .as_ref()
                            .ok()
                            .and_then(|s| collectors::detect(s))
                            .or_else(|| {
                                text.as_ref()
                                    .ok()
                                    .filter(|s| s.lines().any(|l| l == "SODECK_WINDOWS_1"))
                                    .map(|_| Family::Windows)
                            })
                            .or_else(|| {
                                text.as_ref()
                                    .ok()
                                    .filter(|s| s.contains("SODECK_UNIX_1"))
                                    .map(|_| Family::Unix)
                            });
                        if let Some(family) = family {
                            v.family = Some(family);
                            v.data.family = Some(family);
                            v.groups = groups();
                            if v.manual {
                                for g in &mut v.groups {
                                    g.manual_pending = true;
                                }
                            }
                            cleanup.push_back(Work{view:work.view.clone(),epoch:v.epoch,kind:Kind::Release,method:"logging.write",params:json!({"level":"info","message":format!("系统状态采用 {} 采集路径",family.name())})});
                        } else {
                            v.probe += 1;
                            if v.probe >= 3 {
                                v.data.collection_error =
                                    Some("此登录环境不支持只读命令采集，可刷新重试".into());
                            }
                        }
                        v.revision += 1;
                        return;
                    }
                    let now = Instant::now();
                    let observed = timestamp();
                    let decoded = if r["state"] == "completed" {
                        text.and_then(|text| {
                            collectors::decode(
                                v.family.unwrap(),
                                GROUPS[t.group],
                                &text,
                                &t.nonce,
                                &collectors::DecodeContext {
                                    total: v.data.identity.value.as_ref().and_then(|v| v.total),
                                    interfaces: v.data.interfaces.value.as_deref().unwrap_or(&[]),
                                    paths: &t.paths,
                                },
                            )
                        })
                    } else {
                        Err(r["error"]["message"].as_str().unwrap_or("采集失败").into())
                    };
                    let (parsed, summary) = match decoded {
                        Ok(sample) => {
                            let summary = metrics::apply(
                                &mut v.data,
                                &mut v.baseline,
                                sample,
                                &t.targets,
                                observed,
                                now,
                                self.interval,
                            );
                            let status = if summary.successes > 0 {
                                Ok(())
                            } else {
                                Err(summary
                                    .failures
                                    .first()
                                    .cloned()
                                    .unwrap_or("此数据组没有可用的数据源".into()))
                            };
                            (status, summary)
                        }
                        Err(error) => {
                            let summary = metrics::failed(
                                &mut v.data,
                                &mut v.baseline,
                                GROUPS[t.group],
                                &error,
                                observed,
                            );
                            (Err(error), summary)
                        }
                    };
                    if t.group == 1 {
                        record_history(v, observed, &summary.points);
                    }
                    let g = &mut v.groups[t.group];
                    match parsed {
                        Ok(()) => {
                            g.success = Some(now);
                            g.observed = Some(observed);
                            g.error = None;
                            g.failures = 0;
                            g.due = next_due(g.due, period(t.group, self.interval), now);
                        }
                        Err(message) => {
                            g.failures = g.failures.saturating_add(1);
                            g.error = Some(metrics::limited(&message, 512));
                            g.quarantined =
                                r["execSent"] == true && r["remoteExitConfirmed"] != true;
                            g.due = if g.quarantined || (t.group == 0 && g.failures >= 3) {
                                None
                            } else {
                                Some(
                                    now + Duration::from_secs(
                                        period(t.group, self.interval)
                                            .unwrap_or(5)
                                            .max([5, 10, 30, 60][(g.failures as usize - 1).min(3)]),
                                    ),
                                )
                            };
                        }
                    }
                    if [2, 3].contains(&t.group) {
                        v.publisher.observe(
                            &v.data.mounts,
                            observed,
                            v.groups[2].error.as_deref(),
                            v.paused,
                        );
                    }
                    v.revision += 1;
                }
            },
        }
    }
}
fn selected_device(rows: &[crate::model::Device], selected: Option<&str>, network: bool) -> Value {
    if let Some(id) = selected {
        return rows
            .iter()
            .find(|d| d.id == id)
            .map(|d| json!(d))
            .unwrap_or_else(|| json!({"id":id,"name":"已移除的设备","removed":true}));
    }
    let rank = |d: &crate::model::Device| {
        if !network {
            return 0;
        }
        let loopback = matches!(d.name.as_str(), "lo" | "lo0");
        let up = d.state.as_deref() == Some("up");
        let usable = d.addresses.iter().any(|a| {
            let raw = a
                .split('/')
                .next()
                .unwrap_or(a)
                .split('%')
                .next()
                .unwrap_or(a);
            match raw.parse::<std::net::IpAddr>() {
                Ok(std::net::IpAddr::V4(v)) => {
                    !v.is_loopback() && !v.is_link_local() && !v.is_unspecified()
                }
                Ok(std::net::IpAddr::V6(v)) => {
                    !v.is_loopback() && !v.is_unicast_link_local() && !v.is_unspecified()
                }
                _ => false,
            }
        });
        if loopback {
            5
        } else if up && usable {
            0
        } else if up && !d.addresses.is_empty() {
            1
        } else if up {
            2
        } else {
            3
        }
    };
    rows.iter()
        .min_by_key(|d| (rank(d), d.name.as_str()))
        .map(|d| json!(d))
        .unwrap_or(Value::Null)
}
fn release(view: &str, epoch: u64, id: &str) -> Work {
    Work {
        view: view.into(),
        epoch,
        kind: Kind::Release,
        method: "session.exec.release",
        params: json!({"taskId":id}),
    }
}
fn groups() -> Vec<Group> {
    GROUPS
        .iter()
        .map(|_| Group {
            due: Some(Instant::now()),
            manual_pending: false,
            observed: None,
            success: None,
            error: None,
            failures: 0,
            quarantined: false,
        })
        .collect()
}
pub fn period(group: usize, interval: u64) -> Option<u64> {
    match group {
        0 => None,
        1 => Some(interval),
        2 => Some(30),
        _ => Some(60),
    }
}
fn next_due(due: Option<Instant>, seconds: Option<u64>, now: Instant) -> Option<Instant> {
    let period = Duration::from_secs(seconds?);
    let due = due.unwrap_or(now);
    if due > now {
        return Some(due);
    }
    let cycles = now.duration_since(due).as_nanos() / period.as_nanos() + 1;
    Some(due + period * u32::try_from(cycles).unwrap_or(u32::MAX))
}
fn fail_ticket(v: &mut View, message: &str, quarantine: bool) {
    if let Some(t) = v.ticket.take() {
        if t.group < 5 {
            let at = timestamp();
            let outcome =
                metrics::failed(&mut v.data, &mut v.baseline, GROUPS[t.group], message, at);
            if t.group == 1 {
                record_history(v, at, &outcome.points);
            }
            let g = &mut v.groups[t.group];
            g.error = Some(metrics::limited(message, 512));
            g.quarantined = quarantine;
            g.due = (!quarantine).then(|| Instant::now() + Duration::from_secs(5));
        } else {
            v.probe = 3;
            v.data.collection_error = Some(message.into());
        }
    }
    v.revision += 1;
}
fn record_history(v: &mut View, at: u64, points: &BTreeMap<String, Option<f64>>) {
    let selected = v.navigation["interfaceId"]
        .as_str()
        .map(|id| format!("net:{id}:"));
    let mut points: Vec<_> = points.iter().collect();
    points.sort_by_key(|(key, _)| {
        (
            if key.as_str() == "cpu" || key.as_str() == "memory" {
                0
            } else if selected.as_ref().is_some_and(|s| key.starts_with(s)) {
                1
            } else {
                2
            },
            key.as_str(),
        )
    });
    points.truncate(130);
    for (key, value) in points {
        let history = v.history.entry(key.clone()).or_default();
        history.push_back(Point {
            at,
            value: value.filter(|v| v.is_finite()),
        });
        while history.len() > 100 || history.front().is_some_and(|v| v.at + 300000 < at) {
            history.pop_front();
        }
    }
    while v.history.len() > 130 {
        if let Some(key) = v
            .history
            .keys()
            .find(|k| *k != "cpu" && *k != "memory")
            .cloned()
        {
            v.history.remove(&key);
        }
    }
}
fn value_size(v: &Value) -> usize {
    std::mem::size_of::<Value>()
        + match v {
            Value::String(s) => s.capacity(),
            Value::Array(a) => {
                a.capacity() * std::mem::size_of::<Value>()
                    + a.iter().map(value_size).sum::<usize>()
            }
            Value::Object(o) => o
                .iter()
                .map(|(k, v)| 128 + k.capacity() + value_size(v))
                .sum(),
            _ => 0,
        }
}
fn cache_size(v: &View) -> usize {
    v.publisher.bytes()
        + v.data.bytes()
        + value_size(&v.navigation)
        + v.baseline.memory_usage()
        + v.history
            .iter()
            .map(|(key, h)| key.capacity() + 96 + h.capacity() * std::mem::size_of::<Point>())
            .sum::<usize>()
}
fn shrink(v: &mut View) -> bool {
    let mut changed = false;
    for h in v.history.values_mut() {
        if !h.is_empty() {
            let remove = h.len().div_ceil(2);
            h.drain(..remove);
            h.shrink_to_fit();
            changed = true;
        }
    }
    if !changed {
        changed = v.data.mounts.trim();
        for source in [
            &mut v.data.interfaces,
            &mut v.data.counter_interfaces,
            &mut v.data.disks,
        ] {
            if let Some(rows) = source.value.as_mut() {
                if !rows.is_empty() {
                    rows.truncate(rows.len() / 2);
                    changed = true;
                }
            }
        }
        if v.baseline.memory_usage() > 0 {
            v.baseline.clear();
            changed = true;
        }
    }
    if changed {
        v.data.cache_limited = true;
        v.revision += 1;
    }
    changed
}
pub fn start(app: Arc<Mutex<App>>, client: Client) {
    let (tx, rx) = mpsc::sync_channel::<Work>(32);
    let rx = Arc::new(Mutex::new(rx));
    let (done, results) = mpsc::sync_channel(32);
    for _ in 0..4 {
        let rx = rx.clone();
        let done = done.clone();
        let c = client.clone();
        std::thread::spawn(move || loop {
            let Ok(work) = rx.lock().unwrap().recv() else {
                break;
            };
            let result = (|| {
                let invalid = |_| RpcError::invalid("状态采集请求参数无效");
                let params = work.params.clone();
                match work.method {
                    "session.target.describe" => c
                        .request(api::call::Target(
                            serde_json::from_value(params).map_err(invalid)?,
                        ))
                        .map(|reply| json!(reply)),
                    "session.exec.start" => c
                        .request(api::call::ExecStart(
                            serde_json::from_value(params).map_err(invalid)?,
                        ))
                        .map(|reply| json!(reply)),
                    "session.exec.poll" => c
                        .request(api::call::ExecPoll(
                            serde_json::from_value(params).map_err(invalid)?,
                        ))
                        .map(|reply| json!(reply)),
                    "session.exec.release" => c.request(api::call::ExecRelease(
                        serde_json::from_value(params).map_err(invalid)?,
                    )),
                    "state.get" => c.request(api::call::StateGet(
                        serde_json::from_value(params).map_err(invalid)?,
                    )),
                    "state.put" => c.request(api::call::StatePut(
                        serde_json::from_value(params).map_err(invalid)?,
                    )),
                    "state.remove" => c.request(api::call::StateRemove(
                        serde_json::from_value(params).map_err(invalid)?,
                    )),
                    "logging.write" => c.request(api::call::Log(
                        serde_json::from_value(params).map_err(invalid)?,
                    )),
                    _ => unreachable!(),
                }
            })();
            if done.send(Result { work, result }).is_err() {
                break;
            }
        });
    }
    std::thread::spawn(move || {
        let mut cleanup = VecDeque::new();
        while !client.is_closed() {
            {
                let mut a = app.lock().unwrap();
                while let Ok(r) = results.try_recv() {
                    a.complete(r, &mut cleanup);
                }
                a.enforce_budget();
                while cleanup.len() > 32 {
                    cleanup.pop_front();
                }
            }
            for _ in 0..8 {
                let work = cleanup
                    .pop_front()
                    .or_else(|| app.lock().unwrap().take_work());
                let Some(work) = work else { break };
                if let Err(mpsc::TrySendError::Full(work)) = tx.try_send(work) {
                    if let Some(v) = app.lock().unwrap().views.get_mut(&work.view) {
                        v.pending = false;
                    }
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    });
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn network_selection_prefers_active_routable_addresses_and_keeps_explicit_identity() {
        use crate::model::Device;
        let rows = vec![
            Device {
                id: "gif".into(),
                name: "gif0".into(),
                state: Some("down".into()),
                ..Device::default()
            },
            Device {
                id: "awdl".into(),
                name: "awdl0".into(),
                state: Some("up".into()),
                addresses: vec!["fe80::1%awdl0/64".into()],
                ..Device::default()
            },
            Device {
                id: "en".into(),
                name: "en0".into(),
                state: Some("up".into()),
                addresses: vec!["192.0.2.1/24".into(), "2001:db8::1/64".into()],
                ..Device::default()
            },
        ];
        assert_eq!(selected_device(&rows, None, true)["id"], "en");
        assert_eq!(selected_device(&rows, Some("gif"), true)["id"], "gif");
        assert_eq!(
            selected_device(&rows, Some("removed"), true)["removed"],
            true
        );
    }
    #[test]
    fn dynamic_setting_does_not_speed_up_slow_groups() {
        assert_eq!(period(1, 3), Some(3));
        assert_eq!(period(2, 3), Some(30));
        assert_eq!(period(3, 3), Some(60));
        assert_eq!(period(0, 3), None);
    }
    #[test]
    fn repeated_mount_pause_and_bounded_history() {
        let mut a = App::new(5);
        a.mount("v", &json!({})).unwrap();
        a.mount("v", &json!({})).unwrap();
        assert_eq!(a.views.len(), 1);
        a.control("v", "pause", &json!({})).unwrap();
        assert!(a.views["v"].paused);
        for i in 0..4000 {
            record_history(
                a.views.get_mut("v").unwrap(),
                i * 5000,
                &BTreeMap::from([("cpu".into(), None), ("memory".into(), None)]),
            );
        }
        assert!(a.views["v"].history["cpu"].len() <= 61);
        a.control("v", "refresh", &json!({})).unwrap();
        assert!(a.views["v"].paused);
    }
    #[test]
    fn sampling_keeps_its_phase_and_skips_missed_cycles() {
        let phase = Instant::now();
        assert_eq!(
            next_due(Some(phase), Some(30), phase + Duration::from_secs(37)),
            Some(phase + Duration::from_secs(60))
        );
        assert_eq!(
            next_due(
                Some(phase + Duration::from_secs(60)),
                Some(30),
                phase + Duration::from_secs(40)
            ),
            Some(phase + Duration::from_secs(60))
        );
        assert_eq!(next_due(Some(phase), None, phase), None);
    }
    #[test]
    fn pause_resolves_an_ambiguous_start_before_releasing_it() {
        let mut a = App::new(5);
        a.mount("v", &json!({})).unwrap();
        let v = a.views.get_mut("v").unwrap();
        v.connected = true;
        v.family = Some(Family::Linux);
        v.state_due = Instant::now() + Duration::from_secs(60);
        let start = a.take_work().unwrap();
        let params = start.params.clone();
        a.control("v", "pause", &json!({})).unwrap();
        let mut cleanup = VecDeque::new();
        a.complete(
            Result {
                work: start,
                result: Err(RpcError {
                    code: -32003,
                    message: "timeout".into(),
                    data: Value::Null,
                }),
            },
            &mut cleanup,
        );
        let retry = a.take_work().unwrap();
        assert_eq!(retry.params, params);
        a.complete(
            Result {
                work: retry,
                result: Ok(json!({"taskId":"task"})),
            },
            &mut cleanup,
        );
        assert!(matches!(a.take_work().unwrap().kind, Kind::Cancel));
    }
    #[test]
    fn pause_during_running_task_resumes_after_cancel_reply() {
        let mut a = App::new(5);
        a.mount("v", &json!({})).unwrap();
        let v = a.views.get_mut("v").unwrap();
        v.connected = true;
        v.family = Some(Family::Linux);
        v.state_due = Instant::now() + Duration::from_secs(60);
        let start = a.take_work().unwrap();
        let mut cleanup = VecDeque::new();
        a.complete(
            Result {
                work: start,
                result: Ok(json!({"taskId":"task"})),
            },
            &mut cleanup,
        );
        a.control("v", "pause", &json!({})).unwrap();
        let cancel = a.take_work().unwrap();
        assert!(matches!(cancel.kind, Kind::Cancel));
        a.complete(
            Result {
                work: cancel,
                result: Ok(json!({})),
            },
            &mut cleanup,
        );
        assert!(!a.views["v"].pending);
        a.control("v", "resume", &json!({})).unwrap();
        assert!(matches!(a.take_work().unwrap().kind, Kind::Start));
    }
    #[test]
    fn manual_refresh_merges_and_preserves_slow_periods() {
        let mut a = App::new(5);
        a.mount("v", &json!({})).unwrap();
        let v = a.views.get_mut("v").unwrap();
        v.connected = true;
        v.family = Some(Family::Linux);
        v.state_due = Instant::now() + Duration::from_secs(60);
        let start = a.take_work().unwrap();
        let due = a.views["v"].groups[2].due;
        for _ in 0..10 {
            a.control("v", "refresh", &json!({})).unwrap();
        }
        assert_eq!(a.views["v"].groups[2].due, due);
        assert!(!a.views["v"].groups[0].manual_pending);
        assert_eq!(
            a.views["v"]
                .groups
                .iter()
                .filter(|g| g.manual_pending)
                .count(),
            4
        );
        assert!(matches!(start.kind, Kind::Start));
        a.control("v", "pause", &json!({})).unwrap();
        assert!(a.views["v"].groups.iter().all(|g| !g.manual_pending));
    }
    #[test]
    fn refresh_while_paused_collects_each_group_once_even_when_overdue() {
        let mut a = App::new(3);
        a.mount("v", &json!({})).unwrap();
        let v = a.views.get_mut("v").unwrap();
        v.connected = true;
        v.family = Some(Family::Linux);
        v.paused = true;
        v.state_due = Instant::now() + Duration::from_secs(60);
        a.control("v", "refresh", &json!({})).unwrap();
        let mut collected = Vec::new();
        for _ in 0..5 {
            let work = a.take_work().unwrap();
            assert!(matches!(work.kind, Kind::Start));
            let v = a.views.get_mut("v").unwrap();
            collected.push(v.ticket.take().unwrap().group);
            v.pending = false;
        }
        assert_eq!(collected, vec![0, 1, 2, 3, 4]);
        assert!(a.take_work().is_none());
        assert!(a.views["v"].paused && !a.views["v"].manual);
    }
    #[test]
    fn many_workspaces_trim_the_cache_budget_and_keep_current_values() {
        let mut app = App::new(5);
        for i in 0..40 {
            let id = i.to_string();
            app.mount(&id, &json!({})).unwrap();
            let v = app.views.get_mut(&id).unwrap();
            v.data = crate::model::StatusState::fixture(
                &json!({"filesystems":(0..32).map(|i|json!({"path":format!("/{i}{}","x".repeat(65500))})).collect::<Vec<_>>(),"cpu":{"value":23.0}}),
            );
        }
        app.enforce_budget();
        assert!(app.views.values().map(cache_size).sum::<usize>() <= 64 * 1024 * 1024);
        assert!(app.views.values().all(|v| cache_size(v) <= 2 * 1024 * 1024
            && v.data.cpu.value.as_ref().is_some_and(|g| g.value == 23.0)));
    }
    #[test]
    fn filesystem_overview_does_not_warn_on_snap_images() {
        let mut app = App::new(5);
        app.mount("v", &json!({})).unwrap();
        let v = app.views.get_mut("v").unwrap();
        let mut fixture = json!({"filesystems":[{"id":"/dev/sda1","path":"/","total":"1000","used":"200","available":"800"},{"id":"/dev/loop0","path":"/snap/core20/2318","total":"1000","used":"1000","available":"0"}]});
        fixture["mountTopology"]=crate::collectors::test_mounts("1 0 8:1 / / rw - ext4 /dev/sda1 rw\n2 1 7:0 / /snap/core20/2318 ro - squashfs /dev/loop0 ro\n");
        v.data = crate::model::StatusState::fixture(&fixture);
        let overview = app
            .list("v", &json!({"kind":"filesystems","group":"overview"}))
            .unwrap();
        assert_eq!(
            overview["total"], 1,
            "Snap images must not fill the overview"
        );
        assert_eq!(
            app.snapshot("v", None).unwrap()["filesystemSummary"]["warnings"],
            0
        );
    }
    #[test]
    fn filesystem_classification_precedes_pagination_and_cursors_are_bounded() {
        let mut app = App::new(5);
        app.mount("v", &json!({})).unwrap();
        let v = app.views.get_mut("v").unwrap();
        let mut fixture = crate::filesystem::tests::screenshot();
        for i in 0..40 {
            fixture["filesystems"].as_array_mut().unwrap().push(json!({"id":i.to_string(),"path":format!("/mnt/{i:02}"),"total":"1000","used":"900","available":"100"}));
        }
        v.data = crate::model::StatusState::fixture(&fixture);
        let raw = app.list("v", &json!({"kind":"filesystems"})).unwrap();
        assert_eq!(raw["total"], 54);
        let first = app
            .list("v", &json!({"kind":"filesystems","group":"overview"}))
            .unwrap();
        assert_eq!(first["total"], 41);
        assert_eq!(first["items"].as_array().unwrap().len(), 32);
        assert_eq!(first["nextOffset"], 32);
        let second = app
            .list(
                "v",
                &json!({"kind":"filesystems","group":"overview","offset":32}),
            )
            .unwrap();
        assert_eq!(second["items"].as_array().unwrap().len(), 9);
        assert_eq!(first["listRevision"], second["listRevision"]);
        assert!(second["nextOffset"].is_null());
        assert_eq!(
            app.snapshot("v", None).unwrap()["filesystemSummary"]["warnings"],
            40
        );
        assert!(app
            .list("v", &json!({"kind":"filesystems","group":"wrong"}))
            .is_err());
        assert!(app
            .list("v", &json!({"kind":"filesystems","expectedRevision":0}))
            .is_err());
        app.views.get_mut("v").unwrap().data = crate::model::StatusState::fixture(
            &json!({"filesystems":(0..6).map(|i|json!({"id":i.to_string(),"path":format!("/{i}{}","x".repeat(4090)),"total":"1000","used":"900","available":"100"})).collect::<Vec<_>>()}),
        );
        let limited = app
            .list("v", &json!({"kind":"filesystems","group":"primary"}))
            .unwrap();
        let n = limited["items"].as_array().unwrap().len();
        assert!(n > 0 && n < 6);
        assert_eq!(limited["nextOffset"], n);
        assert!(serde_json::to_vec(&limited).unwrap().len() < 24 * 1024);
        app.views
            .get_mut("v")
            .unwrap()
            .data
            .mounts
            .entries
            .values_mut()
            .next()
            .unwrap()
            .mount
            .path = "x".repeat(25000);
        assert!(app.list("v", &json!({"kind":"filesystems"})).is_err());
        app.control(
            "v",
            "navigation",
            &json!({"state":{"expandedGroups":["other-mounts","invalid"]}}),
        )
        .unwrap();
        assert_eq!(
            app.snapshot("v", None).unwrap()["navigation"]["expandedGroups"],
            json!(["other-mounts"])
        );
    }
}

use crate::{
    collectors::{self, Family},
    metrics::{self, Baseline},
};
use serde::Serialize;
use serde_json::{json, Value};
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
    pub data: Value,
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
    poll_due: Instant,
}
pub struct App {
    pub shared_enabled: bool,
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
            shared_enabled: false,
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
            self.views.insert(id.into(),View{publisher:crate::shared_state::Publisher::default(),context:context.clone(),paused:false,connected:false,revision:1,data:json!({"identity":{},"cpu":{"value":null,"state":"warmingUp"},"memory":{"value":null,"state":"unsupported"},"rates":{},"interfaces":[],"disks":[],"filesystems":[],"mountPaths":[]}),navigation:json!({}),groups:groups(),history:BTreeMap::new(),family:None,epoch:1,sequence:0,state_due:Instant::now(),pending:false,ticket:None,probe:0,baseline:Baseline::default(),manual:false,capacity_page:0});
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
        let mut data = v.data.clone();
        for key in [
            "interfaces",
            "counterInterfaces",
            "disks",
            "filesystems",
            "mountPaths",
            "mountTopology",
            "rates",
        ] {
            data.as_object_mut().unwrap().remove(key);
        }
        let interfaces = if v.family == Some(Family::Windows) {
            &v.data["counterInterfaces"]
        } else {
            &v.data["interfaces"]
        };
        let selected = |rows: &Value, key: &str| {
            if v.navigation[key].is_string() {
                return rows
                    .as_array()
                    .and_then(|a| a.iter().find(|r| v.navigation[key] == r["id"]))
                    .cloned()
                    .unwrap_or_else(
                        || json!({"id":v.navigation[key],"name":"已移除的设备","removed":true}),
                    );
            }
            rows.as_array()
                .and_then(|a| {
                    a.iter()
                        .find(|r| v.navigation[key] == r["id"])
                        .or_else(|| a.iter().find(|r| r["name"] != "lo" && r["name"] != "lo0"))
                        .or_else(|| a.first())
                })
                .cloned()
                .unwrap_or(Value::Null)
        };
        let network = selected(interfaces, "interfaceId");
        let disk = selected(&v.data["disks"], "diskId");
        let rates = |prefix: &str, row: &Value| {
            v.data["rates"][format!("{prefix}:{}", row["id"].as_str().unwrap_or(""))].clone()
        };
        Ok(
            json!({"revision":v.revision,"context":v.context,"paused":v.paused,"connected":v.connected,"collecting":v.ticket.is_some(),"family":v.family.map(Family::name),"intervalSeconds":self.interval,"data":data,"groups":group,"navigation":v.navigation,"network":{"device":network,"rates":rates("net",&network)},"disk":{"device":disk,"rates":rates("disk",&disk)},"filesystemTotal":v.data["filesystems"].as_array().map(Vec::len).unwrap_or(0)}),
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
                v.baseline.previous = None;
                for g in &mut v.groups {
                    g.manual_pending = false;
                }
            }
            "resume" => {
                v.paused = false;
                v.epoch += 1;
                v.baseline.previous = None;
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
                            "memory"
                        ]
                        .contains(&s)))
                        .collect::<Vec<_>>());
                }
            }
            _ => return Err("未知系统状态操作".into()),
        }
        v.revision += 1;
        if self.shared_enabled && ["pause", "resume"].contains(&action) {
            v.publisher
                .observe(&v.data, timestamp(), v.groups[2].error.as_deref(), v.paused);
        }
        Ok(json!({"accepted":true,"revision":v.revision}))
    }
    pub fn list(&self, id: &str, p: &Value) -> std::result::Result<Value, String> {
        let v = self.views.get(id).ok_or("视图已关闭")?;
        let kind = p["kind"]
            .as_str()
            .filter(|s| ["interfaces", "disks", "filesystems"].contains(s))
            .ok_or("未知列表")?;
        let key = if kind == "interfaces" && v.family == Some(Family::Windows) {
            "counterInterfaces"
        } else {
            kind
        };
        let rows = v.data[key].as_array().cloned().unwrap_or_default();
        let start = p["offset"].as_u64().unwrap_or(0) as usize;
        if start > rows.len() {
            return Err("列表游标已过期".into());
        }
        let mut page = Vec::new();
        let mut bytes = 0;
        for row in rows.iter().skip(start).take(32) {
            let n = serde_json::to_vec(row).unwrap().len();
            if bytes + n > 20000 {
                break;
            }
            bytes += n;
            page.push(row.clone());
        }
        Ok(
            json!({"revision":v.revision,"items":page,"total":rows.len(),"offset":start,"nextOffset":(start+page.len()<rows.len()).then_some(start+page.len())}),
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
                v.baseline.previous = None;
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
                return Some(make(v, Kind::State, "session.state", json!({"viewId":id})));
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
            if self.shared_enabled
                && v.connected
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
            let all: Vec<String> = v.data["mountPaths"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect();
            let paths: Vec<_> = all.iter().skip(v.capacity_page).take(16).cloned().collect();
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
                        v.data["mountTopology"] = Value::Null;
                        v.data["filesystems"] = json!([]);
                        v.epoch += 1;
                        v.family = None;
                        v.probe = 0;
                        v.groups = groups();
                        v.baseline.previous = None;
                        v.history.clear();
                        v.revision += 1;
                    }
                    if connected != v.connected {
                        v.connected = connected;
                        v.baseline.previous = None;
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
                            v.data["capabilities"] = collectors::initial_capabilities(family);
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
                                v.data["collectionError"] =
                                    json!("此登录环境不支持只读命令采集，可刷新重试");
                            }
                        }
                        v.revision += 1;
                        return;
                    }
                    let now = Instant::now();
                    let observed = timestamp();
                    let parsed = if r["state"] == "completed" {
                        text.and_then(|text| {
                            metrics::apply(
                                v.family.unwrap(),
                                GROUPS[t.group],
                                &text,
                                &t.nonce,
                                &t.paths,
                                &mut v.data,
                                &mut v.baseline,
                                self.interval,
                            )
                        })
                    } else {
                        Err(r["error"]["message"].as_str().unwrap_or("采集失败").into())
                    };
                    let g = &mut v.groups[t.group];
                    match parsed {
                        Ok(()) => {
                            if (t.group == 4 || (t.group == 1 && v.family == Some(Family::Linux)))
                                && v.data["uptime"]["seconds"].is_number()
                            {
                                v.data["uptime"]["observedAt"] = json!(observed);
                            }
                            g.success = Some(now);
                            g.observed = Some(observed);
                            g.error = None;
                            g.failures = 0;
                            g.due = next_due(g.due, period(t.group, self.interval), now);
                            if t.group == 1 {
                                record_history(v, observed);
                            }
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
                    if self.shared_enabled && [2, 3].contains(&t.group) {
                        v.publisher.observe(
                            &v.data,
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
            let g = &mut v.groups[t.group];
            g.error = Some(metrics::limited(message, 512));
            g.quarantined = quarantine;
            g.due = (!quarantine).then(|| Instant::now() + Duration::from_secs(5));
        } else {
            v.probe = 3;
            v.data["collectionError"] = json!(message);
        }
    }
    v.revision += 1;
}
fn record_history(v: &mut View, at: u64) {
    let mut points = vec![
        ("cpu".to_string(), v.data["cpu"]["value"].clone()),
        ("memory".to_string(), v.data["memory"]["value"].clone()),
    ];
    if let Some(rates) = v.data["rates"].as_object() {
        for (key, rate) in rates.iter().take(64) {
            points.push((format!("{key}:first"), rate["first"].clone()));
            points.push((format!("{key}:second"), rate["second"].clone()));
        }
    }
    for (key, value) in points {
        let history = v.history.entry(key).or_default();
        history.push_back(Point {
            at,
            value: value.as_f64().filter(|v| v.is_finite()),
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
        + value_size(&v.data)
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
        for key in ["filesystems", "interfaces", "counterInterfaces", "disks"] {
            if let Some(a) = v.data[key].as_array_mut() {
                if !a.is_empty() {
                    a.truncate(a.len() / 2);
                    changed = true;
                }
            }
        }
        if v.baseline.previous.take().is_some() {
            changed = true;
        }
    }
    if changed {
        v.data["cacheLimited"] = json!(true);
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
            let result = c.call_detailed(work.method, work.params.clone());
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
            record_history(a.views.get_mut("v").unwrap(), i * 5000);
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
            v.data["filesystems"] = json!((0..32)
                .map(|_| json!({"path":"x".repeat(65536)}))
                .collect::<Vec<_>>());
            v.data["cpu"] = json!({"value":23.0});
        }
        app.enforce_budget();
        assert!(app.views.values().map(cache_size).sum::<usize>() <= 64 * 1024 * 1024);
        assert!(app
            .views
            .values()
            .all(|v| cache_size(v) <= 2 * 1024 * 1024 && v.data["cpu"]["value"] == 23.0));
    }
}

//! View-owned Docker log sessions. Core resources transport bytes, surfaces render projections.
mod contract;
mod plain;
mod records;
mod source;
mod store;
use crate::runtime;
use contract::*;
use records::Stamp;
use serde_json::{json, Value};
use sodeck_plugin_client::Client;
use source::{Metadata, Purpose, Reader, Selection};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};
use store::{Budget, Record, Store};
fn cli_error(status: &Value) -> Option<String> {
    crate::cli::startup_error(status)
}

enum Work {
    Open(Range),
    Earlier(bool),
    Search(Range, Conditions, bool),
    Latest,
    Previous,
    Stop(bool),
}
struct ViewState {
    store: Store,
    meta: Option<Metadata>,
    selection: Option<Selection>,
    error: Option<String>,
    coverage: Value,
    navigation: Value,
    next_intent: u64,
    receipts: VecDeque<(String, Value)>,
    loading: bool,
    active: bool,
    follow: bool,
    closed: bool,
    focus: Option<Stamp>,
}
struct Session {
    state: Mutex<ViewState>,
    sender: mpsc::SyncSender<(u64, Work)>,
    intent: AtomicU64,
    closed: AtomicBool,
    active: AtomicBool,
}
pub struct Logs {
    sessions: Mutex<BTreeMap<String, Arc<Session>>>,
    app: runtime::Shared,
    client: Client,
    budget: Arc<Budget>,
    ids: Arc<AtomicU64>,
    history: Arc<AtomicUsize>,
}
struct HistoryPermit(Arc<AtomicUsize>);
impl Drop for HistoryPermit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}
struct Batch {
    reader: Reader,
    rows: Vec<Arc<Record>>,
    bytes: usize,
    selection: Selection,
    bookmark: Option<store::Bookmark>,
    last_publish: Instant,
    _permit: HistoryPermit,
    conditions: Option<Conditions>,
    matches: usize,
    context: VecDeque<Arc<Record>>,
    after: usize,
    seen: BTreeSet<u64>,
    segment_end: Stamp,
    segment_start: Stamp,
    started: Instant,
    scanned: u64,
    publish: bool,
}
impl Logs {
    pub fn new(client: Client, app: runtime::Shared) -> Arc<Self> {
        Arc::new(Self {
            sessions: Mutex::new(BTreeMap::new()),
            app,
            client,
            budget: Budget::new(TOTAL_BYTES),
            ids: Arc::new(AtomicU64::new(1)),
            history: Arc::new(AtomicUsize::new(0)),
        })
    }
    fn session(self: &Arc<Self>, view: &str) -> Result<Arc<Session>, String> {
        let mut sessions = self.sessions.lock().unwrap();
        if let Some(s) = sessions.get(view) {
            return Ok(s.clone());
        }
        if !self.app.lock().unwrap().views.contains_key(view) {
            return Err("日志页已关闭".into());
        }
        let (tx, rx) = mpsc::sync_channel(8);
        let session = Arc::new(Session {
            state: Mutex::new(ViewState {
                store: Store::new(self.budget.clone(), self.ids.clone()),
                meta: None,
                selection: None,
                error: None,
                coverage: json!({"state":"waiting"}),
                navigation: json!({}),
                next_intent: 1,
                receipts: VecDeque::new(),
                loading: false,
                active: false,
                follow: false,
                closed: false,
                focus: None,
            }),
            sender: tx,
            intent: AtomicU64::new(0),
            closed: AtomicBool::new(false),
            active: AtomicBool::new(false),
        });
        sessions.insert(view.into(), session.clone());
        let service = self.clone();
        let current = session.clone();
        let view = view.to_owned();
        std::thread::spawn(move || service.run(view, current, rx));
        Ok(session)
    }
    pub fn event(
        self: &Arc<Self>,
        view: &str,
        _target: Value,
        input: &Value,
    ) -> Result<Value, String> {
        let command: Command =
            serde_json::from_value(input.clone()).map_err(|e| format!("日志请求无效：{e}"))?;
        let session = self.session(view)?;
        let mut state = session.state.lock().unwrap();
        let request = match &command {
            Command::Open {
                request_id,
                intent_sequence,
                ..
            }
            | Command::Earlier {
                request_id,
                intent_sequence,
            }
            | Command::Later {
                request_id,
                intent_sequence,
            }
            | Command::Search {
                request_id,
                intent_sequence,
                ..
            } => Some((request_id.clone(), intent_sequence.clone())),
            _ => None,
        };
        let mut next_intent = None;
        if let Some((request, sequence)) = &request {
            if request.is_empty() || request.len() > 128 {
                return Err("日志请求ID无效".into());
            }
            if let Some((_, old)) = state.receipts.iter().find(|(id, _)| id == request) {
                return if old == input {
                    Ok(json!({"accepted":true,"replayed":true}))
                } else {
                    Err("同一日志请求参数不同".into())
                };
            }
            let sequence = sequence.parse::<u64>().map_err(|_| "日志请求序号无效")?;
            if sequence < state.next_intent {
                return Err("旧日志请求已过期".into());
            }
            next_intent = Some(sequence.checked_add(1).ok_or("日志请求序号耗尽")?);
        }
        let work = match command {
            Command::Open { range, .. } => Some(Work::Open(range)),
            Command::Earlier { .. } => Some(Work::Earlier(true)),
            Command::Later { .. } => Some(Work::Earlier(false)),
            Command::Search {
                range,
                conditions,
                resume,
                ..
            } => {
                conditions.validate()?;
                if conditions.query.is_empty() {
                    return Err("请先输入历史搜索关键词".into());
                }
                Some(Work::Search(range, conditions, resume))
            }
            Command::Hold { revision } => {
                state.store.hold_at(revision.as_deref())?;
                None
            }
            Command::Latest => Some(Work::Latest),
            Command::Previous => Some(Work::Previous),
            Command::Stop { all } => Some(Work::Stop(all)),
            Command::Query { conditions } => {
                conditions.validate()?;
                state.store.query(conditions);
                None
            }
            Command::Context { record } => {
                state.store.context(&record)?;
                None
            }
            Command::Navigation { state: navigation } => {
                if serde_json::to_vec(&navigation).unwrap().len() > 8192 {
                    return Err("日志阅读状态超过上限".into());
                }
                state.navigation = navigation;
                None
            }
            Command::ExportBegin {
                record,
                context,
                header,
            } => {
                if header.len() > 8192 {
                    return Err("导出说明超过上限".into());
                }
                state.store.hold();
                state.store.refresh();
                let header = format!(
                    "{header}\n身份：{}\n时段：{}\n覆盖：{}\n条件：{}",
                    state
                        .meta
                        .as_ref()
                        .map(Metadata::value)
                        .unwrap_or(Value::Null),
                    state
                        .selection
                        .as_ref()
                        .map(Selection::value)
                        .unwrap_or(Value::Null),
                    state.coverage,
                    serde_json::to_string(&state.store.conditions).unwrap()
                );
                return state.store.export_begin(record.as_deref(), context, header);
            }
            Command::ExportEnd { token } => {
                state.store.export_end(&token)?;
                None
            }
        };
        if let Some(work) = work {
            if matches!(work, Work::Earlier(_)) && state.active {
                return Err("历史读取尚未结束，请等待或停止后补读".into());
            }
            let generation = session
                .intent
                .load(Ordering::Acquire)
                .checked_add(1)
                .ok_or("日志任务代次耗尽")?;
            session.intent.store(generation, Ordering::Release);
            if session.sender.try_send((generation, work)).is_err() {
                let _ = session.intent.compare_exchange(
                    generation,
                    generation - 1,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                );
                return Err("日志任务队列已满，请稍后重试".into());
            }
            if let Some(next) = next_intent {
                state.next_intent = next;
            }
            state.loading = true;
            state.error = None;
            if let Some((request, _)) = request {
                state.receipts.push_back((request.clone(), input.clone()));
                while state.receipts.len() > 32 {
                    state.receipts.pop_front();
                }
            }
        }
        Ok(json!({"accepted":true}))
    }
    pub fn read(&self, view: &str, query: &Value) -> Result<Value, String> {
        let read: Read =
            serde_json::from_value(query.clone()).map_err(|e| format!("日志只读查询无效：{e}"))?;
        let target = self.app.lock().unwrap().snapshot(view)?;
        let sessions = self.sessions.lock().unwrap();
        if !sessions.contains_key(view) && matches!(read, Read::Status) {
            return Ok(
                json!({"version":2,"ready":false,"nextIntent":"1","total":0,"navigation":{},"targetState":target["state"],"targetError":target["error"]}),
            );
        }
        let session = sessions.get(view).ok_or("日志会话尚未开始")?;
        let state = session.state.lock().unwrap();
        match read {
            Read::Status => {
                let mut value = status(&state, self.budget.used());
                value["targetState"] = target["state"].clone();
                value["targetError"] = target["error"].clone();
                Ok(value)
            }
            Read::Window {
                revision,
                start,
                count,
            } => state.store.window(&revision, start, count),
            Read::Layout { revision, start } => state.store.layout(&revision, start),
            Read::Detail { record, offset } => state.store.detail(&record, offset),
            Read::Navigate { record, direction } => {
                state.store.navigate(record.as_deref(), direction)
            }
            Read::LocateTime { at } => state.store.locate_time(&at),
            Read::Export { token, offset } => state.store.export_read(&token, offset),
        }
    }
    pub fn snapshot(&self, view: &str) -> Result<Value, String> {
        let sessions = self.sessions.lock().unwrap();
        Ok(sessions
            .get(view)
            .map(|s| status(&s.state.lock().unwrap(), self.budget.used()))
            .unwrap_or_else(
                || json!({"version":2,"ready":false,"nextIntent":"1","total":0,"navigation":{}}),
            ))
    }
    pub fn dispose(&self, view: &str) {
        if let Some(session) = self.sessions.lock().unwrap().remove(view) {
            session.closed.store(true, Ordering::Release);
            session.intent.fetch_add(1, Ordering::AcqRel);
        }
    }
    fn permit(&self) -> Result<HistoryPermit, String> {
        self.history
            .try_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < 2).then_some(n + 1)
            })
            .map_err(|_| "同时最多读取两个日志历史任务，请等待其他页面完成")?;
        Ok(HistoryPermit(self.history.clone()))
    }
    fn batch(
        &self,
        view: &str,
        meta: &Metadata,
        purpose: Purpose,
        selection: Selection,
        from: Option<Stamp>,
        end: Stamp,
        tail: Option<usize>,
        bookmark: Option<store::Bookmark>,
        conditions: Option<Conditions>,
    ) -> Result<Batch, String> {
        let permit = self.permit()?;
        let reader = Reader::open(
            &self.app,
            &self.client,
            view,
            meta,
            purpose,
            from,
            Some(end),
            tail,
        )?;
        Ok(Batch {
            reader,
            rows: vec![],
            bytes: 0,
            selection,
            bookmark,
            last_publish: Instant::now(),
            _permit: permit,
            conditions,
            matches: 0,
            context: VecDeque::new(),
            after: 0,
            seen: BTreeSet::new(),
            segment_end: end,
            segment_start: from.unwrap_or(Stamp::seconds(0)),
            started: Instant::now(),
            scanned: 0,
            publish: false,
        })
    }
    fn advance(
        &self,
        view: &str,
        session: &Session,
        batch: &mut Batch,
        meta: &Metadata,
        retired: &mut Vec<Reader>,
    ) -> Result<(bool, bool), String> {
        let mut activity = false;
        for _ in 0..8 {
            let page = batch.reader.poll(&self.client)?;
            activity |= !page.waiting;
            let mut state = session.state.lock().unwrap();
            let records = state.store.ingest(page.rows, false)?;
            for record in records {
                if let Some(conditions) = &batch.conditions {
                    let allowed = (conditions.levels.is_empty()
                        || conditions.levels.iter().any(|l| {
                            l == if record.data.level.is_empty() {
                                "unknown"
                            } else {
                                &record.data.level
                            }
                        }))
                        && (conditions.sources.is_empty()
                            || conditions.sources.iter().any(|s| s == record.data.source));
                    let hit = allowed
                        && record
                            .data
                            .text
                            .to_lowercase()
                            .contains(&conditions.query.to_lowercase());
                    if hit {
                        batch.matches += 1;
                        for before in &batch.context {
                            if batch.seen.insert(before.id) {
                                batch.bytes += before.cost();
                                batch.rows.push(before.clone());
                            }
                        }
                        batch.after = 11;
                    }
                    if batch.after > 0 {
                        if batch.seen.insert(record.id) {
                            batch.bytes += record.cost();
                            batch.rows.push(record.clone());
                        }
                        batch.after -= 1;
                    }
                    batch.context.push_back(record);
                    if batch.context.len() > 10 {
                        batch.context.pop_front();
                    }
                } else {
                    batch.bytes += record.cost();
                    batch.rows.push(record);
                }
                let tail = batch.reader.tail.is_some();
                let limit = if batch.reader.purpose == Purpose::Preview {
                    2 * 1024 * 1024
                } else {
                    READING_BYTES
                };
                if tail {
                    while batch.rows.len() > 1000 || batch.bytes > limit {
                        batch.bytes -= batch.rows.remove(0).cost();
                        batch.reader.clipped = true;
                    }
                } else if batch.rows.len() > ROW_LIMIT || batch.bytes > READING_BYTES {
                    while batch.rows.len() > ROW_LIMIT || batch.bytes > READING_BYTES {
                        batch.bytes -= batch.rows.pop().unwrap().cost();
                    }
                    return Err("当前阅读结果达到容量，请缩小时间范围".into());
                }
                if batch.matches >= 500 {
                    return Err("历史搜索已保留500条命中，搜索未完成".into());
                }
            }
            if batch.matches >= 500
                || batch.scanned + batch.reader.raw_bytes() > SCAN_BYTES
                || batch.started.elapsed() > Duration::from_secs(120)
            {
                return Err(if batch.matches >= 500 {
                    "历史搜索已保留500条命中，搜索未完成"
                } else {
                    "日志扫描达到时间或字节上限，读取未完成"
                }
                .into());
            }
            if batch.conditions.is_some()
                && batch.last_publish.elapsed() > Duration::from_millis(250)
                && !batch.rows.is_empty()
            {
                if let Some(bookmark) = batch.bookmark.take() {
                    state.store.remember(bookmark);
                }
                state.store.conditions = batch.conditions.clone().unwrap();
                state.store.conditions.only = false;
                state.store.set_history(batch.rows.clone(), Mode::Search)?;
                state.selection = Some(batch.selection.clone());
                state.coverage = json!({"state":"reading","searched":true,"from":batch.selection.from.map(Stamp::text),"to":batch.selection.end.text(),"checkedFrom":batch.segment_end.text(),"checkingFrom":batch.segment_start.text(),"matches":batch.matches});
                batch.last_publish = Instant::now();
                batch.publish = true;
            }
            if !page.finished {
                if page.waiting {
                    return Ok((false, activity));
                }
                continue;
            }
            if !page.complete {
                let reason = page.status["error"]["message"]
                    .as_str()
                    .map(str::to_owned)
                    .or_else(|| cli_error(&page.status))
                    .unwrap_or_else(|| {
                        format!("日志读取未完成（退出码 {}）", page.status["exitCode"])
                    });
                return Err(reason);
            }
            match batch.reader.purpose {
                Purpose::BoundaryEarlier | Purpose::BoundaryLater => {
                    let earlier = batch.reader.purpose == Purpose::BoundaryEarlier;
                    let time = batch.reader.from.unwrap();
                    state
                        .store
                        .replace_boundary(time, std::mem::take(&mut batch.rows), earlier)?;
                    let start = if earlier {
                        batch.selection.lower
                    } else {
                        Some(time.next())
                    };
                    let end = if earlier {
                        time
                    } else {
                        time.add(900).min(batch.selection.upper)
                    };
                    let reader = Reader::open(
                        &self.app,
                        &self.client,
                        view,
                        meta,
                        if earlier {
                            Purpose::Earlier
                        } else {
                            Purpose::Later
                        },
                        start,
                        Some(end),
                        if earlier { Some(1000) } else { None },
                    )?;
                    let old = std::mem::replace(&mut batch.reader, reader);
                    retired.push(old);
                    batch.bytes = 0;
                    batch.segment_start = start.unwrap_or(Stamp::seconds(0));
                    batch.segment_end = end;
                    state.coverage = json!({"state":"reading","reason":"正在补读相邻日志"});
                    return Ok((false, true));
                }
                Purpose::Search => {
                    batch.scanned += batch.reader.raw_bytes();
                    let start = batch.segment_start;
                    let lower = batch.selection.from.unwrap_or(Stamp::seconds(0));
                    if start > lower {
                        let end = start;
                        let start = end.before(900).max(lower);
                        let reader = Reader::open(
                            &self.app,
                            &self.client,
                            view,
                            meta,
                            Purpose::Search,
                            Some(start),
                            Some(end),
                            None,
                        )?;
                        let old = std::mem::replace(&mut batch.reader, reader);
                        retired.push(old);
                        batch.segment_start = start;
                        batch.segment_end = end;
                        batch.context.clear();
                        batch.after = 0;
                        state.coverage = json!({"state":"reading","searched":true,"from":lower.text(),"to":batch.selection.end.text(),"checkedFrom":end.text(),"checkingFrom":start.text(),"matches":batch.matches,"resumeBefore":end.text()});
                        return Ok((false, true));
                    }
                    if let Some(bookmark) = batch.bookmark.take() {
                        state.store.remember(bookmark);
                    }
                    state.store.conditions = batch.conditions.clone().unwrap();
                    state.store.conditions.only = false;
                    state
                        .store
                        .set_history(std::mem::take(&mut batch.rows), Mode::Search)?;
                    state.selection = Some(batch.selection.clone());
                    state.coverage = json!({"state":"complete","searched":true,"from":lower.text(),"to":batch.selection.end.text(),"matches":batch.matches,"scannedBytes":batch.scanned});
                }
                Purpose::Earlier => {
                    let empty = batch.rows.is_empty();
                    state.store.prepend(std::mem::take(&mut batch.rows))?;
                    state.coverage = json!({"state":"preview","earlierAvailable":!empty,"reason":if empty{"没有更多可读日志"}else{"已补读更早内容；所选历史尚未全部加载"},"retainedPartial":batch.reader.clipped});
                }
                Purpose::Later => {
                    let empty = batch.rows.is_empty();
                    state
                        .store
                        .append_history(std::mem::take(&mut batch.rows))?;
                    state.coverage = json!({"state":"preview","laterAvailable":!empty,"reason":if empty{"相邻时段没有可读输出，可选择时间定位"}else{"已补读更晚内容"}});
                }
                Purpose::Preview => {
                    if let Some(bookmark) = batch.bookmark.take() {
                        if bookmark.mode != Mode::Latest {
                            state.store.remember(bookmark);
                        }
                    }
                    if batch.selection.live
                        || batch.selection.lower.is_none() && batch.selection.focus.is_none()
                    {
                        state
                            .store
                            .commit_latest(batch.selection.end, std::mem::take(&mut batch.rows));
                    } else {
                        state
                            .store
                            .set_history(std::mem::take(&mut batch.rows), Mode::History)?;
                    }
                    state.selection = Some(batch.selection.clone());
                    state.focus = batch.selection.focus;
                    state.coverage = json!({"state":"preview","from":batch.selection.from.map(Stamp::text),"to":batch.selection.end.text(),"reason":"当前显示有限尾部，尚未读取更早历史","retainedPartial":batch.reader.clipped});
                }
                Purpose::Follow => {}
            }
            state.loading = false;
            state.error = None;
            state.store.refresh();
            return Ok((true, true));
        }
        Ok((false, activity))
    }
    fn keep_partial(&self, session: &Session, batch: &mut Batch, reason: &str) {
        let mut state = session.state.lock().unwrap();
        if let Some(bookmark) = batch.bookmark.take() {
            state.store.remember(bookmark);
        }
        state.store.conditions = batch.conditions.clone().unwrap();
        state.store.conditions.only = false;
        let _ = state.store.set_history(batch.rows.clone(), Mode::Search);
        state.selection = Some(batch.selection.clone());
        state.store.refresh();
        state.coverage = json!({"state":"partial","searched":true,"reason":reason,"from":batch.selection.from.map(Stamp::text),"to":batch.selection.end.text(),"checkedFrom":batch.segment_end.text(),"matches":batch.matches,"resumeBefore":if batch.segment_end<batch.selection.end{Some(batch.segment_end.text())}else{None}});
    }
    fn run(
        self: Arc<Self>,
        view: String,
        session: Arc<Session>,
        receiver: mpsc::Receiver<(u64, Work)>,
    ) {
        let mut follow: Option<Reader> = None;
        let mut batch: Option<Batch> = None;
        let mut retired: Vec<Reader> = vec![];
        let mut metadata: Option<Metadata> = None;
        let mut last_meta = Instant::now();
        let mut reconnects = 0usize;
        let mut automatic = true;
        let mut active_generation = 0;
        let mut last_refresh = Instant::now();
        while !self.client.is_closed() && !session.closed.load(Ordering::Acquire) {
            if !self.app.lock().unwrap().views.contains_key(&view) {
                break;
            }
            while let Ok((generation, work)) = receiver.try_recv() {
                if generation != session.intent.load(Ordering::Acquire) {
                    continue;
                }
                active_generation = generation;
                reconnects = 0;
                automatic = !matches!(work, Work::Stop(true));
                if let Some(mut old) = batch.take() {
                    if matches!(work, Work::Stop(_)) && old.conditions.is_some() {
                        self.keep_partial(&session, &mut old, "搜索已停止");
                    }
                    old.reader.cancel(&self.client);
                    retired.push(old.reader);
                }
                let result = (|| -> Result<(), String> {
                    match work {
                        Work::Open(range) => {
                            if let Some(mut old) = follow.take() {
                                old.cancel(&self.client);
                                retired.push(old);
                            }
                            let meta = Metadata::read(&self.app, &self.client, &view)?;
                            let selection = meta.selection(&range)?;
                            if selection.live {
                                follow = Some(Reader::open(
                                    &self.app,
                                    &self.client,
                                    &view,
                                    &meta,
                                    Purpose::Follow,
                                    Some(selection.end),
                                    None,
                                    None,
                                )?);
                            }
                            let bookmark = {
                                let state = session.state.lock().unwrap();
                                Some(state.store.bookmark())
                            };
                            batch = Some(self.batch(
                                &view,
                                &meta,
                                Purpose::Preview,
                                selection.clone(),
                                selection.from,
                                selection.end,
                                Some(1000),
                                bookmark,
                                None,
                            )?);
                            metadata = Some(meta);
                        }
                        Work::Latest => {
                            let already =
                                (metadata.as_ref().is_some_and(|m| m.status != "running")
                                    && session.state.lock().unwrap().store.has_recent())
                                    || follow.is_some();
                            if already {
                                session.state.lock().unwrap().store.latest();
                            } else {
                                let meta = Metadata::read(&self.app, &self.client, &view)?;
                                let selection = meta.selection(&Range::Latest)?;
                                if selection.live {
                                    follow = Some(Reader::open(
                                        &self.app,
                                        &self.client,
                                        &view,
                                        &meta,
                                        Purpose::Follow,
                                        Some(selection.end),
                                        None,
                                        None,
                                    )?);
                                }
                                let bookmark = Some(session.state.lock().unwrap().store.bookmark());
                                batch = Some(self.batch(
                                    &view,
                                    &meta,
                                    Purpose::Preview,
                                    selection.clone(),
                                    None,
                                    selection.end,
                                    Some(1000),
                                    bookmark,
                                    None,
                                )?);
                                metadata = Some(meta);
                            }
                        }
                        Work::Previous => {
                            let mut state = session.state.lock().unwrap();
                            if !state.store.restore_previous() {
                                return Err("没有可以返回的历史位置".into());
                            }
                        }
                        Work::Earlier(earlier) => {
                            let meta = Metadata::read(&self.app, &self.client, &view)?;
                            let mut state = session.state.lock().unwrap();
                            state.store.hold();
                            let selection = state.selection.clone().ok_or("请先打开日志")?;
                            let (first, last) = state.store.bounds();
                            let boundary = if earlier { first } else { last }
                                .ok_or("当前内容没有可靠的源时间，请选择其他时段")?;
                            if earlier && selection.lower.is_some_and(|start| boundary <= start) {
                                return Err("已到所选时段起点，请扩大时间范围".into());
                            }
                            if !earlier && boundary >= selection.upper {
                                return Err("已到所选时段终点".into());
                            }
                            drop(state);
                            batch = Some(self.batch(
                                &view,
                                &meta,
                                if earlier {
                                    Purpose::BoundaryEarlier
                                } else {
                                    Purpose::BoundaryLater
                                },
                                selection,
                                Some(boundary),
                                boundary.next(),
                                None,
                                None,
                                None,
                            )?);
                            metadata = Some(meta);
                        }
                        Work::Search(range, conditions, resume) => {
                            if let Some(mut old) = follow.take() {
                                old.cancel(&self.client);
                                retired.push(old);
                            }
                            let meta = Metadata::read(&self.app, &self.client, &view)?;
                            let mut selection = meta.selection(&range)?;
                            if matches!(range, Range::Latest) {
                                selection = meta.selection(&Range::Recent { seconds: 86400 })?;
                            }
                            let mut state = session.state.lock().unwrap();
                            let end = if resume {
                                state.coverage["resumeBefore"]
                                    .as_str()
                                    .and_then(|t| Stamp::parse(t).ok())
                                    .ok_or("当前搜索没有可靠的继续位置，请缩小时段重新搜索")?
                            } else {
                                selection.end
                            };
                            let bookmark = Some(state.store.bookmark());
                            let retained = if resume {
                                if state.store.conditions.query != conditions.query
                                    || state.store.conditions.levels != conditions.levels
                                    || state.store.conditions.sources != conditions.sources
                                {
                                    return Err("继续搜索前条件已改变，请重新搜索".into());
                                }
                                state
                                    .store
                                    .all()
                                    .into_iter()
                                    .filter(|r| r.data.time.is_some_and(|t| t >= end))
                                    .collect::<Vec<_>>()
                            } else {
                                vec![]
                            };
                            state.store.hold();
                            drop(state);
                            let start = end
                                .before(900)
                                .max(selection.from.unwrap_or(Stamp::seconds(0)));
                            batch = Some(self.batch(
                                &view,
                                &meta,
                                Purpose::Search,
                                selection,
                                Some(start),
                                end,
                                None,
                                bookmark,
                                Some(conditions),
                            )?);
                            if resume {
                                let batch = batch.as_mut().unwrap();
                                batch.rows = retained;
                                batch.bytes = batch.rows.iter().map(|r| r.cost()).sum();
                                batch.matches = batch
                                    .rows
                                    .iter()
                                    .filter(|r| {
                                        r.data.text.to_lowercase().contains(
                                            &batch
                                                .conditions
                                                .as_ref()
                                                .unwrap()
                                                .query
                                                .to_lowercase(),
                                        )
                                    })
                                    .count();
                                batch.seen.extend(batch.rows.iter().map(|r| r.id));
                            }
                            metadata = Some(meta);
                        }
                        Work::Stop(all) => {
                            if all {
                                if let Some(mut old) = follow.take() {
                                    old.cancel(&self.client);
                                    retired.push(old);
                                }
                            }
                            let mut state = session.state.lock().unwrap();
                            state.coverage["state"] = json!("partial");
                            state.coverage["reason"] = json!("读取已停止");
                        }
                    }
                    Ok(())
                })();
                let mut state = session.state.lock().unwrap();
                state.loading = false;
                state.active = batch.is_some();
                state.follow = follow.is_some();
                if let Some(meta) = &metadata {
                    state.meta = Some(meta.clone());
                }
                if let Err(error) = result {
                    state.error = Some(error);
                }
                state.store.refresh();
                last_meta = Instant::now();
            }
            let current = session.intent.load(Ordering::Acquire) == active_generation;
            let mut busy = false;
            if let Some(reader) = follow.as_mut() {
                for _ in 0..8 {
                    match reader.poll(&self.client) {
                        Ok(page) => {
                            busy |= !page.waiting;
                            let mut state = session.state.lock().unwrap();
                            if state
                                .error
                                .as_ref()
                                .is_some_and(|e| e.starts_with("日志状态暂时不可用"))
                            {
                                state.error = None;
                            }
                            if !page.rows.is_empty() {
                                match state.store.ingest(page.rows, true) {
                                    Ok(rows) => state.store.append_recent(rows),
                                    Err(error) => {
                                        state.error = Some(error);
                                        reader.cancel(&self.client);
                                    }
                                }
                            }
                            if page.finished {
                                if !page.complete {
                                    state.error = Some(
                                        cli_error(&page.status)
                                            .or_else(|| {
                                                page.status["error"]["message"]
                                                    .as_str()
                                                    .map(str::to_owned)
                                            })
                                            .unwrap_or_else(|| {
                                                "日志接收已结束，结束结果未确认".into()
                                            }),
                                    );
                                }
                                state.follow = false;
                                drop(state);
                                let old = follow.take().unwrap();
                                retired.push(old);
                                break;
                            }
                            if page.waiting {
                                break;
                            }
                        }
                        Err(error) => {
                            if error.starts_with("日志状态暂时不可用") {
                                session.state.lock().unwrap().error = Some(error);
                                break;
                            }
                            let mut state = session.state.lock().unwrap();
                            state.error = Some(error);
                            state.store.count_known = false;
                            reader.cancel(&self.client);
                            let old = follow.take().unwrap();
                            retired.push(old);
                            break;
                        }
                    }
                }
            }
            if current && batch.is_some() {
                let result = self.advance(
                    &view,
                    &session,
                    batch.as_mut().unwrap(),
                    metadata.as_ref().unwrap(),
                    &mut retired,
                );
                match result {
                    Ok((done, activity)) => {
                        busy |= activity;
                        if done {
                            let old = batch.take().unwrap();
                            retired.push(old.reader);
                        }
                    }
                    Err(error) if error.starts_with("日志状态暂时不可用") => {
                        session.state.lock().unwrap().error = Some(error);
                    }
                    Err(error) => {
                        let mut old = batch.take().unwrap();
                        if old.conditions.is_some() {
                            self.keep_partial(&session, &mut old, &error);
                        }
                        old.reader.cancel(&self.client);
                        let mut state = session.state.lock().unwrap();
                        state.error = Some(error.clone());
                        if old.conditions.is_none() {
                            state.coverage = json!({"state":"partial","reason":error,"searched":false,"from":old.selection.from.map(Stamp::text),"to":old.selection.end.text(),"checkedFrom":old.segment_end.text()});
                        }
                        retired.push(old.reader);
                    }
                }
            }
            if last_meta.elapsed() > Duration::from_secs(5)
                && !session.closed.load(Ordering::Acquire)
            {
                let observed = Metadata::read(&self.app, &self.client, &view);
                if let Err(error) = &observed {
                    session.state.lock().unwrap().error = Some(error.clone());
                }
                if let Ok(meta) = observed {
                    let restarted = metadata
                        .as_ref()
                        .is_some_and(|old| old.started != meta.started);
                    let mut state = session.state.lock().unwrap();
                    state.meta = Some(meta.clone());
                    if restarted {
                        state.error = Some("容器重新启动；已保留当前阅读内容".into());
                    }
                    let wants = state.store.mode == Mode::Latest;
                    drop(state);
                    if (restarted || follow.is_none())
                        && meta.status == "running"
                        && wants
                        && batch.is_none()
                        && automatic
                        && reconnects < 3
                    {
                        reconnects += 1;
                        if let Some(mut old) = follow.take() {
                            old.cancel(&self.client);
                            retired.push(old);
                        }
                        let from = metadata.as_ref().map(|m| m.now).unwrap_or(meta.now);
                        match Reader::open(
                            &self.app,
                            &self.client,
                            &view,
                            &meta,
                            Purpose::Follow,
                            Some(from),
                            None,
                            None,
                        ) {
                            Ok(reader) => {
                                follow = Some(reader);
                                let mut state = session.state.lock().unwrap();
                                state.error =
                                    Some("日志接收已恢复；断开区间可能存在缺口或重复".into());
                                state.store.count_known = false;
                            }
                            Err(error) => session.state.lock().unwrap().error = Some(error),
                        }
                    }
                    metadata = Some(meta);
                }
                last_meta = Instant::now();
            }
            if last_refresh.elapsed() > Duration::from_millis(120) {
                let mut state = session.state.lock().unwrap();
                state.store.refresh();
                state.active = batch.is_some();
                state.follow = follow.is_some();
                session.active.store(batch.is_some(), Ordering::Release);
                last_refresh = Instant::now();
            }
            retired.retain_mut(|reader| !reader.release(&self.client));
            if !busy {
                std::thread::sleep(Duration::from_millis(25));
            }
        }
        if let Some(mut reader) = follow {
            reader.cancel(&self.client);
            retired.push(reader);
        }
        if let Some(mut old) = batch {
            old.reader.cancel(&self.client);
            retired.push(old.reader);
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        while !retired.is_empty() && !self.client.is_closed() && Instant::now() < deadline {
            retired.retain_mut(|reader| !reader.release(&self.client));
            if !retired.is_empty() {
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        session.state.lock().unwrap().closed = true;
    }
}
fn status(state: &ViewState, budget: usize) -> Value {
    let mut value = state.store.status();
    value["version"] = json!(2);
    value["ready"] = json!(true);
    value["source"] = state
        .meta
        .as_ref()
        .map(Metadata::value)
        .unwrap_or(Value::Null);
    value["nextIntent"] = json!(state.next_intent.to_string());
    value["loading"] = json!(state.loading);
    value["active"] = json!(state.active);
    value["receiving"] = json!(state.follow);
    value["error"] = json!(state.error);
    value["coverage"] = state.coverage.clone();
    value["selection"] = state
        .selection
        .as_ref()
        .map(Selection::value)
        .unwrap_or(Value::Null);
    value["navigation"] = state.navigation.clone();
    value["budgetBytes"] = json!(budget);
    value["focusTime"] = json!(state.focus.map(Stamp::text));
    value
}

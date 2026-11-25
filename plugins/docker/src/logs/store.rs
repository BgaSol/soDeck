use super::{
    contract::*,
    records::{self, Draft, Stamp},
};
use serde_json::{json, Value};
use sodeck_plugin_client::encoding::base64;
use std::{
    collections::{BTreeSet, VecDeque},
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc,
    },
};

pub struct Budget {
    used: AtomicUsize,
    limit: usize,
}
impl Budget {
    pub fn new(limit: usize) -> Arc<Self> {
        Arc::new(Self {
            used: AtomicUsize::new(0),
            limit,
        })
    }
    fn reserve(self: &Arc<Self>, bytes: usize) -> Result<Reservation, String> {
        self.used
            .try_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                n.checked_add(bytes).filter(|n| *n <= self.limit)
            })
            .map_err(|_| "多个日志页的数据达到容量上限，请关闭其他日志页后重试")?;
        Ok(Reservation {
            budget: self.clone(),
            bytes,
        })
    }
    pub fn used(&self) -> usize {
        self.used.load(Ordering::Acquire)
    }
}
struct Reservation {
    budget: Arc<Budget>,
    bytes: usize,
}
impl Drop for Reservation {
    fn drop(&mut self) {
        self.budget.used.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}
pub struct Record {
    pub id: u64,
    pub data: Draft,
    reservation: Reservation,
}
impl Record {
    pub fn cost(&self) -> usize {
        self.reservation.bytes
    }
}
#[derive(Clone)]
pub struct Bookmark {
    pub rows: Vec<Arc<Record>>,
    pub conditions: Conditions,
    pub mode: Mode,
}
struct Projected {
    record: Arc<Record>,
    matched: bool,
    context: bool,
}
struct Projection {
    revision: u64,
    rows: Vec<Projected>,
    all: Vec<Arc<Record>>,
    received: u64,
}
struct Export {
    token: String,
    header: Vec<u8>,
    rows: Vec<Arc<Record>>,
    starts: Vec<usize>,
    length: usize,
}
pub struct Store {
    pub mode: Mode,
    pub conditions: Conditions,
    pub revision: u64,
    pub received: u64,
    pub count_known: bool,
    recent: VecDeque<Arc<Record>>,
    held: Option<Vec<Arc<Record>>>,
    held_at: u64,
    previous: Option<Bookmark>,
    expanded: BTreeSet<u64>,
    projected: Vec<Projected>,
    snapshots: VecDeque<Projection>,
    published_all: Vec<Arc<Record>>,
    published_received: u64,
    matched: usize,
    pub dirty: bool,
    ids: Arc<AtomicU64>,
    budget: Arc<Budget>,
    export: Option<Export>,
    pub retained_partial: bool,
}
impl Store {
    pub fn new(budget: Arc<Budget>, ids: Arc<AtomicU64>) -> Self {
        Self {
            mode: Mode::Latest,
            conditions: Conditions::default(),
            revision: 0,
            received: 0,
            count_known: true,
            recent: VecDeque::new(),
            held: None,
            held_at: 0,
            previous: None,
            expanded: BTreeSet::new(),
            projected: Vec::new(),
            snapshots: VecDeque::new(),
            published_all: Vec::new(),
            published_received: 0,
            matched: 0,
            dirty: false,
            ids,
            budget,
            export: None,
            retained_partial: false,
        }
    }
    pub fn record(&mut self, data: Draft) -> Result<Arc<Record>, String> {
        let cost = data.raw.capacity() + data.text.capacity() + 512;
        let reservation = match self.budget.reserve(cost) {
            Ok(r) => r,
            Err(error) => {
                if self.held.is_some() {
                    self.recent.clear();
                }
                self.budget.reserve(cost).map_err(|_| error)?
            }
        };
        Ok(Arc::new(Record {
            id: self.ids.fetch_add(1, Ordering::Relaxed),
            data,
            reservation,
        }))
    }
    pub fn ingest(&mut self, rows: Vec<Draft>, live: bool) -> Result<Vec<Arc<Record>>, String> {
        if live {
            self.received = self.received.saturating_add(rows.len() as u64);
        }
        rows.into_iter().map(|r| self.record(r)).collect()
    }
    pub fn append_recent(&mut self, rows: Vec<Arc<Record>>) {
        for row in rows {
            let stamp = |r: &Arc<Record>| {
                r.data
                    .time
                    .unwrap_or(Stamp(r.data.received_at as u128 * 1_000_000))
            };
            if self
                .recent
                .back()
                .is_none_or(|last| stamp(last) <= stamp(&row))
            {
                self.recent.push_back(row);
            } else {
                let at = self
                    .recent
                    .make_contiguous()
                    .partition_point(|r| stamp(r) <= stamp(&row));
                self.recent.insert(at, row);
            }
        }
        let mut bytes = self.recent.iter().map(|r| r.cost()).sum::<usize>();
        while self.recent.len() > RECENT_ROWS || bytes > RECENT_BYTES {
            bytes -= self.recent.pop_front().unwrap().cost();
        }
        if self.held.is_none() {
            self.dirty = true;
        }
    }
    pub fn has_recent(&self) -> bool {
        !self.recent.is_empty()
    }
    pub fn commit_latest(&mut self, before: Stamp, rows: Vec<Arc<Record>>) {
        self.recent
            .retain(|r| r.data.time.is_none_or(|t| t >= before));
        self.append_recent(rows);
        self.latest();
    }
    pub fn hold(&mut self) {
        if self.held.is_none() {
            self.held = Some(self.recent.iter().cloned().collect());
            self.held_at = self.received;
            self.dirty = true;
        }
    }
    pub fn hold_at(&mut self, revision: Option<&str>) -> Result<(), String> {
        if self.held.is_some() {
            return Ok(());
        }
        let Some(revision) = revision else {
            self.hold();
            return Ok(());
        };
        let revision = revision.parse::<u64>().map_err(|_| "阅读窗口版本无效")?;
        let (rows, received) = if revision == self.revision {
            (self.published_all.clone(), self.published_received)
        } else {
            let snapshot = self
                .snapshots
                .iter()
                .find(|s| s.revision == revision)
                .ok_or("原阅读窗口已退出保留范围，请重试")?;
            (snapshot.all.clone(), snapshot.received)
        };
        self.held = Some(rows);
        self.held_at = received;
        self.dirty = true;
        Ok(())
    }
    pub fn latest(&mut self) {
        if self.mode != Mode::Latest {
            self.previous = Some(self.bookmark());
        }
        self.mode = Mode::Latest;
        self.held = None;
        self.expanded.clear();
        self.dirty = true;
    }
    pub fn bookmark(&self) -> Bookmark {
        Bookmark {
            rows: self.all(),
            conditions: self.conditions.clone(),
            mode: self.mode,
        }
    }
    pub fn remember(&mut self, bookmark: Bookmark) {
        self.previous = Some(bookmark);
    }
    pub fn restore_previous(&mut self) -> bool {
        let Some(previous) = self.previous.take() else {
            return false;
        };
        self.held = Some(previous.rows);
        self.conditions = previous.conditions;
        self.mode = previous.mode;
        self.dirty = true;
        true
    }
    pub fn has_previous(&self) -> bool {
        self.previous.is_some()
    }
    pub fn set_history(&mut self, mut rows: Vec<Arc<Record>>, mode: Mode) -> Result<(), String> {
        if rows.len() > ROW_LIMIT || rows.iter().map(|r| r.cost()).sum::<usize>() > READING_BYTES {
            return Err("当前阅读窗口达到容量，请缩小时间范围".into());
        }
        rows.sort_by_key(|r| {
            r.data
                .time
                .unwrap_or(Stamp(r.data.received_at as u128 * 1_000_000))
        });
        self.held = Some(rows);
        self.held_at = self.received;
        self.mode = mode;
        self.expanded.clear();
        self.dirty = true;
        Ok(())
    }
    pub fn prepend(&mut self, mut rows: Vec<Arc<Record>>) -> Result<(), String> {
        self.hold();
        rows.extend(self.all());
        self.set_history(rows, self.mode)
    }
    pub fn append_history(&mut self, rows: Vec<Arc<Record>>) -> Result<(), String> {
        self.hold();
        let mut all = self.all();
        all.extend(rows);
        self.set_history(all, self.mode)
    }
    pub fn replace_boundary(
        &mut self,
        time: Stamp,
        rows: Vec<Arc<Record>>,
        earlier: bool,
    ) -> Result<(), String> {
        self.hold();
        let old = self.all();
        let existing: Vec<_> = old
            .iter()
            .filter(|r| r.data.time == Some(time))
            .cloned()
            .collect();
        let mut replacements = rows;
        for source in ["stdout", "stderr", "combined"] {
            let before: Vec<_> = existing
                .iter()
                .filter(|r| r.data.source == source)
                .collect();
            let indexes: Vec<_> = replacements
                .iter()
                .enumerate()
                .filter(|(_, r)| r.data.source == source)
                .map(|(i, _)| i)
                .collect();
            if before.len() > indexes.len() {
                return Err("同时间戳边界已改变，无法确认连续补读，请重新选择时段".into());
            }
            let offset = if earlier {
                indexes.len() - before.len()
            } else {
                0
            };
            for (n, row) in before.iter().enumerate() {
                let at = indexes[n + offset];
                if replacements[at].data.raw != row.data.raw {
                    return Err("同时间戳边界已改变，无法确认连续补读，请重新选择时段".into());
                }
                replacements[at] = (*row).clone();
            }
        }
        let first = old
            .iter()
            .position(|r| r.data.time == Some(time))
            .unwrap_or(0);
        let mut next: Vec<_> = old
            .iter()
            .filter(|r| r.data.time != Some(time))
            .cloned()
            .collect();
        next.splice(first..first, replacements);
        self.set_history(next, self.mode)
    }
    pub fn all(&self) -> Vec<Arc<Record>> {
        self.held
            .clone()
            .unwrap_or_else(|| self.recent.iter().cloned().collect())
    }
    pub fn bounds(&self) -> (Option<Stamp>, Option<Stamp>) {
        let rows = self
            .held
            .as_ref()
            .map(|r| r.iter().collect::<Vec<_>>())
            .unwrap_or_else(|| self.recent.iter().collect());
        (
            rows.iter().filter_map(|r| r.data.time).min(),
            rows.iter().filter_map(|r| r.data.time).max(),
        )
    }
    pub fn query(&mut self, conditions: Conditions) {
        self.hold();
        self.conditions = conditions;
        self.expanded.clear();
        self.dirty = true;
    }
    pub fn refresh(&mut self) {
        if !self.dirty {
            return;
        }
        let all = self.all();
        let query = self.conditions.query.to_lowercase();
        let published = all.clone();
        let mut projected = Vec::new();
        let mut matched = 0;
        for row in all {
            let allowed = (self.conditions.levels.is_empty()
                || self.conditions.levels.iter().any(|l| {
                    l == if row.data.level.is_empty() {
                        "unknown"
                    } else {
                        &row.data.level
                    }
                }))
                && (self.conditions.sources.is_empty()
                    || self.conditions.sources.iter().any(|s| s == row.data.source));
            let hit = !query.is_empty() && row.data.text.to_lowercase().contains(&query);
            if allowed && hit {
                matched += 1;
            }
            if (allowed && (!self.conditions.only || hit)) || self.expanded.contains(&row.id) {
                projected.push(Projected {
                    context: !allowed || (self.conditions.only && !hit),
                    record: row,
                    matched: allowed && hit,
                });
            }
        }
        let old = std::mem::replace(&mut self.projected, projected);
        let old_all = std::mem::replace(&mut self.published_all, published);
        self.snapshots.push_back(Projection {
            revision: self.revision,
            rows: old,
            all: old_all,
            received: self.published_received,
        });
        self.published_received = self.received;
        while self.snapshots.len() > 4 {
            self.snapshots.pop_front();
        }
        self.matched = matched;
        self.revision += 1;
        self.dirty = false;
    }
    pub fn status(&self) -> Value {
        let (first, last) = self.bounds();
        json!({"revision":self.revision.to_string(),"mode":self.mode,"following":self.held.is_none(),"total":self.projected.len(),"windowTotal":self.held.as_ref().map_or(self.recent.len(),Vec::len),"matches":self.matched,"newCount":self.received.saturating_sub(self.held_at),"countKnown":self.count_known,"conditions":self.conditions,"first":first.map(Stamp::text),"last":last.map(Stamp::text),"hasPrevious":self.has_previous(),"retainedPartial":self.retained_partial,"exporting":self.export.is_some()})
    }
    fn projection(&self, revision: &str) -> Result<&[Projected], String> {
        let revision = revision.parse::<u64>().map_err(|_| "日志投影版本无效")?;
        if revision == self.revision {
            return Ok(&self.projected);
        }
        self.snapshots
            .iter()
            .find(|p| p.revision == revision)
            .map(|p| p.rows.as_slice())
            .ok_or_else(|| "日志投影已更新，请读取当前版本".into())
    }
    pub fn layout(&self, revision: &str, start: usize) -> Result<Value, String> {
        let projected = self.projection(revision)?;
        let mut ids = Vec::new();
        let mut bytes = 512;
        for row in projected.iter().skip(start) {
            let id = row.record.id.to_string();
            bytes += id.len() + 3; // JSON quotes and separator, IDs are decimal ASCII.
            if bytes > PAGE_BYTES {
                break;
            }
            ids.push(id);
        }
        Ok(json!({"revision":revision,"total":projected.len(),"next":start+ids.len(),"ids":ids}))
    }
    pub fn window(&self, revision: &str, start: usize, count: usize) -> Result<Value, String> {
        let projected = self.projection(revision)?;
        let mut rows = Vec::new();
        for (index, row) in projected
            .iter()
            .enumerate()
            .skip(start)
            .take(count.min(500))
        {
            let text: String = row.record.data.text.chars().take(2048).collect();
            let hits = records::matches(&text, &self.conditions.query)
                .into_iter()
                .take(128)
                .collect::<Vec<_>>();
            let record = &row.record;
            rows.push(json!({"id":record.id.to_string(),"index":index,"text":text,"length":record.data.text.encode_utf16().count(),"matches":hits,"time":record.data.time.map(Stamp::text),"receivedAt":record.data.received_at,"source":record.data.source,"level":record.data.level,"incomplete":record.data.incomplete,"context":row.context}));
            if serde_json::to_vec(&rows).unwrap().len() > PAGE_BYTES - 512 {
                rows.pop();
                break;
            }
        }
        Ok(json!({"revision":revision,"total":projected.len(),"next":start+rows.len(),"rows":rows}))
    }
    pub fn find(&self, id: &str) -> Option<Arc<Record>> {
        let id = id.parse::<u64>().ok()?;
        self.all().into_iter().find(|r| r.id == id).or_else(|| {
            self.projected
                .iter()
                .find(|r| r.record.id == id)
                .map(|r| r.record.clone())
        })
    }
    pub fn detail(&self, id: &str, offset: usize) -> Result<Value, String> {
        let row = self.find(id).ok_or("日志记录已退出当前窗口")?;
        if offset > row.data.raw.len() {
            return Err("日志详情偏移无效".into());
        }
        let end = (offset + 12 * 1024).min(row.data.raw.len());
        Ok(
            json!({"id":id,"dataBase64":base64(&row.data.raw[offset..end]),"nextOffset":end,"length":row.data.raw.len(),"done":end==row.data.raw.len(),"time":row.data.time.map(Stamp::text),"receivedAt":row.data.received_at,"source":row.data.source,"incomplete":row.data.incomplete}),
        )
    }
    pub fn navigate(&self, id: Option<&str>, direction: i8) -> Result<Value, String> {
        if ![-1, 1].contains(&direction) {
            return Err("匹配导航方向无效".into());
        }
        let len = self.projected.len();
        if len == 0 {
            return Ok(Value::Null);
        }
        let at = id.and_then(|id| {
            self.projected
                .iter()
                .position(|r| r.record.id.to_string() == id)
        });
        let mut at = at.unwrap_or(if direction > 0 { len - 1 } else { 0 });
        for _ in 0..len {
            at = if direction > 0 {
                (at + 1) % len
            } else {
                (at + len - 1) % len
            };
            if self.projected[at].matched {
                return Ok(json!({"id":self.projected[at].record.id.to_string(),"index":at}));
            }
        }
        Ok(Value::Null)
    }
    pub fn locate_time(&self, at: &str) -> Result<Value, String> {
        let at = Stamp::parse(at)?;
        if self.projected.is_empty() {
            return Ok(Value::Null);
        }
        let index = self
            .projected
            .partition_point(|r| r.record.data.time.is_some_and(|t| t < at))
            .min(self.projected.len() - 1);
        Ok(json!({"id":self.projected[index].record.id.to_string(),"index":index}))
    }
    pub fn context(&mut self, id: &str) -> Result<(), String> {
        let row = self.find(id).ok_or("日志记录已退出当前窗口")?;
        let all = self.all();
        let at = all.iter().position(|r| r.id == row.id).unwrap();
        self.expanded.extend(
            all.iter()
                .skip(at.saturating_sub(10))
                .take(at + 11 - at.saturating_sub(10))
                .map(|r| r.id),
        );
        self.dirty = true;
        Ok(())
    }
    pub fn export_begin(
        &mut self,
        id: Option<&str>,
        context: bool,
        header: String,
    ) -> Result<Value, String> {
        if self.export.is_some() {
            return Err("已有日志导出正在进行".into());
        }
        let rows = if let Some(id) = id {
            let row = self.find(id).ok_or("日志记录已退出当前窗口")?;
            if context {
                let all = self.all();
                let at = all.iter().position(|r| r.id == row.id).unwrap();
                all.into_iter()
                    .skip(at.saturating_sub(10))
                    .take(at + 11 - at.saturating_sub(10))
                    .collect()
            } else {
                vec![row]
            }
        } else {
            self.projected
                .iter()
                .map(|r| r.record.clone())
                .collect::<Vec<_>>()
        };
        if rows.iter().any(|r| r.data.incomplete) {
            return Err("当前结果包含未完整保留的长记录，不能导出为原文证据".into());
        }
        let token = self.ids.fetch_add(1, Ordering::Relaxed).to_string();
        let header = (header + "\n\n").into_bytes();
        let mut length = header.len();
        let mut starts = vec![0, length];
        for row in &rows {
            length += row.data.raw.len();
            starts.push(length);
        }
        self.export = Some(Export {
            token: token.clone(),
            header,
            rows,
            starts,
            length,
        });
        Ok(json!({"token":token,"length":length}))
    }
    pub fn export_read(&self, token: &str, offset: usize) -> Result<Value, String> {
        let export = self
            .export
            .as_ref()
            .filter(|e| e.token == token)
            .ok_or("日志导出快照已释放")?;
        if offset > export.length {
            return Err("日志导出偏移无效".into());
        }
        let mut bytes = Vec::new();
        let mut at = offset;
        while bytes.len() < 12 * 1024 && at < export.length {
            let index = export.starts.partition_point(|start| *start <= at) - 1;
            let part = if index == 0 {
                &export.header
            } else {
                &export.rows[index - 1].data.raw
            };
            let from = at - export.starts[index];
            let size = (12 * 1024 - bytes.len()).min(part.len() - from);
            bytes.extend_from_slice(&part[from..from + size]);
            at += size;
        }
        Ok(json!({"dataBase64":base64(&bytes),"nextOffset":at,"done":at==export.length}))
    }
    pub fn export_end(&mut self, token: &str) -> Result<(), String> {
        if self.export.as_ref().is_some_and(|e| e.token != token) {
            return Err("日志导出快照已改变".into());
        }
        self.export = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sodeck_plugin_client::encoding::unbase64;
    fn store() -> Store {
        Store::new(Budget::new(TOTAL_BYTES), Arc::new(AtomicU64::new(1)))
    }
    fn row(text: &str, time: u64) -> Draft {
        Draft {
            raw: format!("{text}\n").into_bytes(),
            text: text.into(),
            time: Some(Stamp::seconds(time)),
            received_at: time * 1000,
            source: "stdout",
            level: "INFO".into(),
            incomplete: false,
        }
    }
    #[test]
    fn layout_pages_follow_the_byte_budget_and_reassemble_the_projection() {
        let mut s = store();
        let rows = s
            .ingest((0..6000).map(|n| row("INFO", n)).collect(), true)
            .unwrap();
        s.append_recent(rows);
        s.refresh();
        let revision = s.status()["revision"].as_str().unwrap().to_owned();
        let mut ids = Vec::new();
        let mut pages = 0;
        while ids.len() < 6000 {
            let page = s.layout(&revision, ids.len()).unwrap();
            assert!(serde_json::to_vec(&page).unwrap().len() <= PAGE_BYTES);
            ids.extend(
                page["ids"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_str().unwrap().to_owned()),
            );
            assert_eq!(page["next"].as_u64().unwrap() as usize, ids.len());
            pages += 1;
        }
        assert_eq!(ids.len(), 6000);
        assert_eq!(
            ids.iter().collect::<std::collections::HashSet<_>>().len(),
            6000
        );
        assert!(
            pages < 7,
            "small IDs should not use the previous fixed 900-row pages"
        );
    }

    #[test]
    fn fixed_reading_counts_all_new_rows_after_recent_eviction() {
        let mut s = store();
        let first = s.ingest(vec![row("initial", 1)], true).unwrap();
        s.append_recent(first);
        s.refresh();
        s.hold();
        let new = s
            .ingest(
                (0..RECENT_ROWS + 5)
                    .map(|n| row("new", n as u64 + 2))
                    .collect(),
                true,
            )
            .unwrap();
        s.append_recent(new);
        s.refresh();
        assert_eq!(s.status()["windowTotal"], 1);
        assert_eq!(s.status()["newCount"], RECENT_ROWS + 5);
        s.latest();
        s.refresh();
        assert_eq!(s.status()["windowTotal"], RECENT_ROWS);
    }
    #[test]
    fn shared_records_keep_budget_until_export_and_reading_release() {
        let budget = Budget::new(TOTAL_BYTES);
        let mut s = Store::new(budget.clone(), Arc::new(AtomicU64::new(1)));
        let rows = s.ingest(vec![row("中文", 1)], true).unwrap();
        s.append_recent(rows);
        s.refresh();
        s.hold();
        s.refresh();
        let token = s.export_begin(None, false, "header".into()).unwrap()["token"]
            .as_str()
            .unwrap()
            .to_owned();
        let count = budget.used();
        s.recent.clear();
        assert_eq!(budget.used(), count);
        s.held = None;
        s.projected.clear();
        s.snapshots.clear();
        s.published_all.clear();
        assert_eq!(budget.used(), count);
        s.export_end(&token).unwrap();
        assert_eq!(budget.used(), 0);
    }
    #[test]
    fn bounded_projection_counts_records_and_keeps_raw_snapshot() {
        let mut s = store();
        let rows = s
            .ingest(vec![row("a.b a.b 中文", 1), row("other", 2)], false)
            .unwrap();
        s.append_recent(rows);
        s.query(Conditions {
            query: "a.b".into(),
            only: true,
            ..Conditions::default()
        });
        s.refresh();
        assert_eq!(s.status()["matches"], 1);
        let rev = s.revision.to_string();
        assert_eq!(
            s.window(&rev, 0, 500).unwrap()["rows"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert!(s.window("stale", 0, 1).is_err());
        let answer = s.export_begin(None, false, "header".into()).unwrap();
        let token = answer["token"].as_str().unwrap();
        let data = s.export_read(token, 0).unwrap();
        let raw = unbase64(data["dataBase64"].as_str().unwrap()).unwrap();
        assert_eq!(String::from_utf8(raw).unwrap(), "header\n\na.b a.b 中文\n");
    }
    #[test]
    fn timestamp_boundary_preserves_repeated_records_and_existing_ids() {
        let mut s = store();
        let old = s
            .ingest(
                vec![row("repeat", 1), row("repeat", 1), row("later", 2)],
                false,
            )
            .unwrap();
        let held_ids: Vec<_> = old.iter().map(|r| r.id).collect();
        s.set_history(old, Mode::History).unwrap();
        let group = s
            .ingest(
                vec![row("earlier repeat", 1), row("repeat", 1), row("repeat", 1)],
                false,
            )
            .unwrap();
        s.replace_boundary(Stamp::seconds(1), group, true).unwrap();
        assert_eq!(s.all().len(), 4);
        assert_eq!(s.all()[1].id, held_ids[0]);
        assert_eq!(s.all()[2].id, held_ids[1]);
        let changed = s.ingest(vec![row("changed", 1)], false).unwrap();
        assert!(s
            .replace_boundary(Stamp::seconds(1), changed, true)
            .is_err());
        assert_eq!(s.all().len(), 4);
    }
    #[test]
    fn incomplete_record_cannot_be_exported() {
        let mut s = store();
        let mut draft = row("partial", 1);
        draft.incomplete = true;
        let rows = s.ingest(vec![draft], false).unwrap();
        s.append_recent(rows);
        s.refresh();
        assert!(s.export_begin(None, false, "header".into()).is_err());
    }
}

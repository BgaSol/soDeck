//! Local archives. Original log bytes and searchable records commit together.
//! The frontend receives only committed sequence numbers, never owns the archive.
use crate::model::{AppError, AppResult};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub(crate) const FRAGMENT_SIZE: usize = 64 * 1024;
const SCAN_SIZE: i64 = 20_000;
const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS records (
 seq INTEGER PRIMARY KEY, stream TEXT NOT NULL, raw TEXT NOT NULL, text TEXT NOT NULL, prefix TEXT NOT NULL DEFAULT '',
 folded TEXT NOT NULL, timestamp TEXT, time_ns INTEGER, level TEXT NOT NULL,
 complete INTEGER NOT NULL, continuation INTEGER NOT NULL, replayed INTEGER NOT NULL,
 run_id TEXT NOT NULL, received_at INTEGER NOT NULL);
CREATE INDEX IF NOT EXISTS record_time ON records(time_ns);
CREATE TABLE IF NOT EXISTS totals (id INTEGER PRIMARY KEY, count INTEGER NOT NULL, last_seq INTEGER NOT NULL, last_time INTEGER, last_timestamp TEXT);
INSERT OR IGNORE INTO totals VALUES(1,0,0,NULL,NULL);
CREATE VIRTUAL TABLE IF NOT EXISTS search USING fts5(folded, content='records', content_rowid='seq', tokenize='trigram case_sensitive 1');
CREATE TRIGGER IF NOT EXISTS record_insert AFTER INSERT ON records BEGIN
 INSERT INTO search(rowid,folded) VALUES(new.seq,new.folded);
 UPDATE totals SET count=count+1,last_seq=max(last_seq,new.seq),
 last_timestamp=CASE WHEN new.time_ns>coalesce(last_time,-9223372036854775808) THEN new.timestamp ELSE last_timestamp END,
 last_time=CASE WHEN new.time_ns>coalesce(last_time,-9223372036854775808) THEN new.time_ns ELSE last_time END WHERE id=1; END;
CREATE TRIGGER IF NOT EXISTS record_delete AFTER DELETE ON records BEGIN
 INSERT INTO search(search,rowid,folded) VALUES('delete',old.seq,old.folded); END;
CREATE TRIGGER IF NOT EXISTS record_update AFTER UPDATE ON records BEGIN
 INSERT INTO search(search,rowid,folded) VALUES('delete',old.seq,old.folded);
 INSERT INTO search(rowid,folded) VALUES(new.seq,new.folded);
 UPDATE totals SET last_timestamp=CASE WHEN new.time_ns>coalesce(last_time,-9223372036854775808) THEN new.timestamp ELSE last_timestamp END,
 last_time=CASE WHEN new.time_ns>coalesce(last_time,-9223372036854775808) THEN new.time_ns ELSE last_time END WHERE id=1; END;
CREATE TABLE IF NOT EXISTS chunks (id INTEGER PRIMARY KEY, run_id TEXT NOT NULL,
 stream TEXT NOT NULL, data BLOB NOT NULL, received_at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS runs (id TEXT PRIMARY KEY, started_at INTEGER NOT NULL,
 ended_at INTEGER, since TEXT, note TEXT);
";

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub source_id: String,
    pub server_id: String,
    pub server_name: String,
    pub container_name: String,
    pub container_id: String,
    pub endpoint: String,
    pub host: String,
    pub port: u16,
    #[serde(default)]
    pub tty: bool,
}
impl Source {
    pub fn new(spec: &crate::ssh::ConnectionSpec, target: &crate::docker::ContainerTarget) -> Self {
        let identity = serde_json::to_vec(&(
            &spec.server.host,
            spec.server.port,
            &spec.server.username,
            &spec.fingerprint,
            &target.endpoint,
            &target.id,
        ))
        .unwrap();
        Self {
            source_id: format!("{:x}", Sha256::digest(identity)),
            server_id: spec.server.id.clone(),
            server_name: spec.server.name.clone(),
            container_name: target.name.clone(),
            container_id: target.id.clone(),
            endpoint: target.endpoint.clone(),
            host: spec.server.host.clone(),
            port: spec.server.port,
            tty: false,
        }
    }
}
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub records: i64,
    pub last_seq: i64,
    pub bytes: u64,
    pub last_timestamp: Option<String>,
    pub runs: i64,
    pub interrupted_runs: i64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Archive {
    #[serde(flatten)]
    pub source: Source,
    #[serde(flatten)]
    pub stats: Stats,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub seq: i64,
    pub stream: String,
    pub raw: String,
    pub text: String,
    pub timestamp: Option<String>,
    pub level: String,
    pub complete: bool,
    pub continuation: bool,
    pub prefix: String,
}
#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Query {
    pub before: Option<i64>,
    pub after: Option<i64>,
    pub snapshot: Option<i64>,
    pub limit: usize,
    pub keyword: String,
    pub case_sensitive: bool,
    pub stream: String,
    pub from: Option<String>,
    pub to: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub rows: Vec<Record>,
    pub next_cursor: i64,
    pub has_more: bool,
    pub scanned: i64,
    pub snapshot: i64,
}

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}
pub fn source_dir(root: &Path, id: &str) -> AppResult<PathBuf> {
    if id.len() != 64 || !id.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(AppError::new("validation", "无效的日志存档"));
    }
    Ok(root.join(id))
}
pub(crate) fn files(dir: &Path) -> AppResult<Vec<PathBuf>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut paths = Vec::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("segment-") && n.ends_with(".db"))
        {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}
pub(crate) fn reader(path: &Path) -> AppResult<Connection> {
    let c = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    c.busy_timeout(std::time::Duration::from_secs(5))?;
    Ok(c)
}
pub(crate) fn writer(path: &Path) -> AppResult<Connection> {
    let c = Connection::open(path)?;
    let version: i64 = c.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version > 3 {
        return Err(AppError::new(
            "logs",
            "此日志存档由更新版本的应用创建，请升级后读取",
        ));
    }
    c.busy_timeout(std::time::Duration::from_secs(5))?;
    if version == 1 {
        c.execute_batch("ALTER TABLE records ADD COLUMN prefix TEXT NOT NULL DEFAULT '';")?;
    }
    c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;")?;
    c.execute_batch(SCHEMA)?;
    crate::log_ingest::upgrade(&c, version)?;
    Ok(c)
}
pub fn source(root: &Path, id: &str) -> AppResult<Source> {
    serde_json::from_slice(&fs::read(source_dir(root, id)?.join("source.json"))?)
        .map_err(|e| AppError::new("logs", e))
}
pub fn stats(root: &Path, id: &str) -> AppResult<Stats> {
    let mut stats = Stats::default();
    let mut last_time = i64::MIN;
    for path in files(&source_dir(root, id)?)? {
        let c = reader(&path)?;
        let (count, seq): (i64, i64) =
            c.query_row("SELECT count,last_seq FROM totals WHERE id=1", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
        stats.records += count;
        stats.last_seq = stats.last_seq.max(seq);
        let (runs, interrupted): (i64, i64) = c.query_row(
            "SELECT count(*),coalesce(sum(ended_at IS NULL),0) FROM runs",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        stats.runs += runs;
        stats.interrupted_runs += interrupted;
        let ts: Option<(String, i64)> = c
            .query_row(
                "SELECT last_timestamp,last_time FROM totals WHERE id=1 AND last_time IS NOT NULL",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((value, time)) = ts {
            if time > last_time {
                last_time = time;
                stats.last_timestamp = Some(value);
            }
        }
        stats.bytes += fs::metadata(&path)?.len();
        if let Ok(meta) = fs::metadata(format!("{}-wal", path.display())) {
            stats.bytes += meta.len();
        }
    }
    Ok(stats)
}
pub fn archives(root: &Path) -> AppResult<Vec<Archive>> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            if let Some(id) = entry.file_name().to_str() {
                if id.len() == 64 {
                    result.push(Archive {
                        source: source(root, id)?,
                        stats: stats(root, id)?,
                    });
                }
            }
        }
    }
    result.sort_by(|a, b| {
        a.source
            .server_name
            .cmp(&b.source.server_name)
            .then(a.source.container_name.cmp(&b.source.container_name))
    });
    Ok(result)
}
pub fn since(timestamp: &str) -> Option<String> {
    chrono::DateTime::parse_from_rfc3339(timestamp)
        .ok()
        .map(|t| {
            (t - chrono::Duration::seconds(1)).to_rfc3339_opts(chrono::SecondsFormat::Nanos, true)
        })
}
pub(crate) fn time_ns(value: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()?
        .timestamp_nanos_opt()
}
pub fn plain_text(raw: &str) -> String {
    let mut state = 0;
    let mut text = String::new();
    for ch in raw.chars() {
        match state {
            1 => {
                state = match ch {
                    '[' => 2,
                    ']' => 3,
                    _ => 0,
                }
            }
            2 => {
                if ('@'..='~').contains(&ch) {
                    state = 0
                }
            }
            3 => {
                if ch == '\u{7}' {
                    state = 0
                } else if ch == '\u{1b}' {
                    state = 4
                }
            }
            4 => state = if ch == '\\' { 0 } else { 3 },
            _ => {
                if ch == '\u{1b}' {
                    state = 1
                } else if ch == '\t' || ch == '\n' || !ch.is_control() {
                    text.push(ch)
                }
            }
        }
    }
    text
}
pub fn level(text: &str) -> &'static str {
    let body = text
        .split_once(' ')
        .filter(|(p, _)| time_ns(p).is_some())
        .map_or(text, |(_, b)| b)
        .trim();
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(body) {
        for key in ["level", "severity", "log_level"] {
            if let Some(v) = value.get(key).and_then(|v| v.as_str()) {
                if let Some(l) = known_level(v) {
                    return l;
                }
            }
        }
    }
    // Only standalone/prefix level tokens, not an arbitrary mention inside prose.
    for token in body.split_whitespace().take(5) {
        if token.starts_with('[')
            || token.to_ascii_lowercase().starts_with("level=")
            || token == body.split_whitespace().next().unwrap_or("")
        {
            let token = token.trim_matches(|c: char| matches!(c, '[' | ']' | ':' | '"' | ','));
            let token = token
                .split_once('=')
                .map_or(token, |(_, v)| v.trim_matches('"'));
            if let Some(l) = known_level(token) {
                return l;
            }
        }
    }
    ""
}
fn known_level(value: &str) -> Option<&'static str> {
    match value.to_ascii_uppercase().as_str() {
        "ERROR" | "FATAL" | "CRITICAL" => Some("error"),
        "WARN" | "WARNING" => Some("warn"),
        "INFO" => Some("info"),
        "DEBUG" | "TRACE" => Some("debug"),
        _ => None,
    }
}

pub use crate::log_ingest::Store;

fn read_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<Record> {
    Ok(Record {
        seq: row.get(0)?,
        stream: row.get(1)?,
        raw: row.get(2)?,
        text: row.get(3)?,
        timestamp: row.get(4)?,
        level: row.get(5)?,
        complete: row.get(6)?,
        continuation: row.get(7)?,
        prefix: row.get(8)?,
    })
}
pub fn query(root: &Path, id: &str, q: &Query) -> AppResult<Page> {
    let snapshot = match q.snapshot {
        Some(seq) => seq,
        None => stats(root, id)?.last_seq,
    }
    .max(0);
    let forward = q.after.is_some();
    let start = if forward {
        q.after.unwrap_or(0)
    } else {
        q.before.unwrap_or(snapshot + 1)
    };
    let keyword = if q.case_sensitive {
        q.keyword.clone()
    } else {
        q.keyword.to_lowercase()
    };
    if keyword.len() > 4096 {
        return Err(AppError::new("validation", "搜索关键词过长"));
    }
    if !["", "stdout", "stderr"].contains(&q.stream.as_str()) {
        return Err(AppError::new("validation", "无效的输出流"));
    }
    let from = q.from.as_deref().map(time_ns).transpose_option()?;
    let to = q.to.as_deref().map(time_ns).transpose_option()?;
    if from.zip(to).is_some_and(|(a, b)| a > b) {
        return Err(AppError::new("validation", "开始时间不能晚于结束时间"));
    }
    let indexed = keyword.chars().count() >= 3;
    let fts = format!("\"{}\"", keyword.to_lowercase().replace('"', "\"\""));
    let limit = q.limit.clamp(1, 1000);
    let lower = if forward {
        start.saturating_add(1)
    } else {
        start.saturating_sub(SCAN_SIZE).max(1)
    };
    let upper = if forward {
        start.saturating_add(SCAN_SIZE).min(snapshot)
    } else {
        start.saturating_sub(1).min(snapshot)
    };
    let mut rows: Vec<Record> = Vec::new();
    for path in files(&source_dir(root, id)?)? {
        let c = reader(&path)?;
        let has_prefix: bool = c.query_row(
            "SELECT count(*)>0 FROM pragma_table_info('records') WHERE name='prefix'",
            [],
            |r| r.get(0),
        )?;
        let sql = format!(
            "SELECT seq,stream,raw,text,timestamp,level,complete,continuation,{} FROM records
   WHERE seq BETWEEN ?1 AND ?2 {} AND (?3='' OR stream=?3)
   AND (?4 IS NULL OR time_ns>=?4) AND (?5 IS NULL OR time_ns<=?5)
   AND (?6='' OR instr({},?6)>0) ORDER BY seq {} LIMIT ?8",
            if has_prefix { "prefix" } else { "''" },
            if indexed {
                "AND seq IN (SELECT rowid FROM search WHERE search MATCH ?7)"
            } else {
                "AND (?7 IS NOT NULL)"
            },
            if q.case_sensitive {
                if has_prefix {
                    "prefix||text"
                } else {
                    "text"
                }
            } else {
                "folded"
            },
            if forward { "ASC" } else { "DESC" }
        );
        let mut st = c.prepare(&sql)?;
        let mut payload = 0;
        for row in st.query_map(
            params![lower, upper, q.stream, from, to, keyword, fts, limit as i64],
            read_record,
        )? {
            let row = row?;
            payload += row.raw.len() + row.text.len() + row.prefix.len();
            rows.push(row);
            if payload >= 2 * 1024 * 1024 {
                break;
            }
        }
        rows.sort_by_key(|r| if forward { r.seq } else { -r.seq });
        let mut bytes = 0;
        let keep = rows
            .iter()
            .take(limit)
            .take_while(|r| {
                if bytes >= 2 * 1024 * 1024 {
                    return false;
                }
                bytes += r.raw.len() + r.text.len() + r.prefix.len();
                true
            })
            .count();
        rows.truncate(keep);
    }
    let payload: usize = rows
        .iter()
        .map(|r| r.raw.len() + r.text.len() + r.prefix.len())
        .sum();
    let cursor = if rows.len() >= limit || payload >= 2 * 1024 * 1024 {
        rows.last().map_or(start, |r| r.seq)
    } else if forward {
        upper.max(start)
    } else {
        lower.min(start)
    };
    let scanned = (cursor - start).abs().min(SCAN_SIZE);
    let has_more = if forward {
        cursor < snapshot
    } else {
        cursor > 1
    };
    rows.sort_by_key(|r| r.seq);
    Ok(Page {
        rows,
        next_cursor: cursor,
        has_more,
        scanned,
        snapshot,
    })
}
trait OptionalTime {
    fn transpose_option(self) -> AppResult<Option<i64>>;
}
impl OptionalTime for Option<Option<i64>> {
    fn transpose_option(self) -> AppResult<Option<i64>> {
        match self {
            Some(None) => Err(AppError::new("validation", "无效的日志时间范围")),
            Some(Some(t)) => Ok(Some(t)),
            None => Ok(None),
        }
    }
}

/// Export canonical original bytes. No frontend-size limit and no ANSI loss.
pub fn export(
    root: &Path,
    id: &str,
    path: &Path,
    from: Option<i64>,
    to: Option<i64>,
) -> AppResult<()> {
    use std::io::Write;
    let mut output = std::io::BufWriter::new(fs::File::create(path)?);
    for segment in files(&source_dir(root, id)?)? {
        let c = reader(&segment)?;
        if from.is_none() && to.is_none() {
            let version: i64 = c.query_row("PRAGMA user_version", [], |r| r.get(0))?;
            let mut st = c.prepare(if version >= 3 {
                "SELECT data FROM chunks ORDER BY message_seq,part"
            } else {
                "SELECT data FROM chunks ORDER BY id"
            })?;
            for bytes in st.query_map([], |r| r.get::<_, Vec<u8>>(0))? {
                output.write_all(&bytes?)?;
            }
        } else {
            let mut st = c.prepare(
                "SELECT raw,complete FROM records WHERE seq BETWEEN ?1 AND ?2 ORDER BY seq",
            )?;
            for row in st.query_map(params![from.unwrap_or(0), to.unwrap_or(i64::MAX)], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?))
            })? {
                let (raw, complete) = row?;
                output.write_all(raw.as_bytes())?;
                if complete {
                    output.write_all(b"\n")?;
                }
            }
        }
    }
    output.flush()?;
    output.get_ref().sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn source_fixture() -> Source {
        Source {
            source_id: "a".repeat(64),
            server_id: "test".into(),
            server_name: "server".into(),
            container_name: "web".into(),
            container_id: "b".repeat(64),
            endpoint: "unix:///docker.sock".into(),
            host: "localhost".into(),
            port: 22,
            tty: false,
        }
    }
    #[test]
    fn archive_survives_reopen_and_searches_old_chinese_and_repeated_records() {
        let dir = tempfile::tempdir().unwrap();
        let source = source_fixture();
        let mut store = Store::open(dir.path(), &source, "one", false).unwrap();
        let logs = format!(
            "2026-10-14T09:00:00.123456789Z [ERROR] 中文错误\n{}",
            (0..12000)
                .map(|n| format!("INFO row-{n}\n"))
                .collect::<String>()
        );
        store
            .append(&[
                (0, logs.into_bytes()),
                (1, b"WARN repeated\nWARN repeated\n".to_vec()),
            ])
            .unwrap();
        store.finish("closed").unwrap();
        drop(store);
        assert_eq!(stats(dir.path(), &source.source_id).unwrap().records, 12003);
        for keyword in ["中文错误", "错误", "错"] {
            let p = query(
                dir.path(),
                &source.source_id,
                &Query {
                    keyword: keyword.into(),
                    limit: 100,
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(p.rows.len(), 1);
            assert_eq!(p.rows[0].level, "error");
        }
        let p = query(
            dir.path(),
            &source.source_id,
            &Query {
                keyword: "repeated".into(),
                limit: 100,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(p.rows.len(), 2);
        let s = Store::open(dir.path(), &source, "two", false).unwrap();
        assert!(s
            .since
            .unwrap()
            .starts_with("2026-10-14T08:59:59.123456789"));
    }
    #[test]
    fn fragments_utf8_and_ansi_are_saved_without_raw_byte_loss_and_rotation_keeps_sequences() {
        let dir = tempfile::tempdir().unwrap();
        let source = source_fixture();
        let mut s = Store::open(dir.path(), &source, "run", false).unwrap();
        s.segment_size = 1;
        let raw = format!(
            "2026-10-14T09:00:00Z 中文 \x1b[32mgreen\x1b[0m\r\n{}\n",
            "长".repeat(50000)
        );
        for chunk in raw.as_bytes().chunks(317) {
            s.append(&[(0, chunk.to_vec())]).unwrap();
        }
        s.append(&[(1, b"no newline".to_vec())]).unwrap();
        s.finish("closed").unwrap();
        drop(s);
        let export_path = dir.path().join("out.log");
        export(dir.path(), &source.source_id, &export_path, None, None).unwrap();
        let mut expected = raw.as_bytes().to_vec();
        expected.extend_from_slice(b"no newline");
        assert_eq!(fs::read(export_path).unwrap(), expected);
        let p = query(
            dir.path(),
            &source.source_id,
            &Query {
                after: Some(0),
                limit: 100,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(p.rows.windows(2).all(|w| w[0].seq < w[1].seq));
        assert!(p.rows[0].text.contains("中文 green"));
        assert!(p.rows.iter().all(|row| !row.text.contains('�')));
        assert!(!p.rows.last().unwrap().complete);
        assert!(
            files(&source_dir(dir.path(), &source.source_id).unwrap())
                .unwrap()
                .len()
                > 1
        );
    }
    #[test]
    fn bounded_search_pages_snapshot_case_and_time_filters() {
        let dir = tempfile::tempdir().unwrap();
        let source = source_fixture();
        let mut s = Store::open(dir.path(), &source, "run", false).unwrap();
        s.append(&[(
            0,
            b"2026-10-14T09:00:00Z ERROR Needle\n2026-10-14T10:00:00Z INFO needle\n".to_vec(),
        )])
        .unwrap();
        let p = query(
            dir.path(),
            &source.source_id,
            &Query {
                keyword: "Needle".into(),
                case_sensitive: true,
                limit: 1,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(p.rows[0].seq, 1);
        let p = query(
            dir.path(),
            &source.source_id,
            &Query {
                keyword: "needle".into(),
                from: Some("2026-10-14T09:30:00Z".into()),
                limit: 100,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(p.rows[0].seq, 2);
        s.append(&[(0, b"INFO newest\n".to_vec())]).unwrap();
        let p = query(
            dir.path(),
            &source.source_id,
            &Query {
                snapshot: Some(2),
                limit: 100,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(p.rows.len(), 2);
        assert_eq!(level("this mentions an error in a sentence"), "");
        assert_eq!(level("{\"level\":\"fatal\",\"message\":\"oops\"}"), "error");
    }
    #[test]
    #[ignore = "writes one million real archived records; run explicitly for performance verification"]
    fn million_record_archive_remains_pageable_and_searchable() {
        let temporary = tempfile::tempdir().unwrap();
        let requested = std::env::var_os("SSH_WORKSPACE_QA_STRESS_DIR").map(PathBuf::from);
        let root = requested.as_deref().unwrap_or(temporary.path());
        let mut source = source_fixture();
        source.source_id = "f".repeat(64);
        source.container_name = "million-record-stress".into();
        source.server_name = "本地压力测试".into();
        let mut store = Store::open(root, &source, "million", false).unwrap();
        let started = std::time::Instant::now();
        for batch in 0..1000 {
            let bytes = (batch * 1000..batch * 1000 + 1000)
                .map(|n| {
                    format!(
                        "INFO pressure-{n:07} {}\n",
                        if n == 7 {
                            "唯一历史中文错误"
                        } else {
                            "正常输出"
                        }
                    )
                })
                .collect::<String>()
                .into_bytes();
            store.append(&[(0, bytes)]).unwrap();
        }
        store.finish("finished").unwrap();
        drop(store);
        assert_eq!(stats(root, &source.source_id).unwrap().records, 1_000_000);
        let page = query(
            root,
            &source.source_id,
            &Query {
                limit: 500,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(page.rows.len(), 500);
        assert_eq!(page.rows.last().unwrap().seq, 1_000_000);
        let mut before = None;
        let mut found = Vec::new();
        loop {
            let page = query(
                root,
                &source.source_id,
                &Query {
                    before,
                    keyword: "唯一历史中文错误".into(),
                    limit: 10,
                    ..Default::default()
                },
            )
            .unwrap();
            found.extend(page.rows);
            if !page.has_more {
                break;
            }
            before = Some(page.next_cursor);
        }
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].seq, 8);
        println!(
            "One million records persisted and searched in {:?}; archive: {}",
            started.elapsed(),
            root.display()
        );
    }
    #[test]
    fn search_matches_a_keyword_crossing_a_long_line_fragment() {
        let temporary = tempfile::tempdir().unwrap();
        let source = source_fixture();
        let mut store = Store::open(temporary.path(), &source, "long", false).unwrap();
        let raw = format!("{}Boundary-token 中文\n", "x".repeat(FRAGMENT_SIZE - 4));
        store.append(&[(0, raw.as_bytes().to_vec())]).unwrap();
        store.finish("end").unwrap();
        let page = query(
            temporary.path(),
            &source.source_id,
            &Query {
                keyword: "Boundary-token".into(),
                case_sensitive: true,
                limit: 10,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(page.rows.len(), 1);
        assert!((page.rows[0].prefix.clone() + &page.rows[0].text).contains("Boundary-token"));
    }
    fn all_rows(root: &Path, source: &Source) -> Vec<Record> {
        query(
            root,
            &source.source_id,
            &Query {
                after: Some(0),
                limit: 1000,
                ..Default::default()
            },
        )
        .unwrap()
        .rows
    }
    fn exported(root: &Path, source: &Source) -> Vec<u8> {
        let path = root.join("export.log");
        export(root, &source.source_id, &path, None, None).unwrap();
        fs::read(path).unwrap()
    }
    #[test]
    fn reconnect_deduplicates_occurrences_without_merging_real_repeated_lines() {
        let root = tempfile::tempdir().unwrap();
        let source = source_fixture();
        let line = b"2026-10-14T09:00:00.123456789Z identical\n";
        let mut s = Store::open(root.path(), &source, "first", true).unwrap();
        s.append(&[(0, line.repeat(2))]).unwrap();
        s.finish("closed").unwrap();
        drop(s);
        let mut s = Store::open(root.path(), &source, "second", false).unwrap();
        s.append(&[(0,line.repeat(3)),(1,line.to_vec()),(0,b"2026-10-14T09:00:00.123456789Z different\n2026-10-14T09:00:00.123456790Z identical\n".to_vec())]).unwrap();
        s.finish("closed").unwrap();
        drop(s);
        assert_eq!(stats(root.path(), &source.source_id).unwrap().records, 6);
        let before = exported(root.path(), &source);
        let mut s = Store::open(root.path(), &source, "third", false).unwrap();
        for bytes in line.repeat(3).chunks(7) {
            s.append(&[(0, bytes.to_vec())]).unwrap();
        }
        s.append(&[(1, line.to_vec())]).unwrap();
        s.finish("closed").unwrap();
        drop(s);
        assert_eq!(exported(root.path(), &source), before);
        assert_eq!(stats(root.path(), &source.source_id).unwrap().records, 6);
    }
    #[test]
    fn original_bytes_not_display_text_decide_duplicates() {
        let root = tempfile::tempdir().unwrap();
        let source = source_fixture();
        let bytes = b"2026-10-14T09:00:00Z \x1b[31mred\x1b[0m\xff\n2026-10-14T09:00:00Z red\xfe\n";
        for run in ["first", "second"] {
            let mut s = Store::open(root.path(), &source, run, false).unwrap();
            for part in bytes.chunks(3) {
                s.append(&[(0, part.to_vec())]).unwrap();
            }
            s.finish("closed").unwrap();
        }
        assert_eq!(stats(root.path(), &source.source_id).unwrap().records, 2);
        assert_eq!(exported(root.path(), &source), bytes);
        for run in ["third", "fourth"] {
            let mut s = Store::open(root.path(), &source, run, false).unwrap();
            s.append(&[(0, b"no timestamp\n".to_vec())]).unwrap();
            s.finish("closed").unwrap();
        }
        assert_eq!(stats(root.path(), &source.source_id).unwrap().records, 4);
    }
    #[test]
    fn stopped_mid_line_is_extended_in_place_and_staged_crash_tail_is_recovered() {
        let root = tempfile::tempdir().unwrap();
        let source = source_fixture();
        let line = format!("2026-10-14T09:00:00Z {} 中文尾部\n", "长".repeat(50000));
        {
            let mut s = Store::open(root.path(), &source, "first", true).unwrap();
            s.append(&[(0, line.as_bytes()[..100].to_vec())]).unwrap();
            assert_eq!(all_rows(root.path(), &source).len(), 1);
            // Drop without finish simulates application interruption.
            s.append(&[(0, line.as_bytes()[100..100000].to_vec())])
                .unwrap();
        }
        crate::log_ingest::recover_archive(root.path(), &source).unwrap();
        assert_eq!(exported(root.path(), &source), line.as_bytes()[..100000]);
        let mut s = Store::open(root.path(), &source, "second", true).unwrap();
        for bytes in line.as_bytes().chunks(317) {
            s.append(&[(0, bytes.to_vec())]).unwrap();
        }
        s.finish("closed").unwrap();
        drop(s);
        let before = stats(root.path(), &source.source_id).unwrap().records;
        let rows = all_rows(root.path(), &source);
        assert!(rows.iter().all(|r| !r.raw.contains('�')));
        assert_eq!(exported(root.path(), &source), line.as_bytes());
        let mut s = Store::open(root.path(), &source, "third", false).unwrap();
        s.append(&[(0, line.as_bytes().to_vec())]).unwrap();
        s.finish("closed").unwrap();
        drop(s);
        assert_eq!(
            stats(root.path(), &source.source_id).unwrap().records,
            before
        );
        assert_eq!(exported(root.path(), &source), line.as_bytes());
    }
    #[test]
    fn replay_stopped_mid_an_existing_complete_line_adds_no_tail() {
        let root = tempfile::tempdir().unwrap();
        let source = source_fixture();
        let line = b"2026-10-14T09:00:00Z existing complete log\n";
        let mut s = Store::open(root.path(), &source, "one", true).unwrap();
        s.append(&[(0, line.to_vec())]).unwrap();
        s.finish("closed").unwrap();
        drop(s);
        let mut s = Store::open(root.path(), &source, "two", false).unwrap();
        s.append(&[(0, line[..35].to_vec())]).unwrap();
        s.finish("closed").unwrap();
        drop(s);
        assert_eq!(exported(root.path(), &source), line);
        assert_eq!(all_rows(root.path(), &source).len(), 1);
    }
    #[test]
    fn deduplication_spans_segments_and_preserves_sequence_paging_after_tail_extension() {
        let root = tempfile::tempdir().unwrap();
        let source = source_fixture();
        let prefix = b"2026-10-14T09:00:00Z prefix";
        let later = b"2026-10-14T09:00:01Z later\n";
        let full = format!(
            "{}{}\n",
            std::str::from_utf8(prefix).unwrap(),
            "x".repeat(140000)
        );
        let mut s = Store::open(root.path(), &source, "one", true).unwrap();
        s.segment_size = 1;
        s.append(&[(0, prefix.to_vec())]).unwrap();
        s.finish("closed").unwrap();
        drop(s);
        let mut s = Store::open(root.path(), &source, "two", false).unwrap();
        s.segment_size = 1;
        s.append(&[(0, later.to_vec())]).unwrap();
        s.finish("closed").unwrap();
        drop(s);
        let mut s = Store::open(root.path(), &source, "three", true).unwrap();
        s.append(&[(0, full.as_bytes().to_vec()), (0, later.to_vec())])
            .unwrap();
        s.finish("closed").unwrap();
        drop(s);
        let rows = all_rows(root.path(), &source);
        assert_eq!(rows.len(), 4);
        assert!(rows.windows(2).all(|w| w[0].seq < w[1].seq));
        let mut cursor = None;
        let mut backward = Vec::new();
        loop {
            let p = query(
                root.path(),
                &source.source_id,
                &Query {
                    before: cursor,
                    limit: 1,
                    ..Default::default()
                },
            )
            .unwrap();
            backward.extend(p.rows.iter().map(|r| r.seq));
            if !p.has_more {
                break;
            }
            cursor = Some(p.next_cursor);
        }
        assert_eq!(backward, vec![4, 3, 2, 1]);
        let before = exported(root.path(), &source);
        let mut s = Store::open(root.path(), &source, "four", false).unwrap();
        s.segment_size = 1;
        s.append(&[(0, full.as_bytes().to_vec()), (0, later.to_vec())])
            .unwrap();
        s.finish("closed").unwrap();
        drop(s);
        assert_eq!(exported(root.path(), &source), before);
    }
    #[test]
    fn legacy_archive_gets_byte_exact_identity_index_and_history_sync_marker() {
        let root = tempfile::tempdir().unwrap();
        let source = source_fixture();
        let dir = source_dir(root.path(), &source.source_id).unwrap();
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("source.json"),
            serde_json::to_vec(&source).unwrap(),
        )
        .unwrap();
        let c = Connection::open(dir.join("segment-00000001.db")).unwrap();
        c.execute_batch(SCHEMA).unwrap();
        c.execute_batch("PRAGMA user_version=2").unwrap();
        let line = b"2026-10-14T09:00:00Z old \xff\n";
        let raw = String::from_utf8_lossy(line.strip_suffix(b"\n").unwrap());
        c.execute("INSERT INTO records(seq,stream,raw,text,folded,prefix,timestamp,time_ns,level,complete,continuation,replayed,run_id,received_at) VALUES(1,'stdout',?1,?1,?1,'','2026-10-14T09:00:00Z',?2,'',1,0,0,'old',0)",params![raw,time_ns("2026-10-14T09:00:00Z")]).unwrap();
        c.execute(
            "INSERT INTO chunks(run_id,stream,data,received_at) VALUES('old','stdout',?1,0)",
            [line.as_slice()],
        )
        .unwrap();
        drop(c);
        assert!(!crate::log_ingest::history_complete(root.path(), &source.source_id).unwrap());
        let mut s = Store::open(root.path(), &source, "new", true).unwrap();
        s.append(&[(0, line.to_vec())]).unwrap();
        s.mark_history_complete().unwrap();
        s.finish("done").unwrap();
        drop(s);
        assert_eq!(exported(root.path(), &source), line);
        assert_eq!(all_rows(root.path(), &source).len(), 1);
        assert!(crate::log_ingest::history_complete(root.path(), &source.source_id).unwrap());
    }
}

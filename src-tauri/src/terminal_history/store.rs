use super::model::*;
use crate::model::{AppError, AppResult};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

pub const SHARD_BYTES: u64 = 256 * 1024 * 1024;
const CATALOG: &str = "
CREATE TABLE IF NOT EXISTS sessions (
 id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL, source_id TEXT NOT NULL, source TEXT NOT NULL,
 started INTEGER NOT NULL, ended INTEGER, state TEXT NOT NULL, saved INTEGER NOT NULL DEFAULT 0,
 indexed INTEGER NOT NULL DEFAULT 0, last_record INTEGER NOT NULL DEFAULT 0, bytes INTEGER NOT NULL DEFAULT 0, error TEXT);
CREATE INDEX IF NOT EXISTS session_source ON sessions(source_id,started);
CREATE INDEX IF NOT EXISTS session_workspace ON sessions(workspace_id,started);
PRAGMA user_version=1;";
const RAW: &str = "CREATE TABLE IF NOT EXISTS events(seq INTEGER PRIMARY KEY,time INTEGER NOT NULL,elapsed INTEGER NOT NULL,kind TEXT NOT NULL,data BLOB NOT NULL);";
const VIEW: &str = "
CREATE TABLE IF NOT EXISTS records(id INTEGER PRIMARY KEY,seq INTEGER NOT NULL,offset INTEGER NOT NULL,time INTEGER NOT NULL,segment INTEGER NOT NULL,kind TEXT NOT NULL,text TEXT NOT NULL,ansi TEXT NOT NULL,folded TEXT NOT NULL);
CREATE VIRTUAL TABLE IF NOT EXISTS search USING fts5(folded,content='records',content_rowid='id',tokenize='trigram');
CREATE TRIGGER IF NOT EXISTS record_insert AFTER INSERT ON records BEGIN INSERT INTO search(rowid,folded) VALUES(new.id,new.folded); END;
CREATE TABLE IF NOT EXISTS progress(seq INTEGER PRIMARY KEY,last_record INTEGER NOT NULL);
CREATE INDEX IF NOT EXISTS record_time ON records(time);";

fn db(path: &Path, schema: &str) -> AppResult<Connection> {
    let c = Connection::open(path)?;
    c.busy_timeout(Duration::from_secs(5))?;
    c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;")?;
    c.execute_batch(schema)?;
    Ok(c)
}
pub fn reader(path: &Path) -> AppResult<Connection> {
    let c = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    c.busy_timeout(Duration::from_millis(1500))?;
    Ok(c)
}
fn shard(path: &Path, schema: &str) -> AppResult<Connection> {
    if !path.exists() {
        // Readers enumerate published names. Never expose a new shard until its schema exists.
        let temporary = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
        let connection = db(temporary.path(), schema)?;
        connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
        drop(connection);
        temporary
            .persist_noclobber(path)
            .map_err(|e| AppError::new("history", e))?;
    }
    db(path, schema)
}
pub fn init(root: &Path) -> AppResult<()> {
    fs::create_dir_all(root.join("sessions"))?;
    let c = db(&root.join("catalog.sqlite"), CATALOG)?;
    c.execute("UPDATE sessions SET state='interrupted',error=coalesce(error,'上次异常中断；仅保证已保存的记录') WHERE state='recording'", [])?;
    Ok(())
}
pub fn catalog(root: &Path) -> AppResult<Connection> {
    db(&root.join("catalog.sqlite"), CATALOG)
}
pub fn session_dir(root: &Path, id: &str) -> AppResult<PathBuf> {
    if id.is_empty()
        || id.len() > 128
        || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err(AppError::new("history", "无效的历史会话标识"));
    }
    Ok(root.join("sessions").join(id))
}
pub fn register(root: &Path, session: &Session) -> AppResult<()> {
    fs::create_dir_all(session_dir(root, &session.id)?)?;
    catalog(root)?.execute("INSERT INTO sessions(id,workspace_id,source_id,source,started,state) VALUES(?1,?2,?3,?4,?5,'recording')", params![session.id,session.workspace_id,session.source.id,serde_json::to_string(&session.source).unwrap(),session.started_at])?;
    Ok(())
}
fn read_session(row: &rusqlite::Row<'_>) -> rusqlite::Result<Session> {
    let json: String = row.get(3)?;
    let source = serde_json::from_str(&json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(e))
    })?;
    Ok(Session {
        id: row.get(0)?,
        workspace_id: row.get(1)?,
        source,
        started_at: row.get(4)?,
        ended_at: row.get(5)?,
        state: row.get(6)?,
        saved_seq: row.get::<_, i64>(7)?.to_string(),
        indexed_seq: row.get::<_, i64>(8)?.to_string(),
        last_record: row.get::<_, i64>(9)?.to_string(),
        bytes: row.get::<_, i64>(10)? as u64,
        error: row.get(11)?,
    })
}
pub fn sessions(root: &Path, query: &Query) -> AppResult<Vec<Session>> {
    let c = reader(&root.join("catalog.sqlite"))?;
    let mut st = c.prepare("SELECT * FROM sessions WHERE (?1 IS NULL OR id=?1) AND (?2 IS NULL OR source_id=?2) AND (?3 IS NULL OR workspace_id=?3) AND (?4 IS NULL OR coalesce(ended,9223372036854775807)>=?4) AND (?5 IS NULL OR started<=?5) ORDER BY started DESC,id")?;
    let rows = st
        .query_map(
            params![
                query.session_id,
                query.source_id,
                query.workspace_id,
                query.from,
                query.to
            ],
            read_session,
        )?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}
pub fn get_session(root: &Path, id: &str) -> AppResult<Session> {
    sessions(
        root,
        &Query {
            session_id: Some(id.into()),
            ..Default::default()
        },
    )?
    .into_iter()
    .next()
    .ok_or_else(|| AppError::new("history", "终端历史不存在"))
}
pub fn error(root: &Path, id: &str, message: &str) {
    if let Ok(c) = catalog(root) {
        let _ = c.execute(
            "UPDATE sessions SET error=?2 WHERE id=?1",
            params![id, message],
        );
    }
}
pub fn parts(root: &Path, id: &str, kind: &str) -> AppResult<Vec<PathBuf>> {
    let dir = session_dir(root, id)?;
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut paths = fs::read_dir(dir)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(&format!("{kind}-")) && n.ends_with(".sqlite"))
        })
        .collect::<Vec<_>>();
    paths.sort();
    Ok(paths)
}
pub struct ShardWriter {
    pub connection: Connection,
    root: PathBuf,
    id: String,
    kind: &'static str,
    part: usize,
    bytes: u64,
    pub limit: u64,
}
impl ShardWriter {
    pub fn new(root: &Path, id: &str, kind: &'static str) -> AppResult<Self> {
        let paths = parts(root, id, kind)?;
        let part = paths.len().max(1);
        let path = session_dir(root, id)?.join(format!("{kind}-{part:08}.sqlite"));
        let bytes = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        Ok(Self {
            connection: shard(&path, if kind == "raw" { RAW } else { VIEW })?,
            root: root.into(),
            id: id.into(),
            kind,
            part,
            bytes,
            limit: SHARD_BYTES,
        })
    }
    fn rotate(&mut self) -> AppResult<()> {
        if self.bytes < self.limit {
            return Ok(());
        }
        self.connection
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
        self.part += 1;
        let path = session_dir(&self.root, &self.id)?
            .join(format!("{}-{:08}.sqlite", self.kind, self.part));
        self.connection = shard(&path, if self.kind == "raw" { RAW } else { VIEW })?;
        self.bytes = 0;
        Ok(())
    }
    pub fn append_events(&mut self, events: &[Event]) -> AppResult<()> {
        if events.is_empty() {
            return Ok(());
        }
        self.rotate()?;
        let tx = self.connection.transaction()?;
        let mut bytes = 0;
        for event in events {
            let (kind, data) = match &event.kind {
                EventKind::Output { bytes } => ("output", bytes.clone()),
                kind => ("event", serde_json::to_vec(kind).unwrap()),
            };
            tx.execute(
                "INSERT INTO events VALUES(?1,?2,?3,?4,?5)",
                params![event.seq, event.time, event.elapsed, kind, data],
            )?;
            bytes += data.len() as u64;
        }
        tx.commit()?;
        self.bytes += bytes;
        if events
            .iter()
            .any(|e| matches!(e.kind, EventKind::Gap { .. }))
        {
            error(
                &self.root,
                &self.id,
                "历史记录存在缺口，请查看记录中的缺口标记",
            );
        }
        let last = events.last().unwrap();
        let ended = events
            .iter()
            .any(|e| matches!(e.kind, EventKind::End { .. }));
        catalog(&self.root)?.execute("UPDATE sessions SET saved=?2,bytes=bytes+?3,state=CASE WHEN ?4 THEN 'closed' ELSE state END,ended=CASE WHEN ?4 THEN ?5 ELSE ended END WHERE id=?1",params![self.id,last.seq,bytes as i64,ended,last.time])?;
        Ok(())
    }
    pub fn append_rows(&mut self, rows: &[Row], seq: i64, last_record: i64) -> AppResult<()> {
        self.rotate()?;
        let tx = self.connection.transaction()?;
        for row in rows {
            tx.execute(
                "INSERT OR IGNORE INTO records VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                params![
                    row.id.parse::<i64>().unwrap(),
                    row.seq.parse::<i64>().unwrap(),
                    row.offset as i64,
                    row.time,
                    row.segment as i64,
                    row.kind,
                    row.text,
                    row.ansi,
                    row.text.to_lowercase()
                ],
            )?;
        }
        tx.execute(
            "INSERT OR REPLACE INTO progress VALUES(?1,?2)",
            params![seq, last_record],
        )?;
        tx.commit()?;
        self.bytes += rows
            .iter()
            .map(|r| (r.text.len() + r.ansi.len()) as u64 * 3 + 256)
            .sum::<u64>();
        catalog(&self.root)?.execute(
            "UPDATE sessions SET indexed=?2,last_record=?3 WHERE id=?1",
            params![self.id, seq, last_record],
        )?;
        Ok(())
    }
}
pub fn read_events(root: &Path, id: &str, after: i64, limit: usize) -> AppResult<Vec<Event>> {
    let mut events = Vec::new();
    for path in parts(root, id, "raw")? {
        let c = reader(&path)?;
        let mut st = c.prepare(
            "SELECT seq,time,elapsed,kind,data FROM events WHERE seq>?1 ORDER BY seq LIMIT ?2",
        )?;
        let rows = st.query_map(params![after, (limit - events.len()) as i64], |r| {
            let kind: String = r.get(3)?;
            let data: Vec<u8> = r.get(4)?;
            let kind = if kind == "output" {
                EventKind::Output { bytes: data }
            } else {
                serde_json::from_slice(&data).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        4,
                        rusqlite::types::Type::Blob,
                        Box::new(e),
                    )
                })?
            };
            Ok(Event {
                seq: r.get(0)?,
                time: r.get(1)?,
                elapsed: r.get(2)?,
                kind,
            })
        })?;
        events.extend(rows.collect::<Result<Vec<_>, _>>()?);
        if events.len() >= limit {
            break;
        }
    }
    Ok(events)
}
pub fn parse_seq(s: Option<&str>, default: i64) -> AppResult<i64> {
    s.map(|s| {
        s.parse::<i64>()
            .ok()
            .filter(|n| *n >= 0)
            .ok_or_else(|| AppError::new("history", "无效的历史位置"))
    })
    .unwrap_or(Ok(default))
}
pub fn page(
    root: &Path,
    id: &str,
    q: &Query,
    cancel: &std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> AppResult<Page> {
    let session = get_session(root, id)?;
    let snapshot = parse_seq(
        q.snapshot.as_deref(),
        session.last_record.parse().unwrap_or(0),
    )?;
    let before = parse_seq(q.before.as_deref(), i64::MAX)?;
    let after = parse_seq(q.after.as_deref(), 0)?;
    let limit = q.limit.clamp(1, 500);
    let ascending = q.after.is_some();
    let mut paths = parts(root, id, "view")?;
    if !ascending {
        paths.reverse();
    }
    let mut result = Vec::new();
    let folded = q.keyword.to_lowercase();
    let needle = if q.case_sensitive {
        &q.keyword
    } else {
        &folded
    };
    let fts = if folded.chars().count() >= 3 {
        " AND id IN (SELECT rowid FROM search WHERE search MATCH ?7)".to_string()
    } else {
        String::new()
    };
    let mut response_bytes = 0;
    let mut byte_limit = false;
    for path in paths {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(AppError::new("cancelled", "查询已取消"));
        }
        let c = reader(&path)?;
        c.progress_handler(
            2000,
            Some({
                let cancel = cancel.clone();
                move || cancel.load(std::sync::atomic::Ordering::Relaxed)
            }),
        )?;
        let sql=format!("SELECT id,seq,offset,time,segment,kind,text,ansi FROM records WHERE id<=?1 AND id<?2 AND id>?3 AND (?4 IS NULL OR time>=?4) AND (?5 IS NULL OR time<=?5) AND instr({},?6)>0{} ORDER BY id {} LIMIT ?8",if q.case_sensitive{"text"}else{"folded"},fts,if ascending{"ASC"}else{"DESC"});
        let mut st = c.prepare(&sql)?;
        let rows = st.query_map(
            params![
                snapshot,
                before,
                after,
                q.from,
                q.to,
                needle,
                format!("\"{}\"", folded.replace('"', "\"\"")),
                (limit + 1 - result.len()) as i64
            ],
            |r| {
                Ok(Row {
                    id: r.get::<_, i64>(0)?.to_string(),
                    session_id: id.into(),
                    seq: r.get::<_, i64>(1)?.to_string(),
                    offset: r.get::<_, i64>(2)? as usize,
                    time: r.get(3)?,
                    segment: r.get::<_, i64>(4)? as u64,
                    kind: r.get(5)?,
                    text: r.get(6)?,
                    ansi: r.get(7)?,
                })
            },
        )?;
        for row in rows {
            let row = row?;
            let size = row.text.len() + row.ansi.len() + 256;
            if !result.is_empty() && response_bytes + size > 4 * 1024 * 1024 {
                byte_limit = true;
                break;
            }
            response_bytes += size;
            result.push(row);
        }
        if result.len() > limit || byte_limit {
            break;
        }
    }
    let has_more = result.len() > limit || byte_limit;
    result.truncate(limit);
    if !ascending {
        result.reverse();
    }
    Ok(Page {
        rows: result,
        has_more,
        snapshot: snapshot.to_string(),
    })
}

// Reconcile metadata after an interrupted commit between a shard and the catalog.
pub fn recover(root: &Path) -> AppResult<()> {
    for session in sessions(root, &Query::default())? {
        let mut saved = 0;
        let mut indexed = 0;
        let mut last_record = 0;
        for path in parts(root, &session.id, "raw")? {
            let c = reader(&path)?;
            saved = saved.max(
                c.query_row("SELECT coalesce(max(seq),0) FROM events", [], |r| {
                    r.get::<_, i64>(0)
                })?,
            );
        }
        for path in parts(root, &session.id, "view")? {
            let c = reader(&path)?;
            if let Some((s, r)) = c
                .query_row(
                    "SELECT seq,last_record FROM progress ORDER BY seq DESC LIMIT 1",
                    [],
                    |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
                )
                .optional()?
            {
                if s > indexed {
                    indexed = s;
                    last_record = r;
                }
            }
        }
        catalog(root)?.execute(
            "UPDATE sessions SET saved=?2,indexed=?3,last_record=?4 WHERE id=?1",
            params![session.id, saved, indexed, last_record],
        )?;
        if session.state == "interrupted" {
            let already_ended = read_events(root, &session.id, saved.saturating_sub(1), 1)?
                .last()
                .is_some_and(|e| matches!(e.kind, EventKind::End { .. }));
            if !already_ended {
                // Flush a persisted partial line through the normal deterministic projector.
                // The missing in-memory tail is explicitly outside the recovered recording.
                let mut writer = ShardWriter::new(root, &session.id, "raw")?;
                writer.append_events(&[Event {
                    seq: saved + 1,
                    time: now(),
                    elapsed: 0,
                    kind: EventKind::End {
                        reason: "上次异常中断；最后未提交的输出无法恢复".into(),
                    },
                }])?;
                catalog(root)?.execute(
                    "UPDATE sessions SET state='interrupted' WHERE id=?1",
                    params![session.id],
                )?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn fixture(id: &str) -> Session {
    Session {
        id: id.into(),
        workspace_id: "workspace-a".into(),
        source: Source {
            id: "source-a".into(),
            server_id: "server-a".into(),
            server_name: "测试服务器".into(),
            address: "127.0.0.1:22".into(),
            username: "ops".into(),
            container_name: None,
            container_id: None,
            endpoint: None,
        },
        started_at: now(),
        ended_at: None,
        state: "recording".into(),
        saved_seq: "0".into(),
        indexed_seq: "0".into(),
        last_record: "0".into(),
        bytes: 0,
        error: None,
    }
}
#[cfg(test)]
mod tests {
    use super::super::projection::Projection;
    use super::*;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    #[test]
    fn durable_shards_search_old_chinese_and_keep_raw_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        init(root).unwrap();
        register(root, &fixture("session-a")).unwrap();
        let mut raw = ShardWriter::new(root, "session-a", "raw").unwrap();
        raw.limit = 16 * 1024;
        let mut view = ShardWriter::new(root, "session-a", "view").unwrap();
        view.limit = 128 * 1024;
        let mut projector = Projection::new("session-a");
        let mut original = Vec::<u8>::new();
        let mut seq = 0;
        for batch in 0..120 {
            let data = (0..100)
                .map(|n| format!("旧历史 {} value-{}\r\n", batch * 100 + n, batch))
                .collect::<String>()
                .into_bytes();
            original.extend(&data);
            seq += 1;
            let e = Event {
                seq,
                time: now(),
                elapsed: seq,
                kind: EventKind::Output { bytes: data },
            };
            raw.append_events(&[e.clone()]).unwrap();
            let rows = projector.apply(&e);
            view.append_rows(&rows, seq, projector.last_id()).unwrap();
        }
        let last = Event {
            seq: seq + 1,
            time: now(),
            elapsed: 0,
            kind: EventKind::Output {
                bytes: b"\xff\x1b[2Jtail\r\n".to_vec(),
            },
        };
        original.extend(b"\xff\x1b[2Jtail\r\n");
        raw.append_events(&[last.clone()]).unwrap();
        let rows = projector.apply(&last);
        view.append_rows(&rows, last.seq, projector.last_id())
            .unwrap();
        drop(raw);
        drop(view);
        assert!(parts(root, "session-a", "raw").unwrap().len() > 1);
        assert!(parts(root, "session-a", "view").unwrap().len() > 1);
        let cancel = Arc::new(AtomicBool::new(false));
        let found = page(
            root,
            "session-a",
            &Query {
                keyword: "旧历史 15 value-0".into(),
                limit: 5,
                ..Default::default()
            },
            &cancel,
        )
        .unwrap();
        assert_eq!(found.rows.len(), 1);
        assert!(found.rows[0].text.contains("旧历史 15 value-0"));
        let old = page(
            root,
            "session-a",
            &Query {
                keyword: "历史".into(),
                after: Some("0".into()),
                limit: 500,
                ..Default::default()
            },
            &cancel,
        )
        .unwrap();
        assert_eq!(old.rows.len(), 500);
        assert!(old.has_more);
        let newest = page(
            root,
            "session-a",
            &Query {
                limit: 20,
                ..Default::default()
            },
            &cancel,
        )
        .unwrap();
        assert!(newest.rows.last().unwrap().text.contains("tail"));
        let events = read_events(root, "session-a", 0, 1000).unwrap();
        let restored = events
            .into_iter()
            .flat_map(|e| match e.kind {
                EventKind::Output { bytes } => bytes,
                _ => vec![],
            })
            .collect::<Vec<_>>();
        assert_eq!(original, restored);
        // Metadata can lag a successful shard commit when a process is killed.
        catalog(root)
            .unwrap()
            .execute("UPDATE sessions SET saved=0,indexed=0,last_record=0", [])
            .unwrap();
        recover(root).unwrap();
        let session = get_session(root, "session-a").unwrap();
        assert_eq!(session.saved_seq, (seq + 1).to_string());
        assert_eq!(session.last_record, projector.last_id().to_string());
        cancel.store(true, Ordering::Relaxed);
        assert!(page(
            root,
            "session-a",
            &Query {
                limit: 5,
                ..Default::default()
            },
            &cancel
        )
        .is_err());
    }
    #[test]
    fn query_snapshot_identity_case_and_path_bounds() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        init(root).unwrap();
        register(root, &fixture("a")).unwrap();
        register(root, &fixture("b")).unwrap();
        let mut writer = ShardWriter::new(root, "a", "view").unwrap();
        for (id, text) in [(1, "ERROR 中文"), (2, "error 中文"), (3, "error 中文")] {
            writer
                .append_rows(
                    &[Row {
                        id: id.to_string(),
                        session_id: "a".into(),
                        seq: id.to_string(),
                        offset: 0,
                        time: id,
                        segment: 0,
                        kind: "text".into(),
                        text: text.into(),
                        ansi: String::new(),
                    }],
                    id,
                    id,
                )
                .unwrap();
        }
        let cancel = Arc::new(AtomicBool::new(false));
        assert_eq!(
            page(
                root,
                "a",
                &Query {
                    keyword: "ERROR".into(),
                    case_sensitive: true,
                    limit: 10,
                    ..Default::default()
                },
                &cancel
            )
            .unwrap()
            .rows
            .len(),
            1
        );
        assert_eq!(
            page(
                root,
                "a",
                &Query {
                    keyword: "error".into(),
                    snapshot: Some("2".into()),
                    limit: 10,
                    ..Default::default()
                },
                &cancel
            )
            .unwrap()
            .rows
            .len(),
            2
        );
        assert!(page(
            root,
            "b",
            &Query {
                limit: 10,
                ..Default::default()
            },
            &cancel
        )
        .unwrap()
        .rows
        .is_empty());
        assert!(session_dir(root, "../outside").is_err());
    }
    #[test]
    #[ignore = "writes one million indexed records; run for performance verification"]
    fn million_records_remain_pageable() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        init(root).unwrap();
        register(root, &fixture("million")).unwrap();
        let mut writer = ShardWriter::new(root, "million", "view").unwrap();
        for batch in 0..1000 {
            let rows = (0..1000)
                .map(|n| {
                    let id = batch * 1000 + n + 1;
                    Row {
                        id: id.to_string(),
                        session_id: "million".into(),
                        seq: id.to_string(),
                        offset: 0,
                        time: id,
                        segment: 0,
                        kind: "text".into(),
                        text: format!("历史记录编号 {id}"),
                        ansi: String::new(),
                    }
                })
                .collect::<Vec<_>>();
            let last = (batch + 1) * 1000;
            writer.append_rows(&rows, last, last).unwrap();
        }
        let page = page(
            root,
            "million",
            &Query {
                keyword: "历史记录编号 12345".into(),
                limit: 50,
                ..Default::default()
            },
            &Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        assert!(page.rows.iter().any(|r| r.text == "历史记录编号 12345"));
    }
}

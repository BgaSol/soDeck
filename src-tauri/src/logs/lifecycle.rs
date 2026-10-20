use super::protocol::ContainerObservation;
use crate::{
    log_store,
    model::{AppError, AppResult},
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::path::Path;
const SCHEMA:&str="CREATE TABLE IF NOT EXISTS events(id INTEGER PRIMARY KEY,kind TEXT NOT NULL,observed_at INTEGER NOT NULL,actual_at TEXT,anchor_seq INTEGER NOT NULL,detail TEXT NOT NULL); CREATE TABLE IF NOT EXISTS boots(id TEXT PRIMARY KEY,started_at TEXT NOT NULL,observed_at INTEGER NOT NULL,finished_at TEXT); CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY,value TEXT NOT NULL); PRAGMA user_version=1;";
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub id: i64,
    pub kind: String,
    pub observed_at: i64,
    pub actual_at: Option<String>,
    pub anchor_seq: i64,
    pub detail: String,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Boot {
    pub id: String,
    pub started_at: String,
    pub observed_at: i64,
    pub finished_at: Option<String>,
}
#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Timeline {
    pub events: Vec<Event>,
    pub boots: Vec<Boot>,
    pub next_cursor: i64,
    pub has_more: bool,
}
fn writer(root: &Path, source: &str) -> AppResult<Connection> {
    let dir = log_store::source_dir(root, source)?;
    std::fs::create_dir_all(&dir)?;
    let c = Connection::open(dir.join("lifecycle.db"))?;
    c.busy_timeout(std::time::Duration::from_secs(5))?;
    let v: i64 = c.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if v > 1 {
        return Err(AppError::new("logs", "运行事件由较新版本创建"));
    }
    c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;")?;
    c.execute_batch(SCHEMA)?;
    Ok(c)
}
pub fn record(
    root: &Path,
    source: &str,
    kind: &str,
    detail: &str,
    actual: Option<&str>,
) -> AppResult<()> {
    let c = writer(root, source)?;
    c.execute_batch("BEGIN IMMEDIATE")?;
    let previous: Option<(String, String)> = c
        .query_row(
            "SELECT kind,detail FROM events ORDER BY id DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if previous
        .as_ref()
        .is_some_and(|(k, d)| k == kind && d == detail)
    {
        c.execute_batch("COMMIT")?;
        return Ok(());
    }
    let seq = log_store::stats(root, source)?.last_seq;
    c.execute(
        "INSERT INTO events(kind,observed_at,actual_at,anchor_seq,detail) VALUES(?1,?2,?3,?4,?5)",
        params![
            kind,
            log_store::now(),
            actual,
            seq,
            detail.chars().take(8192).collect::<String>()
        ],
    )?;
    c.execute_batch("COMMIT")?;
    Ok(())
}
pub fn observe(
    root: &Path,
    source: &str,
    o: &ContainerObservation,
    labels: Option<&serde_json::Value>,
) -> AppResult<bool> {
    let c = writer(root, source)?;
    let prior: Option<String> = c
        .query_row("SELECT value FROM meta WHERE key='startedAt'", [], |r| {
            r.get(0)
        })
        .optional()?;
    c.execute_batch("BEGIN IMMEDIATE")?;
    if let Some(labels) = labels {
        let mut identity = serde_json::Map::new();
        for key in [
            "com.docker.compose.project",
            "com.docker.compose.service",
            "com.docker.compose.container-number",
        ] {
            if let Some(value) = labels[key].as_str() {
                identity.insert(key.into(), serde_json::Value::String(value.into()));
            }
        }
        let labels = serde_json::Value::Object(identity);
        c.execute("INSERT INTO meta VALUES('labels',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[labels.to_string()])?;
    }
    let prior_restart: Option<u64> = c
        .query_row("SELECT value FROM meta WHERE key='restartCount'", [], |r| {
            r.get::<_, String>(0)
        })
        .optional()?
        .and_then(|s| s.parse().ok());
    let changed = o.started_at.is_some() && o.started_at != prior;
    if changed
        && prior_restart
            .is_some_and(|n| o.restart_count >= n && o.restart_count <= n.saturating_add(1))
    {
        if let (Some(previous), Some(start), Some(finish)) = (&prior, &o.started_at, &o.finished_at)
        {
            if log_store::time_ns(finish) > log_store::time_ns(previous)
                && log_store::time_ns(finish) < log_store::time_ns(start)
            {
                c.execute(
                    "UPDATE boots SET finished_at=?1 WHERE id=?2",
                    params![finish, previous],
                )?;
            }
        }
    }
    c.execute("INSERT INTO meta VALUES('restartCount',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[o.restart_count.to_string()])?;
    if let Some(start) = &o.started_at {
        c.execute(
            "INSERT OR IGNORE INTO boots VALUES(?1,?1,?2,NULL)",
            params![start, o.observed_at],
        )?;
        if let Some(end) = &o.finished_at {
            if log_store::time_ns(end) > log_store::time_ns(start)
                && o.state != "running"
                && o.state != "restarting"
            {
                c.execute(
                    "UPDATE boots SET finished_at=?1 WHERE id=?2",
                    params![end, start],
                )?;
            }
        }
        c.execute("INSERT INTO meta VALUES('startedAt',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[start])?;
    }
    c.execute_batch("COMMIT")?;
    if changed && prior.is_some() {
        record(
            root,
            source,
            "started",
            "检测到新的启动周期",
            o.started_at.as_deref(),
        )?;
    }
    Ok(changed && prior.is_some())
}
pub fn labels(root: &Path, source: &str) -> AppResult<Option<serde_json::Value>> {
    let p = log_store::source_dir(root, source)?.join("lifecycle.db");
    if !p.exists() {
        return Ok(None);
    }
    let c = log_store::reader(&p)?;
    let text: Option<String> = c
        .query_row("SELECT value FROM meta WHERE key='labels'", [], |r| {
            r.get(0)
        })
        .optional()?;
    Ok(text.and_then(|s| serde_json::from_str(&s).ok()))
}
pub fn timeline(root: &Path, source: &str, after: i64, before: Option<i64>) -> AppResult<Timeline> {
    let p = log_store::source_dir(root, source)?.join("lifecycle.db");
    if !p.exists() {
        return Ok(Timeline::default());
    }
    let c = log_store::reader(&p)?;
    let mut events=c.prepare("SELECT id,kind,observed_at,actual_at,anchor_seq,detail FROM events WHERE id>?1 AND (?2 IS NULL OR id<=?2) ORDER BY id LIMIT 501")?.query_map(params![after,before],|r|Ok(Event{id:r.get(0)?,kind:r.get(1)?,observed_at:r.get(2)?,actual_at:r.get(3)?,anchor_seq:r.get(4)?,detail:r.get(5)?}))?.collect::<Result<Vec<_>,_>>()?;
    let has_more = events.len() > 500;
    events.truncate(500);
    let next_cursor = events.last().map_or(after, |e| e.id);
    let boots=c.prepare("SELECT id,started_at,observed_at,finished_at FROM boots ORDER BY observed_at DESC,id DESC LIMIT 100")?.query_map([],|r|Ok(Boot{id:r.get(0)?,started_at:r.get(1)?,observed_at:r.get(2)?,finished_at:r.get(3)?}))?.collect::<Result<Vec<_>,_>>()?;
    Ok(Timeline {
        events,
        boots,
        next_cursor,
        has_more,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn offline_timeline_does_not_create_files() {
        let d = tempfile::tempdir().unwrap();
        assert!(timeline(d.path(), &"a".repeat(64), 0, None)
            .unwrap()
            .events
            .is_empty());
        assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 0);
    }
    #[test]
    fn baseline_is_not_a_restart_and_missing_cycles_are_not_invented() {
        let root = tempfile::tempdir().unwrap();
        let source = "a".repeat(64);
        let first = ContainerObservation {
            state: "running".into(),
            started_at: Some("2026-10-20T01:00:00Z".into()),
            finished_at: None,
            restart_count: 0,
            observed_at: 1,
        };
        let labels = serde_json::json!({"com.docker.compose.project":"project","secret-label":"do-not-save"});
        assert!(!observe(root.path(), &source, &first, Some(&labels)).unwrap());
        let mut next = first.clone();
        next.started_at = Some("2026-10-20T02:00:00Z".into());
        next.finished_at = Some("2026-10-20T01:59:00Z".into());
        next.restart_count = 3;
        next.observed_at = 2;
        assert!(observe(root.path(), &source, &next, None).unwrap());
        let timeline = timeline(root.path(), &source, 0, None).unwrap();
        assert_eq!(timeline.events.len(), 1);
        assert_eq!(timeline.boots.len(), 2);
        assert!(timeline
            .boots
            .iter()
            .find(|b| b.started_at == first.started_at.clone().unwrap())
            .unwrap()
            .finished_at
            .is_none());
        assert!(super::labels(root.path(), &source)
            .unwrap()
            .unwrap()
            .get("secret-label")
            .is_none());
        record(root.path(), &source, "disconnected", "retry", None).unwrap();
        record(root.path(), &source, "disconnected", "retry", None).unwrap();
        assert_eq!(
            super::timeline(root.path(), &source, 0, None)
                .unwrap()
                .events
                .len(),
            2
        );
    }
}

pub fn metadata(root: &Path, source: &str, key: &str) -> AppResult<Option<String>> {
    let p = log_store::source_dir(root, source)?.join("lifecycle.db");
    if !p.exists() {
        return Ok(None);
    }
    Ok(log_store::reader(&p)?
        .query_row("SELECT value FROM meta WHERE key=?1", [key], |r| r.get(0))
        .optional()?)
}
pub fn set_metadata(root: &Path, source: &str, key: &str, value: &str) -> AppResult<()> {
    writer(root, source)?.execute(
        "INSERT INTO meta VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, value],
    )?;
    Ok(())
}

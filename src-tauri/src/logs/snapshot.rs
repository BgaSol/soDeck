//! Freeze only mutable messages. Completed messages stay in the original archive.
use crate::{
    log_store::{self, QuerySegment},
    model::{AppError, AppResult},
    ssh_transport::Cancellation,
};
use rusqlite::{params_from_iter, types::Value, Connection};
use serde::Serialize;
use std::{path::Path, sync::Arc};
const MAX_FROZEN_BYTES: usize = 128 * 1024 * 1024;
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    pub id: String,
    pub last_seq: i64,
    pub event_cursor: i64,
}
pub struct Snapshot {
    pub info: Info,
    pub segments: Vec<QuerySegment>,
    _lease: std::fs::File,
    _directory: tempfile::TempDir,
}
impl Snapshot {
    pub fn create(
        root: &Path,
        source: &str,
        cache: &Path,
        cancel: &Cancellation,
    ) -> AppResult<Arc<Self>> {
        std::fs::create_dir_all(cache)?;
        cleanup_stale(cache)?;
        let directory = tempfile::Builder::new()
            .prefix("query-")
            .tempdir_in(cache)?;
        let lease = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(directory.path().join(".lease-pending"))?;
        lease.lock()?;
        std::fs::rename(
            directory.path().join(".lease-pending"),
            directory.path().join(".lease"),
        )?;
        let gate = log_store::source_gate(root, source);
        let guard = gate.lock().unwrap();
        cancel.check()?;
        let mut readers = Vec::new();
        let mut high = 0;
        for path in log_store::files(&log_store::source_dir(root, source)?)? {
            let c = log_store::reader(&path)?;
            c.execute_batch("BEGIN")?;
            let last: i64 =
                c.query_row("SELECT coalesce(max(seq),0) FROM records", [], |r| r.get(0))?;
            high = high.max(last);
            readers.push((path, c));
        }
        // Timeline pages are capped; query the actual event boundary directly.
        let event_path = log_store::source_dir(root, source)?.join("lifecycle.db");
        let event_cursor = if event_path.exists() {
            log_store::reader(&event_path)?.query_row(
                "SELECT coalesce(max(id),0) FROM events",
                [],
                |r| r.get(0),
            )?
        } else {
            0
        };
        drop(guard);
        let mut segments = Vec::new();
        let mut budget = 0;
        for (index, (path, c)) in readers.into_iter().enumerate() {
            cancel.check()?;
            let version: i64 = c.query_row("PRAGMA user_version", [], |r| r.get(0))?;
            let frozen = directory.path().join(format!("segment-{index:08}.db"));
            let count: i64 = if version >= 3 {
                c.query_row("SELECT count(*) FROM messages WHERE complete=0", [], |r| {
                    r.get(0)
                })?
            } else {
                1
            };
            if count > 0 {
                let dest = log_store::writer(&frozen)?;
                dest.execute_batch("BEGIN IMMEDIATE")?;
                let condition = if version >= 3 {
                    "WHERE message_seq IN (SELECT first_seq FROM messages WHERE complete=0)"
                } else {
                    ""
                };
                copy(&c, &dest, "records", condition, cancel, &mut budget)?;
                copy(&c, &dest, "chunks", condition, cancel, &mut budget)?;
                if version >= 3 {
                    copy(
                        &c,
                        &dest,
                        "messages",
                        "WHERE complete=0",
                        cancel,
                        &mut budget,
                    )?;
                }
                dest.execute_batch("COMMIT; PRAGMA wal_checkpoint(TRUNCATE)")?;
                if version >= 3 {
                    segments.push(QuerySegment {
                        path,
                        exclude: Some(frozen.clone()),
                    });
                }
                segments.push(QuerySegment {
                    path: frozen,
                    exclude: None,
                });
            } else {
                segments.push(QuerySegment {
                    path,
                    exclude: None,
                });
            }
            c.execute_batch("ROLLBACK")?;
        }
        cancel.check()?;
        Ok(Arc::new(Self {
            info: Info {
                id: uuid::Uuid::new_v4().to_string(),
                last_seq: high,
                event_cursor,
            },
            segments,
            _lease: lease,
            _directory: directory,
        }))
    }
    pub fn query(&self, q: &log_store::Query) -> AppResult<log_store::Page> {
        let mut q = q.clone();
        q.snapshot = Some(
            q.snapshot
                .unwrap_or(self.info.last_seq)
                .min(self.info.last_seq),
        );
        log_store::query_segments(&self.segments, &q, self.info.last_seq)
    }
}
fn copy(
    source: &Connection,
    dest: &Connection,
    table: &str,
    condition: &str,
    cancel: &Cancellation,
    budget: &mut usize,
) -> AppResult<()> {
    let mut statement = source.prepare(&format!("SELECT * FROM {table} {condition}"))?;
    let columns = statement
        .column_names()
        .iter()
        .map(|n| format!("\"{n}\""))
        .collect::<Vec<_>>();
    let sql = format!(
        "INSERT INTO {table}({}) VALUES({})",
        columns.join(","),
        vec!["?"; columns.len()].join(",")
    );
    let mut insert = dest.prepare(&sql)?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        cancel.check()?;
        let values = (0..columns.len())
            .map(|i| row.get::<_, Value>(i))
            .collect::<Result<Vec<_>, _>>()?;
        *budget += values
            .iter()
            .map(|v| match v {
                Value::Blob(v) => v.len(),
                Value::Text(v) => v.len(),
                _ => 8,
            })
            .sum::<usize>();
        if *budget > MAX_FROZEN_BYTES {
            return Err(AppError::new(
                "logs",
                "未完成日志超过 128 MiB，暂时无法创建稳定快照；请等待日志完成后重试",
            ));
        }
        insert.execute(params_from_iter(values))?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frozen_tail_does_not_change_when_old_message_is_extended() {
        let d = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let s = log_store::Source {
            source_id: "a".repeat(64),
            server_id: "s".into(),
            server_name: "s".into(),
            container_name: "c".into(),
            container_id: "b".repeat(64),
            endpoint: "unix:///docker.sock".into(),
            host: "localhost".into(),
            port: 22,
            tty: false,
        };
        let mut store = log_store::Store::open(d.path(), &s, "first", false).unwrap();
        store
            .append(&[(0, b"2026-10-20T00:00:00.000000000Z start".to_vec())])
            .unwrap();
        let snap = Snapshot::create(
            d.path(),
            &s.source_id,
            cache.path(),
            &Cancellation::default(),
        )
        .unwrap();
        store.append(&[(0, b" finished\n".to_vec())]).unwrap();
        let q = log_store::Query {
            limit: 100,
            ..Default::default()
        };
        assert!(!snap.query(&q).unwrap().rows[0].raw.contains("finished"));
        assert!(
            log_store::query(d.path(), &s.source_id, &q).unwrap().rows[0]
                .raw
                .contains("finished")
        );
    }
    #[test]
    fn cache_cleanup_preserves_live_leases_and_removes_abandoned_snapshots() {
        let cache = tempfile::tempdir().unwrap();
        let live = cache.path().join("query-live");
        std::fs::create_dir(&live).unwrap();
        let held = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(live.join(".lease"))
            .unwrap();
        held.lock().unwrap();
        let stale = cache.path().join("query-stale");
        std::fs::create_dir(&stale).unwrap();
        std::fs::write(stale.join(".lease"), b"").unwrap();
        let other = cache.path().join("other");
        std::fs::create_dir(&other).unwrap();
        std::fs::write(other.join(".lease"), b"").unwrap();
        cleanup_stale(cache.path()).unwrap();
        assert!(live.exists());
        assert!(other.exists());
        assert!(!stale.exists());
    }
}

fn cleanup_stale(cache: &Path) -> AppResult<()> {
    let root = cache.canonicalize()?;
    for entry in std::fs::read_dir(&root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir()
            || !entry.file_name().to_string_lossy().starts_with("query-")
        {
            continue;
        }
        let path = entry.path().canonicalize()?;
        if path.parent() != Some(root.as_path()) {
            continue;
        }
        let Ok(lease) = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path.join(".lease"))
        else {
            continue;
        };
        if lease.try_lock().is_ok() {
            // Only orphaned cache directories have an unlocked lease. Never
            // touch a snapshot held by another process or an in-flight export.
            let _ = std::fs::remove_dir_all(&path);
        }
    }
    Ok(())
}

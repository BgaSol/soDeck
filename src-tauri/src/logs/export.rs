use super::*;
use rusqlite::OptionalExtension;
use serde::Deserialize;
use std::io::Write;
#[derive(Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Scope {
    All,
    Selection {
        from: i64,
        to: i64,
        #[serde(default)]
        snapshot_id: Option<String>,
    },
    Query {
        snapshot_id: String,
        query: log_store::Query,
    },
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub records: i64,
    pub bytes: u64,
}
// Reconstruct the exact same UTF-8-safe fragment boundary as log_ingest,
// retaining invalid bytes, ANSI sequences and the original newline.
pub fn original(
    segments: &[log_store::QuerySegment],
    seq: i64,
    cancel: &Cancellation,
) -> AppResult<Vec<u8>> {
    for segment in segments {
        let c = segment.open()?;
        let identity: Option<(Option<i64>, Option<i64>)> = c
            .query_row(
                &format!(
                    "SELECT message_seq,fragment FROM records WHERE seq=?1 {}",
                    segment.clause()
                ),
                [seq],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((message, fragment)) = identity else {
            continue;
        };
        let (Some(message), Some(wanted)) = (message, fragment) else {
            let (raw, complete): (String, bool) = c.query_row(
                "SELECT raw,complete FROM records WHERE seq=?1",
                [seq],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            let mut bytes = raw.into_bytes();
            if complete {
                bytes.push(b'\n');
            }
            return Ok(bytes);
        };
        let mut buffer = Vec::new();
        let mut fragment = 0;
        let mut statement =
            c.prepare("SELECT data FROM chunks WHERE message_seq=?1 ORDER BY part")?;
        for row in statement.query_map([message], |r| r.get::<_, Vec<u8>>(0))? {
            cancel.check()?;
            buffer.extend(row?);
            while buffer.len() >= log_store::FRAGMENT_SIZE {
                let mut take = log_store::FRAGMENT_SIZE;
                if let Err(e) = std::str::from_utf8(&buffer[..take]) {
                    if e.error_len().is_none() {
                        take = e.valid_up_to();
                    }
                }
                let tail = buffer.split_off(take);
                if fragment == wanted {
                    return Ok(buffer);
                }
                buffer = tail;
                fragment += 1;
            }
        }
        if fragment == wanted {
            return Ok(buffer);
        }
        return Err(AppError::new("logs", "日志原文片段不可用"));
    }
    Err(AppError::new("logs", "日志序号不在导出快照中"))
}
pub fn save(
    snapshot: &snapshot::Snapshot,
    q: log_store::Query,
    from: Option<i64>,
    to: Option<i64>,
    path: &std::path::Path,
    cancel: &Cancellation,
    output: &Channel<Progress>,
) -> AppResult<()> {
    let parent = path
        .parent()
        .ok_or_else(|| AppError::new("validation", "无效导出路径"))?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    let mut progress = Progress {
        records: 0,
        bytes: 0,
    };
    if from.is_none()
        && to.is_none()
        && q.keyword.is_empty()
        && q.stream.is_empty()
        && q.from.is_none()
        && q.to.is_none()
    {
        raw_archive(snapshot, &mut temporary, cancel, &mut progress, output)?;
        cancel.check()?;
        temporary.as_file().sync_all()?;
        temporary
            .persist(path)
            .map_err(|e| AppError::new("io", e.error))?;
        return Ok(());
    }
    let mut query = q;
    query.after = Some(from.unwrap_or(1) - 1);
    query.before = None;
    query.snapshot = Some(
        to.unwrap_or(snapshot.info.last_seq)
            .min(snapshot.info.last_seq),
    );
    query.limit = 500;
    loop {
        cancel.check()?;
        let page = snapshot.query(&query)?;
        for row in page.rows {
            cancel.check()?;
            let bytes = original(&snapshot.segments, row.seq, cancel)?;
            temporary.write_all(&bytes)?;
            progress.records += 1;
            progress.bytes += bytes.len() as u64;
        }
        let _ = output.send(Progress {
            records: progress.records,
            bytes: progress.bytes,
        });
        if !page.has_more || page.next_cursor <= query.after.unwrap_or(0) {
            break;
        }
        query.after = Some(page.next_cursor);
    }
    cancel.check()?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(path)
        .map_err(|e| AppError::new("io", e.error))?;
    Ok(())
}

fn raw_archive(
    snapshot: &snapshot::Snapshot,
    destination: &mut impl Write,
    cancel: &Cancellation,
    progress: &mut Progress,
    output: &Channel<Progress>,
) -> AppResult<()> {
    let frozen = snapshot
        .segments
        .iter()
        .filter_map(|s| s.exclude.as_ref())
        .collect::<std::collections::HashSet<_>>();
    for segment in &snapshot.segments {
        if frozen.contains(&segment.path) {
            continue;
        }
        let c = segment.open()?;
        let sql = if segment.exclude.is_some() {
            "SELECT message_seq AS m,part AS p,id,data FROM chunks WHERE message_seq<=?1 AND message_seq NOT IN (SELECT first_seq FROM frozen.messages) UNION ALL SELECT message_seq,part,id,data FROM frozen.chunks ORDER BY m,p,id"
        } else {
            "SELECT message_seq,part,id,data FROM chunks WHERE message_seq IS NULL OR message_seq<=?1 ORDER BY coalesce(message_seq,id),coalesce(part,0),id"
        };
        let mut stmt = c.prepare(sql)?;
        for bytes in stmt.query_map([snapshot.info.last_seq], |r| r.get::<_, Vec<u8>>(3))? {
            cancel.check()?;
            let bytes = bytes?;
            destination.write_all(&bytes)?;
            progress.bytes += bytes.len() as u64;
            if progress.bytes % 1048576 < bytes.len() as u64 {
                let _ = output.send(Progress {
                    records: progress.records,
                    bytes: progress.bytes,
                });
            }
        }
        let count: i64 = c.query_row(
            &format!(
                "SELECT count(*) FROM records WHERE seq<=?1 {}",
                segment.clause()
            ),
            [snapshot.info.last_seq],
            |r| r.get(0),
        )?;
        progress.records += count;
        if segment.exclude.is_some() {
            progress.records +=
                c.query_row::<i64, _, _>("SELECT count(*) FROM frozen.records", [], |r| r.get(0))?;
        }
        let _ = output.send(Progress {
            records: progress.records,
            bytes: progress.bytes,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn source() -> log_store::Source {
        log_store::Source {
            source_id: "a".repeat(64),
            server_id: "s".into(),
            server_name: "s".into(),
            container_name: "c".into(),
            container_id: "b".repeat(64),
            endpoint: "unix:///docker.sock".into(),
            host: "localhost".into(),
            port: 22,
            tty: false,
        }
    }
    #[test]
    fn exports_original_bytes_from_a_frozen_tail_and_keeps_existing_file_on_cancel() {
        let root = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let s = source();
        let mut store = log_store::Store::open(root.path(), &s, "one", false).unwrap();
        let first = b"2026-10-20T00:00:00Z complete\n";
        let tail = b"2026-10-20T00:00:01Z \xff\x1b[32mneedle";
        store
            .append(&[(0, first.to_vec()), (1, tail.to_vec())])
            .unwrap();
        let snapshot = snapshot::Snapshot::create(
            root.path(),
            &s.source_id,
            cache.path(),
            &Cancellation::default(),
        )
        .unwrap();
        store.append(&[(1, b" later\n".to_vec())]).unwrap();
        let dest = root.path().join("out.log");
        let output = Channel::new(|_| Ok(()));
        save(
            &snapshot,
            log_store::Query::default(),
            None,
            None,
            &dest,
            &Cancellation::default(),
            &output,
        )
        .unwrap();
        assert_eq!(
            std::fs::read(&dest).unwrap(),
            [first.as_slice(), tail.as_slice()].concat()
        );
        save(
            &snapshot,
            log_store::Query {
                keyword: "needle".into(),
                ..Default::default()
            },
            None,
            None,
            &dest,
            &Cancellation::default(),
            &output,
        )
        .unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), tail);
        let cancel = Cancellation::default();
        cancel.cancel();
        assert!(save(
            &snapshot,
            log_store::Query::default(),
            None,
            None,
            &dest,
            &cancel,
            &output
        )
        .is_err());
        assert_eq!(std::fs::read(&dest).unwrap(), tail);
    }
}

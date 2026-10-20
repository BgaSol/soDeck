//! Assemble physical Docker lines before admitting them to the canonical archive.
//! Staging is durable and bounded in memory; replayed bytes never enter exports/FTS.
use crate::{
    log_store::{self, Source, FRAGMENT_SIZE},
    model::{AppError, AppResult},
};
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::{
    cell::RefCell,
    collections::VecDeque,
    fs,
    path::{Path, PathBuf},
};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS messages (
 first_seq INTEGER PRIMARY KEY, stream INTEGER NOT NULL, time_ns INTEGER,
 digest BLOB NOT NULL, byte_len INTEGER NOT NULL, complete INTEGER NOT NULL);
CREATE INDEX IF NOT EXISTS message_identity ON messages(time_ns,stream,digest);
CREATE INDEX IF NOT EXISTS message_partial ON messages(time_ns,stream) WHERE complete=0;
CREATE TABLE IF NOT EXISTS staging (stream INTEGER, part INTEGER, data BLOB NOT NULL,
 PRIMARY KEY(stream,part));
CREATE TABLE IF NOT EXISTS incoming (stream INTEGER PRIMARY KEY, message_seq INTEGER,
 segment INTEGER, run_id TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS archive_meta (key TEXT PRIMARY KEY,value TEXT NOT NULL);
PRAGMA user_version=3;";
const LEDGER:&str="PRAGMA temp_store=FILE;
CREATE TEMP TABLE consumed(segment INTEGER,seq INTEGER,stream INTEGER,time_ns INTEGER,digest BLOB,PRIMARY KEY(segment,seq));
CREATE INDEX temp.consumed_identity ON consumed(segment,stream,time_ns,digest,seq);";

fn stream_name(stream: usize) -> &'static str {
    if stream == 0 {
        "stdout"
    } else {
        "stderr"
    }
}

// Legacy archives keep sequence numbers and exact transport bytes. Split their
// chunks into line parts to build the same identity index used by new archives.
pub(crate) fn upgrade(c: &Connection, version: i64) -> AppResult<()> {
    if version == 3 {
        return Ok(());
    }
    c.execute_batch("BEGIN IMMEDIATE")?;
    let result = (|| {
        c.execute_batch(
            "ALTER TABLE records ADD COLUMN message_seq INTEGER;
            ALTER TABLE records ADD COLUMN fragment INTEGER;
            CREATE UNIQUE INDEX record_fragment ON records(message_seq,fragment);
            ALTER TABLE chunks ADD COLUMN message_seq INTEGER;
            ALTER TABLE chunks ADD COLUMN part INTEGER;
            CREATE UNIQUE INDEX chunk_part ON chunks(message_seq,part);",
        )?;
        c.execute_batch(SCHEMA)?;
        let mut groups: [Option<(String, i64, i64)>; 2] = [None, None];
        let mut rows =
            c.prepare("SELECT seq,stream,run_id,continuation FROM records ORDER BY seq")?;
        for row in rows.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, bool>(3)?,
            ))
        })? {
            let (seq, stream, run, continuation) = row?;
            let index = usize::from(stream == "stderr");
            let group = &mut groups[index];
            if !continuation || group.as_ref().is_none_or(|g| g.0 != run) {
                *group = Some((run, seq, 0));
            }
            let (_, first, fragment) = group.as_mut().unwrap();
            c.execute(
                "UPDATE records SET message_seq=?1,fragment=?2 WHERE seq=?3",
                params![*first, *fragment, seq],
            )?;
            *fragment += 1;
        }
        let max: i64 = c.query_row("SELECT coalesce(max(id),0) FROM chunks", [], |r| r.get(0))?;
        let mut migration: [Legacy; 2] = Default::default();
        let mut chunks =
            c.prepare("SELECT run_id,stream,data FROM chunks WHERE id<=?1 ORDER BY id")?;
        for chunk in chunks.query_map([max], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Vec<u8>>(2)?,
            ))
        })? {
            let (run, stream, bytes) = chunk?;
            let index = usize::from(stream == "stderr");
            let state = &mut migration[index];
            if state.run != run {
                state.finish(c, index, false)?;
                state.run = run;
                state.last_seq = 0;
            }
            for data in bytes.split_inclusive(|b| *b == b'\n') {
                if state.seq.is_none() {
                    state.seq=c.query_row("SELECT seq FROM records WHERE run_id=?1 AND stream=?2 AND continuation=0 AND seq>?3 ORDER BY seq LIMIT 1",params![state.run,stream,state.last_seq],|r|r.get(0)).optional()?;
                    if state.seq.is_none() {
                        return Err(AppError::new(
                            "logs",
                            "旧日志的原始数据与索引不一致，未修改存档",
                        ));
                    }
                }
                state.hash.update(data);
                state.len += data.len();
                for part in data.chunks(FRAGMENT_SIZE) {
                    state.bytes.extend_from_slice(part);
                    while state.bytes.len() >= FRAGMENT_SIZE {
                        let rest = state.bytes.split_off(FRAGMENT_SIZE);
                        state.save(c, index)?;
                        state.bytes = rest;
                    }
                }
                if data.last() == Some(&b'\n') {
                    state.finish(c, index, true)?;
                }
            }
        }
        for (index, state) in migration.iter_mut().enumerate() {
            state.finish(c, index, false)?;
        }
        c.execute("DELETE FROM chunks WHERE id<=?1", [max])?;
        Ok::<_, AppError>(())
    })();
    match result {
        Ok(()) => c.execute_batch("COMMIT")?,
        Err(e) => {
            let _ = c.execute_batch("ROLLBACK");
            return Err(e);
        }
    }
    Ok(())
}

#[derive(Default)]
struct Legacy {
    run: String,
    seq: Option<i64>,
    last_seq: i64,
    bytes: Vec<u8>,
    part: i64,
    len: usize,
    hash: Sha256,
}
impl Legacy {
    fn save(&mut self, c: &Connection, stream: usize) -> AppResult<()> {
        c.execute("INSERT INTO chunks(run_id,stream,data,received_at,message_seq,part) VALUES(?1,?2,?3,?4,?5,?6)",params![self.run,stream_name(stream),self.bytes,log_store::now(),self.seq,self.part])?;
        self.part += 1;
        self.bytes.clear();
        Ok(())
    }
    fn finish(&mut self, c: &Connection, stream: usize, complete: bool) -> AppResult<()> {
        if let Some(seq) = self.seq {
            if !self.bytes.is_empty() {
                self.save(c, stream)?;
            }
            c.execute("INSERT INTO messages(first_seq,stream,time_ns,digest,byte_len,complete) SELECT ?1,?2,time_ns,?3,?4,?5 FROM records WHERE seq=?1",params![seq,stream as i64,self.hash.clone().finalize().as_slice(),self.len as i64,complete])?;
            self.last_seq = seq;
            self.seq = None;
            self.part = 0;
            self.len = 0;
            self.hash = Sha256::new();
        }
        Ok(())
    }
}

pub fn history_complete(root: &Path, id: &str) -> AppResult<bool> {
    for path in log_store::files(&log_store::source_dir(root, id)?)? {
        let c = log_store::reader(&path)?;
        let version: i64 = c.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version >= 3
            && c.query_row(
                "SELECT EXISTS(SELECT 1 FROM archive_meta WHERE key='history_complete')",
                [],
                |r| r.get::<_, bool>(0),
            )?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Caller serializes this with collector registration. Offline readers must
/// also see tails received immediately before an application crash.
pub fn recover_archive(root: &Path, source: &Source) -> AppResult<()> {
    let paths = log_store::files(&log_store::source_dir(root, &source.source_id)?)?;
    let Some(path) = paths.last() else {
        return Ok(());
    };
    let c = log_store::reader(path)?;
    let version: i64 = c.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version == 3
        && c.query_row("SELECT EXISTS(SELECT 1 FROM incoming)", [], |r| {
            r.get::<_, bool>(0)
        })?
    {
        drop(c);
        let mut store = Store::open(root, source, &uuid::Uuid::new_v4().to_string(), false)?;
        store.finish("恢复中断前已收到的日志")?;
    }
    Ok(())
}

#[derive(Default)]
struct Pending {
    bytes: Vec<u8>,
    part: i64,
    len: usize,
    published: Option<(usize, i64)>,
    published_len: usize,
}
#[derive(Clone, Copy)]
struct Candidate {
    segment: usize,
    seq: i64,
    len: usize,
}
struct PriorSegment {
    path: PathBuf,
    bounds: Option<(i64, i64)>,
}
fn prior_segments(dir: &Path) -> AppResult<Vec<PriorSegment>> {
    log_store::files(dir)?.into_iter().map(|path| {
        let c=log_store::reader(&path)?;
        let (lo,hi):(Option<i64>,Option<i64>)=c.query_row("SELECT (SELECT time_ns FROM messages WHERE time_ns IS NOT NULL ORDER BY time_ns LIMIT 1),(SELECT time_ns FROM messages WHERE time_ns IS NOT NULL ORDER BY time_ns DESC LIMIT 1)",[],|r|Ok((r.get(0)?,r.get(1)?)))?;
        Ok(PriorSegment {path,bounds:lo.zip(hi)})
    }).collect()
}
pub struct Store {
    gate: std::sync::Arc<std::sync::Mutex<()>>,
    pub changed_from: Option<i64>,
    dir: PathBuf,
    connection: Connection,
    segment: usize,
    prior_seq: i64,
    next_seq: i64,
    pending: [Pending; 2],
    run_id: String,
    pub since: Option<String>,
    prior_segments: Vec<PriorSegment>,
    prior_cache: RefCell<VecDeque<(usize, Connection)>>,
    pub(crate) segment_size: u64,
}
impl Store {
    pub fn open(root: &Path, source: &Source, run_id: &str, history: bool) -> AppResult<Self> {
        let gate = log_store::source_gate(root, &source.source_id);
        let _guard = gate.lock().unwrap();
        let dir = log_store::source_dir(root, &source.source_id)?;
        fs::create_dir_all(&dir)?;
        let metadata = dir.join("source.json");
        if !metadata.exists() {
            use std::io::Write;
            let mut file = tempfile::NamedTempFile::new_in(&dir)?;
            file.write_all(&serde_json::to_vec(source).map_err(|e| AppError::new("logs", e))?)?;
            file.as_file().sync_all()?;
            file.persist_noclobber(&metadata)
                .map_err(|e| AppError::new("logs", e))?;
        }
        let paths = log_store::files(&dir)?;
        for path in &paths {
            log_store::writer(path)?;
        }
        let prior = log_store::stats(root, &source.source_id)?;
        let segment = paths.len().max(1);
        let connection = log_store::writer(&dir.join(format!("segment-{segment:08}.db")))?;
        connection.execute_batch(LEDGER)?;
        let segments = prior_segments(&dir)?;
        let mut store = Self {
            gate: gate.clone(),
            changed_from: None,
            dir,
            connection,
            segment,
            prior_seq: prior.last_seq,
            next_seq: prior.last_seq + 1,
            prior_segments: segments,
            prior_cache: Default::default(),
            pending: Default::default(),
            run_id: run_id.into(),
            since: None,
            segment_size: 256 * 1024 * 1024,
        };
        // A crash can leave a received tail in staging. Recover it before taking
        // the next run's snapshot; a published tail is updated in place.
        for stream in 0..2 {
            let incoming: Option<(Option<i64>, Option<i64>)> = store
                .connection
                .query_row(
                    "SELECT message_seq,segment FROM incoming WHERE stream=?1",
                    [stream as i64],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            if let Some((seq, segment)) = incoming {
                store.pending[stream].len = store.connection.query_row::<i64, _, _>(
                    "SELECT coalesce(sum(length(data)),0) FROM staging WHERE stream=?1",
                    [stream as i64],
                    |r| r.get(0),
                )? as usize;
                store.pending[stream].published = segment.map(|n| n as usize).zip(seq);
                store.transaction(|s| s.finalize(stream, false))?;
            }
        }
        let prior = log_store::stats(root, &source.source_id)?;
        store.prior_seq = prior.last_seq;
        store.prior_segments = prior_segments(&store.dir)?;
        store.next_seq = prior.last_seq + 1;
        store.since = if history {
            None
        } else {
            prior.last_timestamp.as_deref().and_then(log_store::since)
        };
        store.connection.execute("DELETE FROM consumed", [])?;
        store.register_run()?;
        Ok(store)
    }
    fn register_run(&self) -> AppResult<()> {
        self.connection.execute(
            "INSERT OR IGNORE INTO runs(id,started_at,since,note) VALUES(?1,?2,?3,'自动采集')",
            params![self.run_id, log_store::now(), self.since],
        )?;
        Ok(())
    }
    fn transaction(&mut self, f: impl FnOnce(&mut Self) -> AppResult<()>) -> AppResult<()> {
        self.connection.execute_batch("BEGIN IMMEDIATE")?;
        match f(self) {
            Ok(()) => self.connection.execute_batch("COMMIT")?,
            Err(e) => {
                let _ = self.connection.execute_batch("ROLLBACK");
                return Err(e);
            }
        }
        Ok(())
    }
    fn stage(&self, stream: usize) -> AppResult<()> {
        let p = &self.pending[stream];
        if p.bytes.is_empty() {
            return Ok(());
        }
        self.connection.execute("INSERT INTO staging(stream,part,data) VALUES(?1,?2,?3) ON CONFLICT(stream,part) DO UPDATE SET data=excluded.data",params![stream as i64,p.part,p.bytes])?;
        let (segment, seq) = p
            .published
            .map_or((None, None), |(a, b)| (Some(a as i64), Some(b)));
        self.connection.execute("INSERT INTO incoming(stream,message_seq,segment,run_id) VALUES(?1,?2,?3,?4) ON CONFLICT(stream) DO UPDATE SET message_seq=excluded.message_seq,segment=excluded.segment",params![stream as i64,seq,segment,self.run_id])?;
        Ok(())
    }
    pub fn append(&mut self, chunks: &[(usize, Vec<u8>)]) -> AppResult<i64> {
        let gate = self.gate.clone();
        let _guard = gate.lock().unwrap();
        self.changed_from = None;
        self.transaction(|s| {
            for (stream, bytes) in chunks {
                let mut rest = bytes.as_slice();
                while !rest.is_empty() {
                    let p = &mut s.pending[*stream];
                    let take = rest
                        .iter()
                        .position(|b| *b == b'\n')
                        .map_or(rest.len(), |n| n + 1)
                        .min(FRAGMENT_SIZE - p.bytes.len());
                    p.bytes.extend_from_slice(&rest[..take]);
                    p.len += take;
                    rest = &rest[take..];
                    let newline = p.bytes.last() == Some(&b'\n');
                    if newline || p.bytes.len() == FRAGMENT_SIZE {
                        s.stage(*stream)?;
                        s.pending[*stream].bytes.clear();
                        s.pending[*stream].part += 1;
                        if newline {
                            s.finalize(*stream, true)?;
                        }
                    }
                }
            }
            for stream in 0..2 {
                if s.pending[stream].len > 0 {
                    s.stage(stream)?;
                    // Geometric checkpoints keep huge unterminated lines
                    // visible without rebuilding their index on every packet.
                    if (s.pending[stream].len <= FRAGMENT_SIZE
                        || s.pending[stream].published_len == 0
                        || s.pending[stream].len
                            >= s.pending[stream].published_len.saturating_mul(2))
                        && s.pending[stream].len != s.pending[stream].published_len
                        && (s.pending[stream].published.is_some()
                            || !s.has_prefix_candidate(stream)?)
                    {
                        s.publish(stream, false, None)?;
                    }
                }
            }
            Ok(())
        })?;
        if self.pending.iter().all(|p| p.len == 0) {
            let size: i64 = self.connection.query_row(
                "SELECT page_count*page_size FROM pragma_page_count(),pragma_page_size()",
                [],
                |r| r.get(0),
            )?;
            if size as u64 >= self.segment_size {
                self.rotate()?;
            }
        }
        Ok(self.next_seq - 1)
    }
    fn timestamp(&self, stream: usize) -> AppResult<Option<String>> {
        let bytes: Vec<u8> = self.connection.query_row(
            "SELECT data FROM staging WHERE stream=?1 AND part=0",
            [stream as i64],
            |r| r.get(0),
        )?;
        Ok(bytes
            .split(|b| *b == b' ')
            .next()
            .and_then(|b| std::str::from_utf8(b).ok())
            .filter(|t| log_store::time_ns(t).is_some())
            .map(str::to_owned))
    }
    fn used(&self, candidate: Candidate) -> AppResult<bool> {
        Ok(self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM consumed WHERE segment=?1 AND seq=?2)",
            params![candidate.segment as i64, candidate.seq],
            |r| r.get(0),
        )?)
    }
    fn consume(&self, candidate: Candidate, stream: usize, digest: Option<&[u8]>) -> AppResult<()> {
        self.connection.execute(
            "INSERT OR IGNORE INTO consumed VALUES(?1,?2,?3,?4,?5)",
            params![
                candidate.segment as i64,
                candidate.seq,
                stream as i64,
                self.timestamp(stream)?
                    .as_deref()
                    .and_then(log_store::time_ns),
                digest
            ],
        )?;
        Ok(())
    }
    fn matches(
        &self,
        stream: usize,
        c: &Connection,
        candidate: Candidate,
        len: usize,
    ) -> AppResult<bool> {
        let mut offset = 0;
        let mut st = self
            .connection
            .prepare("SELECT part,data FROM staging WHERE stream=?1 ORDER BY part")?;
        for row in st.query_map([stream as i64], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?))
        })? {
            let (part, bytes) = row?;
            let old: Option<Vec<u8>> = c
                .query_row(
                    "SELECT data FROM chunks WHERE message_seq=?1 AND part=?2",
                    params![candidate.seq, part],
                    |r| r.get(0),
                )
                .optional()?;
            let count = bytes.len().min(len - offset);
            if old
                .as_ref()
                .is_none_or(|old| old.len() < count || old[..count] != bytes[..count])
            {
                return Ok(false);
            }
            offset += count;
            if offset == len {
                return Ok(true);
            }
        }
        Ok(offset == len)
    }
    fn find(
        &self,
        stream: usize,
        digest: Option<&[u8]>,
        partial: bool,
    ) -> AppResult<Option<Candidate>> {
        if self.prior_seq == 0 {
            return Ok(None);
        }
        let ns = self
            .timestamp(stream)?
            .as_deref()
            .and_then(log_store::time_ns);
        let Some(ns) = ns else { return Ok(None) };
        let mut cache = self.prior_cache.borrow_mut();
        for (index, segment) in self.prior_segments.iter().enumerate() {
            if segment.bounds.is_none_or(|(lo, hi)| ns < lo || ns > hi) {
                continue;
            }
            let position = if let Some(position) = cache.iter().position(|(n, _)| *n == index) {
                position
            } else {
                if cache.len() == 8 {
                    cache.pop_front();
                }
                let c = log_store::reader(&segment.path)?;
                c.execute_batch("PRAGMA cache_size=-512")?;
                cache.push_back((index, c));
                cache.len() - 1
            };
            let c = &cache[position].1;
            let last: i64 = if digest.is_some() {
                self.connection.query_row("SELECT coalesce(max(seq),0) FROM consumed WHERE segment=?1 AND stream=?2 AND time_ns=?3 AND digest=?4",params![(index+1) as i64,stream as i64,ns,digest],|r|r.get(0))?
            } else {
                0
            };
            let sql=format!("SELECT first_seq,byte_len FROM messages WHERE time_ns=?1 AND stream=?2 AND first_seq<=?3 AND first_seq>?6 AND {} AND {} ORDER BY first_seq",
                if digest.is_some() {"digest=?4"}else{"?4 IS NULL"},
                if partial {"complete=0 AND ?5=1"}else{"?5=0"});
            let mut st = c.prepare_cached(&sql)?;
            for row in st.query_map(
                params![ns, stream as i64, self.prior_seq, digest, partial, last],
                |r| {
                    Ok(Candidate {
                        segment: index + 1,
                        seq: r.get(0)?,
                        len: r.get::<_, i64>(1)? as usize,
                    })
                },
            )? {
                let candidate = row?;
                if self.used(candidate)? {
                    continue;
                }
                let incoming = self.pending[stream].len;
                if digest.is_some() && candidate.len != incoming {
                    continue;
                }
                if self.matches(stream, c, candidate, candidate.len.min(incoming))? {
                    return Ok(Some(candidate));
                }
            }
        }
        Ok(None)
    }
    fn incomplete_replay_header(&self, stream: usize) -> AppResult<bool> {
        if self.prior_seq == 0 {
            return Ok(false);
        }
        let first: Vec<u8> = self.connection.query_row(
            "SELECT data FROM staging WHERE stream=?1 AND part=0",
            [stream as i64],
            |r| r.get(0),
        )?;
        Ok(first.len() < 64
            && first.first().is_some_and(u8::is_ascii_digit)
            && !first.contains(&b' '))
    }
    fn has_prefix_candidate(&self, stream: usize) -> AppResult<bool> {
        // Don't publish a replay merely because a packet split its timestamp.
        let first: Vec<u8> = self.connection.query_row(
            "SELECT data FROM staging WHERE stream=?1 AND part=0",
            [stream as i64],
            |r| r.get(0),
        )?;
        if self.prior_seq > 0
            && first.len() < 64
            && first.first().is_some_and(u8::is_ascii_digit)
            && !first.contains(&b' ')
        {
            return Ok(true);
        }
        Ok(self.find(stream, None, false)?.is_some())
    }
    fn digest(&self, stream: usize) -> AppResult<Vec<u8>> {
        let mut hash = Sha256::new();
        let mut st = self
            .connection
            .prepare("SELECT data FROM staging WHERE stream=?1 ORDER BY part")?;
        for bytes in st.query_map([stream as i64], |r| r.get::<_, Vec<u8>>(0))? {
            hash.update(bytes?);
        }
        Ok(hash.finalize().to_vec())
    }
    fn finalize(&mut self, stream: usize, complete: bool) -> AppResult<()> {
        if self.pending[stream].len == 0 {
            return Ok(());
        }
        if self.pending[stream].published.is_some() {
            self.publish(stream, complete, None)?;
        } else {
            let digest = self.digest(stream)?;
            if let Some(candidate) = self.find(stream, Some(&digest), false)? {
                self.consume(candidate, stream, Some(&digest))?;
            } else if let Some(candidate) = self.find(stream, None, true)? {
                if self.pending[stream].len >= candidate.len {
                    self.publish(stream, complete, Some(candidate))?;
                }
                self.consume(candidate, stream, None)?;
            } else if !complete && self.incomplete_replay_header(stream)? {
                // A reconnect cancelled inside the timestamp has not established
                // a new message identity. Do not publish the replay header as a
                // new application record; the next --since overlap replays it.
            } else if !complete && self.find(stream, None, false)?.is_some() {
                // The connection stopped during an already archived complete
                // line: the received prefix adds no new bytes.
            } else {
                self.publish(stream, complete, None)?;
            }
        }
        self.connection
            .execute("DELETE FROM staging WHERE stream=?1", [stream as i64])?;
        self.connection
            .execute("DELETE FROM incoming WHERE stream=?1", [stream as i64])?;
        self.pending[stream] = Pending::default();
        Ok(())
    }
    fn publish(
        &mut self,
        stream: usize,
        complete: bool,
        replace: Option<Candidate>,
    ) -> AppResult<()> {
        let target = replace
            .map(|c| (c.segment, c.seq))
            .or(self.pending[stream].published);
        let (segment, first) = target.unwrap_or_else(|| {
            let seq = self.next_seq;
            self.next_seq += 1;
            (self.segment, seq)
        });
        // Extending an older partial record is committed first. If interrupted
        // before staging is cleared, recovery compares it with the new bytes.
        let old = if segment != self.segment {
            Some(log_store::writer(
                &self.dir.join(format!("segment-{segment:08}.db")),
            )?)
        } else {
            None
        };
        let destination = old.as_ref().unwrap_or(&self.connection);
        if old.is_some() {
            destination.execute_batch("BEGIN IMMEDIATE")?;
        }
        let mut next_seq = self.next_seq;
        let result = (|| {
            destination.execute("INSERT OR IGNORE INTO runs(id,started_at,since,note) VALUES(?1,?2,?3,'续写未完成日志')",params![self.run_id,log_store::now(),self.since])?;
            let timestamp = self.timestamp(stream)?;
            let ns = timestamp.as_deref().and_then(log_store::time_ns);
            let mut parts = self
                .connection
                .prepare("SELECT part,data FROM staging WHERE stream=?1 ORDER BY part")?;
            let mut buffer = Vec::new();
            let mut fragment = 0;
            let mut prefix = String::new();
            let mut detected_level = String::new();
            for row in parts.query_map([stream as i64], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?))
            })? {
                let (part, bytes) = row?;
                destination.execute("INSERT INTO chunks(run_id,stream,data,received_at,message_seq,part) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(message_seq,part) DO UPDATE SET data=excluded.data",params![self.run_id,stream_name(stream),bytes,log_store::now(),first,part])?;
                buffer.extend_from_slice(&bytes);
                while buffer.len() >= FRAGMENT_SIZE {
                    let mut take = FRAGMENT_SIZE;
                    if let Err(e) = std::str::from_utf8(&buffer[..take]) {
                        if e.error_len().is_none() {
                            take = e.valid_up_to();
                        }
                    }
                    let tail = buffer.split_off(take);
                    let newline = complete && buffer.last() == Some(&b'\n');
                    write_record(
                        destination,
                        &mut next_seq,
                        first,
                        fragment,
                        stream,
                        &buffer,
                        newline,
                        timestamp.as_deref(),
                        &mut detected_level,
                        &mut prefix,
                        &self.run_id,
                    )?;
                    fragment += 1;
                    buffer = tail;
                }
            }
            if !buffer.is_empty() {
                write_record(
                    destination,
                    &mut next_seq,
                    first,
                    fragment,
                    stream,
                    &buffer,
                    complete,
                    timestamp.as_deref(),
                    &mut detected_level,
                    &mut prefix,
                    &self.run_id,
                )?;
            }
            destination.execute("INSERT INTO messages(first_seq,stream,time_ns,digest,byte_len,complete) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(first_seq) DO UPDATE SET time_ns=excluded.time_ns,digest=excluded.digest,byte_len=excluded.byte_len,complete=excluded.complete",params![first,stream as i64,ns,self.digest(stream)?,self.pending[stream].len as i64,complete])?;
            Ok::<_, AppError>(())
        })();
        if old.is_some() {
            match &result {
                Ok(()) => destination.execute_batch("COMMIT")?,
                Err(_) => {
                    let _ = destination.execute_batch("ROLLBACK");
                }
            }
        }
        result?;
        self.changed_from = Some(self.changed_from.map_or(first, |n| n.min(first)));
        self.next_seq = next_seq;
        self.pending[stream].published = Some((segment, first));
        self.pending[stream].published_len = self.pending[stream].len;
        self.connection.execute(
            "UPDATE incoming SET segment=?1,message_seq=?2 WHERE stream=?3",
            params![segment as i64, first, stream as i64],
        )?;
        Ok(())
    }
    fn rotate(&mut self) -> AppResult<()> {
        self.connection.execute(
            "UPDATE runs SET ended_at=?1 WHERE id=?2",
            params![log_store::now(), self.run_id],
        )?;
        self.connection
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
        // Move the disk-backed occurrence ledger with the writer.
        self.connection.execute_batch("DROP TABLE IF EXISTS saved_consumed; CREATE TABLE saved_consumed AS SELECT * FROM consumed;")?;
        let old_path = self.dir.join(format!("segment-{:08}.db", self.segment));
        self.segment += 1;
        self.connection =
            log_store::writer(&self.dir.join(format!("segment-{:08}.db", self.segment)))?;
        self.connection.execute(
            "ATTACH DATABASE ?1 AS prior",
            [old_path.to_string_lossy().as_ref()],
        )?;
        self.connection.execute_batch(LEDGER)?;
        self.connection.execute_batch("INSERT INTO consumed SELECT * FROM prior.saved_consumed; DROP TABLE prior.saved_consumed; DETACH DATABASE prior;")?;
        self.register_run()
    }
    pub fn mark_history_complete(&self) -> AppResult<()> {
        self.connection.execute(
            "INSERT OR REPLACE INTO archive_meta VALUES('history_complete','1')",
            [],
        )?;
        Ok(())
    }
    pub fn finish(&mut self, note: &str) -> AppResult<()> {
        let gate = self.gate.clone();
        let _guard = gate.lock().unwrap();
        self.changed_from = None;
        self.transaction(|s| {
            for stream in 0..2 {
                s.stage(stream)?;
                s.finalize(stream, false)?;
            }
            Ok(())
        })?;
        self.connection.execute(
            "UPDATE runs SET ended_at=?1,note=?2 WHERE id=?3",
            params![log_store::now(), note, self.run_id],
        )?;
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
fn write_record(
    c: &Connection,
    next: &mut i64,
    first: i64,
    fragment: i64,
    stream: usize,
    bytes: &[u8],
    complete: bool,
    timestamp: Option<&str>,
    detected_level: &mut String,
    prefix: &mut String,
    run: &str,
) -> AppResult<()> {
    let seq: Option<i64> = c
        .query_row(
            "SELECT seq FROM records WHERE message_seq=?1 AND fragment=?2",
            params![first, fragment],
            |r| r.get(0),
        )
        .optional()?;
    let seq = seq.unwrap_or_else(|| {
        if fragment == 0 {
            first
        } else {
            let seq = *next;
            *next += 1;
            seq
        }
    });
    let raw = String::from_utf8_lossy(bytes.strip_suffix(b"\n").unwrap_or(bytes)).into_owned();
    let text = log_store::plain_text(&raw);
    if fragment == 0 {
        *detected_level = log_store::level(&text).into();
    }
    c.prepare_cached("INSERT INTO records(seq,stream,raw,text,folded,timestamp,time_ns,level,complete,continuation,replayed,run_id,received_at,prefix,message_seq,fragment) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,0,?11,?12,?13,?14,?15) ON CONFLICT(seq) DO UPDATE SET raw=excluded.raw,text=excluded.text,folded=excluded.folded,timestamp=excluded.timestamp,time_ns=excluded.time_ns,level=excluded.level,complete=excluded.complete,prefix=excluded.prefix")?.execute(params![seq,stream_name(stream),raw,text,format!("{prefix}{text}").to_lowercase(),timestamp,timestamp.and_then(log_store::time_ns),detected_level.as_str(),complete,fragment>0,run,log_store::now(),prefix.as_str(),first,fragment])?;
    *prefix = text
        .chars()
        .rev()
        .take(4096)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    Ok(())
}

use super::{model::*, projection::Projection, store};
use crate::model::{AppError, AppResult};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering},
        mpsc, Arc, Condvar, Mutex,
    },
    time::{Duration, Instant},
};

const SESSION_QUEUE: usize = 4 * 1024 * 1024;
const GLOBAL_QUEUE: usize = 64 * 1024 * 1024;
#[derive(Default)]
pub struct HistoryState {
    pub root: Option<PathBuf>,
    pub startup_error: Option<String>,
    pub queries: Mutex<HashMap<String, Arc<AtomicBool>>>,
    runtimes: Mutex<HashMap<String, Arc<Runtime>>>,
    queued: Arc<AtomicUsize>,
}
struct Runtime {
    session: Mutex<Session>,
    bytes: AtomicUsize,
    received: AtomicI64,
    lost: AtomicUsize,
    writer_done: AtomicBool,
    done: (Mutex<bool>, Condvar),
    reason: Mutex<String>,
}
struct Queued {
    events: Vec<Event>,
    bytes: usize,
    runtime: Arc<Runtime>,
    global: Arc<AtomicUsize>,
}
impl Drop for Queued {
    fn drop(&mut self) {
        self.runtime.bytes.fetch_sub(self.bytes, Ordering::Relaxed);
        self.global.fetch_sub(self.bytes, Ordering::Relaxed);
    }
}
impl HistoryState {
    pub fn open(root: PathBuf) -> Self {
        let error = store::init(&root)
            .and_then(|_| store::recover(&root))
            .err()
            .map(|e| e.message);
        let state = Self {
            root: Some(root.clone()),
            startup_error: error,
            ..Default::default()
        };
        if state.startup_error.is_none() {
            // Recovery is bounded and runs independently from startup/terminal IO.
            std::thread::spawn(move || {
                if let Ok(sessions) = store::sessions(&root, &Query::default()) {
                    for session in sessions
                        .into_iter()
                        .filter(|s| s.saved_seq != s.indexed_seq && s.state != "recording")
                    {
                        if let Err(e) = index_session(&root, &session.id, None) {
                            store::error(&root, &session.id, &e.message);
                        }
                    }
                }
            });
        }
        state
    }
    pub fn start(&self, id: &str, workspace: &str, source: Source) -> Recorder {
        let session = Session {
            id: id.into(),
            workspace_id: workspace.into(),
            source,
            started_at: now(),
            ended_at: None,
            state: "recording".into(),
            saved_seq: "0".into(),
            indexed_seq: "0".into(),
            last_record: "0".into(),
            bytes: 0,
            error: self.startup_error.clone(),
        };
        let runtime = Arc::new(Runtime {
            session: Mutex::new(session),
            bytes: AtomicUsize::new(0),
            received: AtomicI64::new(0),
            lost: AtomicUsize::new(0),
            writer_done: AtomicBool::new(false),
            done: (Mutex::new(false), Condvar::new()),
            reason: Mutex::new("连接结束".into()),
        });
        self.runtimes
            .lock()
            .unwrap()
            .retain(|_, r| !r.writer_done.load(Ordering::Relaxed));
        self.runtimes
            .lock()
            .unwrap()
            .insert(id.into(), runtime.clone());
        let (sender, receiver) = mpsc::sync_channel(512);
        let root = self.root.clone();
        let shared = runtime.clone();
        std::thread::spawn(move || {
            let result = if let Some(root) = root {
                write_session(&root, &shared, receiver)
            } else {
                Err(AppError::new("history", "历史服务未初始化"))
            };
            if let Err(error) = result {
                shared.session.lock().unwrap().error =
                    Some(format!("终端记录未完整保存：{}", error.message));
            }
            shared.writer_done.store(true, Ordering::Release);
            *shared.done.0.lock().unwrap() = true;
            shared.done.1.notify_all();
        });
        Recorder {
            sender: Some(sender),
            runtime,
            global: self.queued.clone(),
            started: Instant::now(),
            seq: 0,
            dirty: false,
        }
    }
    pub fn status(&self, id: &str) -> AppResult<Session> {
        let live = self.runtimes.lock().unwrap().get(id).cloned();
        let mut session = if let Some(root) = &self.root {
            store::get_session(root, id).ok()
        } else {
            None
        };
        if let Some(live) = live {
            let memory = live.session.lock().unwrap();
            if let Some(session) = session.as_mut() {
                if memory.error.is_some() {
                    session.error = memory.error.clone();
                }
            } else {
                session = Some(memory.clone());
            }
        }
        session.ok_or_else(|| AppError::new("history", "没有此终端的历史记录"))
    }
    pub fn shutdown(&self) -> AppResult<()> {
        let runtimes = self
            .runtimes
            .lock()
            .unwrap()
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let deadline = Instant::now() + Duration::from_secs(30);
        for runtime in runtimes {
            let (done, _) = runtime
                .done
                .1
                .wait_timeout_while(
                    runtime.done.0.lock().unwrap(),
                    deadline.saturating_duration_since(Instant::now()),
                    |done| !*done,
                )
                .unwrap();
            if !*done {
                return Err(AppError::new("history", "终端历史仍在写入，请稍后退出"));
            }
        }
        Ok(())
    }
    pub fn cancel_window(&self, window: &str) {
        let prefix = format!("{window}:");
        self.queries.lock().unwrap().retain(|key, cancel| {
            if key.starts_with(&prefix) {
                cancel.store(true, Ordering::Relaxed);
                false
            } else {
                true
            }
        });
    }
}
pub struct Recorder {
    sender: Option<mpsc::SyncSender<Queued>>,
    runtime: Arc<Runtime>,
    global: Arc<AtomicUsize>,
    started: Instant,
    seq: i64,
    dirty: bool,
}
impl Recorder {
    pub fn push(&mut self, kind: EventKind) {
        self.seq += 1;
        self.runtime.received.store(self.seq, Ordering::Relaxed);
        self.dirty |= matches!(kind, EventKind::Output { .. });
        let lost = self.runtime.lost.swap(0, Ordering::Relaxed);
        let mut events = Vec::new();
        if lost > 0 {
            events.push(Event {
                seq: self.seq,
                time: now(),
                elapsed: self.started.elapsed().as_micros() as i64,
                kind: EventKind::Gap {
                    reason: format!("记录队列未能保存 {lost} 个事件"),
                },
            });
            self.seq += 1;
            self.runtime.received.store(self.seq, Ordering::Relaxed);
        }
        events.push(Event {
            seq: self.seq,
            time: now(),
            elapsed: self.started.elapsed().as_micros() as i64,
            kind,
        });
        let bytes = events.iter().map(Event::size).sum::<usize>();
        let local = self.runtime.bytes.fetch_add(bytes, Ordering::Relaxed);
        let global = self.global.fetch_add(bytes, Ordering::Relaxed);
        let queued = Queued {
            events,
            bytes,
            runtime: self.runtime.clone(),
            global: self.global.clone(),
        };
        let sent = local + bytes <= SESSION_QUEUE
            && global + bytes <= GLOBAL_QUEUE
            && self
                .sender
                .as_ref()
                .is_some_and(|s| s.try_send(queued).is_ok());
        if !sent {
            self.runtime.lost.fetch_add(lost + 1, Ordering::Relaxed);
            self.runtime.session.lock().unwrap().error =
                Some("历史记录未完整保存：写入队列已满或写入失败；终端继续工作".into());
        }
    }
    pub fn checkpoint(&mut self) {
        if self.dirty {
            self.dirty = false;
            self.push(EventKind::Checkpoint);
        }
    }
    pub fn finish(&mut self, reason: &str) {
        *self.runtime.reason.lock().unwrap() = reason.into();
        self.sender.take();
    }
}
fn write_session(
    root: &Path,
    runtime: &Arc<Runtime>,
    receiver: mpsc::Receiver<Queued>,
) -> AppResult<()> {
    let session = runtime.session.lock().unwrap().clone();
    // Always drain on error so accounting and terminal producers remain bounded.
    let result: AppResult<()> = (|| {
        store::register(root, &session)?;
        let mut writer = store::ShardWriter::new(root, &session.id, "raw")?;
        let index_root = root.to_owned();
        let index_id = session.id.clone();
        let index_runtime = runtime.clone();
        std::thread::spawn(move || {
            if let Err(e) = index_session(&index_root, &index_id, Some(&index_runtime)) {
                store::error(
                    &index_root,
                    &index_id,
                    &format!("历史索引未完成：{}", e.message),
                );
            }
        });
        loop {
            let first = match receiver.recv() {
                Ok(events) => events,
                Err(_) => break,
            };
            let mut size = first.bytes;
            let mut queued = vec![first];
            let deadline = Instant::now() + Duration::from_millis(50);
            while size < 256 * 1024 && Instant::now() < deadline {
                match receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                    Ok(next) => {
                        size += next.bytes;
                        queued.push(next);
                    }
                    Err(_) => break,
                }
            }
            let batch = queued
                .iter_mut()
                .flat_map(|q| std::mem::take(&mut q.events))
                .collect::<Vec<_>>();
            writer.append_events(&batch)?;
        }
        let mut seq = runtime.received.load(Ordering::Relaxed) + 1;
        let mut last = Vec::new();
        let lost = runtime.lost.load(Ordering::Relaxed);
        if lost > 0 {
            last.push(Event {
                seq,
                time: now(),
                elapsed: 0,
                kind: EventKind::Gap {
                    reason: format!("结束前有 {lost} 个事件未能保存"),
                },
            });
            seq += 1;
        }
        last.push(Event {
            seq,
            time: now(),
            elapsed: 0,
            kind: EventKind::End {
                reason: runtime.reason.lock().unwrap().clone(),
            },
        });
        writer.append_events(&last)?;
        Ok(())
    })();
    if let Err(e) = &result {
        store::error(root, &session.id, &e.message);
    }
    // Dropping the receiver stops new submissions immediately. Account only queued messages.
    drop(receiver);
    result
}
fn index_session(root: &Path, id: &str, runtime: Option<&Arc<Runtime>>) -> AppResult<()> {
    let mut projector = Projection::new(id);
    let session = store::get_session(root, id)?;
    let indexed = session.indexed_seq.parse::<i64>().unwrap_or(0);
    let mut writer = store::ShardWriter::new(root, id, "view")?;
    let mut cursor = 0;
    loop {
        let events = store::read_events(root, id, cursor, 64)?;
        if events.is_empty() {
            if runtime.is_none_or(|r| r.writer_done.load(Ordering::Acquire)) {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
            continue;
        }
        let mut rows = Vec::new();
        let mut bytes = 0;
        for event in events {
            cursor = event.seq;
            let next = projector.apply(&event);
            if cursor > indexed {
                bytes += next
                    .iter()
                    .map(|r| r.text.len() + r.ansi.len() + 256)
                    .sum::<usize>();
                rows.extend(next);
                if bytes >= 4 * 1024 * 1024 {
                    writer.append_rows(&rows, cursor, projector.last_id())?;
                    rows.clear();
                    bytes = 0;
                }
            }
        }
        if cursor > indexed {
            writer.append_rows(&rows, cursor, projector.last_id())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn wait_index(root: &Path, id: &str) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let s = store::get_session(root, id).unwrap();
            if s.indexed_seq == s.saved_seq {
                return;
            }
            assert!(Instant::now() < deadline, "index did not catch up: {:?}", s);
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    #[test]
    fn close_flushes_raw_and_index_recovers_without_duplicate_rows() {
        let dir = tempfile::tempdir().unwrap();
        let state = HistoryState::open(dir.path().into());
        let mut recorder = state.start("session-a", "workspace-a", store::fixture("a").source);
        recorder.push(EventKind::Start {
            cols: 100,
            rows: 30,
        });
        recorder.push(EventKind::Output {
            bytes: "命令\r\n结果\r\n提示符".as_bytes().into(),
        });
        recorder.checkpoint();
        recorder.push(EventKind::Clear);
        recorder.finish("测试关闭");
        state.shutdown().unwrap();
        wait_index(dir.path(), "session-a");
        let before = store::get_session(dir.path(), "session-a").unwrap();
        assert_eq!(before.state, "closed");
        index_session(dir.path(), "session-a", None).unwrap();
        let after = store::get_session(dir.path(), "session-a").unwrap();
        assert_eq!(before.last_record, after.last_record);
        let rows = store::page(
            dir.path(),
            "session-a",
            &Query {
                limit: 100,
                ..Default::default()
            },
            &Arc::new(AtomicBool::new(false)),
        )
        .unwrap()
        .rows;
        assert!(rows.iter().any(|r| r.text == "提示符"));
        assert!(rows.iter().any(|r| r.text.starts_with("已清理显示")));
        assert!(rows.last().unwrap().text.contains("测试关闭"));
        assert_eq!(state.queued.load(Ordering::Relaxed), 0);
    }
    #[test]
    fn queue_overflow_is_explicit_and_does_not_block() {
        let dir = tempfile::tempdir().unwrap();
        let state = HistoryState::open(dir.path().into());
        let mut recorder = state.start("overflow", "work", store::fixture("a").source);
        let started = Instant::now();
        recorder.push(EventKind::Output {
            bytes: vec![b'x'; SESSION_QUEUE + 1],
        });
        assert!(started.elapsed() < Duration::from_secs(1));
        recorder.push(EventKind::Output {
            bytes: b"after\r\n".to_vec(),
        });
        recorder.finish("done");
        state.shutdown().unwrap();
        wait_index(dir.path(), "overflow");
        let events = store::read_events(dir.path(), "overflow", 0, 100).unwrap();
        assert!(events
            .iter()
            .any(|e| matches!(e.kind, EventKind::Gap { .. })));
        assert!(state.status("overflow").unwrap().error.is_some());
        assert_eq!(state.queued.load(Ordering::Relaxed), 0);
    }
    #[test]
    fn interrupted_partial_line_is_indexed_once_after_reopen() {
        let dir = tempfile::tempdir().unwrap();
        store::init(dir.path()).unwrap();
        store::register(dir.path(), &store::fixture("crashed")).unwrap();
        let mut writer = store::ShardWriter::new(dir.path(), "crashed", "raw").unwrap();
        writer
            .append_events(&[Event {
                seq: 1,
                time: now(),
                elapsed: 10,
                kind: EventKind::Output {
                    bytes: "尚未换行的内容".as_bytes().to_vec(),
                },
            }])
            .unwrap();
        drop(writer);
        store::init(dir.path()).unwrap();
        store::recover(dir.path()).unwrap();
        index_session(dir.path(), "crashed", None).unwrap();
        let first = store::get_session(dir.path(), "crashed").unwrap();
        assert_eq!(first.state, "interrupted");
        let rows = store::page(
            dir.path(),
            "crashed",
            &Query {
                limit: 10,
                ..Default::default()
            },
            &Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        assert_eq!(rows.rows[0].text, "尚未换行的内容");
        assert!(rows.rows[1].text.contains("异常中断"));
        store::init(dir.path()).unwrap();
        store::recover(dir.path()).unwrap();
        index_session(dir.path(), "crashed", None).unwrap();
        assert_eq!(
            first.last_record,
            store::get_session(dir.path(), "crashed")
                .unwrap()
                .last_record
        );
    }
}

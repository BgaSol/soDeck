//! Bounded replayable byte output. Checkpoints are opaque plugin display state.
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
mod journal;

pub const BLOCK: usize = 16 * 1024;
const RETAIN: usize = 512 * 1024;
const SNAPSHOT: usize = 2 * 1024 * 1024;
const LARGE_SNAPSHOT: usize = 128 * 1024 * 1024;
pub type Sink = Arc<dyn Fn(Frame) -> Result<(), String> + Send + Sync>;
pub(crate) fn cleanup(directory: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if !kind.is_dir() || kind.is_symlink() {
            continue;
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let parts: Vec<_> = name.split('-').collect();
        if parts.len() != 5
            || parts.iter().any(|p| p.parse::<u64>().is_err())
            || parts[0] != parts[3]
        {
            continue;
        }
        let Ok(pid) = parts[0].parse::<u32>() else {
            continue;
        };
        if !crate::platform::process_exists(pid) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

#[cfg(test)]
mod cleanup_tests {
    #[test]
    fn cleanup_only_removes_recognized_directories_whose_owner_has_exited() {
        let root = std::env::temp_dir().join(format!(
            "sodeck-cleanup-{}-{}",
            std::process::id(),
            crate::core::now()
        ));
        let pid = std::process::id();
        let alive = root.join(format!("{pid}-1-2-{pid}-3"));
        #[cfg(windows)]
        let mut child = std::process::Command::new("cmd")
            .args(["/C", "exit", "0"])
            .spawn()
            .unwrap();
        #[cfg(unix)]
        let mut child = std::process::Command::new("sh")
            .args(["-c", "exit 0"])
            .spawn()
            .unwrap();
        let exited = child.id();
        child.wait().unwrap();
        drop(child);
        let gone = root.join(format!("{exited}-1-2-{exited}-3"));
        let foreign = root.join("user-files");
        for path in [&alive, &gone, &foreign] {
            std::fs::create_dir_all(path).unwrap();
            std::fs::write(path.join("data"), b"kept").unwrap();
        }
        super::cleanup(&root);
        assert!(alive.join("data").exists());
        assert!(foreign.join("data").exists());
        assert!(!gone.exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Frame {
    pub kind: &'static str,
    pub packet: u64,
    pub offset: u64,
    pub data: Vec<u8>,
    pub metadata: Value,
}
#[derive(Default)]
struct DisplayBytes {
    data: Vec<u8>,
    reserved: usize,
}
impl Drop for DisplayBytes {
    fn drop(&mut self) {
        journal::release(self.reserved);
    }
}
#[derive(Clone, Default)]
struct Checkpoint {
    offset: u64,
    bytes: Arc<DisplayBytes>,
    metadata: Value,
}
#[derive(Default)]
struct Reader {
    cursor: u64,
    packet: u64,
    acknowledged: u64,
    delivered: u64,
}
#[derive(Default)]
struct State {
    chunks: VecDeque<(u64, Vec<u8>)>,
    produced: u64,
    checkpoint: Checkpoint,
    readers: HashMap<String, Reader>,
    upload: HashMap<String, Checkpoint>,
    ended: Option<Value>,
    journal: Option<journal::Journal>,
    recovery_error: Option<String>,
    unrecoverable_until: u64,
    ui: Value,
}
#[derive(Default)]
pub struct Replay {
    state: Mutex<State>,
    changed: Notify,
}
impl Replay {
    pub fn journaled(directory: std::path::PathBuf) -> Result<Self, String> {
        Ok(Self {
            state: Mutex::new(State {
                journal: Some(journal::Journal::new(directory)?),
                ..State::default()
            }),
            changed: Notify::new(),
        })
    }
    pub fn metadata(&self, token: &str, metadata: Value) -> Result<(), String> {
        if metadata.to_string().len() > 8192 {
            return Err("终端阅读状态超过上限".into());
        }
        let mut state = self.state.lock().unwrap();
        if !state.readers.contains_key(token) {
            return Err("终端读取页面已失效".into());
        }
        state.ui = metadata;
        Ok(())
    }
    pub(crate) fn watermark(&self) -> u64 {
        self.state.lock().unwrap().produced
    }
    pub(crate) fn caught_up(&self, token: &str, watermark: u64) -> bool {
        self.state
            .lock()
            .unwrap()
            .readers
            .get(token)
            .is_some_and(|r| r.cursor >= watermark)
    }
    pub(crate) fn checkpoint_bytes(&self) -> Vec<u8> {
        self.state.lock().unwrap().checkpoint.bytes.data.clone()
    }
    pub async fn push(
        self: &Arc<Self>,
        bytes: &[u8],
        cancel: &CancellationToken,
    ) -> Result<(), String> {
        for block in bytes.chunks(BLOCK) {
            if cancel.is_cancelled() {
                return Err("数据流已取消".into());
            }
            if self.state.lock().unwrap().journal.is_some() {
                loop {
                    let changed = self.changed.notified();
                    let blocked = {
                        let state = self.state.lock().unwrap();
                        state.recovery_error.is_some()
                            && state.chunks.iter().map(|(_, b)| b.len()).sum::<usize>() > RETAIN
                    };
                    if !blocked {
                        break;
                    }
                    tokio::select! {_=changed=>{},_=cancel.cancelled()=>return Err("数据流已取消".into())}
                }
                let replay = self.clone();
                let data = block.to_vec();
                tokio::task::spawn_blocking(move || {
                    let mut state = replay.state.lock().unwrap();
                    let offset = state.produced;
                    let checkpoint = state
                        .readers
                        .values()
                        .map(|r| r.cursor)
                        .min()
                        .unwrap_or(state.checkpoint.offset)
                        .min(state.checkpoint.offset);
                    let journal = state.journal.as_mut().unwrap();
                    journal.prune(checkpoint);
                    if let Err(error) = journal.append(offset, &data) {
                        state.recovery_error = Some(error);
                        state.unrecoverable_until = offset + data.len() as u64;
                    }
                    state.produced += data.len() as u64;
                    state.chunks.push_back((offset, data));
                    Self::prune(&mut state);
                    while state.recovery_error.is_none()
                        && state.chunks.iter().map(|(_, b)| b.len()).sum::<usize>() > RETAIN
                    {
                        state.chunks.pop_front();
                    }
                })
                .await
                .map_err(|e| e.to_string())?;
                self.changed.notify_waiters();
                continue;
            }
            loop {
                let changed = self.changed.notified();
                let appended = {
                    let mut s = self.state.lock().unwrap();
                    Self::prune(&mut s);
                    let first = s.chunks.front().map(|c| c.0).unwrap_or(s.produced);
                    if s.produced - first + block.len() as u64 <= RETAIN as u64 {
                        let offset = s.produced;
                        s.chunks.push_back((offset, block.to_vec()));
                        s.produced += block.len() as u64;
                        true
                    } else {
                        false
                    }
                };
                if appended {
                    self.changed.notify_waiters();
                    break;
                }
                tokio::select! { biased; _=cancel.cancelled()=>return Err("数据流已取消".into()), _=changed=>{} }
            }
        }
        Ok(())
    }
    fn prune(s: &mut State) {
        let consumed =
            s.readers
                .values()
                .map(|r| r.cursor)
                .min()
                .unwrap_or(if s.journal.is_some() {
                    s.produced
                } else {
                    s.checkpoint.offset
                });
        let before = if s.journal.is_some() {
            consumed
        } else {
            consumed.min(s.checkpoint.offset)
        };
        while s
            .chunks
            .front()
            .is_some_and(|(n, b)| *n + b.len() as u64 <= before)
        {
            s.chunks.pop_front();
        }
    }
    pub fn finish(&self, metadata: Value) {
        self.state.lock().unwrap().ended = Some(metadata);
        self.changed.notify_waiters();
    }
    pub fn detach(&self, token: &str) {
        let mut s = self.state.lock().unwrap();
        s.readers.remove(token);
        s.upload.remove(token);
        Self::prune(&mut s);
        drop(s);
        self.changed.notify_waiters();
    }
    pub fn ack(&self, token: &str, packet: u64) -> Result<(), String> {
        let mut s = self.state.lock().unwrap();
        let r = s.readers.get_mut(token).ok_or("数据订阅已失效")?;
        if packet != r.packet || packet != r.acknowledged + 1 {
            return Err("数据确认序号无效".into());
        }
        r.acknowledged = packet;
        r.cursor = r.delivered;
        Self::prune(&mut s);
        drop(s);
        self.changed.notify_waiters();
        Ok(())
    }
    pub fn checkpoint(
        &self,
        token: &str,
        offset: u64,
        bytes: Vec<u8>,
        metadata: Value,
        begin: bool,
        done: bool,
    ) -> Result<(), String> {
        if bytes.len() > BLOCK {
            return Err("快照块超过上限".into());
        }
        if !metadata.is_null() && (!metadata.is_object() || metadata.to_string().len() > 8192) {
            return Err("终端快照元数据无效或超过上限".into());
        }
        let mut s = self.state.lock().unwrap();
        let reader = s.readers.get(token).ok_or("数据订阅已失效")?;
        if offset > reader.cursor || offset < s.checkpoint.offset {
            return Err("快照序号不对应已消费输出".into());
        }
        if begin {
            s.upload.insert(
                token.into(),
                Checkpoint {
                    offset,
                    bytes: Arc::new(DisplayBytes::default()),
                    metadata,
                },
            );
        }
        let limit = if s.journal.is_some() {
            LARGE_SNAPSHOT
        } else {
            SNAPSHOT
        };
        let c = s
            .upload
            .get_mut(token)
            .filter(|c| c.offset == offset)
            .ok_or("快照上传尚未开始")?;
        if c.bytes.data.len() + bytes.len() > limit {
            s.upload.remove(token);
            return Err("终端显示快照超过恢复额度".into());
        }
        let display = Arc::get_mut(&mut c.bytes).ok_or("终端快照正在共享，不能继续修改")?;
        let required = display.data.len() + bytes.len();
        if required > display.data.capacity() {
            let capacity = required.next_power_of_two().min(limit);
            let extra = capacity - display.data.capacity();
            journal::reserve(extra)?;
            if let Err(error) = display
                .data
                .try_reserve_exact(capacity - display.data.len())
            {
                journal::release(extra);
                return Err(format!("无法分配终端恢复快照：{error}"));
            }
            display.reserved += extra;
        }
        display.data.extend(bytes);
        if done {
            s.checkpoint = s.upload.remove(token).unwrap();
            if offset >= s.unrecoverable_until {
                s.recovery_error = None;
            }
            Self::prune(&mut s);
        }
        drop(s);
        self.changed.notify_waiters();
        Ok(())
    }
    async fn send(
        &self,
        token: &str,
        sink: &Sink,
        mut frame: Frame,
        cursor: u64,
        alive: &(dyn Fn() -> bool + Sync),
    ) -> Result<(), String> {
        if !alive() {
            return Err("界面挂载已失效".into());
        }
        let packet = {
            let mut s = self.state.lock().unwrap();
            let r = s.readers.get_mut(token).ok_or("订阅已失效")?;
            r.packet += 1;
            r.delivered = cursor;
            r.packet
        };
        frame.packet = packet;
        sink(frame)?;
        loop {
            let changed = self.changed.notified();
            {
                let s = self.state.lock().unwrap();
                let r = s.readers.get(token).ok_or("订阅已失效")?;
                if r.acknowledged == packet {
                    return Ok(());
                }
            }
            // Check abandoned surfaces even when no output arrives; no payload polling.
            tokio::select! {_=changed=>{},_=tokio::time::sleep(std::time::Duration::from_secs(2))=>if !alive(){return Err("界面已关闭".into());}}
        }
    }
    pub async fn subscribe(
        self: Arc<Self>,
        token: String,
        sink: Sink,
        alive: impl Fn() -> bool + Send + Sync + 'static,
    ) {
        let (checkpoint, initial) = {
            let mut s = self.state.lock().unwrap();
            let mut checkpoint = s.checkpoint.clone();
            checkpoint.metadata["ui"] = s.ui.clone();
            let initial = s.produced;
            if s.readers.contains_key(&token) {
                return;
            }
            s.readers.insert(
                token.clone(),
                Reader {
                    cursor: checkpoint.offset,
                    ..Default::default()
                },
            );
            (checkpoint, initial)
        };
        let work = async {
            let frame = |kind, offset, data, metadata| Frame {
                kind,
                packet: 0,
                offset,
                data,
                metadata,
            };
            self.send(
                &token,
                &sink,
                frame(
                    "reset",
                    checkpoint.offset,
                    vec![],
                    checkpoint.metadata.clone(),
                ),
                checkpoint.offset,
                &alive,
            )
            .await?;
            for chunk in checkpoint.bytes.data.chunks(BLOCK) {
                self.send(
                    &token,
                    &sink,
                    frame("snapshot", checkpoint.offset, chunk.to_vec(), Value::Null),
                    checkpoint.offset,
                    &alive,
                )
                .await?;
            }
            self.send(
                &token,
                &sink,
                frame("restore", checkpoint.offset, vec![], Value::Null),
                checkpoint.offset,
                &alive,
            )
            .await?;
            let mut cursor = checkpoint.offset;
            drop(checkpoint);
            let mut caught_up = false;
            let mut recovery_error = None;
            loop {
                let changed = self.changed.notified();
                if !alive() {
                    return Err("界面已关闭".into());
                }
                let error = self.state.lock().unwrap().recovery_error.clone();
                if error != recovery_error {
                    self.send(
                        &token,
                        &sink,
                        frame(
                            "recovery",
                            cursor,
                            vec![],
                            serde_json::json!({"available":error.is_none(),"error":error}),
                        ),
                        cursor,
                        &alive,
                    )
                    .await?;
                    recovery_error = error;
                }
                if !caught_up && cursor >= initial {
                    self.send(
                        &token,
                        &sink,
                        frame("caughtUp", cursor, vec![], Value::Null),
                        cursor,
                        &alive,
                    )
                    .await?;
                    caught_up = true;
                }
                let replay = self.clone();
                let (chunk, ended) = tokio::task::spawn_blocking(move || {
                    let s = replay.state.lock().unwrap();
                    let memory = s
                        .chunks
                        .iter()
                        .find(|(n, b)| *n + b.len() as u64 > cursor)
                        .cloned();
                    let chunk = if cursor < s.produced
                        && memory.as_ref().is_none_or(|(start, _)| *start > cursor)
                    {
                        if let Some(journal) = &s.journal {
                            Some((
                                cursor,
                                journal
                                    .read(cursor, (s.produced - cursor).min(BLOCK as u64) as usize)
                                    .map_err(|_| "终端恢复数据不可用，请保留原页面".to_string())?,
                            ))
                        } else {
                            memory
                        }
                    } else {
                        memory
                    };
                    Ok::<_, String>((chunk, s.ended.clone()))
                })
                .await
                .map_err(|e| e.to_string())??;
                if let Some((start, data)) = chunk {
                    if start > cursor {
                        return Err("终端输出序号出现缺口".into());
                    }
                    let data = data[(cursor - start) as usize..].to_vec();
                    cursor += data.len() as u64;
                    self.send(
                        &token,
                        &sink,
                        frame("data", cursor, data, Value::Null),
                        cursor,
                        &alive,
                    )
                    .await?;
                } else if let Some(ended) = ended {
                    self.send(
                        &token,
                        &sink,
                        frame("end", cursor, vec![], ended),
                        cursor,
                        &alive,
                    )
                    .await?;
                    return Ok::<(), String>(());
                } else {
                    tokio::select! {_=changed=>{},_=tokio::time::sleep(std::time::Duration::from_secs(2))=>{}}
                }
            }
        };
        if let Err(error) = work.await {
            let _ = sink(Frame {
                kind: "error",
                packet: 0,
                offset: 0,
                data: vec![],
                metadata: Value::String(error),
            });
            self.detach(&token);
        }
    }
}

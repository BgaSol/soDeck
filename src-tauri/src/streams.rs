//! Bounded replayable byte output. Checkpoints are opaque plugin display state.
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

pub const BLOCK: usize = 16 * 1024;
const RETAIN: usize = 512 * 1024;
const SNAPSHOT: usize = 2 * 1024 * 1024;
pub type Sink = Arc<dyn Fn(Frame) -> Result<(), String> + Send + Sync>;
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Frame {
    pub kind: &'static str,
    pub packet: u64,
    pub offset: u64,
    pub data: Vec<u8>,
    pub metadata: Value,
}
#[derive(Clone, Default)]
struct Checkpoint {
    offset: u64,
    bytes: Vec<u8>,
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
}
#[derive(Default)]
pub struct Replay {
    state: Mutex<State>,
    changed: Notify,
}
impl Replay {
    pub(crate) fn checkpoint_bytes(&self) -> Vec<u8> {
        self.state.lock().unwrap().checkpoint.bytes.clone()
    }
    pub async fn push(&self, bytes: &[u8], cancel: &CancellationToken) -> Result<(), String> {
        for block in bytes.chunks(BLOCK) {
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
        let before = s
            .readers
            .values()
            .map(|r| r.cursor)
            .min()
            .unwrap_or(s.checkpoint.offset)
            .min(s.checkpoint.offset);
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
                    bytes: vec![],
                    metadata,
                },
            );
        }
        let c = s
            .upload
            .get_mut(token)
            .filter(|c| c.offset == offset)
            .ok_or("快照上传尚未开始")?;
        if c.bytes.len() + bytes.len() > SNAPSHOT {
            s.upload.remove(token);
            return Err("终端显示快照超过 2 MiB".into());
        }
        c.bytes.extend(bytes);
        if done {
            s.checkpoint = s.upload.remove(token).unwrap();
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
            let checkpoint = s.checkpoint.clone();
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
            for chunk in checkpoint.bytes.chunks(BLOCK) {
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
            let mut caught_up = false;
            loop {
                let changed = self.changed.notified();
                if !alive() {
                    return Err("界面已关闭".into());
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
                let (chunk, ended) = {
                    let s = self.state.lock().unwrap();
                    (
                        s.chunks
                            .iter()
                            .find(|(n, b)| *n + b.len() as u64 > cursor)
                            .cloned(),
                        s.ended.clone(),
                    )
                };
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

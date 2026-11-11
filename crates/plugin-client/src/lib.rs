//! Small synchronous plugin client with a continuously running response reader.
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    io::{self, BufRead, Read, Write},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    time::Duration,
};
const MAX: usize = 64 * 1024;
#[derive(Clone)]
pub struct Client {
    inner: Arc<Inner>,
}
struct Inner {
    state: shared_state::Inbox,
    closed: AtomicBool,
    writer: Mutex<io::Stdout>,
    pending: Mutex<HashMap<String, mpsc::SyncSender<Result<Value, RpcError>>>>,
    sequence: AtomicU64,
}
impl Client {
    pub fn is_closed(&self) -> bool {
        self.inner.closed.load(Ordering::Acquire)
    }
    fn send(&self, frame: Value) -> Result<(), String> {
        if self.inner.closed.load(Ordering::Acquire) {
            return Err("核心通信已结束".into());
        }
        let mut bytes = serde_json::to_vec(&frame).map_err(|e| e.to_string())?;
        bytes.push(b'\n');
        if bytes.len() > MAX {
            return Err("控制帧超过 64 KiB".into());
        }
        let mut out = self.inner.writer.lock().unwrap();
        out.write_all(&bytes)
            .and_then(|_| out.flush())
            .map_err(|e| e.to_string())
    }
    pub fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        self.call_detailed(method, params).map_err(|e| e.message)
    }
    pub fn call_detailed(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        let id = format!(
            "plugin-{}",
            self.inner.sequence.fetch_add(1, Ordering::Relaxed)
        );
        let (tx, rx) = mpsc::sync_channel(1);
        {
            let mut pending = self.inner.pending.lock().unwrap();
            if pending.len() >= 32 {
                return Err(RpcError::local("插件请求已满"));
            }
            pending.insert(id.clone(), tx);
        }
        let result = self
            .send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .map_err(RpcError::local)
            .and_then(|_| {
                rx.recv_timeout(Duration::from_secs(4))
                    .map_err(|_| RpcError::local("核心请求超时"))?
            });
        self.inner.pending.lock().unwrap().remove(&id);
        result
    }
}
fn read_control_line(input: &mut impl BufRead) -> io::Result<Vec<u8>> {
    let mut line = Vec::new();
    Read::take(&mut *input, MAX as u64 + 1).read_until(b'\n', &mut line)?;
    Ok(line)
}
pub fn run(
    handler: impl Fn(&Client, &str, Value) -> Result<Value, String> + Send + Sync + 'static,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = Arc::new(Client {
        inner: Arc::new(Inner {
            closed: AtomicBool::new(false),
            writer: Mutex::new(io::stdout()),
            pending: Mutex::new(HashMap::new()),
            sequence: AtomicU64::new(0),
            state: shared_state::Inbox::default(),
        }),
    });
    let handler = Arc::new(handler);
    let (tx, rx) = mpsc::sync_channel::<Value>(32);
    let rx = Arc::new(Mutex::new(rx));
    for _ in 0..4 {
        let client = client.clone();
        let handler = handler.clone();
        let rx = rx.clone();
        std::thread::spawn(move || loop {
            let frame = { rx.lock().unwrap().recv() };
            let Ok(frame) = frame else { break };
            let result = handler(
                &client,
                frame["method"].as_str().unwrap_or_default(),
                frame["params"].clone(),
            );
            let response = match result {
                Ok(value) => json!({"jsonrpc":"2.0","id":frame["id"],"result":value}),
                Err(error) => {
                    json!({"jsonrpc":"2.0","id":frame["id"],"error":{"code":-32000,"message":error}})
                }
            };
            let _ = client.send(response);
        });
    }
    let mut input = io::stdin().lock();
    loop {
        let line = read_control_line(&mut input)?;
        if line.is_empty() {
            break;
        }
        if line.len() > MAX {
            return Err("控制帧超过上限".into());
        }
        let frame: Value = serde_json::from_slice(&line)?;
        if frame["jsonrpc"] != "2.0" {
            return Err("需要 JSON-RPC 2.0".into());
        }
        if frame["method"] == "shutdown" {
            client.send(json!({"jsonrpc":"2.0","id":frame["id"],"result":{}}))?;
            break;
        }
        if frame.get("method").is_some() {
            if frame["method"] == "state.changed" {
                let result = client.inner.state.receive(&frame["params"]);
                client.send(match result {
                    Ok(value)=>json!({"jsonrpc":"2.0","id":frame["id"],"result":value}),
                    Err(message)=>json!({"jsonrpc":"2.0","id":frame["id"],"error":{"code":-32000,"message":message}}),
                })?;
                continue;
            }
            if let Err(mpsc::TrySendError::Full(frame)) = tx.try_send(frame) {
                client.send(json!({"jsonrpc":"2.0","id":frame["id"],"error":{"code":-32001,"message":"插件队列已满"}}))?;
            }
        } else if let Some(id) = frame["id"].as_str() {
            if let Some(reply) = client.inner.pending.lock().unwrap().remove(id) {
                let result = if frame.get("error").is_some() {
                    Err(RpcError::from_frame(&frame["error"]))
                } else {
                    Ok(frame["result"].clone())
                };
                let _ = reply.send(result);
            }
        }
    }
    client.inner.closed.store(true, Ordering::Release);
    for (_, reply) in client.inner.pending.lock().unwrap().drain() {
        let _ = reply.send(Err(RpcError::local("核心通信已结束")));
    }
    Ok(())
}
pub mod encoding;
pub mod settings;
pub mod shared_state;

#[derive(Debug, Clone)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    pub data: Value,
}
impl RpcError {
    fn local(message: impl Into<String>) -> Self {
        Self {
            code: -32003,
            message: message.into(),
            data: Value::Null,
        }
    }
    fn from_frame(error: &Value) -> Self {
        Self {
            code: error["code"].as_i64().unwrap_or(-32000),
            message: error["message"].as_str().unwrap_or("核心调用失败").into(),
            data: error["data"].clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::VecDeque, io::BufReader};

    enum ReadStep {
        Data(io::Cursor<Vec<u8>>),
        Error(io::ErrorKind, &'static str),
    }
    impl ReadStep {
        fn data(bytes: impl Into<Vec<u8>>) -> Self {
            Self::Data(io::Cursor::new(bytes.into()))
        }
    }
    struct ScriptedRead {
        steps: VecDeque<ReadStep>,
        reads: usize,
        bytes_read: usize,
    }
    impl ScriptedRead {
        fn new(steps: impl IntoIterator<Item = ReadStep>) -> Self {
            Self {
                steps: steps.into_iter().collect(),
                reads: 0,
                bytes_read: 0,
            }
        }
    }
    impl Read for ScriptedRead {
        fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
            if output.is_empty() {
                return Ok(0);
            }
            self.reads += 1;
            match self.steps.pop_front() {
                Some(ReadStep::Data(mut data)) => {
                    let count = data.read(output)?;
                    self.bytes_read += count;
                    if data.position() < data.get_ref().len() as u64 {
                        self.steps.push_front(ReadStep::Data(data));
                    }
                    Ok(count)
                }
                Some(ReadStep::Error(kind, message)) => Err(io::Error::new(kind, message)),
                None => Ok(0),
            }
        }
    }

    #[test]
    fn control_line_limit_counts_lf_crlf_and_eof_bytes() {
        for ending in [b"\n".as_slice(), b"\r\n".as_slice(), b"".as_slice()] {
            for length in [MAX - 1, MAX, MAX + 1] {
                for capacity in [7, MAX + 8] {
                    let mut frame = vec![b'x'; length - ending.len()];
                    frame.extend_from_slice(ending);
                    let mut stream = frame.clone();
                    if !ending.is_empty() {
                        stream.extend_from_slice(b"next\n");
                    }
                    let mut input = BufReader::with_capacity(
                        capacity,
                        ScriptedRead::new([ReadStep::data(stream)]),
                    );
                    let line = read_control_line(&mut input).unwrap();
                    assert_eq!(line, frame);
                    assert_eq!(line.len() > MAX, length == MAX + 1);
                    let next = read_control_line(&mut input).unwrap();
                    assert_eq!(
                        next,
                        if ending.is_empty() {
                            b"".as_slice()
                        } else {
                            b"next\n".as_slice()
                        }
                    );
                }
            }
        }
    }

    #[test]
    fn control_line_empty_eof_is_empty() {
        let mut input = BufReader::new(ScriptedRead::new([]));
        assert!(read_control_line(&mut input).unwrap().is_empty());
        assert_eq!(input.get_ref().bytes_read, 0);
    }

    #[test]
    fn oversized_control_line_keeps_its_unread_tail() {
        let mut stream = vec![b'x'; MAX + 1];
        stream.extend_from_slice(b"tail\nnext\n");
        let mut input =
            BufReader::with_capacity(MAX + 32, ScriptedRead::new([ReadStep::data(stream)]));
        assert_eq!(read_control_line(&mut input).unwrap(), vec![b'x'; MAX + 1]);
        assert_eq!(input.buffer(), b"tail\nnext\n");
        assert_eq!(read_control_line(&mut input).unwrap(), b"tail\n");
        assert_eq!(read_control_line(&mut input).unwrap(), b"next\n");
    }

    #[test]
    fn control_line_does_not_observe_errors_after_lf_or_the_limit() {
        for prefix in [b"ok\n".to_vec(), vec![b'x'; MAX + 1]] {
            let expected = prefix.clone();
            let mut input = BufReader::with_capacity(
                MAX + 1,
                ScriptedRead::new([
                    ReadStep::data(prefix),
                    ReadStep::Error(io::ErrorKind::BrokenPipe, "later error"),
                ]),
            );
            assert_eq!(read_control_line(&mut input).unwrap(), expected);
            assert_eq!(input.get_ref().reads, 1);
            assert_eq!(input.get_ref().steps.len(), 1);
            let error = read_control_line(&mut input).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
            assert_eq!(error.to_string(), "later error");
        }
    }

    #[test]
    fn control_line_errors_preserve_kind_message_and_consumed_prefix() {
        for kind in [
            io::ErrorKind::BrokenPipe,
            io::ErrorKind::InvalidData,
            io::ErrorKind::UnexpectedEof,
        ] {
            for prefix in [b"".as_slice(), b"ab".as_slice()] {
                let mut steps = Vec::new();
                if !prefix.is_empty() {
                    steps.push(ReadStep::data(prefix));
                }
                steps.push(ReadStep::Error(kind, "read failed"));
                steps.push(ReadStep::data(b"c\nnext\n".as_slice()));
                let mut input = BufReader::with_capacity(3, ScriptedRead::new(steps));
                let error = read_control_line(&mut input).unwrap_err();
                assert_eq!(error.kind(), kind);
                assert_eq!(error.to_string(), "read failed");
                assert_eq!(input.get_ref().bytes_read, prefix.len());
                assert_eq!(read_control_line(&mut input).unwrap(), b"c\n");
                assert_eq!(read_control_line(&mut input).unwrap(), b"next\n");
            }
        }
    }

    #[test]
    fn control_line_retries_interrupted_without_losing_its_prefix() {
        let mut input = BufReader::with_capacity(
            3,
            ScriptedRead::new([
                ReadStep::Error(io::ErrorKind::Interrupted, "before input"),
                ReadStep::data(b"ab".as_slice()),
                ReadStep::Error(io::ErrorKind::Interrupted, "after prefix"),
                ReadStep::Error(io::ErrorKind::Interrupted, "interrupted again"),
                ReadStep::data(b"c\nnext\n".as_slice()),
            ]),
        );
        assert_eq!(read_control_line(&mut input).unwrap(), b"abc\n");
        assert_eq!(read_control_line(&mut input).unwrap(), b"next\n");
    }

    #[test]
    fn structured_errors_keep_recovery_code_and_legacy_message() {
        let e = RpcError::from_frame(
            &json!({"code":-32000,"message":"过期","data":{"code":"staleContext"}}),
        );
        assert_eq!(e.data["code"], "staleContext");
        assert_eq!(e.message, "过期");
    }
}

//! Small synchronous plugin client with a continuously running response reader.
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    io::{self, BufRead, Write},
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
        let mut line = Vec::new();
        // Stop allocating at the protocol limit even if the sender never writes LF.
        while line.len() <= MAX {
            let buf = input.fill_buf()?;
            if buf.is_empty() {
                break;
            }
            let n = buf
                .iter()
                .position(|b| *b == b'\n')
                .map(|n| n + 1)
                .unwrap_or(buf.len())
                .min(MAX + 1 - line.len());
            let end = buf[n - 1] == b'\n';
            line.extend_from_slice(&buf[..n]);
            input.consume(n);
            if end {
                break;
            }
        }
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
    #[test]
    fn structured_errors_keep_recovery_code_and_legacy_message() {
        let e = RpcError::from_frame(
            &json!({"code":-32000,"message":"过期","data":{"code":"staleContext"}}),
        );
        assert_eq!(e.data["code"], "staleContext");
        assert_eq!(e.message, "过期");
    }
}

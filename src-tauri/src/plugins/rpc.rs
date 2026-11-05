use super::Output;
use serde_json::{json, Value};
use std::{
    io::{self, BufRead, BufReader, BufWriter, Write},
    process::ChildStdin,
    sync::mpsc::SyncSender,
    thread,
};

pub(super) const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
pub(super) const FRAME_LIMIT: usize = 64 * 1024;

#[derive(Debug)]
pub(super) struct RpcError {
    pub code: i32,
    pub message: String,
}
impl RpcError {
    pub fn invalid(message: impl Into<String>) -> Self {
        Self {
            code: -32602,
            message: message.into(),
        }
    }
    pub fn execution(message: impl Into<String>) -> Self {
        Self {
            code: -32000,
            message: message.into(),
        }
    }
    pub fn method_not_found() -> Self {
        Self {
            code: -32601,
            message: "未知核心 API 方法".into(),
        }
    }
    pub fn busy() -> Self {
        Self {
            code: -32001,
            message: "插件核心请求队列已满，请稍后重试".into(),
        }
    }
    pub fn timeout() -> Self {
        Self {
            code: -32003,
            message: "核心请求执行超时".into(),
        }
    }
    pub fn response(self, id: Value) -> Value {
        json!({"jsonrpc":"2.0","id":id,"error":{"code":self.code,"message":self.message}})
    }
}

pub(super) fn response(id: Value, result: Result<Value, RpcError>) -> Value {
    match result {
        Ok(value) => json!({"jsonrpc":"2.0","id":id,"result":value}),
        Err(error) => error.response(id),
    }
}

pub(super) fn encode(frame: &Value) -> Result<Vec<u8>, String> {
    let mut bytes = serde_json::to_vec(frame).map_err(|e| e.to_string())?;
    if bytes.len() + 1 > FRAME_LIMIT {
        return Err("RPC 控制帧超过 64 KiB；请使用数据通道传输正文".into());
    }
    bytes.push(b'\n');
    Ok(bytes)
}

pub(super) struct Writer {
    priority: SyncSender<Vec<u8>>,
    calls: SyncSender<Vec<u8>>,
}

pub(super) fn spawn_writer(input: ChildStdin) -> Writer {
    use std::{sync::mpsc, time::Duration};
    // A full batch of completed inbound calls must fit even before the writer
    // thread gets CPU time. Ordinary outbound calls cannot occupy these slots.
    let (priority, urgent) = mpsc::sync_channel::<Vec<u8>>(64);
    let (calls, normal) = mpsc::sync_channel::<Vec<u8>>(16);
    thread::spawn(move || {
        let mut input = BufWriter::new(input);
        loop {
            let bytes = match urgent.try_recv() {
                Ok(bytes) => bytes,
                Err(_) => match normal.recv_timeout(Duration::from_millis(10)) {
                    Ok(bytes) => bytes,
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        match urgent.recv_timeout(Duration::from_millis(10)) {
                            Ok(bytes) => bytes,
                            Err(mpsc::RecvTimeoutError::Timeout) => continue,
                            Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        }
                    }
                },
            };
            if input.write_all(&bytes).and_then(|_| input.flush()).is_err() {
                break;
            }
        }
    });
    Writer { priority, calls }
}

pub(crate) fn read_frame(reader: &mut impl BufRead, limit: usize) -> io::Result<Option<Vec<u8>>> {
    let mut bytes = Vec::new();
    let mut oversized = false;
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            if oversized {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "单行数据超过长度上限",
                ));
            }
            return Ok((!bytes.is_empty()).then_some(bytes));
        }
        let length = available
            .iter()
            .position(|b| *b == b'\n')
            .map_or(available.len(), |i| i + 1);
        let finished = available[length - 1] == b'\n';
        if !oversized {
            if bytes.len() + length > limit {
                oversized = true;
                bytes.clear();
            } else {
                bytes.extend_from_slice(&available[..length]);
            }
        }
        reader.consume(length);
        if finished {
            if oversized {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "单行数据超过长度上限",
                ));
            }
            return Ok(Some(bytes));
        }
    }
}

pub(super) fn queue_frame(writer: &Writer, frame: Value) -> Result<(), String> {
    let bytes = encode(&frame)?;
    let queue = if frame.get("method").is_none()
        || matches!(frame["method"].as_str(), Some("initialize" | "shutdown"))
    {
        &writer.priority
    } else {
        &writer.calls
    };
    queue
        .try_send(bytes)
        .map_err(|_| "插件未及时读取核心响应，已停止通信".into())
}

pub(super) fn spawn_reader(
    output: impl io::Read + Send + 'static,
    sender: SyncSender<Output>,
    diagnostic: bool,
) {
    thread::spawn(move || {
        let mut reader = BufReader::new(output);
        loop {
            let item = match read_frame(&mut reader, if diagnostic { 8192 } else { 65536 }) {
                Ok(Some(bytes)) if diagnostic => {
                    Output::Diagnostic(String::from_utf8_lossy(&bytes).trim_end().into())
                }
                Ok(Some(bytes)) => match serde_json::from_slice(&bytes) {
                    Ok(value) => Output::Frame(value),
                    Err(error) => {
                        let _ = sender.send(Output::Broken(format!(
                            "插件标准输出不是有效 RPC JSON：{error}"
                        )));
                        break;
                    }
                },
                Ok(None) => {
                    if !diagnostic {
                        let _ = sender.send(Output::Broken("插件通信管道已关闭".into()));
                    }
                    break;
                }
                Err(error) if diagnostic && error.kind() == io::ErrorKind::InvalidData => {
                    Output::Diagnostic("诊断日志行过长，已省略".into())
                }
                Err(error) => {
                    let _ = sender.send(Output::Broken(format!("读取插件管道失败：{error}")));
                    break;
                }
            };
            if sender.send(item).is_err() {
                break;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn outgoing_control_limit_counts_json_escaping_and_the_line_ending() {
        let frame = json!({"text":""});
        let overhead = encode(&frame).unwrap().len();
        assert_eq!(
            encode(&json!({"text":"x".repeat(FRAME_LIMIT-overhead)}))
                .unwrap()
                .len(),
            FRAME_LIMIT
        );
        assert!(encode(&json!({"text":"x".repeat(FRAME_LIMIT-overhead+1)})).is_err());
        // Newlines expand to two bytes inside JSON; raw string length is insufficient.
        assert!(encode(&json!({"text":"\n".repeat(FRAME_LIMIT/2)})).is_err());
    }
}

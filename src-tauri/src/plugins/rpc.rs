use super::Output;
use serde_json::{json, Value};
use std::{
    io::{self, BufRead, BufReader, BufWriter, Read, Write},
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
    pub data: Value,
}
impl RpcError {
    pub fn invalid(message: impl Into<String>) -> Self {
        Self {
            code: -32602,
            message: message.into(),
            data: Value::Null,
        }
    }
    pub fn execution(message: impl Into<String>) -> Self {
        Self {
            code: -32000,
            message: message.into(),
            data: Value::Null,
        }
    }
    pub fn method_not_found() -> Self {
        Self {
            code: -32601,
            message: "未知核心 API 方法".into(),
            data: Value::Null,
        }
    }
    pub fn busy() -> Self {
        Self {
            code: -32001,
            message: "插件核心请求队列已满，请稍后重试".into(),
            data: Value::Null,
        }
    }
    pub fn timeout() -> Self {
        Self {
            code: -32003,
            message: "核心请求执行超时".into(),
            data: Value::Null,
        }
    }
    pub fn response(self, id: Value) -> Value {
        json!({"jsonrpc":"2.0","id":id,"error":{"code":self.code,"message":self.message,"data":self.data}})
    }
    pub fn session(code: &str, message: String) -> Self {
        Self {
            code: -32000,
            message,
            data: json!({"code":code}),
        }
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
    let cap = limit
        .checked_add(1)
        .and_then(|n| u64::try_from(n).ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "读取长度上限过大"))?;
    let mut bytes = Vec::new();
    Read::take(&mut *reader, cap).read_until(b'\n', &mut bytes)?;
    if bytes.len() > limit {
        if bytes.last() != Some(&b'\n') {
            reader.skip_until(b'\n')?;
        }
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "单行数据超过长度上限",
        ));
    }
    Ok((!bytes.is_empty()).then_some(bytes))
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
    use std::{collections::VecDeque, io::Read};

    enum ReadStep {
        Data(&'static [u8]),
        Error(io::ErrorKind, &'static str),
    }

    struct ScriptedRead {
        steps: VecDeque<ReadStep>,
        offset: usize,
    }

    impl ScriptedRead {
        fn new(steps: impl IntoIterator<Item = ReadStep>) -> Self {
            Self {
                steps: steps.into_iter().collect(),
                offset: 0,
            }
        }
    }

    impl Read for ScriptedRead {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if buf.is_empty() {
                return Ok(0);
            }
            loop {
                match self.steps.front() {
                    Some(ReadStep::Data(data)) if self.offset < data.len() => {
                        let count = buf.len().min(data.len() - self.offset);
                        buf[..count].copy_from_slice(&data[self.offset..self.offset + count]);
                        self.offset += count;
                        return Ok(count);
                    }
                    Some(ReadStep::Data(_)) => {
                        self.steps.pop_front();
                        self.offset = 0;
                    }
                    Some(ReadStep::Error(_, _)) => {
                        let Some(ReadStep::Error(kind, message)) = self.steps.pop_front() else {
                            unreachable!()
                        };
                        return Err(io::Error::new(kind, message));
                    }
                    None => return Ok(0),
                }
            }
        }
    }

    fn remaining(reader: &mut impl Read) -> Vec<u8> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).unwrap();
        bytes
    }

    #[test]
    fn incoming_frames_preserve_bytes_line_endings_and_eof_tails() {
        for capacity in [1, 2, 3, 8] {
            for bytes in [b"".as_slice(), b"abc\n", b"ab\r\n", b"abcd", b"\xff\xfe\n"] {
                let mut reader = BufReader::with_capacity(capacity, bytes);
                let expected = (!bytes.is_empty()).then(|| bytes.to_vec());
                assert_eq!(read_frame(&mut reader, 4).unwrap(), expected);
                assert_eq!(read_frame(&mut reader, 4).unwrap(), None);
            }
            let mut reader = BufReader::with_capacity(capacity, b"a\nb\r\ntail".as_slice());
            for expected in [b"a\n".as_slice(), b"b\r\n", b"tail"] {
                assert_eq!(read_frame(&mut reader, 4).unwrap().unwrap(), expected);
            }
            assert_eq!(read_frame(&mut reader, 4).unwrap(), None);
        }
        for ending in [b"".as_slice(), b"\n", b"\r\n"] {
            let mut bytes = vec![b'x'; FRAME_LIMIT - ending.len()];
            bytes.extend_from_slice(ending);
            let mut reader = BufReader::with_capacity(7, bytes.as_slice());
            assert_eq!(read_frame(&mut reader, FRAME_LIMIT).unwrap(), Some(bytes));
        }
    }

    #[test]
    fn oversized_frames_drain_only_the_current_line() {
        for capacity in [1, 2, 3, 8] {
            for bytes in [b"12345\nok\n".as_slice(), b"123456789\nok\n"] {
                let mut reader = BufReader::with_capacity(capacity, bytes);
                let error = read_frame(&mut reader, 5).unwrap_err();
                assert_eq!(error.kind(), io::ErrorKind::InvalidData);
                assert_eq!(error.to_string(), "单行数据超过长度上限");
                assert_eq!(read_frame(&mut reader, 5).unwrap(), Some(b"ok\n".to_vec()));
                assert_eq!(read_frame(&mut reader, 5).unwrap(), None);
            }
            let mut reader = BufReader::with_capacity(capacity, b"123456789".as_slice());
            assert_eq!(
                read_frame(&mut reader, 5).unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
            assert_eq!(read_frame(&mut reader, 5).unwrap(), None);
        }
    }

    #[test]
    fn zero_limit_rejects_nonempty_lines_and_preserves_the_next_line() {
        for bytes in [b"\nok\n".as_slice(), b"a\nok\n", b"a"] {
            let mut reader = BufReader::with_capacity(2, bytes);
            assert_eq!(
                read_frame(&mut reader, 0).unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
            let expected = bytes.ends_with(b"ok\n").then(|| b"ok\n".to_vec());
            assert_eq!(read_frame(&mut reader, 3).unwrap(), expected);
        }
        assert_eq!(read_frame(&mut b"".as_slice(), 0).unwrap(), None);
    }

    #[test]
    fn overflowing_limit_is_rejected_before_accessing_the_reader() {
        struct Unreadable;
        impl Read for Unreadable {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                panic!("invalid limit must not read")
            }
        }
        impl BufRead for Unreadable {
            fn fill_buf(&mut self) -> io::Result<&[u8]> {
                panic!("invalid limit must not access the buffer")
            }
            fn consume(&mut self, _: usize) {
                panic!("invalid limit must not consume")
            }
        }
        let error = read_frame(&mut Unreadable, usize::MAX).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(error.to_string(), "读取长度上限过大");
    }

    #[test]
    fn non_interrupted_errors_keep_the_error_and_consumed_prefix() {
        for capacity in [1, 2, 8] {
            for prefix in [b"".as_slice(), b"ab", b"123456"] {
                let mut reader = BufReader::with_capacity(
                    capacity,
                    ScriptedRead::new([
                        ReadStep::Data(prefix),
                        ReadStep::Error(io::ErrorKind::WouldBlock, "scripted read failure"),
                        ReadStep::Data(b"rest\nok\n"),
                    ]),
                );
                let error = read_frame(&mut reader, 4).unwrap_err();
                assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
                assert_eq!(error.to_string(), "scripted read failure");
                assert_eq!(remaining(&mut reader), b"rest\nok\n");
            }
        }
    }

    #[test]
    fn interrupted_reads_retry_before_data_after_prefix_and_during_drain() {
        for capacity in [1, 2, 8] {
            let mut reader = BufReader::with_capacity(
                capacity,
                ScriptedRead::new([
                    ReadStep::Error(io::ErrorKind::Interrupted, "before data"),
                    ReadStep::Data(b"ab"),
                    ReadStep::Error(io::ErrorKind::Interrupted, "after prefix"),
                    ReadStep::Data(b"c\nok\n"),
                ]),
            );
            assert_eq!(read_frame(&mut reader, 4).unwrap(), Some(b"abc\n".to_vec()));
            assert_eq!(read_frame(&mut reader, 4).unwrap(), Some(b"ok\n".to_vec()));

            let mut reader = BufReader::with_capacity(
                capacity,
                ScriptedRead::new([
                    ReadStep::Data(b"123456"),
                    ReadStep::Error(io::ErrorKind::Interrupted, "during drain"),
                    ReadStep::Data(b"78\nok\n"),
                ]),
            );
            assert_eq!(
                read_frame(&mut reader, 4).unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
            assert_eq!(read_frame(&mut reader, 4).unwrap(), Some(b"ok\n".to_vec()));
        }
    }

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

//! The three existing output uses of an owned remote execution.
use crate::{
    remote_program::Decoder,
    streams::{capture::Capture, Replay},
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

pub(crate) struct MemoryOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    limit: usize,
    decoder: Decoder,
}
impl MemoryOutput {
    pub fn new(limit: usize, startup: bool) -> Self {
        Self {
            stdout: vec![],
            stderr: vec![],
            limit,
            decoder: Decoder::new(startup, limit),
        }
    }
    pub fn startup(&self) -> Value {
        self.decoder.metadata()
    }
}
pub(crate) struct CapturedOutput {
    pub capture: Mutex<Capture>,
    decoder: Mutex<Decoder>,
    sequence: Mutex<u64>,
    terminal: bool,
}
impl CapturedOutput {
    pub fn new(capture: Capture, limit: usize, startup: bool, terminal: bool) -> Self {
        Self {
            capture: Mutex::new(capture),
            decoder: Mutex::new(Decoder::new(startup, limit)),
            sequence: Mutex::new(0),
            terminal,
        }
    }
    pub fn startup(&self) -> Value {
        self.decoder.lock().unwrap().metadata()
    }
}
#[derive(Clone)]
pub(crate) enum OutputTarget {
    Memory(Arc<Mutex<MemoryOutput>>),
    Capture(Arc<CapturedOutput>),
    Terminal(Arc<Replay>),
}
impl OutputTarget {
    pub fn ready(&self) -> bool {
        match self {
            Self::Memory(out) => out.lock().unwrap().decoder.ready(),
            Self::Capture(out) => out.decoder.lock().unwrap().ready(),
            Self::Terminal(_) => true,
        }
    }
    pub async fn append(
        &self,
        stderr: bool,
        bytes: &[u8],
        cancel: &CancellationToken,
    ) -> Result<(), String> {
        match self {
            Self::Memory(out) => {
                let mut out = out.lock().unwrap();
                for (stderr, bytes) in out.decoder.push(stderr, bytes)? {
                    if out.stdout.len() + out.stderr.len() + bytes.len() > out.limit {
                        return Err("outputLimit:命令输出超过预算".into());
                    }
                    if stderr {
                        out.stderr.extend(bytes);
                    } else {
                        out.stdout.extend(bytes);
                    }
                }
                Ok(())
            }
            Self::Capture(out) => {
                let chunks = out.decoder.lock().unwrap().push(stderr, bytes)?;
                let mut encoded = Vec::new();
                for (stderr, bytes) in chunks {
                    for bytes in bytes.chunks(8192) {
                        let mut sequence = out.sequence.lock().unwrap();
                        *sequence += 1;
                        let mut frame = serde_json::to_vec(&json!({"v":if out.terminal {2}else{1},"sequence":sequence.to_string(),"source":if out.terminal {"terminal"}else if stderr {"stderr"}else{"stdout"},"receivedAt":crate::core::now(),"dataBase64":STANDARD.encode(bytes)})).unwrap();
                        frame.push(b'\n');
                        encoded.push(frame);
                    }
                }
                if !encoded.is_empty() {
                    let out = out.clone();
                    // The execution reader awaits this worker even when cancellation arrives.
                    tokio::task::spawn_blocking(move || {
                        let mut capture = out.capture.lock().unwrap();
                        for frame in encoded {
                            capture.append(&frame)?;
                        }
                        Ok::<_, String>(())
                    })
                    .await
                    .map_err(|e| e.to_string())??;
                }
                Ok(())
            }
            Self::Terminal(out) => out.push(bytes, cancel).await,
        }
    }
    pub fn finish(&self, terminal_result: Value) -> Result<(), String> {
        match self {
            Self::Memory(out) => out.lock().unwrap().decoder.finish(),
            Self::Capture(out) => out.decoder.lock().unwrap().finish(),
            Self::Terminal(out) => {
                out.finish(terminal_result);
                Ok(())
            }
        }
    }
}

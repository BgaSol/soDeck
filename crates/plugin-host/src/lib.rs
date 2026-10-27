//! One bounded QuickJS process for each plugin business instance.
mod engine;
pub mod dispatcher;
mod process;
mod transport;

pub use process::{BinaryCallbacks, BinarySink, CoreCallback, PluginProcess};
use serde::{Deserialize, Serialize};
use std::{
    io,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::Duration,
};

pub const INTERNAL_MODE: &str = "--sodeck-internal-plugin-host";

#[derive(Clone, Debug, Deserialize, Serialize, thiserror::Error)]
#[serde(rename_all = "camelCase")]
#[error("{message}")]
pub struct RpcError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub operation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub phase: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub details: Option<serde_json::Value>,
}

impl RpcError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            operation_id: None,
            phase: None,
            details: None,
        }
    }
}

pub fn run_internal_host_if_requested() -> Option<i32> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if !args.iter().any(|a| a == INTERNAL_MODE) {
        return None;
    }
    if args.len() != 1 {
        return Some(2);
    }
    let result = std::thread::Builder::new()
        .name("plugin-js".into())
        .stack_size(4 * 1024 * 1024)
        .spawn(run)
        .map_err(|e| e.to_string())
        .and_then(|t| t.join().map_err(|_| "Plugin thread panicked".into()))
        .and_then(|r| r);
    Some(match result {
        Ok(()) => 0,
        Err(message) => {
            eprintln!("Plugin host stopped: {message}");
            1
        }
    })
}

fn run() -> Result<(), String> {
    let disconnected = Arc::new(AtomicBool::new(false));
    let reader_closed = disconnected.clone();
    let (send, receive) = mpsc::sync_channel(32);
    std::thread::spawn(move || {
        let mut input = io::stdin().lock();
        loop {
            match transport::read_wire(&mut input) {
                Ok(Some(frame)) => {
                    if send.send(Ok(frame)).is_err() {
                        break;
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    let _ = send.send(Err(e));
                    break;
                }
            }
        }
        reader_closed.store(true, Ordering::SeqCst);
    });
    let mut engine = engine::Engine::new(disconnected.clone())?;
    let mut output = io::stdout().lock();
    loop {
        match receive.recv_timeout(Duration::from_millis(5)) {
            Ok(Ok(frame)) => match frame {transport::Wire::Json(v)=>engine.receive(v)?,transport::Wire::Binary(f)=>engine.receive_binary(f)?},
            Ok(Err(e)) => return Err(e.to_string()),
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        for _ in 0..3 {
            match receive.try_recv() {
                Ok(Ok(transport::Wire::Json(v))) => engine.receive(v)?,
                Ok(Ok(transport::Wire::Binary(f))) => engine.receive_binary(f)?,
                Ok(Err(e))=>return Err(e.to_string()),
                Err(_)=>break,
            }
        }
        engine.tick()?;
        for frame in engine.drain()? {
            transport::write_frame(&mut output, &frame).map_err(|e| e.to_string())?;
        }
        for frame in engine.drain_binary() { transport::write_binary(&mut output,&frame).map_err(|e|e.to_string())?; }
        if disconnected.load(Ordering::SeqCst) {
            break;
        }
    }
    Ok(())
}

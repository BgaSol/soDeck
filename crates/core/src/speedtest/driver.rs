use super::protocol::*;
use crate::{CoreError, Result};
use serde::{Serialize, Deserialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{io::{Read, Write}, sync::atomic::AtomicBool, time::{Duration, Instant}};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DirectionResult {
    pub direction: String,
    pub bytes: String,
    pub warmup_bytes: String,
    pub duration_ms: u64,
    pub mean_bytes_per_second: f64,
    pub process_peak_bytes_per_second: Option<f64>,
    pub sha256: String,
    pub verified: bool,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress { pub phase: u8, pub stage: String, pub bytes: String, pub elapsed_ms: u64 }

pub fn run<T: Read + Write>(stream: &mut T, bootstrap: &[u8], expected_method: &str,
    profile: &str, directions: &[String], canceled: &AtomicBool,
    mut progress: impl FnMut(Progress)) -> Result<Vec<DirectionResult>>
{
    let (limit, warm, formal) = match profile {
        "standard" => (256 * 1048576usize, 2, 10),
        "save-data" => (32 * 1048576usize, 1, 5),
        _ => return Err(CoreError::new("INVALID_PARAMS", "测速档位无效")),
    };
    if !matches!(directions.iter().map(String::as_str).collect::<Vec<_>>().as_slice(),
        ["upload"] | ["download"] | ["upload", "download"]) { return Err(protocol_error()); }
    let mut wire = Wire::new(stream, canceled);
    progress(Progress { phase: 0, stage: "bootstrap".into(), bytes: "0".into(), elapsed_ms: 0 });
    wire.bootstrap(bootstrap)?;
    progress(Progress { phase: 0, stage: "hello".into(), bytes: "0".into(), elapsed_ms: 0 });
    let hello = wire.receive(&mut |_| {})?;
    let hello_value = hello.json()?;
    if hello.kind != HELLO || hello.phase != 0 || hello_value["method"] != expected_method || hello_value["protocol"] != 1 {
        return Err(CoreError::new("UNSUPPORTED", "服务器内存测速方法不受支持"));
    }
    wire.send_json(CONFIG, 0, &json!({"profile":profile,"directions":directions}))?;
    progress(Progress { phase: 0, stage: "ready".into(), bytes: "0".into(), elapsed_ms: 0 });
    let ready = wire.receive(&mut |_| {})?;
    if ready.kind != READY || ready.phase != 0 || ready.json()?["ready"] != true { return Err(protocol_error()); }
    let mut results = Vec::new();
    for direction in directions {
        let mut remaining = limit;
        let mut warmup = 0usize;
        for formal_phase in [false, true] {
            let phase = (if direction == "upload" { 1 } else { 3 }) + u8::from(formal_phase);
            let seconds = if formal_phase { formal } else { warm };
            let phase_limit = if formal_phase { remaining } else { limit / 2 };
            let mut digest = Sha256::new();
            let mut total = 0usize;
            let mut process_bytes = 0usize;
            let started = Instant::now();
            let mut last_progress = started;
            let mut bucket = started;
            let mut bucket_bytes = 0usize;
            let mut peak: Option<f64> = None;
            let mut sampled = |bytes: usize| {
                process_bytes += bytes;
                bucket_bytes += bytes;
                let now = Instant::now();
                if now.duration_since(bucket) >= Duration::from_secs(1) {
                    let rate = bucket_bytes as f64 / now.duration_since(bucket).as_secs_f64();
                    peak = Some(peak.map_or(rate, |p| p.max(rate)));
                    bucket = now; bucket_bytes = 0;
                }
                if now.duration_since(last_progress) >= Duration::from_millis(100) {
                    progress(Progress { phase, stage: "transferring".into(), bytes: process_bytes.to_string(), elapsed_ms: started.elapsed().as_millis() as u64 });
                    last_progress = now;
                }
            };
            wire.send_json(BEGIN, phase, &json!({}))?;
            let measured_start;
            let receipt;
            if direction == "upload" {
                let mut block = vec![0u8; 16 * 1024];
                measured_start = Instant::now();
                while total < phase_limit && measured_start.elapsed() < Duration::from_secs(seconds) {
                    let count = block.len().min(phase_limit - total);
                    getrandom::fill(&mut block[..count]).map_err(|_| CoreError::new("UNAVAILABLE", "无法生成测速数据"))?;
                    wire.send(DATA, phase, &block[..count], &mut sampled)?;
                    digest.update(&block[..count]); total += count;
                }
                let hash = format!("{:x}", digest.clone().finalize());
                wire.send_json(END, phase, &json!({"bytes":total.to_string(),"sha256":hash}))?;
                loop {
                    let frame = wire.receive(&mut |_| {})?;
                    if frame.phase != phase { return Err(protocol_error()); }
                    if frame.kind == RESULT { receipt = frame.json()?; break; }
                    if frame.kind != ACK || frame.json()?["bytes"].as_str().and_then(|v| v.parse::<usize>().ok()).is_none_or(|v| v > total) {
                        return Err(protocol_error());
                    }
                }
            } else {
                measured_start = started;
                loop {
                    let frame = wire.receive(&mut sampled)?;
                    if frame.phase != phase { return Err(protocol_error()); }
                    if frame.kind == RESULT { receipt = frame.json()?; break; }
                    if frame.kind != DATA || frame.payload.is_empty() || frame.payload.len() > phase_limit - total { return Err(protocol_error()); }
                    digest.update(&frame.payload); total += frame.payload.len();
                }
            }
            let hash = format!("{:x}", digest.finalize());
            if receipt["verified"] != true || receipt["bytes"] != total.to_string() || receipt["sha256"] != hash {
                return Err(CoreError::new("VERIFY_FAILED", "测速数据校验不一致"));
            }
            let elapsed = measured_start.elapsed();
            if direction == "download" { wire.send_json(ACK, phase, &json!({"bytes":total.to_string(),"sha256":hash}))?; }
            progress(Progress { phase, stage: "verified".into(), bytes: total.to_string(), elapsed_ms: elapsed.as_millis() as u64 });
            remaining -= total;
            if formal_phase {
                results.push(DirectionResult { direction: direction.clone(), bytes: total.to_string(), warmup_bytes: warmup.to_string(),
                    duration_ms: elapsed.as_millis() as u64, mean_bytes_per_second: total as f64 / elapsed.as_secs_f64().max(0.000001),
                    process_peak_bytes_per_second: peak, sha256: hash, verified: true });
            } else { warmup = total; }
        }
    }
    let receipt = wire.receive(&mut |_| {})?;
    if receipt.kind != RESULT || receipt.phase != 0 || receipt.json()?["complete"] != true {
        return Err(CoreError::new("RESULT_PARTIAL", "测速结束回执缺失"));
    }
    Ok(results)
}

//! Core result records. Plugin-defined payloads remain JSON at their explicit boundaries.
use crate::{program::Mode, Context};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Resource {
    pub id: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenView {
    pub view_id: String,
    #[serde(default)]
    pub closed: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Program {
    pub command: String,
    pub mode: Mode,
    pub length: usize,
    pub program_output: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Target {
    pub name: String,
    pub protocol: String,
    pub host: String,
    pub port: u16,
    pub endpoint: String,
    pub username: Option<String>,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ConnectionState {
    Connected,
    Disconnected,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum TargetSource {
    Current,
    LastConnected,
    Unknown,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetFacts {
    pub context: Context,
    pub state: ConnectionState,
    pub target: Option<Target>,
    pub target_source: TargetSource,
    pub closing: bool,
    pub capabilities: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecReceipt {
    pub task_id: String,
    pub state: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputPage {
    pub data_base64: String,
    pub next_offset: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecPoll {
    pub state: String,
    pub stdout: OutputPage,
    pub stderr: OutputPage,
    pub output_complete: bool,
    pub exit_code: Option<u32>,
    pub exit_signal: Option<String>,
    pub error: Option<ExecutionError>,
    pub remote_exit_confirmed: bool,
    pub exec_sent: bool,
    pub elapsed_ms: u64,
    pub output_limit_exceeded: bool,
    #[serde(flatten)]
    pub startup: Startup,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecutionError {
    pub code: String,
    pub message: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Startup {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program_started: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check_failure: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program_error: Option<Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessReceipt {
    pub process_id: String,
    pub status: ProcessStatus,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessLookup {
    pub known: bool,
    pub process_id: Option<String>,
    pub status: Option<ProcessStatus>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessRead {
    pub chunks: Vec<OutputChunk>,
    pub next_offset: String,
    pub watermark: String,
    pub earliest_available_offset: String,
    pub status: ProcessStatus,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessRelease {
    pub release_requested: bool,
    pub resource_released: bool,
    pub status: ProcessStatus,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessStatus {
    pub state: String,
    pub phase: String,
    pub exec_sent: bool,
    pub launch_queued: bool,
    pub launch_accepted: bool,
    pub remote_exit_confirmed: bool,
    pub exit_code: Option<u32>,
    pub exit_signal: Option<String>,
    pub error: Option<ExecutionError>,
    pub finished: bool,
    pub worker_finished: bool,
    pub output_complete: bool,
    pub elapsed_ms: u64,
    pub input_closed: bool,
    pub input: ProcessInput,
    pub produced_bytes: String,
    pub earliest_available_offset: String,
    #[serde(flatten)]
    pub startup: Startup,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessInput {
    pub state: String,
    pub available: bool,
    pub next_input_sequence: String,
    pub last: Option<InputReceipt>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InputReceipt {
    pub sequence: String,
    pub action: String,
    pub state: String,
    pub sent_bytes: Option<usize>,
    pub error: Option<ExecutionError>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputChunk {
    pub v: u8,
    pub sequence: String,
    pub source: String,
    pub received_at: u64,
    pub data_base64: String,
}

pub fn decode<T: for<'de> Deserialize<'de>>(value: Value) -> Result<T, crate::ApiError> {
    serde_json::from_value(value).map_err(|cause| {
        let mut error = crate::ApiError::local("核心回复不符合当前协议；原请求结果待确认");
        error.data["protocolError"] = Value::String(cause.to_string());
        error
    })
}

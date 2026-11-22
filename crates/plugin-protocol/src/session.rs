use super::Context;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum StdinPolicy {
    Closed,
    Open,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum ExecutionIo {
    Pipe { stdin: StdinPolicy },
    Pty { cols: u32, rows: u32 },
}
impl Default for ExecutionIo {
    fn default() -> Self {
        Self::Pipe {
            stdin: StdinPolicy::Closed,
        }
    }
}
impl ExecutionIo {
    pub fn terminal(&self) -> bool {
        matches!(self, Self::Pty { .. })
    }
    pub fn interactive(&self) -> bool {
        !matches!(
            self,
            Self::Pipe {
                stdin: StdinPolicy::Closed
            }
        )
    }
    pub fn validate(&self) -> Result<(), String> {
        if let Self::Pty { cols, rows } = self {
            if !(2..=1000).contains(cols) || !(1..=500).contains(rows) {
                return Err("invalidParams:终端尺寸超出范围".into());
            }
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessStart {
    pub view_id: String,
    pub expected_context: Context,
    pub operation_id: String,
    pub sequence: String,
    pub command: String,
    pub max_output_bytes: usize,
    pub retention: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program_output: Option<String>,
    #[serde(default)]
    pub io: ExecutionIo,
}

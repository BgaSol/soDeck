use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const ROW_LIMIT: usize = 50_000;
pub const READING_BYTES: usize = 32 * 1024 * 1024;
pub const RECENT_ROWS: usize = 10_000;
pub const RECENT_BYTES: usize = 16 * 1024 * 1024;
pub const TOTAL_BYTES: usize = 128 * 1024 * 1024;
pub const RECORD_BYTES: usize = 1024 * 1024;
pub const PAGE_BYTES: usize = 20 * 1024;
pub const SCAN_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Clone, Default, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Conditions {
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub levels: Vec<String>,
    #[serde(default)]
    pub sources: Vec<String>,
    #[serde(default)]
    pub only: bool,
}
impl Conditions {
    pub fn validate(&self) -> Result<(), String> {
        if self.query.chars().count() > 1024
            || self.levels.len() > 6
            || self.sources.len() > 2
            || self.levels.iter().any(|s| {
                ![
                    "TRACE", "DEBUG", "INFO", "WARN", "ERROR", "FATAL", "unknown",
                ]
                .contains(&s.as_str())
            })
            || self
                .sources
                .iter()
                .any(|s| !["stdout", "stderr", "combined"].contains(&s.as_str()))
        {
            return Err("日志查找条件无效".into());
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Range {
    Latest,
    Recent { seconds: u64 },
    Startup,
    Custom { from: u64, to: u64 },
    At { at: u64 },
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "action", deny_unknown_fields)]
pub enum Command {
    #[serde(rename = "logs.open")]
    Open {
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(rename = "intentSequence")]
        intent_sequence: String,
        range: Range,
    },
    #[serde(rename = "logs.earlier")]
    Earlier {
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(rename = "intentSequence")]
        intent_sequence: String,
    },
    #[serde(rename = "logs.later")]
    Later {
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(rename = "intentSequence")]
        intent_sequence: String,
    },
    #[serde(rename = "logs.search")]
    Search {
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(rename = "intentSequence")]
        intent_sequence: String,
        range: Range,
        conditions: Conditions,
        #[serde(default)]
        resume: bool,
    },
    #[serde(rename = "logs.hold")]
    Hold {
        #[serde(default)]
        revision: Option<String>,
    },
    #[serde(rename = "logs.latest")]
    Latest,
    #[serde(rename = "logs.previous")]
    Previous,
    #[serde(rename = "logs.query")]
    Query { conditions: Conditions },
    #[serde(rename = "logs.stop")]
    Stop {
        #[serde(default)]
        all: bool,
    },
    #[serde(rename = "logs.context")]
    Context { record: String },
    #[serde(rename = "logs.navigation")]
    Navigation { state: Value },
    #[serde(rename = "logs.export.begin")]
    ExportBegin {
        #[serde(default)]
        record: Option<String>,
        #[serde(default)]
        context: bool,
        header: String,
    },
    #[serde(rename = "logs.export.end")]
    ExportEnd { token: String },
}
#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", deny_unknown_fields)]
pub enum Read {
    Status,
    Window {
        revision: String,
        #[serde(default)]
        start: usize,
        #[serde(default = "page_count")]
        count: usize,
    },
    Layout {
        revision: String,
        #[serde(default)]
        start: usize,
    },
    Detail {
        record: String,
        #[serde(default)]
        offset: usize,
    },
    Navigate {
        #[serde(default)]
        record: Option<String>,
        direction: i8,
    },
    LocateTime {
        at: String,
    },
    Export {
        token: String,
        offset: usize,
    },
}
fn page_count() -> usize {
    100
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    Latest,
    History,
    Search,
}

use crate::model::AppError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Instant;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    #[serde(skip)]
    pub captured_at: i64,
    pub level: &'static str,
    pub module: String,
    pub event: String,
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub source: String,
    pub details: Value,
}

pub fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}
pub fn timestamp(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|v| v.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .unwrap_or_default()
}
pub fn source(file: &str, line: u32) -> String {
    let file = file.replace('\\', "/");
    let relative = file
        .find("src-tauri/src/")
        .map(|p| &file[p..])
        .or_else(|| file.strip_prefix("src/").map(|_| file.as_str()));
    relative
        .map(|v| format!("{v}:{line}"))
        .unwrap_or_else(|| "external".into())
}
pub fn error_details(error: &AppError) -> Value {
    json!({"errorCode": error.diagnostic.code, "nativeCode": error.diagnostic.native_code,
        "nativeDomain": error.diagnostic.domain, "nativeKind": error.diagnostic.native_kind, "errorSource": error.diagnostic.source,
        "hop":error.hop.map(|hop| u16::from(hop) + 1), "detailOmitted":true})
}

/// Only fields explicitly produced by the application may cross this boundary.
/// Free-form exception text, command arguments and business objects are never recorded.
pub fn safe_details(value: Value) -> Value {
    let Some(input) = value.as_object() else {
        return json!({});
    };
    let mut out = serde_json::Map::new();
    for (key, value) in input {
        let allowed_number = matches!(
            key.as_str(),
            "durationMs"
                | "timeoutMs"
                | "attempt"
                | "hop"
                | "hops"
                | "resolvedCount"
                | "candidate"
                | "exitCode"
                | "bytes"
                | "count"
                | "dropped"
                | "generation"
                | "nativeCode"
                | "frontendSeq"
                | "frontendMs"
                | "line"
                | "column"
                | "completed"
                | "total"
        );
        let allowed_bool = matches!(
            key.as_str(),
            "detailOmitted"
                | "cancelled"
                | "retryPlanned"
                | "uncertain"
                | "truncated"
                | "developerMode"
                | "automatic"
                | "success"
        );
        if (allowed_number && value.is_number()) || (allowed_bool && value.is_boolean()) {
            out.insert(key.clone(), value.clone());
        } else if matches!(
            key.as_str(),
            "outcome"
                | "phase"
                | "method"
                | "family"
                | "addressKind"
                | "reason"
                | "nativeDomain"
                | "nativeKind"
                | "errorCode"
                | "command"
                | "kind"
        ) {
            if let Some(v) = value.as_str().filter(|v| safe_token(v)) {
                out.insert(key.clone(), json!(v));
            }
        } else if matches!(
            key.as_str(),
            "windowRef" | "taskRef" | "connectionRef" | "targetRef" | "parentTraceId"
        ) {
            if let Some(v) = value.as_str().filter(|v| {
                uuid::Uuid::parse_str(v).is_ok()
                    || (v.starts_with("ref-")
                        && v.len() == 28
                        && v[4..].bytes().all(|c| c.is_ascii_hexdigit()))
            }) {
                out.insert(key.clone(), json!(v));
            }
        } else if matches!(key.as_str(), "currentVersion" | "targetVersion") {
            if let Some(v) = value.as_str().filter(|v| {
                v.len() < 32
                    && semver::Version::parse(v)
                        .is_ok_and(|v| v.pre.is_empty() && v.build.is_empty())
            }) {
                out.insert(key.clone(), json!(v));
            }
        } else if key == "updateTarget" {
            if let Some(v) = value.as_str().filter(|v| {
                matches!(
                    *v,
                    "windows-x86_64-nsis" | "darwin-aarch64" | "linux-x86_64"
                )
            }) {
                out.insert(key.clone(), json!(v));
            }
        } else if key == "errorSource" {
            if let Some(v) = value
                .as_str()
                .filter(|v| v.starts_with("src/") || v.starts_with("src-tauri/src/"))
            {
                if v.len() < 160
                    && !v.contains("..")
                    && v.bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"/_-.:".contains(&c))
                {
                    out.insert(key.clone(), json!(v));
                }
            }
        }
    }
    Value::Object(out)
}
pub fn safe_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && value
            .bytes()
            .all(|v| v.is_ascii_alphanumeric() || b"_.-".contains(&v))
}

#[derive(Clone, Debug)]
pub struct Context {
    pub trace_id: String,
    pub span_id: String,
}
impl Context {
    pub fn new() -> Self {
        Self {
            trace_id: id(),
            span_id: id(),
        }
    }
    pub fn from_trace(trace: Option<&str>) -> Self {
        Self {
            trace_id: trace
                .filter(|v| uuid::Uuid::parse_str(v).is_ok())
                .map(str::to_owned)
                .unwrap_or_else(id),
            span_id: id(),
        }
    }
    #[track_caller]
    pub fn event(
        &self,
        level: &'static str,
        module: &'static str,
        name: &'static str,
        details: Value,
    ) {
        let at = std::panic::Location::caller();
        super::record(Event {
            captured_at: now(),
            level,
            module: module.into(),
            event: name.into(),
            trace_id: self.trace_id.clone(),
            span_id: self.span_id.clone(),
            parent_span_id: None,
            source: source(at.file(), at.line()),
            details: safe_details(details),
        });
    }
    #[track_caller]
    pub fn stage(&self, module: &'static str, name: &'static str, details: Value) -> Stage {
        let context = Self {
            trace_id: self.trace_id.clone(),
            span_id: id(),
        };
        let at = std::panic::Location::caller();
        let stage = Stage {
            context,
            parent: self.span_id.clone(),
            module,
            name,
            started: Instant::now(),
            source: source(at.file(), at.line()),
            finished: false,
        };
        stage.emit("DEBUG", "start", details);
        stage
    }
}
pub struct Stage {
    pub context: Context,
    parent: String,
    module: &'static str,
    name: &'static str,
    started: Instant,
    source: String,
    finished: bool,
}
impl Stage {
    fn emit(&self, level: &'static str, phase: &str, details: Value) {
        let mut details = safe_details(details);
        details["phase"] = json!(phase);
        details["durationMs"] = json!(self.started.elapsed().as_millis() as u64);
        super::record(Event {
            captured_at: now(),
            level,
            module: self.module.into(),
            event: self.name.into(),
            trace_id: self.context.trace_id.clone(),
            span_id: self.context.span_id.clone(),
            parent_span_id: Some(self.parent.clone()),
            source: self.source.clone(),
            details,
        });
    }
    pub fn finish<T>(&mut self, result: &Result<T, AppError>) {
        let mut details = result
            .as_ref()
            .err()
            .map(error_details)
            .unwrap_or_else(|| json!({}));
        details["outcome"] = json!(if result.is_ok() { "ok" } else { "failed" });
        self.emit(
            if result.is_ok() { "DEBUG" } else { "WARN" },
            "end",
            details,
        );
        self.finished = true;
    }
}
impl Drop for Stage {
    fn drop(&mut self) {
        if !self.finished {
            self.emit(
                "WARN",
                "end",
                json!({"outcome":"interrupted","detailOmitted":true}),
            );
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorInfo {
    pub code: String,
    pub native_code: Option<i64>,
    pub domain: Option<&'static str>,
    pub native_kind: Option<String>,
    pub source: String,
}
impl ErrorInfo {
    #[track_caller]
    pub fn new(kind: &str) -> Self {
        let at = std::panic::Location::caller();
        // kind comes from application error constructors, never from the error message.
        Self {
            code: if safe_token(kind) {
                kind.to_owned()
            } else {
                "unknown".into()
            },
            source: source(at.file(), at.line()),
            ..Default::default()
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FrontendEvent {
    pub event: String,
    pub trace_id: Option<String>,
    pub command: Option<String>,
    pub outcome: Option<String>,
    pub duration_ms: Option<f64>,
    pub frontend_seq: Option<u64>,
    pub frontend_ms: Option<f64>,
    pub error_code: Option<String>,
    pub native_code: Option<i64>,
    pub subject: Option<String>,
    pub stack: Option<String>,
    pub dropped: Option<u64>,
}

/// Keep only bundled asset frame positions; arbitrary exception messages/function names are discarded.
pub fn stack_frames(stack: &str) -> Vec<String> {
    stack
        .lines()
        .take(80)
        .filter_map(|line| {
            let start = line.find("/assets/")?;
            let frame = line[start + 1..].split([')', ' ', '?', '#']).next()?;
            (frame.len() < 180
                && frame
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"/_-.:".contains(&c))
                && frame.contains(".js:"))
            .then(|| frame.to_owned())
        })
        .take(24)
        .collect()
}

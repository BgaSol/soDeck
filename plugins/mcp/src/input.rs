//! Ordered input for an existing command task. RPC loss retains the same attempt.
use crate::{
    app::App,
    task::{CapturedTask, Task, TaskFailure},
    tasks::failure,
};
use serde_json::{json, Value};
use sodeck_plugin_client::encoding::{base64, unbase64};
use sodeck_plugin_client::protocol::requests as api;
use std::sync::Arc;
use tokio::sync::OwnedSemaphorePermit;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum NormalizedInput {
    Write(Arc<[u8]>),
    Eof,
    Resize(u32, u32),
}
impl NormalizedInput {
    pub fn action(&self) -> &'static str {
        match self {
            Self::Write(_) => "write",
            Self::Eof => "eof",
            Self::Resize(..) => "resize",
        }
    }
    pub fn request(&self, process: &str, sequence: u64) -> Value {
        let mut value = json!({"processId":process,"inputSequence":sequence.to_string(),"action":self.action()});
        match self {
            Self::Write(data) => value["dataBase64"] = json!(base64(data)),
            Self::Resize(cols, rows) => {
                value["cols"] = json!(cols);
                value["rows"] = json!(rows);
            }
            Self::Eof => {}
        }
        value
    }
}
#[derive(Clone, Default)]
pub(crate) struct CommandControl {
    pub process_id: Option<String>,
    pub last_accepted: Option<(u64, NormalizedInput)>,
    pub attempt: Option<InputAttempt>,
    pub sealed: bool,
}
#[derive(Clone)]
pub(crate) struct InputAttempt {
    pub sequence: u64,
    pub operation: NormalizedInput,
    pub rpc_in_flight: bool,
    pub admission_unknown: bool,
    pub error: Value,
}
pub(crate) struct InputCallGuard {
    pub task: Arc<Task>,
    pub sequence: u64,
    pub _permit: OwnedSemaphorePermit,
}
impl Drop for InputCallGuard {
    fn drop(&mut self) {
        self.task.input_finished(self.sequence);
    }
}
pub(crate) enum Dispatch {
    Existing(CapturedTask),
    Send {
        process: String,
        guard: InputCallGuard,
    },
}
pub(crate) struct Request {
    pub task_id: String,
    pub sequence: u64,
    pub operation: NormalizedInput,
}
pub(crate) fn parse(p: &Value) -> Result<Request, Value> {
    let task_id = crate::tasks::text(p, "taskId", 256)?.to_owned();
    let raw = crate::tasks::text(p, "sequence", 20)?;
    if !raw.bytes().all(|b| b.is_ascii_digit()) {
        return Err(failure("invalidSequence", "sequence 必须是十进制字符串"));
    }
    let sequence = raw
        .parse::<u64>()
        .ok()
        .filter(|n| *n > 0)
        .ok_or_else(|| failure("invalidSequence", "输入序号从 1 开始"))?;
    let action = crate::tasks::text(p, "action", 16)?;
    let fields: &[&str] = match action {
        "write" => &["taskId", "sequence", "action", "text", "dataBase64"],
        "eof" => &["taskId", "sequence", "action"],
        "resize" => &["taskId", "sequence", "action", "cols", "rows"],
        _ => return Err(failure("invalidParams", "action 无效")),
    };
    if !p
        .as_object()
        .is_some_and(|v| v.keys().all(|k| fields.contains(&k.as_str())))
    {
        return Err(failure("invalidParams", "输入包含不适用参数"));
    }
    let operation = match action {
        "write" => {
            let data = match (p.get("text"), p.get("dataBase64")) {
                (Some(text), None) => text
                    .as_str()
                    .ok_or_else(|| failure("invalidParams", "text 必须是文字"))?
                    .as_bytes()
                    .to_vec(),
                (None, Some(data)) => {
                    let data = data
                        .as_str()
                        .filter(|s| s.len() <= 24 * 1024)
                        .ok_or_else(|| failure("invalidParams", "输入编码无效或超限"))?;
                    unbase64(data).map_err(|_| failure("invalidParams", "Base64 输入无效"))?
                }
                _ => return Err(failure("invalidParams", "text 和 dataBase64 必须二选一")),
            };
            if data.len() > 16 * 1024 {
                return Err(failure("invalidParams", "输入超过 16 KiB"));
            }
            NormalizedInput::Write(data.into())
        }
        "eof" => NormalizedInput::Eof,
        _ => {
            let cols = p["cols"]
                .as_u64()
                .filter(|n| (2..=1000).contains(n))
                .ok_or_else(|| failure("invalidParams", "cols 超出范围"))?;
            let rows = p["rows"]
                .as_u64()
                .filter(|n| (1..=500).contains(n))
                .ok_or_else(|| failure("invalidParams", "rows 超出范围"))?;
            NormalizedInput::Resize(cols as u32, rows as u32)
        }
    };
    Ok(Request {
        task_id,
        sequence,
        operation,
    })
}
pub async fn submit(
    app: Arc<App>,
    p: &Value,
    cancel: CancellationToken,
) -> Result<CapturedTask, TaskFailure> {
    let request = parse(p)?;
    let task = app.resolve(&json!({"taskId":request.task_id}), None)?;
    if cancel.is_cancelled() {
        return Err(failure("requestCancelled", "输入调用已取消").into());
    }
    let dispatch = task
        .input_dispatch(request.sequence, request.operation.clone(), &app.io)
        .map_err(|error| TaskFailure {
            error,
            captured: Some(CapturedTask::new(task.clone())),
        })?;
    let Dispatch::Send { process, guard } = dispatch else {
        if let Dispatch::Existing(captured) = dispatch {
            return Ok(captured);
        }
        unreachable!()
    };
    let params = request.operation.request(&process, request.sequence);
    let sequence = request.sequence;
    app.runtime
        .spawn_blocking({
            let app = app.clone();
            move || {
                let result = serde_json::from_value::<api::ProcessInput>(params)
                    .map_err(|_| failure("invalidParams", "核心请求参数无效"))
                    .and_then(|params| app.rpc(api::call::ProcessInput(params)));
                let error = match result {
                    Ok(native) => {
                        let valid = native["input"]["last"]["sequence"]
                            .as_str()
                            .and_then(|s| s.parse::<u64>().ok())
                            == Some(sequence)
                            && native["input"]["last"]["action"] == request.operation.action();
                        if valid {
                            task.observe(native, None);
                            None
                        } else {
                            let e = failure("inputUnknown", "核心输入回包缺少原序号或动作");
                            task.input_error(sequence, e.clone(), true);
                            Some(e)
                        }
                    }
                    Err(e) => {
                        let code = e["details"]["code"].as_str().unwrap_or("");
                        let rejected = e["code"] == -32602
                            || [
                                "invalidParams",
                                "invalidSequence",
                                "inputExpired",
                                "inputConflict",
                                "inputClosed",
                                "inputNotReady",
                                "unsupported",
                                "resourceBusy",
                                "staleContext",
                                "processNotFound",
                            ]
                            .contains(&code);
                        task.input_error(sequence, e.clone(), !rejected);
                        Some(e)
                    }
                };
                drop(guard);
                let captured = CapturedTask::new(task);
                match error {
                    Some(error) => Err(TaskFailure {
                        error,
                        captured: Some(captured),
                    }),
                    None => Ok(captured),
                }
            }
        })
        .await
        .map_err(|e| TaskFailure::from(failure("inputUnknown", e.to_string())))?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn input_preserves_bytes_and_validates_action() {
        let text = parse(&json!({"taskId":"task","sequence":"1","action":"write","text":"中\0\r"}))
            .unwrap();
        let raw=parse(&json!({"taskId":"task","sequence":"1","action":"write","dataBase64":base64("中\0\r".as_bytes())})).unwrap();
        assert_eq!(text.operation, raw.operation);
        for p in [
            json!({"taskId":"task","sequence":"0","action":"eof"}),
            json!({"taskId":"task","sequence":"1","action":"eof","text":"x"}),
            json!({"taskId":"task","sequence":"1","action":"write","text":"x","dataBase64":"eA=="}),
            json!({"taskId":"task","sequence":"1","action":"resize","cols":1,"rows":30}),
            json!({"taskId":"task","sequence":"1","action":"barrier"}),
        ] {
            assert!(parse(&p).is_err());
        }
    }
}

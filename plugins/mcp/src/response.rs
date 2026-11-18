use crate::{
    evidence::{Page, PageBody},
    query::{TaskFailure, TaskList, TaskRead},
    task::{preview, CapturedTask, Kind, Phase, Record, Saved, Task},
};
use rmcp::model::{CallToolResult, ContentBlock};
use serde_json::{json, Value};
use std::io::{self, Write};

fn brief_error(value: &Value) -> Value {
    if value.is_null() {
        return Value::Null;
    }
    let mut result = json!({"code":value["code"],"message":value["message"]});
    if let Some(message) = value["message"].as_str() {
        let (text, cut) = preview(message, 512);
        result["message"] = json!(text);
        if cut {
            result["messageTruncated"] = json!(true);
        }
    }
    if value
        .as_object()
        .is_some_and(|o| o.keys().any(|k| k != "code" && k != "message"))
    {
        result["detailsAvailable"] = json!(true);
    }
    result
}
fn native_facts(native: &Value, history: bool) -> Value {
    let mut value = json!({});
    for key in [
        "state",
        "admission",
        "execSent",
        "exitCode",
        "exitSignal",
        "remoteExitConfirmed",
        "outputComplete",
        "received",
        "total",
    ] {
        if let Some(v) = native.get(key) {
            value[key] = v.clone();
        }
    }
    if native.get("error").is_some_and(|e| !e.is_null()) {
        value["error"] = brief_error(&native["error"]);
    }
    if let Some(last) = native.get("lastResult") {
        if history {
            value["lastResult"] = native_facts(last, false);
        } else {
            value["lastResultDetailsAvailable"] = json!(true);
        }
    }
    value
}
fn identity(task: &Task, r: &Record) -> Value {
    json!({"taskId":task.id,"runId":task.run,"kind":task.kind.name(),"targetId":task.target.id,
        "requestId":task.input["requestId"],"target":r.target,"done":r.phase==Phase::Done,
        "phase":match r.phase {Phase::Collecting=>"collecting",Phase::Finalizing=>"finalizing",Phase::Done=>"done"},
        "revision":r.revision.to_string(),"cancelRequested":r.cancel_requested,"resourceReleased":r.released})
}
fn evidence(r: &Record, detail: bool) -> Value {
    let mut value = json!({"state":match r.saved {Saved::Collecting=>"collecting",Saved::Finalizing=>"finalizing",Saved::Complete=>"saved",Saved::Incomplete=>"incomplete",Saved::Failed=>"failed"},"complete":matches!(r.saved,Saved::Complete)});
    if detail || !r.error.is_null() {
        value["error"] = if detail {
            r.error.clone()
        } else {
            brief_error(&r.error)
        };
    }
    if detail {
        value["path"] = json!(r.path);
    }
    value
}
fn diagnostic(task: &Task, r: &Record) -> Value {
    let mut value = identity(task, r);
    value["native"] = r.native.clone();
    value["evidence"] = evidence(r, true);
    value["cleanupError"] = r.cleanup_error.clone();
    value["createdAt"] = json!(task.created);
    value["finishedAt"] = json!(r.finished);
    value["observedAt"] = json!(r.observed);
    value["input"] = task.input.clone();
    value["directory"] = json!(task.directory);
    value
}
fn ordinary(task: &Task, r: &Record) -> Value {
    let mut value = identity(task, r);
    value["native"] = native_facts(&r.native, true);
    value["evidence"] = evidence(r, false);
    if !r.cleanup_error.is_null() {
        value["cleanupError"] = brief_error(&r.cleanup_error);
    }
    value
}
fn output(page: &Page) -> Value {
    let mut value = json!({"nextCursor":page.next_cursor.to_string(),"savedBytes":page.saved_bytes.to_string(),"hasMore":page.has_more});
    match &page.body {
        PageBody::Text { stdout, stderr } => {
            value["stdout"] = json!(stdout);
            value["stderr"] = json!(stderr);
            value["previewEncoding"] = json!("utf-8-lossy-per-page");
        }
        PageBody::Full { chunks } => {
            value["chunks"] = json!(chunks);
        }
    }
    value
}
fn mcp_facts(c: &CapturedTask, detail: bool, body: bool) -> Value {
    let (task, r) = (&c.task, &c.record);
    let mut value = if detail {
        diagnostic(task, r)
    } else {
        ordinary(task, r)
    };
    if detail {
        value["context"] = task.target.context.clone();
    }
    if task.kind == Kind::Download {
        value["file"] = json!({"remotePath":task.input["remotePath"],"localPath":if r.phase==Phase::Done && matches!(r.saved,Saved::Complete) {json!(r.path)} else {Value::Null}});
    } else if !body {
        value["outputInfo"] = json!({"savedBytes":r.committed.to_string()});
    }
    value
}
pub fn mcp_task(read: &TaskRead, detail: bool) -> Value {
    let mut value = mcp_facts(&read.captured, detail, read.page.is_some());
    if let Some(page) = &read.page {
        value["output"] = output(page);
    }
    if let Some(wait) = &read.wait {
        value["wait"] = json!(wait);
    }
    value
}
pub fn mcp_row(c: &CapturedTask) -> Value {
    let mut value = ordinary(&c.task, &c.record);
    row_fields(&mut value, c);
    value["serverId"] = c.task.target.context["serverId"].clone();
    value
}
fn row_fields(value: &mut Value, c: &CapturedTask) {
    let text = c.task.input[if c.task.kind == Kind::Exec {
        "command"
    } else {
        "remotePath"
    }]
    .as_str()
    .unwrap();
    let (text, cut) = preview(text, 256);
    value["inputPreview"] = json!(text);
    value["inputTruncated"] = json!(cut);
    value["createdAt"] = json!(c.task.created);
    value["finishedAt"] = json!(c.record.finished);
}
pub fn mcp_list(list: TaskList) -> Value {
    json!({"runId":list.run,"tasks":list.tasks.iter().map(mcp_row).collect::<Vec<_>>(),"counts":{"active":list.active,"done":list.done},"hasMore":list.has_more,"nextCursor":list.next_cursor})
}
pub fn mcp_cancel(c: &CapturedTask) -> Value {
    json!({"taskId":c.task.id,"cancelRequested":c.record.cancel_requested,"result":mcp_facts(c,false,false)})
}
pub fn ui_task(read: &TaskRead) -> Value {
    let mut value = diagnostic(&read.captured.task, &read.captured.record);
    value["outputInfo"] = json!({"savedBytes":if read.captured.task.kind==Kind::Exec {json!(read.captured.record.committed.to_string())} else {Value::Null}});
    if let Some(page) = &read.page {
        let mut body = output(page);
        if let PageBody::Full { chunks } = &page.body {
            let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
            for chunk in chunks {
                let bytes =
                    sodeck_plugin_client::encoding::unbase64(chunk["dataBase64"].as_str().unwrap())
                        .expect("reader validated chunk");
                if chunk["source"] == "stderr" {
                    stderr.extend(bytes);
                } else {
                    stdout.extend(bytes);
                }
            }
            body["stdout"] = json!(String::from_utf8_lossy(&stdout));
            body["stderr"] = json!(String::from_utf8_lossy(&stderr));
            body["previewEncoding"] = json!("utf-8-lossy-per-page");
        }
        value["output"] = body;
    }
    value
}
pub fn ui_row(c: &CapturedTask) -> Value {
    let mut value = identity(&c.task, &c.record);
    let mut native = json!({});
    for key in [
        "state",
        "admission",
        "execSent",
        "exitCode",
        "exitSignal",
        "remoteExitConfirmed",
        "workerFinished",
        "outputComplete",
        "received",
        "total",
        "streamId",
        "id",
        "error",
    ] {
        if let Some(field) = c.record.native.get(key) {
            native[key] = field.clone();
        }
    }
    if native.get("error").is_some() {
        native["error"] = brief_error(&native["error"]);
    }
    value["native"] = native;
    let mut saved = evidence(&c.record, false);
    saved["error"] = brief_error(&c.record.error);
    value["evidence"] = saved;
    value["cleanupError"] = brief_error(&c.record.cleanup_error);
    value["observedAt"] = json!(c.record.observed);
    value["context"] = c.task.target.context.clone();
    row_fields(&mut value, c);
    value
}
pub fn ui_cancel(c: &CapturedTask) -> Value {
    json!({"taskId":c.task.id,"cancelRequested":c.record.cancel_requested,"result":ui_row(c)})
}
pub fn metadata(task: &Task, r: &Record) -> Value {
    let mut value = diagnostic(task, r);
    value["schemaVersion"] = json!(1);
    value["context"] = task.target.context.clone();
    value["outputInfo"] = json!({"savedBytes":if task.kind==Kind::Exec {json!(r.committed.to_string())} else {Value::Null}});
    value
}
pub fn mcp_task_error(failure: &TaskFailure, detail: bool) -> Value {
    let mut value = json!({"error":if detail || failure.captured.is_none() {failure.error.clone()} else {brief_error(&failure.error)}});
    if let Some(c) = &failure.captured {
        value["task"] = mcp_facts(c, detail, false);
    }
    value
}
pub fn ui_read_error(failure: TaskFailure) -> Value {
    let mut error = failure.error;
    if let Some(c) = failure.captured {
        error["task"] = diagnostic(&c.task, &c.record);
    }
    error
}
pub fn mcp_download_start(task: &Task) -> Value {
    json!({"runId":task.run,"taskId":task.id,"kind":task.kind.name()})
}

const LIMIT: usize = 256 * 1024;
struct Budget {
    bytes: usize,
    buffer: Option<Vec<u8>>,
}
impl Write for Budget {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > LIMIT - self.bytes {
            return Err(io::Error::other("response budget exceeded"));
        }
        self.bytes += bytes.len();
        if let Some(buffer) = &mut self.buffer {
            buffer.extend_from_slice(bytes);
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn encode(value: &Value, error: bool) -> Result<CallToolResult, serde_json::Error> {
    let mut buffer = Budget {
        bytes: 0,
        buffer: Some(Vec::new()),
    };
    serde_json::to_writer(&mut buffer, value)?;
    let text = String::from_utf8(buffer.buffer.unwrap()).expect("JSON is UTF-8");
    let result = if error {
        CallToolResult::error(vec![ContentBlock::text(text)])
    } else {
        CallToolResult::success(vec![ContentBlock::text(text)])
    };
    serde_json::to_writer(
        Budget {
            bytes: 0,
            buffer: None,
        },
        &result,
    )?;
    Ok(result)
}
pub fn tool_result(
    value: Value,
    error: bool,
    operation: &str,
    task_id: Option<&str>,
    cursor: Option<&str>,
) -> CallToolResult {
    encode(&value,error).unwrap_or_else(|_| {
        let mut limit = json!({"error":{"code":"resourceLimit","message":"MCP result exceeds 256 KiB; read bounded text/none or local evidence","operation":operation}});
        if let Some(id) = task_id { limit["error"]["taskId"] = json!(id); }
        if let Some(cursor) = cursor { limit["error"]["cursor"] = json!(cursor); }
        encode(&limit,true).expect("bounded resourceLimit response")
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn captured() -> CapturedTask {
        CapturedTask::new(crate::task::fixture(std::path::PathBuf::new()))
    }
    #[test]
    fn projections_keep_actual_failure_facts_and_diagnostics_separate() {
        let mut c = captured();
        c.record.native = json!({"state":"unavailable","error":{"code":-32000,"message":"中".repeat(180),"details":{"why":"lost"}},"lastResult":{"state":"running","execSent":true,"workerFinished":false,"lastResult":{"state":"opening"}}});
        c.record.error = json!({"code":"disk","message":"disk failed","details":{"path":"secret"}});
        c.record.saved = Saved::Failed;
        c.record.cleanup_error =
            json!({"code":"cleanup","message":"release failed","details":["release"]});
        let ordinary = mcp_facts(&c, false, false);
        assert_eq!(ordinary["native"]["error"]["code"], -32000);
        assert_eq!(
            ordinary["native"]["error"]["message"]
                .as_str()
                .unwrap()
                .len(),
            510
        );
        assert_eq!(ordinary["native"]["error"]["messageTruncated"], true);
        assert_eq!(
            ordinary["native"]["lastResult"]["lastResultDetailsAvailable"],
            true
        );
        assert!(ordinary["native"].get("remoteExitConfirmed").is_none());
        assert!(ordinary["native"]["lastResult"]
            .get("workerFinished")
            .is_none());
        assert_eq!(ordinary["evidence"]["error"]["detailsAvailable"], true);
        assert_eq!(ordinary["cleanupError"]["detailsAvailable"], true);
        assert_eq!(ordinary["resourceReleased"], false);
        assert_eq!(ordinary["evidence"]["complete"], false);
        let diagnostic = mcp_facts(&c, true, false);
        assert_eq!(diagnostic["native"], c.record.native);
        assert_eq!(diagnostic["evidence"]["error"], c.record.error);
        assert_eq!(diagnostic["cleanupError"], c.record.cleanup_error);
        assert_eq!(diagnostic["context"], c.task.target.context);
        c.record.native = json!({"state":"completed","exitCode":7,"remoteExitConfirmed":true,"outputComplete":true});
        c.record.error = Value::Null;
        c.record.saved = Saved::Complete;
        c.record.phase = Phase::Done;
        let value = mcp_facts(&c, false, false);
        assert_eq!(value["native"]["exitCode"], 7);
        assert_eq!(value["evidence"]["complete"], true);
        assert!(value["evidence"].get("error").is_none());
        assert!(value.get("success").is_none());
        assert_eq!(
            c.task.snapshot().revision,
            1,
            "projection does not mutate revision"
        );
    }
    #[test]
    fn download_path_is_available_only_after_complete_commit() {
        let task = std::sync::Arc::new(Task::new(crate::task::NewTask {
            run: "run".into(),
            order: 1,
            admission_sequence: 1,
            target: captured().task.target.clone(),
            input: json!({"requestId":"download","remotePath":"/data/中文"}),
            kind: Kind::Download,
            root: std::path::PathBuf::new(),
        }));
        let mut c = CapturedTask::new(task);
        c.record.path = Some("download.bin".into());
        c.record.saved = Saved::Complete;
        assert!(mcp_facts(&c, false, false)["file"]["localPath"].is_null());
        c.record.phase = Phase::Done;
        assert_eq!(
            mcp_facts(&c, false, false)["file"]["localPath"],
            "download.bin"
        );
        c.record.saved = Saved::Failed;
        assert!(mcp_facts(&c, false, false)["file"]["localPath"].is_null());
        assert_eq!(
            mcp_facts(&c, true, false)["evidence"]["path"],
            "download.bin"
        );
    }
    #[test]
    fn result_budget_counts_outer_escaping_and_returns_one_bounded_error() {
        let small = tool_result(json!({"stdout":"中文\n\"\\"}), false, "get", None, None);
        let serialized = serde_json::to_value(&small).unwrap();
        assert!(serialized.get("structuredContent").is_none());
        assert_eq!(serialized["content"].as_array().unwrap().len(), 1);
        assert_eq!(
            serde_json::from_str::<Value>(serialized["content"][0]["text"].as_str().unwrap())
                .unwrap()["stdout"],
            "中文\n\"\\"
        );
        let overhead = serde_json::to_vec(&encode(&json!(""), false).unwrap())
            .unwrap()
            .len();
        let at_limit = json!("a".repeat(LIMIT - overhead));
        assert_eq!(
            serde_json::to_vec(&encode(&at_limit, false).unwrap())
                .unwrap()
                .len(),
            LIMIT
        );
        assert!(encode(&json!("a".repeat(LIMIT - overhead + 1)), false).is_err());
        let escaping = json!("\\".repeat(LIMIT / 3));
        assert!(serde_json::to_vec(&escaping).unwrap().len() < LIMIT);
        assert!(encode(&escaping, false).is_err());
        for error in [false, true] {
            let result = tool_result(
                escaping.clone(),
                error,
                "sodeck_task_get",
                Some("task-1"),
                Some("100"),
            );
            let result = serde_json::to_value(result).unwrap();
            assert_eq!(result["isError"], true);
            let value: Value =
                serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
            assert_eq!(value["error"]["code"], "resourceLimit");
            assert_eq!(value["error"]["taskId"], "task-1");
            assert_eq!(value["error"]["cursor"], "100");
            assert!(serde_json::to_vec(&result).unwrap().len() < 1024);
        }
    }
}

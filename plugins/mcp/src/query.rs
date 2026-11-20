use crate::app::App;
use crate::{
    evidence::{self, Page, PageFormat, ReadError},
    task::{CapturedTask, Kind, Phase, Task, TaskFailure},
    tasks::{failure, text, StartedTask, WaitSlots},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sodeck_plugin_client::encoding::{base64, unbase64};
use std::{path::PathBuf, sync::Arc, time::Instant};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;

pub struct Options {
    pub format: Option<PageFormat>,
    pub cursor: u64,
    pub tail: bool,
    pub wait_ms: u64,
    pub since: Option<u64>,
}
fn decimal(value: &Value, name: &str) -> Result<u64, Value> {
    let code = if name == "cursor" {
        "invalidCursor"
    } else {
        "invalidParams"
    };
    let s = value
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 20 && s.bytes().all(|b| b.is_ascii_digit()))
        .ok_or_else(|| failure(code, format!("{name} 必须为十进制字符串")))?;
    s.parse()
        .map_err(|_| failure(code, format!("{name} 超出范围")))
}
fn read_options(p: &Value) -> Result<Options, Value> {
    let format = match p
        .get("outputMode")
        .map(|v| v.as_str().unwrap_or(""))
        .unwrap_or("text")
    {
        "text" => Some(PageFormat::Text),
        "full" => Some(PageFormat::Full),
        "none" => None,
        _ => return Err(failure("invalidParams", "outputMode 无效")),
    };
    let cursor = p
        .get("cursor")
        .map(|v| decimal(v, "cursor"))
        .transpose()?
        .unwrap_or(0);
    if format.is_none() && p.get("cursor").is_some_and(|v| v != "0") {
        return Err(failure("invalidParams", "none 模式不能使用正文游标"));
    }
    let wait_ms = p
        .get("waitMs")
        .map(|v| {
            v.as_u64()
                .ok_or_else(|| failure("invalidParams", "waitMs 必须为整数"))
        })
        .transpose()?
        .unwrap_or(0);
    if wait_ms > 20000 {
        return Err(failure("invalidParams", "waitMs 超出范围"));
    }
    let since = p
        .get("sinceRevision")
        .map(|v| decimal(v, "sinceRevision"))
        .transpose()?;
    Ok(Options {
        format,
        cursor,
        tail: false,
        wait_ms,
        since,
    })
}
pub fn parse_mcp_read(p: &Value) -> Result<(Options, bool), Value> {
    let detail = p
        .get("detail")
        .map(|v| {
            v.as_bool()
                .ok_or_else(|| failure("invalidParams", "detail 必须为布尔值"))
        })
        .transpose()?
        .unwrap_or(false);
    Ok((read_options(p)?, detail))
}
pub fn parse_ui_read(p: &Value) -> Result<Options, Value> {
    if !p.as_object().is_some_and(|o| {
        o.keys().all(|k| {
            matches!(
                k.as_str(),
                "action" | "taskId" | "cursor" | "tail" | "outputMode"
            )
        })
    }) {
        return Err(failure("invalidParams", "侧栏读取参数无效"));
    }
    let format = match p.get("outputMode").map(Value::as_str) {
        None => Some(PageFormat::Text),
        Some(Some("none")) if p.get("cursor").is_none() && p.get("tail").is_none() => None,
        _ => {
            return Err(failure(
                "invalidParams",
                "侧栏 outputMode 仅支持无游标的 none",
            ))
        }
    };
    let tail = p
        .get("tail")
        .map(|v| {
            v.as_bool()
                .ok_or_else(|| failure("invalidParams", "tail 必须为布尔值"))
        })
        .transpose()?
        .unwrap_or(false);
    if tail && p.get("cursor").is_some() {
        return Err(failure("invalidParams", "最近页不能组合正文游标"));
    }
    let cursor = p
        .get("cursor")
        .map(|v| decimal(v, "cursor"))
        .transpose()?
        .unwrap_or(0);
    Ok(Options {
        format,
        cursor,
        tail,
        wait_ms: 0,
        since: None,
    })
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WaitResult {
    pub requested_ms: u64,
    pub elapsed_ms: u64,
    pub reason: &'static str,
}
pub struct TaskRead {
    pub captured: CapturedTask,
    pub page: Option<Page>,
    pub wait: Option<WaitResult>,
}
#[derive(Clone, Copy)]
enum WaitUntil {
    Change,
    Done,
}
pub struct ReadJob {
    path: PathBuf,
    committed: u64,
    format: PageFormat,
    cursor: u64,
    tail: bool,
    terminal: bool,
    _permit: OwnedSemaphorePermit,
}
fn validate(c: &CapturedTask, options: &Options) -> Result<(), Value> {
    if options.since.is_some_and(|n| n > c.record.revision) {
        return Err(failure("invalidParams", "sinceRevision 超过任务当前版本"));
    }
    if c.task.kind() == Kind::Exec
        && options.format.is_some()
        && options.cursor > c.record.committed
    {
        return Err(failure("invalidCursor", "cursor 超出已保存范围"));
    }
    Ok(())
}
fn prepare(app: &App, c: &CapturedTask, options: Options) -> Result<Option<ReadJob>, Value> {
    validate(c, &options)?;
    if c.task.kind() != Kind::Exec || options.format.is_none() {
        return Ok(None);
    }
    let permit = app
        .io
        .clone()
        .try_acquire_owned()
        .map_err(|_| failure("resourceBusy", "证据读取已满，请稍后重读同一游标"))?;
    Ok(Some(ReadJob {
        path: c.task.directory.join("output.jsonl"),
        committed: c.record.committed,
        format: options.format.expect("body job has a format"),
        cursor: options.cursor,
        tail: options.tail,
        terminal: c.task.terminal(),
        _permit: permit,
    }))
}
impl ReadJob {
    fn read(self) -> Result<Page, Value> {
        let result = if self.tail {
            evidence::tail(self.path, self.committed, self.format, self.terminal)
        } else {
            evidence::page(
                self.path,
                self.committed,
                self.cursor,
                self.format,
                self.terminal,
            )
        };
        result.map_err(|e| match e {
            ReadError::InvalidCursor(message) => failure("invalidCursor", message),
            ReadError::EvidenceReadFailed(message) => failure("evidenceReadFailed", message),
        })
    }
}
pub fn ui_read(app: &App, task: Arc<Task>, options: Options) -> Result<TaskRead, TaskFailure> {
    let captured = CapturedTask::new(task);
    let page = prepare(app, &captured, options).and_then(|job| job.map(ReadJob::read).transpose());
    match page {
        Ok(page) => Ok(TaskRead {
            captured,
            page,
            wait: None,
        }),
        Err(error) => Err(TaskFailure {
            error,
            captured: Some(captured),
        }),
    }
}
pub async fn get(
    app: Arc<App>,
    p: &Value,
    options: Options,
    ct: CancellationToken,
) -> Result<TaskRead, TaskFailure> {
    let task = app.resolve(p, None)?;
    let started = StartedTask {
        task,
        wait_ms: options.wait_ms,
        started: Instant::now(),
        wait_slots: None,
    };
    read_task(app, started, options, WaitUntil::Change, ct).await
}
pub async fn after_start(
    app: Arc<App>,
    started: StartedTask,
    ct: CancellationToken,
) -> Result<TaskRead, TaskFailure> {
    let options = Options {
        format: Some(PageFormat::Text),
        cursor: 0,
        tail: false,
        wait_ms: started.wait_ms,
        since: None,
    };
    read_task(app, started, options, WaitUntil::Done, ct).await
}
async fn wait_task(
    waiting: &Arc<Semaphore>,
    shutdown: &CancellationToken,
    start: StartedTask,
    options: &Options,
    until: WaitUntil,
    ct: &CancellationToken,
) -> Result<(CapturedTask, &'static str), TaskFailure> {
    let StartedTask {
        task,
        started,
        mut wait_slots,
        ..
    } = start;
    let mut changes = task.subscribe();
    let first = CapturedTask::new(task.clone());
    if let Err(error) = validate(&first, &options) {
        return Err(TaskFailure {
            error,
            captured: Some(first),
        });
    }
    let baseline = options.since.unwrap_or(first.record.revision);
    let deadline =
        tokio::time::Instant::from_std(started + std::time::Duration::from_millis(options.wait_ms));
    let mut captured = first;
    let reason = loop {
        if let Some(error) = call_cancelled(shutdown, ct) {
            return Err(TaskFailure {
                error,
                captured: Some(CapturedTask::new(task.clone())),
            });
        }
        if options.wait_ms == 0 {
            break "";
        }
        let r = &captured.record;
        let reason = if r.phase == Phase::Done {
            "done"
        } else if matches!(until, WaitUntil::Change)
            && task.kind() == Kind::Exec
            && options.format.is_some()
            && options.cursor < r.committed
        {
            "output"
        } else if matches!(until, WaitUntil::Change) && r.revision > baseline {
            "changed"
        } else {
            ""
        };
        if !reason.is_empty() {
            break reason;
        }
        if tokio::time::Instant::now() >= deadline {
            captured = CapturedTask::new(task.clone());
            if captured.record.phase == Phase::Done {
                break "done";
            }
            break "timeout";
        }
        if wait_slots.is_none() {
            match WaitSlots::acquire(waiting, &task) {
                Ok(slots) => wait_slots = Some(slots),
                Err(error) => {
                    captured = CapturedTask::new(task.clone());
                    if captured.record.phase == Phase::Done {
                        break "done";
                    }
                    return Err(TaskFailure {
                        error,
                        captured: Some(captured),
                    });
                }
            }
        }
        let error = tokio::select! {
            _=ct.cancelled()=>Some(failure("requestCancelled","读取等待已取消，远端任务继续运行")),
            _=shutdown.cancelled()=>Some(failure("unavailable","插件正在退出")),
            _=tokio::time::sleep_until(deadline)=>None,
            changed=changes.changed()=>if changed.is_err(){Some(failure("unavailable","任务通知已结束"))}else{None},
        };
        if let Some(error) = error {
            return Err(TaskFailure {
                error,
                captured: Some(CapturedTask::new(task.clone())),
            });
        }
        captured = CapturedTask::new(task.clone());
    };
    drop(wait_slots);
    Ok((captured, reason))
}
fn call_cancelled(shutdown: &CancellationToken, ct: &CancellationToken) -> Option<Value> {
    if ct.is_cancelled() {
        Some(failure("requestCancelled", "读取已取消，远端任务继续运行"))
    } else if shutdown.is_cancelled() {
        Some(failure("unavailable", "插件正在退出"))
    } else {
        None
    }
}
async fn read_task(
    app: Arc<App>,
    start: StartedTask,
    options: Options,
    until: WaitUntil,
    ct: CancellationToken,
) -> Result<TaskRead, TaskFailure> {
    let started = start.started;
    let (captured, reason) =
        wait_task(&app.waiting, &app.shutdown, start, &options, until, &ct).await?;
    if let Some(error) = call_cancelled(&app.shutdown, &ct) {
        return Err(TaskFailure {
            error,
            captured: Some(captured),
        });
    }
    read_captured(&app, captured, options, started, reason).await
}
async fn read_captured(
    app: &App,
    captured: CapturedTask,
    options: Options,
    started: Instant,
    reason: &'static str,
) -> Result<TaskRead, TaskFailure> {
    let wait_ms = options.wait_ms;
    let page = match prepare(&app, &captured, options) {
        Ok(Some(job)) => tokio::task::spawn_blocking(move || job.read())
            .await
            .unwrap_or_else(|e| Err(failure("evidenceReadFailed", e.to_string())))
            .map(Some),
        Ok(None) => Ok(None),
        Err(error) => Err(error),
    };
    match page {
        Ok(page) => Ok(TaskRead {
            captured,
            page,
            wait: (wait_ms > 0).then(|| WaitResult {
                requested_ms: wait_ms,
                elapsed_ms: started.elapsed().as_millis() as u64,
                reason,
            }),
        }),
        Err(error) => Err(TaskFailure {
            error,
            captured: Some(captured),
        }),
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Filter {
    target: Option<String>,
    server: Option<String>,
    request: Option<String>,
    status: String,
    kind: String,
    limit: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    v: u8,
    run: String,
    filter: Filter,
    ceiling: u64,
    before: u64,
}
fn optional(p: &Value, key: &str, max: usize) -> Result<Option<String>, Value> {
    p.get(key)
        .map(|_| text(p, key, max).map(str::to_owned))
        .transpose()
}
pub struct TaskList {
    pub run: String,
    pub tasks: Vec<CapturedTask>,
    pub active: u64,
    pub done: u64,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}
pub fn list(app: &App, p: &Value) -> Result<TaskList, Value> {
    let target = optional(p, "targetId", 256)?;
    let server = optional(p, "serverId", 256)?;
    let request = optional(p, "requestId", 128)?;
    if target.is_some() && server.is_some() {
        return Err(failure(
            "invalidParams",
            "targetId 与 serverId 不能同时指定",
        ));
    }
    let status = p
        .get("status")
        .map(|_| text(p, "status", 16))
        .transpose()?
        .unwrap_or("all")
        .to_owned();
    let kind = p
        .get("kind")
        .map(|_| text(p, "kind", 16))
        .transpose()?
        .unwrap_or("all")
        .to_owned();
    let limit = p
        .get("limit")
        .map(|v| {
            v.as_u64()
                .ok_or_else(|| failure("invalidParams", "limit 必须为整数"))
        })
        .transpose()?
        .unwrap_or(20);
    if !["all", "active", "done"].contains(&status.as_str())
        || !["all", "exec", "download"].contains(&kind.as_str())
        || !(1..=50).contains(&limit)
    {
        return Err(failure("invalidParams", "列表筛选或 limit 无效"));
    }
    let filter = Filter {
        target,
        server,
        request,
        status,
        kind,
        limit,
    };
    let mut records = app.records();
    records.sort_by_key(|t| std::cmp::Reverse(t.order));
    let maximum = records.iter().map(|t| t.order).max().unwrap_or(0);
    let (ceiling, before) = if let Some(s) = p.get("cursor") {
        let s = s
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 2048)
            .ok_or_else(|| failure("invalidCursor", "列表 cursor 无效"))?;
        let decoded = unbase64(s).map_err(|_| failure("invalidCursor", "列表 cursor 编码无效"))?;
        let cursor: Cursor = serde_json::from_slice(&decoded)
            .map_err(|_| failure("invalidCursor", "列表 cursor 无效"))?;
        if cursor.run != app.run {
            return Err(failure("runMismatch", "列表 cursor 来自已结束的运行"));
        }
        if cursor.v != 1
            || cursor.filter != filter
            || cursor.ceiling > maximum
            || cursor.before == 0
            || cursor.before > cursor.ceiling
        {
            return Err(failure("invalidCursor", "列表 cursor 与筛选或边界不匹配"));
        }
        (cursor.ceiling, cursor.before)
    } else {
        (maximum, maximum.saturating_add(1))
    };
    let mut candidates = Vec::new();
    let (mut active, mut done) = (0, 0);
    for task in records {
        if task.order > ceiling
            || filter.target.as_ref().is_some_and(|s| s != &task.target.id)
            || filter
                .server
                .as_ref()
                .is_some_and(|s| &task.target.context.server_id != s)
            || filter
                .request
                .as_ref()
                .is_some_and(|s| task.start_input()["requestId"] != *s)
            || (filter.kind != "all" && filter.kind != task.kind().name())
        {
            continue;
        }
        let captured = CapturedTask::new(task);
        let task = &captured.task;
        let finished = captured.record.phase == Phase::Done;
        if finished {
            done += 1;
        } else {
            active += 1;
        }
        if task.order < before && (filter.status == "all" || (filter.status == "done") == finished)
        {
            if candidates.len() <= limit as usize {
                candidates.push(captured);
            }
        }
    }
    let has_more = candidates.len() > limit as usize;
    candidates.truncate(limit as usize);
    let next = if has_more {
        Some(base64(
            &serde_json::to_vec(&Cursor {
                v: 1,
                run: app.run.clone(),
                filter,
                ceiling,
                before: candidates.last().unwrap().task.order,
            })
            .unwrap(),
        ))
    } else {
        None
    };
    Ok(TaskList {
        run: app.run.clone(),
        tasks: candidates,
        active,
        done,
        has_more,
        next_cursor: next,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn start(task: Arc<Task>, wait_ms: u64, wait_slots: Option<WaitSlots>) -> StartedTask {
        StartedTask {
            task,
            wait_ms,
            started: Instant::now(),
            wait_slots,
        }
    }
    fn options(wait_ms: u64) -> Options {
        Options {
            format: Some(PageFormat::Text),
            cursor: 0,
            tail: false,
            wait_ms,
            since: None,
        }
    }
    fn finish(task: &Task) {
        task.finalizing(true, Value::Null);
        task.cleanup(true, Value::Null);
        task.publish(task.final_candidate(), None);
    }
    #[tokio::test]
    async fn done_wait_observes_outputs_cancellation_and_finalizing_without_early_return() {
        let task = crate::task::fixture(PathBuf::new());
        let waiting = Arc::new(Semaphore::new(1));
        let slots = WaitSlots::acquire(&waiting, &task).unwrap();
        let shutdown = CancellationToken::new();
        let ct = CancellationToken::new();
        let read_options = options(1000);
        let wait = wait_task(
            &waiting,
            &shutdown,
            start(task.clone(), 1000, Some(slots)),
            &read_options,
            WaitUntil::Done,
            &ct,
        );
        let publish = async {
            for n in 1..=3 {
                tokio::time::sleep(std::time::Duration::from_millis(15)).await;
                task.observe(json!({"state":"running","received":n}), Some(n * 10));
                assert_eq!(waiting.available_permits(), 0);
            }
            task.request_cancel();
            task.observe(json!({"state":"completed","exitCode":0}), None);
            task.finalizing(true, Value::Null);
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            assert_eq!(
                waiting.available_permits(),
                0,
                "finalizing is not stable done"
            );
            task.cleanup(true, Value::Null);
            task.publish(task.final_candidate(), None);
        };
        let (result, _) = tokio::join!(wait, publish);
        let (captured, reason) = result.unwrap_or_else(|e| panic!("{}", e.error));
        assert_eq!(reason, "done");
        assert!(captured.record.phase == Phase::Done);
        assert!(captured.record.cancel_requested);
        assert_eq!(waiting.available_permits(), 1);
        assert_eq!(task.waiters.available_permits(), 2);
    }
    #[tokio::test]
    async fn done_deadline_is_fixed_despite_continuous_changes() {
        let task = crate::task::fixture(PathBuf::new());
        let waiting = Arc::new(Semaphore::new(1));
        let shutdown = CancellationToken::new();
        let ct = CancellationToken::new();
        let read_options = options(80);
        let begin = Instant::now();
        let wait = wait_task(
            &waiting,
            &shutdown,
            start(task.clone(), 80, None),
            &read_options,
            WaitUntil::Done,
            &ct,
        );
        let changes = async {
            for n in 1..=12 {
                tokio::time::sleep(std::time::Duration::from_millis(15)).await;
                task.observe(json!({"state":"running","received":n}), Some(n));
            }
        };
        let observe = async {
            let (captured, reason) = wait.await.unwrap_or_else(|e| panic!("{}", e.error));
            assert_eq!(reason, "timeout");
            assert!(captured.record.phase != Phase::Done);
            assert!(begin.elapsed().as_millis() >= 70);
            assert!(
                captured.record.committed < 12,
                "changes must not restart the deadline"
            );
            assert_eq!(waiting.available_permits(), 1);
        };
        tokio::join!(observe, changes);
    }
    #[tokio::test]
    async fn expired_original_window_and_stable_done_need_no_wait_slots() {
        let task = crate::task::fixture(PathBuf::new());
        let waiting = Arc::new(Semaphore::new(0));
        let shutdown = CancellationToken::new();
        let ct = CancellationToken::new();
        let read_options = options(20000);
        let mut request = start(task.clone(), 20000, None);
        request.started -= std::time::Duration::from_millis(20000);
        let (_, reason) = wait_task(
            &waiting,
            &shutdown,
            request,
            &read_options,
            WaitUntil::Done,
            &ct,
        )
        .await
        .unwrap_or_else(|e| panic!("{}", e.error));
        assert_eq!(reason, "timeout");
        finish(&task);
        let mut request = start(task, 20000, None);
        request.started -= std::time::Duration::from_millis(20000);
        let (_, reason) = wait_task(
            &waiting,
            &shutdown,
            request,
            &read_options,
            WaitUntil::Done,
            &ct,
        )
        .await
        .unwrap_or_else(|e| panic!("{}", e.error));
        assert_eq!(reason, "done");
    }
    #[tokio::test]
    async fn cancellation_releases_call_slots_and_keeps_task_running() {
        let task = crate::task::fixture(PathBuf::new());
        let waiting = Arc::new(Semaphore::new(1));
        let shutdown = CancellationToken::new();
        let ct = CancellationToken::new();
        let read_options = options(20000);
        let wait = wait_task(
            &waiting,
            &shutdown,
            start(task.clone(), 20000, None),
            &read_options,
            WaitUntil::Done,
            &ct,
        );
        let cancel = async {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            ct.cancel();
        };
        let (result, _) = tokio::join!(wait, cancel);
        let error = result.err().expect("call cancellation");
        assert_eq!(error.error["code"], "requestCancelled");
        assert_eq!(error.captured.unwrap().task.id, task.id);
        assert!(!task.cancel.is_cancelled());
        assert_eq!(waiting.available_permits(), 1);
        assert_eq!(task.waiters.available_permits(), 2);
    }
    #[test]
    fn partial_wait_acquisition_rolls_back_global_slot() {
        let task = crate::task::fixture(PathBuf::new());
        let waiting = Arc::new(Semaphore::new(1));
        let _held = task.waiters.clone().try_acquire_many_owned(2).unwrap();
        assert!(WaitSlots::acquire(&waiting, &task).is_err());
        assert_eq!(waiting.available_permits(), 1);
    }
    #[test]
    fn read_defaults_are_explicit_and_detail_does_not_change_body_options() {
        let (plain, detail) = parse_mcp_read(&json!({})).unwrap();
        assert!(!detail);
        assert!(plain.format == Some(PageFormat::Text));
        let (none, detail) = parse_mcp_read(&json!({"detail":true,"outputMode":"none"})).unwrap();
        assert!(detail);
        assert!(none.format.is_none());
        assert!(parse_ui_read(&json!({"waitMs":1})).is_err());
        for p in [
            json!({"outputMode":"full"}),
            json!({"outputMode":"none","cursor":"0"}),
            json!({"outputMode":"none","tail":false}),
            json!({"waitMs":0}),
            json!({"sinceRevision":"1"}),
            json!({"detail":false}),
            json!({"tail":true,"cursor":"0"}),
            json!({"unexpected":true}),
        ] {
            assert_eq!(parse_ui_read(&p).err().unwrap()["code"], "invalidParams");
        }
        let sidebar = parse_ui_read(&json!({"action":"task","taskId":"task","tail":true})).unwrap();
        assert!(sidebar.tail);
        assert!(sidebar.format == Some(PageFormat::Text));
        assert_eq!(sidebar.wait_ms, 0);
        assert!(sidebar.since.is_none());
        let facts =
            parse_ui_read(&json!({"action":"task","taskId":"task","outputMode":"none"})).unwrap();
        assert!(facts.format.is_none());
        assert!(parse_mcp_read(&json!({"detail":"true"})).is_err());
        assert!(parse_mcp_read(&json!({"outputMode":"none","cursor":"1"})).is_err());
    }
    #[test]
    fn blocking_job_retains_watermark_and_permit_until_read_finishes() {
        let root = std::env::temp_dir().join(format!("mcp-snapshot-{}", rand::random::<u64>()));
        let task = crate::task::fixture(root.clone());
        std::fs::create_dir_all(&task.directory).unwrap();
        let mut output = evidence::Output::new(&task.directory, false).unwrap();
        let envelope = |n: u64, text: &str| json!({"v":1,"sequence":n.to_string(),"source":"stdout","receivedAt":n,"dataBase64":base64(text.as_bytes())});
        output.append(&[envelope(1, "first")]).unwrap();
        task.observe(json!({"state":"running"}), Some(output.committed));
        let captured = CapturedTask::new(task.clone());
        let saved = captured.record.committed;
        let pool = Arc::new(tokio::sync::Semaphore::new(1));
        let job = ReadJob {
            terminal: false,
            path: task.directory.join("output.jsonl"),
            committed: saved,
            format: PageFormat::Text,
            cursor: 0,
            tail: false,
            _permit: pool.clone().try_acquire_owned().unwrap(),
        };
        assert!(pool.clone().try_acquire_owned().is_err());
        output.append(&[envelope(2, "second")]).unwrap();
        task.observe(json!({"state":"completed"}), Some(output.committed));
        let page = job.read().unwrap();
        assert!(pool.clone().try_acquire_owned().is_ok());
        assert_eq!(page.saved_bytes, saved);
        let read = TaskRead {
            captured,
            page: Some(page),
            wait: None,
        };
        let result = crate::response::mcp_task(&read, false);
        assert_eq!(result["native"]["state"], "running");
        assert_eq!(result["output"]["stdout"], "first");
        assert_eq!(result["output"]["savedBytes"], saved.to_string());
        drop(output);
        std::fs::remove_dir_all(root).unwrap();
    }
}

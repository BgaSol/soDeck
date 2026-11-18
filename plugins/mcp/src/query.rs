use crate::{
    evidence,
    task::{Kind, Snapshot, Task},
    tasks::{failure, text, App},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sodeck_plugin_client::encoding::{base64, unbase64};
use std::{sync::Arc, time::Instant};
use tokio::sync::OwnedSemaphorePermit;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Full,
    Text,
    None,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_use_the_captured_watermark_and_hold_io_permit_until_finished() {
        let root = std::env::temp_dir().join(format!("mcp-snapshot-{}", rand::random::<u64>()));
        let task = crate::task::fixture(root.clone());
        std::fs::create_dir_all(&task.directory).unwrap();
        let mut output = evidence::Output::new(&task.directory).unwrap();
        let envelope = |n: u64, data: &str| json!({"v":1,"sequence":n.to_string(),"source":"stdout","receivedAt":n,"dataBase64":base64(data.as_bytes())});
        output.append(&[envelope(1, "first")]).unwrap();
        task.observe(json!({"state":"running"}), Some(output.committed));
        let snapshot = task.snapshot();
        let saved = snapshot.committed;
        let pool = Arc::new(tokio::sync::Semaphore::new(1));
        let permit = pool.clone().try_acquire_owned().unwrap();
        let job = ReadJob {
            task: task.clone(),
            snapshot,
            options: Options::parse(&json!({"outputMode":"text"}), false).unwrap(),
            _permit: Some(permit),
        };
        assert!(pool.clone().try_acquire_owned().is_err());
        output.append(&[envelope(2, "second")]).unwrap();
        task.observe(json!({"state":"completed"}), Some(output.committed));
        let result = job.read().unwrap();
        assert_eq!(result["native"]["state"], "running");
        assert_eq!(result["output"]["savedBytes"], saved.to_string());
        assert_eq!(result["output"]["stdout"], "first");
        assert!(pool.clone().try_acquire_owned().is_ok());
        let options = Options::parse(&json!({"outputMode":"none"}), false).unwrap();
        let job = ReadJob {
            task: task.clone(),
            snapshot: task.snapshot(),
            options,
            _permit: None,
        };
        drop(output);
        std::fs::remove_file(task.directory.join("output.jsonl")).unwrap();
        assert!(job.read().unwrap().get("output").is_none());
        let job = ReadJob {
            task: task.clone(),
            snapshot: task.snapshot(),
            options: Options::parse(&json!({}), false).unwrap(),
            _permit: None,
        };
        assert_eq!(job.read().unwrap_err()["code"], "evidenceReadFailed");
        std::fs::remove_dir_all(root).unwrap();
    }
}
pub struct Options {
    pub mode: Mode,
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
impl Options {
    pub fn parse(p: &Value, ui: bool) -> Result<Self, Value> {
        let mode = match p
            .get("outputMode")
            .map(|v| v.as_str().unwrap_or(""))
            .unwrap_or(if ui { "text" } else { "full" })
        {
            "full" => Mode::Full,
            "text" => Mode::Text,
            "none" => Mode::None,
            _ => return Err(failure("invalidParams", "outputMode 无效")),
        };
        let cursor = p
            .get("cursor")
            .map(|v| decimal(v, "cursor"))
            .transpose()?
            .unwrap_or(0);
        if mode == Mode::None && p.get("cursor").is_some_and(|v| v != "0") {
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
        if wait_ms > 20000 || (ui && wait_ms != 0) {
            return Err(failure("invalidParams", "waitMs 超出范围"));
        }
        let since = p
            .get("sinceRevision")
            .map(|v| decimal(v, "sinceRevision"))
            .transpose()?;
        let tail = if ui {
            p.get("tail")
                .map(|v| {
                    v.as_bool()
                        .ok_or_else(|| failure("invalidParams", "tail 必须为布尔值"))
                })
                .transpose()?
                .unwrap_or(false)
        } else {
            false
        };
        Ok(Self {
            mode,
            cursor,
            tail,
            wait_ms,
            since,
        })
    }
}
pub struct ReadJob {
    task: Arc<Task>,
    snapshot: Snapshot,
    options: Options,
    _permit: Option<OwnedSemaphorePermit>,
}
pub fn prepare(
    app: &App,
    snapshot: Snapshot,
    task: Arc<Task>,
    options: Options,
) -> Result<ReadJob, Value> {
    if options.since.is_some_and(|n| n > snapshot.revision) {
        return Err(failure("invalidParams", "sinceRevision 超过任务当前版本"));
    }
    if task.kind == Kind::Exec && options.mode != Mode::None && options.cursor > snapshot.committed
    {
        return Err(failure("invalidCursor", "cursor 超出已保存范围"));
    }
    let permit = if task.kind == Kind::Exec && options.mode != Mode::None {
        Some(
            app.io
                .clone()
                .try_acquire_owned()
                .map_err(|_| failure("resourceBusy", "证据读取已满，请稍后重读同一游标"))?,
        )
    } else {
        None
    };
    Ok(ReadJob {
        task,
        snapshot,
        options,
        _permit: permit,
    })
}
impl ReadJob {
    pub fn read(mut self) -> Result<Value, Value> {
        if self.task.kind == Kind::Exec && self.options.mode != Mode::None {
            let path = self.task.directory.join("output.jsonl");
            let missing =
                matches!(path.metadata(), Err(e) if e.kind() == std::io::ErrorKind::NotFound);
            let result = if missing && self.snapshot.committed == 0 {
                if self.options.mode == Mode::Full {
                    None
                } else {
                    Some(Ok(
                        json!({"stdout":"","stderr":"","nextCursor":"0","savedBytes":"0","hasMore":false,"previewEncoding":"utf-8-lossy-per-page"}),
                    ))
                }
            } else if self.options.tail {
                Some(evidence::tail(
                    path,
                    self.snapshot.committed,
                    self.options.mode == Mode::Full,
                ))
            } else {
                Some(evidence::page(
                    path,
                    self.snapshot.committed,
                    self.options.cursor,
                    self.options.mode == Mode::Full,
                ))
            };
            if let Some(result) = result {
                self.snapshot.value["output"] = result.map_err(|message| {
                    let code = if message.starts_with("cursor ") {
                        "invalidCursor"
                    } else {
                        "evidenceReadFailed"
                    };
                    let mut error = failure(code, message);
                    error["task"] = self.snapshot.value.clone();
                    error
                })?;
            }
        }
        Ok(self.snapshot.value)
    }
}
pub async fn get(app: Arc<App>, p: Value, ct: CancellationToken) -> Result<Value, Value> {
    let task = app.resolve(&p, None)?;
    let options = Options::parse(&p, false)?;
    let mut changes = task.subscribe();
    let started = Instant::now();
    let first = task.snapshot();
    if options.since.is_some_and(|n| n > first.revision) {
        return Err(failure("invalidParams", "sinceRevision 超过任务当前版本"));
    }
    if task.kind == Kind::Exec && options.mode != Mode::None && options.cursor > first.committed {
        return Err(failure("invalidCursor", "cursor 超出已保存范围"));
    }
    let baseline = options.since.unwrap_or(first.revision);
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(options.wait_ms);
    let mut permit = None;
    let mut reason = "";
    let snapshot = loop {
        let snapshot = task.snapshot();
        if options.wait_ms == 0 {
            break snapshot;
        }
        reason = if snapshot.done {
            "done"
        } else if task.kind == Kind::Exec
            && options.mode != Mode::None
            && options.cursor < snapshot.committed
        {
            "output"
        } else if snapshot.revision > baseline {
            "changed"
        } else {
            ""
        };
        if !reason.is_empty() {
            break snapshot;
        }
        if tokio::time::Instant::now() >= deadline {
            reason = "timeout";
            break snapshot;
        }
        if permit.is_none() {
            let global = app
                .waiting
                .clone()
                .try_acquire_owned()
                .map_err(|_| failure("resourceBusy", "读取等待已满"))?;
            let local = task
                .waiters
                .clone()
                .try_acquire_owned()
                .map_err(|_| failure("resourceBusy", "该任务读取等待已满"))?;
            permit = Some((global, local));
        }
        tokio::select! {
            _=ct.cancelled()=>return Err(failure("requestCancelled","读取等待已取消，远端任务继续运行")),
            _=app.shutdown.cancelled()=>return Err(failure("unavailable","插件正在退出")),
            _=tokio::time::sleep_until(deadline)=>{}
            changed=changes.changed()=>{if changed.is_err(){return Err(failure("unavailable","任务通知已结束"));}}
        }
    };
    drop(permit);
    let wait = options.wait_ms;
    let job = prepare(&app, snapshot, task, options)?;
    let mut value = if job.task.kind == Kind::Exec && job.options.mode != Mode::None {
        // The permit lives inside the blocking job, even if this HTTP future disappears.
        tokio::task::spawn_blocking(move || job.read())
            .await
            .map_err(|e| failure("evidenceReadFailed", e.to_string()))??
    } else {
        job.read()?
    };
    if wait > 0 {
        value["wait"] = json!({"requestedMs":wait,"elapsedMs":started.elapsed().as_millis() as u64,"reason":reason});
    }
    Ok(value)
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
pub fn list(app: &App, p: &Value) -> Result<Value, Value> {
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
    let records = app.records();
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
                .is_some_and(|s| task.target.context["serverId"] != *s)
            || filter
                .request
                .as_ref()
                .is_some_and(|s| task.input["requestId"] != *s)
            || (filter.kind != "all" && filter.kind != task.kind.name())
        {
            continue;
        }
        let value = task.summary();
        let finished = value["done"] == true;
        if finished {
            done += 1;
        } else {
            active += 1;
        }
        if task.order < before && (filter.status == "all" || (filter.status == "done") == finished)
        {
            candidates.push((task.order, value));
        }
    }
    candidates.sort_by_key(|(order, _)| std::cmp::Reverse(*order));
    let has_more = candidates.len() > limit as usize;
    candidates.truncate(limit as usize);
    let next = if has_more {
        Some(base64(
            &serde_json::to_vec(&Cursor {
                v: 1,
                run: app.run.clone(),
                filter,
                ceiling,
                before: candidates.last().unwrap().0,
            })
            .unwrap(),
        ))
    } else {
        None
    };
    Ok(
        json!({"runId":app.run,"tasks":candidates.into_iter().map(|(_,v)|v).collect::<Vec<_>>(),"counts":{"active":active,"done":done},"hasMore":has_more,"nextCursor":next}),
    )
}

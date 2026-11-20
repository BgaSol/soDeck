//! Compatibility and display adapters for the shared command resource service.
use crate::{
    session_process::{ProcessResource as Stream, Reader},
    streams::{Frame, Sink},
    AppCore,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    sync::{atomic::Ordering, Arc, Mutex, Weak},
    time::Duration,
};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Open {
    view_id: String,
    expected_context: crate::workspaces::Context,
    operation_id: String,
    sequence: String,
    command: String,
    max_output_bytes: usize,
    retention: String,
    #[serde(default)]
    timeout_ms: Option<u64>,
    #[serde(default)]
    program_output: Option<String>,
}
fn integer(p: &Value, name: &str) -> Result<u64, String> {
    p[name]
        .as_str()
        .ok_or_else(|| format!("缺少{name}"))?
        .parse()
        .map_err(|_| format!("{name} 无效"))
}
impl AppCore {
    pub(crate) fn stream_request(
        self: &Arc<Self>,
        plugin: &str,
        instance: &str,
        method: &str,
        p: &Value,
    ) -> Result<Value, String> {
        if matches!(method, "session.stream.open" | "session.stream.lookup") {
            let request: Open = serde_json::from_value(p.clone()).map_err(|e| e.to_string())?;
            let value = serde_json::to_value(request).unwrap();
            let result = self
                .process_request(
                    plugin,
                    instance,
                    if method.ends_with("open") {
                        "session.process.start"
                    } else {
                        "session.process.lookup"
                    },
                    &value,
                )
                .map_err(|e| e.message)?;
            if method.ends_with("open") {
                return Ok(
                    json!({"streamId":result["processId"],"state":result["status"]["state"]}),
                );
            }
            return Ok(if result["known"] == true {
                json!({"known":true,"streamId":result["processId"],"status":result["status"]})
            } else {
                json!({"known":false})
            });
        }
        let mut value = json!({"processId":p["streamId"]});
        let name = match method {
            "session.stream.status" => "session.process.status",
            "session.stream.poll" => {
                value["offset"] = p["offset"].clone();
                "session.process.read"
            }
            "session.stream.close" => "session.process.cancel",
            "session.stream.release" => "session.process.release",
            _ => return Err("未知流式命令方法".into()),
        };
        let result = self
            .process_request(plugin, instance, name, &value)
            .map_err(|e| e.message)?;
        Ok(match method {
            "session.stream.close" => {
                json!({"accepted":true,"released":false,"state":result["state"]})
            }
            "session.stream.release" => {
                json!({"accepted":true,"released":true,"state":result["status"]["state"]})
            }
            _ => result,
        })
    }
    pub async fn session_stream_data(
        self: &Arc<Self>,
        window: &str,
        token: &str,
        p: Value,
        sink: Option<Sink>,
    ) -> Result<Value, String> {
        let view = self.ui_data_view(window, token)?;
        let id = p["id"].as_str().ok_or("缺少id")?.to_string();
        let s = self
            .command_processes
            .0
            .lock()
            .unwrap()
            .resources
            .get(&id)
            .cloned()
            .ok_or("流式命令已关闭")?;
        if s.owner.view != view.id
            || s.owner.plugin != view.plugin_id
            || s.owner.instance != view.plugin_instance_id
            || s.release_requested.load(Ordering::Acquire)
        {
            return Err("数据流不属于此页面".into());
        }
        match p["action"].as_str().unwrap_or("") {
            "read" => {
                let start = integer(&p, "offset")?;
                let end = integer(&p, "watermark")?;
                let s = s.clone();
                let data = tauri::async_runtime::spawn_blocking(move || {
                    s.output.capture.lock().unwrap().read(start, 16 * 1024, end)
                })
                .await
                .map_err(|e| e.to_string())??;
                Ok(json!({"data":data,"nextOffset":(start+data.len() as u64).to_string()}))
            }
            "pin" => {
                let _admission = self.surface_write_admission(window, token)?;
                let registry = self.command_processes.0.lock().unwrap();
                let bytes: u64 = registry
                    .resources
                    .values()
                    .filter(|other| other.owner.view == view.id)
                    .map(|other| {
                        if other.request.retention == "fixed" {
                            other.request.max_output_bytes as u64
                        } else {
                            other.output.capture.lock().unwrap().pinned_bytes()
                        }
                    })
                    .sum();
                let start = integer(&p, "start")?;
                let end = integer(&p, "end")?;
                let additional = if s.request.retention == "rolling" {
                    end.saturating_sub(start)
                } else {
                    0
                };
                if end <= start || bytes.saturating_add(additional) > 64 * 1024 * 1024 {
                    return Err("当前页面固定阅读窗口达到64MiB容量".into());
                }
                s.output
                    .capture
                    .lock()
                    .unwrap()
                    .pin(self.next_id(), start, end)
            }
            "unpin" => {
                let _admission = self.surface_write_admission(window, token)?;
                s.output
                    .capture
                    .lock()
                    .unwrap()
                    .unpin(p["pinId"].as_str().ok_or("缺少pinId")?);
                Ok(json!({}))
            }
            "ack" => {
                let reader = s
                    .readers
                    .lock()
                    .unwrap()
                    .get(token)
                    .cloned()
                    .ok_or("数据订阅已失效")?;
                let packet = p["packet"].as_u64().ok_or("缺少packet")?;
                let mut ack = reader.ack.lock().unwrap();
                if packet == ack.1 {
                    return Ok(json!({}));
                }
                if packet != ack.0 || packet != ack.1 + 1 {
                    return Err("数据确认序号无效".into());
                }
                ack.1 = packet;
                reader.changed.notify_one();
                Ok(json!({}))
            }
            "detach" => {
                if let Some(r) = s.readers.lock().unwrap().remove(token) {
                    r.cancel.cancel()
                }
                Ok(json!({}))
            }
            "attach" => {
                let sink = sink.ok_or("缺少正文通道")?;
                let reader = Arc::new(Reader {
                    ack: Mutex::new((0, 0)),
                    changed: Notify::new(),
                    cancel: CancellationToken::new(),
                });
                let offset = p["offset"]
                    .as_str()
                    .map(str::parse::<u64>)
                    .transpose()
                    .map_err(|_| "偏移无效")?
                    .unwrap_or(s.output.capture.lock().unwrap().earliest);
                if !s.output.capture.lock().unwrap().boundary(offset) {
                    return Err("偏移不在完整输出块边界".into());
                }
                let mut readers = s.readers.lock().unwrap();
                if readers.contains_key(token) {
                    return Ok(json!({}));
                }
                let task = s.tasks.enter()?;
                readers.insert(token.into(), reader.clone());
                drop(readers);
                let core = Arc::downgrade(self);
                let window = window.to_owned();
                let token = token.to_owned();
                tauri::async_runtime::spawn(async move {
                    let _task = task;
                    subscribe(core, s, id, window, token, reader, sink, offset).await;
                });
                Ok(json!({}))
            }
            _ => Err("未知正文操作".into()),
        }
    }
}
fn status(s: &Stream) -> Value {
    crate::session_process::status(s)
}
async fn subscribe(
    core: Weak<AppCore>,
    s: Arc<Stream>,
    id: String,
    window: String,
    token: String,
    r: Arc<Reader>,
    sink: Sink,
    mut cursor: u64,
) {
    let initial = s.output.capture.lock().unwrap().produced;
    let mut caught = false;
    let mut first = true;
    'subscription: loop {
        let changed = s.execution.changed.notified();
        if r.cancel.is_cancelled()
            || !core
                .upgrade()
                .is_some_and(|c| c.ui_data_view(&window, &token).is_ok())
        {
            break;
        }
        let (earliest, produced) = {
            let c = s.output.capture.lock().unwrap();
            (c.earliest, c.produced)
        };
        let finished = s.execution.snapshot()["finished"] == true;
        let (kind, data, metadata) = if first {
            first = false;
            (
                "reset",
                vec![],
                json!({"streamId":id,"earliestAvailableOffset":earliest.to_string()}),
            )
        } else if cursor < earliest
            && !s
                .output
                .capture
                .lock()
                .unwrap()
                .available(cursor, (cursor + 1).min(produced))
        {
            cursor = earliest;
            (
                "gap",
                vec![],
                json!({"streamId":id,"message":"较早输出已退出最近缓冲"}),
            )
        } else if !caught && cursor >= initial {
            caught = true;
            ("caughtUp", vec![], json!({"streamId":id}))
        } else if cursor < produced {
            let output = s.clone();
            let start = cursor;
            let result = tauri::async_runtime::spawn_blocking(move || {
                output
                    .output
                    .capture
                    .lock()
                    .unwrap()
                    .read(start, 16 * 1024, produced)
            })
            .await;
            match result {
                Ok(Ok(data)) => {
                    cursor += data.len() as u64;
                    ("data", data, json!({"streamId":id}))
                }
                _ => {
                    cursor = s.output.capture.lock().unwrap().earliest;
                    (
                        "gap",
                        vec![],
                        json!({"streamId":id,"message":"输出保留范围已变化"}),
                    )
                }
            }
        } else if finished {
            ("end", vec![], json!({"streamId":id,"status":status(&s)}))
        } else {
            tokio::select! {_=changed=>{},_=r.cancel.cancelled()=>break,_=tokio::time::sleep(Duration::from_secs(2))=>{}};
            continue;
        };
        let packet = {
            let mut a = r.ack.lock().unwrap();
            a.0 += 1;
            a.0
        };
        if sink(Frame {
            kind,
            packet,
            offset: cursor,
            data,
            metadata,
        })
        .is_err()
        {
            break;
        }
        loop {
            let wake = r.changed.notified();
            if r.ack.lock().unwrap().1 == packet {
                break;
            }
            tokio::select! {_=wake=>{},_=r.cancel.cancelled()=>break 'subscription,_=tokio::time::sleep(Duration::from_secs(2))=>{if !core.upgrade().is_some_and(|c|c.ui_data_view(&window,&token).is_ok()){r.cancel.cancel();break 'subscription}}}
        }
        if kind == "end" {
            break;
        }
    }
    let mut readers = s.readers.lock().unwrap();
    if readers
        .get(&token)
        .is_some_and(|current| Arc::ptr_eq(current, &r))
    {
        readers.remove(&token);
    }
}

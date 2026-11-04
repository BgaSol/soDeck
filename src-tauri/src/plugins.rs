use crate::{
    core::{now, AppCore},
    model::*,
    platform, API_VERSION, TARGET,
};
use serde_json::{json, Value};
use std::{
    fs,
    io::{self, BufRead, BufReader, BufWriter, Write},
    path::{Component, Path},
    process::ChildStdin,
    sync::{
        mpsc::{self, Receiver, Sender, SyncSender},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

pub fn read_manifest(directory: &Path) -> Result<PluginRecord, String> {
    let directory = directory
        .canonicalize()
        .map_err(|error| format!("无法打开插件目录：{error}"))?;
    let path = directory.join("plugin.json");
    let metadata = fs::metadata(&path).map_err(|error| format!("找不到 plugin.json：{error}"))?;
    if metadata.len() > 64 * 1024 {
        return Err("插件清单超过 64 KiB".into());
    }
    let manifest: Manifest =
        serde_json::from_slice(&fs::read(&path).map_err(|error| error.to_string())?)
            .map_err(|error| format!("插件清单格式错误：{error}"))?;
    if manifest.id.is_empty()
        || manifest.id.len() > 128
        || !manifest.id.starts_with(|c: char| c.is_ascii_alphanumeric())
        || !manifest
            .id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        return Err("插件 ID 只能包含字母、数字、点、下划线或短横线，并以字母或数字开头".into());
    }
    if manifest.name.trim().is_empty() || manifest.name.len() > 256 {
        return Err("插件名称不能为空或过长".into());
    }
    let version = manifest.version.split('-').next().unwrap_or_default();
    if version.split('.').count() != 3
        || !version.split('.').all(|part| part.parse::<u32>().is_ok())
    {
        return Err("插件版本应为 major.minor.patch".into());
    }
    if manifest.api_version.split('.').next() != Some(API_VERSION) {
        return Err(format!(
            "插件要求核心 API {}，当前为 {API_VERSION}",
            manifest.api_version
        ));
    }
    if manifest.target != TARGET {
        return Err(format!("插件适用 {}，当前平台为 {TARGET}", manifest.target));
    }
    let relative = Path::new(&manifest.entry);
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|p| !matches!(p, Component::Normal(_)))
    {
        return Err("插件入口必须是插件目录内的相对路径".into());
    }
    let entry = directory
        .join(relative)
        .canonicalize()
        .map_err(|error| format!("插件入口不存在：{error}"))?;
    if !entry.starts_with(&directory) || !entry.is_file() {
        return Err("插件入口必须是插件目录内的文件".into());
    }
    Ok(PluginRecord {
        directory,
        manifest,
    })
}

pub(crate) fn read_frame(reader: &mut impl BufRead, limit: usize) -> io::Result<Option<Vec<u8>>> {
    let mut bytes = Vec::new();
    let mut oversized = false;
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            if oversized {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "单行数据超过长度上限",
                ));
            }
            return Ok((!bytes.is_empty()).then_some(bytes));
        }
        let length = available
            .iter()
            .position(|b| *b == b'\n')
            .map_or(available.len(), |i| i + 1);
        let finished = available[length - 1] == b'\n';
        if !oversized {
            if bytes.len() + length > limit {
                oversized = true;
                bytes.clear();
            } else {
                bytes.extend_from_slice(&available[..length]);
            }
        }
        reader.consume(length);
        if finished {
            if oversized {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "单行数据超过长度上限",
                ));
            }
            return Ok(Some(bytes));
        }
    }
}

pub(crate) enum Control {
    Stop(Sender<Result<(), String>>),
}
enum Output {
    Frame(Value),
    Diagnostic(String),
    Broken(String),
}

pub(crate) struct Instance {
    view: Mutex<PluginView>,
    control: Sender<Control>,
}

impl Instance {
    pub fn new(
        record: PluginRecord,
        instance: String,
    ) -> (
        Arc<Self>,
        Receiver<Control>,
        Sender<Result<(), String>>,
        Receiver<Result<(), String>>,
    ) {
        let (sender, receiver) = mpsc::channel();
        let (ready, ready_rx) = mpsc::channel();
        (
            Arc::new(Self {
                view: Mutex::new(PluginView {
                    directory: record.directory,
                    manifest: record.manifest,
                    status: PluginStatus::Starting,
                    instance_id: Some(instance),
                    pid: None,
                    started_at: None,
                    error: None,
                    cleanup_complete: false,
                }),
                control: sender,
            }),
            receiver,
            ready,
            ready_rx,
        )
    }
    pub fn view(&self) -> PluginView {
        self.view.lock().unwrap().clone()
    }
    pub fn stop(&self) -> Result<Receiver<Result<(), String>>, String> {
        let (sender, receiver) = mpsc::channel();
        let mut view = self.view.lock().unwrap();
        if view.cleanup_complete {
            let _ = sender.send(Ok(()));
            return Ok(receiver);
        }
        view.status = PluginStatus::Stopping;
        self.control
            .send(Control::Stop(sender))
            .map_err(|_| "插件管理进程不可用，请查看日志".to_string())?;
        Ok(receiver)
    }
    fn update(&self, core: &AppCore, action: impl FnOnce(&mut PluginView)) {
        action(&mut self.view.lock().unwrap());
        core.changed();
    }
}

fn write_frame(writer: &mut BufWriter<ChildStdin>, frame: &Value) -> Result<(), String> {
    serde_json::to_writer(&mut *writer, frame).map_err(|error| error.to_string())?;
    writer
        .write_all(b"\n")
        .and_then(|_| writer.flush())
        .map_err(|error| format!("插件管道写入失败：{error}"))
}

fn queue_frame(writer: &SyncSender<Value>, frame: Value) -> Result<(), String> {
    writer
        .try_send(frame)
        .map_err(|_| "插件未及时读取核心响应，已停止通信".into())
}

fn spawn_reader(
    output: impl io::Read + Send + 'static,
    sender: SyncSender<Output>,
    diagnostic: bool,
) {
    thread::spawn(move || {
        let mut reader = BufReader::new(output);
        loop {
            let item = match read_frame(&mut reader, if diagnostic { 8192 } else { 65536 }) {
                Ok(Some(bytes)) if diagnostic => {
                    Output::Diagnostic(String::from_utf8_lossy(&bytes).trim_end().into())
                }
                Ok(Some(bytes)) => match serde_json::from_slice(&bytes) {
                    Ok(value) => Output::Frame(value),
                    Err(error) => {
                        let _ = sender.send(Output::Broken(format!(
                            "插件标准输出不是有效 RPC JSON：{error}"
                        )));
                        break;
                    }
                },
                Ok(None) => {
                    if !diagnostic {
                        let _ = sender.send(Output::Broken("插件通信管道已关闭".into()));
                    }
                    break;
                }
                Err(error) if diagnostic && error.kind() == io::ErrorKind::InvalidData => {
                    Output::Diagnostic("诊断日志行过长，已省略".into())
                }
                Err(error) => {
                    let _ = sender.send(Output::Broken(format!("读取插件管道失败：{error}")));
                    break;
                }
            };
            if sender.send(item).is_err() {
                break;
            }
        }
    });
}

fn plugin_request(
    core: &AppCore,
    id: &str,
    instance: &str,
    frame: &Value,
) -> Result<Value, String> {
    match frame["method"].as_str().unwrap_or_default() {
        "logging.write" => {
            let params = &frame["params"];
            let level = params["level"].as_str().ok_or("缺少日志级别")?;
            let message = params["message"].as_str().ok_or("缺少日志正文")?;
            let accepted =
                core.plugin_log(id, instance, level, message, params.get("fields").cloned())?;
            Ok(json!({"accepted":accepted}))
        }
        "messages.publish" => {
            let input: PublishMessage = serde_json::from_value(frame["params"].clone())
                .map_err(|error| error.to_string())?;
            let message = core.publish(Some((id, instance)), input)?;
            Ok(json!({"id":message.id}))
        }
        _ => Err("未知核心 API 方法".into()),
    }
}

pub(crate) fn supervise(
    core: Arc<AppCore>,
    instance: Arc<Instance>,
    control: Receiver<Control>,
    ready: Sender<Result<(), String>>,
) {
    let initial = instance.view();
    let id = initial.manifest.id.as_str();
    let instance_id = initial.instance_id.as_deref().unwrap();
    core.log(Some((id, instance_id)), "info", "正在启动插件", None);
    let entry = initial.directory.join(&initial.manifest.entry);
    let started = platform::spawn(&entry, &initial.directory)
        .map_err(|error| format!("无法启动插件程序：{error}"));
    let (mut child, group) = match started {
        Ok(value) => value,
        Err(error) => {
            instance.update(&core, |v| {
                v.status = PluginStatus::Failed;
                v.error = Some(error.clone());
                v.cleanup_complete = true;
            });
            core.plugin_failure(id, instance_id, &error);
            let _ = ready.send(Err(error));
            return;
        }
    };
    instance.update(&core, |v| {
        v.pid = Some(child.id());
        v.started_at = Some(now());
    });
    let (output_tx, output_rx) = mpsc::sync_channel(256);
    spawn_reader(child.stdout.take().unwrap(), output_tx.clone(), false);
    spawn_reader(child.stderr.take().unwrap(), output_tx, true);
    let (writer, outgoing) = mpsc::sync_channel(16);
    let mut stdin = BufWriter::new(child.stdin.take().unwrap());
    thread::spawn(move || {
        while let Ok(frame) = outgoing.recv() {
            if write_frame(&mut stdin, &frame).is_err() {
                break;
            }
        }
    });
    let mut failure=core.plugin_data(id).and_then(|path| queue_frame(&writer,json!({"jsonrpc":"2.0","id":"initialize","method":"initialize","params":{"coreApiVersion":API_VERSION,"dataDirectory":path}}))).err();
    let mut ready = Some(ready);
    let mut initialized = false;
    let init_deadline = Instant::now() + Duration::from_secs(5);
    let mut stop_deadline = None;
    let mut stop_waiters = Vec::new();
    let mut forced = false;
    let mut exited_at: Option<Instant> = None;
    loop {
        while let Ok(Control::Stop(waiter)) = control.try_recv() {
            stop_waiters.push(waiter);
            if stop_deadline.is_none() {
                stop_deadline = Some(Instant::now() + Duration::from_secs(3));
                core.log(Some((id, instance_id)), "info", "正在卸载插件", None);
                let _ = queue_frame(
                    &writer,
                    json!({"jsonrpc":"2.0","id":"shutdown","method":"shutdown","params":{}}),
                );
            }
        }
        if !initialized && stop_deadline.is_none() && Instant::now() >= init_deadline {
            failure = Some("插件初始化超过 5 秒".into());
        }
        let exited = match child.try_wait() {
            Ok(status) => status,
            Err(error) => {
                failure = Some(format!("无法读取插件退出状态：{error}"));
                None
            }
        };
        if let Some(status) = &exited {
            exited_at.get_or_insert_with(Instant::now);
            if stop_deadline.is_none() && failure.is_none() {
                failure = Some(format!("插件意外退出：{status}"));
            }
        }
        let expired = stop_deadline.is_some_and(|deadline| Instant::now() >= deadline);
        // Allow the pipe readers to deliver final diagnostics after process exit.
        let drain_complete = exited_at.is_none_or(|at| at.elapsed() >= Duration::from_millis(150));
        if (failure.is_some() || exited.is_some() || expired) && drain_complete {
            if expired && exited.is_none() {
                forced = true;
            }
            let cleanup = group.terminate();
            let deadline = Instant::now() + Duration::from_secs(2);
            let mut ended = exited.is_some();
            while !ended && Instant::now() < deadline {
                ended = child.try_wait().is_ok_and(|status| status.is_some());
                if !ended {
                    thread::sleep(Duration::from_millis(10));
                }
            }
            if ended && cleanup.is_ok() {
                let outcome = if let Some(reason) = &failure {
                    instance.update(&core, |v| {
                        v.status = PluginStatus::Failed;
                        v.error = Some(reason.clone());
                        v.cleanup_complete = true;
                        v.pid = None;
                    });
                    core.plugin_failure(id, instance_id, reason);
                    Err(reason.clone())
                } else {
                    instance.update(&core, |v| {
                        v.status = PluginStatus::Unloaded;
                        v.error = None;
                        v.cleanup_complete = true;
                        v.pid = None;
                    });
                    core.log(
                        Some((id, instance_id)),
                        if forced { "warn" } else { "info" },
                        if forced {
                            "插件未及时退出，已终止进程及其子进程"
                        } else {
                            "插件已卸载，进程与资源已释放"
                        },
                        None,
                    );
                    Ok(())
                };
                if let Some(sender) = ready.take() {
                    let _ =
                        sender.send(outcome.clone().and(Err("插件在初始化完成前已停止".into())));
                }
                for waiter in stop_waiters.drain(..) {
                    let _ = waiter.send(if failure.is_some() && stop_deadline.is_none() {
                        Ok(())
                    } else {
                        outcome.clone()
                    });
                }
                return;
            }
            let reason = format!(
                "插件退出尚未确认，不能重新加载：{}",
                cleanup
                    .err()
                    .map_or("进程仍在退出".into(), |e| e.to_string())
            );
            instance.update(&core, |v| {
                v.status = PluginStatus::Failed;
                v.error = Some(reason.clone());
            });
            if let Some(sender) = ready.take() {
                let _ = sender.send(Err(reason.clone()));
            }
            for waiter in stop_waiters.drain(..) {
                let _ = waiter.send(Err(reason.clone()));
            }
            // Retain ownership until a subsequent stop or OS exit makes cleanup possible.
            failure = None;
            stop_deadline = Some(Instant::now() + Duration::from_secs(3));
            thread::sleep(Duration::from_millis(50));
            continue;
        }
        match output_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(Output::Diagnostic(text)) => {
                let _ = core.plugin_log(
                    id,
                    instance_id,
                    "info",
                    &text,
                    Some(json!({"stream":"stderr"})),
                );
            }
            Ok(Output::Broken(reason)) => {
                if stop_deadline.is_none() && failure.is_none() {
                    failure = Some(reason);
                }
            }
            Ok(Output::Frame(frame)) => {
                if frame["jsonrpc"] != "2.0" {
                    failure = Some("插件 RPC 必须使用 JSON-RPC 2.0".into());
                    continue;
                }
                if frame.get("method").is_some() {
                    let result = if initialized {
                        plugin_request(&core, id, instance_id, &frame)
                    } else {
                        Err("插件尚未完成初始化".into())
                    };
                    if let Some(request_id) = frame.get("id") {
                        let response = match result {
                            Ok(result) => json!({"jsonrpc":"2.0","id":request_id,"result":result}),
                            Err(message) => {
                                json!({"jsonrpc":"2.0","id":request_id,"error":{"code":if message=="未知核心 API 方法"{-32601}else{-32602},"message":message}})
                            }
                        };
                        if let Err(error) = queue_frame(&writer, response) {
                            if stop_deadline.is_none() {
                                failure = Some(error);
                            }
                        }
                    }
                } else if frame["id"] == "initialize" && !initialized {
                    if frame.get("error").is_some() {
                        failure = Some(format!("插件初始化失败：{}", frame["error"]));
                    } else if frame["result"]["apiVersion"] != API_VERSION {
                        failure = Some("插件初始化响应的 API 版本不匹配".into());
                    } else {
                        initialized = true;
                        if stop_deadline.is_none() {
                            instance.update(&core, |v| v.status = PluginStatus::Running);
                        }
                        core.log(Some((id, instance_id)), "info", "插件初始化完成", None);
                        if let Some(sender) = ready.take() {
                            let _ = sender.send(Ok(()));
                        }
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => (),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                if stop_deadline.is_none() {
                    failure = Some("插件输出管道已断开".into());
                }
            }
        }
    }
}

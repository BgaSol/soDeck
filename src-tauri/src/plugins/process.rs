use super::{
    dispatch::Dispatcher,
    rpc::{self, queue_frame, spawn_reader, RpcError},
    Control, Instance, Output,
};
use crate::{
    core::{now, AppCore},
    model::PluginStatus,
    platform, API_VERSION,
};
use serde_json::{json, Value};
use std::{
    sync::{
        mpsc::{self, Sender},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

pub(crate) fn supervise(
    core: Arc<AppCore>,
    instance: Arc<Instance>,
    control: Control,
    ready: Sender<Result<(), String>>,
) {
    let initial = instance.view();
    let id = initial.manifest.id.as_str();
    let instance_id = initial.instance_id.as_deref().unwrap();
    core.log(Some((id, instance_id)), "info", "正在启动插件", None);
    let entry = initial.directory.join(&initial.manifest.entry);
    // Process creation is part of initialization. In particular, Windows may spend
    // seconds starting a newly built executable before the protocol loop begins.
    let init_deadline = Instant::now() + Duration::from_secs(5);
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
    let writer = rpc::spawn_writer(child.stdin.take().unwrap());
    let mut dispatcher = Dispatcher::new(Arc::downgrade(&core), id.into(), instance_id.into());
    let mut failure = core.plugin_data(id).and_then(|path| queue_frame(&writer, json!({
        "jsonrpc":"2.0", "id":"initialize", "method":"initialize", "params":{
            "coreApiVersion":API_VERSION, "coreVersion":core.version(), "dataDirectory":path,
            "capabilities":["logging.write","messages.publish","ui.v1","rpc.concurrent"]
        }
    }))).err();
    let mut ready = Some(ready);
    let mut initialized = false;
    let mut stop_deadline = None;
    let mut stop_waiters = Vec::new();
    let mut forced = false;
    let mut exited_at: Option<Instant> = None;
    let mut sequence = 0_u64;
    let mut requests: std::collections::HashMap<String, (Instant, Sender<Result<Value, String>>)> =
        std::collections::HashMap::new();
    loop {
        while let Ok(waiter) = control.stops.try_recv() {
            stop_waiters.push(waiter);
            if stop_deadline.is_none() {
                stop_deadline = Some(Instant::now() + Duration::from_secs(3));
                for (_, (_, reply)) in requests.drain() {
                    let _ = reply.send(Err("插件正在停止，请求已取消".into()));
                }
                core.log(Some((id, instance_id)), "info", "正在卸载插件", None);
                if let Err(error) = queue_frame(
                    &writer,
                    json!({"jsonrpc":"2.0","id":"shutdown","method":"shutdown","params":{}}),
                ) {
                    failure = Some(error);
                }
            }
        }
        for _ in 0..64 {
            let Ok(call) = control.calls.try_recv() else {
                break;
            };
            let super::Call {
                method,
                params,
                reply,
                deadline,
            } = call;
            if Instant::now() >= deadline {
                let _ = reply.send(Err("插件请求已超时".into()));
            } else if stop_deadline.is_some() || !initialized || requests.len() >= 64 {
                let _ = reply.send(Err("插件忙碌或正在停止".into()));
            } else {
                sequence += 1;
                let key = format!("ui-{sequence}");
                match queue_frame(
                    &writer,
                    json!({"jsonrpc":"2.0","id":key,"method":method,"params":params}),
                ) {
                    Ok(()) => {
                        requests.insert(key, (deadline, reply));
                    }
                    Err(e) => {
                        let _ = reply.send(Err(e));
                    }
                }
            }
        }
        for frame in dispatcher.drain() {
            if let Err(error) = queue_frame(&writer, frame) {
                failure = Some(error);
            }
        }
        requests.retain(|_, (deadline, reply)| {
            if Instant::now() >= *deadline {
                let _ = reply.send(Err("插件界面请求超时".into()));
                false
            } else {
                true
            }
        });
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
            for (_, (_, reply)) in requests.drain() {
                let _ = reply.send(Err("插件通信已停止，请求已取消".into()));
            }
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
                    if !frame["method"].is_string()
                        || frame
                            .get("id")
                            .is_some_and(|id| !id.is_string() && !id.is_number())
                    {
                        failure = Some("插件请求方法或请求 ID 无效".into());
                        continue;
                    }
                    let request_id = frame.get("id").cloned();
                    let result = if initialized {
                        dispatcher.submit(frame)
                    } else {
                        Err(RpcError::execution("插件尚未完成初始化"))
                    };
                    if let (Some(request_id), Err(error)) = (request_id, result) {
                        if let Err(error) = queue_frame(&writer, error.response(request_id)) {
                            if stop_deadline.is_none() {
                                failure = Some(error);
                            }
                        }
                    }
                } else if let Some((_, reply)) =
                    frame["id"].as_str().and_then(|key| requests.remove(key))
                {
                    let result = if frame.get("error").is_some() {
                        Err(format!("{}", frame["error"]))
                    } else {
                        Ok(frame["result"].clone())
                    };
                    let _ = reply.send(result);
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

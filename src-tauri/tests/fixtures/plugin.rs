use serde_json::{json, Value};
use std::{
    fs,
    io::{self, BufRead, Write},
    process::Command,
    thread,
    time::Duration,
};

fn send(value: Value) {
    let mut out = io::stdout().lock();
    serde_json::to_writer(&mut out, &value).unwrap();
    writeln!(out).unwrap();
    out.flush().unwrap();
}
fn main() {
    if std::env::args().any(|a| a == "--child") {
        loop {
            thread::sleep(Duration::from_millis(100));
        }
    }
    let mode = fs::read_to_string("mode").unwrap_or_default();
    if mode == "no-read" {
        loop {
            thread::sleep(Duration::from_secs(1));
        }
    }
    let mut data = None;
    let mut ui_views: std::collections::HashMap<String, Value> = std::collections::HashMap::new();
    let mut pending_reentry = None;
    let mut pending_terminal = None;
    let mut pending_sessions = std::collections::HashMap::<String, Value>::new();
    let mut pending_actions = std::collections::HashMap::<String, (Value, bool)>::new();
    let mut action_count = 0;
    let mut deferred = None;
    let mut probes: Option<(Value, std::collections::HashMap<String, Value>)> = None;
    for line in io::stdin().lock().lines() {
        let frame: Value = serde_json::from_str(&line.unwrap()).unwrap();
        match frame["method"].as_str() {
            Some("state.changed") => {
                let p = &frame["params"];
                fs::write(
                    format!("{}/shared-frame", data.as_ref().unwrap()),
                    p.to_string(),
                )
                .unwrap();
                send(
                    json!({"jsonrpc":"2.0","id":frame["id"],"result":{"accepted":true,"batchId":p["batchId"],"part":p["part"],"appliedRevision":p["toRevision"]}}),
                );
            }
            Some("ui.action") => {
                action_count += 1;
                fs::write(
                    format!("{}/action-count", data.as_ref().unwrap()),
                    action_count.to_string(),
                )
                .unwrap();
                let p = &frame["params"];
                if p["data"]["none"] == true {
                    send(json!({"jsonrpc":"2.0","id":frame["id"],"result":{}}));
                    continue;
                }
                if let Some(view) = p["data"].get("returnViewId") {
                    send(json!({"jsonrpc":"2.0","id":frame["id"],"result":{"viewId":view}}));
                    continue;
                }
                if p["data"]["fail"] == true {
                    send(
                        json!({"jsonrpc":"2.0","id":frame["id"],"error":{"code":-32000,"message":"action failed"}}),
                    );
                    continue;
                }
                let request = format!("action-open-{action_count}");
                pending_actions.insert(
                    request.clone(),
                    (frame["id"].clone(), p["data"]["failAfterOpen"] == true),
                );
                let open = json!({"jsonrpc":"2.0","id":request,"method":"ui.openView","params":{"expectedContext":p["context"],"requestId":p["requestId"],"viewType":p["data"]["viewType"].as_str().unwrap_or("terminal"),"params":p["data"]["params"]}});
                let delay = p["data"]["delayMs"].as_u64().unwrap_or(0);
                thread::spawn(move || {
                    thread::sleep(Duration::from_millis(delay));
                    send(open);
                });
                continue;
            }
            Some(method) if method.starts_with("ui.") => {
                let p = &frame["params"];
                let id = p["viewId"].as_str().unwrap();
                if method == "ui.event" && p["data"]["rpc"] == "reentry" {
                    pending_reentry = Some(frame["id"].clone());
                    send(
                        json!({"jsonrpc":"2.0","id":"fixture-reentry","method":"messages.publish","params":{"title":"RPC reentry","body":"Call this plugin while this request is running","level":"info","notify":true}}),
                    );
                    continue;
                }
                if method == "ui.event" {
                    if let Some(rpc) = p["data"]["rpc"]
                        .as_str()
                        .filter(|s| s.starts_with("session.") || s.starts_with("state."))
                    {
                        let request = format!("session-{}", pending_sessions.len());
                        pending_sessions.insert(request.clone(), frame["id"].clone());
                        let mut params = p["data"]["params"].clone();
                        if rpc.starts_with("state.") {
                            params["viewId"] = json!(id);
                            params["expectedContext"] = p["context"].clone();
                        }
                        if rpc == "session.state" || rpc == "session.exec.start" {
                            params["viewId"] = json!(id);
                        }
                        send(json!({"jsonrpc":"2.0","id":request,"method":rpc,"params":params}));
                        continue;
                    }
                    if p["data"]["rpc"] == "terminal.open" {
                        pending_terminal = Some(frame["id"].clone());
                        send(
                            json!({"jsonrpc":"2.0","id":"fixture-terminal","method":"terminal.open","params":{"viewId":id,"cols":80,"rows":24}}),
                        );
                        continue;
                    }
                    match p["data"]["rpc"].as_str() {
                        Some("defer") => {
                            deferred = Some(frame["id"].clone());
                            fs::write(format!("{}/deferred", data.as_ref().unwrap()), "ready")
                                .unwrap();
                            continue;
                        }
                        Some("release") => {
                            send(
                                json!({"jsonrpc":"2.0","id":frame["id"],"result":{"data":{"order":2}}}),
                            );
                            if let Some(id) = deferred.take() {
                                send(
                                    json!({"jsonrpc":"2.0","id":id,"result":{"data":{"order":1}}}),
                                );
                            }
                            continue;
                        }
                        Some("ignore") => continue,
                        Some("exit") => std::process::exit(23),
                        Some("probe") => {
                            probes = Some((frame["id"].clone(), std::collections::HashMap::new()));
                            send(
                                json!({"jsonrpc":"2.0","id":"probe-invalid","method":"logging.write","params":{"level":"banana","message":"invalid"}}),
                            );
                            send(
                                json!({"jsonrpc":"2.0","id":"probe-unknown","method":"missing.method","params":{}}),
                            );
                            send(
                                json!({"jsonrpc":"2.0","id":"probe-storage","method":"messages.publish","params":{"title":"probe storage","body":"test","level":"info"}}),
                            );
                            continue;
                        }
                        _ => (),
                    }
                }
                if method == "ui.getView" && !ui_views.contains_key(id) {
                    send(
                        json!({"jsonrpc":"2.0","id":frame["id"],"error":{"code":-1,"message":"view does not exist"}}),
                    );
                    continue;
                }
                if method == "ui.createView" && !ui_views.contains_key(id) {
                    fs::write(
                        format!("{}/created-{id}", data.as_ref().unwrap()),
                        p.to_string(),
                    )
                    .unwrap();
                }
                let v = ui_views
                    .entry(id.into())
                    .or_insert(json!({"text":"","dirty":false}));
                let mut error = None;
                match method {
                    "ui.event" => {
                        *v = json!({"text":p["data"]["text"],"dirty":true});
                    }
                    "ui.save" if v["text"] == "fail" => error = Some("save failed"),
                    "ui.save" => v["dirty"] = json!(false),
                    _ => (),
                }
                let result = json!({"data":v,"metadata":{"dirty":v["dirty"],"busy":false}});
                if method == "ui.disposeView"
                    && matches!(
                        v["text"].as_str(),
                        Some("dispose-late" | "dispose-fail-once")
                    )
                {
                    let directory = data.as_ref().unwrap();
                    let count_path = format!("{directory}/dispose-count-{id}");
                    let count = fs::read_to_string(&count_path)
                        .ok()
                        .and_then(|v| v.parse::<usize>().ok())
                        .unwrap_or(0)
                        + 1;
                    fs::write(count_path, count.to_string()).unwrap();
                    if v["text"] == "dispose-fail-once" && count == 1 {
                        send(
                            json!({"jsonrpc":"2.0","id":frame["id"],"error":{"code":-1,"message":"dispose failed once"}}),
                        );
                        continue;
                    }
                    if v["text"] == "dispose-late" {
                        let release = format!("{directory}/dispose-release-{id}");
                        let request = frame["id"].clone();
                        ui_views.remove(id);
                        thread::spawn(move || {
                            while !std::path::Path::new(&release).exists() {
                                thread::sleep(Duration::from_millis(10));
                            }
                            send(json!({"jsonrpc":"2.0","id":request,"result":result}));
                        });
                        continue;
                    }
                }
                if method == "ui.mount" {
                    fs::write(
                        format!("{}/mounted-{}", data.as_ref().unwrap(), id),
                        p.to_string(),
                    )
                    .unwrap();
                }
                if method == "ui.disposeView" {
                    ui_views.remove(id);
                }
                send(if let Some(message) = error {
                    json!({"jsonrpc":"2.0","id":frame["id"],"error":{"code":-1,"message":message}})
                } else {
                    json!({"jsonrpc":"2.0","id":frame["id"],"result":result})
                });
            }
            Some("initialize") => {
                let directory = frame["params"]["dataDirectory"].as_str().unwrap();
                data = Some(directory.to_owned());
                fs::write(
                    format!("{directory}/initialize.json"),
                    frame["params"].to_string(),
                )
                .unwrap();
                fs::write(format!("{directory}/pid"), std::process::id().to_string()).unwrap();
                if mode == "timeout" {
                    continue;
                }
                if mode == "settings-init" {
                    send(
                        json!({"jsonrpc":"2.0","id":"settings-init","method":"settings.get","params":{"pageId":"appearance"}}),
                    );
                    continue;
                }
                if mode == "init-error" {
                    send(
                        json!({"jsonrpc":"2.0","id":frame["id"],"error":{"code":-1,"message":"fixture init failure"}}),
                    );
                    continue;
                }
                if mode == "child" {
                    let child = Command::new(std::env::current_exe().unwrap())
                        .arg("--child")
                        .spawn()
                        .unwrap();
                    fs::write(format!("{directory}/child-pid"), child.id().to_string()).unwrap();
                }
                send(
                    json!({"jsonrpc":"2.0","id":frame["id"],"result":{"apiVersion":"1","uiApiVersion":"2","uiActionsApiVersion":if mode=="old-actions" {Value::Null} else {json!("1")}}}),
                );
                eprintln!("fixture diagnostic");
                send(
                    json!({"jsonrpc":"2.0","id":1,"method":"logging.write","params":{"level":"error","message":"fixture log"}}),
                );
                send(
                    json!({"jsonrpc":"2.0","id":2,"method":"messages.publish","params":{"title":"Fixture","body":"persisted","level":"info","notify":true,"dedupeKey":"fixture"}}),
                );
                if mode == "oversized" {
                    println!("{}", "x".repeat(70000));
                }
                if mode == "flood" {
                    for i in 0..5000 {
                        send(
                            json!({"jsonrpc":"2.0","method":"logging.write","params":{"level":"info","message":format!("flood {i}")}}),
                        );
                    }
                }
                if mode == "write-flood" {
                    for i in 0..5000 {
                        send(
                            json!({"jsonrpc":"2.0","id":i+100,"method":"logging.write","params":{"level":"info","message":"full stdin"}}),
                        );
                    }
                    loop {
                        thread::sleep(Duration::from_secs(1));
                    }
                }
            }
            Some("settings.changed") => {
                fs::write(
                    format!("{}/settings-changed.json", data.as_ref().unwrap()),
                    frame["params"].to_string(),
                )
                .unwrap();
                send(json!({"jsonrpc":"2.0","id":frame["id"],"result":{"accepted":true}}));
            }
            Some("shutdown") => {
                if mode == "ignore-stop" || mode == "child" {
                    continue;
                }
                send(
                    json!({"jsonrpc":"2.0","id":3,"method":"logging.write","params":{"level":"info","message":"fixture stopping"}}),
                );
            }
            _ if frame["id"] == "settings-init" => {
                fs::write(
                    format!("{}/settings-init.json", data.as_ref().unwrap()),
                    frame.to_string(),
                )
                .unwrap();
                send(
                    json!({"jsonrpc":"2.0","id":"initialize","result":{"apiVersion":"1","uiApiVersion":"2","settingsApiVersion":"1","uiActionsApiVersion":if mode=="old-actions" {Value::Null} else {json!("1")}}}),
                );
            }
            _ if frame["id"]
                .as_str()
                .is_some_and(|id| pending_actions.contains_key(id)) =>
            {
                let (id, fail) = pending_actions
                    .remove(frame["id"].as_str().unwrap())
                    .unwrap();
                send(if frame.get("error").is_some() {
                    json!({"jsonrpc":"2.0","id":id,"error":frame["error"]})
                } else if fail {
                    json!({"jsonrpc":"2.0","id":id,"error":{"code":-32000,"message":"failed after page creation"}})
                } else {
                    json!({"jsonrpc":"2.0","id":id,"result":frame["result"]})
                });
            }
            _ if frame["id"]
                .as_str()
                .is_some_and(|id| pending_sessions.contains_key(id)) =>
            {
                let id = pending_sessions
                    .remove(frame["id"].as_str().unwrap())
                    .unwrap();
                let data = if frame.get("error").is_some() {
                    json!({"rpcError":frame["error"]})
                } else {
                    frame["result"].clone()
                };
                send(json!({"jsonrpc":"2.0","id":id,"result":{"data":data}}));
            }
            _ if frame["id"] == "fixture-terminal" => {
                if let Some(id) = pending_terminal.take() {
                    send(if frame.get("error").is_some() {
                        json!({"jsonrpc":"2.0","id":id,"error":frame["error"]})
                    } else {
                        json!({"jsonrpc":"2.0","id":id,"result":{"data":frame["result"]}})
                    });
                }
            }
            _ if frame["id"] == "fixture-reentry" => {
                if let Some(id) = pending_reentry.take() {
                    send(
                        json!({"jsonrpc":"2.0","id":id,"result":{"data":{"reentered":true},"metadata":{"dirty":false,"busy":false}}}),
                    );
                }
            }
            _ if frame["id"]
                .as_str()
                .is_some_and(|id| id.starts_with("probe-")) =>
            {
                if let Some((request, replies)) = &mut probes {
                    replies.insert(frame["id"].as_str().unwrap().into(), frame.clone());
                    if replies.len() == 3 {
                        send(json!({"jsonrpc":"2.0","id":request,"result":{"data":replies}}));
                        probes = None;
                    }
                }
            }
            _ if frame["id"] == 3 => {
                send(json!({"jsonrpc":"2.0","id":"shutdown","result":{}}));
                return;
            }
            _ if frame["id"] == 2 && mode == "crash" => std::process::exit(23),
            _ => (),
        }
        if let Some(directory) = &data {
            let _ = fs::write(format!("{directory}/last-read"), "ok");
        }
    }
}

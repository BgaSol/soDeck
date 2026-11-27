use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use std::{
    fs,
    io::{self, BufRead, Write},
    process::Command,
    thread,
    time::{Duration, Instant},
};

fn send(value: Value) {
    let mut out = io::stdout().lock();
    serde_json::to_writer(&mut out, &value).unwrap();
    writeln!(out).unwrap();
    out.flush().unwrap();
}
struct Consume {
    process: Value,
    offset: u64,
    bytes: u64,
    pages: u64,
    started: Instant,
    first: Option<Instant>,
    finished: bool,
    error: Value,
}
impl Consume {
    fn metrics(&self) -> Value {
        json!({"finished":self.finished,"error":self.error,"bytes":self.bytes,"pages":self.pages,"seconds":self.first.unwrap_or(self.started).elapsed().as_secs_f64()})
    }
}
fn poll_consume(job: &Consume, wait: bool) {
    let request = json!({"jsonrpc":"2.0","id":"consume-output","method":"session.process.read","params":{"processId":job.process,"offset":job.offset.to_string()}});
    if wait {
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(5));
            send(request);
        });
    } else {
        send(request);
    }
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
    let mut consume: Option<Consume> = None;
    for line in io::stdin().lock().lines() {
        let frame: Value = serde_json::from_str(&line.unwrap()).unwrap();
        match frame["method"].as_str() {
            Some("connections.changed") => {
                send(json!({"jsonrpc":"2.0","id":frame["id"],"result":{}}));
                continue;
            }
            Some("commands.handle") => {
                action_count += 1;
                let p = &frame["params"];
                let directory = data.as_ref().unwrap();
                fs::write(
                    format!("{directory}/command-count"),
                    action_count.to_string(),
                )
                .unwrap();
                fs::write(
                    format!("{directory}/command-{}", p["commandId"].as_str().unwrap()),
                    p.to_string(),
                )
                .unwrap();
                if p["payload"]["ignore"] == true {
                    continue;
                }
                if p["payload"]["exit"] == true {
                    std::process::exit(3);
                }
                if p["payload"]["none"] == true {
                    send(json!({"jsonrpc":"2.0","id":frame["id"],"result":{}}));
                    continue;
                }
                if let Some(view) = p["payload"].get("returnViewId") {
                    send(json!({"jsonrpc":"2.0","id":frame["id"],"result":{"viewId":view}}));
                    continue;
                }
                let delay = p["payload"]["delayMs"].as_u64().unwrap_or(0);
                let request = format!("command-open-{action_count}");
                pending_actions.insert(
                    request.clone(),
                    (frame["id"].clone(), p["payload"]["failAfterOpen"] == true),
                );
                let open = json!({"jsonrpc":"2.0","id":request,"method":"ui.openView","params":{"expectedContext":p["context"],"commandId":p["commandId"],"requestId":format!("open:{}",p["commandId"].as_str().unwrap()),"viewType":p["payload"]["viewType"].as_str().unwrap_or("terminal"),"params":p["payload"]["params"]}});
                let gate = p["payload"]["gate"]
                    .as_str()
                    .map(str::to_owned)
                    .or_else(|| {
                        (mode == "hold-first-command" && action_count == 1)
                            .then(|| format!("{directory}/startup-release"))
                    });
                thread::spawn(move || {
                    thread::sleep(Duration::from_millis(delay));
                    if let Some(gate) = gate {
                        let end = std::time::Instant::now() + Duration::from_secs(30);
                        while !std::path::Path::new(&gate).exists()
                            && std::time::Instant::now() < end
                        {
                            thread::sleep(Duration::from_millis(10));
                        }
                    }
                    send(open);
                });
                continue;
            }
            Some("state.changed") => {
                let p = &frame["params"];
                let path = format!("{}/shared-frame", data.as_ref().unwrap());
                let pending = format!("{path}.tmp");
                fs::write(&pending, p.to_string()).unwrap();
                fs::rename(pending, path).unwrap();
                send(
                    json!({"jsonrpc":"2.0","id":frame["id"],"result":{"accepted":true,"batchId":p["batchId"],"part":p["part"],"appliedRevision":p["toRevision"]}}),
                );
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
                    if p["data"]["rpc"] == "consumeStart" {
                        let job = Consume {
                            process: p["data"]["processId"].clone(),
                            offset: 0,
                            bytes: 0,
                            pages: 0,
                            started: Instant::now(),
                            first: None,
                            finished: false,
                            error: Value::Null,
                        };
                        poll_consume(&job, false);
                        consume = Some(job);
                        send(
                            json!({"jsonrpc":"2.0","id":frame["id"],"result":{"data":{"accepted":true}}}),
                        );
                        continue;
                    }
                    if let Some(rpc) = p["data"]["rpc"].as_str().filter(|s| {
                        s.starts_with("session.")
                            || s.starts_with("connections.")
                            || *s == "servers.list"
                            || s.starts_with("state.")
                            || s.starts_with("commands.")
                    }) {
                        let request = format!("session-{}", pending_sessions.len());
                        pending_sessions.insert(request.clone(), frame["id"].clone());
                        let mut params = p["data"]["params"].clone();
                        if rpc.starts_with("state.") {
                            params["viewId"] = json!(id);
                            params["expectedContext"] = p["context"].clone();
                        }
                        if rpc.starts_with("commands.") && params.is_object() {
                            params["sourceViewId"] = json!(id);
                            params["expectedContext"] = p["context"].clone();
                        }
                        if rpc == "session.exec.start"
                            || rpc == "session.process.start"
                            || rpc == "session.process.lookup"
                        {
                            params["viewId"] = json!(id);
                        }
                        if rpc == "session.target.describe" {
                            if params.get("viewId").is_none() {
                                params["viewId"] = json!(id);
                            }
                            if params.get("expectedContext").is_none() {
                                params["expectedContext"] = p["context"].clone();
                            }
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
                if method == "ui.getView" && p["data"].get("read").is_some() {
                    let query = &p["data"]["read"];
                    if query["op"] == "consumeStats" {
                        send(
                            json!({"jsonrpc":"2.0","id":frame["id"],"result":{"data":consume.as_ref().map(Consume::metrics)}}),
                        );
                        continue;
                    }
                    let value = if query["oversized"] == true {
                        json!({"text":"x".repeat(25 * 1024)})
                    } else {
                        json!({"query":query,"text":ui_views[id]["text"]})
                    };
                    let response = json!({"jsonrpc":"2.0","id":frame["id"],"result":{"data":value,"metadata":{"title":"must-not-change","dirty":false}}});
                    if query["delay"] == true {
                        fs::write(format!("{}/read-pending", data.as_ref().unwrap()), "ready")
                            .unwrap();
                        thread::spawn(move || {
                            thread::sleep(Duration::from_millis(200));
                            send(response);
                        });
                    } else {
                        send(response);
                    }
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
                if method == "ui.getView" {
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
                    json!({"jsonrpc":"2.0","id":frame["id"],"result":{"apiVersion":"2.1","commandHandlers":if mode=="second-command" {json!(["second.open.v1"])} else {json!(["terminal.open.v1"])}}}),
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
                if mode == "oversized-stderr" {
                    eprintln!("{}", "x".repeat(9000));
                    eprintln!("fixture diagnostic after oversized line");
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
            _ if frame["id"] == "consume-output" => {
                let job = consume.as_mut().unwrap();
                job.pages += 1;
                if frame.get("error").is_some() {
                    job.error = frame["error"].clone();
                    job.finished = true;
                    continue;
                }
                let value = &frame["result"];
                let chunks = value["chunks"].as_array().unwrap();
                for chunk in chunks {
                    let bytes = STANDARD
                        .decode(chunk["dataBase64"].as_str().unwrap())
                        .unwrap();
                    if !bytes.is_empty() && job.first.is_none() {
                        job.first = Some(Instant::now());
                    }
                    job.bytes += bytes.len() as u64;
                }
                job.offset = value["nextOffset"].as_str().unwrap().parse().unwrap();
                job.finished = value["status"]["workerFinished"] == true
                    && value["nextOffset"] == value["watermark"];
                if job.finished {
                    if value["status"]["outputComplete"] != true || value["status"]["exitCode"] != 0
                    {
                        job.error = value["status"].clone();
                    }
                } else {
                    poll_consume(job, chunks.is_empty());
                }
            }
            _ if frame["id"] == "settings-init" => {
                fs::write(
                    format!("{}/settings-init.json", data.as_ref().unwrap()),
                    frame.to_string(),
                )
                .unwrap();
                send(json!({"jsonrpc":"2.0","id":"initialize","result":{"apiVersion":"2.1"}}));
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

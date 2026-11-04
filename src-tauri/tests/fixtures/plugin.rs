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
    for line in io::stdin().lock().lines() {
        let frame: Value = serde_json::from_str(&line.unwrap()).unwrap();
        match frame["method"].as_str() {
            Some("initialize") => {
                let directory = frame["params"]["dataDirectory"].as_str().unwrap();
                data = Some(directory.to_owned());
                fs::write(format!("{directory}/pid"), std::process::id().to_string()).unwrap();
                if mode == "timeout" {
                    continue;
                }
                if mode == "child" {
                    let child = Command::new(std::env::current_exe().unwrap())
                        .arg("--child")
                        .spawn()
                        .unwrap();
                    fs::write(format!("{directory}/child-pid"), child.id().to_string()).unwrap();
                }
                send(json!({"jsonrpc":"2.0","id":frame["id"],"result":{"apiVersion":"1"}}));
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
            Some("shutdown") => {
                if mode == "ignore-stop" || mode == "child" {
                    continue;
                }
                send(
                    json!({"jsonrpc":"2.0","id":3,"method":"logging.write","params":{"level":"info","message":"fixture stopping"}}),
                );
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

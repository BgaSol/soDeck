use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

fn send(frame: Value) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    serde_json::to_writer(&mut stdout, &frame)?;
    stdout.write_all(b"\n")?;
    stdout.flush()
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    for line in io::stdin().lock().lines() {
        let frame: Value = serde_json::from_str(&line?)?;
        match frame["method"].as_str() {
            Some("initialize") => {
                send(json!({"jsonrpc":"2.0","id":frame["id"],"result":{"apiVersion":"1"}}))?;
                send(
                    json!({"jsonrpc":"2.0","id":1,"method":"logging.write","params":{"level":"info","message":"Hello 插件已启动"}}),
                )?;
                send(
                    json!({"jsonrpc":"2.0","id":2,"method":"messages.publish","params":{"title":"Hello SoDeck","body":"外置插件已加载。日志与重要消息由核心统一保存。","level":"success","notify":true,"dedupeKey":"hello-started"}}),
                )?;
            }
            Some("shutdown") => {
                // Wait for the log acknowledgement before sending the final shutdown response.
                send(
                    json!({"jsonrpc":"2.0","id":3,"method":"logging.write","params":{"level":"info","message":"Hello 插件正在停止"}}),
                )?;
            }
            _ if frame["id"] == 3 => {
                send(json!({"jsonrpc":"2.0","id":"shutdown","result":{}}))?;
                break;
            }
            _ => (),
        }
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

//! Test-only stateful UI protocol fixture. Never packaged or distributed.
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    io::{self, BufRead, Write},
};

struct View {
    kind: String,
    title: String,
    text: String,
    saved: String,
    cursor: u64,
    scroll: u64,
}
fn send(frame: Value) -> io::Result<()> {
    let mut out = io::stdout().lock();
    serde_json::to_writer(&mut out, &frame)?;
    out.write_all(b"\n")?;
    out.flush()
}
fn dispatch(
    views: &mut HashMap<String, View>,
    frame: &Value,
    serial: &mut u64,
) -> Result<Value, String> {
    let p = &frame["params"];
    let id = p["viewId"].as_str().ok_or("缺少 viewId")?;
    let method = frame["method"].as_str().unwrap_or_default();
    if method == "ui.disposeView" {
        views.remove(id);
        return Ok(json!({}));
    }
    if method == "ui.mount" && !views.contains_key(id) {
        let kind = p["viewType"].as_str().ok_or("缺少视图类型")?;
        let title = match kind {
            "files" => "文件".into(),
            "forwarding" => "网络转发".into(),
            "terminal" => {
                *serial += 1;
                format!("终端 {}", serial)
            }
            "editor" => p["params"]["name"]
                .as_str()
                .unwrap_or("夹具文件")
                .to_string(),
            _ => return Err("未知夹具视图".into()),
        };
        let text = if kind == "editor" {
            "# SoDeck 夹具文件，不会写入服务器\nserver {\n    listen 80;\n}\n"
        } else {
            ""
        }
        .to_string();
        views.insert(
            id.into(),
            View {
                kind: kind.into(),
                title,
                saved: text.clone(),
                text,
                cursor: 0,
                scroll: 0,
            },
        );
    }
    let v = views.get_mut(id).ok_or("视图实例尚未创建")?;
    match method {
        "ui.event" => {
            let d = &p["data"];
            match d["action"].as_str() {
                Some("edit") => {
                    let text = d["text"].as_str().ok_or("缺少文本")?;
                    if text.len() > 20 * 1024 {
                        return Err("示例草稿超过 20 KiB".into());
                    }
                    v.text = text.into();
                    v.cursor = d["cursor"].as_u64().unwrap_or(0);
                    v.scroll = d["scroll"].as_u64().unwrap_or(0);
                }
                Some("save") => {
                    if v.text.contains("SODECK_TEST_SAVE_FAILURE") {
                        return Err("示例保存失败：请移除 SODECK_TEST_SAVE_FAILURE 后重试".into());
                    }
                    v.saved = v.text.clone();
                }
                Some("position") => {
                    v.cursor = d["cursor"].as_u64().unwrap_or(0);
                    v.scroll = d["scroll"].as_u64().unwrap_or(0);
                }
                _ => return Err("未知示例交互".into()),
            }
        }
        "ui.save" => {
            if v.text.contains("SODECK_TEST_SAVE_FAILURE") {
                return Err("示例保存失败，草稿保留".into());
            }
            v.saved = v.text.clone();
        }
        "ui.mount" | "ui.beforeClose" => (),
        _ => return Err("未知插件 UI 方法".into()),
    }
    Ok(
        json!({"data":{"kind":v.kind,"text":v.text,"cursor":v.cursor,"scroll":v.scroll,"title":v.title},"metadata":{"title":v.title,"dirty":v.kind=="editor"&&v.text!=v.saved,"busy":false}}),
    )
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut views = HashMap::new();
    let mut serial = 0;
    let mut pending = HashMap::<String, Value>::new();
    let mut sequence = 0;
    for line in io::stdin().lock().lines() {
        let frame: Value = serde_json::from_str(&line?)?;
        let result = match frame["method"].as_str() {
            Some("initialize") => Ok(json!({"apiVersion":"1","uiActionsApiVersion":"1"})),
            Some("ui.action") => {
                if frame["params"]["data"]["none"] == true {
                    send(json!({"jsonrpc":"2.0","id":frame["id"],"result":{}}))?;
                    continue;
                }
                sequence += 1;
                let id = format!("open-{sequence}");
                pending.insert(id.clone(), frame["id"].clone());
                let p = &frame["params"];
                let open = json!({"jsonrpc":"2.0","id":id,"method":"ui.openView","params":{"expectedContext":p["context"],"requestId":p["requestId"],"viewType":"terminal","params":p["data"]}});
                let delay = p["data"]["delayMs"].as_u64().unwrap_or(0);
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(delay));
                    let _ = send(open);
                });
                continue;
            }
            Some("shutdown") => {
                send(json!({"jsonrpc":"2.0","id":frame["id"],"result":{}}))?;
                break;
            }
            Some(_) => dispatch(&mut views, &frame, &mut serial),
            _ => {
                if let Some(id) = frame["id"].as_str().and_then(|id| pending.remove(id)) {
                    send(if frame.get("error").is_some() {
                        json!({"jsonrpc":"2.0","id":id,"error":frame["error"]})
                    } else {
                        json!({"jsonrpc":"2.0","id":id,"result":frame["result"]})
                    })?;
                }
                continue;
            }
        };
        match result {
            Ok(result) => send(json!({"jsonrpc":"2.0","id":frame["id"],"result":result}))?,
            Err(message) => send(
                json!({"jsonrpc":"2.0","id":frame["id"],"error":{"code":-32602,"message":message}}),
            )?,
        }
    }
    Ok(())
}

//! Headless UI test bridge to the actual catalog service. Never bundled.
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
fn main() {
    let core =
        sodeck::AppCore::open(std::env::args().nth(1).unwrap().into(), "test".into()).unwrap();
    for line in io::stdin().lock().lines() {
        let request: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let args = &request["args"];
        let result = match request["command"].as_str().unwrap() {
            "server_catalog" => Ok(core.server_catalog()),
            "change_catalog" => core.change_catalog(
                args["revision"].as_u64().unwrap(),
                serde_json::from_value(args["change"].clone()).unwrap(),
            ),
            "recover_catalog" => core.recover_catalog(),
            "workspace_fixture_open" => core
                .accept_connected(sodeck::workspaces::ConnectedSession {
                    server_id: args["serverId"].as_str().unwrap().into(),
                    connection_id: args["connectionId"].as_str().unwrap().into(),
                    endpoint: args["endpoint"].as_str().unwrap().into(),
                })
                .map(|w| json!(w)),
            "workspace_snapshot" => core
                .workspace_projection(args["window"].as_str().unwrap())
                .map(|p| json!(p)),
            "workspace_ready" => core
                .workspaces
                .lock()
                .unwrap()
                .ready(args["window"].as_str().unwrap())
                .map(|_| Value::Null),
            "workspace_surface" => Ok(Value::Null),
            "workspace_cancel_move" => Ok(Value::Null),
            "workspace_activate" => core
                .workspaces
                .lock()
                .unwrap()
                .activate(args["window"].as_str().unwrap(), args["id"].as_str())
                .map(|_| Value::Null),
            "workspace_layout" => core
                .workspaces
                .lock()
                .unwrap()
                .layout(
                    args["window"].as_str().unwrap(),
                    args["id"].as_str().unwrap(),
                    serde_json::from_value(args["layout"].clone()).unwrap(),
                )
                .map(|_| Value::Null),
            "workspace_close" => core
                .workspaces
                .lock()
                .unwrap()
                .close(
                    args["window"].as_str().unwrap(),
                    args["id"].as_str().unwrap(),
                )
                .map(|_| Value::Null),
            "workspace_move" => (|| {
                let target = args["target"].as_str().unwrap_or("workspace-browser");
                let mut state = core.workspaces.lock().unwrap();
                if state.projection(target).is_err() {
                    state.register(target)?;
                    state.ready(target)?;
                }
                let m = state.prepare(
                    args["window"].as_str().unwrap(),
                    args["id"].as_str().unwrap(),
                    target,
                    args["index"].as_u64().unwrap() as usize,
                    "browser-move".into(),
                )?;
                state.commit(&m)?;
                Ok(Value::Null)
            })(),
            _ => Err("Unsupported test command".into()),
        };
        let response = match result {
            Ok(value) => json!({"value":value}),
            Err(error) => json!({"error":error}),
        };
        println!("{response}");
        io::stdout().flush().unwrap();
    }
}

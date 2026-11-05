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
            "official_plugins" => Ok(json!(sodeck::official_plugins::OFFICIAL_PLUGINS)),
            "install_official_plugin" => {
                let base = std::env::var("SODECK_TEST_PLUGIN_API")
                    .expect("installer fixture requires a loopback test API");
                core.install_official_plugin(
                    args["id"].as_str().unwrap(),
                    &sodeck::updates::ReleaseClient::test_server(&base),
                )
                .map(|id| json!(id))
            }
            "snapshot" => Ok(json!(core.snapshot())),
            "update_status" => Ok(json!(core.update_views())),
            "set_theme" => core
                .set_theme(args["theme"].as_str().unwrap())
                .map(|_| Value::Null),
            "frontend_ready" => Ok(Value::Null),
            "connection" => {
                let c = &args["command"];
                let window = args["window"].as_str().unwrap_or("main");
                let result = match c["op"].as_str().unwrap() {
                    "snapshot" => Ok(()),
                    "start" => core
                        .start_connection(window, c["serverId"].as_str().unwrap())
                        .map(|_| ()),
                    "cancel" => core.cancel_connection(window, c["attemptId"].as_str().unwrap()),
                    "answer" => core.answer_connection(
                        window,
                        c["attemptId"].as_str().unwrap(),
                        c["questionId"].as_str().unwrap(),
                        serde_json::from_value(c["answer"].clone()).unwrap(),
                    ),
                    "disconnect" => core.disconnect_connection(
                        window,
                        &serde_json::from_value(c["context"].clone()).unwrap(),
                    ),
                    _ => Err("未知测试连接操作".into()),
                };
                result.map(|_| json!(core.connection_views()))
            }
            "server_catalog" => Ok(core.server_catalog()),
            "component_ui" => sodeck::component_ui::dispatch(
                &core,
                args["window"].as_str().unwrap(),
                args["operation"].as_str().unwrap(),
                args["args"].clone(),
            ),
            "fixture_load_plugin" => core
                .load(std::path::Path::new(args["directory"].as_str().unwrap()))
                .map(|p| json!(p)),
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
    // UI integration tests may stop early; explicitly release process groups.
    let ids = core
        .component_ui
        .lock()
        .unwrap()
        .workspaces
        .keys()
        .cloned()
        .collect();
    if let Ok(plan) = core.ui_close_prepare("main", ids, None) {
        let views = serde_json::from_value(plan["views"].clone()).unwrap();
        let _ = core.ui_close_finish(plan["token"].as_str().unwrap(), views, "discard");
    }
    let _ = core.shutdown();
}

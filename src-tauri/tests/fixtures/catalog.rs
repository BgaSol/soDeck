//! Headless UI test bridge to the actual catalog service. Never bundled.
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
fn main() {
    if std::env::args().nth(1).as_deref() == Some("--desktop-script") {
        use tauri::plugin::Plugin;
        let plugin = sodeck::desktop_behavior::plugin::<tauri::Wry>();
        let script = plugin.initialization_script_2().unwrap();
        println!(
            "{}",
            json!({"script": script.script, "allFrames": !script.for_main_frame_only})
        );
        return;
    }
    let core =
        sodeck::AppCore::open(std::env::args().nth(1).unwrap().into(), "test".into()).unwrap();
    if std::env::var_os("SODECK_TEST_SHARED_EVENTS").is_some() {
        core.set_listener(std::sync::Arc::new(|name, payload| {
            if name == "component-state-changed" {
                let mut stdout = io::stdout().lock();
                writeln!(stdout, "{}", json!({"event":"state","payload":payload})).unwrap();
                stdout.flush().unwrap();
            }
        }));
    }
    for line in io::stdin().lock().lines() {
        let request: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let args = &request["args"];
        let result = match request["command"].as_str().unwrap() {
            "close_register_host" => core
                .register_close_host(args["window"].as_str().unwrap_or("main"))
                .map(|_| Value::Null),
            "close_prepare" => serde_json::from_value(args["target"].clone())
                .map_err(|e| e.to_string())
                .and_then(|target| {
                    core.close_prepare(
                        args["window"].as_str().unwrap_or("main"),
                        args["requestId"].as_str().unwrap(),
                        target,
                    )
                })
                .map(|o| json!(o)),
            "close_status" => core
                .close_status(
                    args["window"].as_str().unwrap_or("main"),
                    args["operationId"].as_str().unwrap(),
                )
                .map(|o| json!(o)),
            "close_operations" => Ok(json!(
                core.close_operations(args["window"].as_str().unwrap_or("main"))
            )),
            "close_report_inputs" => core
                .close_report_inputs(
                    args["window"].as_str().unwrap_or("main"),
                    args["operationId"].as_str().unwrap(),
                    args["inputRound"].as_u64().unwrap(),
                    args["report"].clone(),
                )
                .map(|o| json!(o)),
            "close_decide" => core
                .close_decide(
                    args["window"].as_str().unwrap_or("main"),
                    args["operationId"].as_str().unwrap(),
                    args["decisionRevision"].as_u64().unwrap(),
                    args["decision"].as_str().unwrap(),
                )
                .map(|o| json!(o)),
            "close_retry" => core
                .close_retry(
                    args["window"].as_str().unwrap_or("main"),
                    args["operationId"].as_str().unwrap(),
                )
                .map(|o| json!(o)),
            "close_exit_after_failure" => core
                .close_exit_after_failure(
                    args["window"].as_str().unwrap_or("main"),
                    args["operationId"].as_str().unwrap(),
                )
                .map(|o| json!(o)),
            "official_plugins" => {
                let base = std::env::var("SODECK_TEST_PLUGIN_API")
                    .expect("catalog fixture requires a loopback test API");
                core.official_plugins(
                    &sodeck::updates::ReleaseClient::test_server(&base),
                    args["refresh"].as_bool().unwrap_or(false),
                )
                .map(|catalog| json!(catalog))
            }
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
            "read_logs" => Ok(json!(
                core.logs(&serde_json::from_value(args["filter"].clone()).unwrap())
            )),
            "read_messages" => Ok(json!(core.messages())),
            "mark_read" => core.mark_read(args["id"].as_str()).map(|_| Value::Null),
            "settings_catalog" => Ok(core.settings_catalog()),
            "settings_resource" => core
                .settings_resource(
                    args["pluginId"].as_str().unwrap(),
                    args["definitionToken"].as_str().unwrap(),
                    args["relative"].as_str().unwrap(),
                )
                .map(|(bytes, mime)| json!({"bytes":bytes,"mime":mime})),
            "settings_page" => core.settings_page(
                args["pluginId"].as_str().unwrap(),
                args["pageId"].as_str().unwrap(),
            ),
            "save_settings" => core.save_settings_for_window(
                args["window"].as_str().unwrap_or("main"),
                args["pluginId"].as_str().unwrap(),
                args["pageId"].as_str().unwrap(),
                args["input"].clone(),
            ),
            "retry_settings_apply" => core.retry_settings_apply(
                args["pluginId"].as_str().unwrap(),
                args["pageId"].as_str().unwrap(),
                args["revision"].as_str().unwrap(),
            ),
            "settings_for_surface" => core.settings_for_surface(
                args["window"].as_str().unwrap_or("main"),
                args["surfaceId"].as_str().unwrap(),
            ),
            "settings_applied" => core.settings_applied(
                args["window"].as_str().unwrap_or("main"),
                args["surfaceId"].as_str().unwrap(),
                args["input"].clone(),
            ),
            "save_settings_for_surface" => core.save_settings_for_surface(
                args["window"].as_str().unwrap_or("main"),
                args["surfaceId"].as_str().unwrap(),
                args["input"].clone(),
            ),
            "network_snapshot" => Ok(core.network_snapshot()),
            "component_data" => {
                if args["request"]["op"] == "files" {
                    let result = if ["input.chooseUpload", "input.chooseDownload"]
                        .contains(&args["request"]["action"].as_str().unwrap_or(""))
                    {
                        let local =
                            std::env::var(if args["request"]["action"] == "input.chooseUpload" {
                                "SODECK_TEST_UPLOAD_PATH"
                            } else {
                                "SODECK_TEST_DOWNLOAD_PATH"
                            })
                            .expect("test local file source");
                        let local = std::path::PathBuf::from(local);
                        let local = if args["request"]["action"] == "input.chooseDownload"
                            && args["request"]["single"] != true
                        {
                            std::env::var_os("SODECK_TEST_DOWNLOAD_DIRECTORY")
                                .map(std::path::PathBuf::from)
                                .unwrap_or_else(|| local.parent().unwrap().to_path_buf())
                        } else {
                            local
                        };
                        core.files_register_input(
                            args["window"].as_str().unwrap_or("main"),
                            args["surfaceId"].as_str().unwrap(),
                            args["request"]["id"].as_str().unwrap(),
                            vec![local],
                            args["request"]["action"] == "input.chooseDownload",
                        )
                        .map_err(String::from)
                    } else if args["request"]["action"] == "export.start" {
                        match core.files_export_begin(
                            args["window"].as_str().unwrap_or("main"),
                            args["surfaceId"].as_str().unwrap(),
                            &args["request"],
                        ) {
                            Ok((receipt, task)) => {
                                if let Some(task) = task {
                                    let core = core.clone();
                                    let path = std::env::var("SODECK_TEST_DOWNLOAD_PATH")
                                        .expect("test export destination");
                                    tauri::async_runtime::spawn(async move {
                                        core.files_export_finish(task, Ok(Some(path.into()))).await;
                                    });
                                }
                                Ok(json!({"ok":true,"value":receipt}))
                            }
                            Err(error) => Ok(
                                json!({"ok":false,"error":{"code":"exportRejected","message":error}}),
                            ),
                        }
                    } else if args["request"]["action"] == "exportDraft" {
                        let local = std::env::var("SODECK_TEST_DOWNLOAD_PATH")
                            .expect("test export destination");
                        tauri::async_runtime::block_on(core.files_export(
                            args["window"].as_str().unwrap_or("main"),
                            args["surfaceId"].as_str().unwrap(),
                            args["request"].clone(),
                            local.into(),
                        ))
                    } else if args["request"]["action"] == "download" {
                        let local = std::env::var("SODECK_TEST_DOWNLOAD_PATH")
                            .expect("test download destination");
                        tauri::async_runtime::block_on(core.files_download(
                            args["window"].as_str().unwrap_or("main"),
                            args["surfaceId"].as_str().unwrap(),
                            args["request"].clone(),
                            local.into(),
                        ))
                    } else {
                        tauri::async_runtime::block_on(core.files_data(
                            args["window"].as_str().unwrap_or("main"),
                            args["surfaceId"].as_str().unwrap(),
                            args["request"].clone(),
                        ))
                    };
                    let mut out = io::stdout().lock();
                    let reply = match result {
                        Ok(value) => json!({"id":request["id"],"value":value}),
                        Err(error) => json!({"id":request["id"],"error":error}),
                    };
                    writeln!(out, "{reply}").unwrap();
                    out.flush().unwrap();
                    continue;
                }
                let token = args["surfaceId"].as_str().unwrap().to_string();
                let send_token = token.clone();
                let sink: sodeck::streams::Sink = std::sync::Arc::new(move |frame| {
                    let mut out = io::stdout().lock();
                    serde_json::to_writer(
                        &mut out,
                        &json!({"event":"data","surface":send_token,"frame":frame}),
                    )
                    .map_err(|e| e.to_string())?;
                    writeln!(out)
                        .and_then(|_| out.flush())
                        .map_err(|e| e.to_string())
                });
                tauri::async_runtime::block_on(core.terminal_data(
                    args["window"].as_str().unwrap_or("main"),
                    &token,
                    serde_json::from_value(args["request"].clone()).unwrap(),
                    Some(sink),
                ))
                .map(|_| Value::Null)
            }
            "update_status" => Ok(json!(core.update_views())),
            "update_overview" => Ok(json!(core.update_overview())),
            "set_update_network" => core
                .set_update_network(
                    args["expectedRevision"].as_u64().unwrap(),
                    serde_json::from_value(args["preferences"].clone()).unwrap(),
                )
                .map(|saved| json!(saved)),
            "test_update_mirror" => {
                let base = std::env::var("SODECK_TEST_PLUGIN_API").unwrap();
                let client = sodeck::updates::ReleaseClient::test_server(&base);
                // Inject a single local endpoint so the production diagnostic never contacts public nodes.
                let prefix = args["prefix"].as_str().map(|p| {
                    format!(
                        "{base}/{}/",
                        if p.contains("failed") {
                            "diagnostic-failed"
                        } else {
                            "diagnostic"
                        }
                    )
                });
                client
                    .diagnose(prefix.as_deref())
                    .map(|results| json!(results))
            }
            "set_automatic_updates" => core
                .set_automatic_updates(args["enabled"].as_bool().unwrap())
                .map(|_| Value::Null),
            "add_plugin" => core
                .add_plugin(std::path::Path::new(args["directory"].as_str().unwrap()))
                .map(|id| json!(id)),
            "set_plugin_enabled" => core
                .set_enabled(
                    args["id"].as_str().unwrap(),
                    args["enabled"].as_bool().unwrap(),
                )
                .map(|_| Value::Null),
            "set_plugin_update_source" => core
                .set_update_source_checked(
                    args["id"].as_str().unwrap(),
                    serde_json::from_value(args["source"].clone()).unwrap(),
                    serde_json::from_value(args["expected"].clone()).unwrap(),
                )
                .map(|_| Value::Null),
            "check_update" => {
                let base = std::env::var("SODECK_TEST_PLUGIN_API").unwrap();
                core.check_component(
                    args["id"].as_str().unwrap(),
                    &sodeck::updates::ReleaseClient::test_server(&base),
                )
                .map(|_| Value::Null)
            }
            "check_updates" => {
                if let Some(mut batch) =
                    core.begin_update_check(args["automatic"].as_bool().unwrap_or(false))
                {
                    let base = std::env::var("SODECK_TEST_PLUGIN_API").unwrap();
                    let client = sodeck::updates::ReleaseClient::test_server(&base);
                    for id in core
                        .update_check_ids()
                        .into_iter()
                        .filter(|id| id != sodeck::updates::CORE_ID)
                    {
                        if core.check_component(&id, &client).is_ok() {
                            batch.succeeded();
                        }
                    }
                }
                Ok(Value::Null)
            }
            "download_updates" => {
                let ids: Vec<String> = serde_json::from_value(args["ids"].clone()).unwrap();
                core.reserve_update_downloads(&ids).map(|tasks| {
                    let base = std::env::var("SODECK_TEST_PLUGIN_API").unwrap();
                    for (id, operation) in tasks {
                        let _ = core.download_plugin_reserved(
                            &id,
                            &sodeck::updates::ReleaseClient::test_server(&base),
                            operation,
                        );
                    }
                    Value::Null
                })
            }
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
                    "focus" => (|| {
                        let context = serde_json::from_value(c["context"].clone()).unwrap();
                        let mut state = core.workspace_state();
                        state.validate_identity(&context)?;
                        let owner = state.owner(&context.workspace_id).ok_or("工作区已关闭")?;
                        state.activate(&owner, Some(&context.workspace_id))
                    })(),
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
                .workspace_state()
                .ready(args["window"].as_str().unwrap())
                .map(|_| Value::Null),
            "workspace_surface" => Ok(Value::Null),
            "workbench_fixture_prepare" => (|| {
                let scope =
                    serde_json::from_value(args["scope"].clone()).map_err(|e| e.to_string())?;
                let destination: sodeck::workbench_move::Destination =
                    serde_json::from_value(args["destination"].clone()).unwrap_or_default();
                let (target, destination) = if destination.return_to_source {
                    core.resolve_workbench_return(
                        args["window"].as_str().unwrap(),
                        args["id"].as_str().unwrap(),
                        &scope,
                    )?
                } else {
                    (args["target"].as_str().unwrap().into(), destination)
                };
                core.prepare_workbench_move(
                    args["window"].as_str().unwrap(),
                    args["id"].as_str().unwrap(),
                    &target,
                    0,
                    args["requestId"].as_str().unwrap().into(),
                    core.workspace_projection(&target).is_err(),
                    scope,
                    destination,
                )
                .map(|m| {
                    core.workspace_changed();
                    json!(m)
                })
            })(),
            "workbench_fixture_commit" => (|| {
                core.commit_workbench_move(
                    &serde_json::from_value(args["movement"].clone()).map_err(|e| e.to_string())?,
                )
                .map(|_| {
                    core.workspace_changed();
                    Value::Null
                })
            })(),
            "workbench_fixture_return_window" => core
                .prepare_window_return(
                    args["window"].as_str().unwrap(),
                    args["requestId"].as_str().unwrap().into(),
                )
                .map(|m| {
                    core.workspace_changed();
                    json!(m)
                }),
            "workbench_fixture_commit_many" => (|| {
                let movements: Vec<sodeck::workspaces::Move> =
                    serde_json::from_value(args["movements"].clone()).map_err(|e| e.to_string())?;
                core.commit_workbench_moves(&movements).map(|_| {
                    core.workspace_changed();
                    Value::Null
                })
            })(),
            "workbench_fixture_drain" => (|| {
                core.begin_transfer_drain(
                    &serde_json::from_value(args["movement"].clone()).map_err(|e| e.to_string())?,
                )
                .map(|_| Value::Null)
            })(),
            "workspace_cancel_move" => Ok(Value::Null),
            "workspace_activate" => core
                .workspace_state()
                .activate(args["window"].as_str().unwrap(), args["id"].as_str())
                .map(|_| Value::Null),
            "workspace_layout" => core
                .workspace_resize(
                    args["window"].as_str().unwrap(),
                    args["id"].as_str().unwrap(),
                    args["side"].as_str().unwrap_or_default(),
                    args["width"]
                        .as_u64()
                        .and_then(|v| u16::try_from(v).ok())
                        .unwrap_or_default(),
                )
                .map(|value| serde_json::to_value(value).unwrap()),
            "workspace_close" => core
                .close_prepare(
                    args["window"].as_str().unwrap(),
                    "fixture-workspace-close",
                    sodeck::lifecycle::Target::Workspace {
                        id: args["id"].as_str().unwrap().into(),
                    },
                )
                .map(|_| Value::Null),
            "workspace_move" | "workbench_move" => (|| {
                let target = args["target"].as_str().unwrap_or("workspace-browser");
                let register = core.workspace_projection(target).is_err();
                let m = core.prepare_workspace_move(
                    args["window"].as_str().unwrap(),
                    args["id"].as_str().unwrap(),
                    target,
                    args["index"].as_u64().unwrap_or(u64::MAX) as usize,
                    "browser-move".into(),
                    register,
                )?;
                if register {
                    core.workspace_state().ready(target)?;
                }
                core.commit_workspace_move(&m)?;
                core.workspace_changed();
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
    let preparation = core.close_fixture_prepare(
        "main",
        sodeck::lifecycle::Target::Application {
            action: sodeck::lifecycle::Action::Exit,
        },
    );
    let plan = match preparation {
        Ok(plan) => plan,
        Err(error) => {
            eprintln!("Test aborted before close decisions: {error}");
            for plugin in core.snapshot().plugins {
                if !plugin.cleanup_complete {
                    core.unload(&plugin.manifest.id)
                        .expect("aborted fixture process cleanup");
                }
            }
            std::process::exit(1);
        }
    };
    core.ui_close_finish(
        plan["token"].as_str().unwrap(),
        serde_json::from_value(plan["views"].clone()).unwrap(),
        "discard",
    )
    .expect("fixture process/resource cleanup");
}

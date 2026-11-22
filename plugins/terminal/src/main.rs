use creation::{CreateParams, DirectoryIntegration, Launch};
use serde_json::{json, Value};
use sodeck_plugin_client::protocol::requests as api;
use sodeck_plugin_client::RpcError;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
};
mod creation;
mod integration;

fn resolve(client: &sodeck_plugin_client::Client, input: &Value) -> Result<CreateParams, RpcError> {
    CreateParams::parse(input, || {
        let settings = client.request(api::call::Settings(api::Page {
            page_id: "appearance".to_string(),
        }))?;
        settings["values"]["directoryMode"]
            .as_str()
            .map(str::to_owned)
            .ok_or("终端目录设置无效".into())
    })
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    type Slot = Arc<OnceLock<Result<TerminalView, RpcError>>>;
    let terminals = Mutex::new(HashMap::<String, Slot>::new());
    let settings = sodeck_plugin_client::settings::SettingsCache::default();
    let tickets = Mutex::new(HashMap::<String, OpenIntent>::new());
    sodeck_plugin_client::run_with_commands(&["terminal.open.v1"], move |client, callback| {
        let (method, p) = callback.into_parts();
        if method == "initialize" {
            return Ok(json!({}));
        }
        if method == "settings.changed" {
            return settings.changed(&p, &["appearance"]).map_err(Into::into);
        }
        if method == "commands.handle" {
            let command = p["commandId"].as_str().ok_or("缺少 commandId")?;
            let params = resolve(client, &p["payload"])?;
            let opened = client.request(api::call::OpenView(api::OpenView {
                expected_context: serde_json::from_value(p["context"].clone())
                    .map_err(|_| RpcError::invalid("终端上下文无效"))?,
                command_id: Some(command.into()),
                request_id: format!("open:{command}"),
                view_type: "terminal".into(),
                params: Some(params.value()),
                source_view_id: None,
                resource_key: None,
            }))?;
            return Ok(json!({"viewId":opened.view_id}));
        }

        let view = p["viewId"].as_str().ok_or("缺少 viewId")?;
        if method == "ui.event"
            && matches!(
                p["data"]["action"].as_str(),
                Some("createTerminal" | "terminalCommandStatus")
            )
        {
            let ticket = {
                let mut all = tickets.lock().unwrap();
                if p["data"]["action"] == "createTerminal" {
                    let request = p["data"]["requestId"].as_str().ok_or("缺少 requestId")?;
                    let replace = all.get(view).is_none_or(|t| {
                        t.ticket.context != p["context"]
                            || (t.result.as_ref().is_some_and(|r| r["status"] != "pending")
                                && t.ticket.request_id != request)
                    });
                    if replace {
                        all.insert(
                            view.into(),
                            OpenIntent {
                                ticket: sodeck_plugin_client::commands::Ticket::new(
                                    p["context"].clone(),
                                    view,
                                    request,
                                    "terminal.open.v1",
                                    p["data"]["params"].clone(),
                                ),
                                result: None,
                            },
                        );
                    }
                    if let Some(intent) = all.get(view) {
                        if intent.ticket.request_id == request
                            && intent.ticket.payload != p["data"]["params"]
                        {
                            return Err("相同终端意图参数不同".into());
                        }
                    }
                }
                let Some(intent) = all.get(view) else {
                    return Ok(
                        json!({"data":{"status":"unknown","error":{"message":"原始终端请求未被观察到，请核对工作台页面"}}}),
                    );
                };
                if let Some(result) = intent.result.as_ref().filter(|r| r["status"] != "pending") {
                    return Ok(json!({"data":result}));
                }
                intent.ticket.clone()
            };
            let reply = if p["data"]["action"] == "createTerminal" {
                ticket.submit(client)
            } else {
                match ticket.get(client) {
                    Err(e) if e.data["code"] == "notFound" => ticket.submit(client),
                    other => other,
                }
            };
            let result=reply.unwrap_or_else(|e|if e.code == -32003 {json!({"status":"pending","error":{"message":"终端回执待确认，保留原编号继续查询"}})}else{json!({"status":"failed","error":{"code":e.data["code"],"message":e.message}})});
            if let Some(intent) = tickets
                .lock()
                .unwrap()
                .get_mut(view)
                .filter(|t| t.ticket.request_id == ticket.request_id)
            {
                intent.result = Some(result.clone());
            }
            return Ok(json!({"data":result}));
        }

        if method == "ui.disposeView" {
            tickets.lock().unwrap().remove(view);
            let slot = terminals.lock().unwrap().remove(view);
            if let Some(Ok(terminal)) = slot.as_ref().and_then(|slot| slot.get()) {
                client.request(api::call::TerminalClose(api::Resource {
                    id: terminal.id.to_string(),
                }))?;
            }
            return Ok(json!({}));
        }
        if method == "ui.getView" {
            let slot = terminals
                .lock()
                .unwrap()
                .get(view)
                .cloned()
                .ok_or("终端尚未创建")?;
            let terminal = slot
                .get()
                .ok_or("终端仍在创建")?
                .as_ref()
                .map_err(Clone::clone)?;
            return Ok(
                json!({"data":{"streamId":terminal.id,"title":terminal.title,"openedContext":terminal.context,"integration":terminal.integration,"createParams":terminal.create_params,"commandPending":tickets.lock().unwrap().get(view).is_some_and(|t|t.result.as_ref().is_none_or(|r|r["status"]=="pending"))},"metadata":{"title":terminal.title,"dirty":false,"busy":false}}),
            );
        }
        if method == "ui.createView" {
            let (slot, next) = {
                let mut all = terminals.lock().unwrap();
                let next = all.len() + 1;
                (
                    all.entry(view.into())
                        .or_insert_with(|| Arc::new(OnceLock::new()))
                        .clone(),
                    next,
                )
            };
            let terminal = slot
                .get_or_init(|| {
                    let resolved = resolve(client, &p["params"])?;
                    let mut params = json!({"viewId":view,"cols":80,"rows":24});
                    let reading_auto = if resolved.launch == Launch::LoginShell && resolved.directory != DirectoryIntegration::Auto {
                        let settings = client.request(api::call::Settings(api::Page { page_id: "appearance".into() }))?;
                        settings["values"]["readingMode"].as_str().ok_or("终端阅读设置无效")? == "auto"
                    } else { true };
                    let integration = match &resolved.launch {
                        Launch::Command(command) => {
                            params["command"] = json!(command);
                            json!({"mode":"off"})
                        }
                        Launch::LoginShell
                            if !reading_auto =>
                        {
                            json!({"mode":resolved.directory.name()})
                        }
                        Launch::LoginShell => {
                            match integration::prepare(client, view, &p["context"]) {
                                Ok((command, state)) => {
                                    params["command"] = json!(command);
                                    let mut state = state;
                                    if resolved.directory != DirectoryIntegration::Auto {
                                        state["mode"] = json!(resolved.directory.name());
                                    }
                                    state
                                }
                                Err(reason) => json!({"mode":if resolved.directory==DirectoryIntegration::Auto {"unavailable"} else {resolved.directory.name()},"reasonCode":reason}),
                            }
                        }
                    };
                    let result = serde_json::from_value::<api::TerminalOpen>(params)
                        .map_err(|_| sodeck_plugin_client::RpcError::invalid("核心请求参数无效"))
                        .and_then(|params| client.request(api::call::TerminalOpen(params)))?;
                    Ok(TerminalView {
                        id: result.id,
                        title: resolved
                            .title
                            .clone()
                            .unwrap_or_else(|| format!("终端 {next}")),
                        context: p["context"].clone(),
                        integration,
                        create_params: resolved.value(),
                    })
                })
                .as_ref()
                .map_err(Clone::clone)?;
            if !terminals
                .lock()
                .unwrap()
                .get(view)
                .is_some_and(|current| Arc::ptr_eq(current, &slot))
            {
                let _ = client.request(api::call::TerminalClose(api::Resource {
                    id: terminal.id.to_string(),
                }));
                return Err("终端页面已关闭".into());
            }
            return Ok(
                json!({"data":{"streamId":terminal.id,"title":terminal.title,"openedContext":terminal.context,"integration":terminal.integration,"createParams":terminal.create_params,"commandPending":tickets.lock().unwrap().get(view).is_some_and(|t|t.result.as_ref().is_none_or(|r|r["status"]=="pending"))},"metadata":{"title":terminal.title,"dirty":false,"busy":false}}),
            );
        }
        match method {
            "ui.beforeClose" | "ui.save" => Ok(json!({"metadata":{"dirty":false,"busy":false}})),
            _ => Err("未知终端插件方法".into()),
        }
    })
}
#[derive(Clone)]
struct TerminalView {
    id: String,
    title: String,
    context: Value,
    integration: Value,
    create_params: Value,
}

struct OpenIntent {
    ticket: sodeck_plugin_client::commands::Ticket,
    result: Option<Value>,
}

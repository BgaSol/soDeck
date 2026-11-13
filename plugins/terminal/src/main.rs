use creation::{CreateParams, DirectoryIntegration, Launch};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    },
};
mod creation;
mod integration;

fn resolve(
    client: &sodeck_plugin_client::Client,
    input: &Value,
    can_command: bool,
) -> Result<CreateParams, String> {
    CreateParams::parse(
        input,
        || {
            client
                .call("settings.get", json!({"pageId":"appearance"}))
                .unwrap_or(Value::Null)["values"]["directoryMode"]
                .as_str()
                .unwrap_or("auto")
                .into()
        },
        can_command,
    )
}
fn create(
    client: &sodeck_plugin_client::Client,
    context: &Value,
    request: &Value,
    input: &Value,
    can_command: bool,
) -> Result<Value, String> {
    let params = resolve(client, input, can_command)?;
    client.call("ui.openView", json!({"expectedContext":context,"requestId":request,"viewType":"terminal","params":params.value()}))
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    type Slot = Arc<OnceLock<Result<TerminalView, String>>>;
    let terminals = Mutex::new(HashMap::<String, Slot>::new());
    let settings = sodeck_plugin_client::settings::SettingsCache::default();
    let can_command = AtomicBool::new(false);
    let can_integrate = AtomicBool::new(false);
    let can_open = AtomicBool::new(false);
    let commands_api = AtomicBool::new(false);
    let tickets = Mutex::new(HashMap::<String, OpenIntent>::new());
    sodeck_plugin_client::run_with_commands(&["terminal.open.v1"], move |client, method, p| {
        if method == "initialize" {
            let supports = |wanted: &str| {
                p["capabilities"]
                    .as_array()
                    .is_some_and(|c| c.iter().any(|v| v == wanted))
            };
            can_command.store(supports("terminal.launch.v1"), Ordering::Relaxed);
            can_integrate.store(
                supports("terminal.launch.v1") && supports("session.exec.v1"),
                Ordering::Relaxed,
            );
            can_open.store(supports("ui.openView.v1"), Ordering::Relaxed);
            commands_api.store(supports("commands.v1"), Ordering::Relaxed);
            for capability in ["terminal.v1", "viewData.v1", "terminal.history.v1"] {
                if !supports(capability) {
                    return Err(format!("核心缺少 {capability}，请先升级 SoDeck").into());
                }
            }
            return Ok(
                json!({"apiVersion":"1","settingsApiVersion":"1","uiApiVersion":"2","uiActionsApiVersion":"1"}),
            );
        }
        if method == "settings.changed" {
            return settings.changed(&p, &["appearance"]).map_err(Into::into);
        }
        if method == "commands.handle" {
            let command = p["commandId"].as_str().ok_or("缺少 commandId")?;
            let params = resolve(client, &p["payload"], can_command.load(Ordering::Relaxed))?;
            return client.call_detailed("ui.openView",json!({"expectedContext":p["context"],"commandId":command,"requestId":format!("open:{command}"),"viewType":"terminal","params":params.value()}));
        }
        if method == "ui.action" {
            if p["actionId"] != "new-terminal" {
                return Err("未知终端行为".into());
            }
            if !can_open.load(Ordering::Relaxed) {
                return Err("核心缺少 ui.openView.v1".into());
            }
            return create(
                client,
                &p["context"],
                &p["requestId"],
                &p["data"],
                can_command.load(Ordering::Relaxed),
            )
            .map_err(Into::into);
        }
        let view = p["viewId"].as_str().ok_or("缺少 viewId")?;
        if method == "ui.event"
            && matches!(
                p["data"]["action"].as_str(),
                Some("createTerminal" | "terminalCommandStatus")
            )
            && commands_api.load(Ordering::Relaxed)
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
        if method == "ui.event" && p["data"]["action"] == "createTerminal" {
            if !can_open.load(Ordering::Relaxed) {
                return Err("核心缺少 ui.openView.v1".into());
            }
            return create(
                client,
                &p["context"],
                &p["data"]["requestId"],
                &p["data"]["params"],
                can_command.load(Ordering::Relaxed),
            )
            .map(|result| json!({"data":result}))
            .map_err(Into::into);
        }
        if method == "ui.disposeView" {
            tickets.lock().unwrap().remove(view);
            let slot = terminals.lock().unwrap().remove(view);
            if let Some(Ok(terminal)) = slot.as_ref().and_then(|slot| slot.get()) {
                client.call("terminal.close", json!({"id":terminal.id}))?;
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
                json!({"data":{"streamId":terminal.id,"title":terminal.title,"openedContext":terminal.context,"integration":terminal.integration,"createParams":terminal.create_params,"commandsApi":commands_api.load(Ordering::Relaxed),"commandPending":tickets.lock().unwrap().get(view).is_some_and(|t|t.result.as_ref().is_none_or(|r|r["status"]=="pending"))},"metadata":{"title":terminal.title,"dirty":false,"busy":false}}),
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
                    let resolved =
                        resolve(client, &p["params"], can_command.load(Ordering::Relaxed))?;
                    let mut params = json!({"viewId":view,"cols":80,"rows":24});
                    let integration = match &resolved.launch {
                        Launch::Command(command) => {
                            params["command"] = json!(command);
                            json!({"mode":"off"})
                        }
                        Launch::LoginShell if resolved.directory != DirectoryIntegration::Auto => {
                            json!({"mode":resolved.directory.name()})
                        }
                        Launch::LoginShell if !can_integrate.load(Ordering::Relaxed) => {
                            json!({"mode":"unavailable","reasonCode":"old-core"})
                        }
                        Launch::LoginShell => {
                            match integration::prepare(client, view, &p["context"]) {
                                Ok((command, state)) => {
                                    params["command"] = json!(command);
                                    state
                                }
                                Err(reason) => json!({"mode":"unavailable","reasonCode":reason}),
                            }
                        }
                    };
                    let result = client.call("terminal.open", params)?;
                    Ok(TerminalView {
                        id: result["id"].as_str().ok_or("核心没有返回终端 ID")?.into(),
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
                let _ = client.call("terminal.close", json!({"id":terminal.id}));
                return Err("终端页面已关闭".into());
            }
            return Ok(
                json!({"data":{"streamId":terminal.id,"title":terminal.title,"openedContext":terminal.context,"integration":terminal.integration,"createParams":terminal.create_params,"commandsApi":commands_api.load(Ordering::Relaxed),"commandPending":tickets.lock().unwrap().get(view).is_some_and(|t|t.result.as_ref().is_none_or(|r|r["status"]=="pending"))},"metadata":{"title":terminal.title,"dirty":false,"busy":false}}),
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

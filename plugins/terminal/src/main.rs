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
    sodeck_plugin_client::run(move |client, method, p| {
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
            for capability in ["terminal.v1", "viewData.v1"] {
                if !supports(capability) {
                    return Err(format!("核心缺少 {capability}，请先升级 SoDeck"));
                }
            }
            return Ok(
                json!({"apiVersion":"1","settingsApiVersion":"1","uiActionsApiVersion":"1"}),
            );
        }
        if method == "settings.changed" {
            return settings.changed(&p, &["appearance"]);
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
            );
        }
        let view = p["viewId"].as_str().ok_or("缺少 viewId")?;
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
            .map(|result| json!({"data":result}));
        }
        if method == "ui.disposeView" {
            let slot = terminals.lock().unwrap().remove(view);
            if let Some(Ok(terminal)) = slot.as_ref().and_then(|slot| slot.get()) {
                client.call("terminal.close", json!({"id":terminal.id}))?;
            }
            return Ok(json!({}));
        }
        if method == "ui.mount" {
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
                json!({"data":{"streamId":terminal.id,"title":terminal.title,"openedContext":terminal.context,"integration":terminal.integration,"createParams":terminal.create_params},"metadata":{"title":terminal.title,"dirty":false,"busy":false}}),
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

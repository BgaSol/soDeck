use crate::{
    cli,
    connection::Snapshot,
    ports::{self, Binding},
};
use serde_json::{json, Value};
use sodeck_plugin_client::{commands::Ticket, Client};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Terminal,
    Tunnel,
}
#[derive(Clone, Default)]
pub struct Intent {
    pub ticket: Option<Ticket>,
    pub receipt: Value,
    pub input: Value,
}
impl Intent {
    pub fn pending(&self) -> bool {
        self.receipt["status"] == "pending"
    }
    pub fn unresolved(&self) -> bool {
        self.pending() || self.receipt["status"] == "unknown"
    }
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}
#[derive(Clone)]
pub struct Work {
    pub kind: Kind,
    pub intent: Intent,
    pub submit: bool,
}
pub fn prepare(
    client: &Client,
    view: &str,
    context: &Value,
    target: &Snapshot,
    container: &Value,
    kind: Kind,
    input: &Value,
) -> Result<Ticket, String> {
    let (name, payload) = match kind {
        Kind::Terminal => {
            if container["State"]["Running"] != true || container["State"]["Paused"] == true {
                return Err("容器当前不能接受交互终端，请刷新状态".into());
            }
            let id = container["Id"].as_str().ok_or("缺少容器 ID")?;
            let args =
                cli::terminal_args(target, id, input["shell"].as_str().ok_or("缺少容器程序")?)?;
            let prepared = cli::prepare(
                client,
                view,
                target,
                &args.iter().map(String::as_str).collect::<Vec<_>>(),
                Some(id),
                true,
            )
            .map_err(|e| e.message)?;
            (
                "terminal.open.v1",
                json!({"version":1,"title":format!("{} · 容器",crate::commands::text(container["Name"].as_str().unwrap_or("Docker").trim_start_matches('/'),100)),"launch":{"kind":"command","command":prepared.terminal_command()?},"directoryIntegration":"off"}),
            )
        }
        Kind::Tunnel => {
            let binding: Binding =
                serde_json::from_value(input["binding"].clone()).map_err(|_| "端口绑定身份无效")?;
            let destination = ports::verify(container, &binding)?;
            let name = format!(
                "Docker · {} · {}/tcp → {}",
                container["Name"]
                    .as_str()
                    .unwrap_or("容器")
                    .trim_start_matches('/'),
                binding.container_port,
                binding.host_port
            )
            .chars()
            .take(128)
            .collect::<String>();
            (
                "network.tcpForward.ensure.v1",
                json!({"version":1,"target":destination,"suggestedName":name,"selectedRule":input["selectedRule"]}),
            )
        }
    };
    Ok(Ticket::new(
        context.clone(),
        view,
        input["requestId"].as_str().ok_or("缺少请求 ID")?,
        name,
        payload,
    ))
}
pub fn dispatch(client: &Client, ticket: &Ticket, submit: bool) -> Result<Value, String> {
    let result = if submit {
        ticket.resolve(client).map_err(|e| e.message)?;
        ticket.submit(client)
    } else {
        ticket.get(client)
    };
    match result {
        Ok(receipt) => Ok(receipt),
        Err(e) if e.code == -32003 => {
            Ok(json!({"status":"pending","message":"请求回执待确认，保留原编号继续查询"}))
        }
        Err(e) if e.data["code"] == "notFound" => ticket
            .submit(client)
            .or_else(|e| {
                if e.code == -32003 {
                    Ok(json!({"status":"pending"}))
                } else {
                    Err(e)
                }
            })
            .map_err(|e| e.message),
        Err(e) => Err(e.message),
    }
}

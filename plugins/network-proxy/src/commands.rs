use crate::{rules::Target, runtime::App, service::Service};
use serde_json::{json, Value};
use sodeck_plugin_client::{Client, RpcError};
use std::sync::Arc;
pub const HANDLERS: &[&str] = &["network.tcpForward.ensure.v1", "network.rule.reveal.v1"];
#[derive(Debug)]
pub enum Decision {
    Create,
    Use(String),
    Select(usize),
}
pub fn decide(
    a: &App,
    context: &Value,
    target: &Target,
    selected: &Value,
) -> Result<Decision, String> {
    let server = context["serverId"]
        .as_str()
        .ok_or("invalidParams：缺少服务器")?;
    let mut running = vec![];
    let mut stopped = vec![];
    let mut starting = vec![];
    for rule in a.rules.items.iter().filter(|r| r.server == server) {
        let active = a.runs.get(&rule.id).filter(|r| {
            r.snapshot["cleanupComplete"] != true || !r.released || r.query_error.is_some()
        });
        let saved_match = rule.target() == Some(target) && rule.bind.host.is_loopback();
        let runtime_match = active
            .is_some_and(|r| r.rule.target() == Some(target) && r.rule.bind.host.is_loopback());
        if !saved_match && !runtime_match {
            continue;
        }
        if let Some(run) = active {
            if run.request["expectedContext"] != *context {
                return Err("tunnelBusy：规则正在其他连接上下文运行".into());
            }
            if !rule.same_runtime(&run.rule) {
                return Err("ruleConflict：配置已修改，请在网络隧道中处理".into());
            }
            if run.query_error.is_some() {
                return Err("tunnelStateUnknown：原隧道状态待确认，请查询或停止原启动".into());
            }
            if run.stop_requested
                || !matches!(run.snapshot["state"].as_str(), Some("running" | "starting"))
            {
                return Err("tunnelBusy：原监听尚未确认清理".into());
            }
            if run.snapshot["state"] == "running" {
                running.push(rule);
            } else {
                starting.push(rule);
            }
        } else {
            stopped.push(rule);
        }
    }
    let candidates = if !running.is_empty() {
        running
    } else if !starting.is_empty() {
        starting
    } else {
        stopped
    };
    if !selected.is_null() {
        let rule = candidates
            .iter()
            .find(|r| selected["id"] == r.id && selected["revision"].as_u64() == Some(r.revision))
            .ok_or("ruleConflict：所选规则已变化，请重新选择")?;
        return Ok(Decision::Use(rule.id.clone()));
    }
    Ok(match candidates.len() {
        0 => Decision::Create,
        1 => Decision::Use(candidates[0].id.clone()),
        n => Decision::Select(n),
    })
}
pub fn accepted(a: &App, id: &str, disposition: &str) -> Value {
    let rule = a.rules.items.iter().find(|r| r.id == id).unwrap();
    let run = &a.runs[id];
    json!({"version":1,"outcome":"accepted","disposition":disposition,"ruleId":id,"ruleRevision":rule.revision,"runId":run.request["operationId"],"state":run.snapshot["state"]})
}
pub fn error(message: String) -> RpcError {
    let code = message
        .split('：')
        .next()
        .filter(|s| s.chars().all(|c| c.is_ascii_alphanumeric()))
        .unwrap_or("handlerError");
    RpcError {
        code: -32000,
        data: json!({"code":code}),
        message,
    }
}
pub fn handle(service: &Arc<Service>, client: &Client, p: &Value) -> Result<Value, RpcError> {
    let payload = &p["payload"];
    if payload["version"] != 1 {
        return Err(error("invalidParams：未知业务版本".into()));
    }
    match p["name"].as_str() {
        Some("network.tcpForward.ensure.v1") => {
            let target = Target::literal(&payload["target"])
                .map_err(|e| error(format!("invalidParams：{e}")))?;
            let name = payload["suggestedName"]
                .as_str()
                .filter(|s| s.chars().count() <= 128 && !s.chars().any(char::is_control))
                .ok_or_else(|| error("invalidParams：建议名称无效".into()))?;
            let selected = payload.get("selectedRule").unwrap_or(&Value::Null);
            if !selected.is_null()
                && (!selected["id"].is_string() || !selected["revision"].is_u64())
            {
                return Err(error("invalidParams：规则选择无效".into()));
            }
            service
                .ensure(&p["context"], target, name, selected)
                .map_err(error)
        }
        Some("network.rule.reveal.v1") => {
            let id = payload["ruleId"]
                .as_str()
                .ok_or_else(|| error("invalidParams：缺少规则 ID".into()))?;
            {
                let a = service.state.lock().unwrap();
                if !a
                    .rules
                    .items
                    .iter()
                    .any(|r| r.id == id && p["context"]["serverId"] == r.server)
                {
                    return Err(error("ruleConflict：规则已删除或不属于当前服务器".into()));
                }
            }
            let result=client.call_detailed("ui.openView",json!({"expectedContext":p["context"],"commandId":p["commandId"],"requestId":format!("open:{}",p["commandId"].as_str().unwrap_or("")),"viewType":"manager"}))?;
            if result["closed"] != true {
                let mut a = service.state.lock().unwrap();
                let server = p["context"]["serverId"].as_str().unwrap_or("");
                let offset = a
                    .rules
                    .items
                    .iter()
                    .filter(|r| r.server == server)
                    .position(|r| r.id == id)
                    .map(|i| i / 20 * 20)
                    .unwrap_or(0);
                if let Some(v) = result["viewId"]
                    .as_str()
                    .and_then(|view| a.views.get_mut(view))
                    .filter(|v| v.context == p["context"])
                {
                    v.navigation = json!({"selected":id,"offset":offset});
                }
            }
            Ok(result)
        }
        _ => Err(error("invalidParams：未知网络命令".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        rules::{Rule, Rules},
        runtime::View,
    };
    use std::collections::HashMap;
    fn setup() -> (App, Value, Target) {
        let context = json!({"workspaceId":"w","serverId":"s","connectionId":"c","generation":1});
        let rule=Rule::parse(&json!({"id":"one","serverId":"s","revision":1,"name":"one","bind":{"host":"127.0.0.1","port":0},"type":"tcpForward","target":{"host":"::1","port":80}})).unwrap();
        let mut rules =
            Rules::open(std::env::temp_dir().join(format!("sodeck-match-{}", std::process::id())));
        rules.items = vec![rule];
        let mut views = HashMap::new();
        views.insert(
            "manager".into(),
            View {
                server: "s".into(),
                kind: "manager".into(),
                context: context.clone(),
                draft: Value::Null,
                dirty: false,
                navigation: Value::Null,
                draft_revision: 0,
            },
        );
        (
            App {
                rules,
                views,
                runs: HashMap::new(),
                checks: HashMap::new(),
                sequence: 0,
                write_view: None,
            },
            context,
            Target {
                host: "::1".into(),
                port: 80,
            },
        )
    }
    #[test]
    fn same_target_is_reused_and_ambiguity_is_explicit() {
        let (mut a, c, t) = setup();
        assert!(matches!(
            decide(&a, &c, &t, &Value::Null).unwrap(),
            Decision::Use(_)
        ));
        let mut second = a.rules.items[0].clone();
        second.id = "two".into();
        a.rules.items.push(second);
        assert!(matches!(
            decide(&a, &c, &t, &Value::Null).unwrap(),
            Decision::Select(2)
        ));
        assert!(
            matches!(decide(&a,&c,&t,&json!({"id":"two","revision":1})).unwrap(),Decision::Use(id)if id=="two")
        );
        assert!(decide(&a, &c, &t, &json!({"id":"two","revision":0})).is_err());
        a.start("manager", "one").unwrap();
        assert!(matches!(decide(&a,&c,&t,&Value::Null).unwrap(),Decision::Use(id)if id=="one"));
    }
    #[test]
    fn endpoint_changes_unknown_state_and_foreign_context_are_not_reused() {
        let (mut a, c, t) = setup();
        a.start("manager", "one").unwrap();
        a.rules.items[0].name = "rename".into();
        assert!(decide(&a, &c, &t, &Value::Null).is_ok());
        a.rules.items[0].bind.port = 123;
        assert!(decide(&a, &c, &t, &Value::Null)
            .unwrap_err()
            .contains("ruleConflict"));
        a.rules.items[0].bind.port = 0;
        a.runs.get_mut("one").unwrap().query_error = Some("lost".into());
        assert!(decide(&a, &c, &t, &Value::Null)
            .unwrap_err()
            .contains("tunnelStateUnknown"));
        let r = a.runs.get_mut("one").unwrap();
        r.query_error = None;
        r.request["expectedContext"]["generation"] = json!(2);
        assert!(decide(&a, &c, &t, &Value::Null)
            .unwrap_err()
            .contains("tunnelBusy"));
        let r = a.runs.get_mut("one").unwrap();
        r.snapshot["cleanupComplete"] = json!(true);
        r.released = true;
        assert!(decide(&a, &c, &t, &Value::Null).is_ok());
    }
    #[test]
    fn nonloopback_rules_and_other_servers_do_not_expand_quick_binding() {
        let (mut a, c, t) = setup();
        a.rules.items[0].bind.host = "0.0.0.0".parse().unwrap();
        assert!(matches!(
            decide(&a, &c, &t, &Value::Null).unwrap(),
            Decision::Create
        ));
        a.rules.items[0].bind.host = "127.0.0.1".parse().unwrap();
        a.rules.items[0].server = "other".into();
        assert!(matches!(
            decide(&a, &c, &t, &Value::Null).unwrap(),
            Decision::Create
        ));
    }
}

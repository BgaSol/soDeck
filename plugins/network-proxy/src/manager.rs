use crate::{
    rules::Rules,
    socks5::{self, Action, Socks},
};
use serde_json::{json, Value};
use sodeck_plugin_client::Client;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
pub struct View {
    pub server: String,
    pub kind: String,
    pub context: Value,
    pub draft: Value,
    pub dirty: bool,
    pub navigation: Value,
}
pub struct Run {
    pub rule: Value,
    pub view: String,
    pub listener: String,
    pub snapshot: Value,
    pub restart: bool,
    last_error: Option<Value>,
    clients: HashMap<String, Peer>,
}
struct Peer {
    socks: Option<Socks>,
    remote: Option<String>,
    created: Instant,
}
pub struct App {
    pub rules: Rules,
    pub views: HashMap<String, View>,
    pub runs: HashMap<String, Run>,
    pub checks: HashMap<String, Value>,
    pub deletes: HashMap<String, (String, u64)>,
}
pub fn name(r: &Value) -> String {
    r["name"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| {
            if r["type"] == "socks5" {
                "SOCKS5 代理".into()
            } else {
                format!(
                    "转发至 {}:{}",
                    r["target"]["host"].as_str().unwrap_or(""),
                    r["target"]["port"]
                )
            }
        })
}
impl App {
    pub fn snapshot(&self, server: &str, offset: usize) -> Value {
        let total = self
            .rules
            .items
            .iter()
            .filter(|r| r["serverId"] == server)
            .count();
        let offset = offset.min(total.saturating_sub(1) / 20 * 20);
        let rows:Vec<_>=self.rules.items.iter().filter(|r|r["serverId"]==server).skip(offset).take(20).map(|r|json!({"rule":r,"name":name(r),"run":self.runs.get(r["id"].as_str().unwrap()).map(|v|json!({"ruleRevision":v.rule["revision"],"lastError":v.last_error,"snapshot":({let mut snapshot=v.snapshot.clone();snapshot.as_object_mut().map(|s|s.remove("clients"));snapshot})}))})).collect();
        let checks: serde_json::Map<String, Value> = rows
            .iter()
            .filter_map(|row| {
                let id = row["rule"]["id"].as_str()?;
                self.checks
                    .get(id)
                    .map(|check| (id.to_owned(), check.clone()))
            })
            .collect();
        json!({"rules":rows,"storageError":self.rules.error,"checks":checks,"total":total,"offset":offset})
    }
    pub fn start(&mut self, c: &Client, view: &str, id: &str) -> Result<(), String> {
        let v = self.views.get(view).ok_or("工作区已关闭")?;
        if v.kind != "manager" {
            return Err("需要网络侧栏".into());
        }
        let r = self
            .rules
            .items
            .iter()
            .find(|r| r["id"] == id && r["serverId"] == v.server)
            .ok_or("规则不存在")?
            .clone();
        if let Some(old) = self.runs.get(id) {
            if !matches!(old.snapshot["state"].as_str(), Some("closed" | "failed")) {
                return Ok(());
            }
        }
        let operation = format!(
            "{id}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let result = c.call(
            "net.listener.open",
            json!({"viewId":view,"operationId":operation,"bind":r["bind"]}),
        )?;
        let listener = result["listenerId"]
            .as_str()
            .ok_or("没有返回监听 ID")?
            .to_owned();
        self.runs.insert(
            id.into(),
            Run {
                rule: r,
                view: view.into(),
                listener,
                snapshot: json!({"state":"starting"}),
                restart: false,
                last_error: None,
                clients: HashMap::new(),
            },
        );
        self.checks.remove(id);
        Ok(())
    }
    pub fn stop(&mut self, c: &Client, id: &str) -> Result<(), String> {
        self.checks.remove(id);
        if let Some(r) = self.runs.get_mut(id) {
            c.call("net.close", json!({"id":r.listener}))?;
            r.snapshot["state"] = json!("stopping");
            r.restart = false;
            r.clients.clear();
        }
        Ok(())
    }
    pub fn tick(&mut self, c: &Client) {
        let views: Vec<_> = self
            .views
            .iter()
            .filter(|(_, v)| v.kind == "manager")
            .map(|(id, _)| id.clone())
            .collect();
        for view in views {
            let Ok(result) = c.call("net.poll", json!({"viewId":view})) else {
                continue;
            };
            for item in result["items"].as_array().into_iter().flatten() {
                let Some(run) = self
                    .runs
                    .values_mut()
                    .find(|r| r.listener == item["listenerId"].as_str().unwrap_or(""))
                else {
                    continue;
                };
                let Some(id) = item["streamId"].as_str() else {
                    continue;
                };
                if run.clients.len() >= 16 {
                    let _ = c.call("net.close", json!({"id":id}));
                    continue;
                }
                let peer = Peer {
                    socks: if run.rule["type"] == "socks5" {
                        Some(Socks::default())
                    } else {
                        None
                    },
                    remote: None,
                    created: Instant::now(),
                };
                run.clients.entry(id.into()).or_insert(peer);
            }
        }
        for run in self.runs.values_mut() {
            if matches!(run.snapshot["state"].as_str(), Some("closed" | "failed")) {
                continue;
            }
            match c.call("net.listener.snapshot", json!({"listenerId":run.listener})) {
                Ok(snapshot) => {
                    if matches!(snapshot["state"].as_str(), Some("closed" | "failed")) {
                        if snapshot["state"] == "failed" {
                            let _=c.call("messages.publish",json!({"title":"网络隧道已中断","body":format!("{}：{}",name(&run.rule),snapshot["error"]["message"].as_str().unwrap_or("连接已断开")),"level":"warning","notify":true,"dedupeKey":format!("tunnel-{}",run.rule["serverId"])}));
                        }
                        run.clients.clear();
                    }
                    run.snapshot = snapshot;
                }
                Err(e) => {
                    run.snapshot =
                        json!({"state":"failed","error":{"stage":"cleanup","message":e}});
                    run.clients.clear();
                    continue;
                }
            }
            run.clients.retain(|id,peer|{
                let result=advance(c,&run.listener,&run.rule,id,peer);
                match result {Ok(done)=>!done,Err(e)=>{
                    if peer.socks.is_some(){let code=if e.contains("超时"){6}else if e.to_lowercase().contains("refused"){5}else{1};let _=c.call("net.stream.write",json!({"streamId":id,"bytes":socks5::reply(code)}));let _=c.call("net.stream.finish",json!({"streamId":id}));}else{let _=c.call("net.close",json!({"id":id}));}
                    if let Some(remote)=&peer.remote{let _=c.call("net.close",json!({"id":remote}));}
                    let stage=if peer.remote.is_some(){"targetConnect"}else{"protocol"};
                    run.last_error=Some(json!({"stage":stage,"message":e}));
                    let _=c.call("logging.write",json!({"level":"warn","message":"隧道客户端连接失败","fields":{"ruleId":run.rule["id"],"stage":stage,"error":e}}));false
                }}
            });
        }
        let pending: Vec<_> = self
            .runs
            .iter()
            .filter(|(_, r)| {
                r.restart && matches!(r.snapshot["state"].as_str(), Some("closed" | "failed"))
            })
            .map(|(id, r)| (id.clone(), r.view.clone()))
            .collect();
        for (id, view) in pending {
            if let Err(e) = self.start(c, &view, &id) {
                // A save-and-close temporarily freezes the workspace. Keep the
                // restart pending; disposing the manager cancels it on a real close.
                if e == "工作区正在关闭" || e == "工作区正在确认关闭" {
                    continue;
                }
                if let Some(r) = self.runs.get_mut(&id) {
                    r.restart = false;
                    r.snapshot = json!({"state":"failed","error":{"message":e}})
                }
            }
        }
        let deletes: Vec<_> = self
            .deletes
            .iter()
            .filter(|(id, _)| {
                self.runs.get(*id).is_none_or(|r| {
                    matches!(r.snapshot["state"].as_str(), Some("closed" | "failed"))
                })
            })
            .map(|(id, v)| (id.clone(), v.clone()))
            .collect();
        for (id, (server, revision)) in deletes {
            match self.rules.delete(&server, &id, revision) {
                Ok(()) => {
                    self.runs.remove(&id);
                }
                Err(e) => {
                    let _ = c.call(
                        "messages.publish",
                        json!({"title":"删除规则失败","body":e,"level":"error"}),
                    );
                }
            }
            self.deletes.remove(&id);
        }
    }
}
fn target(c: &Client, listener: &str, id: &str, host: &str, port: u16) -> Result<String, String> {
    c.call("net.tcp.open",json!({"listenerId":listener,"clientStreamId":id,"operationId":"target","host":host,"port":port}))?["streamId"].as_str().map(str::to_owned).ok_or("没有返回远端流".into())
}
fn advance(
    c: &Client,
    listener: &str,
    rule: &Value,
    id: &str,
    p: &mut Peer,
) -> Result<bool, String> {
    if p.created.elapsed() > Duration::from_secs(14) {
        return Err("代理协商超时".into());
    }
    if let Some(remote) = &p.remote {
        let status = c.call("net.stream.status", json!({"streamId":remote}))?;
        return match status["state"].as_str() {
            Some("ready") => {
                if p.socks.is_some() {
                    c.call(
                        "net.stream.write",
                        json!({"streamId":id,"bytes":socks5::reply(0)}),
                    )?;
                }
                c.call("net.stream.splice",json!({"listenerId":listener,"leftId":id,"rightId":remote,"operationId":"relay"}))?;
                Ok(true)
            }
            Some("failed" | "closed") => {
                Err(status["error"].as_str().unwrap_or("目标连接失败").into())
            }
            _ => Ok(false),
        };
    }
    let Some(socks) = &mut p.socks else {
        p.remote = Some(target(
            c,
            listener,
            id,
            rule["target"]["host"].as_str().unwrap(),
            rule["target"]["port"].as_u64().unwrap() as u16,
        )?);
        return Ok(false);
    };
    let read = c.call(
        "net.stream.read",
        json!({"streamId":id,"maxBytes":socks.next()}),
    )?;
    if read["pending"] == true {
        return Ok(false);
    }
    if read["eof"] == true {
        return Err("客户端已关闭".into());
    }
    let bytes: Vec<u8> =
        serde_json::from_value(read["data"].clone()).map_err(|_| "协商响应无效")?;
    match socks.feed(&bytes)? {
        Action::Read(needed) => {
            debug_assert!(needed > 0);
        }
        Action::Write(bytes) => {
            c.call("net.stream.write", json!({"streamId":id,"bytes":bytes}))?;
        }
        Action::Reject(bytes) => {
            c.call("net.stream.write", json!({"streamId":id,"bytes":bytes}))?;
            c.call("net.stream.finish", json!({"streamId":id}))?;
            return Ok(true);
        }
        Action::Connect(host, port) => p.remote = Some(target(c, listener, id, &host, port)?),
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pagination_clamps_after_delete_and_keeps_checks_with_their_server_page() {
        let mut rules = Rules::open(
            std::env::temp_dir().join(format!("sodeck-network-page-test-{}", std::process::id())),
        );
        rules.items = (0..21)
            .map(|i| json!({"id":format!("rule-{i}"),"serverId":"server","name":"rule"}))
            .collect();
        let mut app = App {
            rules,
            views: HashMap::new(),
            runs: HashMap::new(),
            checks: (0..21)
                .map(|i| (format!("rule-{i}"), json!({"ok":true})))
                .collect(),
            deletes: HashMap::new(),
        };
        let page = app.snapshot("server", 20);
        assert_eq!(page["rules"].as_array().unwrap().len(), 1);
        assert_eq!(page["checks"].as_object().unwrap().len(), 1);
        assert_eq!(page["checks"]["rule-20"]["ok"], true);
        app.rules.items.pop();
        let page = app.snapshot("server", 20);
        assert_eq!(page["offset"], 0);
        assert_eq!(page["rules"].as_array().unwrap().len(), 20);
        assert!(page["checks"].get("rule-20").is_none());
        let page = app.snapshot("another-server", 0);
        assert_eq!(page["total"], 0);
        assert!(page["checks"].as_object().unwrap().is_empty());
    }
}

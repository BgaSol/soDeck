use crate::{
    rules::Rules,
    socks5::{Action, Socks},
};
use serde_json::{json, Value};
use socker::socks5::Reply;
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
    pub request: Value,
    pub stop_requested: bool,
    pub query_error: Option<String>,
    pub observed_at: Option<u64>,
    sent: bool,
    pub released: bool,
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
    pub sequence: u64,
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
        let rows:Vec<_>=self.rules.items.iter().filter(|r|r["serverId"]==server).skip(offset).take(20).map(|r|json!({"rule":r,"name":name(r),"run":self.runs.get(r["id"].as_str().unwrap()).map(|v|json!({"ruleRevision":v.rule["revision"],"canStart":v.snapshot["cleanupComplete"]==true&&v.released&&v.query_error.is_none(),"lastError":v.last_error,"queryError":v.query_error,"observedAt":v.observed_at,"stopRequested":v.stop_requested,"snapshot":({let mut snapshot=v.snapshot.clone();snapshot.as_object_mut().map(|s|s.remove("clients"));snapshot})}))})).collect();
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
    pub fn start(&mut self, _c: &Client, view: &str, id: &str) -> Result<(), String> {
        let v = self.views.get(view).ok_or("工作区已关闭")?;
        if v.kind != "manager" {
            return Err("需要网络侧栏".into());
        }
        if self.runs.get(id).is_some_and(|r| {
            r.snapshot["cleanupComplete"] != true || !r.released || r.query_error.is_some()
        }) {
            return Err("原监听尚未确认清理，请先停止并核对".into());
        }
        if self.runs.values().any(|r| {
            r.view == view && r.listener.is_empty() && r.snapshot["cleanupComplete"] != true
        }) {
            return Err("此页面有尚未确认的启动，请先查询或停止".into());
        }
        let rule = self
            .rules
            .items
            .iter()
            .find(|r| r["id"] == id && r["serverId"] == v.server)
            .ok_or("规则不存在")?
            .clone();
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or("操作序号已用尽，请重启插件")?;
        let request = json!({"viewId":view,"expectedContext":v.context,"operationId":format!("{id}/{}",self.sequence),"sequence":self.sequence.to_string(),"bind":rule["bind"]});
        self.runs.insert(
            id.into(),
            Run {
                rule,
                view: view.into(),
                listener: String::new(),
                snapshot: json!({"state":"starting","cleanupComplete":false}),
                request,
                stop_requested: false,
                query_error: None,
                observed_at: None,
                sent: false,
                released: false,
                last_error: None,
                clients: HashMap::new(),
            },
        );
        self.checks.remove(id);
        Ok(())
    }
    pub fn stop(&mut self, _c: &Client, id: &str) -> Result<(), String> {
        self.checks.remove(id);
        if let Some(r) = self.runs.get_mut(id) {
            r.stop_requested = true;
        }
        Ok(())
    }
}

// Snapshot requests and stream negotiation never hold App while waiting for RPC.
pub fn tick(shared: &std::sync::Arc<std::sync::Mutex<Option<App>>>, c: &Client) {
    let mut ids = {
        let state = shared.lock().unwrap();
        let Some(a) = state.as_ref() else { return };
        a.runs
            .iter()
            .map(|(id, r)| (id.clone(), !r.stop_requested))
            .collect::<Vec<_>>()
    };
    ids.sort_by_key(|(_, normal)| *normal);
    for (id, _) in ids {
        let (request, method) = {
            let mut state = shared.lock().unwrap();
            let Some(r) = state.as_mut().and_then(|a| a.runs.get_mut(&id)) else {
                continue;
            };
            if r.snapshot["cleanupComplete"] == true && r.released {
                continue;
            }
            let method = if r.snapshot["cleanupComplete"] == true {
                "net.listener.release"
            } else if r.stop_requested {
                "net.listener.cancel"
            } else if !r.sent {
                r.sent = true;
                "net.listener.create"
            } else {
                "net.listener.lookup"
            };
            (r.request.clone(), method)
        };
        let result = c.call_detailed(method, request.clone());
        let mut state = shared.lock().unwrap();
        let Some(r) = state
            .as_mut()
            .and_then(|a| a.runs.get_mut(&id))
            .filter(|r| r.request == request)
        else {
            continue;
        };
        let was_failed = r.snapshot["state"] == "failed";
        match result {
            Ok(value) => {
                if method == "net.listener.release" {
                    r.released = true;
                    r.query_error = None;
                    continue;
                }
                if let Some(listener) = value["listenerId"].as_str() {
                    r.listener = listener.into();
                }
                if value["state"] == "retired" && value["cleanupComplete"] == true {
                    r.snapshot["cleanupComplete"] = json!(true);
                    r.snapshot["state"] = json!("closed");
                } else if value["snapshot"].is_object() {
                    r.snapshot = value["snapshot"].clone();
                } else {
                    r.query_error = Some("核心尚未观测到创建；可以继续查询或停止原启动".into());
                    continue;
                }
                r.query_error = None;
                r.observed_at = Some(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                );
            }
            Err(e) => r.query_error = Some(e.message),
        }
        let notification=(!was_failed&&r.snapshot["state"]=="failed"&&!r.stop_requested).then(||json!({"title":"网络隧道已中断","body":format!("{}：{}",name(&r.rule),r.snapshot["error"]["message"].as_str().unwrap_or("连接已断开")),"level":"warning","notify":true,"dedupeKey":format!("tunnel-{}",r.rule["serverId"])}));
        drop(state);
        if let Some(message) = notification {
            let _ = c.call("messages.publish", message);
        }
    }
    let views = {
        let state = shared.lock().unwrap();
        let Some(a) = state.as_ref() else { return };
        a.views
            .iter()
            .filter(|(_, v)| v.kind == "manager")
            .filter(|(view, _)| {
                !a.runs.values().any(|r| {
                    r.view == **view
                        && r.listener.is_empty()
                        && r.snapshot["cleanupComplete"] != true
                })
            })
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>()
    };
    for view in views {
        let Ok(result) = c.call("net.poll", json!({"viewId":view})) else {
            continue;
        };
        let mut close = Vec::new();
        {
            let mut state = shared.lock().unwrap();
            let Some(a) = state.as_mut() else { return };
            for item in result["items"].as_array().into_iter().flatten() {
                let Some(id) = item["streamId"].as_str() else {
                    continue;
                };
                if let Some(r) = a.runs.values_mut().find(|r| {
                    r.listener == item["listenerId"].as_str().unwrap_or("")
                        && !r.stop_requested
                        && r.query_error.is_none()
                }) {
                    if r.clients.len() < 16 {
                        r.clients.entry(id.into()).or_insert(Peer {
                            socks: if r.rule["type"] == "socks5" {
                                Some(Socks::default())
                            } else {
                                None
                            },
                            remote: None,
                            created: Instant::now(),
                        });
                        continue;
                    }
                }
                close.push(id.to_owned());
            }
        }
        for id in close {
            let _ = c.call("net.close", json!({"id":id}));
        }
    }
    let ids = {
        let state = shared.lock().unwrap();
        let Some(a) = state.as_ref() else { return };
        a.runs.keys().cloned().collect::<Vec<_>>()
    };
    for id in ids {
        let (request, listener, rule, mut clients) = {
            let mut state = shared.lock().unwrap();
            let Some(r) = state.as_mut().and_then(|a| a.runs.get_mut(&id)) else {
                continue;
            };
            if r.stop_requested || r.query_error.is_some() || r.snapshot["state"] != "running" {
                continue;
            }
            (
                r.request.clone(),
                r.listener.clone(),
                r.rule.clone(),
                std::mem::take(&mut r.clients),
            )
        };
        let mut last_error = None;
        clients.retain(|peer_id,peer|{
            let active={let state=shared.lock().unwrap();state.as_ref().and_then(|a|a.runs.get(&id)).is_some_and(|r|r.request==request&&!r.stop_requested)};
            let result=if active{advance(c,&listener,&rule,peer_id,peer)}else{Err("监听正在停止".into())};
            match result{Ok(done)=>!done,Err(e)=>{
                let response=peer.socks.as_mut().and_then(|s|s.failure_reply(if e.contains("超时"){Reply::TTL_EXPIRED}else if e.to_lowercase().contains("refused"){Reply::CONNECTION_REFUSED}else{Reply::GENERAL_FAILURE}));
                if active{if let Some(bytes)=response{let _=c.call("net.stream.write",json!({"streamId":peer_id,"bytes":bytes}));let _=c.call("net.stream.finish",json!({"streamId":peer_id}));}else{let _=c.call("net.close",json!({"id":peer_id}));}}
                if let Some(remote)=&peer.remote{let _=c.call("net.close",json!({"id":remote}));}
                let stage=if peer.remote.is_some(){"targetConnect"}else{"protocol"};last_error=Some(json!({"stage":stage,"message":e}));
                let _=c.call("logging.write",json!({"level":"warn","message":"隧道客户端连接失败","fields":{"ruleId":id,"stage":stage,"error":e}}));false}}
        });
        let mut state = shared.lock().unwrap();
        if let Some(r) = state
            .as_mut()
            .and_then(|a| a.runs.get_mut(&id))
            .filter(|r| r.request == request)
        {
            r.clients.extend(clients);
            if last_error.is_some() {
                r.last_error = last_error;
            }
        }
    }
    let mut state = shared.lock().unwrap();
    if let Some(a) = state.as_mut() {
        a.runs
            .retain(|_, r| a.views.contains_key(&r.view) || !r.released);
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
                if let Some(socks) = &mut p.socks {
                    c.call(
                        "net.stream.write",
                        json!({"streamId":id,"bytes":socks.succeeded()}),
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
            sequence: 0,
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

use crate::{
    rules::Rule,
    service::{now, Service},
};
use serde_json::{json, Value};
use sodeck_plugin_client::{shared_state, Client};
use std::{
    collections::HashMap,
    sync::{mpsc::Receiver, Arc},
    time::{Duration, Instant},
};
pub const SERVICE: &str = "network.tunnelService.v1";
pub const RULES: &str = "network.tunnelRules.v1";
struct Published {
    revision: String,
    value: Value,
}
fn put(
    client: &Client,
    view: &str,
    context: &Value,
    topic: &str,
    key: &str,
    value: &Value,
    cache: &mut HashMap<String, Published>,
) -> Result<(), String> {
    let identity = json!([context, view, topic, key]).to_string();
    if cache.get(&identity).is_some_and(|p| p.value == *value) {
        return Ok(());
    }
    let revision = cache
        .get(&identity)
        .map(|p| p.revision.as_str())
        .unwrap_or("0");
    let request = json!({"topic":topic,"key":key,"expectedRevision":revision,"value":value});
    match shared_state::request(client, view, context, "put", request) {
        Ok(result) => {
            cache.insert(
                identity,
                Published {
                    revision: result["revision"].as_str().ok_or("共享修订缺失")?.into(),
                    value: value.clone(),
                },
            );
            Ok(())
        }
        Err(error) => {
            // A lost put may already have committed. Read before the next CAS.
            let current = shared_state::request(
                client,
                view,
                context,
                "get",
                json!({"topic":topic,"key":key}),
            )
            .map_err(|_| error.message)?;
            let record = &current["record"];
            cache.insert(
                identity,
                Published {
                    revision: record["revision"].as_str().unwrap_or("0").into(),
                    value: record.get("value").cloned().unwrap_or(Value::Null),
                },
            );
            Err("共享状态待重新同步".into())
        }
    }
}
fn summary(value: &Value) -> Value {
    let mut value = value.clone();
    if let Some(failure) = value.get_mut("failure").and_then(Value::as_object_mut) {
        failure.remove("diagnostic");
    }
    if let Some(object) = value.as_object_mut() {
        object.remove("diagnostic");
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shared_failures_keep_recovery_facts_without_raw_diagnostics() {
        let value = json!({"ok":false,"target":{"host":"127.0.0.1","port":8080},"failure":{"code":"forwardingDenied","message":"拒绝转发","hint":"检查策略","diagnostic":"raw remote error"}});
        let result = summary(&value);
        assert!(result["failure"].get("diagnostic").is_none());
        assert_eq!(result["failure"]["hint"], value["failure"]["hint"]);
        assert_eq!(result["target"], value["target"]);
        assert!(value["failure"].get("diagnostic").is_some());
    }
}

pub fn start(service: Arc<Service>, client: Client, receive: Receiver<()>) {
    std::thread::spawn(move || {
        let mut cache: HashMap<String, Published> = HashMap::new();
        let mut last_heartbeat = Instant::now() - Duration::from_secs(5);
        let mut heartbeat = now();
        while !client.is_closed() {
            let _ = receive.recv_timeout(Duration::from_secs(1));
            if last_heartbeat.elapsed() >= Duration::from_secs(5) {
                heartbeat = now();
                last_heartbeat = Instant::now();
            }
            let plans = {
                let a = service.state.lock().unwrap();
                a.views.iter().filter(|(_,v)|v.kind=="manager").map(|(id,v)|{
                let rows=a.rules.items.iter().filter(|r|r.server==v.server).map(|rule|{
                    let run=a.runs.get(&rule.id).map(|r|{
                        let elsewhere=r.request["expectedContext"]!=v.context;
                        json!({"runId":r.request["operationId"],"lastError":r.last_error.as_ref().map(summary),"ruleRevision":r.rule.revision,"rule":r.rule.value(),"ownedElsewhere":elsewhere,"canStart":r.snapshot["cleanupComplete"]==true&&r.released&&r.query_error.is_none(),"released":r.released,"stopRequested":r.stop_requested,"queryError":r.query_error.as_ref().map(|s|s.chars().take(1024).collect::<String>()),"observedAt":r.observed_at.map(|s|s*1000),"snapshot":{"state":r.snapshot["state"],"cleanupComplete":r.snapshot["cleanupComplete"],"address":if elsewhere{Value::Null}else{r.snapshot["address"].clone()},"error":r.snapshot["error"]["message"].as_str().map(|s|s.chars().take(1024).collect::<String>()),"failure":r.snapshot.get("failure").map(summary)}})
                    });
                    (rule.id.clone(),json!({"version":1,"rule":Rule::value(rule),"run":run,"check":a.checks.get(&rule.id).map(summary)}))
                }).collect::<Vec<_>>();
                (id.clone(),v.context.clone(),a.rules.error.clone(),rows)
            }).collect::<Vec<_>>()
            };
            let live = plans
                .iter()
                .map(|(view, context, _, _)| json!([context, view]).to_string())
                .collect::<Vec<_>>();
            cache.retain(|key, _| {
                serde_json::from_str::<Value>(key)
                    .ok()
                    .is_some_and(|v| live.contains(&json!([v[0], v[1]]).to_string()))
            });
            for (view, context, error, rows) in plans {
                let status = |state: &str| json!({"version":1,"ready":true,"publishState":state,"storageError":error,"observedAt":heartbeat});
                let keys = rows.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>();
                let mut synced = true;
                for (id, value) in rows {
                    if put(&client, &view, &context, RULES, &id, &value, &mut cache).is_err() {
                        synced = false;
                        break;
                    }
                }
                let obsolete = cache
                    .keys()
                    .filter_map(|key| {
                        let v: Value = serde_json::from_str(key).ok()?;
                        (v[0] == context
                            && v[1] == view
                            && v[2] == RULES
                            && !keys.iter().any(|k| v[3] == *k))
                        .then(|| (key.clone(), v[3].as_str().unwrap().to_owned()))
                    })
                    .collect::<Vec<_>>();
                for (identity, key) in obsolete {
                    let revision = &cache[&identity].revision;
                    match shared_state::request(
                        &client,
                        &view,
                        &context,
                        "remove",
                        json!({"topic":RULES,"key":key,"expectedRevision":revision}),
                    ) {
                        Ok(_) => {
                            cache.remove(&identity);
                        }
                        Err(_) => {
                            synced = false;
                            if let Ok(current) = shared_state::request(
                                &client,
                                &view,
                                &context,
                                "get",
                                json!({"topic":RULES,"key":key}),
                            ) {
                                if current["record"]["present"] == false {
                                    cache.remove(&identity);
                                } else if let Some(item) = cache.get_mut(&identity) {
                                    item.revision = current["record"]["revision"]
                                        .as_str()
                                        .unwrap_or("0")
                                        .into();
                                }
                            }
                        }
                    }
                }
                let _ = put(
                    &client,
                    &view,
                    &context,
                    SERVICE,
                    "current",
                    &status(if synced { "synced" } else { "limited" }),
                    &mut cache,
                );
            }
            std::thread::sleep(Duration::from_millis(150));
        }
    });
}

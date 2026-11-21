use crate::{
    app::App,
    identity::{MatchMode, Needle},
    targets::{Descriptor, Observation},
    tasks::failure,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sodeck_plugin_client::encoding::{base64, unbase64};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Params {
    query: Option<String>,
    #[serde(rename = "match")]
    mode: Option<MatchMode>,
    server_id: Option<String>,
    #[serde(default)]
    detail: bool,
    #[serde(default = "limit")]
    limit: usize,
    cursor: Option<String>,
}
fn limit() -> usize {
    20
}

pub fn patch_schema() -> Value {
    let string = json!({"type":"string","minLength":1,"maxLength":256});
    let route = json!({"type":"array","items":{"oneOf":[
        {"type":"object","properties":{"kind":{"const":"ssh"},"serverId":string},"required":["kind","serverId"],"additionalProperties":false},
        {"type":"object","properties":{"kind":{"const":"socks5"},"proxyId":string},"required":["kind","proxyId"],"additionalProperties":false}
    ]}});
    let secret = json!({"oneOf":[
        {"type":"object","properties":{"action":{"const":"keep"},"remember":{"type":"boolean"}},"required":["action","remember"],"additionalProperties":false},
        {"type":"object","properties":{"action":{"const":"replace"},"value":{"type":"string"},"remember":{"type":"boolean"}},"required":["action","value","remember"],"additionalProperties":false},
        {"type":"object","properties":{"action":{"const":"clear"}},"required":["action"],"additionalProperties":false}
    ]});
    json!({"type":"object","minProperties":1,"properties":{
        "name":string,"host":string,"port":{"type":"integer","minimum":1,"maximum":65535},"username":string,
        "tagIds":{"type":"array","items":string},"route":route,
        "authentication":{"type":"object","properties":{"type":{"type":"string","enum":["password","privateKey"]},"privateKeyPath":{"type":"string","minLength":1,"maxLength":4096},"secret":secret},"required":["type"],"additionalProperties":false}
    },"additionalProperties":false})
}
pub fn node_schema() -> Value {
    let mut schema = patch_schema();
    schema["required"] = json!(["host", "username"]);
    schema
}
pub async fn write(
    app: Arc<App>,
    name: &str,
    p: Value,
    ct: CancellationToken,
) -> Result<Value, Value> {
    let method = match name {
        "sodeck_server_create" => "servers.create",
        "sodeck_server_update" => "servers.update",
        "sodeck_server_delete" => "servers.delete",
        _ => unreachable!(),
    };
    let permit = app
        .target_reads
        .clone()
        .try_acquire_owned()
        .map_err(|_| failure("resourceBusy", "核心 RPC 位置已满"))?;
    if ct.is_cancelled() || app.shutdown.is_cancelled() {
        return Err(failure("requestCancelled", "服务器写入尚未派发"));
    }
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        if ct.is_cancelled() || app.shutdown.is_cancelled() {
            return Err(failure("requestCancelled", "服务器写入尚未派发"));
        }
        app.rpc(method, p)
    })
    .await
    .map_err(|_| failure("unavailable", "服务器写入结果未知，请查询目录确认"))?
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    run: String,
    revision: String,
    after: usize,
    query: Option<String>,
    mode: MatchMode,
    server: Option<String>,
    limit: usize,
}

fn row(app: &App, mut value: Value, detail: bool) -> Value {
    let current = value.get("current").cloned();
    let background = value.get("background").cloned();
    let context = value.get("context").cloned();
    if let Some(o) = value.as_object_mut() {
        for key in ["current", "background", "context"] {
            o.remove(key);
        }
    }
    value["target"] = Value::Null;
    if let Some(current) = current {
        if current["error"].is_object() {
            value["connectionError"] = current["error"].clone();
        } else {
            match serde_json::from_value::<Observation>(current) {
                Ok(observation) if observation.validate() => {
                    if let Some(background) = background {
                        if background["state"] == "created" {
                            if let Some(view) = background["viewId"].as_str() {
                                if let Some(target) = app.context_target(&observation.context, view)
                                {
                                    value["target"] = json!({"targetId":target.id,"serverId":observation.context.server_id,"workspaceId":observation.context.workspace_id,"target":observation.target,"targetSource":observation.target_source,"availability":observation.availability(),"capabilities":observation.capabilities});
                                }
                            }
                        } else if background["state"] == "failed" {
                            value["targetError"] = json!({"code":"targetInitializationFailed","message":background["error"]});
                        } else {
                            value["targetState"] = json!("preparing");
                        }
                    }
                }
                _ => value["connectionError"] = failure("invalidTarget", "核心会话观察无效"),
            }
        }
    }
    if detail {
        value["context"] = context.unwrap_or(Value::Null);
    }
    value
}

pub async fn discover(app: Arc<App>, p: &Value, ct: CancellationToken) -> Result<Value, Value> {
    let params: Params =
        serde_json::from_value(p.clone()).map_err(|e| failure("invalidParams", e.to_string()))?;
    if p.as_object().is_none_or(|o| o.values().any(Value::is_null))
        || !(1..=50).contains(&params.limit)
        || params.mode.is_some() && params.query.is_none()
        || params.server_id.is_some() && (params.query.is_some() || params.mode.is_some())
        || params
            .server_id
            .as_ref()
            .is_some_and(|s| s.is_empty() || s.len() > 256 || s.contains('\0'))
    {
        return Err(failure("invalidParams", "服务器查询参数组合无效"));
    }
    let query = params
        .query
        .map(|s| {
            if s.trim().is_empty() || s.len() > 256 || s.chars().any(char::is_control) {
                Err(failure("invalidParams", "query 无效"))
            } else {
                Ok(s.trim().to_string())
            }
        })
        .transpose()?;
    let mode = params.mode.unwrap_or_default();
    let cursor = params
        .cursor
        .map(|s| {
            if s.len() > 2048 {
                return Err(failure("invalidCursor", "游标超限"));
            }
            let bytes = unbase64(&s).map_err(|_| failure("invalidCursor", "游标无效"))?;
            serde_json::from_slice::<Cursor>(&bytes)
                .map_err(|_| failure("invalidCursor", "游标无效"))
        })
        .transpose()?;
    if cursor.as_ref().is_some_and(|c| {
        c.run != app.run
            || c.query != query
            || c.mode != mode
            || c.server != params.server_id
            || c.limit != params.limit
    }) {
        return Err(failure("invalidCursor", "游标属于另一范围或运行"));
    }
    let permit = app
        .target_reads
        .clone()
        .try_acquire_owned()
        .map_err(|_| failure("resourceBusy", "发现读取位置已满"))?;
    tokio::task::spawn_blocking(move||{
        let _permit=permit;let needle=query.as_ref().map(|q|Needle::new(q,mode));
        let after=cursor.as_ref().map(|c|c.after).unwrap_or(0);let mut revision=None;let mut native_cursor=None;let mut output=Vec::new();let mut count=0;let mut remaining=0;let mut position=0;let mut next_after=after;let mut previous_match=cursor.is_none();
        loop{
            if ct.is_cancelled()||app.shutdown.is_cancelled(){return Err(failure("requestCancelled","服务器发现已取消"));}
            let mut input=json!({"detail":params.detail,"limit":20,"viewType":"status"});
            if let Some(server)=&params.server_id{input["serverId"]=json!(server);}
            if let Some(c)=&native_cursor{input["cursor"]=json!(c);}
            let page=app.rpc("servers.list",input)?;
            let rev=page["catalogRevision"].as_str().ok_or_else(||failure("invalidResult","目录版本缺失"))?.to_owned();
            if revision.as_ref().is_some_and(|old|old!=&rev)||cursor.as_ref().is_some_and(|c|c.revision!=rev){return Err(failure("catalogChanged","资料目录发生变化，请从第一页查询"));}
            revision=Some(rev);
            let rows=page["servers"].as_array().ok_or_else(||failure("invalidResult","服务器资料页无效"))?;
            for item in rows{
                let descriptor:Descriptor=serde_json::from_value(json!({"name":item["name"],"protocol":item["protocol"],"host":item["host"],"port":item["port"],"endpoint":item["endpoint"],"username":item["username"]})).map_err(|e|failure("invalidResult",e.to_string()))?;
                let reasons=needle.as_ref().map(|n|n.matches(&descriptor)).unwrap_or_default();let matched=needle.is_none()||!reasons.is_empty();
                if cursor.is_some()&&position+1==after{previous_match=matched;}
                if matched{count+=1;if position>=after{remaining+=1;if output.len()<params.limit{let mut item=row(&app,item.clone(),params.detail);if needle.is_some(){item["matchedBy"]=json!(reasons);}output.push(item);next_after=position+1;}}}
                position+=1;
            }
            let more=page["hasMore"].as_bool().ok_or_else(||failure("invalidResult","目录分页标记无效"))?;
            if !more{break;}
            let next=page["nextCursor"].as_str().filter(|s|!s.is_empty()).ok_or_else(||failure("invalidResult","目录游标缺失"))?.to_owned();
            if native_cursor.as_ref()==Some(&next)||rows.is_empty(){return Err(failure("invalidResult","目录分页没有推进"));}native_cursor=Some(next);
        }
        if after>position||!previous_match{return Err(failure("invalidCursor","游标位置无效"));}
        let revision=revision.ok_or_else(||failure("invalidResult","目录版本缺失"))?;
        let more=remaining>output.len();
        let next=if more{Some(base64(&serde_json::to_vec(&Cursor{run:app.run.clone(),revision:revision.clone(),after:next_after,query,mode,server:params.server_id,limit:params.limit}).map_err(|e|failure("invalidCursor",e.to_string()))?))}else{None};
        Ok(json!({"runId":app.run,"catalogRevision":revision,"resolution":if count==0{"none"}else if count==1{"unique"}else{"ambiguous"},"matchCount":count,"servers":output,"hasMore":more,"nextCursor":next}))
    }).await.map_err(|e|failure("unavailable",e.to_string()))?
}

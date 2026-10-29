#![allow(dead_code)]
use sodeck_core::{Caller,Core};
use serde_json::{json,Value};
pub fn ssh_steps(ids:Value)->Value {json!({"steps":ids.as_array().unwrap().iter().map(|id|json!({"stepId":uuid::Uuid::new_v4().to_string(),"kind":"ssh","serverId":id})).collect::<Vec<_>>()})}
pub fn proxy_path(id:Value,resolve:&str)->Value{json!({"steps":[{"stepId":uuid::Uuid::new_v4().to_string(),"kind":"socks5","proxyId":id,"resolve":resolve}]})}
pub fn prefill(core:&Core,kind:&str,owner:&Value,input:Value)->Value {
    let call=|method,p|core.call(&Caller::shell(),method,p).unwrap();
    let (revision,dependencies)=if kind=="ssh" {
        let server=call("servers.routes.get",json!({"serverId":owner}));
        let dependencies=server["accessPath"]["steps"].as_array().unwrap().iter().map(|s|{
            let value=if s["kind"]=="ssh"{call("servers.get",json!({"serverId":s["serverId"]}))}else{call("proxies.socks5.get",json!({"proxyId":s["proxyId"]}))};
            json!({"stepId":s["stepId"],"kind":s["kind"],"id":value["id"],"host":value["host"],"port":value["port"],"username":value["username"],"authType":value["authType"]})
        }).collect::<Vec<_>>();(server["revision"].clone(),json!(dependencies))
    }else{(call("proxies.socks5.get",json!({"proxyId":owner}))["revision"].clone(),json!([]))};
    let context=call("credentials.configuration.prepare",json!({"target":{"kind":kind,"id":owner},"expectedOwnerRevision":revision,"expectedDependencies":dependencies}));
    let mut request=input;request["contextToken"]=context["contextToken"].clone();request["operationId"]=json!(uuid::Uuid::new_v4().to_string());
    if request.get("remember").is_none(){request["remember"]=json!(false);}
    call("credentials.configuration.set",request)
}

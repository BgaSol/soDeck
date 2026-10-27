//! Paged route diagnostics. Old strict v1 projections never truncate a path.
use super::*;
use crate::route::{ConnectionRoute, RouteNode, RoutePlan, digest};

pub(crate) fn saved_meta(plan: &RoutePlan) -> Value {
    meta(plan, json!({"kind":"saved","serverId":plan.target_server_id,"routeRevision":plan.revision}))
}
fn meta(plan: &RoutePlan, reference: Value) -> Value {
    json!({"targetServerId":plan.target_server_id,"targetName":plan.target().name,"routeRevision":plan.revision,"nodeCount":plan.nodes.len(),"reference":reference})
}
fn running_meta(c: &Connection) -> Value {
    meta(&c.route,json!({"kind":"running","serverId":c.server.id,"connectionId":c.id,"generation":c.generation,"routeRevision":c.route.revision}))
}
fn usage(c: &Connection) -> Value { if c.transport_live.load(Ordering::SeqCst){json!({"sessions":c.route.nodes.len(),"bridges":c.route.nodes.len()-1})}else{json!({"sessions":0,"bridges":0})} }
fn failure(c: &Connection) -> Option<usize> {let index=c.failure_hop.load(Ordering::SeqCst);(index!=usize::MAX).then_some(index)}

impl Core {
    pub(crate) fn route_v2_dispatch(&self, caller: &Caller, method: &str, p: &Value) -> Result<Value> {
        check("x-methods",method,p)?;
        // Save takes this lock itself; never perform a mutation and only then
        // discover that it violates the caller's route contract.
        if matches!(method,"servers.routes.v2.create"|"servers.routes.v2.update") {
            let saved=self.save_server_configuration(p,method.ends_with("update"),true)?;
            let _guard=self.configuration.lock().unwrap();
            return checked(method,self.route_server_value(field(&saved,"id")?)?);
        }
        if method=="connections.route.v2.get" {
            return checked(method,self.route_snapshot_v2(field(p,"serverId")?,field(p,"workspaceId")?)?);
        }
        let _guard=self.configuration.lock().unwrap();
        let result=match method {
            "servers.routes.v2.get"=>self.route_server_value(field(p,"serverId")?)?,
            "servers.routes.v2.list"=>{
                let servers=self.store.servers()?;
                let revision=digest(&json!(servers.iter().map(|s|(&s.id,s.revision)).collect::<Vec<_>>()));
                let (items,_,next)=page(servers.into_iter().map(|s|{
                    let mut value=serde_json::to_value(&s).expect("server");
                    value["hasSecret"]=json!(self.secrets.remembered(&s.id));
                    value["connectionRoute"]=serde_json::to_value(self.store.connection_route(&s.id)?).expect("route");
                    Ok(value)
                }),caller,method,p,&revision)?;
                json!({"servers":items,"nextCursor":next})
            },
            "servers.routes.v2.preview"=>{
                let server:Server=serde_json::from_value(p["server"].clone())?;
                let route:ConnectionRoute=serde_json::from_value(p["connectionRoute"].clone())?;
                let nodes=self.store.preview_route(&server,&route)?;
                let revisions=self.credential_revisions.lock().unwrap();
                let plan=RoutePlan::new(nodes.into_iter().map(|server|RouteNode{credential_revision:*revisions.get(&server.id).unwrap_or(&0),server}).collect())?;
                self.node_page(&plan,saved_meta(&plan),caller,method,p)?
            },
            "connections.route.v2.nodes"=>{
                let reference=&p["reference"];
                let plan=self.resolve_route_reference(reference)?;
                self.node_page(&plan,meta(&plan,reference.clone()),caller,method,p)?
            },
            "connections.route.v2.list"=>{
                let rows=self.resource_routes_v2(None);
                let revision=digest(&json!(rows.iter().map(|v|(&v["connectionId"],&v["generation"])).collect::<Vec<_>>()));
                let (items,_,next)=page(rows.into_iter().map(Ok),caller,method,p,&revision)?;
                let used=self.transport_budget.lock().unwrap();
                json!({"connections":items,"usage":{"sessions":used.sessions,"bridges":used.bridges},"nextCursor":next})
            },
            "servers.routes.v2.dependencies"=>{
                let id=field(p,"serverId")?;self.store.server(id)?;
                let mut saved=self.store.dependents(id)?;saved.sort_by(|a,b|a.id.cmp(&b.id));
                let mut rows=Vec::new();
                for server in saved {
                    let direct=self.store.connection_route(&server.id)?.jump_id()==Some(id);
                    rows.push(json!({"kind":"saved","server":server,"direct":direct}));
                }
                rows.extend(self.resource_routes_v2(Some(id)).into_iter().map(|connection|json!({"kind":"running","connection":connection})));
                let revision=digest(&json!(rows.iter().map(|r|if r["kind"]=="saved"{json!([r["server"]["id"],r["server"]["revision"],r["direct"]])}else{json!([r["connection"]["connectionId"],r["connection"]["generation"]])}).collect::<Vec<_>>()));
                let (items,_,next)=page(rows.into_iter().map(Ok),caller,method,p,&revision)?;
                json!({"dependencies":items,"nextCursor":next})
            },
            "connections.route.v2.segments"=>{
                let registry=self.registry.lock().unwrap();
                let c=registry.connections.get(field(p,"connectionId")?).filter(|c|Some(c.generation)==p["generation"].as_u64()).ok_or_else(expired)?;
                let segments=c.bridge_segments.iter().enumerate().map(|(index,metrics)|Ok(json!({"hopIndex":index+1,"fromServerId":c.route.nodes[index].server.id,"toServerId":c.route.nodes[index+1].server.id,"metrics":metrics.lock().unwrap().clone()})));
                let (items,_,next)=page(segments,caller,method,p,&c.route.revision)?;
                json!({"segments":items,"resources":usage(c),"nextCursor":next})
            },
            "auth.challenge.v2.get"=>{
                let challenge=self.checked_challenge(field(p,"challengeId")?)?;
                if challenge["targetServerId"]!=p["targetServerId"] {return Err(CoreError::new("HOST_CHALLENGE_MISMATCH","认证对象不匹配"));}
                let plan=self.route_plan(field(&challenge,"targetServerId")?)?;
                json!({"server":self.store.server(field(&challenge,"serverId")?)?,"targetServerId":challenge["targetServerId"],"workspaceId":challenge["workspaceId"],"operationId":challenge["operationId"],"route":saved_meta(&plan),"role":challenge["role"],"hopIndex":challenge["hopIndex"],"hopCount":challenge["hopCount"]})
            },
            _=>return Err(CoreError::new("UNSUPPORTED_API","未知多级路径接口")),
        };
        checked(method,result)
    }

    fn node_page(&self,plan:&RoutePlan,summary:Value,caller:&Caller,method:&str,p:&Value)->Result<Value> {
        let (nodes,offset,next)=page((0..plan.nodes.len()).map(|i|Ok(plan.node_summary(i))),caller,method,p,&plan.revision)?;
        Ok(json!({"route":summary,"nodes":nodes,"offset":offset,"nextCursor":next}))
    }
    fn resolve_route_reference(&self,reference:&Value)->Result<Arc<RoutePlan>> {
        let plan=match field(reference,"kind")? {
            "saved"=>Arc::new(self.route_plan(field(reference,"serverId")?)?),
            "running"=>{
                let registry=self.registry.lock().unwrap();
                let c=registry.connections.get(field(reference,"connectionId")?).filter(|c|Some(c.generation)==reference["generation"].as_u64()).ok_or_else(expired)?;
                Arc::new(c.route.clone())
            },
            "attempt"=>self.route_attempts.lock().unwrap().get(&format!("{}:{}",field(reference,"serverId")?,field(reference,"workspaceId")?)).map(|a|a.plan.clone()).ok_or_else(expired)?,
            _=>return Err(expired()),
        };
        if reference["routeRevision"]!=plan.revision||reference["serverId"]!=plan.target_server_id {return Err(expired());}
        Ok(plan)
    }
    pub fn route_snapshot_v2(&self,server:&str,workspace:&str)->Result<Value> {
        let _guard=self.configuration.lock().unwrap();
        let saved=self.route_plan(server)?;
        let mut running=self.registry.lock().unwrap().connections.values().filter(|c|c.server.id==server&&c.workspace_id==workspace).map(|c|json!({"connectionId":c.id,"generation":c.generation,"route":running_meta(c),"state":*c.status.lock().unwrap(),"configurationChanged":c.route.revision!=saved.revision,"resources":usage(c),"failureHop":failure(c)})).collect::<Vec<_>>();
        running.sort_by(|a,b|a["connectionId"].as_str().cmp(&b["connectionId"].as_str()));
        let attempt=self.route_attempts.lock().unwrap().get(&format!("{server}:{workspace}")).map(|a|{
            let summary=meta(&a.plan,json!({"kind":"attempt","serverId":server,"workspaceId":workspace,"routeRevision":a.plan.revision}));
            json!({"route":summary,"hopIndex":a.hop,"current":a.plan.node_summary(a.hop),"stage":a.stage,"error":a.error,"updated":a.updated})
        });
        let count=running.len();
        let current=running.iter().filter(|r|r["configurationChanged"]==false&&r["state"]=="active").count();
        let changed=running.iter().filter(|r|r["configurationChanged"]==true).count();
        let mut value=json!({"saved":saved_meta(&saved),"running":running,"runningCount":count,"currentCount":current,"changedCount":changed,"attempt":attempt});
        // State feeds allow 32 KiB. Full diagnostics remain available in pages.
        while serde_json::to_vec(&value)?.len()>28*1024 && !value["running"].as_array().unwrap().is_empty() {value["running"].as_array_mut().unwrap().pop();}
        check("x-results","connections.route.v2.get",&value)?;
        Ok(value)
    }
    fn resource_routes_v2(&self,dependency:Option<&str>)->Vec<Value> {
        let registry=self.registry.lock().unwrap();
        let mut rows=registry.connections.values().filter(|c|dependency.is_none_or(|id|c.route.contains(id))).map(|c|{
            let holders=registry.leases.values().filter(|l|l.connection.id==c.id).map(|l|l.instance_id.clone()).collect::<std::collections::BTreeSet<_>>();
            json!({"connectionId":c.id,"workspaceId":c.workspace_id,"generation":c.generation,"state":*c.status.lock().unwrap(),"route":running_meta(c),"holders":holders,"resources":usage(c),"failureHop":failure(c)})
        }).collect::<Vec<_>>();
        rows.sort_by(|a,b|a["connectionId"].as_str().cmp(&b["connectionId"].as_str()));rows
    }
}

fn expired()->CoreError {CoreError::new("ROUTE_SNAPSHOT_EXPIRED","路径已变化或连接已结束，请重新读取")}

/// Cursors bind pages to caller, query and immutable revision. No server-side
/// copy of the full path is allocated for each page or consumer.
fn page<I:ExactSizeIterator<Item=Result<Value>>>(values:I,caller:&Caller,method:&str,p:&Value,revision:&str)->Result<(Vec<Value>,usize,Option<String>)> {
    let mut query=p.clone();query.as_object_mut().unwrap().remove("cursor");
    let identity=digest(&json!([caller.instance_id,method,query,revision]));
    let offset=if let Some(cursor)=p["cursor"].as_str(){
        let decoded=STANDARD.decode(cursor).map_err(|_|expired())?;
        let value:Value=serde_json::from_slice(&decoded).map_err(|_|expired())?;
        if value["identity"]!=identity {return Err(expired());}
        usize::try_from(value["offset"].as_u64().ok_or_else(expired)?).map_err(|_|expired())?
    }else{0};
    let total=values.len();if offset>total{return Err(expired());}
    let mut rows=Vec::new();let mut bytes=0;
    for value in values.skip(offset).take(64) {
        let value=value?;let size=serde_json::to_vec(&value)?.len();
        if bytes+size>240*1024 {break;}
        bytes+=size;rows.push(value);
    }
    if rows.is_empty()&&offset<total{return Err(CoreError::new("LIMIT_EXCEEDED","单条路径记录超过响应预算"));}
    let next=offset+rows.len();
    let cursor=(next<total).then(||STANDARD.encode(serde_json::to_vec(&json!({"identity":identity,"offset":next})).expect("cursor")));
    Ok((rows,offset,cursor))
}
fn checked(method:&str,value:Value)->Result<Value>{
    check("x-results",method,&value)?;
    if serde_json::to_vec(&value)?.len()>256*1024{return Err(CoreError::new("LIMIT_EXCEEDED","路径响应超过预算"));}
    Ok(value)
}
fn check(kind:&str,method:&str,value:&Value)->Result<()> {
    static SCHEMA:std::sync::OnceLock<Value>=std::sync::OnceLock::new();
    let schema=SCHEMA.get_or_init(||serde_json::from_str(include_str!("../../../contracts/connection-route-v2.schema.json")).unwrap());
    let name=schema[kind][method].as_str().ok_or_else(||CoreError::new("UNSUPPORTED_API","未知多级路径契约"))?;
    crate::contract::check(schema,&schema["$defs"][name],value,method).map_err(|e|CoreError::new(if kind=="x-methods"{"INVALID_PARAMS"}else{"PROTOCOL_ERROR"},e))
}

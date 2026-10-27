use super::*;
use crate::route::{ConnectionRoute, RouteNode, RoutePlan};

pub(crate) struct Attempt {
    pub plan: Arc<RoutePlan>, pub hop: usize, pub stage: String,
    pub error: Option<CoreError>, pub updated: u64,
}
impl Attempt {
    fn v1(&self) -> Result<Value> {
        self.plan.require_v1()?;
        Ok(json!({"route":self.plan.summary(),"hopIndex":self.hop,"stage":self.stage,"error":self.error,"updated":self.updated}))
    }
}

impl Core {
    pub fn route_attempt_timeout(&self, server_id: &str) -> Result<Duration> {
        let _guard=self.configuration.lock().unwrap();
        let nodes=self.store.route_servers(server_id)?.len();
        Ok(Duration::from_secs(120 + 75 * nodes.saturating_sub(2) as u64))
    }
    pub(crate) fn save_server_configuration(&self, p: &Value, update: bool, multihop: bool) -> Result<Value> {
                let _config_guard=self.configuration.lock().unwrap();
                let old = if update {
                    Some(self.store.server(field(p, "serverId")?)?)
                } else {
                    None
                };
                let expected = if old.is_some() {
                    Some(number(p, "expectedRevision")?)
                } else {
                    None
                };
                let mut server = Server {
                    id: old.as_ref().map_or_else(id, |s| s.id.clone()),
                    name: field(p, "name")?.trim().into(),
                    host: field(p, "host")?.trim().into(),
                    port: u16::try_from(p.get("port").and_then(Value::as_u64).unwrap_or(22))
                        .map_err(|_| CoreError::new("INVALID_PARAMS", "端口范围为 1–65535"))?,
                    username: field(p, "username")?.trim().into(),
                    auth_type: p
                        .get("authType")
                        .and_then(Value::as_str)
                        .unwrap_or("password")
                        .into(),
                    revision: old.as_ref().map_or(1, |s| s.revision + 1),
                    has_secret: old.as_ref().is_some_and(|s| self.secrets.remembered(&s.id)),
                };
                let previous_route=old.as_ref().map(|s|self.store.connection_route(&s.id)).transpose()?;
                let route=p.get("connectionRoute").map(|v|serde_json::from_value::<route::ConnectionRoute>(v.clone())).transpose()?;
                self.store.save_server_route(&server, expected, route.as_ref(), multihop)?;
                // Changed authentication must not silently reuse an old secret.
                if old.as_ref().is_some_and(|s| {
                    s.auth_type != server.auth_type
                        || s.username != server.username
                        || s.host != server.host
                        || s.port != server.port
                        || route.as_ref().is_some_and(|r|Some(r)!=previous_route.as_ref())
                }) {
                    self.secrets.remove(&server.id)?;
                    for dependent in self.store.dependents(&server.id)? {
                        self.secrets.remove(&dependent.id)?;
                        *self.credential_revisions.lock().unwrap().entry(dependent.id).or_default() += 1;
                    }
                    *self.credential_revisions.lock().unwrap().entry(server.id.clone()).or_default() += 1;
                    server.has_secret = false;
                }
                Ok(serde_json::to_value(server)?)
    }

    pub(crate) fn route_plan(&self, server_id:&str)->Result<RoutePlan> {
        let servers=self.store.route_servers(server_id)?;
        let revisions=self.credential_revisions.lock().unwrap();
        RoutePlan::new(servers.into_iter().map(|server|RouteNode{credential_revision:*revisions.get(&server.id).unwrap_or(&0),server}).collect())
    }
    pub(crate) fn route_dispatch(&self,_caller:&Caller,method:&str,p:&Value)->Result<Value> {
        check("x-methods",method,p)?;
        let result=match method {
            "servers.routes.list"=>{
                let _guard=self.configuration.lock().unwrap();
                let mut servers=Vec::new();
                for server in self.store.servers()? {self.route_plan(&server.id)?.require_v1()?;servers.push(self.route_server_value(&server.id)?);}
                Ok(json!({"servers":servers}))
            },
            "servers.routes.get"=>{let _guard=self.configuration.lock().unwrap();self.route_plan(field(p,"serverId")?)?.require_v1()?;self.route_server_value(field(p,"serverId")?)},
            "connections.routes.list"=>Ok(json!({"connections":self.route_resources(None)?})),
            "servers.routes.dependencies"=>{
                let _guard=self.configuration.lock().unwrap();
                let id=field(p,"serverId")?;
                let saved=self.store.dependents(id)?;
                for server in &saved {self.route_plan(&server.id)?.require_v1()?;}
                Ok(json!({"saved":saved,"running":self.route_resources(Some(id))?}))
            },
            "servers.routes.create"|"servers.routes.update"=>{
                let config:ConnectionRoute=serde_json::from_value(p.get("connectionRoute").cloned().ok_or_else(||CoreError::new("INVALID_PARAMS","缺少连接方式"))?)?;
                let mut params=p.clone();params["connectionRoute"]=serde_json::to_value(config)?;
                let saved=self.save_server_configuration(&params,method.ends_with("update"),false)?;
                let _guard=self.configuration.lock().unwrap();self.route_server_value(field(&saved,"id")?)
            },
            "connections.route.get"=>self.route_snapshot(field(p,"serverId")?,field(p,"workspaceId")?),
            "auth.challenge.get"=>{
                let _guard=self.configuration.lock().unwrap();
                let challenge=self.checked_challenge(field(p,"challengeId")?)?;
                let plan=self.route_plan(field(&challenge,"targetServerId")?)?;plan.require_v1()?;
                if p.get("targetServerId").is_some_and(|id|*id!=challenge["targetServerId"]) {return Err(CoreError::new("HOST_CHALLENGE_MISMATCH","认证对象不匹配"));}
                Ok(json!({"server":self.store.server(field(&challenge,"serverId")?)?,"targetServerId":challenge["targetServerId"],"workspaceId":challenge["workspaceId"],"operationId":challenge["operationId"],"route":plan.summary(),"role":challenge["role"],"hopIndex":challenge["hopIndex"],"hopCount":challenge["hopCount"]}))
            },
            _=>Err(CoreError::new("UNSUPPORTED_API","未知连接路径接口")),
        };
        let value=result?;check("x-results",method,&value)?;Ok(value)
    }
    /// Internal projection for the host's existing state hub. External queries
    /// still enter Core::call and its audit barrier.
    pub fn route_snapshot(&self,server:&str,workspace:&str)->Result<Value> {
        let _guard=self.configuration.lock().unwrap();
                let plan=self.route_plan(server)?;
                plan.require_v1()?;
                let running=self.registry.lock().unwrap().connections.values().filter(|c|c.server.id==server&&c.workspace_id==workspace).map(|c|{
                    c.route.require_v1()?;
                    Ok(json!({"connectionId":c.id,"generation":c.generation,"route":c.route.summary(),"state":*c.status.lock().unwrap(),"configurationChanged":c.route.revision!=plan.revision,"bridge":c.bridge_metrics.as_ref().map(|m|m.lock().unwrap().clone())}))
                }).collect::<Result<Vec<_>>>()?;
                let attempt=self.route_attempts.lock().unwrap().get(&format!("{server}:{workspace}")).map(Attempt::v1).transpose()?;
                Ok(json!({"saved":plan.summary(),"running":running,"attempt":attempt}))
    }
    fn route_resources(&self,dependency:Option<&str>)->Result<Vec<Value>> {
        let registry=self.registry.lock().unwrap();
        registry.connections.values().filter(|c|dependency.is_none_or(|id|c.route.contains(id))).map(|c|{
            let holders=registry.leases.values().filter(|l|l.connection.id==c.id).map(|l|l.instance_id.clone()).collect::<std::collections::BTreeSet<_>>();
            c.route.require_v1()?;
            Ok(json!({"connectionId":c.id,"workspaceId":c.workspace_id,"generation":c.generation,"state":*c.status.lock().unwrap(),"route":c.route.summary(),"holders":holders}))
        }).collect()
    }
    pub(crate) fn route_server_value(&self,id:&str)->Result<Value> {
        let server=self.store.server(id)?;
        let plan=self.route_plan(id)?;
        let mut value=serde_json::to_value(&server)?;
        value["connectionRoute"]=serde_json::to_value(self.store.connection_route(id)?)?;
        value["hasSecret"]=json!(self.secrets.remembered(id)&&self.credential_for(&plan,plan.nodes.len()-1)?.is_some());
        Ok(value)
    }
    pub(crate) fn credential_for(&self,plan:&RoutePlan,index:usize)->Result<Option<Credential>> {
        let context=plan.credential_context(index);
        Ok(self.secrets.get(&plan.nodes[index].server.id)?.filter(|c|c.context.as_deref()==Some(context.as_str())||(index==0&&c.context.is_none())))
    }
    pub(crate) fn verify_route_node(&self,plan:&RoutePlan,index:usize,fingerprint:&str,algorithm:&str,verified:&[String])->Result<Credential> {
        let _guard=self.configuration.lock().unwrap();
        if self.route_plan(&plan.target_server_id)?.revision!=plan.revision {return Err(CoreError::new("REVISION_CONFLICT","连接路径已变化，请重试"));}
        let server=&plan.nodes[index].server;
        let trust_key=plan.trust_key(index,verified);
        let known=self.store.host_key(&trust_key)?;
        if known.as_deref()!=Some(fingerprint) {
            return Err(CoreError::new(if known.is_some(){"HOST_KEY_CHANGED"}else{"HOST_KEY_REQUIRED"},if known.is_some(){"服务器主机密钥已改变，认证已停止"}else{"请确认服务器主机身份"}).details(json!({"host":server.host,"port":server.port,"algorithm":algorithm,"fingerprint":fingerprint,"previousFingerprint":known,"trustKey":trust_key})));
        }
        self.credential_for(plan,index)?.ok_or_else(||CoreError::new("AUTH_REQUIRED","请输入 SSH 认证信息").details(json!({"authType":server.auth_type})))
    }
    pub(crate) fn route_progress(&self,plan:&RoutePlan,workspace:&str,index:usize,stage:&str,error:Option<&CoreError>) {
        let mut attempts=self.route_attempts.lock().unwrap();
        let key=format!("{}:{workspace}",plan.target_server_id);
        if attempts.len()>=512&&!attempts.contains_key(&key){
            if let Some(old)=attempts.iter().min_by_key(|(_,v)|v.updated).map(|(k,_)|k.clone()){attempts.remove(&old);}
        }
        let shared=attempts.get(&key).filter(|a|a.plan.revision==plan.revision).map(|a|a.plan.clone()).unwrap_or_else(||Arc::new(plan.clone()));
        attempts.insert(key,Attempt{plan:shared,hop:index,stage:stage.into(),error:error.cloned(),updated:model::now_ms()});
    }
    pub(crate) fn route_challenge(&self,caller:&Caller,p:&Value,plan:&RoutePlan,error:&mut CoreError)->Result<()> {
        let mut details=error.details.take().unwrap_or(json!({}));
        let index=details["hopIndex"].as_u64().unwrap_or(0) as usize;
        let node=plan.nodes.get(index).ok_or_else(||CoreError::new("INTERNAL","认证节点无效"))?;
        let challenge=id();details["challengeId"]=json!(challenge);
        details["serverId"]=json!(node.server.id);details["serverRevision"]=json!(node.server.revision);
        details["routeRevision"]=json!(plan.revision);details["targetServerId"]=json!(plan.target_server_id);
        details["workspaceId"]=p["workspaceId"].clone();details["operationId"]=p["operationId"].clone();
        if p["networkOperation"] == true { details["networkOperation"] = json!(true); }
        details["instanceId"]=json!(caller.instance_id);details["created"]=json!(model::now_ms());
        details["route"]=crate::route_v2::saved_meta(plan);details["authType"]=json!(node.server.auth_type);
        details["credentialContext"]=json!(plan.credential_context(index));
        let mut challenges=self.host_challenges.lock().unwrap();
        challenges.retain(|_,v|model::now_ms().saturating_sub(v["created"].as_u64().unwrap_or(0))<300_000);
        if challenges.len()>=512{return Err(CoreError::new("LIMIT_EXCEEDED","待确认认证过多"));}
        challenges.insert(challenge,details.clone());error.details=Some(details);Ok(())
    }
    pub(crate) fn checked_challenge(&self,id:&str)->Result<Value> {
        let challenge=self.host_challenges.lock().unwrap().get(id).cloned().ok_or_else(||CoreError::new("HOST_CHALLENGE_EXPIRED","请重新连接以获取当前身份"))?;
        let expired=||CoreError::new("HOST_CHALLENGE_EXPIRED","连接配置或认证操作已变化，请重新连接");
        if model::now_ms().saturating_sub(challenge["created"].as_u64().unwrap_or(0))>=300_000{return Err(expired())}
        let plan=self.route_plan(field(&challenge,"targetServerId")?).map_err(|_|expired())?;
        if challenge["routeRevision"]!=plan.revision{return Err(expired())}
        if let (Some(instance),Some(operation))=(challenge["instanceId"].as_str(),challenge["operationId"].as_str()) {
            let live = if challenge["networkOperation"] == true { self.network_challenge_live(instance, operation) }
                else { self.operations.lock().unwrap().get(&(instance.into(),operation.into())).is_some_and(|o|!o.canceled.load(Ordering::SeqCst)) };
            if !live || self.registry.lock().unwrap().revoked.contains(instance) {return Err(expired())}
        }
        Ok(challenge)
    }
}

fn check(kind:&str,method:&str,value:&Value)->Result<()> {
    static SCHEMA:std::sync::OnceLock<Value>=std::sync::OnceLock::new();
    let schema=SCHEMA.get_or_init(||serde_json::from_str(include_str!("../../../contracts/connection-route-v1.schema.json")).unwrap());
    let name=schema[kind][method].as_str().ok_or_else(||CoreError::new("UNSUPPORTED_API","未知路径契约"))?;
    crate::contract::check(schema,&schema["$defs"][name],value,method).map_err(|e|CoreError::new(if kind=="x-methods"{"INVALID_PARAMS"}else{"PROTOCOL_ERROR"},e))
}

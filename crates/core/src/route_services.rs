use super::*;
use crate::route::{ResolvedStep, RoutePlan};

pub(crate) struct Attempt {
    pub plan: Arc<RoutePlan>, pub hop: usize, pub stage: String,
    pub error: Option<CoreError>, pub updated: u64,
    pub generation:u64,pub workspace:String,pub workspace_generation:Option<String>,pub instance:String,pub operation:Option<String>,
}

impl Core {
    pub fn route_attempt_timeout(&self, server_id: &str) -> Result<Duration> {
        let _guard=self.configuration.lock().unwrap();
        let (_, steps)=self.store.resolved_route(server_id)?;
        Ok(Duration::from_millis(crate::route::budget::Cost::for_steps(&steps).attempt_timeout_ms()))
    }
    pub(crate) fn save_server_configuration(&self, p: &Value, update: bool) -> Result<Value> {
                let _config_guard=self.configuration.lock().unwrap();
                if p.get("connectionRoute").is_some(){return Err(CoreError::new("INVALID_PARAMS","请使用 accessPath 配置访问方式"));}
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
                    has_secret: false,
                };
                let route=p.get("accessPath").map(|v|serde_json::from_value::<route::AccessPath>(v.clone())).transpose()?;
                self.store.save_server_route(&server, expected, route.as_ref())?;
                // Changed authentication must not silently reuse an old secret.
                if old.as_ref().is_some_and(|s| {
                    s.auth_type != server.auth_type
                        || s.username != server.username
                        || s.host != server.host
                        || s.port != server.port
                }) {
                    *self.credential_revisions.lock().unwrap().entry(server.id.clone()).or_default() += 1;
                    server.has_secret = false;
                }
                Ok(serde_json::to_value(server)?)
    }

    pub(crate) fn route_plan(&self, server_id:&str)->Result<RoutePlan> {
        let (path, resolved)=self.store.resolved_route(server_id)?;
        let revisions=self.credential_revisions.lock().unwrap();
        let steps=resolved.into_iter().map(|step|match step {
            crate::route::ResolvedStep::Ssh(mut s)=>{s.credential_revision=*revisions.get(&s.server.id).unwrap_or(&0);crate::route::ResolvedStep::Ssh(s)},
            crate::route::ResolvedStep::Socks5(mut s)=>{s.credential_revision=*revisions.get(&format!("proxy:{}",s.proxy.id)).unwrap_or(&0);crate::route::ResolvedStep::Socks5(s)},
        }).collect();
        let mut plan=RoutePlan::from_steps(steps,path)?;
        let mut verified=Vec::new();let mut trust=Vec::new();
        for index in 0..plan.steps.len() {
            if matches!(plan.steps[index],ResolvedStep::Ssh(_)) {
                let known=self.store.host_key(&plan.trust_key(index,&verified))?;
                trust.push(known.clone());
                if let Some(key)=known {verified.push(key)} else {break;}
            }
        }
        plan.revision=crate::route::digest(&json!([plan.revision,trust]));
        Ok(plan)
    }
    pub(crate) fn route_server_value(&self,id:&str)->Result<Value> {
        let server=self.store.server(id)?;
        let plan=self.route_plan(id)?;
        let mut value=serde_json::to_value(&server)?;
        value["accessPath"]=serde_json::to_value(self.store.connection_route(id)?)?;
        value["hasSecret"]=json!(self.credential_for(&plan,plan.steps.len()-1)?.is_some()||self.secrets.has_preset(&self.store.db.lock().unwrap(),"ssh",id,&crate::route::identity::identity(plan.steps.last().unwrap()))?);
        Ok(value)
    }
    pub(crate) fn credential_for(&self,plan:&RoutePlan,index:usize)->Result<Option<Credential>> {
        let Some(context)=self.trusted_context(plan,index)?else{return Ok(None)};
        let step=&plan.steps[index];
        self.secrets.get(&self.store.db.lock().unwrap(),step.owner_kind(),step.owner_id(),&context)
    }

    pub(crate) fn verify_route_node(&self,plan:&RoutePlan,index:usize,fingerprint:&str,algorithm:&str,verified:&[String])->Result<Credential> {
        let _guard=self.configuration.lock().unwrap();
        if self.route_plan(&plan.target_server_id)?.revision!=plan.revision {return Err(CoreError::new("REVISION_CONFLICT","连接路径已变化，请重试"));}
        let server=plan.steps[index].ssh()?;
        let trust_key=plan.trust_key(index,verified);
        let known=self.store.host_key(&trust_key)?;
        if known.as_deref()!=Some(fingerprint) {
            let mut prefix=verified.to_vec();prefix.push(fingerprint.into());
            let identity=crate::route::digest(&crate::route::identity::identity(&plan.steps[index]));
            let db=self.store.db.lock().unwrap();
            let available=self.secrets.get(&db,"ssh",&server.id,"preset")?.is_some_and(|c|c.context.as_deref()==Some(identity.as_str()));
            let remembered=available&&self.secrets.remembered(&db,"ssh",&server.id,"preset")?;
            return Err(CoreError::new(if known.is_some(){"HOST_KEY_CHANGED"}else{"HOST_KEY_REQUIRED"},if known.is_some(){"服务器主机密钥已改变，认证已停止"}else{"请确认服务器主机身份"}).details(json!({"host":server.host,"port":server.port,"algorithm":algorithm,"fingerprint":fingerprint,"previousFingerprint":known,"trustKey":trust_key,"_contextKey":plan.verified_context(index,&prefix),"_presetIdentity":identity,"hasPreset":available,"rememberDefault":remembered})));
        }
        let mut verified=verified.to_vec();verified.push(fingerprint.into());
        self.context_credential(plan,index,&verified)
    }
    pub(crate) fn begin_route_attempt(&self,caller:&Caller,p:&Value,plan:&RoutePlan,workspace:&str,token:&str) {
        let generation=self.next_generation.fetch_add(1,Ordering::SeqCst);
        let workspace_generation=self.remote.lock().unwrap().scopes.get(workspace).cloned();
        let operation=p["operationId"].as_str().map(str::to_owned);
        let mut attempts=self.route_attempts.lock().unwrap();
        attempts.retain(|_,a|!(a.instance==caller.instance_id&&a.workspace==workspace&&a.operation==operation));
        if attempts.len()>=512 {if let Some(old)=attempts.iter().min_by_key(|(_,a)|a.generation).map(|(k,_)|k.clone()){attempts.remove(&old);}}
        attempts.insert(token.into(),Attempt{plan:Arc::new(plan.clone()),hop:0,stage:"connect".into(),error:None,updated:model::now_ms(),generation,workspace:workspace.into(),workspace_generation,instance:caller.instance_id.clone(),operation});
    }
    pub(crate) fn route_progress(&self,attempt:&str,index:usize,stage:&str,error:Option<&CoreError>) {
        if let Some(a)=self.route_attempts.lock().unwrap().get_mut(attempt) {
            a.hop=index;a.stage=stage.into();a.error=error.cloned();a.updated=model::now_ms();
        }
    }
    pub(crate) fn route_challenge(&self,caller:&Caller,p:&Value,plan:&RoutePlan,attempt:&str,error:&mut CoreError)->Result<()> {
        let attempts=self.route_attempts.lock().unwrap();
        let active=attempts.get(attempt).ok_or_else(||CoreError::new("HOST_CHALLENGE_EXPIRED","连接尝试已被替换"))?;
        let generation=active.generation;let workspace_generation=active.workspace_generation.clone();drop(attempts);
        let mut details=error.details.take().unwrap_or(json!({}));
        let index=details["stepIndex"].as_u64().unwrap_or(0) as usize;
        let step=plan.steps.get(index).ok_or_else(||CoreError::new("INTERNAL","认证节点无效"))?;
        let challenge=id();details["challengeId"]=json!(challenge);
        details["routeRevision"]=json!(plan.revision);details["targetServerId"]=json!(plan.target_server_id);
        details["workspaceId"]=p["workspaceId"].clone();details["operationId"]=p["operationId"].clone();
        if p["networkOperation"] == true { details["networkOperation"] = json!(true); }
        details["instanceId"]=json!(caller.instance_id);details["attemptId"]=json!(attempt);details["attemptGeneration"]=json!(generation);details["workspaceGeneration"]=json!(workspace_generation);details["created"]=json!(model::now_ms());
        details["route"]=crate::route_diagnostics::saved_meta(plan);
        let trust_key=details.as_object_mut().unwrap().remove("trustKey");
        let context=details.as_object_mut().unwrap().remove("_contextKey");
        let preset_identity=details.as_object_mut().unwrap().remove("_presetIdentity");
        let context=context.or(self.trusted_context(plan,index)?.map(|s|json!(s)));
        if details.get("rememberDefault").is_none(){details["rememberDefault"]=json!(match context.as_ref().and_then(Value::as_str){Some(context)=>self.secrets.remembered(&self.store.db.lock().unwrap(),step.owner_kind(),step.owner_id(),context)?,None=>false});}
        details["stepId"]=json!(step.id());details["stepIndex"]=json!(index);details["stepCount"]=json!(plan.steps.len());
        if let ResolvedStep::Socks5(s)=step {
            let proxy=&s.proxy;
            details["ownerKind"]=json!("socks5");
            details["ownerId"]=json!(proxy.id);details["proxy"]=serde_json::to_value(proxy)?;
        }else{
            let server=step.ssh()?;
            details["ownerKind"]=json!(if index+1==plan.steps.len(){"targetSsh"}else{"sshHost"});details["ownerId"]=json!(server.id);
            details["role"]=json!(if index+1==plan.steps.len(){"target"}else{"jump"});
            details["serverId"]=json!(server.id);details["serverRevision"]=json!(server.revision);details["authType"]=json!(server.auth_type);
        }
        let mut challenges=self.host_challenges.lock().unwrap();
        challenges.retain(|_,v|model::now_ms().saturating_sub(v["created"].as_u64().unwrap_or(0))<300_000);
        if challenges.len()>=512{return Err(CoreError::new("LIMIT_EXCEEDED","待确认认证过多"));}
        let mut stored=details.clone();
        stored["trustKey"]=trust_key.unwrap_or(Value::Null);
        stored["_contextKey"]=context.unwrap_or(Value::Null);
        stored["_presetIdentity"]=preset_identity.unwrap_or(Value::Null);
        challenges.insert(challenge,stored);error.details=Some(details);Ok(())
    }
    pub(crate) fn checked_challenge(&self,id:&str)->Result<Value> {
        let challenge=self.host_challenges.lock().unwrap().get(id).cloned().ok_or_else(||CoreError::new("HOST_CHALLENGE_EXPIRED","请重新连接以获取当前身份"))?;
        let expired=||CoreError::new("HOST_CHALLENGE_EXPIRED","连接配置或认证操作已变化，请重新连接");
        if model::now_ms().saturating_sub(challenge["created"].as_u64().unwrap_or(0))>=300_000{return Err(expired())}
        let attempts=self.route_attempts.lock().unwrap();
        let attempt=attempts.get(field(&challenge,"attemptId")?).ok_or_else(expired)?;
        if challenge["attemptGeneration"].as_u64()!=Some(attempt.generation)||challenge["instanceId"]!=attempt.instance{return Err(expired())}
        let workspace=attempt.workspace.clone();let workspace_generation=attempt.workspace_generation.clone();drop(attempts);
        if self.remote.lock().unwrap().scopes.get(&workspace).cloned()!=workspace_generation{return Err(expired())}
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

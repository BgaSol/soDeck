//! Sequential transport composition. Only the final SSH session is handed off.
use super::*;
use crate::route::{plan::ResolvedStep, ProxyResolve};
use crate::connection_diagnostics::PhaseEvent;

pub(super) trait Hooks {
    fn verify(&mut self, index: usize, fingerprint: &str, algorithm: &str, verified: &[String]) -> Result<Credential>;
    fn proxy_auth(&mut self, index: usize, verified: &[String]) -> Result<Credential>;
    fn progress(&mut self, index: usize, stage: &str);
    fn completed(&mut self, _index:usize, _stage:&str) {}
    fn outcome(&mut self, _index:usize, _event:PhaseEvent<'_>) {}
    fn session(&mut self, _index: usize, _session: &Session) -> Result<()> { Ok(()) }
    fn forward(&mut self, _from: usize, _to: usize, _session: &Session) -> Result<()> { Ok(()) }
    fn proxy_bytes(&mut self, _index: usize, _write: bool, _count: u64) {}
}

pub(super) struct ConnectionHooks<'a, V, A, P> {
    pub verify: &'a mut V,
    pub proxy_auth: &'a mut A,
    pub progress: &'a mut P,
    pub traffic: Option<&'a Arc<crate::traffic::Traffic>>,
    pub route: &'a crate::route::RoutePlan,
    pub workspace_id: &'a str,
    pub connection_id: &'a str,
    pub generation: u64,
    pub registrations: Vec<crate::traffic::Registration>,
    pub proxy_registrations: std::collections::HashMap<usize,crate::traffic::Registration>,
}
impl<V,A,P> ConnectionHooks<'_,V,A,P> {
    fn register(&self, index: usize, kind: &str, layer: &str) -> Option<crate::traffic::Registration> {
        self.traffic.map(|traffic|traffic.register(crate::traffic::Owner {
            server_id:self.route.target_server_id.clone(),config_epoch:traffic.config_epoch.clone(),
            workspace_id:Some(self.workspace_id.into()),workspace_generation:None,plugin_id:None,
            instance_id:"connection-broker".into(),resource_id:format!("{}:{kind}:{index}",self.connection_id),
            generation:self.generation.to_string(),kind:kind.into(),route_run_id:self.connection_id.into(),
            layer:layer.into(),access_mode:self.route.access_path.mode().into(),
        }))
    }
}
impl<V,A,P> Hooks for ConnectionHooks<'_,V,A,P>
where V: FnMut(usize,&str,&str,&[String])->Result<Credential>, A: FnMut(usize,&[String])->Result<Credential>, P: FnMut(usize,PhaseEvent<'_>) {
    fn verify(&mut self,index:usize,fingerprint:&str,algorithm:&str,verified:&[String])->Result<Credential> { (self.verify)(index,fingerprint,algorithm,verified) }
    fn proxy_auth(&mut self,index:usize,verified:&[String])->Result<Credential> { (self.proxy_auth)(index,verified) }
    fn progress(&mut self,index:usize,stage:&str) { (self.progress)(index,PhaseEvent::start(stage)); }
    fn completed(&mut self,index:usize,stage:&str) { (self.progress)(index,PhaseEvent::success(stage)); }
    fn outcome(&mut self,index:usize,event:PhaseEvent<'_>) { (self.progress)(index,event); }
    fn session(&mut self,index:usize,session:&Session)->Result<()> {
        let outer=!self.route.steps[..index].iter().any(|s|matches!(s,ResolvedStep::Ssh(_)));
        if let Some(metric)=self.register(index,"transport",if outer{"transport"}else{"carrier"}) {
            if let (Some(traffic),Some(counter))=(self.traffic,metric.counter.as_ref()) {
                session.set_byte_meter(&traffic.meter,counter).map_err(|_|CoreError::new("SSH_ERROR","无法绑定连接统计"))?;
            }
            self.registrations.push(metric);
        }
        Ok(())
    }
    fn forward(&mut self,from:usize,_to:usize,session:&Session)->Result<()> {
        let metric=self.register(from,"carrier","carrier");
        crate::traffic::origin(session,&metric).map_err(|_|CoreError::new("SSH_ERROR","无法绑定连接统计"))?;
        self.registrations.extend(metric);Ok(())
    }
    fn proxy_bytes(&mut self,index:usize,write:bool,count:u64) {
        // Only initial proxies contribute outer TCP protocol bytes. Nested
        // proxies are already counted inside the first SSH transport.
        if self.route.steps[..index].iter().any(|s|matches!(s,ResolvedStep::Ssh(_))) {return;}
        if !self.proxy_registrations.contains_key(&index) {if let Some(metric)=self.register(index,"proxyProtocol","transport"){self.proxy_registrations.insert(index,metric);}}
        if let Some(counter)=self.proxy_registrations.get(&index).and_then(|m|m.counter.as_ref()) {if write{counter.record_sent(count)}else{counter.record_received(count)}}
    }
}

enum State {
    ConnectedStream(TcpStream),
    AuthenticatedSsh { session: Session, socket: TcpStream },
}

pub(super) struct Established {
    pub session: Session,
    pub socket: TcpStream,
    pub bridges: transport::BridgeChain,
}

pub(super) struct Execution<'a> {
    pub steps: &'a [ResolvedStep],
    pub canceled: &'a AtomicBool,
    pub operation: &'a AtomicBool,
    pub deadline: Instant,
    pub failed: Arc<AtomicBool>,
    pub failure_step: Arc<AtomicUsize>,
}

impl Execution<'_> {
    pub fn connect(&self, hooks: &mut impl Hooks) -> Result<Established> {
        let mut candidates=std::collections::BTreeMap::<usize,Vec<std::net::SocketAddr>>::new();
        let mut selected=std::collections::BTreeMap::<usize,usize>::new();
        for attempt in 0..8 {
            hooks.outcome(0,PhaseEvent{stage:"candidate",state:"round",error:None});
            let result=self.connect_once(hooks,&mut candidates,&selected);
            match result {
                Ok(value)=>return Ok(value),
                Err(mut error)=>{
                    let details=error.details.as_ref();
                    let index=details.and_then(|d|d["stepIndex"].as_u64()).unwrap_or(0) as usize;
                    let stage=details.and_then(|d|d["stage"].as_str()).unwrap_or("");
                    hooks.outcome(index,PhaseEvent{stage,state:if crate::is_connection_authentication_error(&error.code){"waiting"}else if error.code=="CANCELED"{"canceled"}else{"failed"},error:Some(&error)});
                    let retry_edge=if index==0&&stage=="connect"&&error.code=="NETWORK_ERROR" {Some(0)}
                        else if stage=="proxyConnect"&&(matches!(error.code.as_str(),"PROXY_TIMEOUT"|"PROXY_DISCONNECTED"|"PROXY_NETWORK_ERROR")||error.code=="PROXY_CONNECT_FAILED"&&details.is_some_and(|d|matches!(d["reply"].as_u64(),Some(1|3|4|5|6)))) {Some(index+1)}else{None};
                    // A failed CONNECT consumes its stream. connect_once has
                    // already dropped all bridges; every candidate rebuilds
                    // the prefix under this same reservation and deadline.
                    let advanced=retry_edge.is_some_and(|edge|{
                        let next=selected.get(&edge).copied().unwrap_or(0)+1;
                        if candidates.get(&edge).is_some_and(|v|next<v.len()) {
                            selected.insert(edge,next);selected.retain(|i,_|*i<=edge);true
                        } else {false}
                    });
                    if !advanced||attempt==7 {
                        error.details.get_or_insert_with(||json!({}))["candidateAttempts"]=json!(attempt+1);
                        return Err(error);
                    }
                    transport::check(self.canceled,self.operation,self.deadline)?;
                    self.failed.store(false,Ordering::SeqCst);
                    self.failure_step.store(usize::MAX,Ordering::SeqCst);
                }
            }
        }
        unreachable!("bounded candidate loop always returns")
    }
    fn local_address(&self,index:usize,candidates:&mut std::collections::BTreeMap<usize,Vec<std::net::SocketAddr>>,selected:&std::collections::BTreeMap<usize,usize>)->Result<String> {
        if !candidates.contains_key(&index) {
            let (host,port)=self.steps[index].endpoint();
            let addresses=transport::resolve(host,port,self.canceled,self.operation,self.deadline)?;
            candidates.insert(index,addresses);
        }
        candidates[&index].get(selected.get(&index).copied().unwrap_or(0)).map(|s|s.ip().to_string()).ok_or_else(||CoreError::new("NETWORK_ERROR","下一站没有可用地址"))
    }
    fn connect_once(&self, hooks:&mut impl Hooks,candidates:&mut std::collections::BTreeMap<usize,Vec<std::net::SocketAddr>>,selected:&std::collections::BTreeMap<usize,usize>) -> Result<Established> {
        if !matches!(self.steps.last(), Some(ResolvedStep::Ssh(_))) {
            return Err(CoreError::new("INVALID_ROUTE", "连接路径必须以目标 SSH 结束"));
        }
        let mut state = None;
        let mut bridges = transport::BridgeChain::default();
        let mut verified = Vec::new();
        for (index, step) in self.steps.iter().enumerate() {
            let mut stage = if index == 0 { "connect" } else { "forward" };
            let mut outgoing_resolution=false;
            let result = (|| {
                transport::check(self.canceled, self.operation, self.deadline)?;
                let (host, port) = step.endpoint();
                let socket = match state.take() {
                    None => {
                        stage=if candidates.contains_key(&0){"resolveCached"}else{"resolve"};
                        hooks.progress(index, stage);
                        let host=self.local_address(0,candidates,selected)?;
                        hooks.completed(index,stage);
                        stage="connect";hooks.progress(index,stage);
                        let socket=transport::tcp(&host, port, self.canceled, self.operation, self.deadline)?;
                        hooks.completed(index,stage);socket
                    }
                    Some(State::ConnectedStream(socket)) => socket,
                    Some(State::AuthenticatedSsh { session, socket }) => {
                        hooks.progress(index, stage);
                        hooks.forward(index - 1, index, &session)?;
                        let (socket, bridge) = transport::forward(session, socket, host, port,
                            self.canceled, self.operation, self.deadline, self.failed.clone(),
                            self.failure_step.clone(), index)?;
                        // The bridge belongs to the actual SSH -> next-step edge.
                        bridge.metrics.lock().unwrap()["edgeId"] = json!(format!("{}:{}", self.steps[index - 1].id(), step.id()));
                        bridges.0.push(bridge);
                        hooks.completed(index,stage);
                        socket
                    }
                };
                match step {
                    ResolvedStep::Ssh(step) => {
                        let server = &step.server;
                        let mut session = Session::new().map_err(|_| CoreError::new("SSH_ERROR", "无法创建 SSH 会话"))?;
                        hooks.session(index, &session)?;
                        session.set_tcp_stream(socket.try_clone()?);
                        session.set_blocking(false);
                        stage = "handshake"; hooks.progress(index, stage);
                        transport::protocol(self.canceled, self.operation, self.deadline, || session.handshake())?;
                        let (key, _) = session.host_key().ok_or_else(|| CoreError::new("SSH_ERROR", "服务器未提供主机密钥"))?;
                        let fingerprint = format!("SHA256:{}", STANDARD.encode(Sha256::digest(key)).trim_end_matches('='));
                        let length = key.get(..4).map(|v| u32::from_be_bytes(v.try_into().unwrap()) as usize).unwrap_or(0);
                        let algorithm = key.get(4..4 + length).and_then(|v| std::str::from_utf8(v).ok()).unwrap_or("unknown");
                        hooks.completed(index,stage);
                        stage = "verify"; hooks.progress(index, stage);
                        let credential = match hooks.verify(index, &fingerprint, algorithm, &verified) {
                            Ok(c)=>{hooks.completed(index,stage);stage="credentials";hooks.progress(index,stage);hooks.completed(index,stage);c},
                            Err(e) if matches!(e.code.as_str(),"AUTH_REQUIRED"|"CREDENTIAL_CONFIRM_REQUIRED")=>{hooks.completed(index,stage);stage="credentials";hooks.progress(index,stage);return Err(e);},
                            Err(e)=>return Err(e),
                        };
                        transport::check(self.canceled, self.operation, self.deadline)?;
                        stage = "authenticate"; hooks.progress(index, stage);
                        let result = if server.auth_type == "password" {
                            transport::protocol(self.canceled, self.operation, self.deadline,
                                || session.userauth_password(&server.username, &credential.password))
                        } else {
                            transport::protocol(self.canceled, self.operation, self.deadline,
                                || session.userauth_pubkey_memory(&server.username, None, &credential.private_key,
                                    (!credential.passphrase.is_empty()).then_some(credential.passphrase.as_str())))
                        };
                        result.map_err(|e| if matches!(e.code.as_str(), "CANCELED" | "CONNECT_TIMEOUT") { e }
                            else { CoreError::new("AUTH_FAILED", "SSH 认证失败，请检查用户名与凭据").details(e.details.unwrap_or(json!({}))) })?;
                        if !session.authenticated() { return Err(CoreError::new("AUTH_FAILED", "SSH 认证未完成")); }
                        hooks.completed(index,stage);
                        session.set_keepalive(true, 30);
                        verified.push(fingerprint);
                        Ok(State::AuthenticatedSsh { session, socket })
                    }
                    ResolvedStep::Socks5(step) => {
                        stage = "proxyCredentials"; hooks.progress(index, stage);
                        let credential = hooks.proxy_auth(index, &verified)?;
                        hooks.completed(index,stage);
                        let (host, port) = self.steps[index + 1].endpoint();
                        let host = if step.resolve == ProxyResolve::Local {
                            outgoing_resolution=true;
                            stage = if candidates.contains_key(&(index+1)){"resolveCached"}else{"resolve"}; hooks.progress(index, stage);
                            let address=self.local_address(index+1,candidates,selected)?;hooks.completed(index,stage);address
                        } else { host.to_owned() };
                        let auth = if step.proxy.auth_type == "none" { crate::socks5::Auth::None }
                            else { crate::socks5::Auth::Password { username: &step.proxy.username, password: &credential.password } };
                        // Two callbacks share hooks only during synchronous protocol work.
                        let hooks = std::cell::RefCell::new(&mut *hooks);
                        let socket = crate::socks5::connect_metered(socket, &host, port, auth,
                            self.canceled, self.operation, self.deadline,
                            |value,complete| { stage = value; if complete {hooks.borrow_mut().completed(index,stage);if value=="proxyNegotiate"&&step.proxy.auth_type=="none" {hooks.borrow_mut().outcome(index,PhaseEvent{stage:"proxyAuthenticate",state:"notApplicable",error:None});}}else{hooks.borrow_mut().progress(index, stage)} },
                            |write, count| hooks.borrow_mut().proxy_bytes(index, write, count))?;
                        Ok(State::ConnectedStream(socket))
                    }
                }
            })();
            state = Some(result.map_err(|mut error: CoreError| {
                let mut details = error.details.take().unwrap_or(json!({}));
                details["stepId"] = json!(step.id());
                details["stepIndex"] = json!(index);
                details["stepCount"] = json!(self.steps.len());
                details["stage"] = json!(stage);
                let outgoing=stage=="proxyConnect"||(matches!(stage,"resolve"|"resolveCached")&&outgoing_resolution);
                details["fromStepId"] = json!(if outgoing {step.id()}else if index == 0 { "local" } else { self.steps[index - 1].id() });
                details["toStepId"] = json!(if outgoing {self.steps[index+1].id()}else{step.id()});
                error.details = Some(details);
                error
            })?);
        }
        match state.expect("validated nonempty path") {
            State::AuthenticatedSsh { session, socket } => Ok(Established { session, socket, bridges }),
            State::ConnectedStream(_) => unreachable!("validated SSH target"),
        }
    }
}

#[cfg(test)]
mod tests;

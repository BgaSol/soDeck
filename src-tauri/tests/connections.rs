#![cfg(feature = "test-fixtures")]
use russh::{
    server::{self, Session, VerifiedPublicKey},
    Channel, ChannelId,
};
use serde_json::{json, Value};
use sodeck::{
    connections::{Answer, Question, View},
    AppCore,
};
use std::{
    collections::{HashMap, HashSet},
    net::SocketAddr,
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Default)]
struct Ssh {
    terminal_channels: HashSet<ChannelId>,
    shells: Arc<AtomicUsize>,
    sizes: Arc<Mutex<Vec<(u32, u32)>>>,
    auth: Arc<AtomicUsize>,
    live: Arc<AtomicUsize>,
    hosts: Arc<Mutex<Vec<String>>>,
    routes: Arc<Mutex<HashMap<String, SocketAddr>>>,
}
impl server::Handler for Ssh {
    type Error = russh::Error;
    async fn pty_request(
        &mut self,
        id: ChannelId,
        term: &str,
        cols: u32,
        rows: u32,
        _: u32,
        _: u32,
        _: &[(russh::Pty, u32)],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        assert_eq!(term, "xterm-256color");
        self.terminal_channels.insert(id);
        self.sizes.lock().unwrap().push((cols, rows));
        session.channel_success(id)?;
        Ok(())
    }
    async fn shell_request(
        &mut self,
        id: ChannelId,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        let shell = self.shells.fetch_add(1, Ordering::Relaxed) + 1;
        session.channel_success(id)?;
        session.data(id, format!("shell-{shell}> ").into_bytes())?;
        Ok(())
    }
    async fn window_change_request(
        &mut self,
        _: ChannelId,
        cols: u32,
        rows: u32,
        _: u32,
        _: u32,
        _: &mut Session,
    ) -> Result<(), Self::Error> {
        self.sizes.lock().unwrap().push((cols, rows));
        Ok(())
    }
    async fn data(
        &mut self,
        id: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        if !self.terminal_channels.contains(&id) {
            return Ok(());
        }
        if data == b"exit\r" {
            session.exit_status_request(id, 7)?;
            session.eof(id)?;
            session.close(id)?;
        } else {
            session.data(id, data.to_vec())?;
        }
        Ok(())
    }
    async fn auth_password(
        &mut self,
        user: &str,
        password: &str,
    ) -> Result<server::Auth, Self::Error> {
        self.auth.fetch_add(1, Ordering::Relaxed);
        Ok(if user == "ops" && password == "secret" {
            server::Auth::Accept
        } else {
            server::Auth::reject()
        })
    }
    async fn auth_publickey(
        &mut self,
        user: &str,
        _: VerifiedPublicKey<'_>,
    ) -> Result<server::Auth, Self::Error> {
        self.auth.fetch_add(1, Ordering::Relaxed);
        Ok(if user == "ops" {
            server::Auth::Accept
        } else {
            server::Auth::reject()
        })
    }
    async fn channel_open_session(
        &mut self,
        _: Channel<server::Msg>,
        reply: server::ChannelOpenHandle,
        _: &mut Session,
    ) -> Result<(), Self::Error> {
        reply.accept().await;
        Ok(())
    }
    async fn exec_request(
        &mut self,
        id: ChannelId,
        command: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        if command == b"terminal-test:reject" {
            session.channel_failure(id)?;
            return Ok(());
        }
        if command == b"terminal-test:unconfirmed" {
            return Ok(());
        }
        session.channel_success(id)?;
        if command == b"status-test:hang" {
            return Ok(());
        }
        if command == b"status-test:mixed" {
            session.data(id, vec![b'x'; 9000])?;
            session.extended_data(id, 1, b"err\n".to_vec())?;
            session.exit_status_request(id, 7)?;
            session.eof(id)?;
            session.close(id)?;
            return Ok(());
        }
        if command == b"status-test:limit" {
            session.data(id, vec![b'x'; 512])?;
        }
        session.data(id, command.to_vec())?;
        if command != b"status-test:missing-exit" {
            session.exit_status_request(id, 0)?;
        }
        session.eof(id)?;
        session.close(id)?;
        Ok(())
    }
    async fn channel_open_direct_tcpip(
        &mut self,
        channel: Channel<server::Msg>,
        host: &str,
        _: u32,
        _: &str,
        _: u32,
        reply: server::ChannelOpenHandle,
        _: &mut Session,
    ) -> Result<(), Self::Error> {
        self.hosts.lock().unwrap().push(host.into());
        let dest = self.routes.lock().unwrap().get(host).copied();
        if let Some(dest) = dest {
            let socket = TcpStream::connect(dest).await?;
            reply.accept().await;
            tokio::spawn(async move {
                let _ = tokio::io::copy_bidirectional(&mut channel.into_stream(), &mut { socket })
                    .await;
            });
        }
        Ok(())
    }
}
struct SshServer {
    addr: SocketAddr,
    state: Ssh,
    stop: CancellationToken,
    key: russh::keys::PrivateKey,
    configuration: Arc<Mutex<Arc<server::Config>>>,
}
impl SshServer {
    async fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let key =
            russh::keys::PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519)
                .unwrap();
        let config = Arc::new(server::Config {
            keys: vec![key.clone()],
            auth_rejection_time: Duration::ZERO,
            auth_rejection_time_initial: Some(Duration::ZERO),
            ..Default::default()
        });
        let configuration = Arc::new(Mutex::new(config));
        let settings = configuration.clone();
        let state = Ssh::default();
        let state2 = state.clone();
        let stop = CancellationToken::new();
        let stopped = stop.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {_=stopped.cancelled()=>break,result=listener.accept()=>{
                    let (socket,_)=result.unwrap();let config=settings.lock().unwrap().clone();let handler=state2.clone();let stopped=stopped.clone();
                    handler.live.fetch_add(1,Ordering::Relaxed);
                    tokio::spawn(async move{
                        if let Ok(session)=server::run_stream(config,socket,handler.clone()).await {
                            let handle=session.handle();
                            tokio::select!{_=stopped.cancelled()=>{let _=handle.disconnect(russh::Disconnect::ByApplication,"test shutdown".into(),"en".into()).await;},_=session=>{}}
                        }
                        handler.live.fetch_sub(1,Ordering::Relaxed);
                    });
                }}
            }
        });
        Self {
            addr,
            state,
            stop,
            key,
            configuration,
        }
    }
}
impl Drop for SshServer {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

struct Socks {
    addr: SocketAddr,
    hosts: Arc<Mutex<Vec<String>>>,
    stop: CancellationToken,
}
impl Socks {
    async fn independent(destination: SocketAddr, auth: bool) -> Self {
        use fast_socks5::server::Socks5ServerProtocol;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let hosts = Arc::new(Mutex::new(vec![]));
        let stop = CancellationToken::new();
        let stopped = stop.clone();
        let h = hosts.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! { _=stopped.cancelled()=>break, accepted=listener.accept()=>{
                    let (socket, _) = accepted.unwrap();
                    let h = h.clone(); let stopped = stopped.clone();
                    tokio::spawn(async move {
                        let work = async {
                            let protocol = if auth {
                                Socks5ServerProtocol::accept_password_auth(socket, |user, pass| user == "proxy" && pass == "proxy-secret").await?.0
                            } else { Socks5ServerProtocol::accept_no_auth(socket).await? };
                            let (protocol, command, address) = protocol.read_command().await?;
                            assert_eq!(command, fast_socks5::Socks5Command::TCPConnect);
                            h.lock().unwrap().push(address.to_string());
                            let mut remote = TcpStream::connect(destination).await?;
                            let mut socket = protocol.reply_success(remote.local_addr()?).await?;
                            tokio::io::copy_bidirectional(&mut socket, &mut remote).await?;
                            Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
                        };
                        tokio::select! { _=stopped.cancelled()=>{}, _=work=>{} }
                    });
                }}
            }
        });
        Self { addr, hosts, stop }
    }
    async fn new(destination: SocketAddr, auth: bool) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let hosts = Arc::new(Mutex::new(vec![]));
        let h = hosts.clone();
        let stop = CancellationToken::new();
        let stopped = stop.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {_=stopped.cancelled()=>break,accepted=listener.accept()=>{
                    let (mut socket,_)=accepted.unwrap();let h=h.clone();let stopped=stopped.clone();
                    tokio::spawn(async move{let work=async{
                        let mut hello=[0;3];socket.read_exact(&mut hello).await?;
                        assert_eq!(hello,[5,1,if auth{2}else{0}]);socket.write_all(&[5,hello[2]]).await?;
                        if auth {
                            assert_eq!(socket.read_u8().await?,1);let u=socket.read_u8().await?;let mut user=vec![0;u as usize];socket.read_exact(&mut user).await?;
                            let p=socket.read_u8().await?;let mut pass=vec![0;p as usize];socket.read_exact(&mut pass).await?;
                            assert_eq!(user,b"proxy");assert_eq!(pass,b"proxy-secret");socket.write_all(&[1,0]).await?;
                        }
                        let mut head=[0;4];socket.read_exact(&mut head).await?;assert_eq!(head,[5,1,0,3]);
                        let n=socket.read_u8().await?;let mut host=vec![0;n as usize];socket.read_exact(&mut host).await?;h.lock().unwrap().push(String::from_utf8(host).unwrap());
                        let _=socket.read_u16().await?;let mut remote=TcpStream::connect(destination).await?;
                        socket.write_all(&[5,0,0,1,127,0,0,1,0,0]).await?;
                        tokio::io::copy_bidirectional(&mut socket,&mut remote).await?;Ok::<_,std::io::Error>(())
                    };tokio::select!{_=stopped.cancelled()=>{},_=work=>{}}});
                }}
            }
        });
        Self { addr, hosts, stop }
    }
}
impl Drop for Socks {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

static SEQUENCE: AtomicUsize = AtomicUsize::new(0);
include!("fixtures/terminal_flow.rs");
include!("fixtures/files_flow.rs");
include!("fixtures/network_flow.rs");
include!("fixtures/system_status_flow.rs");
struct Fixture {
    core: Arc<AppCore>,
    path: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "sodeck-connections-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        Self {
            core: AppCore::open(path.clone(), "test".into()).unwrap(),
            path,
        }
    }
    fn change(&self, v: Value) -> Value {
        self.core
            .change_catalog(
                self.core.server_catalog()["revision"].as_u64().unwrap(),
                serde_json::from_value(v).unwrap(),
            )
            .unwrap()
    }
    fn server(&self, host: &str, port: u16, route: Value) -> String {
        self.change(json!({"op":"saveServer","draft":{"name":host,"host":host,"port":port,"username":"ops","authentication":{"type":"password","secret":{"action":"replace","value":"secret","remember":true}},"route":route}}))["selectedId"].as_str().unwrap().into()
    }
    fn proxy(&self, host: &str, port: u16, auth: bool) -> String {
        self.change(json!({"op":"saveProxy","draft":{"name":host,"host":host,"port":port,"authentication":if auth{json!({"type":"usernamePassword","username":"proxy","secret":{"action":"replace","value":"proxy-secret","remember":true}})}else{json!({"type":"none","secret":{"action":"clear"}})}}}))["selectedId"].as_str().unwrap().into()
    }
    fn disk(&self) -> Value {
        serde_json::from_slice(&std::fs::read(self.path.join("servers.json")).unwrap()).unwrap()
    }
    fn password(&self, id: &str, value: Option<&str>) {
        let mut draft = self.core.server_catalog()["servers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"] == id)
            .unwrap()
            .clone();
        draft["authentication"] = json!({"type":"password","secret":match value {
            Some(value) => json!({"action":"replace","value":value,"remember":true}),
            None => json!({"action":"clear"}),
        }});
        self.change(json!({"op":"saveServer","draft":draft}));
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.core.shutdown();
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
async fn until(mut check: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(8), async {
        while !check() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("condition did not settle");
}
async fn connected(f: &Fixture, id: &str) -> View {
    connected_in(f, id, "main").await
}
async fn connected_in(f: &Fixture, id: &str, window: &str) -> View {
    let start = f.core.start_connection(window, id).unwrap();
    let mut answered = HashSet::new();
    until(|| {
        let v = f
            .core
            .connection_views()
            .into_iter()
            .find(|v| v.attempt_id == start.attempt_id)
            .unwrap();
        if let Some(Question::HostKey { id, .. }) = v.question {
            if answered.insert(id.clone()) {
                f.core
                    .answer_connection(window, &v.attempt_id, &id, Answer::HostKey { accept: true })
                    .unwrap();
            }
        }
        assert!(v.state != "failed", "{:?}", v.error);
        v.state == "connected"
    })
    .await;
    f.core
        .connection_views()
        .into_iter()
        .find(|v| v.server_id == id)
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn new_connection_belongs_to_initiating_window_and_reconnect_keeps_owner() {
    let f = Fixture::new();
    let target = SshServer::new().await;
    let id = f.server("127.0.0.1", target.addr.port(), json!([]));
    f.core
        .workspaces
        .lock()
        .unwrap()
        .register("secondary")
        .unwrap();
    let first = connected_in(&f, &id, "secondary").await.context.unwrap();
    assert!(f
        .core
        .workspace_projection("main")
        .unwrap()
        .workspaces
        .is_empty());
    let secondary = f.core.workspace_projection("secondary").unwrap();
    assert_eq!(
        secondary.window.active_workspace.as_ref(),
        Some(&first.workspace_id)
    );
    assert_eq!(secondary.workspaces[0].window_id, "secondary");
    assert_eq!(
        f.core
            .start_connection("main", &id)
            .unwrap()
            .context
            .as_ref(),
        Some(&first)
    );
    assert_eq!(target.state.auth.load(Ordering::Relaxed), 1);
    f.core.disconnect_connection("secondary", &first).unwrap();
    until(|| f.core.connection_views()[0].state == "disconnected").await;
    let second = connected(&f, &id).await.context.unwrap();
    assert_eq!(second.workspace_id, first.workspace_id);
    assert_eq!(second.generation, 2);
    assert_eq!(
        f.core.workspace_projection("secondary").unwrap().workspaces[0].context,
        second
    );
    assert!(f
        .core
        .workspace_projection("main")
        .unwrap()
        .workspaces
        .is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn closing_initiating_window_cancels_pending_connection_without_a_workspace() {
    let f = Fixture::new();
    let target = SshServer::new().await;
    let id = f.server("127.0.0.1", target.addr.port(), json!([]));
    f.core
        .workspaces
        .lock()
        .unwrap()
        .register("secondary")
        .unwrap();
    let attempt = f.core.start_connection("secondary", &id).unwrap();
    until(|| f.core.connection_views()[0].question.is_some()).await;
    f.core.cancel_window_connections("secondary");
    f.core
        .workspaces
        .lock()
        .unwrap()
        .close_window("secondary")
        .unwrap();
    until(|| f.core.connection_views()[0].state == "cancelled").await;
    assert!(f
        .core
        .workspace_projection("main")
        .unwrap()
        .workspaces
        .is_empty());
    assert_eq!(f.core.connection_views()[0].attempt_id, attempt.attempt_id);
    assert_eq!(target.state.auth.load(Ordering::Relaxed), 0);
}

async fn credential_prompt(f: &Fixture) -> View {
    until(|| {
        let v = f.core.connection_views().remove(0);
        if let Some(Question::HostKey { id, .. }) = &v.question {
            f.core
                .answer_connection("main", &v.attempt_id, &id, Answer::HostKey { accept: true })
                .unwrap();
        }
        assert!(v.state != "failed", "{:?}", v.error);
        matches!(v.question, Some(Question::Credentials { .. }))
    })
    .await;
    f.core.connection_views().remove(0)
}

async fn credentials_accepted(f: &Fixture) -> View {
    until(|| {
        let v = f.core.connection_views().remove(0);
        if let Some(Question::HostKey { id, .. }) = &v.question {
            f.core
                .answer_connection("main", &v.attempt_id, id, Answer::HostKey { accept: true })
                .unwrap();
        }
        assert!(v.state != "failed", "{:?}", v.error);
        v.state == "connected"
    })
    .await;
    f.core.connection_views().remove(0)
}

async fn next_credential_prompt(f: &Fixture, previous: &View) -> View {
    let Some(Question::Credentials {
        id: previous_id, ..
    }) = &previous.question
    else {
        panic!()
    };
    until(|| {
        let v = f.core.connection_views().remove(0);
        if let Some(Question::HostKey { id, .. }) = &v.question {
            f.core
                .answer_connection("main", &v.attempt_id, id, Answer::HostKey { accept: true })
                .unwrap();
        }
        assert!(v.state != "failed", "{:?}", v.error);
        matches!(&v.question, Some(Question::Credentials { id, .. }) if id != previous_id)
    })
    .await;
    credential_prompt(f).await
}

fn submit_credentials(f: &Fixture, v: &View, secret: &str, remember: bool) {
    let Some(Question::Credentials { id, .. }) = &v.question else {
        panic!("expected credential prompt")
    };
    f.core
        .answer_connection(
            "main",
            &v.attempt_id,
            id,
            serde_json::from_value(json!({"kind":"credentials","secret":secret,"privateKeyPath":null,"remember":remember})).unwrap(),
        )
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn expired_saved_password_is_replaced_after_success_and_reused_on_reconnect() {
    let f = Fixture::new();
    let target = SshServer::new().await;
    let id = f.server("127.0.0.1", target.addr.port(), json!([]));
    f.password(&id, Some("expired-password"));
    f.core.start_connection("main", &id).unwrap();
    let prompt = credential_prompt(&f).await;
    submit_credentials(&f, &prompt, "wrong-password", true);
    let prompt = next_credential_prompt(&f, &prompt).await;
    assert_eq!(
        f.disk()["servers"][0]["authentication"]["savedPassword"],
        "expired-password"
    );
    assert!(matches!(
        &prompt.question,
        Some(Question::Credentials {
            message: Some(_),
            remember: true,
            ..
        })
    ));
    submit_credentials(&f, &prompt, "secret", true);
    until(|| {
        let v = f.core.connection_views().remove(0);
        assert!(v.state != "failed", "{:?}", v.error);
        v.state == "connected"
    })
    .await;
    let disk: Value =
        serde_json::from_slice(&std::fs::read(f.path.join("servers.json")).unwrap()).unwrap();
    assert_eq!(
        disk["servers"][0]["authentication"]["savedPassword"],
        "secret"
    );
    let context = f.core.connection_views()[0].context.clone().unwrap();
    f.core.disconnect_connection("main", &context).unwrap();
    until(|| f.core.connection_views()[0].state == "disconnected").await;
    connected(&f, &id).await;
    assert_eq!(target.state.auth.load(Ordering::Relaxed), 4);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unchecked_credentials_only_apply_to_this_attempt_and_preserve_saved_password() {
    for previous in [None, Some("expired-password")] {
        let f = Fixture::new();
        let target = SshServer::new().await;
        let id = f.server("127.0.0.1", target.addr.port(), json!([]));
        f.password(&id, previous);
        let original = f.disk();
        f.core.start_connection("main", &id).unwrap();
        let prompt = credential_prompt(&f).await;
        assert!(matches!(
            prompt.question,
            Some(Question::Credentials { remember: true, .. })
        ));
        submit_credentials(&f, &prompt, "secret", false);
        // Missing credentials are collected before the host identity question.
        until(|| {
            let v = f.core.connection_views().remove(0);
            if let Some(Question::HostKey { id, .. }) = v.question {
                f.core
                    .answer_connection("main", &v.attempt_id, &id, Answer::HostKey { accept: true })
                    .unwrap();
            }
            v.state == "connected"
        })
        .await;
        assert_eq!(f.disk(), original);
        assert_eq!(
            f.core.server_catalog()["servers"][0]["authentication"]["hasSessionSecret"],
            false
        );
        let context = f.core.connection_views()[0].context.clone().unwrap();
        f.core.disconnect_connection("main", &context).unwrap();
        until(|| f.core.connection_views()[0].state == "disconnected").await;
        f.core.start_connection("main", &id).unwrap();
        let retry = credential_prompt(&f).await;
        f.core.cancel_connection("main", &retry.attempt_id).unwrap();
        until(|| f.core.connection_views()[0].state == "cancelled").await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_or_rejected_host_never_saves_submitted_credentials() {
    for accept in [false, true] {
        let f = Fixture::new();
        let target = SshServer::new().await;
        let id = f.server("127.0.0.1", target.addr.port(), json!([]));
        f.password(&id, None);
        let original = f.disk();
        f.core.start_connection("main", &id).unwrap();
        let prompt = credential_prompt(&f).await;
        submit_credentials(&f, &prompt, "secret", true);
        until(|| {
            matches!(
                f.core.connection_views()[0].question,
                Some(Question::HostKey { .. })
            )
        })
        .await;
        let v = f.core.connection_views().remove(0);
        if accept {
            f.core.cancel_connection("main", &v.attempt_id).unwrap();
        } else if let Some(Question::HostKey { id, .. }) = v.question {
            f.core
                .answer_connection(
                    "main",
                    &v.attempt_id,
                    &id,
                    Answer::HostKey { accept: false },
                )
                .unwrap();
        }
        until(|| f.core.connection_views()[0].state != "connecting").await;
        assert_eq!(f.disk(), original);
        assert_eq!(target.state.auth.load(Ordering::Relaxed), 0);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn credential_save_failure_keeps_old_password_and_can_retry() {
    let f = Fixture::new();
    let target = SshServer::new().await;
    let id = f.server("127.0.0.1", target.addr.port(), json!([]));
    f.password(&id, Some("expired-password"));
    let original = f.disk();
    f.core.start_connection("main", &id).unwrap();
    let prompt = credential_prompt(&f).await;
    std::fs::create_dir(f.path.join("servers.json.tmp")).unwrap();
    submit_credentials(&f, &prompt, "secret", true);
    until(|| f.core.connection_views()[0].state == "failed").await;
    assert_eq!(
        f.core.connection_views()[0].error.as_ref().unwrap().code,
        "credentialsSave"
    );
    assert_eq!(f.disk(), original);
    assert!(f
        .core
        .workspace_projection("main")
        .unwrap()
        .workspaces
        .is_empty());
    std::fs::remove_dir(f.path.join("servers.json.tmp")).unwrap();
    f.core.start_connection("main", &id).unwrap();
    let prompt = credential_prompt(&f).await;
    submit_credentials(&f, &prompt, "secret", true);
    credentials_accepted(&f).await;
    assert_eq!(
        f.disk()["servers"][0]["authentication"]["savedPassword"],
        "secret"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_password_edit_is_not_overwritten_by_an_old_connection_prompt() {
    let f = Fixture::new();
    let target = SshServer::new().await;
    let id = f.server("127.0.0.1", target.addr.port(), json!([]));
    f.password(&id, Some("expired-password"));
    f.core.start_connection("main", &id).unwrap();
    let prompt = credential_prompt(&f).await;
    f.password(&id, Some("newer-edit"));
    submit_credentials(&f, &prompt, "secret", true);
    until(|| f.core.connection_views()[0].state == "failed").await;
    assert_eq!(
        f.core.connection_views()[0].error.as_ref().unwrap().code,
        "profileChanged"
    );
    assert_eq!(
        f.disk()["servers"][0]["authentication"]["savedPassword"],
        "newer-edit"
    );
    assert!(f
        .core
        .workspace_projection("main")
        .unwrap()
        .workspaces
        .is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mixed_route_saves_each_record_atomically_after_target_authentication() {
    for cancel in [true, false] {
        let f = Fixture::new();
        let target = SshServer::new().await;
        let socks = Socks::new(target.addr, true).await;
        let gateway = SshServer::new().await;
        gateway
            .state
            .routes
            .lock()
            .unwrap()
            .insert("proxy.remote.invalid".into(), socks.addr);
        let jump = f.server("127.0.0.1", gateway.addr.port(), json!([]));
        f.password(&jump, Some("old-jump-password"));
        let proxy = f.proxy("proxy.remote.invalid", 1080, true);
        let mut draft = f.core.server_catalog()["proxies"][0].clone();
        draft["authentication"] =
            json!({"type":"usernamePassword","username":"proxy","secret":{"action":"clear"}});
        f.change(json!({"op":"saveProxy","draft":draft}));
        let id = f.server(
            "target.remote.invalid",
            22,
            json!([
                {"kind":"ssh","id":"","serverId":jump},
                {"kind":"socks5","id":"","proxyId":proxy}
            ]),
        );
        f.password(&id, Some("old-target-password"));
        let original = f.disk();
        f.core.start_connection("main", &id).unwrap();
        let prompt = credential_prompt(&f).await;
        assert!(matches!(
            prompt.question,
            Some(Question::Credentials { node_index: 1, .. })
        ));
        submit_credentials(&f, &prompt, "proxy-secret", true);
        let prompt = next_credential_prompt(&f, &prompt).await;
        assert!(matches!(
            prompt.question,
            Some(Question::Credentials { node_index: 0, .. })
        ));
        submit_credentials(&f, &prompt, "secret", true);
        let prompt = next_credential_prompt(&f, &prompt).await;
        assert!(matches!(
            prompt.question,
            Some(Question::Credentials { node_index: 2, .. })
        ));
        assert_eq!(f.disk(), original);
        if cancel {
            f.core
                .cancel_connection("main", &prompt.attempt_id)
                .unwrap();
            until(|| f.core.connection_views()[0].state == "cancelled").await;
            assert_eq!(f.disk(), original);
        } else {
            submit_credentials(&f, &prompt, "secret", true);
            let v = credentials_accepted(&f).await;
            let disk = f.disk();
            for record in disk["servers"].as_array().unwrap() {
                assert_eq!(record["authentication"]["savedPassword"], "secret");
            }
            assert_eq!(
                disk["proxies"][0]["authentication"]["savedPassword"],
                "proxy-secret"
            );
            assert_eq!(
                disk["revision"].as_u64().unwrap(),
                original["revision"].as_u64().unwrap() + 1
            );
            let serialized = serde_json::to_string(&f.core.connection_views()).unwrap();
            assert!(!serialized.contains("proxy-secret"));
            assert_eq!(
                f.core
                    .connection_probe(&v.context.unwrap(), "mixed-credentials")
                    .await
                    .unwrap()
                    .0,
                b"mixed-credentials"
            );
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mixed_chain_uses_remote_dns_and_only_adopts_authenticated_target() {
    let f = Fixture::new();
    let target = SshServer::new().await;
    let inner = Socks::new(target.addr, true).await;
    let gateway = SshServer::new().await;
    gateway
        .state
        .routes
        .lock()
        .unwrap()
        .insert("proxy.remote.invalid".into(), inner.addr);
    let outer = Socks::new(gateway.addr, false).await;
    let p1 = f.proxy("127.0.0.1", outer.addr.port(), false);
    let jump = f.server("gateway.remote.invalid", 22, json!([]));
    let mut gateway_draft = f.core.server_catalog()["servers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == jump)
        .unwrap()
        .clone();
    gateway_draft["route"] = json!([{"kind":"socks5","id":"","proxyId":p1}]);
    gateway_draft["authentication"] =
        json!({"type":"password","secret":{"action":"keep","remember":true}});
    f.change(json!({"op":"saveServer","draft":gateway_draft}));
    let p2 = f.proxy("proxy.remote.invalid", 1080, true);
    let id=f.server("target.remote.invalid",22,json!([{"kind":"socks5","id":"","proxyId":p1},{"kind":"ssh","id":"","serverId":jump},{"kind":"socks5","id":"","proxyId":p2}]));
    let v = connected(&f, &id).await;
    let context = v.context.unwrap();
    let (bytes, code) = f
        .core
        .connection_probe(&context, "mixed-chain-ok")
        .await
        .unwrap();
    assert_eq!(bytes, b"mixed-chain-ok");
    assert_eq!(code, 0);
    assert_eq!(*outer.hosts.lock().unwrap(), vec!["gateway.remote.invalid"]);
    assert_eq!(
        *gateway.state.hosts.lock().unwrap(),
        vec!["proxy.remote.invalid"]
    );
    assert_eq!(*inner.hosts.lock().unwrap(), vec!["target.remote.invalid"]);
    assert_eq!(gateway.state.auth.load(Ordering::Relaxed), 1);
    assert_eq!(target.state.auth.load(Ordering::Relaxed), 1);
    assert!(
        f.core.connection_views()[0]
            .sent_bytes
            .parse::<u64>()
            .unwrap()
            > 0
    );
    f.core.disconnect_connection("main", &context).unwrap();
    until(|| {
        gateway.state.live.load(Ordering::Relaxed) == 0
            && target.state.live.load(Ordering::Relaxed) == 0
    })
    .await;
    until(|| f.core.connection_views()[0].state == "disconnected").await;
    assert!(!f.core.workspace_projection("main").unwrap().workspaces[0].connected);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn host_rejection_never_sends_credentials_and_cancelled_reply_is_rejected() {
    let f = Fixture::new();
    let target = SshServer::new().await;
    let id = f.server("127.0.0.1", target.addr.port(), json!([]));
    f.core.start_connection("main", &id).unwrap();
    until(|| f.core.connection_views()[0].question.is_some()).await;
    let v = f.core.connection_views().remove(0);
    let Question::HostKey { id: q, .. } = v.question.unwrap() else {
        panic!()
    };
    assert!(f
        .core
        .answer_connection("other", &v.attempt_id, &q, Answer::HostKey { accept: true })
        .is_err());
    f.core
        .answer_connection("main", &v.attempt_id, &q, Answer::HostKey { accept: false })
        .unwrap();
    until(|| f.core.connection_views()[0].state == "failed").await;
    assert_eq!(target.state.auth.load(Ordering::Relaxed), 0);
    assert!(f
        .core
        .workspace_projection("main")
        .unwrap()
        .workspaces
        .is_empty());
    assert!(f
        .core
        .answer_connection("main", &v.attempt_id, &q, Answer::HostKey { accept: true })
        .is_err());
    assert!(!std::fs::read_to_string(f.path.join("ssh-hosts.json"))
        .unwrap_or_default()
        .contains("SHA256:"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_handshake_and_profile_edit_release_the_real_socket() {
    let f = Fixture::new();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let id = f.server(
        "127.0.0.1",
        listener.local_addr().unwrap().port(),
        json!([]),
    );
    let v = f.core.start_connection("main", &id).unwrap();
    let (mut socket, _) = tokio::time::timeout(Duration::from_secs(3), listener.accept())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        f.core.start_connection("main", &id).unwrap().attempt_id,
        v.attempt_id
    );
    f.core.cancel_connection("main", &v.attempt_id).unwrap();
    let mut bytes = vec![];
    tokio::time::timeout(Duration::from_secs(3), socket.read_to_end(&mut bytes))
        .await
        .unwrap()
        .unwrap();
    until(|| f.core.connection_views()[0].state == "cancelled").await;
    assert_eq!(
        f.core.connection_views()[0].error.as_ref().unwrap().code,
        "cancelled"
    );
    assert!(f
        .core
        .workspace_projection("main")
        .unwrap()
        .workspaces
        .is_empty());
    f.core.start_connection("main", &id).unwrap();
    let (mut socket, _) = tokio::time::timeout(Duration::from_secs(3), listener.accept())
        .await
        .unwrap()
        .unwrap();
    let mut draft = f.core.server_catalog()["servers"][0].clone();
    draft["host"] = json!("changed.invalid");
    draft["authentication"] = json!({"type":"password","secret":{"action":"keep","remember":true}});
    f.change(json!({"op":"saveServer","draft":draft}));
    let mut bytes = vec![];
    tokio::time::timeout(Duration::from_secs(3), socket.read_to_end(&mut bytes))
        .await
        .unwrap()
        .unwrap();
    until(|| f.core.connection_views()[0].state == "cancelled").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_socks_negotiation_releases_the_socket_at_each_stage() {
    for stage in 0..3 {
        let f = Fixture::new();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy = f.proxy("127.0.0.1", listener.local_addr().unwrap().port(), true);
        let id = f.server(
            "ssh.only.remote.invalid",
            22,
            json!([{"kind":"socks5","id":"","proxyId":proxy}]),
        );
        let view = f.core.start_connection("main", &id).unwrap();
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut greeting = [0; 3];
        socket.read_exact(&mut greeting).await.unwrap();
        assert_eq!(greeting, [5, 1, 2]);
        if stage > 0 {
            socket.write_all(&[5, 2]).await.unwrap();
            let mut auth = [0; 20];
            socket.read_exact(&mut auth).await.unwrap();
            assert_eq!(&auth, b"\x01\x05proxy\x0cproxy-secret");
        }
        if stage > 1 {
            socket.write_all(&[1, 0]).await.unwrap();
            let mut prefix = [0; 5];
            socket.read_exact(&mut prefix).await.unwrap();
            assert_eq!(&prefix[..4], &[5, 1, 0, 3]);
            let mut target = vec![0; usize::from(prefix[4]) + 2];
            socket.read_exact(&mut target).await.unwrap();
            // Suspend while the core is reading a successful binding frame.
            socket.write_all(&[5, 0, 0, 4, 0]).await.unwrap();
        }
        f.core.cancel_connection("main", &view.attempt_id).unwrap();
        let mut rest = Vec::new();
        let closed = tokio::time::timeout(Duration::from_secs(5), socket.read_to_end(&mut rest))
            .await
            .expect("cancelled SOCKS socket remained open");
        assert!(
            closed.is_ok()
                || closed.is_err_and(|e| e.kind() == std::io::ErrorKind::ConnectionReset)
        );
        assert!(rest.is_empty(), "negotiation continued after cancellation");
        until(|| f.core.connection_views()[0].state == "cancelled").await;
        assert!(f
            .core
            .workspace_projection("main")
            .unwrap()
            .workspaces
            .is_empty());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reconnect_preserves_workspace_moves_and_rejects_changed_host_before_auth() {
    let f = Fixture::new();
    let target = SshServer::new().await;
    let id = f.server("127.0.0.1", target.addr.port(), json!([]));
    let first = connected(&f, &id).await.context.unwrap();
    {
        let mut w = f.core.workspaces.lock().unwrap();
        w.register("secondary").unwrap();
        w.ready("secondary").unwrap();
        let m = w
            .prepare("main", &first.workspace_id, "secondary", 0, "move".into())
            .unwrap();
        w.commit(&m).unwrap();
    }
    assert_eq!(
        f.core
            .connection_probe(&first, "after-move")
            .await
            .unwrap()
            .0,
        b"after-move"
    );
    f.core.disconnect_connection("secondary", &first).unwrap();
    until(|| f.core.connection_views()[0].state == "disconnected").await;
    f.core
        .workspaces
        .lock()
        .unwrap()
        .validate_owned("secondary", &first)
        .unwrap();
    let second = connected(&f, &id).await.context.unwrap();
    assert_eq!(first.workspace_id, second.workspace_id);
    assert_eq!(second.generation, 2);
    assert_ne!(first.connection_id, second.connection_id);
    assert!(f.core.connection_probe(&first, "stale").await.is_err());
    assert_eq!(
        f.core.workspace_projection("secondary").unwrap().workspaces[0].context,
        second
    );
    f.core.disconnect_connection("secondary", &second).unwrap();
    until(|| f.core.connection_views()[0].state == "disconnected").await;
    let rotated =
        russh::keys::PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519).unwrap();
    *target.configuration.lock().unwrap() = Arc::new(server::Config {
        keys: vec![rotated],
        auth_rejection_time: Duration::ZERO,
        auth_rejection_time_initial: Some(Duration::ZERO),
        ..Default::default()
    });
    f.core.start_connection("secondary", &id).unwrap();
    until(|| f.core.connection_views()[0].question.is_some()).await;
    let v = f.core.connection_views().remove(0);
    let Question::HostKey {
        id: q,
        previous_fingerprint: Some(old),
        fingerprint: new,
        ..
    } = v.question.unwrap()
    else {
        panic!("changed host must require confirmation")
    };
    assert_ne!(old, new);
    assert_eq!(target.state.auth.load(Ordering::Relaxed), 2);
    f.core
        .answer_connection(
            "secondary",
            &v.attempt_id,
            &q,
            Answer::HostKey { accept: true },
        )
        .unwrap();
    until(|| f.core.connection_views()[0].state == "connected").await;
    assert_eq!(
        f.core.connection_views()[0]
            .context
            .as_ref()
            .unwrap()
            .generation,
        3
    );
    f.core.cancel_workspace_connection(&second.workspace_id);
    until(|| target.state.live.load(Ordering::Relaxed) == 0).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wrong_password_reprompts_without_saving_or_creating_a_workspace() {
    let f = Fixture::new();
    let target = SshServer::new().await;
    let id = f.server("127.0.0.1", target.addr.port(), json!([]));
    let mut draft = f.core.server_catalog()["servers"][0].clone();
    draft["authentication"] = json!({"type":"password","secret":{"action":"clear"}});
    f.change(json!({"op":"saveServer","draft":draft}));
    f.core.start_connection("main", &id).unwrap();
    until(|| f.core.connection_views()[0].question.is_some()).await;
    let v = f.core.connection_views().remove(0);
    let Question::Credentials { id: q, .. } = v.question.unwrap() else {
        panic!()
    };
    f.core
        .answer_connection(
            "main",
            &v.attempt_id,
            &q,
            Answer::Credentials {
                secret: "wrong-password".into(),
                private_key_path: None,
                remember: false,
            },
        )
        .unwrap();
    until(|| {
        let v = f.core.connection_views().remove(0);
        if let Some(Question::HostKey { id, .. }) = &v.question {
            f.core
                .answer_connection("main", &v.attempt_id, &id, Answer::HostKey { accept: true })
                .unwrap();
        }
        matches!(&v.question, Some(Question::Credentials { id, .. }) if id != &q)
    })
    .await;
    assert_eq!(
        matches!(
            f.core.connection_views()[0].question,
            Some(Question::Credentials {
                remember: false,
                message: Some(_),
                ..
            })
        ),
        true
    );
    assert!(f
        .core
        .workspace_projection("main")
        .unwrap()
        .workspaces
        .is_empty());
    assert!(!serde_json::to_string(&f.core.connection_views())
        .unwrap()
        .contains("wrong-password"));
    for name in ["servers.json", "ssh-hosts.json"] {
        assert!(!std::fs::read_to_string(f.path.join(name))
            .unwrap()
            .contains("wrong-password"));
    }
    f.core.cancel_connection("main", &v.attempt_id).unwrap();
    until(|| f.core.connection_views()[0].state == "cancelled").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn private_key_prompt_saves_verified_path_and_passphrase_and_retries_wrong_input() {
    let f = Fixture::new();
    let target = SshServer::new().await;
    let id = f.server("127.0.0.1", target.addr.port(), json!([]));
    let key = target.key.encrypt(&mut rand::rng(), "key-phrase").unwrap();
    let path = f.path.join("prompt-key");
    std::fs::write(
        &path,
        key.to_openssh(russh::keys::ssh_key::LineEnding::LF)
            .unwrap()
            .as_bytes(),
    )
    .unwrap();
    let mut draft = f.core.server_catalog()["servers"][0].clone();
    draft["authentication"] =
        json!({"type":"privateKey","privateKeyPath":null,"secret":{"action":"clear"}});
    f.change(json!({"op":"saveServer","draft":draft}));
    let original = f.disk();
    f.core.start_connection("main", &id).unwrap();
    let prompt = credential_prompt(&f).await;
    let Some(Question::Credentials { id: question, .. }) = &prompt.question else {
        panic!()
    };
    f.core.answer_connection("main", &prompt.attempt_id, question,
        serde_json::from_value(json!({"kind":"credentials","secret":"wrong-phrase","privateKeyPath":path,"remember":true})).unwrap()).unwrap();
    let prompt = next_credential_prompt(&f, &prompt).await;
    assert!(
        matches!(&prompt.question, Some(Question::Credentials { message: Some(message), .. }) if message.contains("口令错误"))
    );
    assert_eq!(f.disk(), original);
    submit_credentials(&f, &prompt, "key-phrase", true);
    let v = credentials_accepted(&f).await;
    assert_eq!(
        f.disk()["servers"][0]["authentication"]["privateKeyPath"],
        path.to_str().unwrap()
    );
    assert_eq!(
        f.disk()["servers"][0]["authentication"]["savedPassphrase"],
        "key-phrase"
    );
    assert!(!serde_json::to_string(&f.core.connection_views())
        .unwrap()
        .contains("key-phrase"));
    f.core
        .disconnect_connection("main", &v.context.unwrap())
        .unwrap();
    until(|| f.core.connection_views()[0].state == "disconnected").await;
    connected(&f, &id).await;
    assert_eq!(target.state.auth.load(Ordering::Relaxed), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn private_key_plain_and_encrypted_ed25519_and_rsa_authenticate() {
    let f = Fixture::new();
    let target = SshServer::new().await;
    let id = f.server("127.0.0.1", target.addr.port(), json!([]));
    for algorithm in [
        russh::keys::Algorithm::Ed25519,
        russh::keys::Algorithm::Rsa { hash: None },
    ] {
        let key = if algorithm == russh::keys::Algorithm::Ed25519 {
            target.key.clone()
        } else {
            russh::keys::PrivateKey::random(&mut rand::rng(), algorithm).unwrap()
        };
        for encrypted in [false, true] {
            let key = if encrypted {
                key.encrypt(&mut rand::rng(), "key-phrase").unwrap()
            } else {
                key.clone()
            };
            let path = f.path.join("test-key");
            std::fs::write(
                &path,
                key.to_openssh(russh::keys::ssh_key::LineEnding::LF)
                    .unwrap()
                    .as_bytes(),
            )
            .unwrap();
            let mut draft = f.core.server_catalog()["servers"][0].clone();
            draft["authentication"] =
                json!({"type":"privateKey","privateKeyPath":path,"secret":{"action":"clear"}});
            f.change(json!({"op":"saveServer","draft":draft}));
            f.core.start_connection("main", &id).unwrap();
            until(|| {
                let v = f.core.connection_views().remove(0);
                if let Some(question) = v.question {
                    let (id, answer) = match question {
                        Question::HostKey { id, .. } => (id, Answer::HostKey { accept: true }),
                        Question::TargetChange { id, .. } => {
                            (id, Answer::TargetChange { accept: true })
                        }
                        Question::Credentials { id, .. } => (
                            id,
                            Answer::Credentials {
                                secret: "key-phrase".into(),
                                private_key_path: None,
                                remember: false,
                            },
                        ),
                    };
                    f.core
                        .answer_connection("main", &v.attempt_id, &id, answer)
                        .unwrap();
                }
                assert!(v.state != "failed", "{:?}", v.error);
                v.state == "connected"
            })
            .await;
            let context = f.core.connection_views()[0].context.clone().unwrap();
            assert_eq!(
                f.core
                    .connection_probe(&context, "key-auth-ok")
                    .await
                    .unwrap()
                    .0,
                b"key-auth-ok"
            );
            f.core.disconnect_connection("main", &context).unwrap();
            until(|| f.core.connection_views()[0].state == "disconnected").await;
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn target_change_and_closing_workspace_cancel_reconnect_before_admission() {
    let f = Fixture::new();
    let target = SshServer::new().await;
    let other = SshServer::new().await;
    let id = f.server("127.0.0.1", target.addr.port(), json!([]));
    let context = connected(&f, &id).await.context.unwrap();
    f.core.disconnect_connection("main", &context).unwrap();
    until(|| f.core.connection_views()[0].state == "disconnected").await;
    let mut draft = f.core.server_catalog()["servers"][0].clone();
    draft["port"] = json!(other.addr.port());
    draft["authentication"] =
        json!({"type":"password","secret":{"action":"replace","value":"secret","remember":true}});
    f.change(json!({"op":"saveServer","draft":draft}));
    f.core.start_connection("main", &id).unwrap();
    until(|| f.core.connection_views()[0].question.is_some()).await;
    assert!(matches!(
        f.core.connection_views()[0].question,
        Some(Question::TargetChange { .. })
    ));
    assert_eq!(other.state.auth.load(Ordering::Relaxed), 0);
    f.core.cancel_workspace_connection(&context.workspace_id);
    f.core
        .workspaces
        .lock()
        .unwrap()
        .close("main", &context.workspace_id)
        .unwrap();
    until(|| f.core.connection_views()[0].state == "cancelled").await;
    assert!(f
        .core
        .workspace_projection("main")
        .unwrap()
        .workspaces
        .is_empty());
    assert_eq!(other.state.live.load(Ordering::Relaxed), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires isolated OpenSSH daemon; run npm run ssh:test"]
async fn openssh_interop() {
    let f = Fixture::new();
    let port = std::env::var("SODECK_TEST_SSH_PORT")
        .unwrap()
        .parse()
        .unwrap();
    let username = std::env::var("SODECK_TEST_SSH_USER").unwrap();
    let key = std::env::var("SODECK_TEST_SSH_KEY").unwrap();
    let id = f.server("127.0.0.1", port, json!([]));
    let mut draft = f.core.server_catalog()["servers"][0].clone();
    draft["username"] = json!(username);
    draft["authentication"] =
        json!({"type":"privateKey","privateKeyPath":key,"secret":{"action":"clear"}});
    f.change(json!({"op":"saveServer","draft":draft}));
    let context = connected(&f, &id).await.context.unwrap();
    let command = if cfg!(windows) {
        "cmd /c echo sodeck-openssh-ok"
    } else {
        "printf sodeck-openssh-ok"
    };
    let (bytes, code) = f.core.connection_probe(&context, command).await.unwrap();
    assert_eq!(code, 0);
    assert!(String::from_utf8(bytes)
        .unwrap()
        .contains("sodeck-openssh-ok"));
    f.core.disconnect_connection("main", &context).unwrap();
    until(|| f.core.connection_views()[0].state == "disconnected").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_profiles_share_the_same_confirmed_host_without_losing_a_login() {
    let f = Fixture::new();
    let target = SshServer::new().await;
    let a = f.server("127.0.0.1", target.addr.port(), json!([]));
    let b = f.server("127.0.0.1", target.addr.port(), json!([]));
    f.core.start_connection("main", &a).unwrap();
    f.core.start_connection("main", &b).unwrap();
    until(|| {
        f.core
            .connection_views()
            .iter()
            .filter(|v| v.question.is_some())
            .count()
            == 2
    })
    .await;
    for v in f.core.connection_views() {
        let Some(Question::HostKey { id, .. }) = v.question else {
            panic!()
        };
        f.core
            .answer_connection("main", &v.attempt_id, &id, Answer::HostKey { accept: true })
            .unwrap();
    }
    until(|| {
        let views = f.core.connection_views();
        assert!(views.iter().all(|v| v.state != "failed"), "{views:?}");
        views.iter().all(|v| v.state == "connected")
    })
    .await;
    assert_eq!(target.state.auth.load(Ordering::Relaxed), 2);
    assert_eq!(
        f.core
            .workspace_projection("main")
            .unwrap()
            .workspaces
            .len(),
        2
    );
}

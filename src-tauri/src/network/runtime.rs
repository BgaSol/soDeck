use super::*;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpSocket},
    sync::{mpsc, OwnedSemaphorePermit},
};

pub(super) fn new_stream(
    id: String,
    l: &Arc<Listener>,
    remote: bool,
    client: Option<String>,
) -> (Arc<TcpStream>, mpsc::Receiver<Command>) {
    let (tx, rx) = mpsc::channel(8);
    (
        Arc::new(TcpStream {
            id,
            client,
            remote,
            state: Mutex::new(StreamState {
                state: if remote { "connecting" } else { "ready" },
                error: None,
                read: None,
                reading: false,
                eof: false,
                budget: 0,
                announced: false,
                claimed: false,
            }),
            commands: tx,
            cancel: l.cancel.child_token(),
            created: Instant::now(),
            target: Mutex::new(None),
        }),
        rx,
    )
}
pub(super) async fn listen(
    l: Arc<Listener>,
    host: std::net::IpAddr,
    port: u16,
    slots: Arc<Semaphore>,
) {
    let result=async {
        let socket=TcpListener::bind((host,port)).await.map_err(|e|e.to_string())?;
        {let mut st=l.status.lock().unwrap();let addr=socket.local_addr().map_err(|e|e.to_string())?;st.address=json!({"host":addr.ip().to_string(),"port":addr.port()});st.state="running";}
        let session_cancel=l.session.cancellation();
        loop {tokio::select!{
            biased;
            _=l.cancel.cancelled()=>break,
            _=session_cancel.cancelled()=>return Err("服务器连接已断开".to_owned()),
            accepted=socket.accept()=>{
                let (socket,peer)=accepted.map_err(|e|e.to_string())?;let _=socket.set_nodelay(true);
                let Ok(permit)=slots.clone().try_acquire_owned() else {drop(socket);continue};
                let (s,rx)={let mut all=l.streams.lock().unwrap();
                    all.retain(|_,s|!matches!(s.state.lock().unwrap().state,"closed"|"failed")||s.created.elapsed()<Duration::from_secs(20));
                    let (active, negotiating) = all.values().filter(|s|!s.remote).fold((0,0), |(active,negotiating),s| {
                        match s.state.lock().unwrap().state {
                            "ready" => (active+1,negotiating+1),
                            "relaying" => (active+1,negotiating),
                            _ => (active,negotiating),
                        }
                    });
                    if active>=64||negotiating>=16||all.len()>=256 {drop(socket);continue}
                    let Some(core)=l.core.upgrade() else {break};
                    let probe=l.probes.lock().unwrap().remove(&peer);
                    let id=format!("{}{}",core.next_id(),probe.map(|id|format!("@probe:{id}")).unwrap_or_default());
                    let (s,rx)=new_stream(id,&l,false,None);all.insert(s.id.clone(),s.clone());(s,rx)};
                tokio::spawn(serve(s,rx,Box::new(socket),Some(permit)));
            }
        }}
        Ok::<(),String>(())
    }.await;
    l.cancel.cancel();
    l.probe_clients.lock().unwrap().clear();
    let mut status = l.status.lock().unwrap();
    status.state = if result.is_ok() { "closed" } else { "failed" };
    if let Err(error) = result {
        status.error = Some(
            json!({"stage":if status.address.is_null(){"listen"}else{"sessionLost"},"message":error}),
        );
    }
}
pub(super) async fn connect(
    l: Arc<Listener>,
    s: Arc<TcpStream>,
    rx: mpsc::Receiver<Command>,
    host: String,
    port: u16,
) {
    let cancel = l.session.cancellation();
    let result = tokio::select! {
        biased;
        _=s.cancel.cancelled()=>Err("已取消".to_owned()),
        _=cancel.cancelled()=>Err("服务器连接已断开".to_owned()),
        r=tokio::time::timeout(Duration::from_secs(10),l.session.open_tcp(&host,port))=>r.unwrap_or_else(|_|Err("目标 TCP 连接超时".into()))
    };
    match result {
        Ok(stream) => {
            s.state.lock().unwrap().state = "ready";
            serve(
                s,
                rx,
                Box::new(super::relay::Metered {
                    inner: stream,
                    listener: l,
                }),
                None,
            )
            .await;
        }
        Err(error) => {
            let mut st = s.state.lock().unwrap();
            st.state = "failed";
            st.error = Some(error);
        }
    }
}
async fn serve(
    s: Arc<TcpStream>,
    mut rx: mpsc::Receiver<Command>,
    stream: Stream,
    _permit: Option<OwnedSemaphorePermit>,
) {
    let mut stream = Some(stream);
    let outcome=async {
        loop {
            let command=tokio::select!{biased;_=s.cancel.cancelled()=>return Ok(()),_=tokio::time::sleep_until((s.created+Duration::from_secs(15)).into())=>return Err("代理协商超时".into()),c=rx.recv()=>c};
            let operation=async {match command {
                Some(Command::Read(max))=>{let mut bytes=vec![0;max];let n=stream.as_mut().unwrap().read(&mut bytes).await.map_err(|e|e.to_string())?;bytes.truncate(n);let mut st=s.state.lock().unwrap();st.budget+=n;st.eof=n==0;st.read=Some(bytes);st.reading=false;}
                Some(Command::Write(bytes))=>{stream.as_mut().unwrap().write_all(&bytes).await.map_err(|e|e.to_string())?;}
                Some(Command::Take(reply))=>{let _=reply.send(stream.take().unwrap());return Ok::<bool,String>(true)},
                Some(Command::Finish)|None=>{let _=stream.as_mut().unwrap().shutdown().await;return Ok(true)}
            }Ok(false)};
            let done=tokio::select!{biased;_=s.cancel.cancelled()=>return Ok(()),r=tokio::time::timeout(Duration::from_secs(10),operation)=>r.map_err(|_|"协商 I/O 超时".to_owned())??};
            if done{return Ok(())}
        }
    }.await;
    if s.state.lock().unwrap().claimed {
        s.cancel.cancelled().await;
    }
    let mut state = s.state.lock().unwrap();
    if !state.claimed {
        state.state = if outcome.is_ok() { "closed" } else { "failed" };
        state.error = outcome.err();
    }
}
pub(super) async fn splice(l: Arc<Listener>, a: Arc<TcpStream>, b: Arc<TcpStream>) {
    let work = async {
        let (tx, rx) = oneshot::channel();
        let (ty, ry) = oneshot::channel();
        a.commands
            .send(Command::Take(tx))
            .await
            .map_err(|_| "本地流已关闭")?;
        b.commands
            .send(Command::Take(ty))
            .await
            .map_err(|_| "远端流已关闭")?;
        let mut left = rx.await.map_err(|_| "本地流移交失败")?;
        let mut right = ry.await.map_err(|_| "远端流移交失败")?;
        // Tokio preserves half-close: EOF in one direction still drains the other.
        tokio::io::copy_bidirectional(&mut left, &mut right)
            .await
            .map_err(|e| e.to_string())?;
        Ok::<(), String>(())
    };
    let session = l.session.cancellation();
    let result = tokio::select! {biased;_=a.cancel.cancelled()=>Ok(()),_=b.cancel.cancelled()=>Ok(()),_=session.cancelled()=>Err("服务器连接已断开".into()),r=work=>r};
    for s in [&a, &b] {
        s.cancel.cancel();
        let mut st = s.state.lock().unwrap();
        st.state = if result.is_ok() { "closed" } else { "failed" };
        st.error = result.clone().err();
    }
    if let Some((_, probe)) = a.id.split_once("@probe:") {
        l.probe_clients.lock().unwrap().remove(probe);
    }
}
pub(super) async fn probe(l: Arc<Listener>, host: std::net::IpAddr, port: u16, id: String) {
    let work = async {
        let socket = if host.is_ipv4() {
            TcpSocket::new_v4()
        } else {
            TcpSocket::new_v6()
        }
        .map_err(|e| e.to_string())?;
        let wildcard = if host.is_ipv4() {
            "0.0.0.0:0"
        } else {
            "[::]:0"
        }
        .parse()
        .unwrap();
        socket.bind(wildcard).map_err(|e| e.to_string())?;
        let mut local = socket.local_addr().map_err(|e| e.to_string())?;
        local.set_ip(host);
        l.probes.lock().unwrap().insert(local, id.clone());
        let connected = socket
            .connect((host, port).into())
            .await
            .map_err(|e| e.to_string())?;
        l.probe_clients
            .lock()
            .unwrap()
            .insert(id.clone(), connected);
        tokio::time::sleep(Duration::from_secs(10)).await;
        Ok::<(), String>(())
    };
    tokio::select! {_=l.cancel.cancelled()=>{},_=work=>{}}
    l.probes.lock().unwrap().retain(|_, v| v != &id);
    l.probe_clients.lock().unwrap().remove(&id);
    l.probe_operations.lock().unwrap().remove(&id);
}

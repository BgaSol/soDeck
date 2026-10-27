//! A private, bounded bridge from an SSH direct-tcpip channel to the next SSH
//! session. The gateway session has exactly one I/O owner and never opens a PTY.
use super::*;
use polling::{Event, Events, Poller};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};

const CAPACITY: usize = 256 * 1024;
static RESOLVERS: AtomicUsize = AtomicUsize::new(0);

pub struct Bridge {
    stop: Arc<AtomicBool>,
    gateway: TcpStream,
    local: TcpStream,
    worker: Option<thread::JoinHandle<()>>,
    pub metrics: Arc<Mutex<Value>>,
}
impl Drop for Bridge {
    fn drop(&mut self) {
        self.stop();
        if let Some(worker) = self.worker.take() { let _ = worker.join(); }
    }
}
impl Bridge {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = self.gateway.shutdown(Shutdown::Both);
        let _ = self.local.shutdown(Shutdown::Both);
    }
}

#[derive(Default)]
pub struct BridgeChain(pub Vec<Bridge>);
impl Drop for BridgeChain {
    fn drop(&mut self) {
        // Wake every worker before joining any of them. Teardown latency must
        // not grow by one network timeout per hop.
        for bridge in &self.0 { bridge.stop(); }
        while let Some(bridge) = self.0.pop() { drop(bridge); }
    }
}

pub fn check(canceled: &AtomicBool, operation: &AtomicBool, deadline: Instant) -> Result<()> {
    if canceled.load(Ordering::SeqCst) || operation.load(Ordering::SeqCst) {
        Err(CoreError::new("CANCELED", "连接已取消"))
    } else if Instant::now() >= deadline {
        Err(CoreError::new("CONNECT_TIMEOUT", "连接阶段超时"))
    } else { Ok(()) }
}
pub fn tcp(host: &str, port: u16, canceled: &AtomicBool, operation: &AtomicBool, total: Instant) -> Result<TcpStream> {
    check(canceled,operation,total)?;
    if RESOLVERS.try_update(Ordering::SeqCst,Ordering::SeqCst,|n|(n<16).then_some(n+1)).is_err() {
        return Err(CoreError::new("LIMIT_EXCEEDED", "主机解析任务已满，请稍后重试"));
    }
    let endpoint = (host.to_string(),port);
    let (tx,rx)=mpsc::sync_channel(1);
    if let Err(e)=thread::Builder::new().name("sodeck-resolve".into()).spawn(move || {
        let result=endpoint.to_socket_addrs().map(|a|a.collect::<Vec<_>>());
        RESOLVERS.fetch_sub(1,Ordering::SeqCst);
        let _=tx.send(result);
    }) { RESOLVERS.fetch_sub(1,Ordering::SeqCst);return Err(e.into()); }
    let deadline=total.min(Instant::now()+Duration::from_secs(15));
    let addresses=loop {
        check(canceled,operation,deadline)?;
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(v)=>break v.map_err(|_|CoreError::new("NETWORK_ERROR","无法解析主机名"))?,
            Err(mpsc::RecvTimeoutError::Timeout)=>{},
            _=>return Err(CoreError::new("NETWORK_ERROR","主机解析未完成")),
        }
    };
    let deadline=total.min(Instant::now()+Duration::from_secs(15));
    for address in addresses {
        let candidate=deadline.min(Instant::now()+Duration::from_secs(5));
        check(canceled,operation,deadline)?;
        let socket=socket2::Socket::new(socket2::Domain::for_address(address),socket2::Type::STREAM,Some(socket2::Protocol::TCP))?;
        socket.set_nonblocking(true)?;
        match socket.connect(&address.into()) {
            Ok(())=>{let stream:TcpStream=socket.into();stream.set_nodelay(true)?;return Ok(stream)},
            Err(e) if e.kind()==std::io::ErrorKind::WouldBlock||matches!(e.raw_os_error(),Some(115|36|10035))=>{},
            Err(_)=>continue,
        }
        let stream:TcpStream=socket.into();let poller=Poller::new()?;
        unsafe{poller.add(&stream,Event::writable(0))?;}
        let result=(||->Result<bool>{
            let mut events=Events::new();
            while Instant::now()<candidate {
                check(canceled,operation,deadline)?;
                events.clear();poller.wait(&mut events,Some(Duration::from_millis(50)))?;
                if events.iter().any(|e|e.writable) {return Ok(stream.take_error()?.is_none()&&stream.peer_addr().is_ok());}
                poller.modify(&stream,Event::writable(0))?;
            }
            Ok(false)
        })();
        let _=poller.delete(&stream);
        if result? {stream.set_nodelay(true)?;return Ok(stream)}
    }
    Err(CoreError::new("NETWORK_ERROR","SSH 连接失败，请检查地址、端口及网络"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bridge_stops_reading_at_capacity_and_keeps_pending_write_stable(){
        let mut buffer=Buffer::new();let mut source=std::io::Cursor::new(vec![7;CAPACITY*2]);
        for _ in 0..8 {assert!(buffer.receive(&mut source).unwrap().0);}
        assert_eq!(buffer.len(),CAPACITY);let position=source.position();
        assert_eq!(buffer.receive(&mut source).unwrap(),(false,false));assert_eq!(source.position(),position);
        struct Blocked;impl Write for Blocked {fn write(&mut self,_:&[u8])->std::io::Result<usize>{Err(std::io::ErrorKind::WouldBlock.into())}fn flush(&mut self)->std::io::Result<()>{Ok(())}}
        let pointer=buffer.bytes.as_ptr();assert_eq!(buffer.send(&mut Blocked).unwrap(),0);assert_eq!(buffer.bytes.as_ptr(),pointer);assert_eq!(buffer.len(),CAPACITY);
    }
    #[test]
    fn cancellation_and_total_deadline_precede_network_work(){
        let no=AtomicBool::new(false);let yes=AtomicBool::new(true);
        assert_eq!(tcp("must-not-resolve.invalid",22,&no,&yes,Instant::now()+Duration::from_secs(30)).unwrap_err().code,"CANCELED");
        assert_eq!(protocol(&no,&no,Instant::now(),||->std::result::Result<(),ssh2::Error>{panic!("expired operation performed I/O")}).unwrap_err().code,"CONNECT_TIMEOUT");
    }
}
pub fn protocol<T>(canceled:&AtomicBool,operation:&AtomicBool,total:Instant,mut call:impl FnMut()->std::result::Result<T,ssh2::Error>)->Result<T> {
    let deadline=total.min(Instant::now()+Duration::from_secs(30));
    loop {
        check(canceled,operation,deadline)?;
        match call() {
            Ok(value)=>return Ok(value),
            Err(e) if e.code()==ssh2::ErrorCode::Session(-37)=>{},
            Err(e)=>return Err(CoreError::new("SSH_ERROR",format!("SSH 协议操作失败 ({:?})",e.code()))),
        }
        thread::sleep(Duration::from_millis(4));
    }
}

fn pair()->Result<(TcpStream,TcpStream)> {
    let listener=TcpListener::bind(("127.0.0.1",0))?;
    listener.set_nonblocking(true)?;
    let client=TcpStream::connect_timeout(&listener.local_addr()?,Duration::from_secs(1))?;
    let expected=client.local_addr()?;
    let deadline=Instant::now()+Duration::from_secs(1);
    loop {
        match listener.accept() {
            Ok((server,peer)) if peer==expected && server.local_addr()?==client.peer_addr()? => {
                // No listener survives this call; the reserved client endpoint
                // must match exactly, so other local processes cannot join it.
                client.set_nonblocking(true)?;server.set_nonblocking(true)?;
                client.set_nodelay(true)?;server.set_nodelay(true)?;
                return Ok((client,server));
            },
            Ok((other,_))=>drop(other),
            Err(e) if e.kind()==std::io::ErrorKind::WouldBlock=>thread::sleep(Duration::from_millis(1)),
            Err(e)=>return Err(e.into()),
        }
        if Instant::now()>=deadline {return Err(CoreError::new("IO_ERROR","无法建立内部传输连接"));}
    }
}

pub fn forward(session:Session,gateway:TcpStream,host:&str,port:u16,canceled:&AtomicBool,operation:&AtomicBool,total:Instant,failed:Arc<AtomicBool>,failure_hop:Arc<AtomicUsize>,hop:usize)->Result<(TcpStream,Bridge)> {
    let deadline=total.min(Instant::now()+Duration::from_secs(15));
    let channel=protocol(canceled,operation,deadline,||session.channel_direct_tcpip(host,port,None)).map_err(|e|{
        if matches!(e.code.as_str(),"CANCELED"|"CONNECT_TIMEOUT") {e} else {CoreError::new("JUMP_FORWARD_FAILED","跳板无法转发到目标，请检查目标地址及 SSH 转发设置").details(json!({"cause":e.code}))}
    })?;
    let (client,local)=pair()?;
    let stop=Arc::new(AtomicBool::new(false));
    let metrics=Arc::new(Mutex::new(json!({"state":"active","upstreamBytes":0,"downstreamBytes":0,"peakBufferedBytes":0})));
    let mut bridge=Bridge{stop:stop.clone(),gateway:gateway.try_clone()?,local:local.try_clone()?,worker:None,metrics:metrics.clone()};
    bridge.worker=Some(thread::Builder::new().name("sodeck-ssh-jump".into()).spawn(move||{
        let result=pump(&session,channel,&gateway,local,&stop,&metrics);
        if !stop.load(Ordering::SeqCst){let _=failure_hop.compare_exchange(usize::MAX,hop,Ordering::SeqCst,Ordering::SeqCst);failed.store(true,Ordering::SeqCst);}
        let _=gateway.shutdown(Shutdown::Both);
        let mut m=metrics.lock().unwrap();
        m["state"]=json!(if result.is_ok()||stop.load(Ordering::SeqCst){"closed"}else{"failed"});
    })?);
    Ok((client,bridge))
}

struct Buffer { bytes:Vec<u8>, offset:usize }
impl Buffer {
    fn new()->Self{Self{bytes:Vec::with_capacity(CAPACITY),offset:0}}
    fn len(&self)->usize{self.bytes.len()-self.offset}
    fn receive(&mut self,reader:&mut impl Read)->std::io::Result<(bool,bool)> {
        if self.len()==CAPACITY {return Ok((false,false))}
        let mut chunk=[0u8;32768];let limit=chunk.len().min(CAPACITY-self.len());
        match reader.read(&mut chunk[..limit]) {
            Ok(0)=>Ok((false,true)),
            Ok(n)=>{if self.offset>0 {self.bytes.drain(..self.offset);self.offset=0;}self.bytes.extend_from_slice(&chunk[..n]);Ok((true,false))},
            Err(e) if e.kind()==std::io::ErrorKind::WouldBlock=>Ok((false,false)),
            Err(e)=>Err(e),
        }
    }
    fn send(&mut self,writer:&mut impl Write)->std::io::Result<usize> {
        if self.len()==0{return Ok(0)}
        match writer.write(&self.bytes[self.offset..]) {
            Ok(0)=>Err(std::io::ErrorKind::WriteZero.into()),
            Ok(n)=>{self.offset+=n;if self.offset==self.bytes.len(){self.bytes.clear();self.offset=0;}Ok(n)},
            Err(e) if e.kind()==std::io::ErrorKind::WouldBlock=>Ok(0),
            Err(e)=>Err(e),
        }
    }
}
fn pump(session:&Session,mut channel:Channel,gateway:&TcpStream,mut local:TcpStream,stop:&AtomicBool,metrics:&Mutex<Value>)->std::io::Result<()> {
    let poller=Poller::new()?;
    unsafe {poller.add(gateway,Event::all(0))?;poller.add(&local,Event::all(1))?;}
    let result=(||{
        let(mut up,mut down)=(Buffer::new(),Buffer::new());
        let(mut local_eof,mut remote_eof,mut sent_eof,mut shut_write)=(false,false,false,false);
        let(mut sent,mut received,mut peak)=(0u64,0u64,0usize);
        let mut events=Events::new();let mut keepalive=Instant::now();
        while !stop.load(Ordering::SeqCst) {
            // Do not change a pending libssh2 write's buffer on EAGAIN.
            let written=up.send(&mut channel)?;
            let write_direction=session.block_directions();
            let mut want_write=up.len()>0&&(written>0||matches!(write_direction,ssh2::BlockDirections::Outbound|ssh2::BlockDirections::Both));
            let mut want_read=up.len()>0&&matches!(write_direction,ssh2::BlockDirections::Inbound|ssh2::BlockDirections::Both);
            sent+=written as u64;
            received+=down.send(&mut local)? as u64;
            if !local_eof && up.len()==0 {local_eof=up.receive(&mut local)?.1;want_write|=up.len()>0;}
            if !remote_eof && down.len()<CAPACITY {
                remote_eof=down.receive(&mut channel)?.1;
                want_write|=matches!(session.block_directions(),ssh2::BlockDirections::Outbound|ssh2::BlockDirections::Both);
                want_read|=!remote_eof&&down.len()<CAPACITY;
            }
            if local_eof && up.len()==0 && !sent_eof {
                match channel.send_eof(){Ok(())=>sent_eof=true,Err(e) if e.code()==ssh2::ErrorCode::Session(-37)=>{
                    want_read|=matches!(session.block_directions(),ssh2::BlockDirections::Inbound|ssh2::BlockDirections::Both);
                    want_write|=matches!(session.block_directions(),ssh2::BlockDirections::Outbound|ssh2::BlockDirections::Both);
                },Err(_)=>break}
            }
            if remote_eof && down.len()==0 && !shut_write {local.shutdown(Shutdown::Write)?;shut_write=true;}
            if local_eof&&remote_eof&&up.len()==0&&down.len()==0 {break}
            peak=peak.max(up.len().max(down.len()));
            *metrics.lock().unwrap()=json!({"state":"active","upstreamBytes":sent,"downstreamBytes":received,"peakBufferedBytes":peak});
            if keepalive.elapsed()>=Duration::from_secs(15){let _=session.keepalive_send();keepalive=Instant::now();}
            poller.modify(gateway,Event::new(0,want_read,want_write))?;
            poller.modify(&local,Event::new(1,!local_eof&&up.len()==0,down.len()>0))?;
            events.clear();poller.wait(&mut events,Some(Duration::from_millis(100)))?;
        }
        Ok(())
    })();
    let _=poller.delete(gateway);let _=poller.delete(&local);let _=local.shutdown(Shutdown::Both);
    result
}

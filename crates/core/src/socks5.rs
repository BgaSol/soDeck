//! SOCKS5 client handshake only. The returned socket belongs to the target SSH.
use crate::{CoreError, Result};
use std::{io::{Read, Write}, net::{IpAddr, TcpStream}, sync::atomic::{AtomicBool, Ordering}, time::{Duration, Instant}};
use zeroize::Zeroizing;

pub enum Auth<'a> { None, Password { username: &'a str, password: &'a str } }

fn check(cancel: &AtomicBool, operation: &AtomicBool, deadline: Instant) -> Result<()> {
    if cancel.load(Ordering::SeqCst) || operation.load(Ordering::SeqCst) {
        Err(CoreError::new("CANCELED", "连接已取消"))
    } else if Instant::now() >= deadline {
        Err(CoreError::new("PROXY_TIMEOUT", "SOCKS5 协商或目标连接超时"))
    } else { Ok(()) }
}

fn transfer(stream: &mut TcpStream, bytes: &mut [u8], write: bool, cancel: &AtomicBool, operation: &AtomicBool, deadline: Instant, meter: &mut impl FnMut(bool, u64)) -> Result<()> {
    let mut offset = 0;
    while offset < bytes.len() {
        check(cancel, operation, deadline)?;
        let result = if write { stream.write(&bytes[offset..]) } else { stream.read(&mut bytes[offset..]) };
        match result {
            Ok(0) => return Err(transfer_error("PROXY_DISCONNECTED", write, offset, bytes.len(), None)),
            Ok(n) => { offset += n; meter(write, n as u64); },
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted) => std::thread::sleep(Duration::from_millis(4)),
            Err(e) => return Err(transfer_error("PROXY_NETWORK_ERROR", write, offset, bytes.len(), e.raw_os_error())),
        }
    }
    Ok(())
}

fn transfer_error(code:&str,write:bool,received:usize,expected:usize,os:Option<i32>)->CoreError {
    let mut details=serde_json::json!({"operation":if write{"write"}else{"read"}});
    if !write {details["receivedBytes"]=serde_json::json!(received);details["expectedBytes"]=serde_json::json!(expected);}
    if let Some(code)=os {details["osErrorCode"]=serde_json::json!(code);}
    CoreError::new(code,if write{"发送 SOCKS5 请求时通道关闭或中断"}else{"读取 SOCKS5 回复时通道关闭或中断"}).details(details)
}

pub fn target(host: &str, port: u16) -> Result<Vec<u8>> {
    if port == 0 { return Err(CoreError::new("INVALID_PARAMS", "目标端口无效")); }
    let mut bytes = vec![5, 1, 0];
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) => { bytes.push(1); bytes.extend(ip.octets()); }
        Ok(IpAddr::V6(ip)) => { bytes.push(4); bytes.extend(ip.octets()); }
        Err(_) => {
            let name = idna::domain_to_ascii_strict(host).map_err(|_|CoreError::new("INVALID_PARAMS", "目标域名无效"))?;
            if name.is_empty() || name.len() > 255 { return Err(CoreError::new("INVALID_PARAMS", "SOCKS5 目标域名必须为 1–255 字节")); }
            bytes.extend([3, name.len() as u8]); bytes.extend(name.as_bytes());
        }
    }
    bytes.extend(port.to_be_bytes());
    Ok(bytes)
}

#[cfg(test)]
pub fn connect(stream: TcpStream, host: &str, port: u16, auth: Auth<'_>, cancel: &AtomicBool, operation: &AtomicBool, total: Instant, mut progress: impl FnMut(&str)) -> Result<TcpStream> {
    connect_metered(stream,host,port,auth,cancel,operation,total,|stage,done|{if !done{progress(stage)}},|_,_|{})
}

pub fn connect_metered(mut stream: TcpStream, host: &str, port: u16, auth: Auth<'_>, cancel: &AtomicBool, operation: &AtomicBool, total: Instant, mut progress: impl FnMut(&'static str,bool), mut meter: impl FnMut(bool, u64)) -> Result<TcpStream> {
    let mut request = target(host, port)?;
    let mut credential = Zeroizing::new(Vec::new());
    let method = match auth {
        Auth::None => 0,
        Auth::Password { username, password } => {
            if !(1..=255).contains(&username.len()) || !(1..=255).contains(&password.len()) { return Err(CoreError::new("INVALID_PARAMS", "SOCKS5 账号与密码须为 1–255 字节")); }
            credential.extend([1, username.len() as u8]); credential.extend(username.as_bytes());
            credential.push(password.len() as u8); credential.extend(password.as_bytes()); 2
        }
    };
    stream.set_nonblocking(true)?;
    let deadline = total.min(Instant::now() + Duration::from_secs(15));
    progress("proxyNegotiate",false);
    transfer(&mut stream, &mut [5, 1, method], true, cancel, operation, deadline, &mut meter)?;
    let mut response = [0; 2];
    transfer(&mut stream, &mut response, false, cancel, operation, deadline, &mut meter)?;
    if response[0] != 5 { return Err(CoreError::new("PROXY_PROTOCOL_ERROR", "代理返回了无效的 SOCKS5 版本")); }
    if response[1] != method { return Err(CoreError::new("PROXY_AUTH_METHOD", "网络代理不接受配置的认证方式")); }
    progress("proxyNegotiate",true);
    if method == 2 {
        progress("proxyAuthenticate",false);
        transfer(&mut stream, &mut credential, true, cancel, operation, deadline, &mut meter)?;
        transfer(&mut stream, &mut response, false, cancel, operation, deadline, &mut meter)?;
        if response[0] != 1 { return Err(CoreError::new("PROXY_PROTOCOL_ERROR", "代理认证回复版本无效")); }
        if response[1] != 0 { return Err(CoreError::new("PROXY_AUTH_FAILED", "网络代理账号或密码错误")); }
    }
    if method==2 {progress("proxyAuthenticate",true);}
    progress("proxyConnect",false);
    let deadline = total.min(Instant::now() + Duration::from_secs(15));
    transfer(&mut stream, &mut request, true, cancel, operation, deadline, &mut meter)?;
    let mut header = [0; 4];
    transfer(&mut stream, &mut header, false, cancel, operation, deadline, &mut meter)?;
    if header[0] != 5 || header[2] != 0 { return Err(CoreError::new("PROXY_PROTOCOL_ERROR", "SOCKS5 CONNECT 回复格式无效")); }
    if header[1]>8 { return Err(CoreError::new("PROXY_PROTOCOL_ERROR", "SOCKS5 CONNECT 回复状态无效")); }
    let length = match header[3] {
        1 => 4, 4 => 16,
        3 => { let mut size = [0]; transfer(&mut stream, &mut size, false, cancel, operation, deadline, &mut meter)?; if size[0] == 0 { return Err(CoreError::new("PROXY_PROTOCOL_ERROR", "代理回复地址为空")); } size[0] as usize },
        _ => return Err(CoreError::new("PROXY_PROTOCOL_ERROR", "代理回复地址类型无效")),
    };
    // Read exactly the remaining reply; SSH may already have sent its banner.
    let mut bound = [0; 257];
    transfer(&mut stream, &mut bound[..length + 2], false, cancel, operation, deadline, &mut meter)?;
    if header[1] != 0 {
        let message = match header[1] {1=>"代理服务失败",2=>"代理规则拒绝连接",3=>"代理无法到达目标网络",4=>"代理无法到达目标主机",5=>"目标拒绝代理连接",6=>"代理连接超时",7=>"代理不支持 CONNECT",8=>"代理不支持目标地址类型",_=>"代理返回未知错误"};
        return Err(CoreError::new("PROXY_CONNECT_FAILED", message).details(serde_json::json!({"reply":header[1]})));
    }
    check(cancel, operation, total)?;
    progress("proxyConnect",true);
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{net::TcpListener, sync::Arc, thread};
    fn fake(reply: Vec<u8>, auth: bool) -> (TcpStream, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let socket = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let worker = thread::spawn(move || {
            let (mut peer, _) = listener.accept().unwrap(); peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            let mut hello = [0;3]; peer.read_exact(&mut hello).unwrap();
            peer.write_all(&[5, if auth { 0 } else { hello[2] }]).unwrap();
            if auth { return; }
            let mut request = [0;4]; peer.read_exact(&mut request).unwrap();
            let size = if request[3] == 3 { let mut n=[0]; peer.read_exact(&mut n).unwrap();n[0] as usize } else if request[3] == 1 {4} else {16};
            let mut rest=vec![0;size+2];peer.read_exact(&mut rest).unwrap();
            peer.write_all(&reply).unwrap();
        }); (socket,worker)
    }
    #[test]
    fn exact_reply_preserves_following_ssh_banner() {
        let (socket,worker)=fake([vec![5,0,0,1,0,0,0,0,0,0],b"SSH-2.0-fixture\r\n".to_vec()].concat(),false);
        let no=AtomicBool::new(false);
        let mut socket=connect(socket,"target.internal",22,Auth::None,&no,&no,Instant::now()+Duration::from_secs(2), |_|{}).unwrap();
        socket.set_nonblocking(false).unwrap();let mut banner=String::new();socket.read_to_string(&mut banner).unwrap();assert_eq!(banner,"SSH-2.0-fixture\r\n");worker.join().unwrap();
    }
    #[test]
    fn rejects_auth_downgrade_and_maps_reply() {
        let no=AtomicBool::new(false);
        let (socket,worker)=fake(vec![],true);
        assert_eq!(connect(socket,"example.test",22,Auth::Password{username:"user",password:"password"},&no,&no,Instant::now()+Duration::from_secs(2), |_|{}).unwrap_err().code,"PROXY_AUTH_METHOD");worker.join().unwrap();
        let (socket,worker)=fake(vec![5,4,0,1,0,0,0,0,0,0],false);
        let error=connect(socket,"example.test",22,Auth::None,&no,&no,Instant::now()+Duration::from_secs(2), |_|{}).unwrap_err();assert_eq!(error.code,"PROXY_CONNECT_FAILED");assert_eq!(error.details.unwrap()["reply"],4);worker.join().unwrap();
    }
    #[test]
    fn cancel_and_deadline_interrupt_silent_proxy() {
        for canceled in [true,false] {
            let listener=TcpListener::bind("127.0.0.1:0").unwrap();let socket=TcpStream::connect(listener.local_addr().unwrap()).unwrap();let (_peer,_)=listener.accept().unwrap();
            let flag=Arc::new(AtomicBool::new(false));let signal=flag.clone();
            let worker=thread::spawn(move||{thread::sleep(Duration::from_millis(30));if canceled{signal.store(true,Ordering::SeqCst);}});
            let start=Instant::now();let error=connect(socket,"target.internal",22,Auth::None,&flag,&AtomicBool::new(false),start+Duration::from_millis(100), |_|{}).unwrap_err();
            assert_eq!(error.code,if canceled{"CANCELED"}else{"PROXY_TIMEOUT"});assert!(start.elapsed()<Duration::from_secs(1));worker.join().unwrap();
        }
    }
    #[test]
    fn address_encoding_is_bounded() {
        assert_eq!(target("127.0.0.1",22).unwrap(),vec![5,1,0,1,127,0,0,1,0,22]);
        assert_eq!(target("::1",22).unwrap().len(),22);
        assert!(target(&"a".repeat(256),22).is_err());assert!(target("host",0).is_err());
    }
    #[test]
    fn malformed_and_truncated_replies_never_hand_off_to_ssh() {
        let no=AtomicBool::new(false);
        for (reply,code) in [(vec![4,0,0,1],"PROXY_PROTOCOL_ERROR"),(vec![5,0,1,1],"PROXY_PROTOCOL_ERROR"),(vec![5,0,0,9],"PROXY_PROTOCOL_ERROR"),(vec![5,9,0,1],"PROXY_PROTOCOL_ERROR"),(vec![5,0,0,3,0],"PROXY_PROTOCOL_ERROR"),(vec![5,0,0,4,0],"PROXY_DISCONNECTED")] {
            let (socket,worker)=fake(reply,false);
            assert_eq!(connect(socket,"host.internal",22,Auth::None,&no,&no,Instant::now()+Duration::from_secs(2),|_|{}).unwrap_err().code,code);worker.join().unwrap();
        }
        for rep in 1..=8 {let (socket,worker)=fake(vec![5,rep,0,1,0,0,0,0,0,0],false);let e=connect(socket,"host.internal",22,Auth::None,&no,&no,Instant::now()+Duration::from_secs(2),|_|{}).unwrap_err();assert_eq!(e.code,"PROXY_CONNECT_FAILED");assert_eq!(e.details.unwrap()["reply"],rep);worker.join().unwrap();}
    }
    #[test]
    fn outer_meter_counts_only_protocol_and_cancels_after_connect() {
        let no=AtomicBool::new(false);let mut sent=0;let mut received=0;
        let (socket,worker)=fake([vec![5,0,0,4],vec![0;18],b"SSH-banner".to_vec()].concat(),false);
        let socket=connect_metered(socket,"::1",22,Auth::None,&no,&no,Instant::now()+Duration::from_secs(2),|_,_|{},|write,n|if write{sent+=n}else{received+=n}).unwrap();
        assert_eq!((sent,received),(25,24));drop(socket);worker.join().unwrap();
        let cancel=AtomicBool::new(false);let(socket,worker)=fake(vec![5,0,0,1,0,0,0,0,0,0],false);
        let error=connect_metered(socket,"127.0.0.1",22,Auth::None,&cancel,&no,Instant::now()+Duration::from_secs(2),|_,_|{},|write,n|{if !write&&n==6{cancel.store(true,Ordering::SeqCst);}}).unwrap_err();
        assert_eq!(error.code,"CANCELED");worker.join().unwrap();
    }
    #[test]
    #[ignore="Requires isolated Python/Paramiko SOCKS fixture"]
    fn real_socks_to_nonblocking_ssh() {
        use std::{io::{BufRead,BufReader},process::{Command,Stdio}};
        let root=tempfile::tempdir().unwrap();
        let mut child=Command::new(std::env::var("SODECK_TEST_PYTHON").unwrap()).args(["-u",concat!(env!("CARGO_MANIFEST_DIR"),"/../../tests/core/socks_route_fixture.py")]).arg(root.path()).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
        struct Stop(std::process::Child);impl Drop for Stop{fn drop(&mut self){drop(self.0.stdin.take());let _=self.0.wait();}}
        let mut line=String::new();BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).unwrap();let info:serde_json::Value=serde_json::from_str(&line).unwrap();let _stop=Stop(child);
        let no=AtomicBool::new(false);
        for (field,auth) in [("port",Auth::None),("authPort",Auth::Password{username:"proxy-user",password:"proxy-password"})] {
            let socket=TcpStream::connect(("127.0.0.1",info[field].as_u64().unwrap() as u16)).unwrap();
            let socket=connect(socket,"target.internal",22,auth,&no,&no,Instant::now()+Duration::from_secs(5), |_|{}).unwrap();
            let mut session=ssh2::Session::new().unwrap();session.set_tcp_stream(socket);session.set_blocking(false);
            let deadline=Instant::now()+Duration::from_secs(5);
            for action in [0,1] {loop{let result=if action==0{session.handshake()}else{session.userauth_password("test-user","isolated-test-password")};match result{Ok(())=>break,Err(e) if e.code()==ssh2::ErrorCode::Session(-37)=>{assert!(Instant::now()<deadline);thread::sleep(Duration::from_millis(4));},Err(e)=>panic!("{e}")}}}
            assert!(session.authenticated());assert!(session.host_key().is_some());
        }
    }
}

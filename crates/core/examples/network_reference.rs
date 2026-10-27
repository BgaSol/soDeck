//! Test-only one-hop throughput baseline: native TCP -> identical ssh2 library.
//! Restricted to loopback fixture input/output; no plugin or core overhead.
use std::{io::{Read,Write},net::{TcpListener,TcpStream,Shutdown},path::Path};
fn main()->Result<(),Box<dyn std::error::Error>> {
    let args:Vec<_>=std::env::args().collect();
    let port:u16=args[1].parse()?;let username=&args[2];let key=Path::new(&args[3]);let target:u16=args[4].parse()?;
    let mut ssh=ssh2::Session::new()?;ssh.set_tcp_stream(TcpStream::connect(("127.0.0.1",port))?);ssh.set_timeout(90000);ssh.handshake()?;ssh.userauth_pubkey_file(username,None,key,None)?;
    let listener=TcpListener::bind(("127.0.0.1",0))?;
    println!("{}",serde_json::json!({"port":listener.local_addr()?.port(),"cipher":ssh.methods(ssh2::MethodType::CryptSc),"library":"workspace ssh2 / vendored OpenSSL","profile":"debug"}));std::io::stdout().flush()?;
    for socket in listener.incoming() {
        let mut socket=socket?;socket.set_nodelay(true)?;socket.set_read_timeout(Some(std::time::Duration::from_secs(10)))?;
        let mut head=Vec::new();let mut byte=[0];while head.len()<65536&&!head.ends_with(b"\r\n\r\n"){if socket.read(&mut byte)?==0{break;}head.push(byte[0]);}
        if head.is_empty(){continue;}
        // The fixture only sends bounded HTTP GET headers, then a finite body.
        if !head.starts_with(b"GET /large HTTP/"){return Err("unexpected benchmark request".into());}
        let mut channel=ssh.channel_direct_tcpip("127.0.0.1",target,Some(("127.0.0.1",0)))?;channel.write_all(&head)?;channel.send_eof()?;
        let mut data=[0;65536];loop{let n=channel.read(&mut data)?;if n==0{break;}socket.write_all(&data[..n])?;}
        let _=socket.shutdown(Shutdown::Both);let _=channel.close();
    }
    Ok(())
}

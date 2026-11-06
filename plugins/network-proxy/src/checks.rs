use crate::socks5;
use serde_json::{json, Value};
use socker::socks5::{
    Address, AuthenticationMethod, ClientGreeting, CommandType, Request, Response, ServerChoice,
    VERSION,
};
use sodeck_plugin_client::Client;
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    time::{Duration, Instant},
};
pub fn check(
    c: &Client,
    listener: &str,
    snapshot: &Value,
    rule: &Value,
    target: &Value,
) -> Result<Value, String> {
    let began = Instant::now();
    if rule["type"] == "tcpForward" {
        let probe = format!(
            "check-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        c.call(
            "net.listener.probe",
            json!({"listenerId":listener,"operationId":probe}),
        )?;
        while began.elapsed() < Duration::from_secs(11) {
            let s = c.call("net.listener.snapshot", json!({"listenerId":listener}))?;
            if s["state"] != "running" {
                return Err("隧道已停止，检查未完成".into());
            }
            if let Some(client) = s["clients"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|v| v["probeId"] == probe)
            {
                match client["target"]["state"].as_str() {
                    // Wait until the plugin has handed off both streams before
                    // closing the probe, so cleanup cannot race its splice.
                    Some("relaying" | "closed") => {
                        let _ = c.call("net.close", json!({"id":client["id"]}));
                        return Ok(
                            json!({"ok":true,"message":"目标 TCP 可达","listenerId":listener}),
                        );
                    }
                    Some("failed") => {
                        let _ = c.call("net.close", json!({"id":client["id"]}));
                        return Err(client["target"]["error"]
                            .as_str()
                            .unwrap_or("目标不可达")
                            .into());
                    }
                    _ => {}
                }
            }
            std::thread::sleep(Duration::from_millis(40));
        }
        return Err("实际入口检查超时".into());
    }
    let mut ip = snapshot["address"]["host"]
        .as_str()
        .ok_or("入口未就绪")?
        .parse::<std::net::IpAddr>()
        .map_err(|_| "入口无效")?;
    if ip.is_unspecified() {
        ip = if ip.is_ipv4() {
            "127.0.0.1".parse().unwrap()
        } else {
            "::1".parse().unwrap()
        }
    }
    let port = snapshot["address"]["port"].as_u64().ok_or("入口未就绪")? as u16;
    let host = target["host"]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 253)
        .ok_or("请输入检查目标主机")?;
    let target_port = target["port"]
        .as_u64()
        .filter(|p| *p > 0 && *p <= 65535)
        .ok_or("请输入检查目标端口")? as u16;
    let mut socket = TcpStream::connect_timeout(&SocketAddr::new(ip, port), Duration::from_secs(2))
        .map_err(|e| e.to_string())?;
    socket
        .set_write_timeout(Some(Duration::from_secs(2)))
        .map_err(|e| e.to_string())?;
    negotiate(&mut socket, host, target_port, Duration::from_secs(12))?;
    Ok(json!({"ok":true,"message":"SOCKS5 入口与目标 TCP 可达","listenerId":listener}))
}
fn read_frame(
    socket: &mut TcpStream,
    bytes: &mut Vec<u8>,
    total: usize,
    deadline: Instant,
) -> Result<(), String> {
    let mut offset = bytes.len();
    bytes.resize(total, 0);
    while offset < total {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
            .ok_or("SOCKS5 检查超时")?;
        socket
            .set_read_timeout(Some(remaining))
            .map_err(|e| e.to_string())?;
        match socket.read(&mut bytes[offset..]) {
            Ok(0) => return Err("SOCKS5 响应提前结束".into()),
            Ok(n) => offset += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) =>
            {
                return Err("SOCKS5 检查超时".into())
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn serve(reply: Vec<u8>, interval: Duration) -> (TcpStream, std::thread::JoinHandle<()>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let socket = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let task = std::thread::spawn(move || {
            let (mut peer, _) = listener.accept().unwrap();
            let mut greeting = [0; 3];
            peer.read_exact(&mut greeting).unwrap();
            assert_eq!(greeting, [5, 1, 0]);
            peer.write_all(&[5, 0]).unwrap();
            let mut request = [0; 12];
            peer.read_exact(&mut request).unwrap();
            assert_eq!(&request, b"\x05\x01\x00\x03\x05probe\x00\x50");
            for byte in reply {
                if !interval.is_zero() {
                    std::thread::sleep(interval);
                }
                if peer.write_all(&[byte]).is_err() {
                    break;
                }
            }
        });
        (socket, task)
    }
    #[test]
    fn reads_variable_binding_frames_without_consuming_body() {
        for frame in [
            vec![5, 0, 0, 1, 0, 0, 0, 0, 0, 0],
            [vec![5, 0, 0, 4], vec![0; 18]].concat(),
            vec![5, 0, 0, 3, 3, b'b', b'n', b'd', 0, 0],
        ] {
            let (mut socket, task) = serve([frame, b"tail".to_vec()].concat(), Duration::ZERO);
            negotiate(&mut socket, "probe", 80, Duration::from_secs(1)).unwrap();
            let mut tail = [0; 4];
            socket.read_exact(&mut tail).unwrap();
            assert_eq!(&tail, b"tail");
            task.join().unwrap();
        }
    }
    #[test]
    fn rejects_malformed_refused_and_truncated_responses() {
        for frame in [
            vec![4, 0, 0, 1],
            vec![5, 0, 1, 1],
            vec![5, 0, 0, 9],
            vec![5, 0, 0, 4, 0],
            vec![5, 5, 0, 1],
        ] {
            let (mut socket, task) = serve(frame.clone(), Duration::ZERO);
            let error = negotiate(&mut socket, "probe", 80, Duration::from_secs(1)).unwrap_err();
            if frame[1] == 5 {
                assert!(error.contains("状态 5"));
            }
            task.join().unwrap();
        }
    }
    #[test]
    fn drip_feed_does_not_reset_the_logical_response_deadline() {
        let (mut socket, task) = serve(
            [vec![5, 0, 0, 4], vec![0; 18]].concat(),
            Duration::from_millis(30),
        );
        let start = Instant::now();
        let error = negotiate(&mut socket, "probe", 80, Duration::from_millis(180)).unwrap_err();
        assert!(error.contains("超时"));
        assert!(
            start.elapsed() < Duration::from_millis(450),
            "deadline reset on partial read"
        );
        drop(socket);
        task.join().unwrap();
    }
}
fn negotiate(
    socket: &mut TcpStream,
    host: &str,
    port: u16,
    timeout: Duration,
) -> Result<(), String> {
    let method = AuthenticationMethod::NO_AUTHENTICATION;
    socket
        .write_all(&socks5::encode(ClientGreeting {
            methods: vec![method].into_boxed_slice(),
        })?)
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    read_frame(socket, &mut bytes, 2, Instant::now() + timeout)?;
    if socks5::decode::<ServerChoice>(&bytes)?.method != method {
        return Err("SOCKS5 协商失败".into());
    }
    let address = host
        .parse::<std::net::IpAddr>()
        .map(Address::from)
        .unwrap_or_else(|_| Address::from(host));
    socket
        .write_all(&socks5::encode(Request {
            command: CommandType::CONNECT,
            address,
            port,
        })?)
        .map_err(|e| e.to_string())?;
    bytes.clear();
    let deadline = Instant::now() + timeout;
    read_frame(socket, &mut bytes, 4, deadline)?;
    if bytes[0] != VERSION || bytes[2] != 0 {
        return Err("SOCKS5 代理响应无效".into());
    }
    if bytes[1] != 0 {
        return Err(format!("目标连接失败，SOCKS5 状态 {}", bytes[1]));
    }
    let first = socks5::frame_len(&bytes)?;
    read_frame(socket, &mut bytes, first, deadline)?;
    let total = socks5::frame_len(&bytes)?;
    read_frame(socket, &mut bytes, total, deadline)?;
    socks5::decode::<Response>(&bytes)?;
    Ok(())
}

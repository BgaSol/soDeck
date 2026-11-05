use serde_json::{json, Value};
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
        .set_read_timeout(Some(Duration::from_secs(12)))
        .map_err(|e| e.to_string())?;
    socket
        .set_write_timeout(Some(Duration::from_secs(2)))
        .map_err(|e| e.to_string())?;
    socket.write_all(&[5, 1, 0]).map_err(|e| e.to_string())?;
    let mut method = [0; 2];
    socket.read_exact(&mut method).map_err(|e| e.to_string())?;
    if method != [5, 0] {
        return Err("SOCKS5 协商失败".into());
    }
    let mut request = vec![5, 1, 0, 3, host.len() as u8];
    request.extend_from_slice(host.as_bytes());
    request.extend_from_slice(&target_port.to_be_bytes());
    socket.write_all(&request).map_err(|e| e.to_string())?;
    let mut reply = [0; 10];
    socket.read_exact(&mut reply).map_err(|e| e.to_string())?;
    if reply[0] != 5 || reply[1] != 0 {
        return Err(format!("目标连接失败，SOCKS5 状态 {}", reply[1]));
    }
    Ok(json!({"ok":true,"message":"SOCKS5 入口与目标 TCP 可达","listenerId":listener}))
}

use super::{validate_target, ProbeProtocol, ProbeRequest, ProbeResult, Route};
use crate::model::{AppError, AppResult};
use std::{
    collections::HashMap,
    io,
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::TcpSocket,
    sync::watch,
};

#[derive(Default)]
pub struct Probes {
    pending: HashMap<(SocketAddr, SocketAddr), String>,
    active: Option<(String, watch::Sender<bool>)>,
}
impl Probes {
    pub fn consume(&mut self, peer: SocketAddr, local: SocketAddr) -> bool {
        self.pending
            .remove(&(peer, local))
            .is_some_and(|id| self.active.as_ref().is_some_and(|(a, _)| *a == id))
    }
    pub fn cancel(&mut self, id: &str) {
        if let Some((_, cancel)) = self.active.as_ref().filter(|(a, _)| a == id) {
            cancel.send_replace(true);
        }
    }
}
struct Guard {
    route: Arc<Route>,
    id: String,
}
impl Drop for Guard {
    fn drop(&mut self) {
        let mut p = self.route.probes.lock().unwrap();
        p.pending.retain(|_, id| *id != self.id);
        if p.active.as_ref().is_some_and(|(id, _)| *id == self.id) {
            p.active = None;
        }
        let _ = self.route.wake.notify();
    }
}
impl Route {
    pub fn start_probe(
        self: &Arc<Self>,
        id: String,
        run_id: String,
        address: SocketAddr,
        mut request: ProbeRequest,
    ) -> AppResult<()> {
        request.target.host = validate_target(&request.target.host, request.target.port)?.host;
        let mut probes = self.probes.lock().unwrap();
        if probes.active.is_some() || self.closed() {
            return Err(AppError::new("busy", "请等待当前代理测试结束"));
        }
        let (cancel, mut cancelled) = watch::channel(false);
        probes.active = Some((id.clone(), cancel));
        drop(probes);
        let route = self.clone();
        self.spawn(async move {
            let _guard = Guard { route: route.clone(), id: id.clone() };
            let started_at = crate::traffic::now();
            let result = tokio::select! {
                _ = cancelled.changed() => return,
                result = tokio::time::timeout(Duration::from_secs(15), async {
                    let socket = if address.is_ipv6() { TcpSocket::new_v6()? } else { TcpSocket::new_v4()? };
                    socket.bind(SocketAddr::new(address.ip(), 0))?;
                    let local = socket.local_addr()?;
                    route.probes.lock().unwrap().pending.insert((local, address), id.clone());
                    let mut socket = socket.connect(address).await?;
                    handshake(&mut socket, &request).await?;
                    let _ = socket.shutdown().await;
                    Ok::<(), io::Error>(())
                }) => result,
            };
            if *cancelled.borrow() || route.closed() { return; }
            let success = matches!(result, Ok(Ok(())));
            route.metrics.lock().unwrap().last_probe = Some(ProbeResult { probe_id: id, run_id, protocol: request.protocol, target: request.target, started_at, finished_at: crate::traffic::now(), success, stage: "channelOpen".into(), message: if success { "代理握手与目标 TCP 通路已通过；未验证网页、证书或登录。" } else { "代理测试未通过，请检查目标、当前 SSH 路径及转发权限。" }.into() });
        });
        Ok(())
    }
    pub fn cancel_probe(&self, id: &str) {
        self.probes.lock().unwrap().cancel(id);
    }
}
async fn handshake<T: AsyncRead + AsyncWrite + Unpin>(
    socket: &mut T,
    request: &ProbeRequest,
) -> io::Result<()> {
    match request.protocol {
        ProbeProtocol::HttpConnect => {
            let host = if request.target.host.contains(':') {
                format!("[{}]", request.target.host)
            } else {
                request.target.host.clone()
            };
            let authority = format!("{host}:{}", request.target.port);
            socket
                .write_all(
                    format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n\r\n").as_bytes(),
                )
                .await?;
            let mut head = Vec::new();
            while !head.ends_with(b"\r\n\r\n") {
                if head.len() >= 16 * 1024 {
                    return Err(io::ErrorKind::InvalidData.into());
                }
                head.push(socket.read_u8().await?);
            }
            if !http_connect_succeeded(&head) {
                return Err(io::ErrorKind::ConnectionRefused.into());
            }
        }
        ProbeProtocol::Socks5 => {
            socket.write_all(&[5, 1, 0]).await?;
            let mut method = [0; 2];
            socket.read_exact(&mut method).await?;
            if method != [5, 0] {
                return Err(io::ErrorKind::InvalidData.into());
            }
            let mut req = vec![5, 1, 0];
            match request.target.host.parse::<IpAddr>() {
                Ok(IpAddr::V4(ip)) => {
                    req.push(1);
                    req.extend(ip.octets());
                }
                Ok(IpAddr::V6(ip)) => {
                    req.push(4);
                    req.extend(ip.octets());
                }
                Err(_) => {
                    req.extend([3, request.target.host.len() as u8]);
                    req.extend(request.target.host.as_bytes());
                }
            }
            req.extend(request.target.port.to_be_bytes());
            socket.write_all(&req).await?;
            read_socks5_response(socket).await?;
        }
    }
    Ok(())
}

async fn read_socks5_response<T: AsyncRead + Unpin>(socket: &mut T) -> io::Result<()> {
    let mut response = [0; 4];
    socket.read_exact(&mut response).await?;
    if response[0] != 5 || response[1] != 0 || response[2] != 0 {
        return Err(io::ErrorKind::ConnectionRefused.into());
    }
    match response[3] {
        1 => {
            let mut address = [0; 4];
            socket.read_exact(&mut address).await?;
        }
        3 => {
            let length = socket.read_u8().await? as usize;
            let mut address = vec![0; length];
            socket.read_exact(&mut address).await?;
        }
        4 => {
            let mut address = [0; 16];
            socket.read_exact(&mut address).await?;
        }
        _ => return Err(io::ErrorKind::InvalidData.into()),
    }
    let _ = socket.read_u16().await?;
    Ok(())
}

fn http_connect_succeeded(head: &[u8]) -> bool {
    let line = head
        .split(|byte| *byte == b'\r' || *byte == b'\n')
        .next()
        .unwrap_or_default();
    (line.starts_with(b"HTTP/1.0") || line.starts_with(b"HTTP/1.1"))
        && line.get(8) == Some(&b' ')
        && line.get(9..12) == Some(b"200")
        && line
            .get(12)
            .map_or(true, |byte| *byte == b' ' || *byte == b'\t')
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{duplex, AsyncWriteExt};

    #[test]
    fn accepts_http_connect_success_without_reason_phrase() {
        assert!(http_connect_succeeded(b"HTTP/1.1 200\r\n\r\n"));
        assert!(http_connect_succeeded(
            b"HTTP/1.0 200 Connection Established\r\n\r\n"
        ));
        assert!(!http_connect_succeeded(b"HTTP/1.1 204 No Content\r\n\r\n"));
        assert!(!http_connect_succeeded(b"HTTP/2 200 OK\r\n\r\n"));
    }

    #[test]
    fn reads_all_socks5_bound_address_types() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            for response in [
                vec![5, 0, 0, 1, 127, 0, 0, 1, 0x10, 0xE1],
                vec![
                    5, 0, 0, 3, 8, b'i', b'n', b't', b'e', b'r', b'n', b'a', b'l', 0x10, 0xE1,
                ],
                vec![
                    5, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0x10, 0xE1,
                ],
            ] {
                let (mut client, mut server) = duplex(128);
                server.write_all(&response).await.unwrap();
                read_socks5_response(&mut client).await.unwrap();
            }
        });
    }
}

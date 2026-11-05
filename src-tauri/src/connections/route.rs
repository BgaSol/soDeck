use super::{
    host_keys::HostKeys,
    io::{Socket, Stream},
    ssh, Attempt, Error,
};
use crate::servers::connection_profile::{Auth, Profile};
use std::{sync::Arc, time::Duration};
use tokio::net::TcpStream;
use tokio_util::sync::CancellationToken;

pub(crate) struct Session {
    pub(super) chain: Vec<russh::client::Handle<ssh::Client>>,
    cancel: CancellationToken,
    pub(crate) path: serde_json::Value,
}
impl Session {
    pub(crate) fn cancellation(&self) -> CancellationToken {
        self.cancel.clone()
    }
    pub(crate) async fn open_channel(&self) -> Result<russh::Channel<russh::client::Msg>, String> {
        if self.is_closed() {
            return Err("服务器连接已断开".into());
        }
        self.chain
            .last()
            .ok_or("SSH 会话不存在")?
            .channel_open_session()
            .await
            .map_err(|e| e.to_string())
    }
    pub(crate) async fn open_tcp(&self, host: &str, port: u16) -> Result<Stream, String> {
        if self.is_closed() {
            return Err("服务器连接已断开".into());
        }
        self.chain
            .last()
            .ok_or("SSH 会话不存在")?
            .channel_open_direct_tcpip(host, port as u32, "127.0.0.1", 0)
            .await
            .map(|channel| Box::new(channel.into_stream()) as Stream)
            .map_err(|error| error.to_string())
    }
    pub fn is_closed(&self) -> bool {
        self.cancel.is_cancelled() || self.chain.iter().any(|s| s.is_closed())
    }
    pub async fn close(&self) {
        // Bounded best effort; socket cancellation is the unconditional cleanup path.
        for s in self.chain.iter().rev() {
            let _ = tokio::time::timeout(
                Duration::from_millis(200),
                s.disconnect(russh::Disconnect::ByApplication, "SoDeck closed", "en"),
            )
            .await;
        }
        self.cancel.cancel();
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

pub(super) async fn connect(
    profile: &mut Profile,
    a: Arc<Attempt>,
    keys: Arc<HostKeys>,
) -> Result<Session, Error> {
    // Complete missing credentials without putting secrets in a projection or event.
    for (index, node) in profile.nodes.iter_mut().enumerate() {
        a.credentials(node, index).await?;
    }
    let first = &profile.nodes[0];
    let addresses = a
        .wait("dns", 0, async {
            let addresses = tokio::net::lookup_host((first.host.as_str(), first.port))
                .await
                .map_err(|_| Error::new("dnsFailed", "第一站地址无法解析，请检查主机名或本机 DNS"))?
                .collect::<Vec<_>>();
            if addresses.is_empty() {
                return Err(Error::new("dnsFailed", "第一站地址没有可用的 IP"));
            }
            Ok(addresses)
        })
        .await?;
    let socket = a
        .wait("tcp", 0, async {
            TcpStream::connect(addresses.as_slice())
                .await
                .map_err(Error::io)
        })
        .await?;
    let _ = socket.set_nodelay(true);
    let mut stream: Option<Stream> = Some(Box::new(Socket::new(
        socket,
        a.cancel.clone(),
        a.counters.clone(),
    )));
    let mut session = Session {
        chain: vec![],
        path: serde_json::json!(profile.nodes.iter().map(|n|serde_json::json!({"kind":if n.is_ssh(){"ssh"}else{"socks5"},"host":n.host,"port":n.port})).collect::<Vec<_>>()),
        cancel: a.cancel.clone(),
    };
    for index in 0..profile.nodes.len() {
        let next = profile
            .nodes
            .get(index + 1)
            .map(|n| (n.host.clone(), n.port));
        let node = &mut profile.nodes[index];
        if node.is_ssh() {
            let handle =
                ssh::connect(stream.take().unwrap(), node, index, a.clone(), keys.clone()).await?;
            session.chain.push(handle);
            if let Some((host, port)) = next {
                let channel = a
                    .wait("sshTunnel", index, async {
                        session
                            .chain
                            .last()
                            .unwrap()
                            .channel_open_direct_tcpip(host, port as u32, "127.0.0.1", 0)
                            .await
                            .map_err(Error::from)
                    })
                    .await?;
                stream = Some(Box::new(channel.into_stream()));
            }
        } else {
            let (host, port) =
                next.ok_or_else(|| Error::new("route", "连接链最后一站必须为 SSH"))?;
            let credentials = match &node.auth {
                Auth::Socks {
                    username: Some(u),
                    password: Some(p),
                } => Some((u.as_str(), p.as_str())),
                Auth::Socks { username: None, .. } => None,
                _ => return Err(Error::new("credentials", "缺少 SOCKS5 认证资料")),
            };
            a.wait(
                "socks5",
                index,
                super::socks5::connect(stream.as_mut().unwrap(), &host, port, credentials),
            )
            .await?;
        }
    }
    Ok(session)
}

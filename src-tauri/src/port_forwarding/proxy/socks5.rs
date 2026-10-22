use super::{connector::Connector, validate_target, Client};
use std::{
    io,
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

async fn request<T: AsyncRead + AsyncWrite + Unpin>(
    io: &mut T,
) -> io::Result<crate::ssh_transport::forwarding::Target> {
    let version = io.read_u8().await?;
    let count = io.read_u8().await? as usize;
    let mut methods = vec![0; count];
    io.read_exact(&mut methods).await?;
    if version != 5 || !methods.contains(&0) {
        io.write_all(&[5, 255]).await?;
        return Err(io::ErrorKind::InvalidData.into());
    }
    io.write_all(&[5, 0]).await?;
    let mut head = [0; 4];
    io.read_exact(&mut head).await?;
    let failure = if head[0] != 5 || head[2] != 0 {
        Some(1)
    } else if head[1] != 1 {
        Some(7)
    } else {
        None
    };
    if let Some(code) = failure {
        reply(io, code).await?;
        return Err(io::ErrorKind::InvalidData.into());
    }
    let host = match head[3] {
        1 => {
            let mut b = [0; 4];
            io.read_exact(&mut b).await?;
            std::net::Ipv4Addr::from(b).to_string()
        }
        4 => {
            let mut b = [0; 16];
            io.read_exact(&mut b).await?;
            std::net::Ipv6Addr::from(b).to_string()
        }
        3 => {
            let count = io.read_u8().await? as usize;
            let mut b = vec![0; count];
            io.read_exact(&mut b).await?;
            String::from_utf8(b).map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?
        }
        _ => {
            reply(io, 8).await?;
            return Err(io::ErrorKind::InvalidData.into());
        }
    };
    let port = io.read_u16().await?;
    match validate_target(&host, port) {
        Ok(t) => Ok(t),
        Err(_) => {
            reply(io, 8).await?;
            Err(io::ErrorKind::InvalidData.into())
        }
    }
}
async fn reply<T: AsyncWrite + Unpin>(io: &mut T, code: u8) -> io::Result<()> {
    io.write_all(&[5, code, 0, 1, 0, 0, 0, 0, 0, 0]).await
}
pub async fn serve<T: AsyncRead + AsyncWrite + Unpin>(
    mut io: T,
    client: Arc<Client>,
    connector: Connector,
) -> io::Result<()> {
    let target = tokio::time::timeout(Duration::from_secs(10), request(&mut io))
        .await
        .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))??;
    client.negotiated();
    let pending = match connector.open(client.clone(), target, "socks5").await {
        Ok(p) => p,
        Err(_) => {
            reply(&mut io, 1).await?;
            return Ok(());
        }
    };
    tokio::time::timeout(Duration::from_secs(5), reply(&mut io, 0))
        .await
        .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))??;
    let mut tunnel = pending
        .activate()
        .await
        .map_err(|_| io::Error::from(io::ErrorKind::ConnectionAborted))?;
    client.count_received.store(true, Ordering::Relaxed);
    tokio::io::copy_bidirectional(&mut io, &mut tunnel).await?;
    Ok(())
}

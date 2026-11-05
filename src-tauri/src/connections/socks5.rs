//! RFC 1928/1929 over an existing stream. Never resolve downstream domains locally.
use super::{io::Stream, Error};
use std::net::IpAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub(super) async fn connect(
    stream: &mut Stream,
    host: &str,
    port: u16,
    credentials: Option<(&str, &str)>,
) -> Result<(), Error> {
    let invalid = || Error::new("socksProtocol", "SOCKS5 代理响应无效");
    let method = if credentials.is_some() { 2 } else { 0 };
    stream.write_all(&[5, 1, method]).await.map_err(Error::io)?;
    let mut response = [0; 2];
    stream.read_exact(&mut response).await.map_err(Error::io)?;
    if response != [5, method] {
        return Err(Error::new(
            "socksAuthentication",
            "SOCKS5 代理拒绝所选认证方式",
        ));
    }
    if let Some((user, password)) = credentials {
        if user.is_empty() || user.len() > 255 || password.len() > 255 {
            return Err(Error::new(
                "socksCredentials",
                "SOCKS5 用户名或密码超过协议长度限制",
            ));
        }
        let mut request = vec![1, user.len() as u8];
        request.extend_from_slice(user.as_bytes());
        request.push(password.len() as u8);
        request.extend_from_slice(password.as_bytes());
        stream.write_all(&request).await.map_err(Error::io)?;
        stream.read_exact(&mut response).await.map_err(Error::io)?;
        if response != [1, 0] {
            return Err(Error::new(
                "socksAuthentication",
                "SOCKS5 用户名或密码被拒绝",
            ));
        }
    }
    let mut request = vec![5, 1, 0];
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) => {
            request.push(1);
            request.extend_from_slice(&ip.octets());
        }
        Ok(IpAddr::V6(ip)) => {
            request.push(4);
            request.extend_from_slice(&ip.octets());
        }
        Err(_) => {
            if host.is_empty() || host.len() > 255 || !host.is_ascii() {
                return Err(Error::new(
                    "socksAddress",
                    "代理域名应为最长 255 字节的 ASCII 域名，可使用 Punycode",
                ));
            }
            request.extend_from_slice(&[3, host.len() as u8]);
            request.extend_from_slice(host.as_bytes());
        }
    }
    request.extend_from_slice(&port.to_be_bytes());
    stream.write_all(&request).await.map_err(Error::io)?;
    let mut header = [0; 4];
    stream.read_exact(&mut header).await.map_err(Error::io)?;
    if header[0] != 5 || header[2] != 0 {
        return Err(invalid());
    }
    if header[1] != 0 {
        return Err(Error::new(
            "socksConnect",
            format!("SOCKS5 无法访问下一站（响应码 {}）", header[1]),
        ));
    }
    let size = match header[3] {
        1 => 4,
        4 => 16,
        3 => stream.read_u8().await.map_err(Error::io)? as usize,
        _ => return Err(invalid()),
    };
    let mut bound = vec![0; size + 2];
    stream.read_exact(&mut bound).await.map_err(Error::io)?;
    Ok(())
}

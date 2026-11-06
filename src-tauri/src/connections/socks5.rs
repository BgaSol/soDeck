//! SOCKS5 wire messages over the existing route stream; policy remains core-owned.
use super::{io::Stream, Error};
use socker::{socks5::*, DecodeStatus, Message};
use std::net::IpAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn protocol() -> Error {
    Error::new("socksProtocol", "SOCKS5 代理响应无效")
}
fn decode<M: Message>(bytes: &[u8], code: &str) -> Result<M, Error> {
    match M::decode(bytes) {
        Ok(DecodeStatus::Complete((message, used))) if used == bytes.len() => Ok(message),
        _ => Err(Error::new(code, "SOCKS5 代理响应无效")),
    }
}
async fn write<M: Message>(stream: &mut Stream, message: M) -> Result<(), Error> {
    let mut bytes = Vec::with_capacity(message.encoded_len());
    message.encode(&mut bytes).map_err(|_| protocol())?;
    stream.write_all(&bytes).await.map_err(Error::io)
}
pub(super) async fn connect(
    stream: &mut Stream,
    host: &str,
    port: u16,
    credentials: Option<(&str, &str)>,
) -> Result<(), Error> {
    let address = match host.parse::<IpAddr>() {
        Ok(ip) => Address::from(ip),
        Err(_) if !host.is_empty() && host.len() <= 255 && host.is_ascii() => Address::from(host),
        _ => {
            return Err(Error::new(
                "socksAddress",
                "代理域名应为最长 255 字节的 ASCII 域名，可使用 Punycode",
            ))
        }
    };
    if credentials.is_some_and(|(u, p)| u.is_empty() || u.len() > 255 || p.len() > 255) {
        return Err(Error::new(
            "socksCredentials",
            "SOCKS5 用户名或密码超过协议长度限制",
        ));
    }
    let method = if credentials.is_some() {
        AuthenticationMethod::USERNAME_PASSWORD
    } else {
        AuthenticationMethod::NO_AUTHENTICATION
    };
    write(
        stream,
        ClientGreeting {
            methods: vec![method].into_boxed_slice(),
        },
    )
    .await?;
    let mut pair = [0; 2];
    stream.read_exact(&mut pair).await.map_err(Error::io)?;
    if decode::<ServerChoice>(&pair, "socksAuthentication")?.method != method {
        return Err(Error::new(
            "socksAuthentication",
            "SOCKS5 代理拒绝所选认证方式",
        ));
    }
    if let Some((username, password)) = credentials {
        write(
            stream,
            auth::username_password::Request {
                username: username.as_bytes().into(),
                password: password.as_bytes().into(),
            },
        )
        .await?;
        stream.read_exact(&mut pair).await.map_err(Error::io)?;
        if !decode::<auth::username_password::Response>(&pair, "socksAuthentication")?.is_success()
        {
            return Err(Error::new(
                "socksAuthentication",
                "SOCKS5 用户名或密码被拒绝",
            ));
        }
    }
    write(
        stream,
        Request {
            command: CommandType::CONNECT,
            address,
            port,
        },
    )
    .await?;
    let mut bytes = vec![0; 4];
    stream.read_exact(&mut bytes).await.map_err(Error::io)?;
    if bytes[0] != VERSION || bytes[2] != 0 {
        return Err(protocol());
    }
    // Preserve a refusal even when a proxy omits the remainder of its error reply.
    let reply = Reply::from(bytes[1]);
    if !reply.is_success() {
        return Err(Error::new(
            "socksConnect",
            format!("SOCKS5 无法访问下一站（响应码 {}）", u8::from(reply)),
        ));
    }
    let total = match AddressType::from(bytes[3]) {
        AddressType::IP_V4 => 10,
        AddressType::IP_V6 => 22,
        AddressType::DOMAIN_NAME => {
            bytes.push(stream.read_u8().await.map_err(Error::io)?);
            7 + usize::from(bytes[4])
        }
        _ => return Err(protocol()),
    };
    let start = bytes.len();
    bytes.resize(total, 0);
    stream
        .read_exact(&mut bytes[start..])
        .await
        .map_err(Error::io)?;
    decode::<Response>(&bytes, "socksProtocol")?;
    Ok(())
}

#[cfg(test)]
#[path = "socks5_tests.rs"]
mod tests;

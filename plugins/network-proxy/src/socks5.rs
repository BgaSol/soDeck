#[derive(Default)]
pub struct Socks {
    stage: u8,
    bytes: Vec<u8>,
    need: usize,
    kind: u8,
    host: Option<String>,
}
pub enum Action {
    Read(usize),
    Write(Vec<u8>),
    Connect(String, u16),
    Reject(Vec<u8>),
}
pub fn reply(code: u8) -> Vec<u8> {
    vec![5, code, 0, 1, 0, 0, 0, 0, 0, 0]
}
impl Socks {
    pub fn next(&self) -> usize {
        if self.need == 0 {
            2
        } else {
            self.need - self.bytes.len()
        }
    }
    pub fn feed(&mut self, bytes: &[u8]) -> Result<Action, String> {
        self.bytes.extend_from_slice(bytes);
        let need = if self.need == 0 { 2 } else { self.need };
        if self.bytes.len() < need {
            return Ok(Action::Read(need - self.bytes.len()));
        }
        if self.bytes.len() != need {
            return Err("SOCKS 协商数据长度无效".into());
        }
        let b = std::mem::take(&mut self.bytes);
        match self.stage {
            0 => {
                if b[0] != 5 || b[1] == 0 {
                    return Err("SOCKS5 协商无效".into());
                }
                self.stage = 1;
                self.need = b[1] as usize;
                Ok(Action::Read(self.need))
            }
            1 => {
                self.stage = 2;
                self.need = 4;
                if !b.contains(&0) {
                    Ok(Action::Reject(vec![5, 255]))
                } else {
                    Ok(Action::Write(vec![5, 0]))
                }
            }
            2 => {
                if b[0] != 5 || b[2] != 0 {
                    return Ok(Action::Reject(reply(1)));
                }
                if b[1] != 1 {
                    return Ok(Action::Reject(reply(7)));
                }
                self.kind = b[3];
                self.stage = 3;
                self.need = match self.kind {
                    1 => 4,
                    4 => 16,
                    3 => 1,
                    _ => return Ok(Action::Reject(reply(8))),
                };
                Ok(Action::Read(self.need))
            }
            3 if self.kind == 3 => {
                if b[0] == 0 {
                    return Ok(Action::Reject(reply(8)));
                }
                self.stage = 4;
                self.need = b[0] as usize;
                Ok(Action::Read(self.need))
            }
            3 | 4 => {
                let host = match self.kind {
                    1 => std::net::Ipv4Addr::new(b[0], b[1], b[2], b[3]).to_string(),
                    4 => std::net::Ipv6Addr::from(
                        <[u8; 16]>::try_from(b.as_slice()).map_err(|_| "IPv6 无效")?,
                    )
                    .to_string(),
                    _ => String::from_utf8(b).map_err(|_| "域名无效")?,
                };
                if host.chars().any(|c| c.is_control() || c.is_whitespace()) {
                    return Ok(Action::Reject(reply(8)));
                }
                self.host = Some(host);
                self.stage = 5;
                self.need = 2;
                Ok(Action::Read(2))
            }
            5 => {
                let port = u16::from_be_bytes([b[0], b[1]]);
                if port == 0 {
                    return Ok(Action::Reject(reply(8)));
                }
                self.stage = 6;
                Ok(Action::Connect(self.host.take().unwrap(), port))
            }
            _ => Err("SOCKS 协商已结束".into()),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn split_domain_request_keeps_target_unresolved() {
        let mut s = Socks::default();
        for b in [5u8, 1] {
            s.feed(&[b]).unwrap();
        }
        assert!(matches!(s.feed(&[0]).unwrap(), Action::Write(_)));
        s.feed(&[5, 1, 0, 3]).unwrap();
        s.feed(&[3]).unwrap();
        s.feed(b"d").unwrap();
        s.feed(b"b").unwrap();
        s.feed(b"s").unwrap();
        assert!(matches!(s.feed(&[1,187]).unwrap(),Action::Connect(h,443) if h=="dbs"));
    }
    #[test]
    fn rejects_password_and_udp() {
        let mut s = Socks::default();
        s.feed(&[5, 1]).unwrap();
        assert!(matches!(s.feed(&[2]).unwrap(),Action::Reject(v) if v==vec![5,255]));
        let mut s = Socks::default();
        s.feed(&[5, 1]).unwrap();
        s.feed(&[0]).unwrap();
        assert!(matches!(s.feed(&[5,3,0,1]).unwrap(),Action::Reject(v) if v[1]==7));
    }
    #[test]
    fn ipv4_and_ipv6_preserve_literal_targets_and_port() {
        for (kind, bytes, expected) in [
            (1, vec![10, 20, 30, 40], "10.20.30.40"),
            (4, std::net::Ipv6Addr::LOCALHOST.octets().to_vec(), "::1"),
        ] {
            let mut s = Socks::default();
            s.feed(&[5, 1]).unwrap();
            s.feed(&[0]).unwrap();
            s.feed(&[5, 1, 0, kind]).unwrap();
            for byte in bytes {
                s.feed(&[byte]).unwrap();
            }
            assert!(
                matches!(s.feed(&[0x20,0xfb]).unwrap(),Action::Connect(host,8443) if host==expected)
            );
        }
    }
}

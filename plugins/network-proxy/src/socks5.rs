use socker::{socks5::*, DecodeStatus, Message};

#[derive(Default, PartialEq)]
enum Stage {
    #[default]
    Greeting,
    Request,
    Target,
    Done,
}
#[derive(Default)]
pub struct Socks {
    stage: Stage,
    bytes: Vec<u8>,
}
pub enum Action {
    Read(usize),
    Write(Vec<u8>),
    Connect(String, u16),
    Reject(Vec<u8>),
}

pub fn encode<M: Message>(message: M) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::with_capacity(message.encoded_len());
    message.encode(&mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes)
}
pub fn decode<M: Message>(bytes: &[u8]) -> Result<M, String> {
    match M::decode(bytes).map_err(|e| e.to_string())? {
        DecodeStatus::Complete((message, used)) if used == bytes.len() => Ok(message),
        _ => Err("SOCKS5 协商数据长度无效".into()),
    }
}
// Framing only: the library decodes the address and port once the frame is complete.
pub fn frame_len(bytes: &[u8]) -> Result<usize, String> {
    if bytes.len() < 4 {
        return Ok(4);
    }
    match AddressType::from(bytes[3]) {
        AddressType::IP_V4 => Ok(10),
        AddressType::IP_V6 => Ok(22),
        AddressType::DOMAIN_NAME if bytes.len() < 5 => Ok(5),
        AddressType::DOMAIN_NAME => Ok(7 + usize::from(bytes[4])),
        _ => Err("SOCKS5 地址类型无效".into()),
    }
}
pub fn reply(code: Reply) -> Vec<u8> {
    encode(Response {
        reply: code,
        address: Address::Ipv4(std::net::Ipv4Addr::UNSPECIFIED),
        port: 0,
    })
    .expect("fixed IPv4 SOCKS5 response")
}
impl Socks {
    pub fn next(&self) -> usize {
        let total = if self.stage == Stage::Greeting {
            self.bytes.get(1).map_or(2, |n| 2 + usize::from(*n))
        } else {
            frame_len(&self.bytes).unwrap_or(self.bytes.len())
        };
        total.saturating_sub(self.bytes.len())
    }
    pub fn failure_reply(&mut self, code: Reply) -> Option<Vec<u8>> {
        let needs_reply = matches!(self.stage, Stage::Request | Stage::Target);
        self.stage = Stage::Done;
        needs_reply.then(|| reply(code))
    }
    pub fn succeeded(&mut self) -> Vec<u8> {
        self.stage = Stage::Done;
        reply(Reply::SUCCESS)
    }
    fn reject(&mut self, code: Reply) -> Action {
        self.stage = Stage::Done;
        Action::Reject(reply(code))
    }
    pub fn feed(&mut self, bytes: &[u8]) -> Result<Action, String> {
        if !matches!(self.stage, Stage::Greeting | Stage::Request) {
            return Err("SOCKS5 协商已结束".into());
        }
        if bytes.len() > self.next() {
            return Err("SOCKS5 协商数据长度无效".into());
        }
        self.bytes.extend_from_slice(bytes);
        if self.stage == Stage::Greeting {
            if self.bytes.first().is_some_and(|v| *v != VERSION) || self.bytes.get(1) == Some(&0) {
                return Err("SOCKS5 协商无效".into());
            }
            if self.next() > 0 {
                return Ok(Action::Read(self.next()));
            }
            let greeting = decode::<ClientGreeting>(&self.bytes)?;
            self.bytes.clear();
            if !greeting
                .methods
                .contains(&AuthenticationMethod::NO_AUTHENTICATION)
            {
                self.stage = Stage::Done;
                return Ok(Action::Reject(encode(ServerChoice {
                    method: AuthenticationMethod::NO_ACCEPTABLE_METHODS,
                })?));
            }
            self.stage = Stage::Request;
            return Ok(Action::Write(encode(ServerChoice {
                method: AuthenticationMethod::NO_AUTHENTICATION,
            })?));
        }
        if self.bytes.len() >= 4 {
            if self.bytes[0] != VERSION || self.bytes[2] != 0 {
                return Ok(self.reject(Reply::GENERAL_FAILURE));
            }
            if CommandType::from(self.bytes[1]) != CommandType::CONNECT {
                return Ok(self.reject(Reply::COMMAND_NOT_SUPPORTED));
            }
            if frame_len(&self.bytes).is_err()
                || (self.bytes[3] == u8::from(AddressType::DOMAIN_NAME)
                    && self.bytes.get(4) == Some(&0))
            {
                return Ok(self.reject(Reply::ADDRESS_TYPE_NOT_SUPPORTED));
            }
        }
        if self.next() > 0 {
            return Ok(Action::Read(self.next()));
        }
        let request = decode::<Request>(&self.bytes)?;
        let host = match request.address {
            Address::Ipv4(ip) => ip.to_string(),
            Address::Ipv6(ip) => ip.to_string(),
            Address::Domain(domain) => {
                String::from_utf8(domain.into_vec()).map_err(|_| "域名无效")?
            }
        };
        if request.port == 0
            || host.is_empty()
            || host.chars().any(|c| c.is_control() || c.is_whitespace())
        {
            return Ok(self.reject(Reply::ADDRESS_TYPE_NOT_SUPPORTED));
        }
        self.bytes.clear();
        self.stage = Stage::Target;
        Ok(Action::Connect(host, request.port))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn feed_frame(s: &mut Socks, bytes: &[u8], chunk: usize) -> Action {
        let mut offset = 0;
        loop {
            let end = (offset + s.next().min(chunk)).min(bytes.len());
            assert!(end > offset);
            let action = s.feed(&bytes[offset..end]).unwrap();
            offset = end;
            if !matches!(action, Action::Read(_)) {
                assert_eq!(offset, bytes.len());
                return action;
            }
        }
    }
    fn ready() -> Socks {
        let mut s = Socks::default();
        assert!(matches!(
            feed_frame(&mut s, &[5, 1, 0], 1),
            Action::Write(_)
        ));
        s
    }
    #[test]
    fn maximum_domain_uses_bounded_reads_and_keeps_the_exact_target() {
        let mut s = ready();
        assert_eq!(s.next(), 4);
        s.feed(&[5, 1, 0, 3]).unwrap();
        assert_eq!(s.next(), 1);
        s.feed(&[255]).unwrap();
        assert_eq!(s.next(), 257);
        let body = [vec![b'a'; 255], vec![0, 80]].concat();
        assert!(matches!(feed_frame(&mut s, &body, 257), Action::Connect(h, 80) if h.len() == 255));
        assert!(s.feed(&[0]).is_err());
    }
    #[test]
    fn unknown_methods_can_be_rejected_or_ignored_when_no_auth_is_offered() {
        let mut s = Socks::default();
        assert!(
            matches!(feed_frame(&mut s, &[5, 2, 0x80, 2], 1), Action::Reject(b) if b == [5, 255])
        );
        assert!(s.failure_reply(Reply::GENERAL_FAILURE).is_none());
        let mut s = Socks::default();
        assert!(
            matches!(feed_frame(&mut s, &[5, 3, 0x80, 2, 0], 2), Action::Write(b) if b == [5, 0])
        );
    }
    #[test]
    fn malformed_targets_and_request_prefixes_have_the_expected_reply() {
        for (input, code) in [
            (vec![4, 1, 0, 1], 1),
            (vec![5, 1, 1, 1], 1),
            (vec![5, 2, 0, 1], 7),
            (vec![5, 3, 0, 1], 7),
            (vec![5, 0x80, 0, 1], 7),
            (vec![5, 1, 0, 0x80], 8),
            (vec![5, 1, 0, 3, 0], 8),
            (vec![5, 1, 0, 3, 1, b' ', 0, 80], 8),
            (vec![5, 1, 0, 3, 1, 0, 0, 80], 8),
            (vec![5, 1, 0, 1, 127, 0, 0, 1, 0, 0], 8),
        ] {
            let mut s = ready();
            assert!(
                matches!(feed_frame(&mut s, &input, 1), Action::Reject(b) if b == reply(Reply::from(code)))
            );
            assert!(s.failure_reply(Reply::GENERAL_FAILURE).is_none());
        }
    }
    #[test]
    fn failure_reply_follows_the_negotiation_stage_and_is_emitted_once() {
        let mut s = Socks::default();
        s.feed(&[5]).unwrap();
        assert!(s.failure_reply(Reply::TTL_EXPIRED).is_none());
        let mut s = ready();
        assert_eq!(
            s.failure_reply(Reply::TTL_EXPIRED),
            Some(reply(Reply::TTL_EXPIRED))
        );
        assert!(s.failure_reply(Reply::GENERAL_FAILURE).is_none());
        let mut s = ready();
        feed_frame(&mut s, &[5, 1, 0, 1, 127, 0, 0, 1, 0, 80], 1);
        assert_eq!(s.succeeded(), [5, 0, 0, 1, 0, 0, 0, 0, 0, 0]);
        assert!(s.failure_reply(Reply::GENERAL_FAILURE).is_none());
    }
    #[test]
    fn oversized_feed_and_invalid_greeting_cannot_consume_payload_or_open_target() {
        for bytes in [vec![4], vec![5, 0], vec![5, 1, 0, 99]] {
            let mut s = Socks::default();
            assert!(s.feed(&bytes).is_err());
            assert!(s.failure_reply(Reply::GENERAL_FAILURE).is_none());
        }
        let mut s = ready();
        let error = s.feed(&[5, 1, 0, 1, 127]).err().unwrap();
        assert!(error.contains("长度"));
    }
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

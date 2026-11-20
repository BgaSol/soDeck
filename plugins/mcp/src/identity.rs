use crate::targets::Descriptor;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum MatchMode {
    #[default]
    Exact,
    Contains,
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Host {
    Ip(IpAddr, Option<String>),
    Dns(String),
}
pub(crate) fn host(text: &str) -> Option<Host> {
    let bracketed = text.starts_with('[') || text.ends_with(']');
    let text = if bracketed {
        if !text.starts_with('[') || !text.ends_with(']') {
            return None;
        }
        &text[1..text.len() - 1]
    } else {
        text
    };
    if let Some((address, zone)) = text.split_once('%') {
        if zone.is_empty() || zone.contains('%') {
            return None;
        }
        return address
            .parse::<std::net::Ipv6Addr>()
            .ok()
            .map(|v| Host::Ip(IpAddr::V6(v), Some(zone.into())));
    }
    if let Ok(ip) = text.parse::<IpAddr>() {
        if bracketed && !ip.is_ipv6() {
            return None;
        }
        return Some(Host::Ip(ip, None));
    }
    if bracketed
        || text.is_empty()
        || text
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, ':' | '[' | ']' | '/' | '@'))
    {
        return None;
    }
    let text = text.strip_suffix('.').unwrap_or(text);
    (!text.is_empty()).then(|| Host::Dns(text.to_ascii_lowercase()))
}
pub(crate) fn endpoint(text: &str) -> Option<(Host, u16)> {
    let (h, p) = if let Some(text) = text.strip_prefix('[') {
        let (h, p) = text.split_once("]:")?;
        let h = host(h)?;
        if !matches!(h, Host::Ip(IpAddr::V6(_), _)) {
            return None;
        }
        (h, p)
    } else {
        if text.matches(':').count() != 1 {
            return None;
        }
        let (h, p) = text.split_once(':')?;
        (host(h)?, p)
    };
    if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let port = p.parse().ok()?;
    (port > 0).then_some((h, port))
}
pub(crate) struct Needle {
    name: String,
    dns_text: String,
    host: Option<Host>,
    endpoint: Option<(Host, u16)>,
    octet: Option<u8>,
    mode: MatchMode,
}
impl Needle {
    pub(crate) fn new(text: &str, mode: MatchMode) -> Self {
        let octet = text.parse::<u8>().ok().filter(|v| v.to_string() == text);
        Self {
            name: text.to_lowercase(),
            dns_text: text
                .strip_suffix('.')
                .filter(|s| !s.is_empty())
                .unwrap_or(text)
                .to_ascii_lowercase(),
            host: host(text),
            endpoint: endpoint(text),
            octet,
            mode,
        }
    }
    pub(crate) fn matches(&self, t: &Descriptor) -> Vec<&'static str> {
        let mut reasons = Vec::new();
        let name = t.name.trim().to_lowercase();
        if name == self.name {
            reasons.push("nameExact");
        }
        let target_host = host(&t.host);
        if self.host.is_some() && self.host == target_host {
            reasons.push("hostExact");
        }
        if self
            .endpoint
            .as_ref()
            .is_some_and(|(h, p)| Some(h) == target_host.as_ref() && *p == t.port)
        {
            reasons.push("endpointExact");
        }
        if self.octet.is_some_and(|octet|matches!(&target_host,Some(Host::Ip(IpAddr::V4(ip),None)) if ip.octets()[3]==octet)) { reasons.push("ipv4LastOctet"); }
        if self.mode == MatchMode::Contains {
            if name != self.name && name.contains(&self.name) {
                reasons.push("nameContains");
            }
            if self.host != target_host
                && matches!(&target_host,Some(Host::Dns(dns)) if dns.contains(&self.dns_text))
            {
                reasons.push("hostnameContains");
            }
        }
        reasons
    }
}

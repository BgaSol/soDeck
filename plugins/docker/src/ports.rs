use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeSet, HashSet},
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
};
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Binding {
    pub container_port: u16,
    pub protocol: String,
    pub host_ip: String,
    pub host_port: u16,
}
impl Binding {
    pub fn target(&self) -> Result<Value, String> {
        if self.protocol != "tcp" {
            return Err("当前快捷隧道仅支持 TCP".into());
        }
        if self.container_port == 0 || self.host_port == 0 {
            return Err("实际发布端口无效".into());
        }
        let ip = if self.host_ip.is_empty() {
            IpAddr::V4(Ipv4Addr::LOCALHOST)
        } else {
            self.host_ip
                .parse::<IpAddr>()
                .map_err(|_| "发布 IP 无效或缺少作用域")?
        };
        let ip = match ip {
            IpAddr::V4(v) if v.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V6(v) if v.is_unspecified() => IpAddr::V6(Ipv6Addr::LOCALHOST),
            v => v,
        };
        if ip.is_multicast()
            || matches!(ip,IpAddr::V4(v)if v.is_broadcast())
            || matches!(ip,IpAddr::V6(v)if v.is_unicast_link_local())
        {
            return Err("发布 IP 无法用于快捷转发".into());
        }
        Ok(json!({"host":ip.to_string(),"port":self.host_port}))
    }
    pub fn value(&self) -> Value {
        serde_json::to_value(self).unwrap()
    }
}
fn port_key(key: &str) -> Option<(u16, &str)> {
    let (port, protocol) = key.split_once('/')?;
    if port.is_empty() || !port.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let port = port.parse::<u16>().ok().filter(|p| *p > 0)?;
    Some((port, protocol))
}
pub fn catalog(container: &Value) -> Value {
    let ports = container["NetworkSettings"]["Ports"].as_object();
    let keys = ports
        .into_iter()
        .flat_map(|p| p.keys())
        .chain(
            container["Config"]["ExposedPorts"]
                .as_object()
                .into_iter()
                .flat_map(|p| p.keys()),
        )
        .collect::<BTreeSet<_>>();
    let mut rows = Vec::new();
    let mut seen = HashSet::new();
    let mut partial = false;
    let mut bytes = 0;
    for key in keys {
        let values = ports.and_then(|p| p.get(key)).and_then(Value::as_array);
        let values = values
            .map(|a| a.iter().collect::<Vec<_>>())
            .filter(|a| !a.is_empty())
            .unwrap_or_else(|| vec![&Value::Null]);
        for published in values {
            if rows.len() >= 100 {
                partial = true;
                break;
            }
            let binding = port_key(key).and_then(|(container_port, protocol)| {
                let host_ip = published["HostIp"].as_str()?;
                if host_ip.len() > 128 {
                    return None;
                }
                let port = published["HostPort"].as_str()?;
                if port.is_empty() || !port.bytes().all(|c| c.is_ascii_digit()) {
                    return None;
                }
                let host_port = port.parse::<u16>().ok().filter(|p| *p > 0)?;
                Some(Binding {
                    container_port,
                    protocol: protocol.into(),
                    host_ip: host_ip.into(),
                    host_port,
                })
            });
            let identity = json!([key, binding.as_ref().map(Binding::value)]).to_string();
            if !seen.insert(identity.clone()) {
                continue;
            }
            let target = binding.as_ref().map(Binding::target);
            let reason = match &target {
                Some(Err(e)) => Some(e.clone()),
                None => Some(
                    if published.is_null() {
                        "此端口未发布到服务器，暂不支持快捷创建"
                    } else {
                        "实际发布绑定无效或不完整"
                    }
                    .to_owned(),
                ),
                _ => None,
            };
            let row = json!({"key":identity,"containerPort":key.chars().take(64).collect::<String>(),"binding":binding.as_ref().map(Binding::value),"target":target.and_then(Result::ok),"reason":reason});
            let size = serde_json::to_vec(&row).unwrap().len();
            if bytes + size > 64 * 1024 {
                partial = true;
                break;
            }
            bytes += size;
            rows.push(row);
        }
        if partial {
            break;
        }
    }
    json!({"rows":rows,"partial":partial})
}
pub fn verify(container: &Value, expected: &Binding) -> Result<Value, String> {
    if container["State"]["Running"] != true
        || container["State"]["Paused"] == true
        || container["State"]["Restarting"] == true
    {
        return Err("容器当前未处于可转发的运行状态".into());
    }
    let key = format!("{}/{}", expected.container_port, expected.protocol);
    let matches = container["NetworkSettings"]["Ports"][&key]
        .as_array()
        .is_some_and(|rows| {
            rows.iter().any(|v| {
                v["HostIp"].as_str() == Some(&expected.host_ip)
                    && v["HostPort"].as_str().and_then(|s| s.parse::<u16>().ok())
                        == Some(expected.host_port)
            })
        });
    if !matches {
        return Err("端口绑定已变化，请刷新后重新选择".into());
    }
    expected.target()
}
#[cfg(test)]
mod tests {
    use super::*;
    fn container() -> Value {
        json!({"State":{"Running":true},"Config":{"ExposedPorts":{"53/udp":{},"99/tcp":{}}},"NetworkSettings":{"Ports":{"80/tcp":[{"HostIp":"0.0.0.0","HostPort":"8080"},{"HostIp":"::","HostPort":"8080"},{"HostIp":"0.0.0.0","HostPort":"8080"}]}}})
    }
    #[test]
    fn actual_binding_addresses_and_declarations() {
        let c = catalog(&container());
        let rows = c["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 4);
        assert_eq!(rows.iter().filter(|r| r["target"].is_object()).count(), 2);
        assert!(rows.iter().any(|r| r["target"]["host"] == "::1"));
        assert!(rows.iter().any(|r| r["target"]["host"] == "127.0.0.1"));
    }
    #[test]
    fn recheck_rejects_changed_bindings_and_non_running_container() {
        let binding = Binding {
            container_port: 80,
            protocol: "tcp".into(),
            host_ip: "0.0.0.0".into(),
            host_port: 8080,
        };
        let mut c = container();
        assert_eq!(verify(&c, &binding).unwrap()["port"], 8080);
        c["NetworkSettings"]["Ports"]["80/tcp"] = json!([]);
        assert!(verify(&c, &binding).is_err());
        c = container();
        c["State"]["Paused"] = json!(true);
        assert!(verify(&c, &binding).is_err());
    }
    #[test]
    fn invalid_and_scoped_bindings_cannot_be_guessed() {
        for host in ["bad", "fe80::1", "ff02::1", "255.255.255.255"] {
            let b = Binding {
                container_port: 80,
                protocol: "tcp".into(),
                host_ip: host.into(),
                host_port: 8080,
            };
            assert!(b.target().is_err());
        }
        let mut c = container();
        c["NetworkSettings"]["Ports"] = json!({"80/tcp":[{"HostPort":"8080"}]});
        assert!(catalog(&c)["rows"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["target"].is_null()));
    }
    #[test]
    fn catalog_is_bounded_and_marks_partial() {
        let mut c = container();
        let bindings = (1..=150)
            .map(|port| json!({"HostIp":"127.0.0.1","HostPort":port.to_string()}))
            .collect::<Vec<_>>();
        c["NetworkSettings"]["Ports"] = json!({"80/tcp":bindings});
        let p = catalog(&c);
        assert!(p["rows"].as_array().unwrap().len() <= 100);
        assert_eq!(p["partial"], true);
    }
}

use serde_json::{json, Value};
use std::{
    fs::{self, File},
    io::Write,
    net::IpAddr,
    path::PathBuf,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bind {
    pub host: IpAddr,
    pub port: u16,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub host: String,
    pub port: u16,
}
impl Target {
    pub fn parse(v: &Value) -> Result<Self, String> {
        let host = v["host"]
            .as_str()
            .filter(|s| {
                !s.is_empty()
                    && s.len() <= 253
                    && !s.chars().any(|c| c.is_whitespace() || c.is_control())
            })
            .ok_or("目标主机无效")?;
        let port = v["port"]
            .as_u64()
            .filter(|p| *p > 0 && *p <= 65535)
            .ok_or("目标端口无效")? as u16;
        Ok(Self {
            host: host
                .parse::<IpAddr>()
                .map(|ip| ip.to_string())
                .unwrap_or_else(|_| host.into()),
            port,
        })
    }
    pub fn value(&self) -> Value {
        json!({"host":self.host,"port":self.port})
    }
    pub fn literal(v: &Value) -> Result<Self, String> {
        let target = Self::parse(v)?;
        let ip = target
            .host
            .parse::<IpAddr>()
            .map_err(|_| "快捷转发需要明确的目标 IP")?;
        if ip.is_unspecified() || ip.is_multicast() {
            return Err("目标 IP 不能是通配或组播地址".into());
        }
        Ok(target)
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    Tcp(Target),
    Socks5,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    pub id: String,
    pub server: String,
    pub revision: u64,
    pub name: String,
    pub bind: Bind,
    pub kind: Kind,
}
impl Rule {
    pub fn parse(v: &Value) -> Result<Self, String> {
        let identifier = |key: &str| {
            v[key]
                .as_str()
                .filter(|s| !s.is_empty() && s.len() <= 128)
                .map(str::to_owned)
                .ok_or_else(|| format!("无效 {key}"))
        };
        let name = v["name"]
            .as_str()
            .filter(|s| s.chars().count() <= 128)
            .ok_or("名称无效")?;
        let host = v["bind"]["host"]
            .as_str()
            .ok_or("监听 IP 无效")?
            .parse()
            .map_err(|_| "监听 IP 无效")?;
        let port = v["bind"]["port"]
            .as_u64()
            .filter(|p| *p <= 65535)
            .ok_or("监听端口无效")? as u16;
        let kind = match v["type"].as_str() {
            Some("tcpForward") => Kind::Tcp(Target::parse(&v["target"])?),
            Some("socks5") if v.get("target").is_none() => Kind::Socks5,
            _ => return Err("未知规则类型或 SOCKS5 包含固定目标".into()),
        };
        Ok(Self {
            id: identifier("id")?,
            server: identifier("serverId")?,
            revision: v["revision"].as_u64().ok_or("修订无效")?,
            name: name.into(),
            bind: Bind { host, port },
            kind,
        })
    }
    pub fn value(&self) -> Value {
        let mut v = json!({"id":self.id,"serverId":self.server,"revision":self.revision,"name":self.name,"bind":{"host":self.bind.host.to_string(),"port":self.bind.port},"type":if matches!(self.kind,Kind::Tcp(_)){"tcpForward"}else{"socks5"}});
        if let Kind::Tcp(target) = &self.kind {
            v["target"] = target.value();
        }
        v
    }
    pub fn target(&self) -> Option<&Target> {
        if let Kind::Tcp(target) = &self.kind {
            Some(target)
        } else {
            None
        }
    }
    pub fn same_runtime(&self, other: &Self) -> bool {
        self.bind == other.bind && self.kind == other.kind
    }
}
#[derive(Clone)]
pub struct Rules {
    path: PathBuf,
    pub items: Vec<Rule>,
    pub error: Option<String>,
}
impl Rules {
    pub fn open(directory: PathBuf) -> Self {
        let path = directory.join("network-rules.json");
        let read = || -> Result<Vec<Rule>, String> {
            let bytes = match fs::read(&path) {
                Ok(v) => v,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
                Err(e) => return Err(e.to_string()),
            };
            if bytes.len() > 512 * 1024 {
                return Err("规则文件过大".into());
            }
            let value: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            if value["schemaVersion"] != 1 {
                return Err("规则文件版本不支持".into());
            }
            let rows = value["rules"]
                .as_array()
                .filter(|r| r.len() <= 256)
                .ok_or("规则数量无效")?;
            let mut ids = std::collections::HashSet::new();
            rows.iter()
                .map(|v| {
                    let rule = Rule::parse(v)?;
                    if !ids.insert(rule.id.clone()) {
                        return Err("规则 ID 重复".into());
                    }
                    Ok(rule)
                })
                .collect()
        };
        match read() {
            Ok(items) => Self {
                path,
                items,
                error: None,
            },
            Err(e) => Self {
                path,
                items: vec![],
                error: Some(format!("规则无法读取，原文件已保留：{e}")),
            },
        }
    }
    fn commit(&mut self, items: Vec<Rule>) -> Result<(), String> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        if items.len() > 256 {
            return Err("最多保存 256 条规则".into());
        }
        let bytes = serde_json::to_vec_pretty(
            &json!({"schemaVersion":1,"rules":items.iter().map(Rule::value).collect::<Vec<_>>()}),
        )
        .map_err(|e| e.to_string())?;
        if bytes.len() > 512 * 1024 {
            return Err("规则容量超过上限".into());
        }
        let temp = self.path.with_extension("json.tmp");
        let write = || -> std::io::Result<()> {
            fs::create_dir_all(self.path.parent().unwrap())?;
            let mut f = File::create(&temp)?;
            f.write_all(&bytes)?;
            f.sync_all()?;
            drop(f);
            fs::rename(&temp, &self.path)
        };
        write().map_err(|e| format!("规则保存失败：{e}"))?;
        self.items = items;
        Ok(())
    }
    pub fn save(&mut self, server: &str, value: Value) -> Result<Value, String> {
        let mut rule = Rule::parse(&value)?;
        if rule.server != server {
            return Err("规则不属于当前服务器".into());
        }
        let mut items = self.items.clone();
        if let Some(old) = items.iter_mut().find(|r| r.id == rule.id) {
            if old.server != server
                || old.revision != rule.revision
                || std::mem::discriminant(&old.kind) != std::mem::discriminant(&rule.kind)
            {
                return Err("规则已变化，请重新打开编辑".into());
            }
            rule.revision = rule.revision.checked_add(1).ok_or("修订已满")?;
            *old = rule.clone();
        } else {
            if rule.revision != 0 {
                return Err("规则已删除，请重新打开".into());
            }
            rule.revision = 1;
            items.push(rule.clone());
        }
        self.commit(items)?;
        Ok(rule.value())
    }
    pub fn delete(&mut self, server: &str, id: &str, revision: u64) -> Result<(), String> {
        let old = self
            .items
            .iter()
            .find(|r| r.id == id && r.server == server)
            .ok_or("规则已删除")?;
        if old.revision != revision {
            return Err("规则已变化，请刷新".into());
        }
        self.commit(self.items.iter().filter(|r| r.id != id).cloned().collect())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn writes_conflicts_and_corruption_preserves_file() {
        let dir = std::env::temp_dir().join(format!("sodeck-rules-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let mut s = Rules::open(dir.clone());
        let r = json!({"id":"a","serverId":"s","revision":0,"name":"","type":"socks5","bind":{"host":"127.0.0.1","port":0}});
        let saved = s.save("s", r.clone()).unwrap();
        assert_eq!(saved["revision"], 1);
        assert!(s.save("s", r).is_err());
        assert_eq!(Rules::open(dir.clone()).items.len(), 1);
        fs::write(dir.join("network-rules.json"), b"broken").unwrap();
        let mut bad = Rules::open(dir.clone());
        assert!(bad.save("s", saved).is_err());
        assert_eq!(fs::read(dir.join("network-rules.json")).unwrap(), b"broken");
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn literal_identity_and_runtime_configuration() {
        let mut rule=Rule::parse(&json!({"id":"a","serverId":"s","revision":0,"name":"first","type":"tcpForward","bind":{"host":"127.0.0.1","port":0},"target":{"host":"2001:0db8::1","port":80}})).unwrap();
        assert_eq!(rule.target().unwrap().host, "2001:db8::1");
        let old = rule.clone();
        rule.name = "renamed".into();
        assert!(rule.same_runtime(&old));
        rule.bind.port = 123;
        assert!(!rule.same_runtime(&old));
        for host in ["localhost", "0.0.0.0", "::", "ff02::1"] {
            assert!(Target::literal(&json!({"host":host,"port":80})).is_err());
        }
    }
}

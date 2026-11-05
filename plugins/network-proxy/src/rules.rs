use serde_json::{json, Value};
use std::{
    fs::{self, File},
    io::Write,
    path::PathBuf,
};
pub struct Rules {
    path: PathBuf,
    pub items: Vec<Value>,
    pub error: Option<String>,
}
pub fn validate(r: &Value) -> Result<(), String> {
    for key in ["id", "serverId"] {
        if r[key]
            .as_str()
            .is_none_or(|s| s.is_empty() || s.len() > 128)
        {
            return Err(format!("无效 {key}"));
        }
    }
    if r["name"].as_str().is_none_or(|s| s.chars().count() > 128)
        || r["revision"].as_u64().is_none()
    {
        return Err("名称或修订无效".into());
    }
    if r["bind"]["host"]
        .as_str()
        .is_none_or(|s| s.parse::<std::net::IpAddr>().is_err())
        || r["bind"]["port"].as_u64().is_none_or(|p| p > 65535)
    {
        return Err("监听 IP 或端口无效".into());
    }
    match r["type"].as_str() {
        Some("socks5") => {
            if r.get("target").is_some() {
                return Err("SOCKS5 不应保存固定目标".into());
            }
        }
        Some("tcpForward") => {
            if r["target"]["host"].as_str().is_none_or(|s| {
                s.is_empty()
                    || s.len() > 253
                    || s.chars().any(|c| c.is_whitespace() || c.is_control())
            }) || r["target"]["port"]
                .as_u64()
                .is_none_or(|p| p == 0 || p > 65535)
            {
                return Err("目标主机或端口无效".into());
            }
        }
        _ => return Err("未知规则类型".into()),
    }
    Ok(())
}
impl Rules {
    pub fn open(directory: PathBuf) -> Self {
        let path = directory.join("network-rules.json");
        let result = (|| {
            let bytes = match fs::read(&path) {
                Ok(b) => b,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
                Err(e) => return Err(e.to_string()),
            };
            if bytes.len() > 512 * 1024 {
                return Err("规则文件过大".into());
            }
            let v: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            if v["schemaVersion"] != 1 {
                return Err("规则文件版本不支持".into());
            }
            let items = v["rules"]
                .as_array()
                .filter(|v| v.len() <= 256)
                .ok_or("规则数量无效")?
                .clone();
            let mut ids = std::collections::HashSet::new();
            for r in &items {
                validate(r)?;
                if !ids.insert(r["id"].clone().to_string()) {
                    return Err("规则 ID 重复".into());
                }
            }
            Ok(items)
        })();
        match result {
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
    fn commit(&mut self, items: Vec<Value>) -> Result<(), String> {
        if let Some(e) = &self.error {
            return Err(e.clone());
        }
        if items.len() > 256 {
            return Err("最多保存 256 条规则".into());
        }
        let bytes = serde_json::to_vec_pretty(&json!({"schemaVersion":1,"rules":items}))
            .map_err(|e| e.to_string())?;
        if bytes.len() > 512 * 1024 {
            return Err("规则容量超过上限".into());
        }
        let temp = self.path.with_extension("json.tmp");
        let save = || -> std::io::Result<()> {
            fs::create_dir_all(self.path.parent().unwrap())?;
            let mut f = File::create(&temp)?;
            f.write_all(&bytes)?;
            f.sync_all()?;
            drop(f);
            fs::rename(&temp, &self.path)
        };
        save().map_err(|e| format!("规则保存失败：{e}"))?;
        self.items = items;
        Ok(())
    }
    pub fn save(&mut self, server: &str, mut r: Value) -> Result<Value, String> {
        validate(&r)?;
        if r["serverId"] != server {
            return Err("规则不属于当前服务器".into());
        }
        let mut items = self.items.clone();
        if let Some(old) = items.iter_mut().find(|v| v["id"] == r["id"]) {
            if old["serverId"] != server
                || old["revision"] != r["revision"]
                || old["type"] != r["type"]
            {
                return Err("规则已变化，请重新打开编辑".into());
            }
            r["revision"] = json!(r["revision"]
                .as_u64()
                .unwrap()
                .checked_add(1)
                .ok_or("修订已满")?);
            *old = r.clone();
        } else {
            if r["revision"] != 0 {
                return Err("规则已删除，请重新打开".into());
            }
            r["revision"] = json!(1);
            items.push(r.clone());
        }
        self.commit(items)?;
        Ok(r)
    }
    pub fn delete(&mut self, server: &str, id: &str, revision: u64) -> Result<(), String> {
        let old = self
            .items
            .iter()
            .find(|r| r["id"] == id && r["serverId"] == server)
            .ok_or("规则已删除")?;
        if old["revision"] != revision {
            return Err("规则已变化，请刷新".into());
        }
        self.commit(
            self.items
                .iter()
                .filter(|r| r["id"] != id)
                .cloned()
                .collect(),
        )
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
}

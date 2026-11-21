//! Local server directory. This module does not open network connections.
mod change;
pub(crate) mod connection_profile;
mod read;
mod rpc;
use crate::{core::now, storage, AppCore};
use change::validate;
pub use change::{AuthInput, CatalogChange, ProxyInput, SecretChange, ServerInput, ServerPatch};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tag {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RouteStep {
    #[serde(rename_all = "camelCase")]
    Ssh { id: String, server_id: String },
    #[serde(rename_all = "camelCase")]
    Socks5 { id: String, proxy_id: String },
}
impl RouteStep {
    fn id(&self) -> &str {
        match self {
            Self::Ssh { id, .. } | Self::Socks5 { id, .. } => id,
        }
    }
    fn new_id(&mut self, value: String) {
        match self {
            Self::Ssh { id, .. } | Self::Socks5 { id, .. } => *id = value,
        }
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Authentication {
    #[serde(rename_all = "camelCase")]
    Password {
        saved_password: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    PrivateKey {
        private_key_path: Option<String>,
        saved_passphrase: Option<String>,
    },
    None,
    #[serde(rename_all = "camelCase")]
    UsernamePassword {
        username: String,
        saved_password: Option<String>,
    },
}
impl Authentication {
    fn secret(&self) -> Option<&String> {
        match self {
            Self::Password { saved_password } | Self::UsernamePassword { saved_password, .. } => {
                saved_password.as_ref()
            }
            Self::PrivateKey {
                saved_passphrase, ..
            } => saved_passphrase.as_ref(),
            Self::None => None,
        }
    }
    fn set_secret(&mut self, value: Option<String>) {
        match self {
            Self::Password { saved_password } | Self::UsernamePassword { saved_password, .. } => {
                *saved_password = value
            }
            Self::PrivateKey {
                saved_passphrase, ..
            } => *saved_passphrase = value,
            Self::None => (),
        }
    }
    fn view(&self, session: bool) -> Value {
        let mut v = serde_json::to_value(self).unwrap();
        let obj = v.as_object_mut().unwrap();
        obj.remove("savedPassword");
        obj.remove("savedPassphrase");
        obj.insert("hasSavedSecret".into(), json!(self.secret().is_some()));
        obj.insert("hasSessionSecret".into(), json!(session));
        v
    }
    fn identity(&self) -> Self {
        let mut a = self.clone();
        a.set_secret(None);
        a
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Server {
    id: String,
    name: String,
    tag_ids: Vec<String>,
    protocol: String,
    host: String,
    port: u16,
    username: String,
    authentication: Authentication,
    route: Vec<RouteStep>,
    created_at: String,
    updated_at: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Proxy {
    id: String,
    name: String,
    protocol: String,
    host: String,
    port: u16,
    authentication: Authentication,
    created_at: String,
    updated_at: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Catalog {
    schema_version: u32,
    revision: u64,
    tags: Vec<Tag>,
    servers: Vec<Server>,
    proxies: Vec<Proxy>,
}
impl Default for Catalog {
    fn default() -> Self {
        Self {
            schema_version: 2,
            revision: 0,
            tags: vec![],
            servers: vec![],
            proxies: vec![],
        }
    }
}

pub(crate) struct ServerStore {
    path: PathBuf,
    catalog: Catalog,
    sessions: HashMap<String, String>,
    pub error: Option<String>,
}
impl ServerStore {
    pub(crate) fn workspace_names(&self) -> HashMap<String, String> {
        self.catalog
            .servers
            .iter()
            .map(|s| (s.id.clone(), s.name.clone()))
            .collect()
    }
}
fn decode_catalog(bytes: &[u8]) -> Result<Catalog, String> {
    let mut value: Value =
        serde_json::from_slice(bytes).map_err(|_| "服务器目录 JSON 格式不正确".to_string())?;
    match value["schemaVersion"].as_u64() {
        Some(1) => {
            let object = value.as_object_mut().ok_or("服务器目录 JSON 格式不正确")?;
            let tags = object.remove("groups").ok_or("服务器目录缺少分组")?;
            object.insert("tags".into(), tags);
            object.insert("schemaVersion".into(), json!(2));
            for server in object
                .get_mut("servers")
                .and_then(Value::as_array_mut)
                .ok_or("服务器目录缺少服务器列表")?
            {
                let server = server.as_object_mut().ok_or("服务器资料格式不正确")?;
                let group = server.remove("groupId").unwrap_or(Value::Null);
                if !group.is_null() && !group.is_string() {
                    return Err("服务器分组格式不正确".into());
                }
                server.insert(
                    "tagIds".into(),
                    if group.is_null() {
                        json!([])
                    } else {
                        json!([group])
                    },
                );
            }
        }
        Some(2) => (),
        _ => return Err("不支持该服务器目录版本".into()),
    }
    serde_json::from_value(value).map_err(|_| "服务器目录 JSON 格式不正确".into())
}
fn read(path: &std::path::Path) -> Result<Catalog, String> {
    let bytes = fs::read(path).map_err(|_| "无法读取服务器目录文件".to_string())?;
    let c = decode_catalog(&bytes)?;
    validate(&c)?;
    Ok(c)
}
impl ServerStore {
    pub fn open(root: PathBuf) -> Self {
        let path = root.join("servers.json");
        let result = if path.try_exists().unwrap_or(true) {
            read(&path)
        } else {
            Ok(Catalog::default())
        };
        let (catalog, error) = match result {
            Ok(c) => (c, None),
            Err(e) => (
                Catalog::default(),
                Some(format!(
                    "{e}。原文件已保留，请从备份恢复或修复文件后重开应用。"
                )),
            ),
        };
        Self {
            path,
            catalog,
            sessions: HashMap::new(),
            error,
        }
    }
    fn view(&self) -> Value {
        // Construct public views explicitly: disk authentication is never returned.
        json!({"schemaVersion":2,"revision":self.catalog.revision,"tags":self.catalog.tags,
            "servers":self.catalog.servers.iter().map(|s|json!({"id":s.id,"name":s.name,"tagIds":s.tag_ids,
                "protocol":s.protocol,"host":s.host,"port":s.port,"username":s.username,"route":s.route,
                "authentication":s.authentication.view(self.sessions.contains_key(&s.id)),"createdAt":s.created_at,"updatedAt":s.updated_at})).collect::<Vec<_>>(),
            "proxies":self.catalog.proxies.iter().map(|p|json!({"id":p.id,"name":p.name,"protocol":p.protocol,"host":p.host,"port":p.port,
                "authentication":p.authentication.view(self.sessions.contains_key(&p.id)),"createdAt":p.created_at,"updatedAt":p.updated_at})).collect::<Vec<_>>(),
            "error":self.error,"canRecover":read(&self.path.with_extension("json.bak")).is_ok()})
    }
    fn commit(&mut self, mut c: Catalog, sessions: HashMap<String, String>) -> Result<(), String> {
        validate(&c)?;
        c.revision = self
            .catalog
            .revision
            .checked_add(1)
            .ok_or("目录修订已超出范围")?;
        if self.path.exists() {
            let original: Value = serde_json::from_slice(
                &fs::read(&self.path).map_err(|e| format!("无法读取原目录：{e}"))?,
            )
            .map_err(|e| format!("无法读取原目录：{e}"))?;
            let legacy = self.path.with_file_name("servers.v1.json.bak");
            if original["schemaVersion"] == 1 && !legacy.exists() {
                storage::write_json(&legacy, &original)?;
            }
            storage::write_json(&self.path.with_extension("json.bak"), &self.catalog)?;
        }
        storage::write_json(&self.path, &c)?;
        self.catalog = c;
        self.sessions = sessions;
        Ok(())
    }
}

fn endpoint_name(name: &str, h: &str, port: u16, default: u16) -> String {
    if !name.trim().is_empty() {
        return name.trim().into();
    }
    if port == default {
        h.into()
    } else if h.contains(':') {
        format!("[{h}]:{port}")
    } else {
        format!("{h}:{port}")
    }
}
fn timestamp(millis: u64) -> Result<String, String> {
    let time = time::OffsetDateTime::from_unix_timestamp((millis / 1000) as i64)
        .map_err(|e| format!("无法生成资料时间：{e}"))?;
    time.format(&time::format_description::well_known::Rfc3339)
        .map_err(|e| format!("无法生成资料时间：{e}"))
}

impl AppCore {
    pub fn server_catalog(&self) -> Value {
        self.servers.lock().unwrap().view()
    }
    pub fn recover_catalog(&self) -> Result<Value, String> {
        let _operation = self.mutations.lock().unwrap();
        if self.is_closing() {
            return Err("应用正在关闭".into());
        }
        let mut store = self.servers.lock().unwrap();
        if store.error.is_none() {
            return Err("当前目录无需恢复".into());
        }
        let mut backup = read(&store.path.with_extension("json.bak"))?;
        let preserved = store
            .path
            .with_file_name(format!("servers.corrupt-{}.json", self.next_id()));
        fs::copy(&store.path, preserved).map_err(|e| format!("无法保留损坏文件：{e}"))?;
        backup.revision = backup.revision.checked_add(1).ok_or("目录修订已超出范围")?;
        storage::write_json(&store.path, &backup)?;
        store.catalog = backup;
        store.sessions.clear();
        store.error = None;
        let view = store.view();
        drop(store);
        self.changed();
        Ok(view)
    }
}

#[cfg(test)]
mod tests {
    use super::timestamp;

    #[test]
    fn catalog_timestamps_keep_utc_second_precision() {
        for (millis, expected) in [
            (0, "1970-01-01T00:00:00Z"),
            (999, "1970-01-01T00:00:00Z"),
            (951_782_400_000, "2000-02-29T00:00:00Z"),
            (1_709_164_800_123, "2024-02-29T00:00:00Z"),
            (2_147_483_648_000, "2038-01-19T03:14:08Z"),
            (4_107_542_400_000, "2100-03-01T00:00:00Z"),
        ] {
            assert_eq!(timestamp(millis).unwrap(), expected);
        }
        assert!(timestamp(u64::MAX).is_err());
    }
}

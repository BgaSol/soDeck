//! Local server directory. This module does not open network connections.
pub(crate) mod connection_profile;
use crate::{core::now, storage, AppCore};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
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
    group_id: Option<String>,
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
    groups: Vec<Group>,
    servers: Vec<Server>,
    proxies: Vec<Proxy>,
}
impl Default for Catalog {
    fn default() -> Self {
        Self {
            schema_version: 1,
            revision: 0,
            groups: vec![],
            servers: vec![],
            proxies: vec![],
        }
    }
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "camelCase")]
pub enum SecretChange {
    Keep { remember: bool },
    Replace { value: String, remember: bool },
    Clear,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthInput {
    pub r#type: String,
    pub private_key_path: Option<String>,
    pub username: Option<String>,
    pub secret: SecretChange,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerInput {
    pub id: Option<String>,
    pub name: String,
    pub group_id: Option<String>,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub authentication: AuthInput,
    pub route: Vec<RouteStep>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyInput {
    pub id: Option<String>,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub authentication: AuthInput,
}
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum CatalogChange {
    SaveServer { draft: ServerInput },
    SaveProxy { draft: ProxyInput },
    DeleteServer { id: String },
    DeleteProxy { id: String },
    CopyServer { id: String },
    CopyProxy { id: String },
    SaveGroup { id: Option<String>, name: String },
    DeleteGroup { id: String },
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
fn read(path: &std::path::Path) -> Result<Catalog, String> {
    let bytes = fs::read(path).map_err(|_| "无法读取服务器目录文件".to_string())?;
    let c: Catalog =
        serde_json::from_slice(&bytes).map_err(|_| "服务器目录 JSON 格式不正确".to_string())?;
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
        json!({"schemaVersion":1,"revision":self.catalog.revision,"groups":self.catalog.groups,
            "servers":self.catalog.servers.iter().map(|s|json!({"id":s.id,"name":s.name,"groupId":s.group_id,
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
            storage::write_json(&self.path.with_extension("json.bak"), &self.catalog)?;
        }
        storage::write_json(&self.path, &c)?;
        self.catalog = c;
        self.sessions = sessions;
        Ok(())
    }
}

fn host(value: &str) -> Result<String, String> {
    let h = value.trim();
    let h = if h.starts_with('[') && h.ends_with(']') {
        &h[1..h.len() - 1]
    } else {
        h
    };
    if h.is_empty()
        || h.chars()
            .any(|c| c.is_whitespace() || c.is_control() || "/\\@?#".contains(c))
    {
        return Err("请输入主机地址，不包含协议、路径或空格".into());
    }
    Ok(h.into())
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
fn auth(input: &AuthInput, ssh: bool) -> Result<Authentication, String> {
    match (ssh, input.r#type.as_str()) {
        (true, "password") => Ok(Authentication::Password {
            saved_password: None,
        }),
        (true, "privateKey") => Ok(Authentication::PrivateKey {
            private_key_path: input
                .private_key_path
                .as_ref()
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty()),
            saved_passphrase: None,
        }),
        (false, "none") => Ok(Authentication::None),
        (false, "usernamePassword") => {
            let username = input.username.as_deref().unwrap_or("").trim();
            if username.is_empty() {
                return Err("请输入代理用户名".into());
            }
            Ok(Authentication::UsernamePassword {
                username: username.into(),
                saved_password: None,
            })
        }
        _ => Err("认证方式不适用于该记录".into()),
    }
}
fn secret(
    id: &str,
    new: &mut Authentication,
    old: Option<&Authentication>,
    change: &SecretChange,
    sessions: &mut HashMap<String, String>,
) {
    let compatible = old.is_some_and(|old| old.identity() == new.identity());
    let previous = if compatible {
        old.and_then(|a| a.secret().cloned())
            .or_else(|| sessions.get(id).cloned())
    } else {
        None
    };
    let (value, remember) = match change {
        SecretChange::Keep { remember } => (previous, *remember),
        SecretChange::Replace { value, remember } => (Some(value.clone()), *remember),
        SecretChange::Clear => (None, false),
    };
    sessions.remove(id);
    if !matches!(new, Authentication::None) {
        if remember {
            new.set_secret(value);
        } else if let Some(value) = value {
            sessions.insert(id.into(), value);
        }
    }
}
fn validate(c: &Catalog) -> Result<(), String> {
    if c.schema_version != 1 {
        return Err("不支持该服务器目录版本".into());
    }
    let mut ids = HashSet::new();
    for id in c
        .groups
        .iter()
        .map(|g| &g.id)
        .chain(c.servers.iter().map(|s| &s.id))
        .chain(c.proxies.iter().map(|p| &p.id))
    {
        if id.is_empty() || !ids.insert(id) {
            return Err("目录记录 ID 重复或为空".into());
        }
    }
    let mut names = HashSet::new();
    for g in &c.groups {
        if g.name.trim().is_empty() || !names.insert(g.name.to_lowercase()) {
            return Err("分组名称为空或重复".into());
        }
    }
    for s in &c.servers {
        host(&s.host)?;
        if s.port == 0
            || s.username.trim().is_empty()
            || s.name.trim().is_empty()
            || s.protocol != "ssh"
            || !matches!(
                s.authentication,
                Authentication::Password { .. } | Authentication::PrivateKey { .. }
            )
        {
            return Err("服务器字段不完整或认证方式错误".into());
        }
        if s.group_id
            .as_ref()
            .is_some_and(|id| !c.groups.iter().any(|g| g.id == *id))
        {
            return Err("服务器引用了不存在的分组".into());
        }
        let mut steps = HashSet::new();
        let mut ssh = HashSet::new();
        for step in &s.route {
            if step.id().is_empty() || !steps.insert(step.id()) {
                return Err("路径步骤 ID 重复或为空".into());
            }
            match step {
                RouteStep::Ssh { server_id, .. } => {
                    if server_id == &s.id
                        || !ssh.insert(server_id)
                        || !c.servers.iter().any(|r| r.id == *server_id)
                    {
                        return Err("SSH 路径不能引用自身、重复服务器或已删除的服务器".into());
                    }
                }
                RouteStep::Socks5 { proxy_id, .. } => {
                    if !c.proxies.iter().any(|p| p.id == *proxy_id) {
                        return Err("路径引用了不存在的 SOCKS5 代理".into());
                    }
                }
            }
        }
    }
    for p in &c.proxies {
        host(&p.host)?;
        let valid_auth = match &p.authentication {
            Authentication::None => true,
            Authentication::UsernamePassword { username, .. } => !username.trim().is_empty(),
            _ => false,
        };
        if p.name.trim().is_empty() || p.port == 0 || p.protocol != "socks5" || !valid_auth {
            return Err("代理字段不完整或认证方式错误".into());
        }
    }
    Ok(())
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
    pub fn change_catalog(&self, revision: u64, change: CatalogChange) -> Result<Value, String> {
        let _operation = self.mutations.lock().unwrap();
        if self.is_closing() {
            return Err("应用正在退出".into());
        }
        let mut store = self.servers.lock().unwrap();
        if let Some(e) = &store.error {
            return Err(e.clone());
        }
        if store.catalog.revision != revision {
            return Err("目录已更新，请重新读取后再保存。当前输入仍保留。".into());
        }
        let mut c = store.catalog.clone();
        let mut sessions = store.sessions.clone();
        let time = timestamp(now())?;
        let mut selected = None;
        let deleting_proxy = matches!(&change, CatalogChange::DeleteProxy { .. });
        match change {
            CatalogChange::SaveServer { draft: d } => {
                let old = if let Some(id) = &d.id {
                    Some(
                        c.servers
                            .iter()
                            .find(|s| &s.id == id)
                            .ok_or("服务器已不存在")?
                            .clone(),
                    )
                } else {
                    None
                };
                let id = d.id.unwrap_or_else(|| self.next_id());
                let h = host(&d.host)?;
                let mut a = auth(&d.authentication, true)?;
                let same = old
                    .as_ref()
                    .filter(|s| s.host == h && s.port == d.port && s.username == d.username.trim());
                secret(
                    &id,
                    &mut a,
                    same.map(|s| &s.authentication),
                    &d.authentication.secret,
                    &mut sessions,
                );
                let mut route = d.route;
                for step in &mut route {
                    if step.id().is_empty() {
                        step.new_id(self.next_id());
                    }
                }
                let s = Server {
                    id: id.clone(),
                    name: endpoint_name(&d.name, &h, d.port, 22),
                    group_id: d.group_id,
                    protocol: "ssh".into(),
                    host: h,
                    port: d.port,
                    username: d.username.trim().into(),
                    authentication: a,
                    route,
                    created_at: old.map(|s| s.created_at).unwrap_or_else(|| time.clone()),
                    updated_at: time,
                };
                c.servers.retain(|s| s.id != id);
                c.servers.push(s);
                selected = Some(id);
            }
            CatalogChange::SaveProxy { draft: d } => {
                let old = if let Some(id) = &d.id {
                    Some(
                        c.proxies
                            .iter()
                            .find(|p| &p.id == id)
                            .ok_or("代理已不存在")?
                            .clone(),
                    )
                } else {
                    None
                };
                let id = d.id.unwrap_or_else(|| self.next_id());
                let h = host(&d.host)?;
                let mut a = auth(&d.authentication, false)?;
                let same = old.as_ref().filter(|p| p.host == h && p.port == d.port);
                secret(
                    &id,
                    &mut a,
                    same.map(|p| &p.authentication),
                    &d.authentication.secret,
                    &mut sessions,
                );
                let p = Proxy {
                    id: id.clone(),
                    name: endpoint_name(&d.name, &h, d.port, 1080),
                    protocol: "socks5".into(),
                    host: h,
                    port: d.port,
                    authentication: a,
                    created_at: old.map(|p| p.created_at).unwrap_or_else(|| time.clone()),
                    updated_at: time,
                };
                c.proxies.retain(|p| p.id != id);
                c.proxies.push(p);
                selected = Some(id);
            }
            CatalogChange::DeleteServer { id } | CatalogChange::DeleteProxy { id } => {
                if !deleting_proxy && self.workspace_state().server_open(&id) {
                    return Err("请先关闭此服务器的工作区，再删除档案".into());
                }
                if if deleting_proxy {
                    !c.proxies.iter().any(|p| p.id == id)
                } else {
                    !c.servers.iter().any(|s| s.id == id)
                } {
                    return Err("记录已不存在".into());
                }
                let dependencies: Vec<_> = c
                    .servers
                    .iter()
                    .filter(|s| {
                        s.route.iter().any(|r| match r {
                            RouteStep::Ssh { server_id, .. } => server_id == &id,
                            RouteStep::Socks5 { proxy_id, .. } => proxy_id == &id,
                        })
                    })
                    .map(|s| s.name.as_str())
                    .collect();
                if !dependencies.is_empty() {
                    return Err(format!(
                        "请先从以下服务器的路径中移除该记录：{}",
                        dependencies.join("、")
                    ));
                }
                c.servers.retain(|s| s.id != id);
                c.proxies.retain(|p| p.id != id);
                sessions.remove(&id);
            }
            CatalogChange::CopyServer { id } => {
                let mut s = c
                    .servers
                    .iter()
                    .find(|s| s.id == id)
                    .ok_or("服务器已不存在")?
                    .clone();
                s.id = self.next_id();
                s.name.push_str(" 副本");
                s.authentication.set_secret(None);
                for step in &mut s.route {
                    step.new_id(self.next_id());
                }
                s.created_at = time.clone();
                s.updated_at = time;
                selected = Some(s.id.clone());
                c.servers.push(s);
            }
            CatalogChange::CopyProxy { id } => {
                let mut p = c
                    .proxies
                    .iter()
                    .find(|p| p.id == id)
                    .ok_or("代理已不存在")?
                    .clone();
                p.id = self.next_id();
                p.name.push_str(" 副本");
                p.authentication.set_secret(None);
                p.created_at = time.clone();
                p.updated_at = time;
                selected = Some(p.id.clone());
                c.proxies.push(p);
            }
            CatalogChange::SaveGroup { id, name } => {
                let id = match id {
                    Some(id) => {
                        if !c.groups.iter().any(|g| g.id == id) {
                            return Err("分组已不存在".into());
                        }
                        id
                    }
                    None => self.next_id(),
                };
                c.groups.retain(|g| g.id != id);
                c.groups.push(Group {
                    id: id.clone(),
                    name: name.trim().into(),
                });
                selected = Some(id);
            }
            CatalogChange::DeleteGroup { id } => {
                if !c.groups.iter().any(|g| g.id == id) {
                    return Err("分组已不存在".into());
                }
                c.groups.retain(|g| g.id != id);
                for s in &mut c.servers {
                    if s.group_id.as_ref() == Some(&id) {
                        s.group_id = None;
                        s.updated_at = time.clone();
                    }
                }
            }
        }
        store.commit(c, sessions)?;
        self.connections.cancel_stale(&store);
        self.workspace_state().rename(&store.workspace_names());
        let view = store.view();
        drop(store);
        self.changed();
        self.workspace_changed();
        Ok(json!({"catalog":view,"selectedId":selected}))
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

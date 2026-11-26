use super::validation::host;
use super::*;
pub use sodeck_plugin_protocol::servers::{
    RouteInput, SecretChange, ServerPatch, WriteAuthentication,
};

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
    #[serde(default)]
    pub copy_from_id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub tag_ids: Vec<String>,
    #[serde(default)]
    pub new_tag_names: Vec<String>,
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
    PatchServer { id: String, patch: ServerPatch },
    SaveProxy { draft: ProxyInput },
    DeleteServer { id: String },
    DeleteProxy { id: String },
    CopyProxy { id: String },
    SaveTag { id: Option<String>, name: String },
    DeleteTag { id: String },
}

#[derive(Debug)]
pub(crate) struct CatalogError {
    pub code: &'static str,
    pub message: String,
}
impl CatalogError {
    pub(crate) fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}
impl From<String> for CatalogError {
    fn from(message: String) -> Self {
        Self::new("invalidParams", message)
    }
}
impl From<&str> for CatalogError {
    fn from(message: &str) -> Self {
        Self::from(message.to_owned())
    }
}
pub(crate) struct CatalogChanged {
    pub revision: u64,
    pub selected_id: Option<String>,
    pub catalog: Value,
}

impl From<RouteInput> for RouteStep {
    fn from(input: RouteInput) -> RouteStep {
        match input {
            RouteInput::Ssh { server_id } => RouteStep::Ssh {
                id: String::new(),
                server_id,
            },
            RouteInput::Socks5 { proxy_id } => RouteStep::Socks5 {
                id: String::new(),
                proxy_id,
            },
        }
    }
}

pub(crate) trait AuthInputExt {
    fn input(self, default: SecretChange) -> AuthInput;
}
impl AuthInputExt for WriteAuthentication {
    fn input(self, default: SecretChange) -> AuthInput {
        AuthInput {
            r#type: self.kind,
            private_key_path: self.private_key_path,
            username: None,
            secret: self.secret.unwrap_or(default),
        }
    }
}
trait MergePatch {
    fn merge(self, store: &ServerStore, id: &str) -> Result<ServerInput, CatalogError>;
}
impl MergePatch for ServerPatch {
    fn merge(self, store: &ServerStore, id: &str) -> Result<ServerInput, CatalogError> {
        if self.name.is_none()
            && self.host.is_none()
            && self.port.is_none()
            && self.username.is_none()
            && self.tag_ids.is_none()
            && self.route.is_none()
            && self.authentication.is_none()
        {
            return Err(CatalogError::new("invalidParams", "patch 不能为空"));
        }
        let old = store
            .catalog
            .servers
            .iter()
            .find(|s| s.id == id)
            .ok_or_else(|| CatalogError::new("notFound", "服务器已不存在"))?;
        let remember = old.authentication.secret().is_some();
        let authentication = match self.authentication {
            Some(a) => {
                let input = a.input(SecretChange::Keep { remember });
                if matches!(input.secret, SecretChange::Keep { .. })
                    && auth(&input, true)?.identity() != old.authentication.identity()
                {
                    return Err(CatalogError::new(
                        "invalidParams",
                        "认证配置变化时请明确替换或清除凭据",
                    ));
                }
                input
            }
            None => AuthInput {
                r#type: match old.authentication {
                    Authentication::PrivateKey { .. } => "privateKey",
                    _ => "password",
                }
                .into(),
                private_key_path: match &old.authentication {
                    Authentication::PrivateKey {
                        private_key_path, ..
                    } => private_key_path.clone(),
                    _ => None,
                },
                username: None,
                secret: SecretChange::Keep { remember },
            },
        };
        Ok(ServerInput {
            id: Some(id.into()),
            copy_from_id: None,
            name: self.name.unwrap_or_else(|| old.name.clone()),
            host: self.host.unwrap_or_else(|| old.host.clone()),
            port: self.port.unwrap_or(old.port),
            username: self.username.unwrap_or_else(|| old.username.clone()),
            tag_ids: self.tag_ids.unwrap_or_else(|| old.tag_ids.clone()),
            new_tag_names: vec![],
            route: self
                .route
                .map(|r| r.into_iter().map(RouteStep::from).collect())
                .unwrap_or_else(|| old.route.clone()),
            authentication,
        })
    }
}

fn save_server(
    core: &AppCore,
    c: &mut Catalog,
    sessions: &mut HashMap<String, String>,
    d: ServerInput,
    time: String,
) -> Result<String, CatalogError> {
    if d.copy_from_id.is_some()
        && (d.id.is_some() || !matches!(d.authentication.secret, SecretChange::Keep { .. }))
    {
        return Err("凭据来源只适用于新增服务器并沿用凭据".into());
    }
    let old = if let Some(id) = &d.id {
        Some(
            c.servers
                .iter()
                .find(|s| &s.id == id)
                .ok_or_else(|| CatalogError::new("notFound", "服务器已不存在"))?
                .clone(),
        )
    } else {
        None
    };
    let id = d.id.clone().unwrap_or_else(|| core.next_id());
    let h = host(&d.host)?;
    let mut a = auth(&d.authentication, true)?;

    let source = if let Some(source_id) = &d.copy_from_id {
        let source = c
            .servers
            .iter()
            .find(|s| &s.id == source_id)
            .ok_or("原服务器已不存在，请输入新凭据或选择不沿用原凭据")?;
        if source.authentication.identity() != a.identity() {
            return Err("原服务器认证配置已变化，请输入新凭据或选择不沿用原凭据".into());
        }
        if source.authentication.secret().is_none() && !sessions.contains_key(source_id) {
            return Err("原服务器凭据已不可用，请输入新凭据或选择不沿用原凭据".into());
        }
        Some(source)
    } else {
        old.as_ref()
    };
    secret(
        &id,
        &mut a,
        source.map(|s| (s.id.as_str(), &s.authentication)),
        &d.authentication.secret,
        sessions,
    );
    let mut route = d.route;
    for step in &mut route {
        if step.id().is_empty() {
            step.new_id(core.next_id());
        }
    }
    let mut tag_ids = d.tag_ids;
    let mut selected_tags = HashSet::new();
    if tag_ids
        .iter()
        .any(|id| !selected_tags.insert(id.clone()) || !c.tags.iter().any(|tag| tag.id == *id))
    {
        return Err("服务器引用了不存在或重复的标签".into());
    }
    for name in d.new_tag_names {
        let name = name.trim();
        if name.is_empty() {
            return Err("标签名称不能为空".into());
        }
        let tag_id = match c
            .tags
            .iter()
            .find(|tag| tag.name.to_lowercase() == name.to_lowercase())
        {
            Some(tag) => tag.id.clone(),
            None => {
                let id = core.next_id();
                c.tags.push(Tag {
                    id: id.clone(),
                    name: name.into(),
                });
                id
            }
        };
        if selected_tags.insert(tag_id.clone()) {
            tag_ids.push(tag_id);
        }
    }
    let s = Server {
        id: id.clone(),
        name: endpoint_name(&d.name, &h, d.port, 22),
        tag_ids,
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
    Ok(id)
}

impl AppCore {
    pub fn change_catalog(&self, revision: u64, change: CatalogChange) -> Result<Value, String> {
        self.apply_catalog_change(revision, change, None)
            .map(|r| json!({"catalog":r.catalog,"selectedId":r.selected_id}))
            .map_err(|e| e.message)
    }
    pub(crate) fn apply_catalog_change(
        &self,
        revision: u64,
        change: CatalogChange,
        source: Option<(&str, &str)>,
    ) -> Result<CatalogChanged, CatalogError> {
        let _operation = self.mutations.lock().unwrap();
        if self.is_closing() {
            return Err("应用正在退出".into());
        }
        let mut store = self.servers.lock().unwrap();
        if let Some((plugin, instance)) = source {
            self.plugin_instance(plugin, instance)
                .ok_or_else(|| CatalogError::new("staleInstance", "插件实例不可用"))?;
        }

        if let Some(e) = &store.error {
            return Err(CatalogError::new("catalogUnavailable", e.clone()));
        }
        if store.catalog.revision != revision {
            return Err(CatalogError::new(
                "conflict",
                "目录已更新，请重新读取后再保存。当前输入仍保留。",
            ));
        }
        let mut c = store.catalog.clone();
        let mut sessions = store.sessions.clone();
        let time = timestamp(now())?;
        let mut selected = None;
        let change = match change {
            CatalogChange::PatchServer { id, patch } => CatalogChange::SaveServer {
                draft: patch.merge(&store, &id)?,
            },
            change => change,
        };
        let deleting_proxy = matches!(&change, CatalogChange::DeleteProxy { .. });
        match change {
            CatalogChange::SaveServer { draft } => {
                selected = Some(save_server(self, &mut c, &mut sessions, draft, time)?);
            }
            CatalogChange::PatchServer { .. } => unreachable!("patch normalized before mutation"),
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
                    same.map(|p| (p.id.as_str(), &p.authentication)),
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
                if !deleting_proxy && self.connections.target_pending(&id) {
                    return Err(CatalogError::new(
                        "inUse",
                        "请先取消连接并等待结束，再删除服务器资料",
                    ));
                }
                if !deleting_proxy && self.workspace_state().server_open(&id) {
                    return Err(CatalogError::new(
                        "inUse",
                        "请先关闭此服务器的工作区，再删除档案",
                    ));
                }
                if if deleting_proxy {
                    !c.proxies.iter().any(|p| p.id == id)
                } else {
                    !c.servers.iter().any(|s| s.id == id)
                } {
                    return Err(CatalogError::new("notFound", "记录已不存在"));
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
                    return Err(CatalogError::new(
                        "referenced",
                        format!(
                            "请先从以下服务器的路径中移除该记录：{}",
                            dependencies.join("、")
                        ),
                    ));
                }
                c.servers.retain(|s| s.id != id);
                c.proxies.retain(|p| p.id != id);
                sessions.remove(&id);
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
            CatalogChange::SaveTag { id, name } => {
                let id = match id {
                    Some(id) => {
                        if !c.tags.iter().any(|g| g.id == id) {
                            return Err("标签已不存在".into());
                        }
                        id
                    }
                    None => self.next_id(),
                };
                c.tags.retain(|g| g.id != id);
                c.tags.push(Tag {
                    id: id.clone(),
                    name: name.trim().into(),
                });
                selected = Some(id);
            }
            CatalogChange::DeleteTag { id } => {
                if !c.tags.iter().any(|g| g.id == id) {
                    return Err("标签已不存在".into());
                }
                c.tags.retain(|g| g.id != id);
                for s in &mut c.servers {
                    if s.tag_ids.contains(&id) {
                        s.tag_ids.retain(|tag| tag != &id);
                        s.updated_at = time.clone();
                    }
                }
            }
        }
        validate(&c)?;
        store
            .commit(c, sessions)
            .map_err(|message| CatalogError::new("persistenceFailed", message))?;
        self.catalog_committed(&store);
        let view = store.view();
        drop(store);
        drop(_operation);
        if let Some(source) = source {
            self.log(Some(source),"info","服务器目录已更新",Some(json!({"catalogRevision":view["revision"].as_u64().unwrap().to_string(),"serverId":selected})));
        }
        self.publish_catalog_changed();
        Ok(CatalogChanged {
            revision: view["revision"].as_u64().expect("catalog revision"),
            catalog: view,
            selected_id: selected,
        })
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
    old: Option<(&str, &Authentication)>,
    change: &SecretChange,
    sessions: &mut HashMap<String, String>,
) {
    let compatible = old.is_some_and(|(_, old)| old.identity() == new.identity());
    let previous = if compatible {
        old.and_then(|(source_id, a)| effective_secret(sessions, source_id, a))
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

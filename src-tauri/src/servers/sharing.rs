use super::*;
use crate::connections::private_key;
use base64::{engine::general_purpose::STANDARD, Engine};
use std::path::Path;

pub const SHARE_LIMIT: usize = 8 * 1024 * 1024;
const PREFIX: &str = "SODECK1:";

#[derive(Debug, Serialize)]
pub struct ShareError {
    pub code: &'static str,
    pub message: String,
}
impl ShareError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}
fn invalid() -> ShareError {
    ShareError::new("invalidShare", "分享码不完整或资料无效，请重新复制")
}
fn too_large() -> ShareError {
    ShareError::new("shareTooLarge", "分享内容超过 8 MiB，请改为逐台分享")
}
fn changed() -> ShareError {
    ShareError::new(
        "catalogChanged",
        "目录已更新，请重新预览后再导入。分享码仍保留。",
    )
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
enum ShareAuth {
    Password {
        password: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    PrivateKey {
        key_id: Option<String>,
        passphrase: Option<String>,
    },
    None,
    #[serde(rename_all = "camelCase")]
    UsernamePassword {
        username: String,
        password: Option<String>,
    },
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum Step {
    Ssh { id: String },
    Socks5 { id: String },
}
impl Step {
    fn id(&self) -> &str {
        match self {
            Self::Ssh { id } | Self::Socks5 { id } => id,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SharedServer {
    id: String,
    name: String,
    host: String,
    port: u16,
    username: String,
    tags: Vec<String>,
    authentication: ShareAuth,
    route: Vec<Step>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SharedProxy {
    id: String,
    name: String,
    host: String,
    port: u16,
    authentication: ShareAuth,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SharedKey {
    id: String,
    content: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Bundle {
    scope: Scope,
    targets: Vec<String>,
    servers: Vec<SharedServer>,
    proxies: Vec<SharedProxy>,
    keys: Vec<SharedKey>,
}
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
enum Scope {
    Single,
    All,
}

fn decode(code: &str) -> Result<Bundle, ShareError> {
    if code.len() > SHARE_LIMIT {
        return Err(too_large());
    }
    let code: String = code.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    if !code.starts_with(PREFIX) {
        return Err(if code.starts_with("SODECK") {
            ShareError::new(
                "unsupportedVersion",
                "分享码版本不支持，请更新 SoDeck 后再导入",
            )
        } else {
            invalid()
        });
    }
    let bytes = STANDARD
        .decode(&code[PREFIX.len()..])
        .map_err(|_| invalid())?;
    let b: Bundle = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    b.validate()?;
    Ok(b)
}
impl Bundle {
    fn validate(&self) -> Result<(), ShareError> {
        let mut ids = HashSet::new();
        for id in self
            .servers
            .iter()
            .map(|s| &s.id)
            .chain(self.proxies.iter().map(|p| &p.id))
        {
            if id.is_empty() || id.len() > 256 || !ids.insert(id) {
                return Err(invalid());
            }
        }
        let servers: HashSet<_> = self.servers.iter().map(|s| &s.id).collect();
        let proxies: HashSet<_> = self.proxies.iter().map(|s| &s.id).collect();
        let targets: HashSet<_> = self.targets.iter().collect();
        if targets.is_empty()
            || targets.len() != self.targets.len()
            || targets.iter().any(|id| !servers.contains(id))
            || (self.scope == Scope::Single && targets.len() != 1)
            || (self.scope == Scope::All && targets.len() != servers.len())
        {
            return Err(invalid());
        }
        let mut needed = targets.clone();
        for s in self.servers.iter().filter(|s| targets.contains(&s.id)) {
            for step in &s.route {
                needed.insert(match step {
                    Step::Ssh { id } | Step::Socks5 { id } => id,
                });
            }
        }
        if needed != ids {
            return Err(invalid());
        }
        let mut keys = HashMap::new();
        for k in &self.keys {
            if k.id.is_empty()
                || k.id.len() > 256
                || keys.insert(&k.id, &k.content).is_some()
                || private_key::inspect(&k.content).is_err()
            {
                return Err(invalid());
            }
        }
        let mut used_keys = HashSet::new();
        let valid_auth = |a: &ShareAuth, ssh: bool, used: &mut HashSet<String>| -> bool {
            let secret = match a {
                ShareAuth::Password { password } if ssh => password,
                ShareAuth::PrivateKey { key_id, passphrase } if ssh => {
                    if let Some(id) = key_id {
                        if !keys.contains_key(id) {
                            return false;
                        }
                        used.insert(id.clone());
                    }
                    passphrase
                }
                ShareAuth::None if !ssh => return true,
                ShareAuth::UsernamePassword { username, password }
                    if !ssh && !username.trim().is_empty() && !username.contains('\0') =>
                {
                    password
                }
                _ => return false,
            };
            secret
                .as_ref()
                .is_none_or(|s| !s.contains('\0') && s.len() <= 64 * 1024)
        };
        for s in &self.servers {
            let mut ssh = HashSet::new();
            if s.name.trim().is_empty()
                || s.username.trim().is_empty()
                || s.name.contains('\0')
                || s.username.contains('\0')
                || validation::host(&s.host).is_err()
                || s.port == 0
                || !valid_auth(&s.authentication, true, &mut used_keys)
                || s.tags
                    .iter()
                    .any(|t| t.trim().is_empty() || t.contains('\0'))
            {
                return Err(invalid());
            }
            for step in &s.route {
                match step {
                    Step::Ssh { id } if id != &s.id && servers.contains(id) && ssh.insert(id) => (),
                    Step::Socks5 { id } if proxies.contains(id) => (),
                    _ => return Err(invalid()),
                }
            }
        }
        for p in &self.proxies {
            if p.name.trim().is_empty()
                || p.name.contains('\0')
                || validation::host(&p.host).is_err()
                || p.port == 0
                || !valid_auth(&p.authentication, false, &mut used_keys)
            {
                return Err(invalid());
            }
        }
        if used_keys.len() != keys.len() {
            return Err(invalid());
        }
        Ok(())
    }
}

fn unique_name(original: &str, names: &mut HashSet<String>) -> String {
    let name = original.trim();
    if names.insert(name.into()) {
        return name.into();
    }
    let mut n = 1;
    loop {
        let candidate = if n == 1 {
            format!("{name}（导入）")
        } else {
            format!("{name}（导入 {n}）")
        };
        if names.insert(candidate.clone()) {
            return candidate;
        }
        n += 1;
    }
}

struct Candidate {
    catalog: Catalog,
    targets: Vec<String>,
    added: Vec<String>,
    summary: Value,
}
fn candidate(
    base: &Catalog,
    b: &Bundle,
    mut next: impl FnMut() -> String,
    paths: &HashMap<String, String>,
) -> Result<Candidate, ShareError> {
    let mut c = base.clone();
    let ids: HashMap<_, _> = b
        .servers
        .iter()
        .map(|s| &s.id)
        .chain(b.proxies.iter().map(|p| &p.id))
        .map(|id| (id.clone(), next()))
        .collect();
    let mut names: HashSet<_> = c
        .servers
        .iter()
        .map(|s| s.name.clone())
        .chain(c.proxies.iter().map(|s| s.name.clone()))
        .collect();
    let mut tag_names: HashMap<_, _> = c
        .tags
        .iter()
        .map(|t| (t.name.to_lowercase(), t.id.clone()))
        .collect();
    let time = timestamp(now()).map_err(|_| invalid())?;
    let auth = |a: &ShareAuth| match a {
        ShareAuth::Password { password } => Authentication::Password {
            saved_password: password.clone(),
        },
        ShareAuth::PrivateKey { key_id, passphrase } => Authentication::PrivateKey {
            private_key_path: key_id.as_ref().map(|id| paths[id].clone()),
            saved_passphrase: passphrase.clone(),
        },
        ShareAuth::None => Authentication::None,
        ShareAuth::UsernamePassword { username, password } => Authentication::UsernamePassword {
            username: username.trim().into(),
            saved_password: password.clone(),
        },
    };
    let targets_set: HashSet<_> = b.targets.iter().collect();
    let encrypted: HashMap<_, _> = b
        .keys
        .iter()
        .map(|k| (&k.id, private_key::inspect(&k.content).unwrap_or(false)))
        .collect();
    let mut rows = Vec::new();
    let mut display: HashMap<_, _> = HashMap::new();
    for s in &b.servers {
        display.insert(s.id.clone(), unique_name(&s.name, &mut names));
    }
    for p in &b.proxies {
        display.insert(p.id.clone(), unique_name(&p.name, &mut names));
    }
    for s in &b.servers {
        let mut tags = Vec::new();
        for name in &s.tags {
            let name = name.trim();
            let lower = name.to_lowercase();
            let id = tag_names.entry(lower).or_insert_with(|| {
                let id = next();
                c.tags.push(Tag {
                    id: id.clone(),
                    name: name.into(),
                });
                id
            });
            if !tags.contains(id) {
                tags.push(id.clone());
            }
        }
        let missing = match &s.authentication {
            ShareAuth::Password { password } => password.is_none(),
            ShareAuth::PrivateKey { key_id, passphrase } => key_id
                .as_ref()
                .is_none_or(|id| encrypted[id] && passphrase.as_ref().is_none_or(|p| p.is_empty())),
            _ => false,
        };
        rows.push(json!({"name":display[&s.id],"originalName":s.name,"endpoint":format!("{}@{}:{}",s.username,s.host,s.port),"target":targets_set.contains(&s.id),"route":s.route.iter().map(|r|display[r.id()].clone()).collect::<Vec<_>>(),"needsCredentials":missing}));
        c.servers.push(Server {
            id: ids[&s.id].clone(),
            name: display[&s.id].clone(),
            host: validation::host(&s.host).map_err(|_| invalid())?,
            port: s.port,
            username: s.username.trim().into(),
            tag_ids: tags,
            protocol: "ssh".into(),
            authentication: auth(&s.authentication),
            route: s
                .route
                .iter()
                .map(|step| match step {
                    Step::Ssh { id } => RouteStep::Ssh {
                        id: next(),
                        server_id: ids[id].clone(),
                    },
                    Step::Socks5 { id } => RouteStep::Socks5 {
                        id: next(),
                        proxy_id: ids[id].clone(),
                    },
                })
                .collect(),
            created_at: time.clone(),
            updated_at: time.clone(),
        });
    }
    for p in &b.proxies {
        let missing = matches!(
            &p.authentication,
            ShareAuth::UsernamePassword { password: None, .. }
        );
        rows.push(json!({"name":display[&p.id],"originalName":p.name,"endpoint":format!("{}:{}",p.host,p.port),"target":false,"route":[],"needsCredentials":missing}));
        c.proxies.push(Proxy {
            id: ids[&p.id].clone(),
            name: display[&p.id].clone(),
            host: validation::host(&p.host).map_err(|_| invalid())?,
            port: p.port,
            protocol: "socks5".into(),
            authentication: auth(&p.authentication),
            created_at: time.clone(),
            updated_at: time.clone(),
        });
    }
    validate(&c).map_err(|_| invalid())?;
    let targets = b.targets.iter().map(|id| ids[id].clone()).collect();
    let added = b
        .servers
        .iter()
        .map(|s| ids[&s.id].clone())
        .chain(b.proxies.iter().map(|p| ids[&p.id].clone()))
        .collect();
    Ok(Candidate {
        catalog: c,
        targets,
        added,
        summary: json!({"catalogRevision":base.revision,"targetCount":b.targets.len(),"serverCount":b.servers.len(),"proxyCount":b.proxies.len(),"keyCount":b.keys.len(),"rows":rows}),
    })
}

impl AppCore {
    pub fn export_server_share(
        &self,
        revision: u64,
        server_id: Option<String>,
    ) -> Result<Value, ShareError> {
        let (catalog, sessions) = {
            let store = self.servers.lock().unwrap();
            if store.error.is_some() {
                return Err(ShareError::new("catalogUnavailable", "请先恢复服务器目录"));
            }
            if store.catalog.revision != revision {
                return Err(changed());
            }
            (store.catalog.clone(), store.sessions.clone())
        };
        let source: HashMap<_, _> = catalog.servers.iter().map(|s| (s.id.clone(), s)).collect();
        let all = server_id.is_none();
        let targets = match server_id {
            Some(id) if source.contains_key(&id) => vec![id],
            None => catalog.servers.iter().map(|s| s.id.clone()).collect(),
            _ => return Err(invalid()),
        };
        if targets.is_empty() {
            return Err(ShareError::new("emptyShare", "暂无可分享的服务器"));
        }
        let scope = if all { Scope::All } else { Scope::Single };
        let mut selected: Vec<(String, Vec<RouteStep>)> = Vec::new();
        if all {
            selected.extend(
                catalog
                    .servers
                    .iter()
                    .map(|s| (s.id.clone(), s.route.clone())),
            );
        } else {
            let t = source[&targets[0]];
            for (i, step) in t.route.iter().enumerate() {
                if let RouteStep::Ssh { server_id, .. } = step {
                    selected.push((server_id.clone(), t.route[..i].to_vec()));
                }
            }
            selected.push((t.id.clone(), t.route.clone()));
        }
        let mut proxy_ids = HashSet::new();
        for (_, route) in &selected {
            for step in route {
                if let RouteStep::Socks5 { proxy_id, .. } = step {
                    proxy_ids.insert(proxy_id.clone());
                }
            }
        }
        let mut mapping = HashMap::new();
        for (i, (id, _)) in selected.iter().enumerate() {
            mapping.insert(id.clone(), format!("s{i}"));
        }
        let proxies: Vec<_> = catalog
            .proxies
            .iter()
            .filter(|p| proxy_ids.contains(&p.id))
            .collect();
        for (i, p) in proxies.iter().enumerate() {
            mapping.insert(p.id.clone(), format!("p{i}"));
        }
        let mut b = Bundle {
            scope,
            targets: targets.iter().map(|id| mapping[id].clone()).collect(),
            servers: vec![],
            proxies: vec![],
            keys: vec![],
        };
        let mut files = HashMap::<String, String>::new();
        let mut content_ids = HashMap::<String, String>::new();
        let mut key_bytes = 0usize;
        let mut export_auth = |id: &str, a: &Authentication| -> Result<ShareAuth, ShareError> {
            let secret = effective_secret(&sessions, id, a);
            Ok(match a {
                Authentication::Password { .. } => ShareAuth::Password { password: secret },
                Authentication::PrivateKey {
                    private_key_path, ..
                } => {
                    let key_id = if let Some(path) = private_key_path {
                        if let Some(id) = files.get(path) {
                            Some(id.clone())
                        } else {
                            let content = private_key::read(Path::new(path)).map_err(|_| {
                                ShareError::new(
                                    "keyReadFailed",
                                    "无法读取私钥文件或文件超过 1 MiB，请检查服务器私钥配置",
                                )
                            })?;
                            private_key::inspect(&content).map_err(|_| {
                                ShareError::new(
                                    "keyReadFailed",
                                    "私钥格式无效或不受支持，请检查服务器私钥配置",
                                )
                            })?;
                            if !content_ids.contains_key(&content)
                                && key_bytes + content.len() > SHARE_LIMIT * 3 / 4
                            {
                                return Err(too_large());
                            }
                            let id = content_ids
                                .entry(content.clone())
                                .or_insert_with(|| {
                                    let id = format!("k{}", b.keys.len());
                                    key_bytes += content.len();
                                    b.keys.push(SharedKey {
                                        id: id.clone(),
                                        content,
                                    });
                                    id
                                })
                                .clone();
                            files.insert(path.clone(), id.clone());
                            Some(id)
                        }
                    } else {
                        None
                    };
                    ShareAuth::PrivateKey {
                        key_id,
                        passphrase: secret,
                    }
                }
                Authentication::None => ShareAuth::None,
                Authentication::UsernamePassword { username, .. } => ShareAuth::UsernamePassword {
                    username: username.clone(),
                    password: secret,
                },
            })
        };
        let tag_names: HashMap<_, _> = catalog.tags.iter().map(|t| (&t.id, &t.name)).collect();
        for (id, route) in selected {
            let s = source[&id];
            b.servers.push(SharedServer {
                id: mapping[&id].clone(),
                name: s.name.clone(),
                host: s.host.clone(),
                port: s.port,
                username: s.username.clone(),
                tags: s.tag_ids.iter().map(|id| tag_names[id].clone()).collect(),
                authentication: export_auth(&id, &s.authentication).map_err(|mut e| {
                    e.message = format!("「{}」：{}", s.name, e.message);
                    e
                })?,
                route: route
                    .iter()
                    .map(|r| match r {
                        RouteStep::Ssh { server_id, .. } => Step::Ssh {
                            id: mapping[server_id].clone(),
                        },
                        RouteStep::Socks5 { proxy_id, .. } => Step::Socks5 {
                            id: mapping[proxy_id].clone(),
                        },
                    })
                    .collect(),
            });
        }
        for p in proxies {
            b.proxies.push(SharedProxy {
                id: mapping[&p.id].clone(),
                name: p.name.clone(),
                host: p.host.clone(),
                port: p.port,
                authentication: export_auth(&p.id, &p.authentication)?,
            });
        }
        b.validate()?;
        let bytes = serde_json::to_vec(&b).map_err(|_| invalid())?;
        if bytes.len().div_ceil(3) * 4 + PREFIX.len() > SHARE_LIMIT {
            return Err(too_large());
        }
        let code = format!("{PREFIX}{}", STANDARD.encode(bytes));
        let store = self.servers.lock().unwrap();
        if store.error.is_some() || store.catalog.revision != revision || store.sessions != sessions
        {
            return Err(changed());
        }
        let paths = b
            .keys
            .iter()
            .map(|k| (k.id.clone(), String::new()))
            .collect();
        let summary = candidate(&Catalog::default(), &b, || self.next_id(), &paths)?.summary;
        Ok(json!({"code":code,"summary":summary}))
    }

    pub fn preview_server_share(&self, code: &str) -> Result<Value, ShareError> {
        let b = decode(code)?;
        let store = self.servers.lock().unwrap();
        if store.error.is_some() {
            return Err(ShareError::new("catalogUnavailable", "请先恢复服务器目录"));
        }
        let paths = b
            .keys
            .iter()
            .map(|k| (k.id.clone(), String::new()))
            .collect();
        Ok(candidate(&store.catalog, &b, || self.next_id(), &paths)?.summary)
    }

    pub fn import_server_share(&self, revision: u64, code: &str) -> Result<Value, ShareError> {
        let b = decode(code)?;
        let root = self
            .servers
            .lock()
            .unwrap()
            .path
            .parent()
            .unwrap()
            .to_path_buf();
        let contents: Vec<_> = b.keys.iter().map(|k| k.content.as_str()).collect();
        let mut keys = managed_keys::Prepared::new(&root, &contents).map_err(|_| {
            ShareError::new("persistenceFailed", "无法准备本机私钥文件，本次未导入")
        })?;
        let paths = b
            .keys
            .iter()
            .enumerate()
            .map(|(i, k)| (k.id.clone(), keys.path(i)))
            .collect();
        let operation = self.mutations.lock().unwrap();
        if self.is_closing() {
            return Err(ShareError::new("closing", "应用正在退出，请稍后导入"));
        }
        let mut store = self.servers.lock().unwrap();
        if store.error.is_some() {
            return Err(ShareError::new("catalogUnavailable", "请先恢复服务器目录"));
        }
        if store.catalog.revision != revision {
            return Err(changed());
        }
        let c = candidate(&store.catalog, &b, || self.next_id(), &paths)?;
        keys.publish()
            .map_err(|_| ShareError::new("persistenceFailed", "无法保存本机私钥，本次未导入"))?;
        let sessions = store.sessions.clone();
        store
            .commit(c.catalog, sessions)
            .map_err(|_| ShareError::new("persistenceFailed", "无法保存服务器目录，本次未导入"))?;
        keys.accept();
        self.catalog_committed(&store);
        let result = json!({"catalog":store.view(),"targetIds":c.targets,"addedIds":c.added,"summary":c.summary});
        drop(store);
        drop(operation);
        self.publish_catalog_changed();
        Ok(result)
    }
}

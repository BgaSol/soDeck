//! Core-only immutable snapshots. Never serialize or Debug authentication material.
use super::*;
use sha2::{Digest, Sha256};

#[derive(Clone)]
pub(crate) struct Profile {
    pub server_id: String,
    pub nodes: Vec<Node>,
    pub identity: String,
}
#[derive(Clone)]
pub(crate) struct Node {
    pub record_id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: Auth,
    pub trust_scope: String,
    pub save_credentials: Option<bool>,
    has_saved_secret: bool,
}
#[derive(Clone, PartialEq)]
pub(crate) enum Auth {
    Password(Option<String>),
    Key {
        path: Option<String>,
        passphrase: Option<String>,
    },
    Socks {
        username: Option<String>,
        password: Option<String>,
    },
}
impl Node {
    pub fn is_ssh(&self) -> bool {
        !matches!(self.auth, Auth::Socks { .. })
    }
    pub fn endpoint(&self) -> String {
        let host = if self.host.contains(':') {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };
        format!("{host}:{}", self.port)
    }
    fn identity(&self) -> Value {
        let authentication = match &self.auth {
            Auth::Password(_) => json!({"type":"password"}),
            Auth::Key { path, .. } => json!({"type":"privateKey", "path":path}),
            Auth::Socks { username, .. } => json!({"type":"socks5", "username":username}),
        };
        json!({"protocol":if self.is_ssh() {"ssh"} else {"socks5"},"host":self.host,"port":self.port,"username":self.username,"authentication":authentication})
    }
}
impl Profile {
    pub fn matches(&self, other: &Self) -> bool {
        self.identity == other.identity
            && self.nodes.len() == other.nodes.len()
            && self.nodes.iter().zip(&other.nodes).all(|(a, b)| {
                a.record_id == b.record_id
                    && a.auth == b.auth
                    && a.has_saved_secret == b.has_saved_secret
            })
    }
}
fn hash(value: &Value) -> String {
    format!("{:x}", Sha256::digest(value.to_string().as_bytes()))
}
impl ServerStore {
    pub(crate) fn connection_snapshot(&self, id: &str) -> Result<Profile, String> {
        if self.error.is_some() {
            return Err("请先恢复服务器目录".into());
        }
        let target = self
            .catalog
            .servers
            .iter()
            .find(|s| s.id == id)
            .ok_or("服务器资料已不存在")?;
        let ssh = |s: &Server| -> Result<Node, String> {
            let secret = self
                .sessions
                .get(&s.id)
                .cloned()
                .or_else(|| s.authentication.secret().cloned());
            let auth = match &s.authentication {
                Authentication::Password { .. } => Auth::Password(secret),
                Authentication::PrivateKey {
                    private_key_path, ..
                } => Auth::Key {
                    path: private_key_path.clone(),
                    passphrase: secret,
                },
                _ => return Err("SSH 认证方式无效".into()),
            };
            Ok(Node {
                record_id: s.id.clone(),
                name: s.name.clone(),
                host: s.host.clone(),
                port: s.port,
                username: s.username.clone(),
                auth,
                trust_scope: String::new(),
                save_credentials: None,
                has_saved_secret: s.authentication.secret().is_some(),
            })
        };
        let mut nodes = Vec::new();
        // References contribute only their own endpoint/authentication, never their route.
        for step in &target.route {
            nodes.push(match step {
                RouteStep::Ssh { server_id, .. } => ssh(self
                    .catalog
                    .servers
                    .iter()
                    .find(|s| &s.id == server_id)
                    .ok_or("SSH 跳板资料已不存在")?)?,
                RouteStep::Socks5 { proxy_id, .. } => {
                    let p = self
                        .catalog
                        .proxies
                        .iter()
                        .find(|p| &p.id == proxy_id)
                        .ok_or("SOCKS5 代理资料已不存在")?;
                    let (username, password) = match &p.authentication {
                        Authentication::None => (None, None),
                        Authentication::UsernamePassword { username, .. } => (
                            Some(username.clone()),
                            self.sessions
                                .get(proxy_id)
                                .cloned()
                                .or_else(|| p.authentication.secret().cloned()),
                        ),
                        _ => return Err("SOCKS5 认证方式无效".into()),
                    };
                    Node {
                        record_id: p.id.clone(),
                        name: p.name.clone(),
                        host: p.host.clone(),
                        port: p.port,
                        username: String::new(),
                        auth: Auth::Socks { username, password },
                        trust_scope: String::new(),
                        save_credentials: None,
                        has_saved_secret: p.authentication.secret().is_some(),
                    }
                }
            });
        }
        nodes.push(ssh(target)?);
        let mut prefix = Vec::new();
        for node in &mut nodes {
            // Trust is scoped by the path, endpoint and account, not local record IDs/names.
            let account = match &node.auth {
                Auth::Socks { username, .. } => username.as_deref().unwrap_or(""),
                _ => node.username.as_str(),
            };
            prefix.push(json!({"protocol":if node.is_ssh() {"ssh"} else {"socks5"},"host":node.host,"port":node.port,"username":account}));
            node.trust_scope = hash(&json!(prefix));
        }
        let identity = hash(&json!(nodes.iter().map(Node::identity).collect::<Vec<_>>()));
        Ok(Profile {
            server_id: id.into(),
            nodes,
            identity,
        })
    }

    // Called only after the whole route authenticates and admission validates its original snapshot.
    pub(crate) fn save_connection_credentials(
        &mut self,
        profile: &Profile,
    ) -> Result<bool, String> {
        if !profile
            .nodes
            .iter()
            .any(|n| n.save_credentials == Some(true))
        {
            return Ok(false);
        }
        let mut catalog = self.catalog.clone();
        let mut sessions = self.sessions.clone();
        let time = timestamp();
        for node in profile
            .nodes
            .iter()
            .filter(|n| n.save_credentials == Some(true))
        {
            let (authentication, updated_at) = if node.is_ssh() {
                let record = catalog
                    .servers
                    .iter_mut()
                    .find(|s| s.id == node.record_id)
                    .ok_or("服务器资料已不存在")?;
                (&mut record.authentication, &mut record.updated_at)
            } else {
                let record = catalog
                    .proxies
                    .iter_mut()
                    .find(|p| p.id == node.record_id)
                    .ok_or("代理资料已不存在")?;
                (&mut record.authentication, &mut record.updated_at)
            };
            let secret = match &node.auth {
                Auth::Password(value)
                | Auth::Socks {
                    password: value, ..
                } => value.clone(),
                Auth::Key { path, passphrase } => {
                    if let Authentication::PrivateKey {
                        private_key_path, ..
                    } = authentication
                    {
                        *private_key_path = path.clone();
                    }
                    passphrase.clone().filter(|p| !p.is_empty())
                }
            };
            authentication.set_secret(secret);
            *updated_at = time.clone();
            sessions.remove(&node.record_id);
        }
        self.commit(catalog, sessions)?;
        Ok(true)
    }
}

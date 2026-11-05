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
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: Auth,
    pub trust_scope: String,
}
#[derive(Clone)]
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
                name: s.name.clone(),
                host: s.host.clone(),
                port: s.port,
                username: s.username.clone(),
                auth,
                trust_scope: String::new(),
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
                        name: p.name.clone(),
                        host: p.host.clone(),
                        port: p.port,
                        username: String::new(),
                        auth: Auth::Socks { username, password },
                        trust_scope: String::new(),
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
}

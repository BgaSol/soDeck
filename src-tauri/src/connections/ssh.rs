use super::{
    host_keys::{HostKeys, Key},
    io::Stream,
    Answer, Attempt, Error, Question,
};
use crate::servers::connection_profile::{Auth, Node};
use russh::{
    client,
    keys::{HashAlg, PrivateKeyWithHashAlg, PublicKeyOrCertificate},
};
use std::{
    sync::{Arc, Weak},
    time::Duration,
};

pub(crate) struct Client {
    attempt: Weak<Attempt>,
    name: String,
    endpoint: String,
    trust_scope: String,
    index: usize,
    keys: Arc<HostKeys>,
}
impl client::Handler for Client {
    type Error = Error;
    async fn check_server_key(&mut self, public: &PublicKeyOrCertificate) -> Result<bool, Error> {
        let public = match public {
            PublicKeyOrCertificate::PublicKey { key, .. } => key,
            _ => {
                return Err(Error::new(
                    "hostCertificate",
                    "当前版本尚不支持 SSH 主机证书，请使用普通主机密钥",
                ))
            }
        };
        let key = Key {
            algorithm: public.algorithm().to_string(),
            public_key: public
                .to_openssh()
                .map_err(|_| Error::new("hostKey", "无法编码 SSH 主机密钥"))?,
            fingerprint: public.fingerprint(HashAlg::Sha256).to_string(),
        };
        let previous = self.keys.get(&self.trust_scope)?;
        if previous
            .as_ref()
            .is_some_and(|k| k.public_key == key.public_key)
        {
            return Ok(true);
        }
        let attempt = self.attempt.upgrade().ok_or_else(Error::cancelled)?;
        if attempt.view().state != "connecting" {
            return Err(Error::new(
                "hostChanged",
                "已连接服务器在密钥重协商时更换身份，连接已停止",
            ));
        }
        let reply = attempt
            .ask(|id| Question::HostKey {
                id,
                node_index: self.index,
                name: self.name.clone(),
                endpoint: self.endpoint.clone(),
                algorithm: key.algorithm.clone(),
                fingerprint: key.fingerprint.clone(),
                previous_fingerprint: previous.as_ref().map(|k| k.fingerprint.clone()),
            })
            .await?;
        if !matches!(reply, Answer::HostKey { accept: true }) {
            return Err(Error::new(
                "hostRejected",
                "未信任此 SSH 主机密钥，连接已停止",
            ));
        }
        if attempt.cancel.is_cancelled() {
            return Err(Error::cancelled());
        }
        self.keys.save(
            &self.trust_scope,
            key,
            previous.as_ref().map(|k| k.public_key.as_str()),
        )?;
        Ok(true)
    }
}
pub(super) async fn connect(
    stream: Stream,
    node: &mut Node,
    index: usize,
    a: Arc<Attempt>,
    keys: Arc<HostKeys>,
) -> Result<client::Handle<Client>, Error> {
    let config = client::Config {
        keepalive_interval: Some(Duration::from_secs(30)),
        keepalive_max: 3,
        ..Default::default()
    };
    let handler = Client {
        attempt: Arc::downgrade(&a),
        name: node.name.clone(),
        endpoint: node.endpoint(),
        trust_scope: node.trust_scope.clone(),
        index,
        keys,
    };
    let mut handle = a
        .wait("sshHandshake", index, async {
            client::connect_stream(Arc::new(config), stream, handler).await
        })
        .await?;
    loop {
        let result = match &node.auth {
            Auth::Password(Some(password)) => {
                a.wait("authentication", index, async {
                    handle
                        .authenticate_password(&node.username, password)
                        .await
                        .map_err(Error::from)
                })
                .await?
            }
            Auth::Key { .. } => {
                let parsed = a.wait("privateKey", index, load_key(node)).await;
                if let Err(error) = &parsed {
                    if error.code == "keyEncrypted" || error.code == "keyPassphrase" {
                        a.fill_credentials(node, index, "privateKey", Some(&error.message))
                            .await?;
                        continue;
                    }
                }
                let key = parsed?;
                a.wait("authentication", index, async {
                    let hash = handle
                        .best_supported_rsa_hash()
                        .await
                        .map_err(Error::from)?
                        .flatten();
                    handle
                        .authenticate_publickey(
                            &node.username,
                            PrivateKeyWithHashAlg::new(Arc::new(key), hash),
                        )
                        .await
                        .map_err(Error::from)
                })
                .await?
            }
            _ => return Err(Error::new("credentials", "缺少 SSH 认证资料")),
        };
        if result.success() {
            return Ok(handle);
        }
        let authentication = if matches!(node.auth, Auth::Key { .. }) {
            "privateKey"
        } else {
            "password"
        };
        a.fill_credentials(
            node,
            index,
            authentication,
            Some("SSH 认证失败，请检查用户名并重新输入凭据。"),
        )
        .await?;
    }
}
async fn load_key(node: &Node) -> Result<russh::keys::PrivateKey, Error> {
    let Auth::Key { path, passphrase } = &node.auth else {
        unreachable!()
    };
    let path = path
        .clone()
        .ok_or_else(|| Error::new("keyPath", "请选择私钥文件"))?;
    let passphrase = passphrase.clone();
    tokio::task::spawn_blocking(move || {
        use std::io::Read;
        let mut bytes = String::new();
        std::fs::File::open(path)
            .and_then(|f| f.take(1024 * 1024 + 1).read_to_string(&mut bytes))
            .map_err(|_| Error::new("keyRead", "无法读取私钥文件"))?;
        if bytes.len() > 1024 * 1024 {
            return Err(Error::new("keyRead", "私钥文件超过 1 MiB"));
        }
        let encrypted =
            russh::keys::PrivateKey::from_openssh(&bytes).is_ok_and(|k| k.is_encrypted());
        if passphrase.as_ref().is_none_or(|p| p.is_empty()) && encrypted {
            return Err(Error::new("keyEncrypted", "私钥已加密，请提供口令"));
        }
        russh::keys::decode_secret_key(&bytes, passphrase.as_deref().filter(|p| !p.is_empty()))
            .map_err(|e| match e {
                russh::keys::Error::KeyIsEncrypted => {
                    Error::new("keyEncrypted", "私钥已加密，请提供口令")
                }
                _ if encrypted => Error::new("keyPassphrase", "私钥口令错误，请重新输入"),
                _ => Error::new("keyParse", "私钥格式无效、不支持或口令错误"),
            })
    })
    .await
    .map_err(|_| Error::new("keyRead", "私钥读取任务失败"))?
}

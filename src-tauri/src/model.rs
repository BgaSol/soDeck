use serde::{Deserialize, Serialize};

// Configuration containing credentials stays in Rust and is never serialized.
#[derive(Clone)]
pub struct ServerConfig {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub tags: Vec<String>,
    pub auth_type: String,
    pub private_key: String,
    pub password: String,
    pub jump_server_id: Option<String>,
    pub direct_host: Option<String>,
    pub direct_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerView {
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub tags: Vec<String>,
    pub auth_type: String,
    pub private_key: String,
    #[serde(default)]
    pub has_saved_password: bool,
    pub jump_server_id: Option<String>,
    pub direct_host: Option<String>,
    pub direct_port: u16,
}
impl ServerConfig {
    pub fn view(&self) -> ServerView {
        ServerView {
            id: self.id.clone(),
            name: self.name.clone(),
            host: self.host.clone(),
            port: self.port,
            username: self.username.clone(),
            tags: self.tags.clone(),
            auth_type: self.auth_type.clone(),
            private_key: self.private_key.clone(),
            has_saved_password: !self.password.is_empty(),
            jump_server_id: self.jump_server_id.clone(),
            direct_host: self.direct_host.clone(),
            direct_port: self.direct_port,
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PasswordChange {
    #[default]
    Keep,
    Set {
        value: String,
    },
    Clear,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveServerRequest {
    #[serde(flatten)]
    pub server: ServerView,
    #[serde(default)]
    pub password_change: PasswordChange,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ServerDraft {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub tags: Vec<String>,
    pub auth_type: String,
    pub private_key: String,
    pub jump_server_id: Option<String>,
    pub direct_host: Option<String>,
    pub direct_port: u16,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum DuplicatePasswordPolicy {
    Inherit,
    Set { value: String },
    Clear,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DuplicateServerRequest {
    pub source_id: String,
    pub draft: ServerDraft,
    pub password_policy: DuplicatePasswordPolicy,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppError {
    #[serde(skip)]
    pub diagnostic: crate::diagnostics::event::ErrorInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native_code: Option<i64>,
    pub kind: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hop: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<Box<str>>,
}
impl AppError {
    #[track_caller]
    pub fn new(kind: &str, message: impl ToString) -> Self {
        Self {
            diagnostic: crate::diagnostics::event::ErrorInfo::new(kind),
            trace_id: None,
            native_code: None,
            kind: kind.into(),
            message: message.to_string(),
            fingerprint: None,
            previous: None,
            hop: None,
            stage: None,
        }
    }
    pub fn host_key(fingerprint: String, previous: Option<String>) -> Self {
        Self {
            diagnostic: crate::diagnostics::event::ErrorInfo::new("hostKey"),
            trace_id: None,
            native_code: None,
            kind: "hostKey".into(),
            message: if previous.is_some() {
                "服务器指纹发生变化，请核对后决定是否更新"
            } else {
                "首次连接，请核对服务器指纹"
            }
            .into(),
            fingerprint: Some(fingerprint),
            previous,
            hop: None,
            stage: None,
        }
    }
    pub fn at(mut self, server: &ServerConfig, hop: usize, stage: &str) -> Self {
        if self.hop.is_none() {
            self.message = format!(
                "{} ({}:{}) · {}：{}",
                server.name, server.host, server.port, stage, self.message
            );
            self.hop = Some(hop as u8);
            self.stage = Some(stage.into());
        }
        self
    }
    pub fn traced(mut self, context: &crate::diagnostics::event::Context) -> Self {
        self.trace_id = Some(context.trace_id.clone());
        self
    }
    pub fn native(mut self, domain: &'static str, code: Option<i64>) -> Self {
        self.diagnostic.domain = Some(domain);
        self.diagnostic.native_code = code;
        self.native_code = code;
        self
    }
    #[track_caller]
    pub fn io(kind: &str, error: std::io::Error) -> Self {
        let code = error.raw_os_error().map(i64::from);
        let native_kind = format!("{:?}", error.kind());
        let mut result = Self::new(kind, error).native("os", code);
        result.diagnostic.native_kind = Some(native_kind);
        result
    }
}
impl From<rusqlite::Error> for AppError {
    #[track_caller]
    fn from(value: rusqlite::Error) -> Self {
        let code = value.sqlite_error().map(|e| i64::from(e.extended_code));
        Self::new("database", value).native("sqlite", code)
    }
}
impl From<std::io::Error> for AppError {
    #[track_caller]
    fn from(value: std::io::Error) -> Self {
        Self::io("io", value)
    }
}
impl From<ssh2::Error> for AppError {
    #[track_caller]
    fn from(value: ssh2::Error) -> Self {
        let kind = match value.code() {
            ssh2::ErrorCode::Session(-15 | -18 | -19 | -48) => "authentication",
            ssh2::ErrorCode::Session(-9 | -30) => "timeout",
            ssh2::ErrorCode::Session(-7 | -13 | -43 | -45) => "network",
            _ => "ssh",
        };
        let (domain, code) = match value.code() {
            ssh2::ErrorCode::Session(c) => ("libssh2", c),
            ssh2::ErrorCode::SFTP(c) => ("sftp", c),
        };
        Self::new(kind, value).native(domain, Some(i64::from(code)))
    }
}
pub type AppResult<T> = Result<T, AppError>;

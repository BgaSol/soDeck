use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Server {
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default = "default_auth")]
    pub auth_type: String,
    #[serde(default)]
    pub private_key: String,
    #[serde(default)]
    pub password: String,
}
fn default_auth() -> String {
    "agent".into()
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppError {
    pub kind: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous: Option<String>,
}
impl AppError {
    pub fn new(kind: &str, message: impl ToString) -> Self {
        Self {
            kind: kind.into(),
            message: message.to_string(),
            fingerprint: None,
            previous: None,
        }
    }
    pub fn host_key(fingerprint: String, previous: Option<String>) -> Self {
        Self {
            kind: "hostKey".into(),
            message: if previous.is_some() {
                "服务器指纹发生变化，请核对后决定是否更新"
            } else {
                "首次连接，请核对服务器指纹"
            }
            .into(),
            fingerprint: Some(fingerprint),
            previous,
        }
    }
}
impl From<rusqlite::Error> for AppError {
    fn from(value: rusqlite::Error) -> Self {
        Self::new("database", value)
    }
}
impl From<std::io::Error> for AppError {
    fn from(value: std::io::Error) -> Self {
        Self::new("io", value)
    }
}
impl From<ssh2::Error> for AppError {
    fn from(value: ssh2::Error) -> Self {
        Self::new("ssh", value)
    }
}
pub type AppResult<T> = Result<T, AppError>;

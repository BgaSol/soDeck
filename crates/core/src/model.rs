use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

pub type Result<T> = std::result::Result<T, CoreError>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}
impl CoreError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            details: None,
        }
    }
    pub fn details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }
}
impl std::fmt::Display for CoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for CoreError {}
impl From<rusqlite::Error> for CoreError {
    fn from(_: rusqlite::Error) -> Self {
        Self::new("STORAGE_ERROR", "本地数据库操作失败")
    }
}
impl From<std::io::Error> for CoreError {
    fn from(e: std::io::Error) -> Self {
        Self::new("IO_ERROR", format!("本地 I/O 操作失败 ({:?})", e.kind()))
    }
}
impl From<serde_json::Error> for CoreError {
    fn from(_: serde_json::Error) -> Self {
        Self::new("INVALID_PARAMS", "请求数据格式无效")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Caller {
    pub source: String,
    pub plugin_id: Option<String>,
    pub plugin_version: Option<String>,
    pub instance_id: String,
    pub view_id: Option<String>,
    pub mount_generation: Option<u64>,
}
impl Caller {
    pub fn shell() -> Self {
        Self {
            source: "shell".into(),
            plugin_id: None,
            plugin_version: None,
            instance_id: "shell".into(),
            view_id: None,
            mount_generation: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Server {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_type: String,
    pub revision: u64,
    pub has_secret: bool,
}
impl Server {
    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty()
            || self.name.len() > 200
            || self.host.trim().is_empty()
            || self.host.len() > 253
            || self.host.chars().any(char::is_whitespace)
            || self.host.contains(['/', '\\', '\0'])
            || self.username.trim().is_empty()
            || self.username.len() > 200
            || self.port == 0
            || !["password", "key"].contains(&self.auth_type.as_str())
        {
            return Err(CoreError::new(
                "INVALID_PARAMS",
                "请填写有效名称、主机、端口、用户名及认证方式",
            ));
        }
        Ok(())
    }
}

pub fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
pub fn field<'a>(v: &'a Value, name: &str) -> Result<&'a str> {
    v.get(name)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| CoreError::new("INVALID_PARAMS", format!("缺少 {name}")))
}
pub fn number(v: &Value, name: &str) -> Result<u64> {
    v.get(name)
        .and_then(Value::as_u64)
        .ok_or_else(|| CoreError::new("INVALID_PARAMS", format!("缺少有效的 {name}")))
}

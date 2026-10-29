//! Update discovery and persistence, independent of UI, plugin runtime and user databases.
pub mod catalog;
pub mod model;
pub mod strict;
pub mod plan;
pub mod atomic;
pub mod download;
pub mod github;
pub mod store;
#[path = "contract.generated.rs"]
pub mod contract;

pub use catalog::{Catalog, VerifiedCatalog};
pub use model::*;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, thiserror::Error)]
#[serde(rename_all = "camelCase")]
#[error("{message}")]
pub struct Error {
    pub code: String,
    pub message: String,
}
pub type Result<T> = std::result::Result<T, Error>;
pub fn error(code: &str, message: impl Into<String>) -> Error {
    Error { code: code.into(), message: message.into().chars().take(512).collect() }
}
pub fn hash(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}
pub fn digest<T: serde::Serialize>(value: &T) -> String {
    hash(&serde_json::to_vec(value).expect("serializable update model"))
}
pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

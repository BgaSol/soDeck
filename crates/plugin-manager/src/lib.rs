//! Signed, immutable plugin packages. Preparing never changes the active process.
mod package;
mod store;

pub use package::{
    Contributions, Entry, Location, Manifest, Requirements, Signature, ViewDeclaration,
};
pub use store::{InstalledPlugin, PluginManager, PluginStatus};

use std::collections::BTreeMap;

pub type TrustedKeys = BTreeMap<String, Vec<u8>>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Plugin storage: {0}")]
    Io(#[from] std::io::Error),
    #[error("Plugin metadata: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Invalid plugin archive: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("{0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, Error>;

pub const CORE_VERSION: &str = "2.0.0";
pub const API_VERSION: &str = "1.0.0";
pub const CAPABILITIES: &[&str] = &[
    "servers.v1",
    "credentials.v1",
    "connections.v1",
    "remote.pty.v1",
    "resources.v1",
    "storage.v1",
    "workspace.v1",
    "diagnostics.v1",
    "clipboard.v1",
    "plugin.invoke.v1",
];

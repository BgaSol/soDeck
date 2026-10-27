//! Signed, immutable plugin packages. Preparing never changes the active process.
mod package;
mod store;

pub use package::{
    CommandDeclaration, Contributions, Entry, Location, Manifest, Requirements, Signature,
    ViewDeclaration, WorkspaceEntry, Presentation, ToolContribution, TabCreateContribution, DialogDeclaration,
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
#[path="../../../contracts/capabilities.rs"]
mod capabilities;
pub use capabilities::{API_VERSION, CAPABILITIES};

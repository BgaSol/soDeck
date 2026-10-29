//! Integrity-checked, immutable plugin packages. Preparing never changes the active process.
mod package;
mod store;
pub mod enablement;
mod atomic_file;
#[path="../../../contracts/observability_descriptor.rs"]
pub mod observation_descriptor;
#[path="../../../contracts/settings_descriptor.rs"]
pub mod settings_descriptor;

pub use package::{
    CommandDeclaration, Contributions, Entry, Location, Manifest, Requirements, Signature, SignatureStatus, VerifiedPackageSnapshot,
    ViewDeclaration, WorkspaceEntry, Presentation, ToolContribution, TabCreateContribution, DialogDeclaration,
    CompatibilityContext, InspectedPackage,
};
pub use store::{InstalledPlugin, PluginManager, PluginStatus, PackageStager, StagedPackage, PackageMetadata, SelectionSnapshot, SelectionMetadata, SelectedPackageMetadata};

use std::collections::BTreeMap;

#[derive(Clone,Debug)]
pub struct TrustedKey { pub public_key: Vec<u8>, pub development_only: bool }
impl From<Vec<u8>> for TrustedKey { fn from(public_key:Vec<u8>)->Self { Self { public_key, development_only:false } } }
pub type TrustedKeys = BTreeMap<String, TrustedKey>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{1}")]
    Fault(&'static str, String),
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

#[path="../../../contracts/capabilities.rs"]
mod capabilities;
pub use capabilities::{API_VERSION, CAPABILITIES, DATA_FORMATS, CORE_COMPATIBILITY_VERSION as CORE_VERSION};

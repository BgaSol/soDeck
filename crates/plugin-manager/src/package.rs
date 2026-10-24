use crate::{Error, Result, TrustedKeys, API_VERSION, CAPABILITIES, CORE_VERSION};
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
};

const MAX_ARCHIVE: u64 = 32 * 1024 * 1024;
const MAX_EXPANDED: u64 = 128 * 1024 * 1024;
const MAX_FILES: usize = 2000;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Manifest {
    pub plugin_id: String,
    pub version: String,
    pub requires: Requirements,
    pub entry: Entry,
    pub contributes: Contributions,
    pub data_schema_version: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Requirements {
    pub core: String,
    pub plugin_api: String,
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub platforms: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub ui: String,
    pub worker: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Contributions {
    pub views: Vec<ViewDeclaration>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ViewDeclaration {
    pub id: String,
    pub title: String,
    pub scope: String,
    pub supported_hosts: Vec<String>,
    #[serde(default)]
    pub dock_edges: Vec<String>,
    pub default_location: Location,
    #[serde(default)]
    pub multiple: bool,
    #[serde(default = "one")]
    pub state_version: u32,
    #[serde(default)]
    pub min_width: Option<u32>,
    #[serde(default)]
    pub min_height: Option<u32>,
}
fn one() -> u32 {
    1
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Location {
    pub host: String,
    #[serde(default)]
    pub edge: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Signature {
    pub key_id: String,
    pub signature: String,
}

pub(crate) fn valid_relative(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 240
        && !name.contains('\\')
        && !name.contains(':')
        && !name.starts_with('/')
        && name.split('/').all(|part| {
            if part.is_empty()
                || part == "."
                || part == ".."
                || part.ends_with('.')
                || part.ends_with(' ')
            {
                return false;
            }
            if part.bytes().any(|b| b < 32) {
                return false;
            }
            let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
            ![
                "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
                "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8",
                "LPT9",
            ]
            .contains(&stem.as_str())
        })
}

pub(crate) fn extract(archive_path: &Path, target: &Path) -> Result<()> {
    if fs::metadata(archive_path)?.len() > MAX_ARCHIVE {
        return Err(invalid("Package exceeds 32 MiB"));
    }
    let mut archive = zip::ZipArchive::new(fs::File::open(archive_path)?)?;
    if archive.len() > MAX_FILES {
        return Err(invalid("Package exceeds 2000 entries"));
    }
    let mut seen = BTreeSet::new();
    let mut total = 0u64;
    // Validate every path and declared size before creating any package files.
    for i in 0..archive.len() {
        let entry = archive.by_index(i)?;
        let name = entry.name();
        if entry.is_dir() || !valid_relative(name) {
            return Err(invalid("Invalid package path"));
        }
        if !seen.insert(name.to_lowercase()) {
            return Err(invalid("Duplicate or case-colliding package path"));
        }
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 != 0 && mode & 0o170000 != 0o100000)
        {
            return Err(invalid("Package links and special files are forbidden"));
        }
        total = total
            .checked_add(entry.size())
            .ok_or_else(|| invalid("Package size overflow"))?;
        if total > MAX_EXPANDED {
            return Err(invalid("Expanded package exceeds 128 MiB"));
        }
    }
    fs::create_dir_all(target)?;
    let mut actual_total = 0u64;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let path = target.join(entry.name());
        fs::create_dir_all(path.parent().ok_or_else(|| invalid("Missing parent"))?)?;
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        let written = std::io::copy(
            &mut entry.by_ref().take(MAX_EXPANDED + 1 - actual_total),
            &mut output,
        )?;
        actual_total += written;
        if actual_total > MAX_EXPANDED || written != entry.size() {
            return Err(invalid("Expanded package size mismatch"));
        }
        output.sync_all()?;
    }
    Ok(())
}

pub(crate) fn verify(root: &Path, keys: &TrustedKeys) -> Result<Manifest> {
    if fs::symlink_metadata(root)?.file_type().is_symlink() {
        return Err(invalid("Package root is a link"));
    }
    let files = collect_files(root)?;
    if files.len() > MAX_FILES {
        return Err(invalid("Too many package files"));
    }
    let integrity_bytes = bounded_read(&root.join("integrity.json"), 1024 * 1024)?;
    let signature: Signature =
        serde_json::from_slice(&bounded_read(&root.join("signature.json"), 16 * 1024)?)?;
    let public_key = keys
        .get(&signature.key_id)
        .ok_or_else(|| invalid("Untrusted plugin signer"))?;
    let signature = base64::engine::general_purpose::STANDARD
        .decode(signature.signature)
        .map_err(|_| invalid("Malformed signature"))?;
    ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, public_key)
        .verify(&integrity_bytes, &signature)
        .map_err(|_| invalid("Invalid plugin signature"))?;
    let integrity: BTreeMap<String, String> = serde_json::from_slice(&integrity_bytes)?;
    if !integrity.contains_key("manifest.json") || integrity.len() + 2 != files.len() {
        return Err(invalid("Package inventory mismatch"));
    }
    let mut total = 0u64;
    for (path, expected_hash) in &integrity {
        if !valid_relative(path)
            || path == "integrity.json"
            || path == "signature.json"
            || !files.contains(path)
        {
            return Err(invalid("Invalid package inventory path"));
        }
        let bytes = bounded_read(&root.join(path), MAX_EXPANDED)?;
        total += bytes.len() as u64;
        if total > MAX_EXPANDED || format!("{:x}", Sha256::digest(bytes)) != *expected_hash {
            return Err(invalid("Plugin content hash mismatch"));
        }
    }
    let manifest: Manifest =
        serde_json::from_slice(&bounded_read(&root.join("manifest.json"), 1024 * 1024)?)?;
    validate_manifest(&manifest)?;
    if !integrity.contains_key(&manifest.entry.ui)
        || !integrity.contains_key(&manifest.entry.worker)
    {
        return Err(invalid("Missing plugin entry"));
    }
    Ok(manifest)
}

fn validate_manifest(m: &Manifest) -> Result<()> {
    if m.plugin_id.is_empty()
        || m.plugin_id.len() > 128
        || !m
            .plugin_id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-')
        || m.plugin_id.starts_with('.')
        || m.plugin_id.ends_with('.')
    {
        return Err(invalid("Invalid plugin identity"));
    }
    if !valid_relative(&m.version) || semver::Version::parse(&m.version).is_err() {
        return Err(invalid("Invalid plugin version"));
    }
    for (requirement, actual) in [
        (&m.requires.core, CORE_VERSION),
        (&m.requires.plugin_api, API_VERSION),
    ] {
        let requirement = semver::VersionReq::parse(requirement)
            .map_err(|_| invalid("Invalid compatibility range"))?;
        if !requirement.matches(&semver::Version::parse(actual).unwrap()) {
            return Err(invalid("Plugin requires an incompatible core or API"));
        }
    }
    if m.requires
        .capabilities
        .iter()
        .any(|c| !CAPABILITIES.contains(&c.as_str()))
    {
        return Err(invalid("Plugin requires an unavailable capability"));
    }
    if !m.requires.platforms.is_empty()
        && !m
            .requires
            .platforms
            .iter()
            .any(|p| p == std::env::consts::OS)
    {
        return Err(invalid("Plugin does not support this platform"));
    }
    if m.data_schema_version == 0
        || !valid_relative(&m.entry.ui)
        || !valid_relative(&m.entry.worker)
    {
        return Err(invalid("Invalid plugin entry or data schema"));
    }
    let mut ids = BTreeSet::new();
    for view in &m.contributes.views {
        if view.id.is_empty()
            || !ids.insert(&view.id)
            || view.title.is_empty()
            || !["server", "global"].contains(&view.scope.as_str())
            || view.state_version == 0
        {
            return Err(invalid("Invalid or duplicate view declaration"));
        }
        if view.supported_hosts.is_empty()
            || view
                .supported_hosts
                .iter()
                .any(|h| !["dock", "tab", "window"].contains(&h.as_str()))
            || !view.supported_hosts.contains(&view.default_location.host)
        {
            return Err(invalid("Default view location is unsupported"));
        }
        if view
            .dock_edges
            .iter()
            .any(|e| !["left", "right", "bottom"].contains(&e.as_str()))
        {
            return Err(invalid("Invalid dock edge"));
        }
        if view.default_location.host == "dock"
            && !view
                .default_location
                .edge
                .as_ref()
                .is_some_and(|e| view.dock_edges.contains(e))
        {
            return Err(invalid("Default dock edge is unsupported"));
        }
    }
    Ok(())
}

fn collect_files(root: &Path) -> Result<BTreeSet<String>> {
    let mut files = BTreeSet::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let ty = entry.file_type()?;
            if ty.is_symlink() {
                return Err(invalid("Package contains a link"));
            }
            if ty.is_dir() {
                pending.push(entry.path());
            } else if ty.is_file() {
                let relative = entry
                    .path()
                    .strip_prefix(root)
                    .map_err(|_| invalid("Package escapes root"))?
                    .to_string_lossy()
                    .replace('\\', "/");
                if !valid_relative(&relative) {
                    return Err(invalid("Invalid installed package path"));
                }
                files.insert(relative);
                if files.len() > MAX_FILES {
                    return Err(invalid("Too many package files"));
                }
            } else {
                return Err(invalid("Package contains a special file"));
            }
        }
    }
    Ok(files)
}

fn bounded_read(path: &PathBuf, limit: u64) -> Result<Vec<u8>> {
    if fs::metadata(path)?.len() > limit {
        return Err(invalid("Package file exceeds limit"));
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(invalid("Package file exceeds limit"));
    }
    Ok(bytes)
}

pub(crate) fn invalid(message: impl Into<String>) -> Error {
    Error::Invalid(message.into())
}

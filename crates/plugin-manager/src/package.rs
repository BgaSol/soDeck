use crate::{Error, Result, TrustedKeys};
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
};

mod integrity;
mod signature;
mod snapshot;
mod compatibility;
pub use signature::SignatureStatus;
pub use snapshot::{InspectedPackage, VerifiedPackageSnapshot};
pub use compatibility::CompatibilityContext;

const MAX_ARCHIVE: u64 = 32 * 1024 * 1024;
const MAX_EXPANDED: u64 = 128 * 1024 * 1024;
const MAX_FILES: usize = 2000;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Manifest {
    pub plugin_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub version: String,
    pub requires: Requirements,
    pub entry: Entry,
    pub contributes: Contributions,
    pub data_schema_version: u32,
    #[serde(default, skip_serializing_if="Option::is_none")]
    pub observability: Option<ObservationReference>,
}

#[derive(Clone,Debug,Deserialize,Serialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct ObservationReference {pub descriptor_version:u32,pub path:String}

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<SettingsReference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presentation: Option<Presentation>,
    pub views: Vec<ViewDeclaration>,
    #[serde(default)]
    pub commands: Vec<CommandDeclaration>,
    #[serde(default, rename = "workspaceEntries")]
    pub workspace_entries: Vec<WorkspaceEntry>,
    #[serde(default, rename = "workspaceInitializers")]
    pub workspace_initializers: Vec<WorkspaceInitializer>,
    #[serde(default, rename = "retiredViews")]
    pub retired_views: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all="camelCase", deny_unknown_fields)]
pub struct SettingsReference { pub descriptor_version: u32, pub path: String }

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Presentation {
    #[serde(default)]
    pub tools: Vec<ToolContribution>,
    #[serde(default)]
    pub tab_create: Vec<TabCreateContribution>,
    #[serde(default)]
    pub dialogs: Vec<DialogDeclaration>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ToolContribution { pub view_type: String, pub command_id: String, pub edge: String, pub icon: String, pub default_width: u32 }
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TabCreateContribution { pub command_id: String, pub icon: String }
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DialogDeclaration { pub id: String, pub title: String, pub width: u32, pub height: u32 }

impl Manifest {
    pub fn display_name(&self) -> String {
        if let Some(name) = &self.display_name { return name.trim().to_owned(); }
        let mut views = self.contributes.views.iter();
        match (views.next(), views.next()) {
            (Some(view), None) => view.title.clone(),
            _ => self.plugin_id.clone(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommandDeclaration {
    pub id: String,
    pub title: String,
    pub scope: String,
    #[serde(default)]
    pub primary: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<CommandInput>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommandInput {
    pub kind: String,
    pub key: String,
    pub label: String,
    pub initial: String,
    pub min_length: u32,
    pub max_length: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceEntry {
    pub view_type: String,
    #[serde(default)]
    pub open_by_default: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceInitializer {
    pub id: String,
    pub command_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ViewDeclaration {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context_providers: Vec<String>,
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

pub(crate) fn read_snapshot(root: &Path, keys: &TrustedKeys) -> Result<VerifiedPackageSnapshot> { snapshot::read(root, keys) }
pub(crate) fn is_link(m: &fs::Metadata) -> bool {
    #[cfg(windows)] { use std::os::windows::fs::MetadataExt; m.file_attributes() & 0x400 != 0 }
    #[cfg(not(windows))] { m.file_type().is_symlink() }
}

pub(crate) fn validate_manifest(m: &Manifest) -> Result<()> {
    validate_structure(m)?;
    CompatibilityContext::current().validate(m)
}

pub(crate) fn validate_structure(m: &Manifest) -> Result<()> {
    if let Some(settings)=&m.contributes.settings {
        if settings.descriptor_version!=1 || !valid_relative(&settings.path)
            || !["settings.contributions.v1","storage.preferences.v1"].iter().all(|c|m.requires.capabilities.iter().any(|v|v==c)) {
            return Err(invalid("Invalid settings reference or missing capabilities"));
        }
    }
    if let Some(observation)=&m.observability {
        if observation.descriptor_version!=1||observation.path!="diagnostics/observability.json"||!["observability.events.v2","observability.context.v1","observability.snapshots.v1","observability.metrics.v1"].iter().all(|required|m.requires.capabilities.iter().any(|c|c==required)){return Err(invalid("Invalid observation descriptor reference or missing capabilities"));}
    }
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
    for requirement in [&m.requires.core, &m.requires.plugin_api] {
        semver::VersionReq::parse(requirement)
            .map_err(|_| invalid("Invalid compatibility range"))?;
    }
    if m.requires.capabilities.len() > 256 || m.requires.platforms.len() > 16
        || m.requires.capabilities.iter().any(|v| v.is_empty() || v.len() > 128)
        || m.requires.platforms.iter().any(|v| v.is_empty() || v.len() > 64) {
        return Err(invalid("Invalid capability or platform declarations"));
    }
    if m.data_schema_version == 0
        || !valid_relative(&m.entry.ui)
        || !valid_relative(&m.entry.worker)
    {
        return Err(invalid("Invalid plugin entry or data schema"));
    }
    if m.display_name.as_ref().is_some_and(|name| name.trim().is_empty() || name.trim().chars().count() > 64 || name.chars().any(char::is_control)) {
        return Err(invalid("Invalid plugin display name"));
    }
    if let Some(presentation) = &m.contributes.presentation {
        if !m.requires.capabilities.iter().any(|c| c == "workspace.contributions.v2") {
            return Err(invalid("Presentation requires workspace.contributions.v2"));
        }
        if presentation.tools.len() > 32 || presentation.tab_create.len() > 1 || presentation.dialogs.len() > 16 {
            return Err(invalid("Too many presentation declarations"));
        }
        let server_command = |id: &str| m.contributes.commands.iter().any(|c| c.id == id && c.scope == "server" && c.input.is_none());
        let mut tools = BTreeSet::new();
        for tool in &presentation.tools {
            if !tools.insert(&tool.view_type) || !["left", "right"].contains(&tool.edge.as_str())
                || !["folder", "arrow-left-right", "terminal", "layout-panel"].contains(&tool.icon.as_str())
                || !(260..=480).contains(&tool.default_width) || !server_command(&tool.command_id)
                || !m.contributes.views.iter().any(|v| v.id == tool.view_type && v.scope == "server" && !v.multiple && v.supported_hosts.iter().any(|h| h == "dock") && v.dock_edges.contains(&tool.edge)) {
                return Err(invalid("Invalid tool contribution"));
            }
        }
        for action in &presentation.tab_create {
            if action.icon != "plus" || !server_command(&action.command_id) { return Err(invalid("Invalid tab creation contribution")); }
        }
        let mut dialogs = BTreeSet::new();
        for dialog in &presentation.dialogs {
            if dialog.id.is_empty() || dialog.id.len() > 256 || !dialogs.insert(&dialog.id) || dialog.title.trim().is_empty() || dialog.title.chars().count() > 256
                || !(320..=960).contains(&dialog.width) || !(240..=960).contains(&dialog.height)
                || !m.requires.capabilities.iter().any(|c| c == "ui.dialog.v1") {
                return Err(invalid("Invalid plugin dialog declaration"));
            }
        }
    }
    let mut commands = BTreeSet::new();
    for command in &m.contributes.commands {
        if command.input.as_ref().is_some_and(|i| i.kind != "text" || i.initial != "viewTitle" || i.key.is_empty() || i.key.len() > 64 || i.label.is_empty() || i.label.len() > 256 || i.max_length > 256 || i.min_length > i.max_length) {
            return Err(invalid("Invalid command input declaration"));
        }
        if command.id.is_empty()
            || command.title.trim().is_empty()
            || !["server", "view", "global"].contains(&command.scope.as_str())
            || !commands.insert(command.id.clone())
        {
            return Err(invalid("Invalid or duplicate command declaration"));
        }
    }
    let mut initializers = BTreeSet::new();
    let mut initial_commands = BTreeSet::new();
    for init in &m.contributes.workspace_initializers {
        if init.id.is_empty() || init.id.len() > 256 || !initializers.insert(&init.id)
            || !initial_commands.insert(&init.command_id)
            || !m.contributes.commands.iter().any(|c| c.id == init.command_id && c.scope == "server" && c.input.is_none()) {
            return Err(invalid("Workspace initializer must reference a unique server command without input"));
        }
    }
    let mut entries = BTreeSet::new();
    for entry in &m.contributes.workspace_entries {
        if !entries.insert(&entry.view_type)
            || !m
                .contributes
                .views
                .iter()
                .any(|view| view.id == entry.view_type && view.scope == "server" && !view.multiple)
        {
            return Err(invalid(
                "Workspace entry must reference a declared singleton server view",
            ));
        }
    }
    let mut ids = BTreeSet::new();
    let mut retired = BTreeSet::new();
    for id in &m.contributes.retired_views {
        if id.is_empty() || !retired.insert(id) || m.contributes.views.iter().any(|v| &v.id == id) {
            return Err(invalid("Invalid retired view declaration"));
        }
    }
    for view in &m.contributes.views {
        if view.context_providers.len()>1||view.context_providers.iter().any(|p|p!="directory") {return Err(invalid("Unknown or duplicate context provider"));}
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
            if is_link(&fs::symlink_metadata(entry.path())?) {
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
                if files.iter().any(|p: &String| p.eq_ignore_ascii_case(&relative)) { return Err(invalid("Duplicate package path")); }
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
    if is_link(&fs::symlink_metadata(path)?) || !fs::metadata(path)?.is_file() { return Err(invalid("Package file is not regular")); }
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

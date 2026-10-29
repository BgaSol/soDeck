use super::*;
use std::sync::Arc;

#[derive(Clone, Debug)]
struct Contents {
    manifest: Manifest,
    signature: SignatureStatus,
    digest: String,
    files: Arc<BTreeMap<String, Arc<[u8]>>>,
}
/// Only current-runtime verification constructs this runnable snapshot.
#[derive(Clone, Debug)]
pub struct VerifiedPackageSnapshot(Contents);
impl VerifiedPackageSnapshot {
    pub fn manifest(&self) -> &Manifest { &self.0.manifest }
    pub fn signature(&self) -> &SignatureStatus { &self.0.signature }
    pub fn digest(&self) -> &str { &self.0.digest }
    pub fn file(&self, path: &str) -> Option<&[u8]> { self.0.files.get(path).map(|b| b.as_ref()) }
    pub fn files(&self) -> Arc<BTreeMap<String, Arc<[u8]>>> { self.0.files.clone() }
}
/// Metadata only: cannot be accepted, loaded or converted to a runnable snapshot.
#[derive(Clone, Debug)]
pub struct InspectedPackage {
    manifest: Manifest,
    signature: SignatureStatus,
    digest: String,
}
impl InspectedPackage {
    pub fn manifest(&self) -> &Manifest { &self.manifest }
    pub fn signature(&self) -> &SignatureStatus { &self.signature }
    pub fn digest(&self) -> &str { &self.digest }
    pub fn inspect_archive(archive: &Path, scratch: &Path, keys: &TrustedKeys, context: &CompatibilityContext) -> Result<Self> {
        // The caller supplies its own scratch root, never the manager staging root.
        fs::create_dir_all(scratch)?;
        if is_link(&fs::symlink_metadata(scratch)?) { return Err(invalid("Inspection root is a link")); }
        let path = scratch.join(uuid::Uuid::new_v4().to_string());
        let result = (|| {
            extract(archive, &path)?;
            let contents = read_contents(&path, keys)?;
            context.validate(&contents.manifest)?;
            Ok(Self { manifest: contents.manifest, signature: contents.signature, digest: contents.digest })
        })();
        if path.exists() { fs::remove_dir_all(&path)?; }
        result
    }
}
pub(super) fn read(root: &Path, keys: &TrustedKeys) -> Result<VerifiedPackageSnapshot> {
    let result = read_contents(root, keys).and_then(|contents| {
        CompatibilityContext::current().validate(&contents.manifest)?;
        Ok(VerifiedPackageSnapshot(contents))
    });
    result.map_err(|e| match e {
        Error::Fault(_,_) => e,
        _ => Error::Fault("PACKAGE_INTEGRITY_FAILED", e.to_string()),
    })
}
fn read_contents(root: &Path, keys: &TrustedKeys) -> Result<Contents> {
    if is_link(&fs::symlink_metadata(root)?) { return Err(invalid("Package root is a link")); }
    let names = collect_files(root)?;
    let mut files = BTreeMap::<String, Arc<[u8]>>::new();
    let mut total = 0u64;
    let mut digest = Sha256::new();
    for name in &names {
        let limit = match name.as_str() { "integrity.json" | "manifest.json" => 1024*1024, "signature.json" => 16*1024, _ => MAX_EXPANDED.saturating_sub(total) };
        let bytes = bounded_read(&root.join(name), limit)?;
        total += bytes.len() as u64;
        if total>MAX_EXPANDED { return Err(invalid("Expanded package exceeds 128 MiB")); }
        digest.update((name.len() as u64).to_le_bytes()); digest.update(name.as_bytes());
        digest.update((bytes.len() as u64).to_le_bytes()); digest.update(&bytes);
        files.insert(name.clone(), Arc::from(bytes));
    }
    let inventory = files.get("integrity.json").ok_or_else(|| invalid("Missing integrity.json"))?;
    let integrity = integrity::parse(inventory)?;
    let metadata_count = 1 + usize::from(files.contains_key("signature.json"));
    if !integrity.contains_key("manifest.json") || integrity.len()+metadata_count!=files.len() { return Err(invalid("Package inventory mismatch")); }
    for (path, hash) in &integrity {
        if !valid_relative(path) || matches!(path.as_str(),"integrity.json"|"signature.json") { return Err(invalid("Invalid package inventory path")); }
        let bytes = files.get(path).ok_or_else(|| invalid("Missing package file"))?;
        if hash.len()!=64 || !hash.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) || format!("{:x}",Sha256::digest(bytes))!=*hash {
            return Err(invalid(format!("Plugin content hash mismatch: {path}")));
        }
    }
    let manifest: Manifest = serde_json::from_slice(files.get("manifest.json").ok_or_else(|| invalid("Missing manifest"))?)?;
    for entry in [&manifest.entry.ui,&manifest.entry.worker] {
        if !integrity.contains_key(entry) { return Err(invalid("Missing plugin entry")); }
    }
    if let Some(settings)=&manifest.contributes.settings {
        let bytes=files.get(&settings.path).ok_or_else(||invalid("Missing settings descriptor"))?;
        if bytes.len()>128*1024 {return Err(invalid("Settings descriptor exceeds limit"));}
    }
    if let Some(observation)=&manifest.observability {
        let bytes=files.get(&observation.path).ok_or_else(||invalid("Missing observation descriptor"))?;
        if bytes.len()>1024*1024{return Err(invalid("Observation descriptor exceeds limit"));}
        crate::observation_descriptor::Descriptor::parse(bytes,&manifest.plugin_id).map_err(invalid)?;
    }
    let signature=signature::evaluate(files.get("signature.json").map(|b|b.as_ref()),inventory,keys)?;
    validate_structure(&manifest).map_err(|e|Error::Fault("PACKAGE_INCOMPATIBLE",e.to_string()))?;
    Ok(Contents {manifest, signature, digest:format!("{:x}",digest.finalize()),files:Arc::new(files)})
}

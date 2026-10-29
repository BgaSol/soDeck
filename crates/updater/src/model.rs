use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const REPOSITORY: &str = "BgaSol/soDeck";
pub const MAX_CATALOG: usize = 1024 * 1024;
pub const MAX_CACHE: u64 = 512 * 1024 * 1024;
pub const MAX_INSTALLER: u64 = 256 * 1024 * 1024;
pub const MAX_PLUGIN: u64 = 32 * 1024 * 1024;

#[derive(Clone, Debug, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all="lowercase")]
pub enum Channel { Stable, Preview }
impl Channel { pub fn name(self)->&'static str {match self{Self::Stable=>"stable",Self::Preview=>"preview"}} }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Compatibility {
    pub core_compatibility_version:String,
    pub plugin_api_version:String,
    pub capabilities:Vec<String>,
    pub platform:String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct RuntimeIdentity {
    pub app_version:String,
    pub build_id:String,
    pub channel:Channel,
    pub compatibility:Compatibility,
    pub data_formats:BTreeMap<String,u32>,
    pub distribution:String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Requirements {
    pub core:String,
    pub plugin_api:String,
    pub capabilities:Vec<String>,
    #[serde(default)]
    pub platforms:Vec<String>,
}
impl Requirements {
    pub fn matches(&self, context:&Compatibility)->bool {
        let matches=|requirement:&str,actual:&str|semver::VersionReq::parse(requirement).ok().zip(semver::Version::parse(actual).ok()).is_some_and(|(r,v)|r.matches(&v));
        matches(&self.core,&context.core_compatibility_version)&&matches(&self.plugin_api,&context.plugin_api_version)
            &&self.capabilities.iter().all(|v|context.capabilities.contains(v))
            &&(self.platforms.is_empty()||self.platforms.iter().any(|p|p==context.platform.split('-').next().unwrap_or(&context.platform)))
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Asset {
    pub repository:String,
    pub release_tag:String,
    pub asset_id:u64,
    pub name:String,
    pub size:u64,
    pub archive_sha256:String,
}
impl Asset {
    pub fn url(&self)->url::Url {
        let mut u=url::Url::parse("https://github.com").unwrap();
        u.path_segments_mut().unwrap().extend(["BgaSol","soDeck","releases","download",&self.release_tag,&self.name]);u
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct CoreInstaller {pub asset:Asset,pub signature:String,pub signed_version:String}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct CoreRelease {
    pub app_version:String,
    pub build_id:String,
    pub core_compatibility_version:String,
    pub plugin_api_version:String,
    pub capabilities:Vec<String>,
    pub data_formats:BTreeMap<String,u32>,
    pub input_fingerprint:String,
    pub platforms:BTreeMap<String,CoreInstaller>,
    pub bundled_plugins:BTreeMap<String,String>,
}
impl CoreRelease {
    pub fn context(&self, platform:&str)->Compatibility { Compatibility{core_compatibility_version:self.core_compatibility_version.clone(),plugin_api_version:self.plugin_api_version.clone(),capabilities:self.capabilities.clone(),platform:platform.into()} }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct PluginRelease {
    pub version:String,
    pub requires:Requirements,
    pub data_schema_version:u32,
    pub input_fingerprint:String,
    pub asset:Asset,
    pub package_digest:String,
    pub signing_key_id:String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields,bound(deserialize="T: Deserialize<'de>"))]
pub struct Versions<T> { pub current:T, #[serde(default)] pub history:Vec<T> }
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct PackageSelection {pub version:String,pub digest:String,pub requires:Requirements,pub data_schema_version:u32}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all="lowercase")]
pub enum Origin { Bundled, Official, Local }
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct InstalledPlugin {
    pub plugin_id:String,
    pub selected:Option<PackageSelection>,
    pub pending:Option<PackageSelection>,
    pub enabled:bool,
    pub origin:Origin,
    pub follow_official:bool,
    pub failure:Option<String>,
}
impl InstalledPlugin {pub fn next(&self)->Option<&PackageSelection>{self.pending.as_ref().or(self.selected.as_ref())}}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct InstalledSnapshot {
    pub runtime:RuntimeIdentity,
    pub selection_revision:String,
    pub enablement_revision:String,
    pub source_revision:String,
    pub plugins:Vec<InstalledPlugin>,
}

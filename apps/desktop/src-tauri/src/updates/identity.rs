use base64::Engine;
use serde::Deserialize;
use sodeck_updater::{Channel, Compatibility, RuntimeIdentity};
use std::{collections::BTreeMap, path::Path};

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BuildIdentity {
    pub schema_version: u32,
    pub app_version: String,
    pub build_id: String,
    pub channel: String,
    pub product_name: String,
    pub identifier: String,
    pub catalog_keys: BTreeMap<String, String>,
    pub updater_key: Option<String>,
    pub qa: bool,
}
impl BuildIdentity {
    pub fn compiled() -> Self { serde_json::from_str(include_str!(concat!(env!("OUT_DIR"), "/build-identity.json"))).expect("Validated build identity") }
    pub fn keys(&self) -> BTreeMap<String, Vec<u8>> {
        self.catalog_keys.iter().filter_map(|(id, key)| base64::engine::general_purpose::STANDARD.decode(key).ok().filter(|k| k.len() == 32).map(|k| (id.clone(), k))).collect()
    }
    pub fn runtime(&self, mode: &str, exe: &Path) -> RuntimeIdentity {
        RuntimeIdentity {
            app_version: self.app_version.clone(), build_id: self.build_id.clone(),
            channel: if self.channel == "stable" { Channel::Stable } else { Channel::Preview },
            compatibility: Compatibility {
                core_compatibility_version: sodeck_plugin_manager::CORE_VERSION.into(),
                plugin_api_version: sodeck_plugin_manager::API_VERSION.into(),
                capabilities: sodeck_plugin_manager::CAPABILITIES.iter().map(|s| (*s).into()).collect(),
                platform: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
            },
            data_formats: sodeck_plugin_manager::DATA_FORMATS.iter().map(|(k,v)| ((*k).into(),*v)).collect(),
            distribution: if mode == "standard" && self.channel != "development" && installed(exe, &self.product_name, &self.app_version) { "installed" } else { mode }.into(),
        }
    }
}
#[cfg(windows)]
fn installed(exe: &Path, product: &str, version: &str) -> bool {
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};
    let Ok(key) = RegKey::predef(HKEY_CURRENT_USER).open_subkey(format!("Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{product}")) else { return false; };
    let Ok(location): Result<String,_> = key.get_value("InstallLocation") else { return false; };
    let Ok(binary): Result<String,_> = key.get_value("MainBinaryName") else { return false; };
    let Ok(display): Result<String,_> = key.get_value("DisplayVersion") else { return false; };
    if display != version || Path::new(&binary).components().count() != 1 { return false; }
    let expected = Path::new(location.trim_matches('"')).join(binary);
    std::fs::canonicalize(exe).ok().zip(std::fs::canonicalize(expected).ok()).is_some_and(|(a,b)| a == b)
}
#[cfg(not(windows))]
fn installed(_: &Path, _: &str, _: &str) -> bool { false }

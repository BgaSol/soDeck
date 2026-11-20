//! Static plugin presentation assets, independent of plugin process/UI lifetime.
use crate::{component_ui::resource, model::Manifest, AppCore};
use std::{collections::BTreeMap, fs};

pub const MAX_BYTES: u64 = 32 * 1024;
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}
pub fn valid_path(path: &str) -> bool {
    path.len() <= 256
        && path.ends_with(".svg")
        && !path.contains(['\\', ':', '%', '?', '#'])
        && path
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != "..")
}
pub fn validate(icons: &BTreeMap<String, String>) -> Result<(), String> {
    if icons.len() > 32
        || icons
            .iter()
            .any(|(name, path)| !valid_name(name) || !valid_path(path))
    {
        return Err("插件图标名称、包内 SVG 路径或数量无效".into());
    }
    Ok(())
}
pub fn url(manifest: &Manifest, name: &str) -> Option<String> {
    manifest
        .icons
        .contains_key(name)
        .then(|| format!("{}/icons/{}/{}", manifest.id, manifest.version, name))
}
impl AppCore {
    pub fn icon_resource(
        &self,
        plugin: &str,
        version: &str,
        name: &str,
    ) -> Result<(Vec<u8>, &'static str), String> {
        let p = self
            .snapshot()
            .plugins
            .into_iter()
            .find(|p| p.manifest.id == plugin)
            .ok_or("插件未安装")?;
        let result = (|| {
            if p.manifest.version != version {
                return Err("图标资源版本已变更".into());
            }
            let relative = p.manifest.icons.get(name).ok_or("图标未声明")?;
            if !valid_name(name) || !valid_path(relative) {
                return Err("图标声明无效".into());
            }
            let path = resource(&p.directory, relative)?;
            if fs::metadata(&path).map_err(|e| e.to_string())?.len() > MAX_BYTES {
                return Err("图标超过 32 KiB".into());
            }
            Ok((fs::read(path).map_err(|e| e.to_string())?, "image/svg+xml"))
        })();
        if let Err(error) = &result {
            self.log(
                None,
                "warn",
                &format!("插件 {plugin} 图标 {name} 无法读取：{error}"),
                None,
            );
        }
        result
    }
}

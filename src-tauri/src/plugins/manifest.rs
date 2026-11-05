use crate::{
    model::{Manifest, PluginRecord},
    API_VERSION, TARGET,
};
use std::{
    fs,
    path::{Component, Path},
};

pub fn read_manifest(directory: &Path) -> Result<PluginRecord, String> {
    let directory = directory
        .canonicalize()
        .map_err(|error| format!("无法打开插件目录：{error}"))?;
    let path = directory.join("plugin.json");
    let metadata = fs::metadata(&path).map_err(|error| format!("找不到 plugin.json：{error}"))?;
    if metadata.len() > 64 * 1024 {
        return Err("插件清单超过 64 KiB".into());
    }
    let manifest: Manifest =
        serde_json::from_slice(&fs::read(&path).map_err(|error| error.to_string())?)
            .map_err(|error| format!("插件清单格式错误：{error}"))?;
    if manifest.id == crate::updates::CORE_ID {
        return Err("插件 ID 不能使用核心保留名称 sodeck.core".into());
    }
    if manifest.id.is_empty()
        || manifest.id.len() > 128
        || !manifest.id.starts_with(|c: char| c.is_ascii_alphanumeric())
        || !manifest
            .id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        return Err("插件 ID 只能包含字母、数字、点、下划线或短横线，并以字母或数字开头".into());
    }
    if manifest.name.trim().is_empty() || manifest.name.len() > 256 {
        return Err("插件名称不能为空或过长".into());
    }
    let version = manifest.version.split('-').next().unwrap_or_default();
    if version.split('.').count() != 3
        || !version.split('.').all(|part| part.parse::<u32>().is_ok())
    {
        return Err("插件版本应为 major.minor.patch".into());
    }
    if manifest.api_version.split('.').next() != Some(API_VERSION) {
        return Err(format!(
            "插件要求核心 API {}，当前为 {API_VERSION}",
            manifest.api_version
        ));
    }
    if manifest.target != TARGET {
        return Err(format!("插件适用 {}，当前平台为 {TARGET}", manifest.target));
    }
    let relative = Path::new(&manifest.entry);
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|p| !matches!(p, Component::Normal(_)))
    {
        return Err("插件入口必须是插件目录内的相对路径".into());
    }
    let entry = directory
        .join(relative)
        .canonicalize()
        .map_err(|error| format!("插件入口不存在：{error}"))?;
    if !entry.starts_with(&directory) || !entry.is_file() {
        return Err("插件入口必须是插件目录内的文件".into());
    }
    if let Some(ui) = &manifest.ui {
        ui.validate(&directory)?;
    }
    Ok(PluginRecord {
        directory,
        manifest,
        enabled: false,
        pending_update: None,
        previous: None,
        update_error: None,
    })
}

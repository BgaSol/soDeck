//! Checks run only by the packaged-application smoke-test entry point.
use crate::{model::LogFilter, AppCore};
use std::{
    path::Path,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

pub fn core_update_bundle(handle: &tauri::AppHandle, bundle: &Path) -> Result<(), String> {
    use tauri_plugin_updater::UpdaterExt;
    let signature =
        std::fs::read_to_string(format!("{}.sig", bundle.display())).map_err(|e| e.to_string())?;
    let updater = handle
        .updater_builder()
        .target(crate::TARGET)
        .endpoints(vec!["https://github.com/BgaSol/soDeck".parse().unwrap()])
        .map_err(|e| e.to_string())?
        .version_comparator(|_, _| true)
        .build()
        .map_err(|e| e.to_string())?;
    let update = updater.restore_update(serde_json::json!({"version": crate::app_version(), "platforms": {crate::TARGET: {"url": "https://github.com/BgaSol/soDeck", "signature": signature.trim()}}})).map_err(|e| e.to_string())?.ok_or("无法恢复签名更新包信息")?;
    let bytes = std::fs::read(bundle).map_err(|e| e.to_string())?;
    update
        .verify(&bytes)
        .map_err(|e| format!("发行包签名验证失败：{e}"))?;
    let mut changed = bytes;
    if let Some(first) = changed.first_mut() {
        *first ^= 1;
    }
    if update.verify(&changed).is_ok() {
        return Err("更新器错误地接受了修改过的发行包".into());
    }
    Ok(())
}

pub fn plugin_roundtrip(
    core: &Arc<AppCore>,
    directory: &Path,
    persisted: bool,
) -> Result<(), String> {
    if persisted
        && (core.snapshot().plugins.is_empty()
            || core.messages().is_empty()
            || core.messages().iter().any(|m| !m.read)
            || core.snapshot().preferences.theme != "dark")
    {
        return Err("重启后插件目录、消息、已读或主题未保留".into());
    }
    if persisted {
        core.start_enabled();
        if !core
            .snapshot()
            .plugins
            .iter()
            .any(|p| p.enabled && p.status == crate::model::PluginStatus::Running)
        {
            return Err("重启后未自动加载启用插件".into());
        }
        let id = core.snapshot().plugins[0].manifest.id.clone();
        core.set_enabled(&id, false)?;
        if core.snapshot().plugins[0].status != crate::model::PluginStatus::Running {
            return Err("关闭插件不应立即停止进程".into());
        }
        core.set_enabled(&id, true)?;
        core.unload(&id)?;
    } else {
        core.add_plugin(directory)?;
        if !core.snapshot().restart_required
            || core.snapshot().plugins[0].status != crate::model::PluginStatus::Unloaded
        {
            return Err("新增启用插件应等待重启".into());
        }
    }
    let first = core.load(directory)?;
    let deadline = Instant::now() + Duration::from_secs(5);
    while core.messages().is_empty() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
    if core.messages().is_empty() {
        return Err("插件没有发布重要消息".into());
    }
    let logs = core.logs(&LogFilter {
        plugin_id: first.manifest.id.clone(),
        ..Default::default()
    });
    if !logs.iter().any(|l| l.source == "plugin") {
        return Err("插件没有写入普通日志".into());
    }
    core.unload(&first.manifest.id)?;
    let second = core.load(directory)?;
    if first.instance_id == second.instance_id {
        return Err("重新加载复用了旧实例".into());
    }
    core.unload(&second.manifest.id)?;
    core.mark_read(None)?;
    core.set_theme("dark")?;
    if !core.snapshot().storage_errors.is_empty() {
        return Err("存储状态异常".into());
    }
    Ok(())
}

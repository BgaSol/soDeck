//! Checks run only by the packaged-application smoke-test entry point.
use crate::{model::LogFilter, AppCore};
use std::{
    path::Path,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

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

//! Checks run only by the packaged-application smoke-test entry point.
use crate::{model::LogFilter, AppCore};
use std::{
    path::Path,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

/// Isolated --smoke-test data only. Exercises actual native webviews, not SSH.
pub fn workspace_roundtrip(handle: &tauri::AppHandle, core: &AppCore) -> Result<(), String> {
    use crate::{
        windows::DesktopWindows,
        workspaces::{ConnectedSession, Layout},
    };
    use tauri::Manager;
    let catalog = core.server_catalog();
    let records = catalog["servers"].as_array().ok_or("测试目录缺失")?;
    let desktop = handle.state::<DesktopWindows>();
    let mut opened = vec![];
    for name in ["Package gateway", "Package target"] {
        let record = records
            .iter()
            .find(|s| s["name"] == name)
            .ok_or("工作区测试档案缺失")?;
        opened.push(desktop.accept_session(
            handle,
            ConnectedSession {
                server_id: record["id"].as_str().unwrap().into(),
                connection_id: format!("smoke-{name}"),
                endpoint: format!("ops@{}.invalid:22", name.replace(' ', "-")),
            },
        )?);
    }
    let a = &opened[0];
    let b = &opened[1];
    let layout = Layout {
        left_open: false,
        right_open: true,
        left_width: 280,
        right_width: 250,
    };
    core.workspaces
        .lock()
        .unwrap()
        .layout("main", &a.context.workspace_id, layout.clone())?;
    desktop.move_to(handle, "main", &a.context.workspace_id, None, 0, None)?;
    let child = core
        .workspaces
        .lock()
        .unwrap()
        .owner(&a.context.workspace_id)
        .ok_or("移窗丢失工作区")?;
    if child == "main" || handle.get_webview_window(&child).is_none() {
        return Err("未创建原生独立窗口".into());
    }
    desktop.move_to(
        handle,
        "main",
        &b.context.workspace_id,
        Some(child.clone()),
        0,
        None,
    )?;
    desktop.verify_native_geometry(handle, &child, &a.context.workspace_id)?;
    let projection = core.workspace_projection(&child)?;
    if projection.window.workspace_ids
        != [
            b.context.workspace_id.clone(),
            a.context.workspace_id.clone(),
        ]
        || projection.workspaces[1].context != a.context
        || projection.workspaces[1].layout != layout
    {
        return Err("原生移窗改变了上下文、布局或顺序".into());
    }
    let again = desktop.accept_session(
        handle,
        ConnectedSession {
            server_id: a.context.server_id.clone(),
            connection_id: "unused".into(),
            endpoint: "unused".into(),
        },
    )?;
    if again.context != a.context || again.window_id != child {
        return Err("重复打开没有定位已有工作区".into());
    }
    desktop.move_to(
        handle,
        &child,
        &a.context.workspace_id,
        Some("main".into()),
        0,
        None,
    )?;
    // Native close event must close B alone; main must stay alive.
    handle
        .get_webview_window(&child)
        .ok_or("子窗口提前关闭")?
        .close()
        .map_err(|e| e.to_string())?;
    let deadline = Instant::now() + Duration::from_secs(3);
    while handle.get_webview_window(&child).is_some() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
    if handle.get_webview_window(&child).is_some() || handle.get_webview_window("main").is_none() {
        return Err("关闭子窗口影响了应用生命周期".into());
    }
    core.workspaces
        .lock()
        .unwrap()
        .validate_context(&a.context)?;
    if core
        .workspaces
        .lock()
        .unwrap()
        .validate_context(&b.context)
        .is_ok()
    {
        return Err("已关闭子窗口仍保留会话上下文".into());
    }
    desktop.move_to(handle, "main", &a.context.workspace_id, None, 0, None)?;
    let last = core
        .workspaces
        .lock()
        .unwrap()
        .owner(&a.context.workspace_id)
        .unwrap();
    desktop.move_to(
        handle,
        &last,
        &a.context.workspace_id,
        Some("main".into()),
        0,
        None,
    )?;
    let deadline = Instant::now() + Duration::from_secs(3);
    while handle.get_webview_window(&last).is_some() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
    if handle.get_webview_window(&last).is_some() {
        return Err("最后一个 Tab 返回后空窗口没有回收".into());
    }
    core.workspaces
        .lock()
        .unwrap()
        .close("main", &a.context.workspace_id)?;
    if !core.workspace_projection("main")?.workspaces.is_empty() {
        return Err("工作区未清理".into());
    }
    core.workspace_changed();
    Ok(())
}

pub fn server_roundtrip(core: &AppCore, persisted: bool) -> Result<(), String> {
    use serde_json::{json, Value};
    let catalog = core.server_catalog();
    if persisted {
        let servers = catalog["servers"].as_array().ok_or("目录未返回服务器")?;
        let target = servers
            .iter()
            .find(|s| s["name"] == "Package target")
            .ok_or("重开后服务器未保留")?;
        if target["route"].as_array().map(Vec::len) != Some(3)
            || target["authentication"]["hasSavedSecret"] != true
            || servers
                .iter()
                .any(|s| s["authentication"]["hasSessionSecret"] == true)
            || catalog["proxies"].as_array().map(Vec::len) != Some(1)
        {
            return Err("重开后路径、代理或凭据状态不正确".into());
        }
        return Ok(());
    }
    let apply = |change: Value| -> Result<String, String> {
        let result = core.change_catalog(
            core.server_catalog()["revision"].as_u64().unwrap(),
            serde_json::from_value(change).map_err(|e| e.to_string())?,
        )?;
        Ok(result["selectedId"].as_str().unwrap_or("").into())
    };
    let group = apply(json!({"op":"saveGroup","id":null,"name":"Package test"}))?;
    let gateway = apply(
        json!({"op":"saveServer","draft":{"id":null,"name":"Package gateway","host":"gateway.invalid","port":22,"username":"ops","groupId":group,"authentication":{"type":"password","secret":{"action":"replace","value":"fixture-session","remember":false}},"route":[]}}),
    )?;
    let proxy = apply(
        json!({"op":"saveProxy","draft":{"id":null,"name":"Package proxy","host":"127.0.0.1","port":1080,"authentication":{"type":"none","secret":{"action":"clear"}}}}),
    )?;
    let target = apply(
        json!({"op":"saveServer","draft":{"id":null,"name":"Package target","host":"db.internal","port":22,"username":"ops","groupId":group,"authentication":{"type":"privateKey","privateKeyPath":"fixture-key","secret":{"action":"replace","value":"fixture-saved","remember":true}},"route":[{"id":"","kind":"ssh","serverId":gateway},{"id":"","kind":"socks5","proxyId":proxy},{"id":"","kind":"socks5","proxyId":proxy}]}}),
    )?;
    let copy = apply(json!({"op":"copyServer","id":target}))?;
    apply(json!({"op":"deleteServer","id":copy}))?;
    if core.server_catalog().to_string().contains("fixture-saved") {
        return Err("目录泄漏测试秘密".into());
    }
    Ok(())
}

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

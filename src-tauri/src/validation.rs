//! Checks run only by the packaged-application smoke-test entry point.
mod network;
mod remote_plugins;
use crate::{model::LogFilter, AppCore};
use std::{
    path::Path,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

/// Explicit package smoke mode only: a generated local OpenSSH daemon, never a user server.
pub async fn ssh_roundtrip(
    handle: &tauri::AppHandle,
    core: &Arc<AppCore>,
    config: &Path,
) -> Result<(), String> {
    use crate::connections::{Answer, Question};
    use serde_json::json;
    use tauri::Manager;
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Config {
        port: u16,
        username: String,
        private_key_path: String,
        remote_file: Option<String>,
    }
    let config: Config = serde_json::from_slice(&std::fs::read(config).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let catalog = core.server_catalog();
    let previous = catalog["servers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "Package OpenSSH");
    let saved=core.change_catalog(catalog["revision"].as_u64().unwrap(),serde_json::from_value(json!({"op":"saveServer","draft":{"id":previous.map(|s|&s["id"]),"name":"Package OpenSSH","host":"127.0.0.1","port":config.port,"username":config.username,"authentication":{"type":"privateKey","privateKeyPath":config.private_key_path,"secret":{"action":"clear"}},"route":[]}})).map_err(|e|e.to_string())?)?;
    let id = saved["selectedId"].as_str().unwrap();
    let attempt = core.start_connection("main", id)?;
    let context = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let v = core
                .connection_views()
                .into_iter()
                .find(|v| v.attempt_id == attempt.attempt_id)
                .ok_or("连接尝试丢失")?;
            if let Some(Question::HostKey { id, .. }) = v.question {
                core.answer_connection(
                    "main",
                    &v.attempt_id,
                    &id,
                    Answer::HostKey { accept: true },
                )?;
            }
            if v.state == "failed" {
                return Err(v.error.map(|e| e.message).unwrap_or("SSH 验证失败".into()));
            }
            if v.state == "connected" {
                break v.context.ok_or("缺少真实会话".into());
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .map_err(|_| "包内 SSH 连接超过 10 秒")??;
    let command = if cfg!(windows) {
        "cmd /c echo sodeck-openssh-ok"
    } else {
        "printf sodeck-openssh-ok"
    };
    let (bytes, status) = core.connection_validation_exec(&context, command).await?;
    if status != 0 || !String::from_utf8_lossy(&bytes).contains("sodeck-openssh-ok") {
        return Err("实际安装包的 SSH 命令验证失败".into());
    }
    if let Some(remote) = &config.remote_file {
        remote_plugins::roundtrip(core, &context, remote).await?;
    }
    let network = network::start(core, &context).await?;
    // Moving a real session must not create a second SSH transport.
    handle.state::<crate::windows::DesktopWindows>().move_to(
        handle,
        "main",
        &context.workspace_id,
        None,
        0,
        None,
    )?;
    let owner = core
        .workspaces
        .lock()
        .unwrap()
        .owner(&context.workspace_id)
        .ok_or("工作区已丢失")?;
    let _ = core.connection_validation_exec(&context, command).await?;
    if let Some(network) = &network {
        network.verify().await?;
    }
    handle.state::<crate::windows::DesktopWindows>().move_to(
        handle,
        &owner,
        &context.workspace_id,
        Some("main".into()),
        0,
        None,
    )?;
    core.disconnect_connection("main", &context)?;
    tokio::time::timeout(Duration::from_secs(3), async {
        while core
            .connection_views()
            .iter()
            .any(|v| v.attempt_id == attempt.attempt_id && v.state != "disconnected")
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .map_err(|_| "包内 SSH 未完成断开")?;
    if let Some(network) = &network {
        tokio::time::sleep(Duration::from_millis(100)).await;
        for addr in network.addresses.iter().take(2) {
            if tokio::net::TcpStream::connect(addr).await.is_ok() {
                return Err("断开后网络监听未释放".into());
            }
        }
    }
    core.ui_snapshot("main")?;
    let plan = core.ui_close_prepare("main", vec![context.workspace_id.clone()], None)?;
    core.ui_close_finish(
        plan["token"].as_str().ok_or("缺少关闭 token")?,
        serde_json::from_value(plan["views"].clone()).map_err(|e| e.to_string())?,
        "discard",
    )?;
    core.workspaces
        .lock()
        .unwrap()
        .close("main", &context.workspace_id)?;
    core.workspace_changed();
    if network.is_some() {
        std::fs::write(
            core.snapshot()
                .data_directory
                .join("network-smoke-result.json"),
            json!({"completed":true,"tcpForward":true,"socks5":true,"moved":true,"released":true})
                .to_string(),
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Isolated --smoke-test data only. Exercises actual native webviews, not SSH.
pub fn component_ui_roundtrip(
    handle: &tauri::AppHandle,
    core: &Arc<AppCore>,
    directory: &Path,
) -> Result<(), String> {
    use crate::{windows::DesktopWindows, workspaces::ConnectedSession};
    use serde_json::json;
    use tauri::Manager;
    let plugin = core.load(directory)?;
    let catalog = core.server_catalog();
    let server = catalog["servers"]
        .as_array()
        .ok_or("缺少测试目录")?
        .iter()
        .find(|s| s["name"] == "Package gateway")
        .ok_or("缺少测试服务器")?;
    let desktop = handle.state::<DesktopWindows>();
    let w = desktop.accept_session(
        handle,
        ConnectedSession {
            server_id: server["id"].as_str().unwrap().into(),
            connection_id: "ui-native-example".into(),
            endpoint: "ops@ui-example.invalid".into(),
        },
    )?;
    core.ui_snapshot("main")?;
    core.workspace_changed();
    let wait = |window: &str| -> Result<(), String> {
        let started = Instant::now();
        while !core
            .component_ui
            .lock()
            .unwrap()
            .target_ready(&w.context.workspace_id, window)
        {
            if started.elapsed() > Duration::from_secs(10) {
                return Err(format!("原生窗口 {window} 中插件界面未就绪"));
            }
            thread::sleep(Duration::from_millis(20));
        }
        Ok(())
    };
    wait("main")?;
    let state = core.ui_snapshot("main")?;
    let views = state["workspaces"][&w.context.workspace_id]["views"]
        .as_array()
        .ok_or("缺少组件页面")?;
    if views.len() != 2 {
        return Err("默认应只有文件列表和一个终端".into());
    }
    let editor = core.ui_action("main", &w.context, json!({"op":"open","key":format!("{}:editor",plugin.manifest.id),"resourceKey":"/etc/native-example.conf","params":{"name":"native-example.conf"},"requestId":"native-open"}))?;
    wait("main")?;
    let id = editor["viewId"].as_str().unwrap();
    let surface = core
        .component_ui
        .lock()
        .unwrap()
        .mounted_surface(id, "main")
        .ok_or("未找到已挂载的编辑器")?;
    core.ui_event(
        "main",
        &surface,
        json!({"action":"edit","text":"native window draft","cursor":7,"scroll":0}),
    )?;
    desktop.move_to(handle, "main", &w.context.workspace_id, None, 0, None)?;
    let owner = core
        .workspaces
        .lock()
        .unwrap()
        .owner(&w.context.workspace_id)
        .ok_or("工作区丢失")?;
    wait(&owner)?;
    if core
        .ui_event("main", &surface, json!({"action":"edit","text":"stale"}))
        .is_ok()
    {
        return Err("旧窗口仍可发送编辑事件".into());
    }
    desktop.move_to(
        handle,
        &owner,
        &w.context.workspace_id,
        Some("main".into()),
        0,
        None,
    )?;
    wait("main")?;
    let restored = core.ui_attach("main", id)?;
    if restored["data"]["text"] != "native window draft" {
        return Err("移窗后编辑草稿未保留".into());
    }
    let plan = core.ui_close_prepare("main", vec![w.context.workspace_id.clone()], None)?;
    if plan["dirty"].as_array().map(Vec::len) != Some(1) {
        return Err("关闭未发现未保存文件".into());
    }
    core.ui_close_finish(
        plan["token"].as_str().unwrap(),
        serde_json::from_value(plan["views"].clone()).map_err(|e| e.to_string())?,
        "save",
    )?;
    core.workspaces
        .lock()
        .unwrap()
        .close("main", &w.context.workspace_id)?;
    core.workspace_changed();
    core.unload(&plugin.manifest.id)?;
    core.forget(&plugin.manifest.id)?;
    Ok(())
}

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

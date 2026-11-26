//! Isolated package verification, with a public synthetic key (never a real credential).
use crate::AppCore;
use serde_json::json;
use std::{sync::Arc, time::Duration};
use tauri::{Listener, Manager};

pub(super) fn verify(
    handle: &tauri::AppHandle,
    core: &Arc<AppCore>,
    persisted: bool,
) -> Result<(), String> {
    let directory = core.snapshot().data_directory;
    let name = "Package shared connection";
    let target = if persisted {
        let catalog = core.server_catalog();
        let record = catalog["servers"]
            .as_array()
            .ok_or("没有服务器列表")?
            .iter()
            .find(|s| s["name"] == name)
            .ok_or("导入连接未持久化")?;
        let path = record["authentication"]["privateKeyPath"]
            .as_str()
            .ok_or("导入私钥路径丢失")?;
        crate::connections::private_key::inspect(
            &crate::connections::private_key::read(std::path::Path::new(path))
                .map_err(|_| "无法重读导入私钥")?,
        )
        .map_err(|_| "导入私钥无效")?;
        record["id"].as_str().ok_or("没有服务器 ID")?.to_string()
    } else {
        let source = directory.join("share-source-key");
        std::fs::write(&source, include_str!("share_key.txt")).map_err(|e| e.to_string())?;
        let result=core.change_catalog(core.server_catalog()["revision"].as_u64().unwrap(),serde_json::from_value(json!({"op":"saveServer","draft":{"name":name,"host":"package.invalid","port":22,"username":"ops","authentication":{"type":"privateKey","privateKeyPath":source,"secret":{"action":"clear"}},"route":[]}})).map_err(|e|e.to_string())?)?;
        let id = result["selectedId"].as_str().unwrap();
        let share = core
            .export_server_share(
                core.server_catalog()["revision"].as_u64().unwrap(),
                Some(id.into()),
            )
            .map_err(|e| e.message)?;
        core.change_catalog(
            core.server_catalog()["revision"].as_u64().unwrap(),
            serde_json::from_value(json!({"op":"deleteServer","id":id})).unwrap(),
        )?;
        std::fs::remove_file(source).map_err(|e| e.to_string())?;
        let code = share["code"].as_str().unwrap();
        let preview = core.preview_server_share(code).map_err(|e| e.message)?;
        let imported = core
            .import_server_share(preview["catalogRevision"].as_u64().unwrap(), code)
            .map_err(|e| e.message)?;
        imported["targetIds"][0].as_str().unwrap().into()
    };
    let (tx, rx) = std::sync::mpsc::channel();
    let listener = handle.listen("server-sharing-native-result", move |event| {
        let _ = tx.send(event.payload().to_string());
    });
    let script = include_str!("server_sharing.js")
        .replace("__SERVER_ID__", &serde_json::to_string(&target).unwrap());
    let result = handle
        .get_webview_window("main")
        .ok_or("分享验收主窗口不存在")?
        .eval(&script)
        .map_err(|e| e.to_string())
        .and_then(|_| {
            rx.recv_timeout(Duration::from_secs(18))
                .map_err(|_| "分享原生验收超时".into())
        });
    handle.unlisten(listener);
    let result: serde_json::Value = serde_json::from_str(&result?).map_err(|e| e.to_string())?;
    std::fs::write(
        directory.join("server-sharing-native.json"),
        serde_json::to_vec_pretty(&result).unwrap(),
    )
    .map_err(|e| e.to_string())?;
    if result["ok"] != true {
        return Err(format!("分享原生验收失败：{}", result["error"]));
    }
    Ok(())
}

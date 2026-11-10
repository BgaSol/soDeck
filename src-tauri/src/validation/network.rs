//! Explicit smoke mode only: native external package, mounted UI and real SSH TCP.
use super::*;
use serde_json::json;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
pub(super) struct Running {
    pub addresses: Vec<std::net::SocketAddr>,
    echo: tokio::task::JoinHandle<()>,
}
impl Drop for Running {
    fn drop(&mut self) {
        self.echo.abort();
    }
}
pub(super) async fn start(
    core: &Arc<AppCore>,
    context: &crate::workspaces::Context,
) -> Result<Option<Running>, String> {
    let Ok(directory) = std::env::var("SODECK_TEST_NETWORK_PLUGIN") else {
        return Ok(None);
    };
    core.load(std::path::Path::new(&directory))?;
    core.ui_snapshot("main")?;
    let opened=core.ui_action("main",context,json!({"op":"open","key":"sodeck.network-proxy:manager","requestId":"native-network-manager"}))?;
    let manager = opened["viewId"].as_str().ok_or("没有网络视图")?.to_owned();
    core.workspace_changed();
    let (window, token) = surface(core, &manager).await?;
    let service = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| e.to_string())?;
    let port = service.local_addr().map_err(|e| e.to_string())?.port();
    let echo = tokio::spawn(async move {
        while let Ok((mut socket, _)) = service.accept().await {
            tokio::spawn(async move {
                let mut bytes = Vec::new();
                if socket.read_to_end(&mut bytes).await.is_ok() {
                    let _ = socket.write_all(&bytes).await;
                    let _ = socket.shutdown().await;
                }
            });
        }
    });
    let mut running = Running {
        addresses: vec![],
        echo,
    };
    for (id, kind) in [("native-forward", "tcpForward"), ("native-socks", "socks5")] {
        let mut rule = json!({"id":id,"revision":0,"serverId":context.server_id,"name":id,"type":kind,"bind":{"host":"127.0.0.1","port":0}});
        let catalog = core.ui_event(&window, &token, json!({"action":"snapshot"}))?;
        if let Some(previous) = catalog["catalog"]["rules"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|r| r["rule"]["id"] == id)
        {
            rule["revision"] = previous["rule"]["revision"].clone();
        }
        if kind == "tcpForward" {
            rule["target"] = json!({"host":"127.0.0.1","port":port})
        }
        let dialog=core.ui_action("main",context,json!({"op":"open","key":"sodeck.network-proxy:rule-editor","requestId":id,"params":{"rule":rule}}))?["viewId"].as_str().ok_or("没有网络弹窗")?.to_owned();
        core.workspace_changed();
        let (win, form) = surface(core, &dialog).await?;
        core.ui_event(&win, &form, json!({"action":"draft","rule":rule}))?;
        core.ui_event(&win, &form, json!({"action":"save"}))?;
        let plan = core.ui_close_prepare(&win, vec![context.workspace_id.clone()], Some(dialog))?;
        core.ui_close_finish(
            plan["token"].as_str().ok_or("没有关闭 token")?,
            serde_json::from_value(plan["views"].clone()).map_err(|e| e.to_string())?,
            "discard",
        )?;
        core.workspace_changed();
        core.ui_event(&window, &token, json!({"action":"start","id":id}))?;
        let addr = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let snapshot = core.ui_event(&window, &token, json!({"action":"snapshot"}))?;
                let row = snapshot["catalog"]["rules"]
                    .as_array()
                    .ok_or("没有规则列表")?
                    .iter()
                    .find(|r| r["rule"]["id"] == id)
                    .ok_or("规则未保存")?;
                let state = &row["run"]["snapshot"];
                if state["state"] == "failed" {
                    return Err(format!("原生网络启动失败：{}", state["error"]));
                }
                if state["state"] == "running" {
                    return Ok(format!(
                        "127.0.0.1:{}",
                        state["address"]["port"].as_u64().ok_or("缺少真实端口")?
                    )
                    .parse::<std::net::SocketAddr>()
                    .map_err(|e| e.to_string())?);
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .map_err(|_| "原生网络监听超时")??;
        running.addresses.push(addr);
    }
    // The listener's target service port is carried only by this validation object.
    running
        .addresses
        .push(format!("127.0.0.1:{port}").parse().unwrap());
    running.verify().await?;
    Ok(Some(running))
}
async fn surface(core: &AppCore, view: &str) -> Result<(String, String), String> {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if let Some(pair) = core.ui_ready_surface(view) {
                return pair;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .map_err(|_| "原生网络界面未挂载".into())
}
impl Running {
    pub(super) async fn verify(&self) -> Result<(), String> {
        for (index, addr) in self.addresses.iter().take(2).enumerate() {
            let work = async {
                let mut socket = TcpStream::connect(addr).await.map_err(|e| e.to_string())?;
                if index == 1 {
                    socket
                        .write_all(&[5, 1, 0])
                        .await
                        .map_err(|e| e.to_string())?;
                    let mut method = [0; 2];
                    socket
                        .read_exact(&mut method)
                        .await
                        .map_err(|e| e.to_string())?;
                    if method != [5, 0] {
                        return Err("原生 SOCKS5 协商失败".into());
                    }
                    let mut req = vec![5, 1, 0, 1, 127, 0, 0, 1];
                    req.extend_from_slice(&self.addresses[2].port().to_be_bytes());
                    socket.write_all(&req).await.map_err(|e| e.to_string())?;
                    let mut reply = [0; 10];
                    socket
                        .read_exact(&mut reply)
                        .await
                        .map_err(|e| e.to_string())?;
                    if reply[1] != 0 {
                        return Err("原生 SOCKS5 目标连接失败".into());
                    }
                }
                let bytes = vec![37; 32768];
                socket.write_all(&bytes).await.map_err(|e| e.to_string())?;
                socket.shutdown().await.map_err(|e| e.to_string())?;
                let mut output = Vec::new();
                socket
                    .read_to_end(&mut output)
                    .await
                    .map_err(|e| e.to_string())?;
                if output != bytes {
                    return Err("原生网络转发正文不一致".into());
                }
                Ok::<(), String>(())
            };
            tokio::time::timeout(Duration::from_secs(15), work)
                .await
                .map_err(|_| "原生网络数据超时")??;
        }
        Ok(())
    }
}

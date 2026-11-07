//! Small built-in list of official download sources; plugin programs stay external.
use crate::{model::UpdateSource, updates::REPOSITORY};
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OfficialPlugin {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub tag_prefix: &'static str,
}

pub const OFFICIAL_PLUGINS: &[OfficialPlugin] = &[
    OfficialPlugin {
        id: "sodeck.network-proxy",
        name: "网络隧道",
        description: "通过当前服务器建立本地 TCP 转发和 SOCKS5 代理，按规则查看连接与流量。",
        tag_prefix: "plugin-network-proxy-v",
    },
    OfficialPlugin {
        id: "sodeck.terminal",
        name: "终端",
        description: "通过当前服务器连接打开真实终端，支持多个终端页和跨窗口接续。",
        tag_prefix: "plugin-terminal-v",
    },
    OfficialPlugin {
        id: "sodeck.files",
        name: "文件",
        description: "浏览服务器目录、上传下载、管理文件，支持 UTF-8 小文本编辑和草稿导出。",
        tag_prefix: "plugin-files-v",
    },
    OfficialPlugin {
        id: "sodeck.system-status",
        name: "系统状态",
        description: "读取服务器设备信息、CPU、内存、磁盘和网络状态，隐藏面板继续采集。",
        tag_prefix: "plugin-system-status-v",
    },
];

pub fn source(id: &str) -> Result<UpdateSource, String> {
    let plugin = OFFICIAL_PLUGINS
        .iter()
        .find(|p| p.id == id)
        .ok_or("未知的官方插件")?;
    Ok(UpdateSource {
        repository: REPOSITORY.into(),
        tag_prefix: plugin.tag_prefix.into(),
    })
}

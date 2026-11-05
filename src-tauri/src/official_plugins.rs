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
        id: "sodeck.hello",
        name: "Hello 插件",
        description: "验证独立插件的启动、日志和重要消息。",
        tag_prefix: "plugin-hello-v",
    },
    OfficialPlugin {
        id: "sodeck.ui-example",
        name: "组件交互示例",
        description:
            "体验文件列表、编辑草稿、多终端和网络侧栏的布局；仅为交互示例，不执行远程业务。",
        tag_prefix: "plugin-ui-example-v",
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

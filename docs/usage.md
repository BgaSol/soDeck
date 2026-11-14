# SoDeck 使用指南

本指南帮助完成安装和第一次远程工作。详细操作及限制由各功能文档维护；仓库文档描述当前源码，待发行变化会单独标明。使用旧版本时可查看对应 Git 标签的文档与发行说明。

## 安装与首次连接

1. 从 [GitHub Releases](https://github.com/BgaSol/soDeck/releases) 下载对应系统和架构的核心包。支持 Windows x86_64/ARM64、Linux x86_64/ARM64、macOS Apple Silicon ARM64。
2. 点击顶部 SoDeck → 插件，选择官方插件或添加本地目录。安装需要的终端、文件、网络隧道、系统状态或 Docker，启用后重启。
3. 在管理台的服务器连接页新增 SSH 资料，填写主机、端口、用户名和密码/私钥；需要跳板时按顺序选择已有 SSH/SOCKS5 资料。
4. 连接服务器，核对主机指纹并补填必要凭据。成功后进入工作区，用中央“＋”新建终端，用两侧工具入口打开文件等插件。
5. 在工作台中调整标签、分屏和窗口。退出前按提示保存或放弃未保存内容，等待资源结束。

选择记住的密码和私钥口令当前以明文存入应用数据，私钥只保存路径；可以取消记住。完整保存、指纹、取消与重连规则见[服务器与连接](features/connections.md)。

## 按任务查阅

| 需要做什么 | 说明 |
| --- | --- |
| 管理资料、跳板、认证和连接 | [服务器与连接](features/connections.md) |
| 标签、分屏、工具停靠、独立窗口 | [工作台](features/workbench.md) |
| 执行命令、目录识别、查找历史输出 | [终端](features/terminal.md) |
| 文件树、上传下载、编辑、搜索及跟随目录 | [文件](features/files.md) |
| TCP 转发、SOCKS5、实际入口与流量 | [网络隧道](features/network-proxy.md) |
| CPU、内存、容量、网络和磁盘指标 | [系统状态](features/system-status.md) |
| 查看容器、日志和执行单项操作 | [Docker](features/docker.md) |
| 安装、启停或移除插件 | [插件管理](features/plugins.md) |
| 更新核心/插件、管理下载备用地址 | [在线更新](features/updates.md) |
| 外观、字体、组件偏好及系统打开 | [设置](features/settings.md) |
| 关闭、取消、保存失败或重启 | [生命周期](features/lifecycle.md) |
| 查看错误来源、日志和消息 | [日志与重要消息](features/logs-messages.md) |

## 数据与升级

应用设置、服务器资料、插件数据、日志和消息保存在系统应用数据目录，独立于安装目录。设置的“应用与数据”提供相关信息和使用帮助。资料异常时保留原文件，按可用备份恢复，不通过重新添加空资料覆盖原件。

核心与插件独立升级。Windows、macOS、Linux AppImage 可使用应用内核心更新；DEB 走系统安装器。插件启停和已加载插件升级在重启后生效，下载不会自动重启。当前功能和限制分别见上表，历史版本变化与发布来源见[版本记录](releases/)及插件 CHANGELOG。

返回[项目首页](../README.md) · [文档索引](README.md)。

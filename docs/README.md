# SoDeck 文档

正式文档与当前 Git 分支的代码一起维护，按功能持续更新。规划中的内容放在[临时计划](plans/)，完成后归入正式说明并清理计划。发布与验证证据保留在[核心版本记录](releases/)和各插件 CHANGELOG。

## 使用与产品

- [使用指南](usage.md)：安装、第一次连接与按任务查阅。
- [产品定义](product.md)：面向用户、当前范围和长期边界。

## 核心功能

| 功能 | 内容 |
| --- | --- |
| [服务器与连接](features/connections.md) | 资料、凭据、主机身份、混合跳板、取消与重连 |
| [工作台](features/workbench.md) | 标签、分屏、多窗口、工具停靠和布局记忆 |
| [插件管理](features/plugins.md) | 官方/本地安装、启停、进程和故障 |
| [状态总览](features/status.md) | 整体健康、服务器网络、插件运行与暂停显示 |
| [生命周期](features/lifecycle.md) | 保存、取消、关闭、退出和重启 |
| [在线更新](features/updates.md) | 核心/插件更新、缓存、校验与下载网络 |
| [设置](features/settings.md) | 外观、组件偏好、字体和桌面行为 |
| [日志与重要消息](features/logs-messages.md) | 来源、查询、消息、未读及通知 |

## 插件功能

| 功能 | 独立版本与发布证据 |
| --- | --- |
| [终端](features/terminal.md) | [CHANGELOG](../plugins/terminal/CHANGELOG.md) |
| [文件](features/files.md) | [CHANGELOG](../plugins/files/CHANGELOG.md) |
| [网络隧道](features/network-proxy.md) | [CHANGELOG](../plugins/network-proxy/CHANGELOG.md) |
| [系统状态](features/system-status.md) | [CHANGELOG](../plugins/system-status/CHANGELOG.md) |
| [Docker](features/docker.md) | [CHANGELOG](../plugins/docker/CHANGELOG.md) |
| [Agent MCP](features/mcp.md) | [CHANGELOG](../plugins/mcp/CHANGELOG.md) |

## 开发与交付

- [开发约定](../AGENTS.md)：开发边界、质量门槛及任务完成条件。
- [当前架构](architecture.md)：分层、资源归属、显示交接和系统适配。
- [界面设计系统](../DESIGN.md)：三主题、公共控件、核心尺寸和插件视觉边界。
- [通用文件编辑 SDK](features/file-editor.md)：组件自己的编辑页、草稿、保存、导出与关闭接入。
- [插件 API](plugin-api.md)：清单、RPC、能力及兼容契约。
- [MCP API](mcp-api.md)：Agent MCP 的资料发现、打开/取消、歧义与任务身份、工具、查询分页、输出模式、等待与错误契约。
- [开发指南](development.md)：环境、命令、检查范围与文档收束流程。
- [分发文档](distribution.md)：五平台原生打包、签名、独立发布和失败恢复。
- [临时计划](plans/)：尚未完成的改动或专项；不代表当前能力。
- [核心 1.7.0](releases/v1.7.0.md)：设置合并导航、紧凑表单及本轮五平台发行证据。
- [核心 1.9.0](releases/v1.9.0.md)：通用保存资料读取、共享连接接纳和原请求取消；Agent MCP 0.6.0 独立使用此能力。
- [核心 1.9.1](releases/v1.9.1.md)：访问路径图标、真实连接状态连线及 Windows 安装范围选择。
- [核心 1.8.1](releases/v1.8.1.md)：通用 Windows/POSIX 远端程序启动、原始字节、启动身份检查及环境结果预算；Docker 0.6.0 独立使用此能力。
- [核心版本记录](releases/)与 [GitHub Releases](https://github.com/BgaSol/soDeck/releases)：版本变化、原始包来源和验证范围；[核心 1.6.0](releases/v1.6.0.md)记录可靠会话目标摘要与旧接口兼容，[核心 1.5.0](releases/v1.5.0.md)记录支持 MCP 排障插件的通用会话、命令流和证据复制能力，[核心 1.4.0](releases/v1.4.0.md)记录复制服务器预填与凭据沿用，[核心 1.3.3](releases/v1.3.3.md)记录动效连续性改进，[核心 1.3.1](releases/v1.3.1.md)记录插件加载底色与默认停靠修复，[核心 1.3.0](releases/v1.3.0.md)记录工作台动效，[核心 1.2.3](releases/v1.2.3.md)记录状态总览重构，[核心 1.2.2](releases/v1.2.2.md)记录连接树箭头与连线优化，[核心 1.2.1](releases/v1.2.1.md)记录管理台侧栏样式与分组调整，[核心 1.2.0](releases/v1.2.0.md)记录连接目录交互调整，[核心 1.1.1](releases/v1.1.1.md)记录管理台导航调整，[核心 1.1.0](releases/v1.1.0.md)记录日志工作台的核心交付与实际验收。

返回[项目首页](../README.md)。

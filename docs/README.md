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

## 开发与交付

- [开发约定](../AGENTS.md)：开发边界、质量门槛及任务完成条件。
- [当前架构](architecture.md)：分层、资源归属、显示交接和系统适配。
- [插件 API](plugin-api.md)：清单、RPC、能力及兼容契约。
- [开发指南](development.md)：环境、命令、检查范围与文档收束流程。
- [分发文档](distribution.md)：五平台原生打包、签名、独立发布和失败恢复。
- [临时计划](plans/)：尚未完成的改动或专项；不代表当前能力。
- [核心版本记录](releases/)与 [GitHub Releases](https://github.com/BgaSol/soDeck/releases)：版本变化、原始包来源和验证范围。

返回[项目首页](../README.md)。

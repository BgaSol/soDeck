# SoDeck 文档索引

## 使用与开发

- [使用指南](usage.md)：安装插件、连接服务器、工作台操作和数据存储。
- [开发指南](development.md)：开发环境、检查、测试和打包入口。
- [分发文档](distribution.md)：平台产物、在线更新和 GitHub Releases 发布流程。
- [插件 API v1](plugin-api.md)：核心能力、RPC、生命周期和兼容要求。
- 官方插件说明：[终端](../plugins/terminal/README.md)、[文件](../plugins/files/README.md)、[网络隧道](../plugins/network-proxy/README.md)。

## 产品与架构

- [产品定义](product.md)：用户场景、产品范围和功能边界。
- [架构与边界](architecture.md)：核心与插件职责、连接服务和系统适配。
- [开发约定](../AGENTS.md)：质量门槛与交付规则。

## 功能方案与实施记录

功能方案描述目标和可验收行为；实施记录描述实际完成情况、测试结果、未完成项和已知限制。已发布版本的范围以正式 Release 和对应版本说明为准。

| 主题 | 功能与代码方案 | 实施与验收 |
| --- | --- | --- |
| 服务器管理 | [资料管理](plan-server-management.md) | — |
| SSH 与混合连接 | [SSH 直连](plan-ssh-direct.md)、[远程工作台](plan-remote-workbench.md) | [混合连接](implementation-connections.md) |
| 工作区与组件宿主 | [多窗口工作区](plan-server-workspaces.md)、[组件 UI](plan-component-ui.md) | — |
| 真实终端与文件 | [远程工作台](plan-remote-workbench.md) | [真实插件](implementation-remote-plugins.md) |
| 文件目录树 | [文件树设计](plan-file-tree.md) | [文件树](implementation-file-tree.md) |
| 文件管理与传输 | [功能范围](plan-file-management.md)、[编码方案](plan-file-management-coding.md) | [文件管理](implementation-file-management.md) |
| 网络隧道 | [插件方案](plan-network-proxy.md) | [网络隧道](implementation-network.md) |
| 工作台样式与剪贴板 | [设计方案](plan-workbench-design.md) | [样式与剪贴板](implementation-workbench-design.md) |
| Tab 新建入口 | [交互方案](plan-tab-creation.md) | [两级 Tab](implementation-tab-creation.md) |
| 工作区菜单 | — | [菜单修复](implementation-component-menus.md) |
| 工作台按钮与文件菜单 | [整改设计](plan-workbench-interactions.md) | [实施与验收](implementation-workbench-interactions.md) |
| 桌面一致性 | [滚动条与默认行为](plan-desktop-consistency.md) | [桌面一致性](implementation-desktop-consistency.md) |
| 组件设置与终端外观 | [功能设计](plan-component-settings.md)、[编码方案](plan-component-settings-coding.md) | [组件设置](implementation-component-settings.md) |
| 在线更新 | [基础方案](plan-online-updates.md)、[体验改版](plan-update-experience.md) | [更新体验](implementation-update-experience.md) |
| 打包与发布工作流 | [分步优化计划](plan-packaging-workflows.md) | [实施与验收](implementation-packaging-workflows.md) |

## 版本与历史资料

- [GitHub Releases](https://github.com/BgaSol/soDeck/releases)：已发布的核心和插件版本、下载附件与发布说明。
- [核心版本记录](releases/)：各版本的功能变化、验收与交付边界，包含历史候选版本记录。
- 插件变更记录：[终端](../plugins/terminal/CHANGELOG.md)、[文件](../plugins/files/CHANGELOG.md)、[网络隧道](../plugins/network-proxy/CHANGELOG.md)。
- [外置插件基础实施计划](plan-plugin-foundation.md)、[代码整改方案](plan-code-reorganization.md) 与 [旧版功能和界面提取](legacy-review.md)：早期规划和演进背景。

## 文档分工

| 文档 | 维护内容 |
| --- | --- |
| 根目录 README | 项目定位、主要能力、支持平台、下载、快速开始和固定文档入口 |
| 使用指南 | 用户操作与实际使用限制 |
| 开发指南、分发文档、插件 API | 开发命令、发布规则与接口契约 |
| 产品定义、架构文档、功能方案 | 产品要求、架构决策和可验收行为 |
| 实施记录 | 实现细节、测试证据、待验收事项和风险 |
| 版本说明、插件 CHANGELOG | 对应版本的变化与升级注意事项 |
| 本索引 | 文档导航，新增文档时补充对应入口 |

返回 [项目首页](../README.md)

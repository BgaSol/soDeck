# SoDeck 文档索引

## 使用与开发

- [使用指南](usage.md)：安装插件、连接服务器、工作台操作和数据存储。
- [开发指南](development.md)：开发环境、检查、测试和打包入口。
- [分发文档](distribution.md)：平台产物、在线更新和 GitHub Releases 发布流程。
- [优化前工作流全景图](workflows-current.md)：五个 Actions 工作流、平台步骤、缓存、产物流转、重试和历史耗时。
- [插件 API v1](plugin-api.md)：核心能力、RPC、生命周期和兼容要求。
- 插件说明：[终端](../plugins/terminal/README.md)、[文件](../plugins/files/README.md)、[网络隧道](../plugins/network-proxy/README.md)、[系统状态](../plugins/system-status/README.md)、[Docker](../plugins/docker/README.md)。

## 产品与架构

- [产品定义](product.md)：用户场景、产品范围和功能边界。
- [架构与边界](architecture.md)：核心与插件职责、连接服务和系统适配。
- [功能设计方法](plan-feature-design.md)：需求理解、职责与完整生命周期、失败处理和验收提纲；当前为设计提案。
- [开发约定](../AGENTS.md)：质量门槛与交付规则。

## 功能方案与实施记录

- [GLib 兼容安全补丁](implementation-glib-variant-iter.md)：GTK3 依赖链、VariantStrIter 上游补丁回移及 Linux 优化构建回归。
功能方案描述目标和可验收行为；实施记录描述实际完成情况、测试结果、未完成项和已知限制。已发布版本的范围以正式 Release 和对应版本说明为准。

| 主题 | 功能与代码方案 | 实施与验收 |
| --- | --- | --- |
| 全页面体验修复 | [逐项修复与验收方案](plan-ui-remediation.md) | [实施与验收](implementation-ui-remediation.md) |
| 服务器管理 | [资料管理](plan-server-management.md) | — |
| SSH 与混合连接 | [SSH 直连](plan-ssh-direct.md)、[远程工作台](plan-remote-workbench.md) | [混合连接](implementation-connections.md) |
| 工作区与组件宿主 | [多窗口工作区](plan-server-workspaces.md)、[组件 UI](plan-component-ui.md) | — |
| 工作台分屏与多窗口体系 | [完整功能设计](plan-workbench-layout.md)，含视图归属、工具停靠、插件 UI v2、迁移与关闭 | [源码实施与验收](implementation-workbench-layout.md) |
| 侧边工具入口与停靠 | [功能设计](plan-tool-docking.md)、[代码整改方案](plan-tool-docking-coding.md) | [实施与本机验证](implementation-tool-docking.md) |
| 组件通信与共享状态 | [功能设计](plan-component-communication.md)、[代码实施方案](plan-component-communication-coding.md) | [实施与验收](implementation-component-communication.md) |
| 终端目录自动识别 | [首次连接、自动集成与跟随体验](plan-terminal-directory.md) | [实施与验收](implementation-terminal-directory.md) |
| 关闭、退出与重启 | [完整生命周期设计](plan-close-lifecycle.md)、[代码整改方案](plan-close-lifecycle-coding.md) | [实施与验收](implementation-close-lifecycle.md) |
| 真实终端与文件 | [远程工作台](plan-remote-workbench.md) | [真实插件](implementation-remote-plugins.md) |
| 文件目录树 | [文件树设计](plan-file-tree.md) | [文件树](implementation-file-tree.md) |
| 文件管理与传输 | [功能范围](plan-file-management.md)、[编码方案](plan-file-management-coding.md) | [文件管理](implementation-file-management.md) |
| 网络隧道 | [插件方案](plan-network-proxy.md) | [网络隧道](implementation-network.md) |
| Docker | [第一版范围与后续设计](plan-docker-plugin.md) | [实现、验证与平台限制](implementation-docker-plugin.md) |
| 系统状态 | [功能设计](plan-system-status.md)、[编码方案](plan-system-status-coding.md) | [实现、验证与平台限制](implementation-system-status.md)、[界面规范](../plugins/system-status/DESIGN.md) |
| 工作台样式与剪贴板 | [设计方案](plan-workbench-design.md) | [样式与剪贴板](implementation-workbench-design.md) |
| Tab 新建入口 | [交互方案](plan-tab-creation.md) | [两级 Tab](implementation-tab-creation.md) |
| 默认新建行为与通用终端 | [功能方案](plan-default-behavior-terminal.md)、[代码整改方案](plan-default-behavior-terminal-coding.md) | [实现、本机验证与兼容边界](implementation-default-behavior-terminal.md) |
| 工作区菜单 | [两级 Tab 与右键菜单精简](plan-tab-interactions.md) | [Tab 精简实施与验收](implementation-tab-interactions.md)、[既有菜单修复](implementation-component-menus.md) |
| 工作台按钮与文件菜单 | [整改设计](plan-workbench-interactions.md) | [实施与验收](implementation-workbench-interactions.md) |
| 面板拖拽与终端尺寸 | [状态归属、完整手势与尺寸同步](plan-panel-resize.md) | [实现与本机验收](implementation-panel-resize.md) |
| 桌面一致性 | [滚动条与默认行为](plan-desktop-consistency.md) | [桌面一致性](implementation-desktop-consistency.md) |
| 组件设置与终端外观 | [功能设计](plan-component-settings.md)、[编码方案](plan-component-settings-coding.md) | [组件设置](implementation-component-settings.md) |
| 在线更新 | [基础方案](plan-online-updates.md)、[体验改版](plan-update-experience.md) | [更新体验](implementation-update-experience.md) |
| 更新下载与加速 | [多地址切换、用户设置与代码整改](plan-update-acceleration.md) | [实施与验收](implementation-update-acceleration.md) |
| 打包与发布工作流 | [发布提速修改方案](plan-release-performance.md)、[既有分步优化计划](plan-packaging-workflows.md) | [提速实施与验收](implementation-release-performance.md)、[既有实施与验收](implementation-packaging-workflows.md) |
| 基础逻辑与成熟库复用 | [分步整改计划](plan-standard-library-adoption.md)，含三系统本地文件适配详细方案 | [版本、文件 URI 与菜单](implementation-standard-library-adoption.md) |
| 本机系统绑定 | [功能与代码设计](plan-native-system-bindings.md)，覆盖运行代码、原生测试和系统打开入口 | [实现与原生验收](implementation-native-system-bindings.md) |
| SOCKS5 协议库复用 | [核心、网络插件与检查器完整计划](plan-socks5-library-adoption.md) | [接入与验证](implementation-standard-library-adoption.md#第六阶段socks5-报文库接入) |

## 版本与历史资料

- [GitHub Releases](https://github.com/BgaSol/soDeck/releases)：已发布的核心和插件版本、下载附件与发布说明。
- [核心版本记录](releases/)：各版本的功能变化、验收与交付边界，包含历史候选版本记录。
- [v0.21.0](releases/v0.21.0.md)：工具按钮与面板共同停靠、拖动排序、布局记忆及终端入口调整。
- [v0.20.1](releases/v0.20.1.md)：Docker 标准图标、右侧推荐入口及本地添加插件的键盘焦点修复。
- [v0.19.0](releases/v0.19.0.md)：精简两级 Tab 右键菜单、取消分屏保留页面及空标签组关闭修复。
- [v0.18.0](releases/v0.18.0.md)：GitHub 全链路备用地址、失败切换和可管理的下载网络设置。
- [v0.17.0](releases/v0.17.0.md)：嵌套分屏、独立窗口、来源返回与四个 UI v2 插件；发行状态见记录。
- [v0.16.0](releases/v0.16.0.md)：插件声明默认新建行为、进程主动打开页面与通用终端创建参数。
- [v0.14.0](releases/v0.14.0.md)：终端自动识别目录、文件跟随状态与文件系统对应检查。
- [v0.13.0](releases/v0.13.0.md)：核心共享状态、终端目录跟随与挂载容量联动。
- [v0.12.3](releases/v0.12.3.md)：面板拖动状态、完整手势生命周期及终端尺寸同步整改。
- [v0.12.2](releases/v0.12.2.md)：macOS 原生窗口按钮在标题栏中垂直居中，主窗口与拆出窗口保持一致。
- [v0.12.1 候选](releases/v0.12.1.md)：原生按钮坐标验收未通过，保留未发行候选来源。
- [v0.12.0](releases/v0.12.0.md)：统一关闭、退出与重启生命周期，取消后保留原工作台和资源。
- [v0.11.0](releases/v0.11.0.md)：官方插件从仓库自动发现、插件安装与设置更新入口调整。
- [v0.10.0](releases/v0.10.0.md)：系统状态插件官方安装、后台采集和有界短命令支持。
- [v0.9.4](releases/v0.9.4.md)：标题栏加号位置修复及验证范围。
- [v0.9.3](releases/v0.9.3.md)：本轮全页面体验修复及独立插件升级说明，验收状态见实施记录。
- [v0.9.2](releases/v0.9.2.md)：工作台交互、协议库复用及独立插件升级说明。
- 插件变更记录：[终端](../plugins/terminal/CHANGELOG.md)、[文件](../plugins/files/CHANGELOG.md)、[网络隧道](../plugins/network-proxy/CHANGELOG.md)、[系统状态](../plugins/system-status/CHANGELOG.md)、[Docker](../plugins/docker/CHANGELOG.md)。
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

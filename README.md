# SoDeck

面向运维人员的远程服务器工作台。v0.7.1 退役 Hello 和组件交互示例，官方插件仅保留终端、文件和网络隧道；升级启动前自动移除旧示例登记。五目标发布验证进行中，范围见 [v0.7.1](docs/releases/v0.7.1.md)。已发布的 [Latest](https://github.com/BgaSol/soDeck/releases/latest) 提供正式安装包。核心与业务插件独立分发，核心包不包含业务插件。

外置插件声明侧栏、中央页面及编辑弹窗，核心统一管理两侧入口、混合 Tab、布局偏好、关闭确认和跨窗重挂载。`plugins/terminal` 提供真实多 Tab 终端；`plugins/files` 提供目录、1 MiB 内 UTF-8 文件编辑、保存冲突检查和单文件下载；`plugins/network-proxy` 提供按服务器管理的 TCP 转发与 SOCKS5 代理，复用现有混合跳板，域名由最终远端解析。核心包不包含这些插件。

当前版本沿用旧版紧凑工作台：文件侧栏提供真正的多分支目录树、路径定位、独立分页、右键操作和键盘导航；单击选中、双击文件打开中央编辑器。隐藏、重挂载和移窗保留导航状态。多巴胺、浅色、深色主题同步到文件与终端。新版文件插件 0.2.0 需要核心 0.6.1 的目录游标能力，旧版文件插件仍可使用原来的接口。范围见 [文件树设计](docs/plan-file-tree.md)，验证和限制见 [实施记录](docs/implementation-file-tree.md)，发行说明见 [v0.6.1](docs/releases/v0.6.1.md)。

核心支持主机指纹、密码和私钥认证，以及工作台断开/原位重连；插件控制通信支持有界并发和退出取消。正文数据桥使用 16 KiB 分块；终端有确认、背压与跨窗显示恢复。实现与验收边界见 [真实插件实施记录](docs/implementation-remote-plugins.md)，本版说明见 [v0.6.0](docs/releases/v0.6.0.md)。

v0.5.1 修复正式构建中服务器表单无法滚动、布局错位及多巴胺配色不完整的问题，保留已保存的主题。修复范围及验证见 [v0.5.1](docs/releases/v0.5.1.md)。

v0.5.2 补齐设置中的官方插件下载安装入口，并独立发行组件交互示例。该已发行版本的两个官方插件均为示例；真实终端与文件插件从 v0.6.0 开发版本提供。发行及验证见 [v0.5.2](docs/releases/v0.5.2.md)。

核心与插件独立版本、独立打包，从公开的 [GitHub Releases](https://github.com/BgaSol/soDeck/releases) 分发。核心安装包不含业务插件。版本与验收记录见 [v0.1.1](docs/releases/v0.1.1.md)、[v0.2.0](docs/releases/v0.2.0.md)、[v0.2.1](docs/releases/v0.2.1.md)、[v0.3.0](docs/releases/v0.3.0.md)、[v0.4.0](docs/releases/v0.4.0.md) 和 [v0.5.0](docs/releases/v0.5.0.md)。

## 使用

工作台直接管理服务器与 SOCKS5 代理：新增、编辑、复制、删除、搜索和分组；SSH 与 SOCKS5 路径可混合排序。记住的密码和私钥口令按选择明文保存到应用数据目录的 `servers.json`，私钥只保存路径。未记住的秘密仅当前进程有效。数据异常时保留原文件，可从上一份有效备份恢复。

1. 下载对应系统和架构的核心安装包，或从旧版的应用更新入口升级。
2. 先更新核心，再在 **设置 → 插件管理 → 官方插件** 点击“下载并启用”，客户端下载对应正式 Release 的本机版本。v0.7.0 配套终端 0.1.2、文件 0.2.1 和网络隧道 0.1.0，均已正式发布并可独立下载。官方列表仅保留终端、文件和网络隧道；其他本地插件仍通过“添加插件目录”选择包含 plugin.json 的目录。
3. 重启后自动加载启用插件；启用、关闭和已启用插件更新都在重启后生效。多项修改可统一重启一次。
4. 插件详情检查与下载其独立更新；核心更新位于 **设置 → 应用更新**。
5. **设置 → 外观**可选多巴胺、跟随系统、浅色和深色，即选即保存。多巴胺沿用旧版奶油底、粉色导航、黄色选中、薄荷绿点缀与深色粗描边，无装饰阴影；升级保留当前偏好。

终端和文件使用独立 Release。开发验收也可以独立打包后添加 `artifacts/plugins/sodeck-terminal_<版本>_<target>` 和 `sodeck-files_<版本>_<target>` 目录。核心安装包保持不含插件。

Hello 与组件交互示例已退役：源码、官方安装来源和发行配置已删除，升级后自动移除旧登记。原外部程序、数据、消息和日志按移除记录规则保留。宿主回归使用仅测试的夹具，不再发行示例插件；历史版本记录仍可查阅。没有更新来源的其他插件继续手动维护。

Windows x86_64/ARM64 使用 EXE，Linux x86_64/ARM64 提供 DEB 与 AppImage，Apple Silicon macOS 使用 DMG。Linux 的应用内升级使用 AppImage；DEB 继续使用系统包管理器。

应用无独立标题栏，状态、设置、消息位于右上角浮层。插件日志和重要消息独立；普通错误日志不自动弹通知。设置、消息、日志和插件数据存于系统应用数据目录，独立于安装目录。

可从服务器详情点击“连接服务器”；每站指纹确认后才提交该站凭据。连接中的补填仅用于这次尝试；记住凭据仍通过编辑资料完成。工作台底部可以断开或手动重连，右上角应用状态显示连接数量。

## 开发与发布

固定 Node.js 24.22.0、Rust 1.99.1，需要 [Tauri 系统构建依赖](https://v2.tauri.app/start/prerequisites/)。

```sh
npm ci
npm run dev
npm run check
npm run plugin:check
npm run scripts:test
npm run test
npx playwright install chromium
npm run ui:test
npm run terminal:input:test
# 本机需 OpenSSH 服务端与 ssh-keygen；脚本仅启动临时回环测试端
npm run ssh:test
npm run terminal:test
npm run files:test
npm run network:test
npm run build -- --bundles nsis -- --locked
npm run plugin:build -- --plugin terminal
npm run plugin:build -- --plugin files
npm run plugin:build -- --plugin network-proxy
```

核心版本只修改 `src-tauri/tauri.conf.json`，插件版本只修改各自的 `plugins/<名称>/Cargo.toml`。核心标签 `v<版本>` 和插件标签 `plugin-terminal-v<版本>`、`plugin-files-v<版本>`、`plugin-network-proxy-v<版本>` 分别触发五目标独立构建及 Release 草稿；更新清单自动生成，检查后正式发布。普通本地打包不需要更新签名密钥。

| 工作流 | 用途 |
| --- | --- |
| Check | 类型、Rust 格式、发布脚本测试及前端构建 |
| Package SoDeck | 五目标原生核心安装包、目录保存重开、原生工作区移窗/关闭烟测、更新签名；标签发布时生成 core-update.json |
| Package plugin | 按插件目录独立打包、生成 plugin-update.json |
| Verify native remote plugins | 独立打包的实际核心和终端/文件/网络隧道包，验证原生数据桥、OpenSSH 与网络业务 |

详细步骤、签名密钥与验证边界见 [分发文档](docs/distribution.md)。Windows 未配置商业代码签名，macOS 使用 ad-hoc 签名；更新包签名由 Tauri updater 单独处理。插件按可信扩展使用，没有权限清单和逐项授权。

## 设计与验收

- [产品范围](docs/product.md)
- [架构与边界](docs/architecture.md)
- [服务器管理设计（暂不建连）](docs/plan-server-management.md)
- [服务器 Tab 与可拆合工作区设计](docs/plan-server-workspaces.md)
- [组件与核心 UI 交互设计（提案）](docs/plan-component-ui.md)
- [远程工作台：混合跳板、终端、文件与带宽统计（提案）](docs/plan-remote-workbench.md)
- [远程工作台代码整改方案（提案）](docs/plan-code-reorganization.md)
- [网络隧道插件设计与实施范围](docs/plan-network-proxy.md)
- [工作台统一样式与终端剪贴板设计](docs/plan-workbench-design.md)
- [工作台样式与剪贴板实施、验证边界](docs/implementation-workbench-design.md)
- [插件 API v1](docs/plugin-api.md)
- [在线更新实施与验收](docs/plan-online-updates.md)
- [上一阶段实施计划](docs/plan-plugin-foundation.md)
- [旧版功能和界面提取](docs/legacy-review.md)

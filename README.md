# SoDeck

面向运维人员的远程服务器工作台。当前开发版本 v0.6.0 在 SSH / SOCKS5 混合多层连接和多窗口工作台上，增加真实终端、SFTP 文件插件和服务器/插件业务流量统计。普通启动显示服务器管理，真实认证成功后才创建工作区。

外置插件声明侧栏与中央页面，核心统一管理两侧入口、混合 Tab、布局偏好、关闭确认和跨窗重挂载。`plugins/terminal` 提供真实多 Tab 终端；`plugins/files` 提供目录、1 MiB 内 UTF-8 文件编辑、保存冲突检查和单文件下载。`plugins/ui-example` 仅保留交互测试。核心包不包含这些插件。

核心支持主机指纹、密码和私钥认证，以及工作台断开/原位重连；插件控制通信支持有界并发和退出取消。正文数据桥使用 16 KiB 分块；终端有确认、背压与跨窗显示恢复。实现与验收边界见 [真实插件实施记录](docs/implementation-remote-plugins.md)。v0.6.0 尚待正式发行，不代表 GitHub Latest 已升级。

v0.5.1 修复正式构建中服务器表单无法滚动、布局错位及多巴胺配色不完整的问题，保留已保存的主题。修复范围及验证见 [v0.5.1](docs/releases/v0.5.1.md)。

v0.5.2 补齐设置中的官方插件下载安装入口，并独立发行组件交互示例。点击下载后重启启用；现有两个官方插件均为示例，真实终端与文件业务仍待实现。发行及验证见 [v0.5.2](docs/releases/v0.5.2.md)。

核心与插件独立版本、独立打包，从公开的 [GitHub Releases](https://github.com/BgaSol/soDeck/releases) 分发。核心安装包不含业务插件。版本与验收记录见 [v0.1.1](docs/releases/v0.1.1.md)、[v0.2.0](docs/releases/v0.2.0.md)、[v0.2.1](docs/releases/v0.2.1.md)、[v0.3.0](docs/releases/v0.3.0.md)、[v0.4.0](docs/releases/v0.4.0.md) 和 [v0.5.0](docs/releases/v0.5.0.md)。

## 使用

工作台直接管理服务器与 SOCKS5 代理：新增、编辑、复制、删除、搜索和分组；SSH 与 SOCKS5 路径可混合排序。记住的密码和私钥口令按选择明文保存到应用数据目录的 `servers.json`，私钥只保存路径。未记住的秘密仅当前进程有效。数据异常时保留原文件，可从上一份有效备份恢复。

1. 下载对应系统和架构的核心安装包，或从旧版的应用更新入口升级。
2. v0.5.2 起，在 **设置 → 插件管理 → 官方插件** 点击“下载并启用”，客户端自动下载本机版本并解压保存。现有 Hello 和组件交互示例均不提供真实终端或文件业务；其他本地插件仍通过“添加插件目录”选择包含 plugin.json 的目录。
3. 重启后自动加载启用插件；启用、关闭和已启用插件更新都在重启后生效。多项修改可统一重启一次。
4. 插件详情检查与下载其独立更新；核心更新位于 **设置 → 应用更新**。
5. **设置 → 外观**可选多巴胺、跟随系统、浅色和深色，即选即保存。多巴胺沿用旧版奶油底、粉色导航、黄色选中、薄荷绿点缀与深色粗描边，无装饰阴影；升级保留当前偏好。

v0.1.1 用户需手动安装一次带更新功能的版本。旧插件记录默认关闭，启用后重启；旧 Hello 可配置仓库 `BgaSol/soDeck`、标签前缀 `plugin-hello-v` 获取独立更新。没有来源的插件继续手动维护。

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
# 本机需 OpenSSH 服务端与 ssh-keygen；脚本仅启动临时回环测试端
npm run ssh:test
npm run terminal:test
npm run files:test
npm run build -- --bundles nsis -- --locked
npm run plugin:build
node scripts/package-plugin.mjs --plugin terminal
node scripts/package-plugin.mjs --plugin files
```

核心版本只修改 `src-tauri/tauri.conf.json`，Hello 版本只修改 `plugins/hello/Cargo.toml`。核心标签 `v<版本>` 和插件标签 `plugin-hello-v0.1.1` 分别触发五目标独立构建及 Release 草稿；更新清单自动生成，检查后正式发布。普通本地打包不需要更新签名密钥。

| 工作流 | 用途 |
| --- | --- |
| Check | 类型、Rust 格式、发布脚本测试及前端构建 |
| Package SoDeck | 五目标原生核心安装包、目录保存重开、原生工作区移窗/关闭烟测、更新签名；标签发布时生成 core-update.json |
| Package plugin | 按插件目录独立打包、生成 plugin-update.json |
| Verify packaged core and plugin | 五目标实际包的插件、日志、消息与重启保留 |
| Verify native remote plugins | 三个独立打包运行的实际核心/终端/文件包，验证原生数据桥与 OpenSSH 业务 |

详细步骤、签名密钥与验证边界见 [分发文档](docs/distribution.md)。Windows 未配置商业代码签名，macOS 使用 ad-hoc 签名；更新包签名由 Tauri updater 单独处理。插件按可信扩展使用，没有权限清单和逐项授权。

## 设计与验收

- [产品范围](docs/product.md)
- [架构与边界](docs/architecture.md)
- [服务器管理设计（暂不建连）](docs/plan-server-management.md)
- [服务器 Tab 与可拆合工作区设计](docs/plan-server-workspaces.md)
- [组件与核心 UI 交互设计（提案）](docs/plan-component-ui.md)
- [远程工作台：混合跳板、终端、文件与带宽统计（提案）](docs/plan-remote-workbench.md)
- [远程工作台代码整改方案（提案）](docs/plan-code-reorganization.md)
- [插件 API v1](docs/plugin-api.md)
- [在线更新实施与验收](docs/plan-online-updates.md)
- [上一阶段实施计划](docs/plan-plugin-foundation.md)
- [旧版功能和界面提取](docs/legacy-review.md)

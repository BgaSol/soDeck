# SoDeck

面向运维人员的远程服务器工作台。v0.4.0 在服务器资料管理基础上加入服务器工作区壳、原生多窗口归属、Tab 排序和拆合。SSH 与业务组件尚未接入，普通启动仍显示服务器管理，连接成功后才会产生工作区。测试使用隔离会话验证，不伪造真实连接。

核心与插件独立版本、独立打包，从公开的 [GitHub Releases](https://github.com/BgaSol/soDeck/releases) 分发。核心安装包不含业务插件。版本与验收记录见 [v0.1.1](docs/releases/v0.1.1.md)、[v0.2.0](docs/releases/v0.2.0.md)、[v0.2.1](docs/releases/v0.2.1.md) 、[v0.3.0](docs/releases/v0.3.0.md) 和 [v0.4.0](docs/releases/v0.4.0.md)。

## 使用

工作台直接管理服务器与 SOCKS5 代理：新增、编辑、复制、删除、搜索和分组；SSH 与 SOCKS5 路径可混合排序。记住的密码和私钥口令按选择明文保存到应用数据目录的 `servers.json`，私钥只保存路径。未记住的秘密仅当前进程有效。数据异常时保留原文件，可从上一份有效备份恢复。

1. 下载对应系统和架构的核心安装包与独立插件归档，解压插件。
2. 右上角 **设置 → 插件管理 → 添加插件目录**，选择包含 plugin.json 的目录。
3. 重启后自动加载启用插件；启用、关闭和已启用插件更新都在重启后生效。多项修改可统一重启一次。
4. 插件详情检查与下载其独立更新；核心更新位于 **设置 → 应用更新**。
5. **设置 → 外观**可选多巴胺、跟随系统、浅色和深色，即选即保存。多巴胺沿用旧版奶油底、粉色导航、黄色选中、薄荷绿点缀与深色粗描边，无装饰阴影；升级保留当前偏好。

v0.1.1 用户需手动安装一次带更新功能的版本。旧插件记录默认关闭，启用后重启；旧 Hello 可配置仓库 `BgaSol/soDeck`、标签前缀 `plugin-hello-v` 获取独立更新。没有来源的插件继续手动维护。

Windows x86_64/ARM64 使用 EXE，Linux x86_64/ARM64 提供 DEB 与 AppImage，Apple Silicon macOS 使用 DMG。Linux 的应用内升级使用 AppImage；DEB 继续使用系统包管理器。

应用无独立标题栏，状态、设置、消息位于右上角浮层。插件日志和重要消息独立；普通错误日志不自动弹通知。设置、消息、日志和插件数据存于系统应用数据目录，独立于安装目录。

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
npm run build -- --bundles nsis -- --locked
npm run plugin:build
```

核心版本只修改 `src-tauri/tauri.conf.json`，Hello 版本只修改 `plugins/hello/Cargo.toml`。核心标签 `v<版本>` 和插件标签 `plugin-hello-v0.1.1` 分别触发五目标独立构建及 Release 草稿；更新清单自动生成，检查后正式发布。普通本地打包不需要更新签名密钥。

| 工作流 | 用途 |
| --- | --- |
| Check | 类型、Rust 格式、发布脚本测试及前端构建 |
| Package SoDeck | 五目标原生核心安装包、目录保存重开、原生工作区移窗/关闭烟测、更新签名；标签发布时生成 core-update.json |
| Package plugin | 按插件目录独立打包、生成 plugin-update.json |
| Verify packaged core and plugin | 五目标实际包的插件、日志、消息与重启保留 |

详细步骤、签名密钥与验证边界见 [分发文档](docs/distribution.md)。Windows 未配置商业代码签名，macOS 使用 ad-hoc 签名；更新包签名由 Tauri updater 单独处理。插件按可信扩展使用，没有权限清单和逐项授权。

## 设计与验收

- [产品范围](docs/product.md)
- [架构与边界](docs/architecture.md)
- [服务器管理设计（暂不建连）](docs/plan-server-management.md)
- [服务器 Tab 与可拆合工作区设计](docs/plan-server-workspaces.md)
- [插件 API v1](docs/plugin-api.md)
- [在线更新实施与验收](docs/plan-online-updates.md)
- [上一阶段实施计划](docs/plan-plugin-foundation.md)
- [旧版功能和界面提取](docs/legacy-review.md)

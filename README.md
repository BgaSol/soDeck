# SoDeck

面向运维人员的远程服务器工作台。当前 v0.1.1 实现无独立标题栏的桌面壳、外置插件、统一日志和重要消息；SSH、终端、文件、Docker、NetBird 与网络代理在后续迭代实现。

## 使用

从 [Releases](https://github.com/BgaSol/soDeck/releases) 下载对应系统和架构的核心安装包。核心包不含插件，也不需要用户安装开发工具。

1. 独立下载对应平台的 Hello 插件并解压。
2. 右上角 **设置 → 插件管理 → 加载插件目录**，选择包含 `plugin.json` 的目录。
3. 加载后查看运行状态、日志和右上角消息；点击卸载即可停止，文件、目录记录和数据保留。

重新启动核心后手动加载已记录的插件。插件不做权限申请、签名验证、市场或自动更新。

应用状态位于右上角浮层；设置包含插件管理、浅色/深色/跟随系统主题、应用数据路径。重要消息保存已读状态；普通错误日志不会自动生成通知。

## 开发与打包

固定 Node.js 24.22.0、Rust 1.99.1；需要 [Tauri 系统构建依赖](https://v2.tauri.app/start/prerequisites/)。提交 npm 与两个 Cargo 锁文件。

```sh
npm ci
npm run dev
npm run check
npm run test
npm run build -- --bundles nsis -- --locked
npm run plugin:build
```

Windows 使用 `nsis`，Linux 使用 `deb`，macOS 使用 `dmg`。核心产物在 `src-tauri/target/release/bundle/`，指定 `--target` 后在对应目标子目录。插件在独立的 `artifacts/plugins/`，其构建不需要 `npm ci`，可直接 `node scripts/package-plugin.mjs`；核心构建不调用插件构建。

核心版本只修改 `src-tauri/tauri.conf.json`，Hello 版本只修改 `plugins/hello/Cargo.toml`。插件协议及开发方式见 [API v1](docs/plugin-api.md)。

## GitHub 交付

| 工作流 | 用途 |
| --- | --- |
| Check | 类型、Rust 格式及前端构建 |
| Package SoDeck | 手动生成五目标核心 Artifact；推送 `v<核心版本>` 标签生成 Release 草稿 |
| Package Hello plugin | 独立生成五目标插件归档；可填写已有 Release 标签上传附件 |
| Verify packaged core and plugin | 下载两次构建的实际产物，五目标原生验证加载、卸载、日志、消息与重启 |

五个正式目标为 Windows x86_64 / ARM64、Linux x86_64 / ARM64、macOS ARM64，ARM 使用原生 Runner 和原生程序。核心与插件分别打包、分别下载。检查草稿附件后正式发布，普通 Releases 列表才会显示。

升级时先退出程序，运行 Windows 新安装器、安装新版 Linux 包或替换 macOS 应用。数据使用系统应用数据目录，独立于安装目录；插件文件保留在用户选择的外部目录。

Windows 未配置商业代码签名，macOS 使用 ad-hoc 签名。仓库目前为私有，查看与下载需有仓库访问权限。

## 验收记录

- v0.1.0 的 [五目标标签构建](https://github.com/BgaSol/soDeck/actions/runs/41873423553) 已通过，安装包已在 [v0.1.0](https://github.com/BgaSol/soDeck/releases/tag/v0.1.0) 正式发布。
- v0.1.1 本机 Windows x86_64：类型检查、格式、前端构建、NSIS 打包通过；9 项 Rust 测试通过，覆盖真实进程重载、初始化超时、崩溃、忽略退出、子进程清理、输出洪水、日志轮转和存储错误。
- 本机实际窗口已检查无独立标题栏、设置中的目录选择与 Hello 插件运行。其余交互和五目标实际安装包验证正在完成，最终结果在本节更新。
- v0.1.0 没有插件/消息数据，因此本次升级不能证明业务数据跨版本迁移；另行验证 v0.1.1 的重启和同版覆盖保留。Linux/macOS 尚无真实设备人工界面和系统升级验收。

## 设计文档

- [产品范围](docs/product.md)
- [架构与边界](docs/architecture.md)
- [安装与分发](docs/distribution.md)
- [外置插件基础功能实施计划](docs/plan-plugin-foundation.md)
- [旧版功能和界面提取](docs/legacy-review.md)

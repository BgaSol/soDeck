# SoDeck 开发指南

## 环境与启动

Node.js 版本以 [.node-version](../.node-version) 为准；Rust 工具链以 [rust-toolchain.toml](../rust-toolchain.toml) 为准。准备 [Tauri 系统构建依赖](https://v2.tauri.app/start/prerequisites/)，使用仓库锁文件安装依赖。

```sh
npm ci
npm run dev
```

开发核心时不需要构建业务插件。使用终端、文件或网络隧道时，先独立打包对应插件，在应用设置中添加产物目录并重启。

## 检查与测试

按改动范围运行相关检查；具体开发约定见 [AGENTS.md](../AGENTS.md)。

| 命令 | 验证内容 |
| --- | --- |
| `npm run check` | TypeScript 类型与核心 Rust 格式 |
| `npm run plugin:check` | 三个官方插件的 Rust 格式 |
| `npm run scripts:test` | 构建、打包、更新、原生验收和 CI 软件源配置回归 |
| `npm run test` | 核心 Rust 测试，包含仅测试使用的宿主夹具 |
| `npm run web:build` | TypeScript 检查与正式前端构建 |
| `npm run interactions:test` | 浏览器真实按压刷新、文件菜单可用状态、子菜单与溢出回归；需要 Playwright Chromium |

界面测试需要 Playwright Chromium：

```sh
npx playwright install chromium
npm run ui:test
npm run terminal:input:test
```

`ui:test` 包含服务器、工作区、连接界面、正式前端产物、桌面一致性和组件设置回归。`terminal:input:test` 验证实际 xterm 输入与剪贴板桥，无需 OpenSSH。

Windows 对运行中的测试程序有文件锁，使用同一 Rust target 目录的核心/插件集成与界面测试应顺序运行。修改网络 UI 后可用 `npm run network:test -- --network-ui-only` 重新独立打包并只复验实际插件界面与隧道，不重复协议压力测试；完整网络验证仍使用 `npm run network:test`。

`npm run ui:production:test -- --updates-only` 验证正式前端的更新说明 Markdown、链接分发、批量更新和三主题小窗口布局，不运行后续工作区菜单与拆窗专项。

真实远程业务测试还需要本机 OpenSSH 服务端与 `ssh-keygen`，并启动隔离的临时回环服务：

| 命令 | 验证内容 |
| --- | --- |
| `npm run ssh:test` | SSH 连接与认证 |
| `npm run terminal:test` | 真实 PTY 与终端业务 |
| `npm run files:test` | SFTP 与文件业务 |
| `npm run network:test` | TCP 转发、实际插件、独立 SOCKS 服务端、curl 互操作及资源限额 |
| `npm run network:netem` | Linux 私有 network namespace 内的真实延迟、丢包及资源清理 |

`network:test` 使用测试依赖 `fast-socks5 1.0.0` 和固定 curl 8.21.0。`scripts/network-tools.mjs` 优先复用匹配版本的 curl；缺少时从 curl 官方下载源码，经 SHA-256 核验后用 CMake 和本机 C 工具链编译测试工具。Windows 源码编译还需要 Visual Studio C++ 构建工具，Linux/macOS 需要 `cmake`、`tar` 和 C 编译器；可用 `SODECK_TEST_CURL` 指向已安装的对应版本。工具缓存于 `artifacts/network-tools/`，不进入核心或插件发行包。

`network:netem` 仅支持原生 Linux x86_64/ARM64，需要 `iproute2`、root 或无交互 `sudo`，以及允许创建 network namespace 的环境。脚本只修改自建 namespace 的回环接口，结束或失败时删除该 namespace；容器需具备相应网络管理能力。默认执行六组配置并保存 `artifacts/network-netem/report.json`，可用 `SODECK_NETEM_PROFILES=baseline,cross-region` 选择配置，用 `SODECK_NETEM_REPORT=artifacts/network-netem/repeat.json` 指定独立报告路径，避免复测覆盖已有报告。报告在成功或失败时都记录完成状态和资源清理结果。当前拓扑对夹具全部 TCP 段施加同一参数，属于多段叠加测试；协议分片用例与真实丢包结果分别记录。

原生包验收步骤和各类测试的边界见 [分发文档](distribution.md)。

`Verify published release upgrade` 手动工作流验证本轮正式核心 0.9.2 → 0.9.3、终端 0.2.1 → 0.2.2、文件 0.4.1 → 0.4.2、网络隧道 0.2.1 → 0.2.2。五个原生 runner 执行 `node scripts/test-release-upgrade.mjs`（`SODECK_UPGRADE_TARGET` 指定本机 Rust target），下载真实正式包并核对 GitHub SHA-256，使用仅测试的 `release-upgrade-fixture` 调用生产插件更新服务与官方 Tauri updater 安装，随后核对安装字节、实际新程序、设置及服务器资料，再验证远程业务。Linux 另执行 DEB 系统安装升级。夹具不发行；此入口验证安装和持久化，完整更新页面点击流程由本机桌面验收另行记录。后续发布新版本后，严格版本预期会拒绝误用该历史版本对。

## 打包

核心与插件各自打包。核心打包只依赖核心和自身运行资源，不需要构建业务插件。

| 本机目标 | 核心打包命令 |
| --- | --- |
| Windows | `npm run build -- --bundles nsis -- --locked` |
| Linux | `npm run build -- --bundles deb,appimage -- --locked` |
| macOS | `npm run build -- --bundles dmg,app -- --locked` |

普通本地打包不需要更新签名密钥。正式更新包的签名与发布配置见 [分发文档](distribution.md)。

每个官方插件独立构建。先在仓库根目录执行 `npm ci`，共享菜单脚本使用根目录锁定的 Vite 和 Floating UI 打成插件自带的普通脚本；不依赖核心前端产物。终端入口还会安装其自身锁定的 xterm 依赖：

```sh
npm run plugin:build -- --plugin terminal
npm run plugin:build -- --plugin files
npm run plugin:build -- --plugin network-proxy
```

产物位于 `artifacts/plugins/sodeck-<插件名>_<版本>_<target>`。在应用设置中添加对应目录并重启；正在运行的插件不能原位覆盖，需要关闭并重启后构建，或指定其他输出目录。宿主测试夹具只用于回归，不发行。

## 版本与持续集成

核心版本只修改 [src-tauri/tauri.conf.json](../src-tauri/tauri.conf.json)，插件版本只修改各自的 `plugins/<名称>/Cargo.toml`。核心与插件使用独立标签、独立 Release；发布步骤、更新清单和签名配置统一维护在 [分发文档](distribution.md)。

插件版本读取由 `cargo metadata --format-version 1 --no-deps --locked` 解析，并按清单路径选取包，支持合法 TOML 写法与 workspace 版本继承。版本相关脚本测试同样需要上述固定 Rust 工具链；缺少 Cargo 或清单无效时明确失败，不从文本猜测版本。

| 工作流 | 用途 |
| --- | --- |
| [Check](../.github/workflows/check.yml) | main/PR 保持现有常规检查；手动选择完整 Windows UI/SSH、原生核心或网络插件测试；同一引用和套件只保留最新运行 |
| [Package SoDeck](../.github/workflows/build.yml) | 五目标原生核心构建、打包、包内验证及更新签名 |
| [Package plugin](../.github/workflows/plugin.yml) | 按插件目录独立打包，生成插件更新清单 |
| [Verify native remote plugins](../.github/workflows/remote-integration.yml) | 按组件版本或明确 run 定位实际包，校验来源后执行五平台原生界面/远程业务验收，支持输入快照重放 |

五个系统与架构组合分别使用原生构建和实际包验证。已完成的验收、未完成项和已知风险记录在对应实施记录或版本说明中。

Check 的 **Run workflow** 先选择需要验证的分支或标签，再选择 `suite`。检查结果只适用于该次运行的确切提交；打包工作流不等待 Check。main/PR 的自动检查保持原有范围，新增的五平台和完整 UI 套件仅手动运行。

| suite | 执行范围 |
| --- | --- |
| `standard`（默认） | 原有 `format` 检查及 Linux x86_64/ARM64 `network`，当前仍包含 netem |
| `desktop` | Windows x86_64 顺序执行源码 `ssh:test` 和完整 `ui:test`，包含 Chromium 与终端 UI 依赖 |
| `native-core` | 五个原生目标各执行 `npm run test`；Windows x86_64、Linux x86_64、macOS ARM64 另执行原宿主脚本回归 |
| `network-plugin` | 五个原生目标仅执行网络插件 `cargo test --manifest-path plugins/network-proxy/Cargo.toml --locked`，不构建核心测试夹具 |
| `all` | 上述所有 job；同一 job 内顺序执行，原生矩阵并行 |

原宿主脚本回归为 `collect-core.test.mjs`、`core-release.test.mjs`、`plugin-settings-assets.test.mjs`、`release-metadata.test.mjs` 和 `smoke.test.mjs`。常规 Check 在 Linux 执行完整 `scripts:test`。公共核心逻辑沿用常规 Linux Rust 测试；本地路径、进程、凭据、窗口及平台适配变化需要补做 `native-core`，网络插件或共享客户端变化需要补做 `network-plugin`，核心 UI/SSH 变化需要补做 `desktop`。当前手动入口以既有整套测试为单位，按文件或模块自动选择留待后续优化。

`native-core` 覆盖当前核心单元测试及 `tests/lifecycle.rs`、`updates.rs`、`connections.rs`、`component_ui.rs`、`settings.rs`、`servers.rs`、`streams.rs`、`window_config.rs`、`workspaces.rs`；Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 都在对应原生 runner 执行。`platform`、`desktop_files`、桌面行为及更新等系统分支随原生测试验证。标记为 ignored 的真实远端用例仍由对应 SSH/终端/文件/网络专项提供环境后显式运行，不能把 `native-core` 通过当作这些专项已通过。

核心与插件打包中的源码测试已经移到 Check；核心 preflight 只使用 Node 检查发行输入，正式前端构建仍执行 TypeScript 检查。实际包的架构、更新签名、启动、持久资料重开和退出验证继续执行，Windows x86_64 另保留包内 SSH 烟测。

CI 的 npm 下载缓存按任务实际使用的锁文件配置；核心打包只使用根锁文件，终端 UI 测试同时使用终端锁文件。Rust 缓存区分日常检查、核心打包和各插件，只有成功的 main 运行保存，其他引用尝试恢复；插件及其原生测试显式覆盖共享 plugin-client 清单。恢复缓存后仍执行当前输入的安装、测试或正式构建；此次未迁移 `core-package` 缓存族，发行缓存优化留在提速方案第二步。

返回 [项目首页](../README.md) · [文档索引](README.md)

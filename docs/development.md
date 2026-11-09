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
| `npm run plugin:check` | 终端、文件、网络隧道和系统状态的 Rust 格式 |
| `npm run scripts:test` | 构建、打包、更新、原生验收和 CI 软件源配置回归 |
| `npm run test` | 核心 Rust 测试，包含仅测试使用的宿主夹具 |
| `npm run web:build` | TypeScript 检查与正式前端构建 |
| `npm run communication:test` | 独立打包终端、文件、系统状态，在隔离 OpenSSH 上验证共享状态、目录跟随、挂载容量和页面恢复；需要 SSH 服务与 Playwright Chromium |
| `npm run interactions:test` | 浏览器真实按压刷新、文件菜单可用状态、子菜单与溢出回归；需要 Playwright Chromium |
| `npm run layout:test` | 真实 UI v2 插件进程和核心的嵌套分屏、独立创建、比例键盘/取消、紧凑恢复、页面返回来源、两工作台原子返回与草稿连续性；浏览器页面替代原生窗口，不能替代系统手势验收 |

界面测试需要 Playwright Chromium：

```sh
npx playwright install chromium
npm run ui:test
npm run terminal:input:test
```

`ui:test` 包含服务器、工作区、连接界面、正式前端产物、桌面一致性和组件设置回归。`terminal:input:test` 验证实际 xterm 输入与剪贴板桥，无需 OpenSSH。

Windows 对运行中的测试程序有文件锁，使用同一 Rust target 目录的核心/插件集成与界面测试应顺序运行。修改网络 UI 后可用 `npm run network:test -- --network-ui-only` 重新独立打包并只复验实际插件界面与隧道，不重复协议压力测试；完整网络验证仍使用 `npm run network:test`。

`npm run ui:production:test -- --updates-only` 验证正式前端的官方插件发现、安装和统一更新：仓库列表延迟加载、失败重试、运行中新插件出现与移除、离线缓存、连续安装、更新说明 Markdown、链接分发、批量更新，以及下载网络设置的草稿、地址排序/停用、保存冲突、单地址诊断和三主题小窗口布局，不运行后续工作区菜单与拆窗专项。列表、安装和诊断回归使用真实核心及隔离回环 HTTP，不依赖公网。

`npm run test` 的 `tests/update_network.rs` 覆盖线路切换、API/文件健康隔离、URL/偏好校验、设置修订冲突、下载中断/损坏/取消和单地址诊断；`tests/updates.rs` 包含代理上的完整官方插件安装链路。核心单元测试用官方公开签名向量验证坏包重试与已准备包的离线恢复。公网节点的历史探测与当前源代码测试分别记录在[实施记录](implementation-update-acceleration.md)，不能互相替代。

需要核对实际 GitHub Trees/Blobs 接口时，显式执行以下烟测；它只读取官方仓库列表，不安装插件，依赖网络及 GitHub API 可用额度，默认核心测试不运行：

```sh
cargo test --manifest-path src-tauri/Cargo.toml --features test-fixtures --test updates --locked official_catalog_live_repository -- --ignored --nocapture
```

需要验证完整公网加速安装链路时，设置 `SODECK_TEST_MIRROR` 为 `auto`（实际默认线路切换）或单一 HTTPS 转发前缀，再运行下面的显式专项。它读取官方名单和核心清单，下载并校验真实终端插件到隔离临时目录，不启动插件；不更新正在使用的应用。需要公网及可用 API 额度，不能放进默认离线测试：

```sh
cargo test --manifest-path src-tauri/Cargo.toml --features test-fixtures --test updates --locked official_install_live_mirror -- --ignored --nocapture
```

同样的环境变量下，可显式读取并下载本机平台的真实核心更新包，以 Tauri 配置中的公钥验签并准备到隔离临时目录；下面的专项不运行安装器：

```sh
cargo test --manifest-path src-tauri/Cargo.toml --features test-fixtures --lib --locked core_download_live_mirror -- --ignored --nocapture
```

真实远程业务测试还需要本机 OpenSSH 服务端与 `ssh-keygen`，并启动隔离的临时回环服务：

| 命令 | 验证内容 |
| --- | --- |
| `npm run ssh:test` | SSH 连接与认证 |
| `npm run terminal:test` | 真实 PTY 与终端业务 |
| `npm run files:test` | SFTP 与文件业务 |
| `npm run network:test` | TCP 转发、实际插件、独立 SOCKS 服务端、curl 互操作及资源限额 |
| `npm run network:netem` | Linux 私有 network namespace 内的真实延迟、丢包及资源清理 |
| `npm run system-status:test` | 独立插件包、真实 OpenSSH 采集/隐藏/暂停/关闭和三主题侧栏 |
| `npm run system-status:ui:test` | 固定数据 UI 与 160/280/480px 布局；`SODECK_TEST_SYSTEM_STATUS_PLUGIN` 可指向实际包 |

`network:test` 使用测试依赖 `fast-socks5 1.0.0` 和固定 curl 8.21.0。`scripts/network-tools.mjs` 优先复用匹配版本的 curl；缺少时从 curl 官方下载源码，经 SHA-256 核验后用 CMake 和本机 C 工具链编译测试工具。Windows 源码编译还需要 Visual Studio C++ 构建工具，Linux/macOS 需要 `cmake`、`tar` 和 C 编译器；可用 `SODECK_TEST_CURL` 指向已安装的对应版本。工具缓存于 `artifacts/network-tools/`，不进入核心或插件发行包。

`network:netem` 仅支持原生 Linux x86_64/ARM64，需要 `iproute2`、root 或无交互 `sudo`，以及允许创建 network namespace 的环境。脚本只修改自建 namespace 的回环接口，结束或失败时删除该 namespace；容器需具备相应网络管理能力。默认执行六组配置并保存 `artifacts/network-netem/report.json`，可用 `SODECK_NETEM_PROFILES=baseline,cross-region` 选择配置，用 `SODECK_NETEM_REPORT=artifacts/network-netem/repeat.json` 指定独立报告路径，避免复测覆盖已有报告。报告在成功或失败时都记录完成状态和资源清理结果。当前拓扑对夹具全部 TCP 段施加同一参数，属于多段叠加测试；协议分片用例与真实丢包结果分别记录。

原生包验收步骤和各类测试的边界见 [分发文档](distribution.md)。

Windows 关闭生命周期交互验收使用 `node scripts/test-openssh.mjs --close-native --packaged <解包后的sodeck.exe>`，并通过 `SODECK_TEST_TERMINAL_PLUGIN`、`SODECK_TEST_FILES_PLUGIN`、`SODECK_TEST_NETWORK_PLUGIN`、`SODECK_TEST_SYSTEM_STATUS_PLUGIN` 指定四个独立包目录。此入口复用隔离 localhost OpenSSH，运行真实包内 WebView2，检查右上角关闭与原生 CloseRequested、取消保留原 view/草稿和网络转发、再次关闭能够继续、明确放弃不写远端，以及四个受管插件退出、真实流结束与端口可重绑。也检查左右分隔线拖动、捕获期间刷新、Esc 取消、宽度落盘及终端 iframe 保留；窄布局使用 inspector 视口模拟，不替代真实窗口缩放和 DPI 切换。结果写入 `artifacts/close-lifecycle/native-close.json`，拖拽截图写入 `artifacts/panel-resize/`。

该入口还验证嵌套分屏、真实终端页面拖出、PTY 输入/输出接续、返回来源组与空窗口回收。`scripts/native-pointer.ps1` 仅按该测试进程定位原生窗口，用系统 DPI 与客户区原点转换坐标，发送真实系统鼠标事件并恢复鼠标位置；运行时需要可交互 Windows 桌面，测试窗口会短暂置前。它不替代副屏混合 DPI、整组/工作台手势和其他系统的验收。每次开始清除旧报告，失败写入 `completed: false`。

该验收使用显式 `--interactive-test --test-data-dir <隔离目录>` 启动，缺少数据目录拒绝启动；普通启动继续使用原系统应用数据目录。WebView2 数据与 inspector 也限定在该测试进程中，避免连接用户正在运行的应用。此模式不自动执行常规 smoke 或在线检查，但加载已保存的启用插件；不作为用户日常入口。常规包烟测仍由 `scripts/smoke.mjs` 提供，两者结果分别记录。

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
npm run plugin:build -- --plugin system-status
```

产物位于 `artifacts/plugins/sodeck-<插件名>_<版本>_<target>`。在应用设置中添加对应目录并重启；正在运行的插件不能原位覆盖，需要关闭并重启后构建，或指定其他输出目录。宿主测试夹具只用于回归，不发行。

## 版本与持续集成

核心版本只修改 [src-tauri/tauri.conf.json](../src-tauri/tauri.conf.json)，插件版本只修改各自的 `plugins/<名称>/Cargo.toml`。核心与插件使用独立标签、独立 Release；发布步骤、更新清单和签名配置统一维护在 [分发文档](distribution.md)。

插件版本读取由 `cargo metadata --format-version 1 --no-deps --locked` 解析，并按清单路径选取包，支持合法 TOML 写法与 workspace 版本继承。版本相关脚本测试同样需要上述固定 Rust 工具链；缺少 Cargo 或清单无效时明确失败，不从文本猜测版本。

| 工作流 | 用途 |
| --- | --- |
| [Check](../.github/workflows/check.yml) | main/PR 按实际改动选择源码和业务回归，汇总执行/跳过状态；手动补测同一引用，netem 单独按需 |
| [Package SoDeck](../.github/workflows/build.yml) | 五目标原生核心构建、打包、包内验证及更新签名；可填写原始 `core_run_id` 跳过构建完成汇总/草稿收尾 |
| [Package plugin](../.github/workflows/plugin.yml) | 按插件目录独立打包，生成插件更新清单 |
| [Verify native remote plugins](../.github/workflows/remote-integration.yml) | 按组件版本或明确 run 定位实际包，校验来源后执行五平台原生界面/远程业务验收，支持输入快照重放 |

五个系统与架构组合分别使用原生构建和实际包验证。已完成的验收、未完成项和已知风险记录在对应实施记录或版本说明中。

`Verify native remote plugins` 的 `platform` 默认验证五平台；补测时可选择单个平台并重放 `verification_run_id`，或填写相同的原始组件打包 run。单平台汇总只表示所选平台通过，发行记录必须同时保留其他平台的原始成功报告。Windows ARM64 的隔离 OpenSSH 测试服务从固定 Microsoft Release 资产 API 下载并校验大小与 SHA-256。

Check 始终响应 main push/PR，不使用 workflow 级 `paths` 跳过。`select` 使用 push 的 before/after，PR 使用 base/head 的 merge-base；删除和重命名同时计入原路径。取不到完整差异、新分支零 before、未知路径、共享客户端、工具链和构建输入按扩大范围验证。选择规则集中在 [select-checks.mjs](../scripts/select-checks.mjs)，不维护依赖图。

文档检查只用 Node，运行 `node scripts/check-docs.mjs` 校验全部已提交 Markdown 的结尾换行、冲突标记和本地文件链接（含图片和引用式链接）；不访问外部网址或校验标题锚点。删除/重命名文档也会检查其他文档留下的链接。`format` 保留为最终汇总状态：选中的任务失败、取消或缺失会失败；未选中的任务显示 skipped，不代表测试通过。分支保护若单独要求旧 `network (ubuntu-22.04...)` 状态，需要改用始终出现的 `format`，本轮不修改仓库保护设置。

| 自动改动范围 | 实际选择 |
| --- | --- |
| Markdown、docs 下的说明/配图 | 仅文档检查；不安装 npm/Rust，不构建产品 |
| 核心前端 | 正式前端构建（含 tsc）、脚本、Linux interactions、Windows 完整 UI（含 settings）；不因此运行 SSH、网络或原生 Rust 矩阵 |
| Tauri 版本/核心配置、capabilities | 核心格式/前端/脚本、五平台原生核心和 Windows UI；不追加插件源码和普通网络/文件/终端业务 |
| 单插件 UI/发行元数据 | 脚本和该插件普通业务验收；终端额外验证输入，网络使用 Linux 两架构，文件/终端使用 Linux x86_64 |
| 单插件 Rust/清单/锁文件/构建输入 | 该插件格式与五平台原生 Rust 测试、该插件普通业务验收；其他插件不跟随 |
| desktop_files 的 windows/linux/macos.rs | 对应系统两架构/一个 macOS 架构的核心测试、文件业务；Windows 另有完整 UI |
| network_metrics.rs、settings/declaration.rs | 一次 Linux x86_64 完整核心套件；声明变化另走前端相关回归 |
| 其他已映射核心系统/更新模块 | 五平台原生核心和相关 UI；网络、文件、终端服务分别追加自己的普通业务验收 |
| 连接、流、生命周期/RPC、平台进程、服务器路由 | 五平台原生核心、Windows UI/SSH、四插件普通业务验收 |
| 共享 UI/字体/菜单 | 扩大至核心与四插件 UI/普通业务验收；不因此运行 netem 或旧版升级 |
| 共享 plugin-client、根工具链/锁文件/构建脚本、未知输入 | 五平台核心与四插件源码、相关 UI、普通业务和宿主脚本；仍排除 netem |

已知 UI 测试入口也按组件路由：`test-terminal-ui`/`test-files-ui` 仅运行对应普通业务，`test-terminal-input` 仅输入/脚本，核心 UI 测试脚本走前端和完整 desktop。共享 OpenSSH/打包脚本和未映射脚本继续扩大范围。

Check 的 **Run workflow** 先选择需要验证的分支或标签，再选择 `suite`，手动选择覆盖自动判定。检查结果只适用于该次运行的确切提交；打包工作流不等待 Check。

| suite | 执行范围 |
| --- | --- |
| `standard`（默认） | 公共 Linux 核心/四插件格式和源码、前端/脚本/输入/设置回归及 Linux x86_64/ARM64 普通网络；排除 netem、完整 Windows UI |
| `desktop` | Windows x86_64 顺序执行源码 `ssh:test` 和完整 `ui:test`，包含 Chromium 与终端 UI 依赖 |
| `native-core` | 五个原生目标各执行 `npm run test`；Windows x86_64、Linux x86_64、macOS ARM64 另执行原宿主脚本回归 |
| `network-plugin` | 网络插件格式、五个原生目标的 `cargo test --manifest-path plugins/network-proxy/Cargo.toml --locked`，不构建核心测试夹具 |
| `all` | 核心与四插件五平台源码、完整 Windows UI/SSH、普通网络/文件/终端和脚本回归；同一 job 顺序执行，矩阵并行；排除 netem |
| `netem` | 单独运行 Linux x86_64/ARM64 私有 namespace 弱网矩阵；成功/失败报告 Artifact 保留 14 天，不安装 Chromium 或 OpenSSH |

原宿主脚本回归为 `collect-core.test.mjs`、`core-release.test.mjs`、`plugin-settings-assets.test.mjs`、`release-metadata.test.mjs` 和 `smoke.test.mjs`；在 `native-core`/`all` 和共享未知输入下于 Windows x86_64、Linux x86_64、macOS ARM64 执行。自动选择仍以既有套件为单位，不能独立切分时运行较大范围；需要额外验证时可对同一 ref 补做手动套件。真实包组合与升级保持原入口，不把工作流选择当作已验收。

完整 Windows `ui:test` 已包含 settings 回归，选中 desktop 时不再在 Linux 重复。手动 `standard` 没有 desktop，Linux 单独执行 `settings:test`，为其真实 catalog 核心夹具安装 WebKit/GTK 构建库并恢复 `check-tests` 依赖缓存。

`native-core` 覆盖当前核心单元测试及 `tests/lifecycle.rs`、`close_lifecycle.rs`、`updates.rs`、`update_network.rs`、`connections.rs`、`component_ui.rs`、`settings.rs`、`servers.rs`、`streams.rs`、`window_config.rs`、`workspaces.rs`；Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 都在对应原生 runner 执行。`platform`、`desktop_files`、桌面行为及更新等系统分支随原生测试验证。标记为 ignored 的真实远端用例仍由对应 SSH/终端/文件/网络专项提供环境后显式运行，不能把 `native-core` 通过当作这些专项已通过。

核心与插件打包中的源码测试已经移到 Check；核心 preflight 只使用 Node 检查发行输入，正式前端构建仍执行 TypeScript 检查。实际包的架构、更新签名、启动、持久资料重开和退出验证继续执行，Windows x86_64 另保留包内 SSH 烟测。

CI 的 npm 下载缓存按任务实际使用的锁文件配置；核心打包只使用根锁文件，终端 UI 测试同时使用终端锁文件。Rust 缓存区分日常 `check-tests`、正式核心 `core-release-v2` 和各插件，只有成功的 main 运行保存，其他引用尝试恢复；插件及其原生测试显式覆盖共享 plugin-client 清单。升级验收夹具使用 `check-tests`，不恢复发行缓存。

包含核心的原生测试统一声明现有核心/四插件 workspace 输入，复用同一稳定 `check-tests` 键，避免每个组件/平台范围生成大份重复核心依赖。声明缓存目录不启动对应插件测试；实际执行仍由所选范围控制。仅插件的原生 job 保留小范围 workspace，不恢复核心 target。发行缓存仍只有核心自身输入。

`core-release-v2` 是一次性的缓存迁移标识，不随提交、版本或 run ID 变化。它保留固定 action 的默认工具链/环境/Cargo 输入隔离，只缓存依赖编译结果；宿主构建依赖与目标架构 release 依赖一并保留，不开启 workspace crate 或 incremental 缓存。恢复后仍执行 `npm ci`、当前前端及当前核心的正式构建。main 手动打包会保存依赖，后续适用分支和标签可恢复默认分支缓存；详见[分发文档](distribution.md)。Linux 打包依赖安装复用官方软件源修正与有限重试，不缓存系统安装状态。

返回 [项目首页](../README.md) · [文档索引](README.md)

### 自动目录集成回归

Linux 安装 Bash、Zsh 和 Python 3 后运行 `python3 scripts/test-terminal-shell.py`，以真实 PTY 验证随终端发行的启动脚本、用户配置顺序、字符串/数组提示符钩子、退出状态、特殊字符、嵌套 shell、Ctrl-C 与临时文件清理。Check 的终端业务任务执行此入口；Python/Zsh 是测试工具，不加入产品包。`npm run communication:test` 在 Linux 使用默认自动集成，不配置 hostname 或注入提示符钩子；Windows 保留 PowerShell 手动 OSC 7 兼容回归。

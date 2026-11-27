# SoDeck 开发指南

DOM 回归在现有服务器、设置、交互及正式产物测试中运行。正式产物用真实 main/settings 入口与 CDP composition 通道检查连续组合更新、一次提交及子树从未断开，并注入快照/网络/设置读取失败；这些结果不能替代各系统原生输入法和 DPI 设备验收。共享 DOM 资源变更同时选择网络插件独立构建和集成检查，包内验收检查脚本与 MIT 许可证；核心产物携带 licenses.txt。

## 环境与启动

Node.js 版本以 [.node-version](../.node-version) 为准；Rust 工具链以 [rust-toolchain.toml](../rust-toolchain.toml) 为准。准备 [Tauri 系统构建依赖](https://v2.tauri.app/start/prerequisites/)，使用仓库锁文件安装依赖。

```sh
npm ci
npm run dev
```

开发核心时不需要构建业务插件。使用终端、文件、网络隧道、系统状态或 Docker 时，先独立打包对应插件，在应用设置中添加产物目录并重启。

## 检查与测试

命令回归在核心 component_ui 的真实进程测试覆盖去重、来源隔离、单处理者路由、关闭撤销、在途额度、移窗和默认创建竞争。terminal:test 验证实际终端进程与 PTY/UI；docker:test 使用隔离 Docker Engine、OpenSSH 和两个实际插件验证容器终端及来源关闭后的独立运行。test:prepare 选择 Docker 时同时准备终端测试目录；核心独立构建仍无需插件。

按改动范围运行相关检查；具体开发约定见 [AGENTS.md](../AGENTS.md)。

| 命令 | 验证内容 |
| --- | --- |
| `npm run check` | TypeScript 类型与核心 Rust 格式 |
| `npm run plugin:check` | 终端、文件、网络隧道、系统状态和 Docker 的 Rust 格式 |
| `npm run scripts:test` | 构建、打包、更新、原生验收和 CI 软件源配置回归 |
| `npm run test` | 核心 Rust 测试，包含仅测试使用的宿主夹具 |
| `npm run web:build` | 只构建正式前端资源；类型检查使用 `npm run check` |
| `npm run files:editor:test` | 独立编辑器构建、实际产物的搜索、语法颜色、缩略图、字节上限、剪贴板与恢复回归；需要 Chromium，不需要 OpenSSH |
| `npm run communication:test` | 使用已准备的终端、文件、系统状态测试目录，在隔离 OpenSSH 上验证共享状态、目录跟随、挂载容量和页面恢复；需要 SSH 服务与 Playwright Chromium |
| `npm run interactions:test` | 浏览器真实按压刷新、文件菜单可用状态、子菜单与溢出回归；需要 Playwright Chromium |
| `npm run layout:test` | 真实 UI v2 插件进程和核心的嵌套分屏、独立创建、比例键盘/取消、紧凑恢复、页面返回来源、两工作台原子返回与草稿连续性；浏览器页面替代原生窗口，不能替代系统手势验收 |

OpenSSH、终端输入和正式 UI 测试使用 Node `util.parseArgs`；未知开关、非预期位置参数和缺失值在构建或资源启动前失败。OpenSSH 的第一个 `--packaged` 是启动器边界，前面的开关属于测试脚本，后面的程序、`--`、`-a` 等参数逐项原样透传，不能把测试开关放在后面。终端输入的 `--desktop-script <路径>` 保留第一个重复值，正式 UI 的 `--updates-only` 仍只选择更新专项。

状态与 Docker UI 夹具在短命浏览器页面中加载共享 UI 与主题 CSS，通过宿主 `themeTokens()` 读取三主题的真实级联快照；页面用后关闭，业务 UI 仍使用独立插件目录的静态资源。主题读取需要锁定的 Node 版本和 Playwright Chromium，普通工程脚本回归不增加浏览器前提。

界面测试需要 Playwright Chromium：

```sh
npm run test:prepare -- --ui-only terminal files
npx playwright install chromium
npm run ui:test
npm run terminal:input:test
npm run terminal:history:test
```

`ui:test` 包含服务器、工作区、连接界面、正式前端产物、桌面一致性和组件设置回归。`terminal:input:test` 验证实际 xterm 输入与剪贴板桥，无需 OpenSSH。

日志与消息的聚焦回归 `scripts/test-logs-messages-ui.mjs` 已接入 `ui:test`，加载实际页面控制器，在受控核心边界验证单在途、迟到回包、暂停、IME composition、筛选、已读队列、去重、淘汰、来源回退及三主题内容区。正式产物回归另用真实核心进程验证原入口。核心实际包烟测包含日志/字段/暂停、消息阅读与已读、三主题及返回筛选，并要求独立 `logsMessagesNative` 回执；它不证明真实 IME、读屏或鼠标输入。

标签溢出回归在 `interactions:test` 验证可打断缓动、像素输入接管、边缘停留、减少动态与搜索输入保持；`ui:test` 使用真实核心覆盖 20 个服务器、单组 50 页、各分屏位置、关闭取消和 iframe 保留。设备专项仍需在原生包中验证鼠标/触控板、静止边缘拖动、跨窗松手后的最终落点与混合 DPI；浏览器合成 WheelEvent 不证明真实设备惯性。

拖拽生命周期回归也在 `interactions:test`：真实鼠标在 iframe 边界松手、无按钮移动、延迟启动/采样、准备失败、结束先到及乱序预览；核心 Rust 单测验证释放、幂等取消、事件顺序与目标身份。`test-workspace-ui.mjs` 使用实际组件来源，`layout:test` 保留工具停靠与草稿连续性。Windows 的 `test-close-native.mjs` 与同架构 `native-pointer-fixture` 增加系统指针边缘松手后悬停、固定落点和重复清理；运行方式仍为 `node scripts/test-openssh.mjs --close-native --packaged <本次核心程序>`，设置四个独立插件目录及 `SODECK_TEST_NATIVE_POINTER`。该专项使用真实 WebView2 和系统注入指针，不代表其他平台或物理输入设备已验证。

CI 使用 `npm run ci:test -- <现有测试命令>`，例如 `npm run ci:test -- ui:test`。该入口保留原命令的退出码，输出实时日志，并将 SHA、attempt、平台、耗时和结果写入 `artifacts/ci/<命令>/`；命令中的冒号转换为连字符。服务器、工作台和正式产物 UI 在该入口下记录 Playwright trace，失败时保存，成功时丢弃；本机直接运行原入口不记录 trace。

`terminal:history:test` 验证本地打包的真实 xterm/Worker：10 万行检索、计数/定位、移窗元数据、预览手势、输入法组合事件、清屏/备用屏幕/重排和冻结快照。最后运行 100 个不同关键词的计算与内存基准；数字报告写入 `artifacts/terminal-history/`，界面截图写入 `artifacts/terminal-search/`。无需 OpenSSH，不能代替原生 WebView、系统 IME 或五平台实际包验收。xterm / SerializeAddon 更新前必须复验本入口，构建会检查固定适配版本。

Windows 对运行中的测试程序有文件锁，使用同一 Rust target 目录的核心/插件集成与界面测试应顺序运行。修改网络 UI 后可用 `npm run network:test -- --network-ui-only` 使用已准备的测试目录并只复验实际插件界面与隧道，不重复协议压力测试；完整网络验证仍使用 `npm run network:test`。

文件 UI 可用 `node scripts/test-openssh.mjs --files --files-ui-only` 使用已准备的测试目录复验真实 SFTP 界面。文件编辑 UI SDK 使用 packages/file-editor 的独立 npm 锁文件，声明 build.fileEditor 的插件打包时自动构建并携带它；类型检查运行 `npm run check --prefix packages/file-editor`，单独构建运行 `npm run build --prefix packages/file-editor`（首次先 `npm ci --prefix packages/file-editor`）。核心构建不依赖 SDK UI。Windows 原生编辑器专项：先独立打包文件插件，将 `SODECK_TEST_FILES_PLUGIN` 指向完整包目录，再运行 `node scripts/test-openssh.mjs --file-editor-native --packaged <本次核心可执行文件>`。该专项使用隔离应用数据和 SSH，验证 WebView2/CSP/worker、编辑功能、保存与退出，不操作日常桌面剪贴板。传入构建目录程序只能证明本机原生运行，发行前仍需实际安装包与五平台验收。

`Verify native remote plugins` 的 `file_editor` 是 Windows 深度界面专项的显式入口，默认关闭，需要运行环境允许 WebView2 调试端口。环境未应用调试参数时会在连接渲染器之前失败，此时不能声称编辑器专项通过；五平台常规实际包验收仍独立执行。具体执行结果写入文件插件 CHANGELOG。显式选中后失败仍使工作流失败，不计为通过。


`npm run ui:production:test -- --updates-only` 验证正式前端的官方插件发现、安装和统一更新：仓库列表延迟加载、失败重试、运行中新插件出现与移除、离线缓存、连续安装、更新说明 Markdown、链接分发、批量更新，以及下载网络设置的草稿、地址排序/停用、保存冲突、单地址诊断和三主题小窗口布局，不运行后续工作区菜单与拆窗专项。列表、安装和诊断回归使用真实核心及隔离回环 HTTP，不依赖公网。

更新夹具复用应用级服务，自动专项在隔离回环来源下启动后台检查，覆盖前端/目录初读失败、未访问更新页也发现版本及顶部入口；常规夹具不启动后台网络。核心服务测试覆盖关闭取消后继续、禁用/手动检查、初始化错误重试，更新测试覆盖显式批次完成和单项不推迟全量。实际核心包的 `--package-only` 验收确认服务已随原生 setup 初始化，自动联网在隔离烟测中关闭；这项包验证不等同五平台真实公网更新检查。

正式前端更新专项还使用独立的空数据进程与回环目录/真实本机架构测试包，自动执行整轮安装，覆盖首项成功后继续、后项挂起时重载/切页和重启阻止、部分失败、三主题/两宽度及实际重开后的插件加载、不自动补齐。常规夹具保持显式安装，空启动导航按真实启动字段验证；无插件默认准备不由页面命令触发。原生包烟测不自动安装公网业务插件，分别验证空启动默认官方页和已有登记重开默认连接页；五平台原生源码集成验证实际安装/重开，不能把包启动导航验收描述成五平台真实公网整轮安装。

`npm run test` 的 `tests/update_network.rs` 覆盖线路切换、API/文件健康隔离、URL/偏好校验、设置修订冲突、下载中断/损坏/取消和单地址诊断；`tests/updates.rs` 包含代理上的完整官方插件安装链路。核心单元测试用官方公开签名向量验证坏包重试与已准备包的离线恢复。公网可用性与当前源代码测试分别验证，不能互相替代；具体发行结果进入核心版本记录。

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
| `npm run system-status:test` | 独立测试目录、真实 OpenSSH 采集/隐藏/暂停/关闭和三主题侧栏 |
| `npm run system-status:ui:test` | 固定数据 UI、概览用量条、CPU/内存趋势切换与重挂载、三主题 160/200/240/280/320/400/480px；`SODECK_TEST_SYSTEM_STATUS_PLUGIN` 可指向实际包 |
| `npm run docker:test` | 本机原生插件、真实核心与隔离 Linux Docker Engine/OpenSSH；验证单容器操作、日志、资源、断线保留及删容器保留卷 |
| `npm run docker:windows:test` | Windows 本机 OpenSSH 与 Docker Desktop Linux Engine，验证原生 CLI/npipe、完整容器/资源/日志/PTY/拉取流程；使用独立标记测试资源并清理 |
| `npm run docker:process:test` | 实际插件进程与假核心；验证树内对象／集合、查询复用、搜索、操作去重、丢失回执、失败/中断状态、关闭与资源释放 |
| `npm run docker:ui:test` | 单层资源树、树内连接／维护／拉取／任务、端口协作；日志持续接收、固定阅读、搜索／筛选／详情、缩略拖动取消、候选原捕获恢复；三主题与 280/320/400/900px 布局 |

源码业务测试先运行 `npm run test:prepare -- <插件名称...>`，例如 `npm run test:prepare -- terminal files system-status docker network-proxy`，一次准备 `artifacts/test-fixtures/<插件>`，再执行所需测试。准备入口编译 dev 程序与 UI，不生成发行归档；测试入口只消费目录，缺少时明确失败。可用对应 `SODECK_TEST_*_PLUGIN` 指定显式目录。源码修改后重新准备。Docker 的进程和真实业务测试均使用该目录，不隐式打包。真实联调需要本机 Docker CLI、支持 privileged 容器的 Linux daemon、`ssh-keygen` 和可下载 Docker/Alpine 镜像的软件源。脚本创建独立 Docker-in-Docker daemon，只共享其测试 Unix socket，不使用宿主 Docker socket；测试结束清理自身容器和测试卷。可用 `SODECK_TEST_ALPINE_MIRROR` 指定测试镜像构建所用的 HTTPS Alpine 仓库根地址。此变量只用于测试，不影响产品。

macOS 状态真机专项使用 `node plugins/system-status/tests/remote.mjs`，显式提供 `SODECK_STATUS_REMOTE_HOST`、`SODECK_STATUS_REMOTE_USER`、`SODECK_STATUS_REMOTE_KEY` 和 `SODECK_TEST_SYSTEM_STATUS_PLUGIN`；先按既有方式构建 `catalog-fixture`。该入口使用实际核心/插件经 SSH 只读采集，不修改目标系统配置；默认连续五分钟，并验证隐藏、暂停、单轮刷新、手动接口选择与重连。`SODECK_STATUS_REMOTE_SECONDS` 可选 30–1800 秒，报告默认在 `artifacts/system-status-remote/report.json`，可用 `SODECK_STATUS_REMOTE_REPORT` 指定独立路径。目标账号需已有可用测试认证；短跑不能替代五分钟验收，报告和原生包验证分别记录。

Windows 日志专项使用 `node scripts/test-docker.mjs --native <独立核心.exe>`，并以 `SODECK_TEST_DOCKER_PLUGIN` 指定实际解包插件目录。默认持续接收 60 秒；`SODECK_LOG_DURATION_MS` 可设置 1 秒至 90 分钟。驱动验证真实 WebView2/CSP、只读正文查询、隔离 SSH/Engine、持续接收中的固定阅读与新增计数、字面查找及零结果、回到最新和退出，并默认执行 20 次真实日志打开/固定/关闭，检查 View 会话数据有界及进程退出；`SODECK_LOG_REOPEN_CYCLES` 可设置 20 至 100 次。报告、查询耗时、长任务、逐进程内存与截图在 `artifacts/log-validation/native-<时间>/`。历史搜索/上下文、导出及重挂载由实际插件进程和浏览器回归分别验证；本入口不证明完整设备交互、系统保存框、其他系统 WebView 或长期内存稳定。

`network:test` 使用测试依赖 `fast-socks5 1.0.0` 和固定 curl 8.21.0。`scripts/network-tools.mjs` 优先复用匹配版本的 curl；缺少时从 curl 官方下载源码，经 SHA-256 核验后用 CMake 和本机 C 工具链编译测试工具。Windows 源码编译还需要 Visual Studio C++ 构建工具，Linux/macOS 需要 `cmake`、`tar` 和 C 编译器；可用 `SODECK_TEST_CURL` 指向已安装的对应版本。工具缓存于 `artifacts/network-tools/`，不进入核心或插件发行包。

`network:netem` 仅支持原生 Linux x86_64/ARM64，需要 `iproute2`、root 或无交互 `sudo`，以及允许创建 network namespace 的环境。脚本只修改自建 namespace 的回环接口，结束或失败时删除该 namespace；容器需具备相应网络管理能力。默认执行六组配置并保存 `artifacts/network-netem/report.json`，可用 `SODECK_NETEM_PROFILES=baseline,cross-region` 选择配置，用 `SODECK_NETEM_REPORT=artifacts/network-netem/repeat.json` 指定独立报告路径，避免复测覆盖已有报告。报告在成功或失败时都记录完成状态和资源清理结果。当前拓扑对夹具全部 TCP 段施加同一参数，属于多段叠加测试；协议分片用例与真实丢包结果分别记录。

原生包验收步骤和各类测试的边界见 [分发文档](distribution.md)。

Windows 关闭生命周期交互验收使用 `node scripts/test-openssh.mjs --close-native --packaged <解包后的sodeck.exe>`，并通过 `SODECK_TEST_TERMINAL_PLUGIN`、`SODECK_TEST_FILES_PLUGIN`、`SODECK_TEST_NETWORK_PLUGIN`、`SODECK_TEST_SYSTEM_STATUS_PLUGIN` 指定四个独立包目录。此入口复用隔离 localhost OpenSSH，运行真实包内 WebView2，检查右上角关闭与原生 CloseRequested、取消保留原 view/草稿和网络转发、再次关闭能够继续、明确放弃不写远端，以及四个受管插件退出、真实流结束与端口可重绑。也检查左右分隔线拖动、捕获期间刷新、Esc 取消、宽度落盘及终端 iframe 保留；窄布局使用 inspector 视口模拟，不替代真实窗口缩放和 DPI 切换。结果写入 `artifacts/close-lifecycle/native-close.json`，拖拽截图写入 `artifacts/panel-resize/`。

可显式设置 `SODECK_TEST_TRAY_ONLY=1` 只验收后台保留与退出，跳过面板/拆窗/停靠系统指针专项，报告保留该范围。2.4.0 起该入口在隔离数据中预置已读首次后台说明，增加真实主窗口收起、隧道字节、编辑草稿与 view 身份保持、恢复后再退出；退出场景显式选择 `exit`。`ui:test` 的 `test-desktop-shell-ui.mjs` 覆盖菜单导航恢复、关闭决定优先、旧确认与收起预检。核心实际包烟测增加 `desktopShellNative`，执行真实托盘构造、菜单分派、窗口收起/恢复和未读投影；这不证明系统图标可见或真实鼠标点击，Linux Xvfb 没有窗口管理器时只证明 minimize 原生调用完成，不能记作桌面最小化交互通过。

该入口还验证嵌套分屏、真实终端页面拖出、PTY 输入/输出接续、返回来源组与空窗口回收，以及工具按钮跨侧保留文件/网络 iframe、草稿与运行中的转发。先用 `cargo build --manifest-path src-tauri/Cargo.toml --features test-fixtures --bin native-pointer-fixture --locked` 构建本机架构夹具，并将其完整路径写入 `SODECK_TEST_NATIVE_POINTER`。Rust 夹具通过 windows-sys 绑定按该测试进程定位原生窗口，用系统 DPI 与客户区原点转换坐标，发送真实系统鼠标事件，并在成功/失败时恢复鼠标和 DPI 上下文；检查器在系统手势期间按该客户区及 DPI 校准，不能使用窄窗口模拟的坐标。运行时需要可交互 Windows 桌面，测试窗口会短暂置前。它不替代副屏混合 DPI、整组/工作台手势和其他系统的验收。每次开始清除旧报告，失败写入 `completed: false`。

实际文件剪贴板验收仅在隔离桌面显式执行：用相同构建入口生成 `desktop-files-fixture`，运行 `node scripts/test-desktop-files-native.mjs <夹具完整路径> <实际包内sodeck程序>`。Linux 在同一 `dbus-run-session -- xvfb-run -a` 会话内运行该 Node 脚本，使提供者与核心共享 CLIPBOARD。脚本使用独立进程提供真实文件格式，验证中文/空格/特殊字符文件与目录、文件→文本的 revision 变化和退出；Linux 另验证 GNOME copied-files 回退。结果写入 `artifacts/native-desktop-files/<target>.json`，不与文本 clipboardNative 混用。测试会替换所在桌面的剪贴板，不在日常用户桌面自动运行。`Verify native remote plugins` 的 `desktop_files` 可选项启用这层实际包验收，要求核心 0.20.0 或更新版本，测试夹具不发行。

该验收使用显式 `--interactive-test --test-data-dir <隔离目录>` 启动，缺少数据目录拒绝启动；普通启动继续使用原系统应用数据目录。WebView2 数据与 inspector 也限定在该测试进程中，避免连接用户正在运行的应用。此模式不自动执行常规 smoke 或在线检查，但加载已保存的启用插件；不作为用户日常入口。常规包烟测仍由 `scripts/smoke.mjs` 提供，两者结果分别记录。

`Verify published release upgrade` 每次显式选择 `component`、`from_version` 和 `to_version`；插件还必须填写固定的 `core_version`。`platform` 默认 all，修复验收工具后可用相同版本对补跑单个平台；单平台结果不替代其余平台证据。五个原生 runner 下载精确正式 Release，固定 manifest/附件 ID、摘要与版本，不读取其他组件的最新版本。新版本必须大于旧版本。测试专用 `release-upgrade-fixture` 调用生产插件更新服务和官方 Tauri updater，核对包摘要、实际安装字节、重启激活、设置及服务器资料；Linux 核心升级另验 DEB 系统安装。夹具不发行，固定输入能力只编入 test-fixtures；远程业务和完整更新页面点击另行验证。正常发布无需在仓库提交运行 ID 或修改历史版本常量。

本次最新版发行可开启测试输入 `verify_discovery`：安装前调用未固定候选的生产 `discover()`，核对所选组件的最新版、完整清单、更新说明及本机包来源，结果写入同一验收报告。它不改变应用更新配置；历史版本对重放默认关闭，避免后来发布的最新版改变固定包安装验收。固定候选的安装通过不能单独作为版本发现通过的证据。核心夹具向官方 updater 传入完整清单，并断言说明保留。

原生包准备与正式升级测试共用流式 size/SHA-256 writer，先写同目录 `.part`，完整校验并关闭后才提交最终文件；截断、超量、错误摘要、I/O 错误或取消均清理本次临时文件并保留已有最终文件，清理失败明确报告。同一目标串行下载。升级测试的公开附件请求不发送认证头，仍可不提供 `GH_TOKEN`；每次请求的 120 秒期限覆盖响应头和正文。原生 GitHub API 客户端继续要求 token，来源固定与请求重试规则保持原样。纯下载和匿名请求回归不联网，也不代替实际版本对升级验收。

系统默认应用验收使用 `node scripts/test-desktop-open-native.mjs <实际包内程序>`，仅在一次性 Actions runner 或显式 `SODECK_TEST_OPEN_ISOLATED=1` 的隔离桌面运行。Windows 编译无控制台 Rust handler，解析测试账户真实 UserChoice ProgID 并临时配置 HTTP/HTTPS 与目录关联，退出时恢复；目录收据按文件系统身份接受 8.3 别名，URL 仍逐字节校验。Linux 使用 xdg-utils、desktop-file-utils 和隔离 XDG 目录，在同一 dbus/Xvfb 会话设置默认 handler。脚本核对唯一 URL 与中文/空格/特殊字符目录各一次、非法目标拒绝和程序退出；Windows/Linux 另设启动器失败用例，Shell 处理坏关联可能超时，超时必须记为失败。`Verify native remote plugins` 显式开启 `desktop_open` 后在所选原生目标调用此入口，Linux 分别运行 DEB 内程序和 AppImage；夹具不发行，结果随 Native-verification-result 上传。

macOS 用系统 clang、AppKit/LaunchServices、Apple Events 和 ad hoc 签名构建临时 app 并恢复关联。原生 Check 先运行 `node scripts/test-desktop-open-native.mjs --handler-only`。如果系统保留受保护的默认关联，此模式仅编译 handler 并标记 skipped；实际包模式仍验证核心交付、非法目标拒绝和退出，但报告 `targetsExactlyOnce:false` 与 `receiptSkipReason`，不能记作目标收据通过。关联可控时才核对 HTTP URL 和目录各到达一次。`/usr/bin/open` 成功不能确认目标应用后续状态；此专项不替代真实浏览器/Finder/Explorer 的可见窗口、UNC 或 Wayland 桌面验收。

### 自动目录集成回归

Linux 安装 Bash、Zsh 和 Python 3 后运行 `python3 scripts/test-terminal-shell.py`，以真实 PTY 验证随终端发行的启动脚本、用户配置顺序、字符串/数组提示符钩子、退出状态、特殊字符、嵌套 shell、Ctrl-C 与临时文件清理。Check 的终端业务任务执行此入口；Python/Zsh 是测试工具，不加入产品包。`npm run communication:test` 在 Linux 使用默认自动集成，不配置 hostname 或注入提示符钩子；Windows 保留 PowerShell 手动 OSC 7 兼容回归。

Windows 命令阅读 hook 使用 `node scripts/test-terminal-powershell.mjs`，实际运行已安装的 Windows PowerShell 5.1/PowerShell 7；独立终端包构建包含此回归。`node scripts/test-terminal-powershell-pty.mjs` 另使用本机 OpenSSH sshd、ssh-keygen 和两个 PowerShell，通过临时回环服务与密钥验证真实 PSReadLine 回车、执行结果和绝对程序路径；不修改系统 SSH 服务或服务器 profile，不等同于系统 IME/ARM64 远端验收。

docker:windows:test 先运行 test:prepare -- docker；需要本机 Docker Desktop 已启动，docker.exe/ssh-keygen/Cargo 在 PATH，Windows OpenSSH sshd 可用。测试只启动临时回环服务与独立密钥，不修改系统 SSH 配置；创建唯一名称/标签的容器、卷、网络与派生镜像，结束仅清理这些资源。它不等于 Windows Server Engine 或 ARM64 远端验收。Docker 的 test:prepare 同时准备终端、文件、系统状态和网络隧道作为组合测试输入；这不改变核心／插件独立打包边界。隔离 Linux Engine 和 Windows Desktop 夹具还从 Docker 发布端口创建网络隧道，经真实核心命令及共享状态验证 HTTP 字节、复用、来源关闭与端口释放。docker:process:test 与实际插件包回归另覆盖模拟 POSIX/Linux、Windows/Linux、Windows/Windows 三种组合，模拟不计为真实 Engine 验收。

### MCP 调查与交互回归

0.11.0 起，进程/原包夹具同时验证十五工具及节点写入转发、目录冲突、准确 annotations 和 HTTP 中断后在途 RPC 位置保留。真实核心三服务器打开流程还通过 MCP 新建节点、部分修改后建连、在用删除拒绝、关闭工作区后删除及磁盘目录核对；不以模拟宿主代替实际提交。核心 servers 回归验证 patch、统一 SSH keep、失败落盘与同修订竞争。

`npm run mcp:test` 在三个隔离 OpenSSH 服务上从零工作区发现保存资料、打开并直接执行/下载，验证主机密钥交接、固定打开回执、重连及插件停止保留接纳会话；另保留真实核心与 MCP 插件的名称/主机/端点/IPv4 尾段目标发现、断开/重连与旧任务恢复、首次采集、补采、任务查找、文本/状态等待、去重和原字节下载；`mcp:process:test` 用实际插件进程验证保存资料完整分页、歧义、共享打开、创建 ACK、换代销毁、延迟接纳前取消，以及原任务接纳回包丢失、归属、稳定收尾、输出模式、分页、等待唤醒/HTTP 中断及容量、最终元数据和清理失败。`mcp:ui:test` 验证摘要与完整详情、主题、窄屏、焦点、接入配置和隐藏/卸载。先执行 `npm run test:prepare -- mcp`。本机可选设 `SODECK_TEST_CODEX` 为已登录的 Codex CLI 路径，在同一真实三服务器夹具中增加实际 Codex 工具调用验收，包括唯一目标两次调用、歧义不执行和重连后查询原任务；此项依赖本机认证，不在无认证 CI 中自动运行。CI 业务矩阵包含前三项；独立插件 package/verify 在五个原生目标验收实际包，并对当前 MCP 原包运行同一打开、进程查询与断开测试。

`mcp:test` 还通过真实 pipe 验证多轮问答、9000 字节跨块输入和 EOF，通过 PTY 验证输入与 resize。POSIX 远端另验证无回显原始控制字节、窗口尺寸、持续 shell、Ctrl+C 与 Ctrl+D；Linux 创建一次性账号和 sudo 规则，验证错误密码后继续原任务、实际 root 文件副作用及退出码。夹具使用独立回环 SSH 服务，结束时有界停止服务并删除临时账号和规则，不在用户服务器执行此专项。`mcp:process:test` 覆盖输入去重、回包丢失、迟到观察、接纳前拒绝与 EOF；五平台插件原包验收复用该进程入口，受控宿主不能替代真实 SSH。

本机可选设 `SODECK_TEST_CODEX_OPEN` 为已登录 Codex CLI 路径，增加零工作区的实际客户端资料发现、原打开等待与直接执行验收；夹具用户只回答自己的隔离主机密钥提示，不修改客户端永久配置。原 `SODECK_TEST_CODEX` 继续用于已有任务读取/恢复验收。两项都依赖本机认证，CI 不自动执行。

## 打包

核心与插件各自打包。核心打包只依赖核心和自身运行资源，不需要构建业务插件。

| 本机目标 | 核心打包命令 |
| --- | --- |
| Windows | `npm run build -- --bundles nsis -- --locked` |
| Linux | `npm run build -- --bundles deb,appimage -- --locked` |
| macOS | `npm run build -- --bundles dmg,app -- --locked` |

普通本地打包不需要更新签名密钥。正式更新包的签名与发布配置见 [分发文档](distribution.md)。

每个官方插件独立构建。先在仓库根目录执行 `npm ci`，共享菜单脚本使用根目录锁定的 Vite 和 Floating UI 打成插件自带的普通脚本；不依赖核心前端产物。终端与文件入口还会安装各自锁定的 xterm/Monaco 依赖：

```sh
npm run plugin:build -- --plugin terminal
npm run plugin:build -- --plugin files
npm run plugin:build -- --plugin network-proxy
npm run plugin:build -- --plugin system-status
npm run plugin:build -- --plugin docker
npm run plugin:build -- --plugin mcp
```

产物位于 `artifacts/plugins/sodeck-<插件名>_<版本>_<target>`。在应用设置中添加对应目录并重启；正在运行的插件不能原位覆盖，需要关闭并重启后构建，或指定其他输出目录。宿主测试夹具只用于回归，不发行。

## 版本与持续集成

macOS 登录项包专项为 `scripts/test-macos-login-package.mjs`，仅在干净原生 CI 用户的 Applications 安装中执行；实际包入口自动调用。它核验主应用登记/取消、2.5.0 文件迁移、幂等和进程存续，采集系统登记及可用的系统设置截图，并通过 `NSWorkspace` 传递登录 Apple Event。普通包烟测只读 Service Management 与校验隔离 plist，不修改真实登录项；真实注销登录、旧 macOS 和用户机器的系统 UI 仍须另记专项结果。

核心包烟测在原程序中覆盖登录来源后台、同目录第二进程激活、原数据锁连续持有和从登录来源经关闭协调器替换重启；`SODECK_TEST_LOGIN_RESTART=1` 交给 `scripts/smoke.mjs` 开启，该脚本等待替换进程的新报告。系统登记 IO 使用非自启动测试注册表键或临时 desktop/plist 文件，开发和隔离验收程序禁止写真实用户登录项。它证明已执行断言，不证明真实系统注销/登录、全部系统禁用策略或实际鼠标入口；这些需单独记录。设置交互回归 `scripts/test-application-settings-ui.mjs` 已包含在 `npm run ui:test`。

核心版本只修改 [src-tauri/tauri.conf.json](../src-tauri/tauri.conf.json)，插件版本只修改各自的 `plugins/<名称>/Cargo.toml`。核心与插件使用独立标签、独立 Release；发布步骤、更新清单和签名配置统一维护在 [分发文档](distribution.md)。

插件版本读取由 `cargo metadata --format-version 1 --no-deps --locked` 解析，并按清单路径选取包，支持合法 TOML 写法与 workspace 版本继承。版本相关脚本测试同样需要上述固定 Rust 工具链；缺少 Cargo 或清单无效时明确失败，不从文本猜测版本。

依赖更新由 [.github/dependabot.yml](../.github/dependabot.yml) 配置：每月检查根目录与终端插件的 npm、核心/共享 plugin-client/六插件的 Cargo，以及 GitHub Actions 引用。各配置块最多保持两个未关闭的常规更新 PR，小版本和补丁更新分组，主版本更新单独审查。

漏洞告警、安全更新与安全更新分组在 GitHub 仓库安全设置中开启，不受常规更新的每月周期和 PR 数量限制。所有依赖更新沿用 Check 与相关验收，由维护者合并；配置不启用自动合并或发布，Node/Rust 工具链仍人工维护。共享客户端与插件的依赖清单、锁文件须核对兼容，源码检查不能替代五平台实际包验收。

| 工作流 | 用途 |
| --- | --- |
| [Check](../.github/workflows/check.yml) | main/PR 按实际改动选择源码和业务回归，汇总执行/跳过状态；手动补测同一引用，netem 单独按需 |
| [Package SoDeck](../.github/workflows/build.yml) | 五目标原生核心构建、打包、包内验证及更新签名；可填写原始 `core_run_id` 跳过构建，复验原包后汇总/草稿收尾 |
| [Package plugin](../.github/workflows/plugin.yml) | 按插件目录独立打包，生成插件更新清单 |
| [Verify native remote plugins](../.github/workflows/remote-integration.yml) | 按组件版本或明确 run 定位实际包，校验来源后执行五平台原生界面/远程业务验收，支持输入快照重放 |

五个系统与架构组合分别使用原生构建和实际包验证。结果、未完成项和已知风险记录在核心版本说明或对应插件 CHANGELOG；尚在实施的范围保留在临时计划。

`Verify native remote plugins` 的 `platform` 默认验证五平台；补测时可选择单个平台并重放 `verification_run_id`，或填写相同的原始组件打包 run。单平台汇总只表示所选平台通过，发行记录必须同时保留其他平台的原始成功报告。Windows ARM64 的隔离 OpenSSH 测试服务从固定 Microsoft Release 资产 API 下载并校验大小与 SHA-256。

Check 始终响应 main push/PR，不使用 workflow 级 `paths` 跳过。`select` 使用 push 的 before/after，PR 使用 base/head 的 merge-base；删除和重命名同时计入原路径。取不到完整差异、新分支零 before、未知路径、共享客户端、工具链和构建输入按扩大范围验证。选择规则集中在 [select-checks.mjs](../scripts/select-checks.mjs)，不维护依赖图。

文档检查使用 Node 和根 `package-lock.json` 锁定的现有运行时依赖；`select` 先运行 `npm ci --ignore-scripts --omit=dev`，再运行 `node scripts/check-docs.mjs` 校验全部已提交 Markdown 的结尾换行、冲突标记和本地文件链接（含图片和引用式链接）。此检查不安装 Rust 或插件依赖，不执行产品构建，也不访问外部网址或校验标题锚点。删除/重命名文档也会检查其他文档留下的链接。`format` 保留为最终汇总状态：选中的任务失败、取消或缺失会失败；未选中的任务显示 skipped，不代表测试通过。分支保护应使用始终出现的汇总状态；未选中的矩阵任务可能不出现。

| 自动改动范围 | 实际选择 |
| --- | --- |
| Markdown、docs 下的说明/配图 | 仅文档检查及其根目录依赖准备；不安装 Rust/插件依赖，不构建产品 |
| 核心前端 | 脚本、Linux 完整 UI（含 interactions、settings、正式前端构建/tsc）；不因此运行 SSH、网络或原生 Rust 矩阵 |
| Tauri 版本/核心配置、capabilities | 核心格式/脚本、Linux 完整 UI 与五平台原生核心；不追加插件源码和普通网络/文件/终端业务 |
| 单插件 UI/发行元数据 | 脚本和该插件普通业务验收；终端额外验证输入，网络使用 Linux 两架构，文件/终端使用 Linux x86_64 |
| 单插件 Rust/清单/锁文件/构建输入 | 该插件格式与五平台原生 Rust 测试、该插件普通业务验收；其他插件不跟随 |
| desktop_files 的 windows/linux/macos.rs | 对应系统两架构/一个 macOS 架构的核心测试、文件业务；Windows 保留系统专项，普通 UI 不跟随 |
| network_metrics.rs、settings/declaration.rs | 一次 Linux x86_64 完整核心套件；声明变化另走前端相关回归 |
| session_process（含 output）、remote_execution、execution_output、content_export | 五平台原生核心、Linux UI、Windows SSH、六插件普通业务验收，与连接/流生命周期相同 |
| 其他已映射核心系统/更新模块 | 五平台原生核心和相关 UI；网络、文件、终端服务分别追加自己的普通业务验收 |
| 连接、流、生命周期/RPC、平台进程、服务器路由 | 五平台原生核心、Linux UI、Windows SSH、六插件普通业务验收 |
| 共享 UI/字体/菜单 | 扩大至核心与六插件 UI/普通业务验收；不因此运行 netem 或旧版升级 |
| 共享 plugin-client、根工具链/锁文件/构建脚本、未知输入 | 五平台核心与六插件源码、相关 UI、普通业务和宿主脚本；仍排除 netem |

已知 UI 测试入口也按组件路由：`test-terminal-ui`/`test-files-ui` 仅运行对应普通业务，`test-terminal-input` 与终端 history/search-ui/benchmark 入口走终端输入、历史与脚本回归，核心 UI 测试脚本走前端和完整 desktop。共享 OpenSSH/打包脚本和未映射脚本继续扩大范围。

正式版升级夹具、脚本及其工作流变化选择五平台核心与工程脚本，工作流另做 actionlint；升级测试输入解析只选择脚本回归。精确版本对的线上升级仍在独立工作流显式执行。

Check 的 **Run workflow** 先选择需要验证的分支或标签，再选择 `suite`，手动选择覆盖自动判定。检查结果只适用于该次运行的确切提交；打包工作流不等待 Check。

| suite | 执行范围 |
| --- | --- |
| `standard`（默认） | 公共 Linux 核心/六插件格式和源码、Linux 完整 UI/脚本/输入及所选普通业务；排除 netem、Windows desktop |
| `desktop` | 保留手动 Windows x86_64 `ssh:test` 和完整 `ui:test` 对照，包含 Chromium 与终端 UI 依赖 |
| `native-core` | 五个原生目标各执行 `npm run test`；Windows x86_64、Linux x86_64、macOS ARM64 另执行原宿主脚本回归 |
| `network-plugin` | 网络插件格式、五个原生目标的 `cargo test --manifest-path plugins/network-proxy/Cargo.toml --locked`，不构建核心测试夹具 |
| `all` | 核心与六插件五平台源码、Linux 完整 UI、Windows SSH、普通网络/文件/终端与其他业务、脚本及 actionlint；组内顺序执行，矩阵并行；排除 netem |
| `netem` | 单独运行 Linux x86_64/ARM64 私有 namespace 弱网矩阵；成功/失败报告 Artifact 保留 14 天，不安装 Chromium 或 OpenSSH |

原宿主脚本回归为 `collect-core.test.mjs`、`core-release.test.mjs`、`plugin-settings-assets.test.mjs`、`release-metadata.test.mjs` 和 `smoke.test.mjs`；在 `native-core`/`all` 和共享未知输入下于 Windows x86_64、Linux x86_64、macOS ARM64 执行。自动选择仍以既有套件为单位，不能独立切分时运行较大范围；需要额外验证时可对同一 ref 补做手动套件。真实包组合与升级保持原入口，不把工作流选择当作已验收。

普通 `ui:test` 集中在 Linux x86_64 `browser` job 顺序执行，包含既有 settings、interactions 和正式 dist 回归；公共 `checks` 不再安装 Chromium 或重复这些测试。终端基础输入按选择单独执行，桌面策略输入保留其不同的测试模式。Linux UI 使用真实 catalog/workspace 核心夹具，安装 WebKit/GTK 开发库并核对 Unix 入口执行位；它不能替代原生 WebView、实际包或系统指针验收。

自动 Windows `desktop` 仅执行已选中的 SSH/系统专项；手动 `desktop` 保留完整 Windows UI 对照。Linux integration 使用三行矩阵：网络 x86_64、网络 ARM64，以及文件/终端/状态/Docker/通信的 x86_64 业务组；最多 3 行并行，各行自己的目录、端口、进程和 target，组内串行。单插件变化只生成所需行。三个任务恢复适用的既有 `network-tests` 缓存，未新增按业务组划分的大缓存键。

相关工作流/本地 action 变化和手动 `all` 使用校验下载摘要的 actionlint 1.7.12；纯文档仍按本节文档检查范围执行。`format` 汇总包含新增 `browser`，选中后失败、取消、跳过或缺失均不能通过。浏览器、Windows desktop 和 integration 的诊断 Artifact 在成功/失败时均上传，保留 14 天；发行/实际包的 30 天保留规则不变。

`native-core` 覆盖当前核心单元测试及 `tests/lifecycle.rs`、`close_lifecycle.rs`、`updates.rs`、`update_network.rs`、`connections.rs`、`component_ui.rs`、`settings.rs`、`servers.rs`、`streams.rs`、`window_config.rs`、`workspaces.rs`；Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 都在对应原生 runner 执行。`platform`、`desktop_files`、桌面行为及更新等系统分支随原生测试验证。标记为 ignored 的真实远端用例仍由对应 SSH/终端/文件/网络专项提供环境后显式运行，不能把 `native-core` 通过当作这些专项已通过。

Check 独立执行源码、类型及业务测试；核心与插件现有工作流内分别使用 package → verify → metadata/release。package 只编译、签名、归档和上传，不运行产品或业务测试；核心 `tsc --noEmit` 与文件编辑器类型检查都在 Check 单独执行；Rust 编译器自身的检查属于构建。verify 下载原包，在原生环境验证架构、资源、核心更新签名、启动和退出，核心另验资料重开；不安装 npm/Rust 构建依赖或重新构建。metadata/release 核对五份通过报告与原包提交、版本、SHA-256 后生成清单。SSH、工作台及系统桌面行为移至独立专项。

CI 的 npm 下载缓存按任务实际使用的锁文件配置；核心打包只使用根锁文件，终端 UI 测试同时使用终端锁文件，文件编辑 SDK 消费方同时使用 packages/file-editor 锁文件。Rust 缓存区分日常 `check-tests`、正式核心 `core-release-v2` 和各插件，只有成功的 main 运行保存，其他引用尝试恢复；插件及其原生测试显式覆盖共享 plugin-client 清单。Linux browser 与 Windows desktop 对齐各自原生核心 `check-tests` 的 workspace 顺序和共享清单输入，减少重复大缓存；不同 OS/架构仍隔离。升级验收夹具使用 `check-tests`，不恢复发行缓存。

包含核心的原生测试统一声明现有核心/六插件 workspace 输入，复用同一稳定 `check-tests` 键，避免每个组件/平台范围生成大份重复核心依赖。声明缓存目录不启动对应插件测试；实际执行仍由所选范围控制。仅插件的原生 job 保留小范围 workspace，不恢复核心 target。发行缓存仍只有核心自身输入。

`core-release-v2` 是一次性的缓存迁移标识，不随提交、版本或 run ID 变化。它保留固定 action 的默认工具链/环境/Cargo 输入隔离，只缓存依赖编译结果；宿主构建依赖与目标架构 release 依赖一并保留，不开启 workspace crate 或 incremental 缓存。恢复后仍执行 `npm ci`、当前前端及当前核心的正式构建。main 手动打包会保存依赖，后续适用分支和标签可恢复默认分支缓存；详见[分发文档](distribution.md)。Linux 打包依赖安装复用官方软件源修正与有限重试，不缓存系统安装状态。

MCP 的真实 OpenSSH 入口继续为 `npm run mcp:test`，覆盖多轮 pipe、跨块输入、EOF 和 PTY；Linux 额外创建一次性账号与 sudoers，用真实 sudo 验证错误密码后同任务继续。该 Linux 测试要求 openssh-server、sudo、useradd/chpasswd/visudo 及无交互 sudo 夹具管理能力，退出清理账号、配置与测试副作用。其余平台验证通用输入；不把 Linux sudo 专项推定为全部远端系统验证。

## 解析依赖与性能基准

MCP 使用已在 rmcp 链中的 schemars 1.2.3 生成参数结构、jsonschema 0.63.1 校验本地 schema；关闭后者默认远程引用、HTTP 和 TLS 功能。Docker 使用 chrono 0.4.45 的 std 功能处理时间、vte 0.15.0 的无 std 固定容量状态机清理控制序列，避免无界 OSC 缓冲。核心及配置写入插件使用 tempfile 3.27.0；核心使用既有 SSH 链的 ssh-encoding 0.3.0 和 dunce 1.0.5。均固定版本并提交各自锁文件。schema、日历、终端控制和平台临时文件的替代方案是继续维护手工实现；已确认边界较多的部分采用库，POSIX 单引号编码和 Monaco 搜索适配继续保留。

这些库的上游分别为 [schemars](https://github.com/GREsau/schemars)、[jsonschema](https://github.com/Stranger6667/jsonschema)、[chrono](https://github.com/chronotope/chrono)、[vte](https://github.com/alacritty/vte)、[tempfile](https://github.com/Stebalien/tempfile)、[RustCrypto SSH](https://github.com/RustCrypto/SSH)、[dunce](https://gitlab.com/kornelski/dunce)。本轮核对各库发布和 API；dunce 属于低频维护的小型路径库，其余仍有近期发行。升级仍需运行对应边界回归，不依据下载量替代兼容验证。

局部性能基准使用锁定 Node/Chromium：

```sh
node scripts/benchmark-boundaries.mjs --report artifacts/boundaries.json
node scripts/benchmark-boundaries.mjs --process --report artifacts/boundaries-process.json
```

第二条需要准备当前 Docker 测试插件。每场景预热 2 次、采样 30 次，记录 P50/P95/最大值和样本；文件树测已加载行的重复刷新，核心日志测 500 条短/长正文的 DOM 更新加布局，Docker 的 5 万/20 万投影测合成布局与绘制，process 场景另测真实插件 RPC 的 1 万条布局分页。合成场景的 900-ID 页只是固定对照，不代表当前后端页大小。结果不包含磁盘扫描、原生 WebView 或远端 IO，也不作为跨机器 CI 耗时断言。先分解实际瓶颈，再决定是否引入虚拟化库。

## 文档与开发收束

正式文档描述当前检出的代码，功能迭代持续修改同一份说明。开始改动前阅读对应功能、相关源码与回归，写清用户可验收行为；代码与既定要求不一致时，先确定缺陷、文档过时还是已确认的需求变化。

| 内容 | 唯一维护位置 |
| --- | --- |
| 功能入口、行为、失败、生命周期与使用限制 | [功能目录](README.md#核心功能)中的对应文档 |
| 产品范围、架构归属、接口字段与兼容 | product.md、architecture.md、plugin-api.md |
| 环境、命令、检查选择 | 本文 |
| 打包、签名、发布、包复用与恢复 | [分发流程](distribution.md) |
| 核心/插件版本、包来源和实际验证范围 | docs/releases/ 或插件 CHANGELOG |
| 正在设计、实施或约定验收未完成的工作 | docs/plans/ 中每主题一份计划 |

功能文档按“用途与入口 → 当前行为 → 生命周期/失败/限制 → 维护约束 → 主要源码与回归入口”组织；简单功能可合并章节。只链接主要模块，不维护完整函数清单，不抄命令和 API 字段，不追加逐轮进度。README、usage 和插件 README 提供入口，详细规则链接唯一归属。

需要跨会话规划时，在 docs/plans/ 新建一个主题，最低模板如下；小改动在任务描述中说明即可：

```markdown
# 主题
状态：正在设计 / 实施中 / 待验收；核对基线。

## 目标与验收
本次范围、可见行为、成功/失败条件。

## 现状与决定
源码证据、必要选择、尚未解决的问题。

## 剩余步骤与验证
影响的代码、回归入口、实际通过和跳过范围。

## 回写与收束
完成后更新哪些功能、架构/API 章节，证据写到哪里。
```

实现过程中及时把已完成行为写入正式文档，计划只留下未完成部分及必要上下文。待定提案不能出现在当前能力描述中；源码已实现但待发行的部分简短标注，并链接组件版本记录。

结束任务前核对：相关代码和验收完成；受影响的正式文档与代码一致；必要设计理由已进入架构、API 或功能约束；发行证据已进入组件版本记录；引用有效；已完成计划、评审过程和无用附件已删除。没有完成计划归档目录，也不把计划改名成实施记录继续累积。Git 保留历史，历史发行记录可链接固定提交中的原始材料。

源码完成但约定验收尚未完成时保留对应计划；取消或被替代的工作先安置仍有效的需求。若任务包含发布，必须完成该组件的发布验收；若发布另行安排，待发行记录承接剩余交付事项。浏览器、源码、原生包和设备验证分别记载，跳过不等于通过。

纯文档改动运行文档链接/格式检查与 diff 检查，不为此构建产品；若同时变更代码，按上面的实际范围检查。文档检查脚本默认读取 Git 已跟踪文件；未暂存删除与新增并存时，可调用其导出的 checkMarkdown 检查当前实际文件集合，不能漏掉新文档或把删除文件读失败当成有效验收。

返回 [项目首页](../README.md) · [文档索引](README.md)

## 服务器连接分享回归

`npm run test` 中 `tests/server_sharing.rs` 验证独立数据目录往返、凭据/密钥、范围映射、冲突、回滚及备份清理；`tests/connections.rs` 的分享专项复用真实 SSH/SOCKS5 夹具验证导入后的加密私钥认证。`npm run ssh:test` 的 OpenSSH 路径也通过实际导入的私钥建立连接。`node scripts/test-server-ui.mjs` 验证真实核心与目录、三主题窄窗及导入期间的关闭忙状态。

实际核心包烟测增加 `serverSharingNative`，覆盖分享按钮、原生剪贴板、超过 256 KiB 的右键粘贴预览、受管私钥落盘与重开，不连接用户服务器。使用公开的合成测试私钥，剪贴板测试恢复原文本。沿现有五平台 package/verify 执行，不以浏览器夹具替代原生包结果。

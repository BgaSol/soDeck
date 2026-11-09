# 基础逻辑复用实施记录

对应[分步整改计划](plan-standard-library-adoption.md)。本记录覆盖第一至第三阶段及第六阶段已经实施的部分；每个阶段分别记录代码变化、验证证据和待验收目标。第四阶段三系统文件适配及扩展的窗口/测试绑定见[系统绑定实施记录](implementation-native-system-bindings.md)，DOM 库迁移继续按原方案推进。

## 缺陷复现与原因

修改前新增五项 Cargo 元数据回归，均失败：合法的 `version="1.2.3"` 与单引号写法被拒绝；依赖表在前时误读 `9.8.7`；workspace 继承版本无法读取；无效 TOML 中存在版本行时仍被接受。原因是发布和打包脚本用全文件正则读取版本，未识别 Cargo 包和 TOML 结构。

修改前新增文件 URI 大小写回归，在核心 library 测试中失败，合法 `FILE:///…` 被报为非本地文件 URL。原因是手写 scheme/localhost 前缀匹配区分大小写，同时自行维护百分号解码和本地路径转换。

## 第一阶段：插件版本读取

- 新增 `scripts/cargo-package.mjs`，调用 `cargo metadata --format-version 1 --no-deps --locked --manifest-path <路径>`；按规范化后的清单路径选择唯一包，Windows 比较使用不区分大小写的路径身份。
- 发布元数据、插件打包和 OpenSSH 测试中的包目录定位共用此函数；不按 workspace 包顺序、二进制名或版本文本猜测包身份。
- Cargo 启动/退出失败、无效 JSON、缺少包或非正式版本明确失败，无正则回退。保留现有正式版本与标签约定，核心版本仍取 `tauri.conf.json`。
- 插件工作流的选择和 Release 汇总任务增加固定 Rust 1.99.1 安装步骤，打包任务沿用原有固定工具链。核心与插件继续独立构建、分发。
- 七项真实 Cargo 回归覆盖 TOML 写法、前置依赖、workspace 多包顺序、继承版本、无效清单、虚拟 workspace 误选和缺少 Cargo。

未新增生产依赖。版本读取使用项目原有 Cargo 工具链；替代方案为新增 TOML 解析库，但自行解析仍需处理 Cargo workspace 继承和包身份，因此采用 Cargo 官方元数据接口。

## 第二阶段：文件 URI 转换

- 复用直接依赖中的 `reqwest::Url`，由标准 URL 解析器处理 scheme/host，再用 `to_file_path` 转为当前平台的本地路径。
- 保留 URI-list 的换行、注释、GNOME copy/cut 标记、仅本地 file 来源和 4096 项限制；不接受普通文本路径或远端主机。
- 使用库提供的语法违规回调，拒绝非法百分号和会被忽略的空白，不再手写百分号解码器。NUL 在规范化前拒绝，避免包含 `%00` 的路径段被 `..` 消除后漏检。
- 查询与片段不作为文件名静默丢弃；文件名中的空格、`#`、`?` 等应由文件 URI 百分号编码。转换只解码一次，`%2520` 保留为文件名中的 `%20`。
- URI 本身须为有效 UTF-8 文本，Unix 非 UTF-8 文件名通过百分号表示，由库转换回 `OsString` 字节；不通过有损字符串转换创建路径。

核心包含五项 Windows 可执行的 URI 回归，覆盖大小写、中文、百分号、一次解码、无效输入、部分列表失败及条目上限；另有 Unix 专用回归验证 `%FF%FE%25` 转换为原始路径字节。

未新增依赖或改变插件 API。本地文件上传的目标选择、任务与会话归属仍由既有核心和文件插件实现。

## 第一、第二阶段本机验证（2026-11-06）

环境为 Windows x86_64，Node 24.22.0、Rust 1.99.1。实际使用本工作树源码和锁文件。

| 验证 | 结果 |
| --- | --- |
| `npm run check` | TypeScript 与核心 Rust 格式通过 |
| `npm run scripts:test` | 33 项通过，含新增七项 Cargo 回归 |
| 改动脚本 `node --check` | 语法检查通过 |
| `npm run test` | 94 项通过，5 项需要实际插件/OpenSSH 的用例按原规则 ignored |
| 核心 URI 定向回归 | Windows 上五项通过；修改前大小写用例已失败复现 |
| `npm run web:build` | 正式前端构建通过 |
| `npm run build -- --bundles nsis -- --locked` | Windows x86_64 核心 NSIS 构建成功 |
| 核心实际包运行 | 从 NSIS 解包后运行包内 `sodeck.exe`；渲染、多原生窗口移出/回移、关闭、保存资料、重开与正常退出通过 |
| 三个独立 `plugin:build` | 终端、文件、网络隧道的 Windows x86_64 ZIP 均构建成功，并检查原生二进制架构 |
| 实际插件 ZIP 验证 | 解包后 `plugin.json`、归档元数据、更新清单与 Cargo 版本一致，SHA-256 校验通过；包内程序均完成 initialize/shutdown 并以 0 退出 |

插件验证报告位于 `artifacts/standard-library-adoption/plugin-packages.json`，验证使用隔离的数据目录，未建立远程会话。核心包为 `src-tauri/target/release/bundle/nsis/SoDeck_0.9.0_x64-setup.exe`，包内运行报告位于 `artifacts/standard-library-adoption/core-data/smoke-result.json`。本次解包验证未执行安装器的安装/卸载流程。

核心两次启动均正常完成烟测并以 0 退出。WebView2 在关闭时输出过 `Failed to unregister class Chrome_WidgetWin_0. Error = 1412`，未导致烟测失败；本批未整改该框架诊断。

## 第三阶段：菜单定位与独立资源构建

第三阶段按完整方案一次实施，包含工作区、组件列表、文件主/子菜单、共享文本菜单和终端专用菜单，以及三个插件的共享资源构建。

修改前增加工作区缩窗回归：菜单在 1100×720 窗口右边打开后，将窗口缩至 720×480，菜单仍保留旧坐标，3 秒内无法进入视口，稳定失败。原因是打开时只计算一次位置，并用固定宽度手工避让。

- 固定直接依赖 `@floating-ui/dom 1.8.0` 并更新锁文件。2026-11-06 查询 npm 元数据，该版本发布于 2026-07-11，MIT 许可，来自持续维护的 Floating UI 项目。替代方案是 Popper；选择 Floating UI 是因为原生 DOM、虚拟指针锚点和 iframe 视口适合当前代码，并可复用已有 Vite。
- 新增 `src/menu-position.js` 和类型声明。通过 `computePosition`、`offset`、`flip`、`shift`、`size`、`hide`、`autoUpdate` 处理定位，只返回 `ready/update/dispose`，不维护菜单动作或服务器状态。
- 菜单按自身视口留 8px，按钮菜单留 4px 间距，子菜单留 7px，依次尝试右、左、下、上。元素菜单跟随滚动；指针菜单在来源滚动时关闭，缩窗时重新避让。来源移除、隐藏、inert 或工作区失效时关闭，迟到测量和尺寸写入检查实例序号。
- 区分来源的实际滚动与打开前排队的旧滚动事件，避免窗口恢复尺寸时误关新菜单。菜单自己的列表滚动继续有效。
- 文件子菜单保留在父菜单 DOM 内，切换/离开使用 180ms 悬停缓冲，进入子菜单取消关闭；点击和键盘立即操作。文件选择集合、目录目标及异步粘贴状态继续使用打开时快照，更新状态不重建按钮。
- 外框保留装饰边框和阴影，只有内部列表滚动。共同菜单显式使用 `border-box`，避免插件页面默认 `content-box` 将边框/内边距加到可用尺寸之外。显式隐藏关闭的 Popover 与子菜单，避免 flex 样式覆盖浏览器默认隐藏规则。
- 文本菜单保留选区、只读/密码规则及异步剪贴板校验；对话框内使用自身 DOM，支持手动 Popover 时进入顶层，不支持时使用对话框内固定定位。终端专用菜单复用定位入口，仍保留 xterm 的鼠标报告与输入逻辑。
- `scripts/plugin-menu-assets.mjs` 使用锁定 Vite 输出自包含的普通 `ui/text-menu.js`，暴露原有 `window.installTextMenu` 与共用定位入口，并附带 Floating UI 三个包的许可证。保留其他插件资源，包内无运行时 npm 导入或 CDN 请求。
- 终端本地 UI 构建和三个插件打包使用同一资源入口。插件 Actions 打包任务增加根目录 `npm ci`；核心仍独立构建。构建前提更新在开发和分发文档，未更新根 README 或应用/插件版本。

## 第三阶段本机验证（2026-11-06）

| 验证 | 结果 |
| --- | --- |
| `npm run check`、`npm run plugin:check`、`git diff --check` | 类型、核心/三个插件 Rust 格式、补丁空白检查通过 |
| `npm run scripts:test` | 34 项通过；新增资源回归实际构建普通脚本，验证浏览器入口、许可证与保留原 UI |
| `npm run test` | 94 项通过，5 项按既有规则 ignored；文件 OpenSSH 用例另行实际执行并通过 |
| `npm run interactions:test` | 8 项通过；缩窗失败复现修复，三主题、过滤、滚动、目标替换、迟到测量均通过 |
| 文件真实 iframe | 600×540、320×200、180×160；子菜单右/左/下/上均出现，真实鼠标跨越间距后等待并点击，操作仍作用于原目录 |
| `node scripts/test-workspace-ui.mjs` | 18 项通过；菜单、搜索、焦点、刷新、移窗与工作区目标回归通过，无页面异常 |
| `npm run desktop:test` | 核心/插件文本菜单、选区、撤销、异步剪贴板和终端真实 xterm 输入通过；另覆盖 320×240 对话框的 Popover 与禁用该 API 的回退 |
| `npm run ui:production:test` | 正式前端构建与 10 项回归通过，无页面异常；截图改为获取当前页面区域，避免状态刷新替换节点导致截图中断，内容断言保留 |
| `npm run files:test` | 隔离回环 OpenSSH 的真实文件 Rust 用例与发行 UI 通过；包括上传/下载、新建、重命名、删除、复制移动、多选、草稿及离线导出 |
| Windows x86_64 核心 NSIS | 重新构建成功；解包后的程序完成渲染、多原生窗口移动/关闭、资料保存重开及正常退出 |
| 三个独立 Windows x86_64 插件 ZIP | 架构、版本、归档哈希验证通过；包内程序 initialize/shutdown 后以 0 退出 |
| 实际 ZIP 资源 | 三包脚本与浏览器回归实际加载的脚本逐字节一致，普通脚本入口、加载顺序与许可证完整 |

第三阶段实际包报告位于 `artifacts/menu-position/plugin-packages.json`；三个包分别位于 `artifacts/menu-position/plugins/<插件名>/`。核心来自重新生成的 `SoDeck_0.9.0_x64-setup.exe`，解包目录为 `artifacts/menu-position/core-unpacked-final/`，烟测报告为 `artifacts/menu-position/core-data/smoke-result.json`。验证均使用隔离资料，不修改用户安装或真实远程服务器；未执行 NSIS 的安装/卸载流程。WebView2 退出时仍有既有 class 1412 诊断，烟测成功。

## 第六阶段：SOCKS5 报文库接入

2026-11-07 按[完整计划](plan-socks5-library-adoption.md)接入核心客户端、网络插件入口和同步检查器。库负责报文编码与解码，核心继续负责已有 Stream、逐站认证、远端 DNS、超时取消和正文转发；网络插件继续通过版本化 RPC 连接目标及移交通道。

修改前的现有基线为 Windows 核心 94 项通过、5 项 ignored，网络插件 5 项通过。新增真实插件异常输入回归在修改前失败：问候只有 `[5]`、错误版本或空方法列表时，manager 发出了十字节 CONNECT 错误回复。原因是错误分支只判断是否存在 SOCKS 对象，未判断协商阶段；完整入口回归在修改后通过。

- 核心用库编码问候、密码认证和 CONNECT，解码方法、认证与绑定地址响应。配置密码时只提供密码方式，拒绝免认证降级；目标和凭据在网络写入前检查。非零 REP 保留原始数值，只有四字节失败前缀也立即失败；成功帧精确读取，不预读下一站或业务正文。
- 插件仅保留问候、请求、目标连接及完成状态。按当前帧剩余量请求 RPC 读取，最长域名仍按 4、1、257 字节读取；地址和端口交由库解析。免认证 CONNECT、目标校验、默认回环地址及 REP 映射保持原有产品规则。失败响应按阶段发送且只发送一次，成功回复先排入核心命令队列再移交正文。
- 检查器复用插件内的帧长度和编解码函数，完整读取 IPv4、IPv6、域名绑定响应。每个逻辑响应使用 12 秒绝对截止时间，每次 read 只使用剩余时间；逐字节输入不能不断重置超时。检查仍表示目标 TCP 可达。
- 新增核心八项协议回归及一项实际 socket 取消回归，覆盖认证拒绝、空密码与字节长度、三类地址、每个绑定帧截断边界、原始 REP、正文保留及三个协商阶段取消。插件新增分帧、方法拒绝、目标/前缀错误、重复响应和检查 deadline 回归。
- 真实入口回归使用 curl 8.21.0 检查 IP、远端域名及目标连接失败；失败须返回 curl 的代理错误码 97、没有 HTTP 正文，并确认目标主机已到达远端 SSH 夹具。测试核心用 `fast-socks5 1.0.0` 独立服务端建立免认证、密码及 SSH/SOCKS 混合链。另验 128 KiB 上传、64 KiB 下载、双方半关闭、请求紧跟 `tail`、停用/异常退出、16 个待协商与 64 个活动连接的限额和名额恢复。
- `network_socks_lifecycle` 在 Windows 与 Linux 验证停在问候/请求阶段的输入按原期限结束、受控挂起的目标连接返回 REP 6，以及目标仍在连接时停用插件后五秒内关闭客户端和复用监听端口。请求读取超时时，核心可能先关闭流，插件无法补发错误回复；测试接受关闭或完整失败帧，禁止误报成功，没有扩大产品超时。

### 依赖与代码规模

两个组件分别增加生产依赖 `socker = "=1.0.0"`，锁定 `caret 0.10.1`。socker 为 MIT-0，caret 为 MIT OR Apache-2.0；库没有 IO、Tokio、socket、DNS 或平台 FFI。固定 Rust 1.99.1 满足其最低版本要求。它是较新的小项目，使用规模与维护证据见计划；本次没有把它称为成熟的大规模部署方案。完整运行时库会影响现有 IO、认证选择或诊断，所以采用报文接口；保留现有实现是替换无法通过验收时的回退选择。

核心新增的 `fast-socks5` 仅为 MIT 测试依赖，新增其所需 `async-trait`、`socket2 0.5.10`、`syn 3.0.9`、`tokio-stream`。两个锁文件没有升级原有包版本；生产新增包仅为 socker 与 caret。curl 只用于测试工具，下载版本、SHA-256 和源码编译入口在 `scripts/network-tools.mjs` 中固定，发行构建不调用它。

下表按完整生产文件计算物理行数，包含空行及新增辅助函数，排除测试模块；基线取本阶段开始时的对应文件，与 HEAD 一致。不能把原计划约 210 行的协议片段估计和完整文件混用。

| 文件 | 修改前 | 修改后 |
| --- | ---: | ---: |
| 核心 `connections/socks5.rs` | 86 | 120 |
| 插件 `socks5.rs` | 107 | 143 |
| 插件 `checks.rs` | 107 | 173 |
| 插件 `manager.rs` | 321 | 323 |
| 合计 | 621 | 759 |

净增加 138 行。手工报文拼装、地址和端口解析已移除，但显式消息类型、按帧读取、检查截止时间与阶段异常处理增加了适配代码。因此收益是减少自行维护的协议细节并把策略写清楚，当前实现没有达到总代码更少的目标，不宣称大幅精简。统计报告为 `artifacts/socks-library/code-size.json`。

### 第六阶段验证与实际包

Windows x86_64 使用 Node 24.22.0、Rust 1.99.1。Linux x86_64 使用 Docker 内的原生 Rust 1.99.1、Node 24.22.0 与 Debian Bookworm；GUI 包验证在 Xvfb 和 D-Bus 会话中运行，容器 root 测试显式关闭 WebKit 沙箱。没有用 x86_64 容器替代 ARM64 验收。

| 验证 | 结果 |
| --- | --- |
| `npm run check`、`npm run plugin:check` | Windows 类型与相关 Rust 格式通过 |
| `npm run scripts:test` | 34 项通过；新增工具脚本另经 `node --check` 验证 |
| 核心 `npm run test` | Windows 103 项通过；Linux 104 项分别验证通过，首轮耗时断言重跑情况见下文。默认另有 8 项 ignored，本阶段的五项网络用例分别运行；其余终端、文件与独立 SSH 用例未在本阶段重跑 |
| 网络插件 `cargo test --locked` | Windows、Linux x86_64 各 13 项通过 |
| `npm run network:test` | Windows 独立服务端混合链、真实 OpenSSH、协议/curl/双向传输和网络 UI 通过；随后扩展的限额回归在 Windows 与 Linux 通过 |
| 最终 `network_socks_protocol` 定向回归 | 补入 curl 目标拒绝后，Windows 197.01s、Linux 138.53s 分别通过；包含 IP/远端域名成功、代理失败码 97、正文传输及资源名额恢复 |
| Linux 协议互操作 | 原生 curl 源码编译分支通过；实际网络插件的异常报文、curl、双向传输和限额回归通过 |
| Windows 核心及网络插件独立构建 | 新 NSIS 与 ZIP 构建成功；从包中解出程序并检查 x86_64 架构、插件版本与归档哈希 |
| Windows 实际包运行 | 两次启动含持久资料重开；真实 OpenSSH 下的 TCP/SOCKS、原生窗口移出/回移、资源释放与退出通过 |
| Linux 核心及网络插件独立构建 | DEB、AppImage 与网络插件 tar.gz 原生构建成功，解包后校验核心架构；两种核心实际包各两次启动完成同一 OpenSSH/网络烟测并退出 |

Windows 产物来自本阶段重新构建的 `SoDeck_0.9.0_x64-setup.exe`，报告为 `artifacts/socks-library/packages.json` 与 `core-data/`。NSIS 中的程序只因 Tauri 的包类型标记与构建目录程序相差三个字节，报告校验除此以外完全一致。未执行安装器安装/卸载。WebView2 退出时仍有既有 class 1412 诊断，没有导致验证失败。

Linux 包为 `SoDeck_0.9.0_amd64.deb` 与 `SoDeck_0.9.0_amd64.AppImage`，测试加载解包后的核心与网络插件程序，不使用开发目录程序替代实际包；归档哈希和两种包的运行结果汇总为 `artifacts/socks-library/linux/packages.json`。Linux 首轮全量测试中的既有 `timeout_and_ignored_shutdown_are_bounded` 用例在 7 秒上限外多用约 4ms，未修改其断言；单独重跑通过，修改前实现的同一用例也通过，其余 103 项另运行通过。该耗时边界对并行负载敏感，应保留首轮失败信息。Linux 容器与桌面运行环境仍有差异，未将 Xvfb 结果外推至 ARM64 或 macOS。

### 第六阶段弱网络对照

`scripts/test-network-netem.mjs` 在私有 network namespace 的回环接口使用真实 `tc netem`；测试实际核心、密码 SOCKS 服务端、SSH 会话、外置网络插件与 128 KiB 传输。正常结束及脚本异常使用 finally 删除 namespace 和临时插件目录；失败也输出 JSON 报告。核心和插件的原有超时没有拉长。

当前拓扑对全部夹具 TCP 段施加相同延迟/丢包，覆盖入口和远端段，但没有实现计划中用 veth 分别控制两段的拓扑；回环 MTU 为 65536，分包与真实网卡也有差异。多段延迟会叠加，结果不能换算成同参数互联网链路成功率。测试内核为 WSL2 Linux 6.18.54.1，iproute2 6.1.0 不支持 netem seed，报告记录 `randomSeed: null`，修改前后为同参数的独立随机样本。报告记录分组总耗时与失败所在 route/relay 阶段，没有单独收集每站协商耗时；正文测试另有 20 秒等待保护，不能把该保护触发都归为产品协议超时。

修改前对照只在容器内的独立 `/baseline` 目录恢复四个生产文件，保持相同测试夹具、锁文件和工具；当前工作树没有重置。对应源码哈希为 `artifacts/socks-library/baseline-sources.json`。两组使用独立 namespace，生产只有接库后一条实现路径。

修改前与接库后各运行完整 82 次矩阵，再分别复测严重组 20 次，共 204 次尝试。四份运行报告的 `completed` 均为 true，所有配置测试进程以 0 退出；这表示按该配置的验收规则结束，不表示所有网络尝试成功。

| netem 配置 | 修改前成功次数 | 接库后成功次数 | 首轮分组总耗时：修改前 / 接库后 |
| --- | ---: | ---: | ---: |
| 0ms、0% 丢包 | 10/10 | 10/10 | 23.269s / 20.532s |
| 50ms ± 10ms、0% 丢包 | 10/10 | 10/10 | 55.682s / 46.761s |
| 150ms ± 30ms、1% 丢包 | 20/20 | 20/20 | 220.643s / 217.341s |
| 300ms ± 50ms、3% 丢包 | 20/20 | 20/20 | 441.175s / 453.200s |
| 500ms ± 100ms、5% 丢包，首轮 | 12/20 | 16/20 | 742.614s / 735.972s |
| 同一严重组复测 | 11/20 | 8/20 | 759.799s / 748.377s |
| 100% 丢包 | 0/2 | 0/2 | 60.022s / 60.043s |

严重组两轮合计为修改前 23/40、接库后 24/40，首轮方向在复测中反转。随机样本规模有限且没有固定种子，不能据此判定库提高或降低了成功率。两轮严重组全部失败均发生在正文 `relay` 的测试侧 20 秒等待保护；黑洞每次在核心 `tcp` 阶段按原有 30 秒期限失败，没有误报成功。所有成功尝试逐字节验证 128 KiB 数据及紧跟请求的 `tail`，无丢失、重复或混入协议字节。

原始报告为 `artifacts/socks-library/netem-baseline.json`、`netem-current.json`、`netem-baseline-repeat.json`、`netem-current-repeat.json`。首轮使用较早的报告格式，没有 `resourcesCleaned` 字段；运行后实查 namespace 列表和临时插件目录均为空。两份复测报告均记录 `resourcesCleaned: true`；另用未知配置真实触发异常，`netem-invalid-profile.json` 记录 `completed: false`、`resourcesCleaned: true`，确认失败路径也清理并留存报告。

完成报告复制和最终 curl 回归后，已删除本任务创建的 `sodeck-socks-netem` 容器；宿主保存上述报告、源码对照哈希和实际包产物。没有停止 Docker Desktop 或删除其他容器、镜像。

### 第六阶段待验收与风险

- Windows ARM64、Linux ARM64、macOS ARM64 尚无本阶段的原生编译、打包及实际包运行证据，不能判定五目标验收完成。现有工作流已加入网络插件协议测试，Check 另加入 Linux x86_64/ARM64 的互操作与 netem；尚未执行 GitHub Actions。
- 按段分别施加干扰、单站与复杂混合路径的完整弱网络矩阵，以及每站耗时仍未覆盖。
- socker 的维护规模小；库允许的未知协议值、空域名等仍需产品策略拦截。版本升级应复跑本阶段边界及独立互操作；报文库不接管 TCP 重传，不能据此宣称高丢包稳定性提高。
- 源码实施时未修改应用、插件 API 与发行版本，没有创建标签、发布 Release 或执行部署。随后按用户要求整理源码提交并同步远端，合并检查见下节；本记录只说明上述库复用工作。

## 远端源码同步（2026-11-07）

以远端 `main` 的 `1de90a9a685354ae668960e99a421fdecbbd30cf` 为基础整合本工作树。远端已包含 Markdown 与工作台点击修复，本次只提交尚未合入的 Cargo 元数据、文件 URI、菜单定位和 SOCKS5 改动；保留远端已有的缓存、草稿恢复、按版本选包及原生验收流程。网络检查复用远端的 Ubuntu 镜像适配和固定缓存动作版本。

合并后在 Windows x86_64 再次通过类型、核心及插件 Rust 格式、前端正式构建、65 项脚本测试、103 项核心测试、13 项网络插件测试、8 项菜单交互回归，以及受影响工作流的 actionlint 检查。核心常规测试另有 8 项环境用例按既有入口 ignored，其此前的独立执行证据保留在第六阶段记录中。

本次同步只推送源码，不准备或发布 Release；保留远端现有核心 0.9.1 和各插件版本。合并检查不替代上述三个 ARM64 目标、分段弱网络及每站耗时的待验收项。

## 第一至第三阶段当时的未完成与风险

- Windows ARM64、Linux x86_64/ARM64、macOS ARM64 尚未在本批原生构建、打包或运行，不能将 Windows 结果外推。现有五目标核心打包工作流会执行核心 Rust 测试，但工作流声明不等于已经运行。
- Unix 非 UTF-8 路径回归尚未在 Unix 系统运行；实际 Linux 文件管理器复制、GTK 文件剪贴板读取和上传场景仍待原生验证。本机 Docker Linux 引擎未运行，没有以模拟路径转换替代该验证。
- Cargo 元数据读取现在明确要求 Cargo 可用；发布相关任务已经声明工具链，调用失败不会猜测版本。
- URL 按标准规范化路径；不再将含未编码查询、片段、非法百分号或违规空白的条目直接拼为文件路径。
- 本批没有创建 Git 标签或 GitHub Release，尚未执行 GitHub Actions。应用/插件版本未修改。
- 第三阶段的浏览器与 Windows x86_64 实际包验证已完成；其他四个目标仍待原生环境验收，混合 DPI、多显示器以及不同 WebView 版本的实际菜单交互也需要对应环境核验。

工作区原有未提交的工作台交互、剪贴板系统适配、Markdown 更新说明和其他工作流改动保留。本批运行结果基于包含这些改动的当前工作树；不要将整个工作区作为本批单一主题提交。

# 首版本机验证记录

## macOS 首次本机验证（2026-10-15）

环境：Apple Silicon arm64，macOS 27.0.1，Node.js 24.22.0，Rust 1.99.0，Xcode Command Line Tools。

- `npm ci`、前端 61 项测试、TypeScript/Vite 生产构建通过。
- Rust 常规测试 38 项通过，2 项默认忽略；其中 SSH/SFTP loopback 测试另外显式运行并通过，百万条日志压力测试未执行。`cargo fmt --check` 通过。
- 修正下载相关测试的临时目录准备：先解析 macOS `/var` 临时目录路径中的系统符号链接，再验证同名处理、空目录及链接拦截；链接拦截测试同时确认普通路径能通过，避免因系统路径而误判通过。此次未修改下载功能实现。
- `npm run tauri -- build --debug --bundles app` 成功生成原生 arm64 调试应用，输出到 `src-tauri/target/debug/bundle/macos/SSH Workspace.app`；通过 `open` 启动并确认应用进程运行。应用内嵌前端资源，无需 Vite 服务。
- 协议测试使用 `tests/ssh_fixture.py`，SSH/SFTP 为真实本地传输，Docker 为模拟响应。未验证真实远程服务器、release 构建或分发签名/公证。
- 既有 Playwright 桌面用例依赖 Windows WebView2/CDP，本轮未在 Mac 执行。原生窗口中的交互、文件选择器及完整多窗口流程仍需手动测试，不能将启动成功视为完整桌面回归通过。

本机联调服务可用 `.qa/venv/bin/python tests/ssh_fixture.py` 启动，连接地址 `127.0.0.1:22222`，用户名 `ops`，密码 `fixture-password`。下文保留 Windows 历次验证记录。

验证日期：2026-10-14。环境：Windows x64，Node.js 24，Rust 1.99，MSVC 2022，WebView2 154。

## 已通过

- TypeScript 类型检查与 Vite 生产构建。
- 前端规则测试 4 项：多标签规范化/去重，名称、地址与标签组合筛选，已有 Tab 聚焦与同服务器独立新连接，关闭活动/后台/最后一个 Tab 的选择行为。
- Rust 单元测试代码已增加密码原样保存、清空和单字段更新覆盖；本机应用程序控制策略阻止执行新生成的 Rust 测试二进制，因此本轮只确认编译和 Clippy，通过实际桌面端验证了相同的 SQLite 路径。
- Rust loopback 联调：真实 SSH 握手、未信任/错误主机指纹拒绝、错误密码拒绝、SFTP 目录读取、Docker 检测/列表、bash 缺失后回退 sh。
- 真实 Tauri 桌面端自动化场景 5 项：服务器管理与 SSH/SFTP/容器窗口，Docker 失败状态，多 SSH Tab 状态隔离，失败/待连接/远端退出的会话生命周期，密码保存/自动连接/清空/修改。最终完整回归 5 项全部通过。
- 多会话：不同服务器与同一服务器的额外连接输入输出独立；后台终端继续接收输出；切回 Tab 后目录、文件选中项、Docker 展开状态保留；切换后到达的目录查询结果归属于原页。
- Docker 自动刷新只在当前活动 Tab 执行，切回时更新；关闭活动或后台 Tab 不影响其他 SSH，也不关闭已打开的独立容器窗口。
- 错误密码只影响对应 Tab；认证尚未完成时关闭 Tab，后续返回的连接会释放；远端退出保留页面与输出，重新连接保留历史且不增加 Tab；键盘 Delete 可关闭最后一个 Tab。
- 密码认证配置可在编辑窗口保存、修改和清空；保存密码会在重启后读取，新 Tab 与重新连接自动使用；认证失败后修改密码只更新数据库字段，不影响其他活动 Tab。
- Docker 失败场景：命令缺失、权限不足、服务不可达、查询超时、容器已停止、容器无 bash/sh；主终端保持可用。
- 界面刷新后的 SSH 会话清理；关闭容器窗口不关闭主终端。
- Rust Clippy（warnings 视为错误）。
- npm 全依赖审计：0 个已报告漏洞。
- Windows release 可执行文件构建成功（约 11 MB）。关闭 Vite 开发服务后，内嵌页面、SQLite 读取、同服务器两个独立 SSH Tab、SFTP 目录隔离、Docker 树与独立容器窗口均可使用，无前端运行时错误。深浅主题截图已检查。已验证的文件复制到 `src-tauri/target/release/ssh-workspace.exe`，与显式 Windows target 构建产物哈希一致。

上述协议与桌面联调使用 `tests/ssh_fixture.py`：SSH/SFTP 为真实本地协议服务，Docker 命令输出为受控测试响应，不代表已经验证真实 Docker daemon 的所有版本或配置。测试过程没有连接用户的生产服务器。

桌面回归使用独立的 `com.sshworkspace.qa` 数据库和 WebView2 配置目录。打包验证仅创建本地测试服务器，完成后删除测试记录并关闭连接与测试服务。联调中曾观察到一次测试 SSH 服务的密钥交换失败，诊断日志保存在 `.qa/ssh-paramiko.log`；调整测试服务算法配置后最终完整回归通过。

## 验证范围

Windows 本机构建与交互已执行。macOS、Linux 的构建检查已配置在 GitHub Actions 中，本次没有对应系统的执行环境；尚不能将其标记为实测通过。真实 OpenSSH/Docker 服务器、特殊 SSH Agent、需要 TLS 或 SSH transport 的 Docker context 仍需进一步兼容性验证。

截图位于本地 `.qa/desktop-connected.png`、`.qa/container-independent.png`、`.qa/multiple-ssh-tabs.png` 与 `.qa/release-multiple-tabs{,-dark}.png`，该目录及测试密钥不进入 Git。

## Apple 风格界面优化验证

同日完成全局样式优化：侧栏、会话标签、终端工具栏、文件/服务面板、状态标签、菜单、编辑/认证/指纹弹窗和独立容器窗口。终端颜色与滚动条底色保持一致，深色模式的主按钮白字对比度约为 4.95:1。

- TypeScript/Vite 生产构建通过，前端规则测试 4 项通过，真实桌面端完整回归 5 项通过。
- 在独立的 `com.sshworkspace.appleqa` 数据库中验证三个并发 SSH Tab、主终端输入、SFTP 文件和 Docker 树、独立容器窗口；未出现前端运行时错误。服务器均为本地 SSH 测试服务，Docker 列表来自受控测试响应。
- 检查浅色/深色、空状态、弹窗与菜单，以及 860 × 580 内容视口：无横向溢出，文件和服务内容可独立滚动。
- 手动验证分隔条连续拖动、保留抓取位置、宽高边界和方向键调整；分隔条向左拖动 60px，右侧宽度同步增加 60px。
- 通过 WebView2 的媒体偏好模拟确认减少动态效果、减少透明度和提高对比度生效；减少动态效果时禁用终端光标闪烁。
- Windows release 构建成功。关闭 Vite 开发服务后确认独立程序从 `http://tauri.localhost/` 加载新版内嵌界面、正常读取本地 SQLite，无启动错误。测试记录和连接已清理，测试客户端及服务已关闭；正式数据库仅做读取验证。
- 更新后的可执行文件已复制到 `src-tauri/target/release/ssh-workspace.exe`，与显式 Windows target 的构建产物 SHA-256 一致。

本轮截图位于 `.qa/apple-connected.png`、`.qa/apple-connected-dark.png`、`.qa/apple-small.png`、`.qa/apple-editor-light.png`、`.qa/apple-editor-dark.png`、`.qa/apple-fingerprint.png`、`.qa/apple-menu.png`、`.qa/apple-container.png`、`.qa/apple-release.png` 和 `.qa/apple-release-dark.png`。本轮未在 macOS 或 Linux 上实际运行。

## 三栏布局调整验证

同日将服务器列表移到右侧，将每个会话的文件和 Docker 服务面板移到左侧，终端保持在中间。DOM 顺序和分隔条的拖动、方向键行为同步调整。

- TypeScript/Vite 生产构建、前端规则测试 4 项和 Windows release 构建通过。
- 独立的 `com.sshworkspace.layoutqa` 桌面窗口中检查空状态、连接状态和三栏边界，1360 × 860 内容视口没有横向溢出；实际 SSH 终端和 SFTP 目录读取正常。
- 资源区初始宽度 320px，按右方向键后为 330px，继续向右拖动 60px 后为 390px。
- 检查 860 × 580 最小内容视口和深色主题，三栏没有重叠或横向溢出。
- 本轮没有修改 Rust、正式数据库或 SSH 会话管理逻辑，也没有重新执行完整桌面回归；Docker 测试服务在手动检查中出现过密钥交换失败，因此不将本轮 Docker 协议检查标记为通过。

截图位于 `.qa/layout-empty.png`、`.qa/layout-connected.png`、`.qa/layout-small.png` 和 `.qa/layout-dark.png`。本轮未在 macOS 或 Linux 上实际运行。

## 本地日志存档、搜索与高亮验证

2026-10-14，Windows 本机，独立的 `com.sshworkspace.logsqa` 数据目录、WebView2 配置目录、CDP 9224 和 SSH 测试服务 22223。Docker 响应来自测试服务，实际执行了 Tauri IPC、SQLite、SSH 传输及原生独立日志窗口；没有连接真实 Docker 服务器。

- TypeScript/Vite 生产构建通过；前端 45 项测试、Rust 19 项常规测试、`cargo fmt --check` 和 Clippy 严格检查通过。百万存档压力测试单独显式执行通过；常规测试中的真实 SSH ignored 场景未在本轮重跑。
- 三项真实桌面回归通过：日志等级、JSON、异常位置和 ANSI 颜色；搜索命中及上下文；时间戳与换行；查看行数及刷新视图不重启 SSH；关闭主 Tab 后继续采集、关闭日志窗口后停止；离线存档仍可搜索和复制；12000 条突发输出全部保存；断线重连、驱动错误和空历史；服务器删除时的活动窗口约束。
- 持久化 1000000 条真实 SQLite 记录，重新打开存档后数量完整；可分页读取最新记录，并从最早的记录中搜索中文长、短关键词。桌面端查看百万条存档时实际渲染 26 行日志 DOM，前端缓存有行数及字节双重上限。
- 原始字节导出覆盖跨包 UTF-8、ANSI、未换行内容及分片；超长行跨片段关键词、大小写及时间筛选均有回归。桌面 IPC 的范围导出与原文复制内容相同。
- 退出并重启测试应用后已保存数量保持一致；内嵌资源的 release 客户端从 `http://tauri.localhost/` 加载，百万存档分页、中文搜索及 Worker 搜索高亮通过。正式标识的 Windows 独立可执行文件已重新构建。

截图为 `.qa/container-logs-light.png`、`.qa/container-logs-dark.png`、`.qa/container-logs-small.png` 和 `.qa/log-million-search.png`。本轮未在 macOS、Linux 或真实 Docker 日志驱动上实测。

## 远程文件树与终端目录同步验证

2026-10-14，Windows 本机，独立的 `com.sshworkspace.treeqa` 数据目录、WebView2 配置目录、CDP 9225 和 SSH/SFTP 测试服务 22225（HOME `/home/ops`）。最终桌面回归使用内嵌页面的 Tauri 调试程序，没有依赖 Vite 热更新；测试观察真实目录请求、树节点和终端输出，不替换 IPC 响应。

- 前端 61 项测试通过，其中 16 项覆盖文件树缓存、路径链懒加载、请求去重、连续定位竞态、手动收起/选择优先、权限祖先、软链接、父缓存过期、刷新删除子树、HOME 和迁移恢复。TypeScript/Vite 生产构建通过。
- Rust 37 项常规测试通过，2 项既有显式协议/压力场景仍标记 ignored；Clippy `-D warnings` 通过。新增目录协议覆盖任意 SSH 分包、UTF-8/特殊路径、BEL/ST、外来消息、异常帧上限、迁移回放和缓冲背压。
- 真实 Git Bash 实测原登录 profile、scalar/array `PROMPT_COMMAND`、退出状态、别名、中文/空格/百分号/分号路径、失败 `cd`；临时启动配置写入失败时回退正常登录配置并清理文件。
- `npm run test:tree` 最终 6 项真实桌面回归全部通过，主窗口和子窗口无前端运行时错误：根到 HOME 的读取范围和缓存；软链接逻辑路径、可穿过但不可列出的祖先；`cd ..`/`~`/`-`/绝对/相对/中文路径及失败/迟到结果；双会话及拆出/移回原连接；树中双击编辑、文件/目录下载的实际字节；未知 shell/探测失败回退普通 SSH shell，以及重新连接恢复跟随。
- 正式标识的 Windows release 可执行文件重新构建成功，输出为 `src-tauri/target/release/ssh-workspace.exe`。桌面回归运行的是对应源码的内嵌调试构建；本轮未在 macOS、Linux、真实 OpenSSH 服务器、zsh 或 fish 环境实测。

自动跟随在 shell 提示符返回时同步，组合命令需等待整条命令结束。Bash 临时 rc 集成的 `login_shell` 标志及 `.bash_logout` 与原生登录 shell 有差异；嵌套 SSH、切换账号及容器目录暂不映射到宿主 SFTP。截图位于 `.qa/file-tree-lazy.png`、`.qa/file-tree-terminal-follow.png`、`.qa/file-tree-migration.png`、`.qa/tree-connected.png`、`.qa/tree-permissions.png` 和 `.qa/tree-editor-and-download.png`。


## Docker 日志自动接续和去重验证

2026-10-14，独立 `com.sshworkspace.logsingestqa` 数据目录、本机 SSH 测试服务 22224、冻结的前端副本 1421。Docker 响应来自 fixture；没有连接真实 Docker 服务器。

- 日志存储回归覆盖跨采集次数匹配、纳秒时间戳差异、不同内容和输出流、原始 ANSI/无效 UTF-8 字节、缺少时间戳的保守保存、拆包、未换行尾部、超长行、跨分片续写、异常退出的暂存恢复、v2 存档索引迁移和历史同步完成标记。
- 前端 60 项测试通过；Rust 全部 37 项常规测试通过，2 项默认忽略。Clippy all-targets `-D warnings`、日志模块格式检查和生产构建通过。
- 两项原生桌面回归通过：工具栏简化、高亮/查找/上下文/自动滚动、关闭主 Tab 后继续保存、关闭日志窗口后停止、离线读取；首次全量保存 12005 条，关闭重开及断线自动重连后仍为 12005 条，驱动错误可重试，停止容器启动后自动接续。
- 显式百万条压力测试通过，真实 SQLite 档案完成持久化、分页和全历史中文搜索；本次 debug 测试约 324 秒，不作为生产性能基准。
- 生产版加载内置 `http://tauri.localhost/` 资源，重启后测试存档共 24040 条保持不变，离线 Worker 搜索高亮正常，12006 条存档实际渲染 26 行 DOM。截图 `.qa/log-ingest-release.png`。
- 旧存档已有重复不自动清理；新采集参与去重。远端轮转删除的内容无法补回，无有效 Docker 时间戳的诊断内容不做去重。

## 单层 SSH 跳板连接验证

2026-10-15，macOS arm64，本机独立 A/B Paramiko 测试服务。A/B 使用不同账号和主机密钥，B 的逻辑域名 `target.internal.invalid:22` 仅由 A 转发到测试目标；SSH 握手、认证、转发、PTY、SFTP 和 SQLite 为真实执行，Docker 命令响应由 fixture 模拟。未连接用户的远端服务器，未重建用户数据库。

- Rust 常规测试 47 项通过；5 项需 fixture/Agent 的跳板测试另行显式执行并全部通过。原有百万日志压力测试本轮未重跑。
- 真实传输覆盖直连回归、A/B 分别认证、5 MiB 双向 SFTP 字节校验、Docker 查询及容器 shell 回退、终端目录同步、输出冻结与迁移回放、关闭终端后独立连接继续使用、A/B 指纹变化、A/B 密码错误、禁止转发、A 断线与握手/第二跳确认期间取消。测试确认跳板打开和关闭计数最终一致。
- 密码、加密私钥和临时 SSH Agent 的双跳认证通过。补测发现并修复了非阻塞 Agent 辅助对象在 EAGAIN 重试间释放导致的崩溃；当前显式保持 Agent 与密钥列表直到认证结束。测试代理独立创建并自动退出，不使用或清理用户原有 Agent。
- 后台下载实测包含多任务排队、传输中取消、临时文件清理、重试及 8 MiB 文件字节比对；日志实测包含历史存档、stdout/stderr、持续采集和停止收尾。并发回归发现并修复了存档已登记、远端通道未打开时取消留下未结束 run 的问题。
- Chromium IPC 模拟交互 4 项通过：下一跳提示早于回复确认返回、连接 ID 返回前取消并忽略迟到 Ready、已保存密码保留/替换/明确清空、跳板候选筛选与配置保存。它们验证前端时序，不代表原生 Tauri IPC 或跨窗口拖动实测。
- 前端 61 项规则测试、TypeScript/Vite 生产构建、Rust 格式检查及 Clippy all-targets `-D warnings` 通过。macOS 调试 `.app` 构建通过，包含内置前端资源。
- 数据库测试覆盖跳板双向约束、被引用时禁止删除、网络 scope 指纹隔离、不返回密码的 DTO、显式密码操作，以及旧跳板结构缺失时仅经明确备份重建恢复；不执行旧数据迁移。

本轮没有运行 Windows WebView2/Linux 原生桌面测试，也没有对真实 OpenSSH 服务或真实 Docker 引擎实测。既有 Windows 桌面用例已更新密码接口和认证重试预期，仅验证可发现/解析；Windows 回环 socket 配对实现仍需该平台运行验证。详细命令见 README 的“跳板连接回归”。

## macOS 数据库重建确认修复

2026-10-15，macOS arm64。将“备份旧库并重建”的浏览器 `window.confirm()` 替换为应用内模态确认框；项目当前 Wry 的 macOS delegate 未实现 JavaScript confirm 回调，WKWebView 对这种情况默认按取消处理。确认框展示实际数据库路径、清空范围及备份失败保护，失败保留弹窗供重试。

- `npm run test:database`：Chromium/WebKit 共 8 项通过，模拟 Tauri IPC 并让 `window.confirm()` 抛错，覆盖取消/Esc 不调用重建、执行中禁止取消与重复提交、失败重试、成功展示备份路径，以及刷新列表失败仍保留备份路径。
- `cargo test --manifest-path src-tauri/Cargo.toml db::tests`：10 项真实 SQLite 测试通过，包括旧库备份内容、备份失败不修改原库、重建后重新打开及正常结构拒绝重建。
- 前端 61 项测试及 TypeScript/Vite 生产构建通过。前端交互测试使用浏览器 WebKit，不等同于原生 Tauri 窗口端到端测试；本轮未重建用户实际数据库。
- `npm run tauri -- build --debug --bundles app` 成功，更新了包含内置页面的 `src-tauri/target/debug/bundle/macos/SSH Workspace.app`，需退出已运行的旧进程后重新打开。

## 跳板连接界面设计复核

2026-10-15，使用已安装的 apple-design skill 整理配置、认证与连接状态界面。配置采用分组表单、直连/跳板分段选择和路径预览；操作栏固定，保存失败原因不随内容滚走；密码保存/清空/替换有明确状态。认证弹窗标明服务器角色与跳点，指纹变化显示新旧记录，等待 IPC 回复仍能取消。放弃未保存配置改用窗口内确认，不依赖系统 confirm。

- `npm run test:connection`：Chromium/WebKit 共 14 项通过，包含原有连接时序、密码操作，以及键盘方向键切换、跳板选择恢复、初始焦点、Esc、认证回复等待期间取消、无系统确认框时继续编辑/放弃修改。
- 实际截图检查 1440×950 深浅色及 860×580 小窗口，保存按钮保持可见，长名称和指纹可换行，无弹窗横向溢出；同时以减少动态效果和高对比度配置运行。WebKit 下统一了 select 高度，表单使用实色背景，避免背景文字透出干扰阅读。
- 前端规则测试 61 项通过；既有数据库弹窗 Chromium/WebKit 回归 8 项通过；TypeScript/Vite 及包含内置前端的 macOS 调试 `.app` 构建通过。
- 界面测试使用浏览器中的 IPC 模拟，不等同于原生 Tauri 窗口端到端测试。截图位于 `.qa/jump-design-editor-{light,dark,small}.png`、`.qa/jump-design-fingerprint-{light,dark}.png` 与 `.qa/jump-design-login-light.png`。

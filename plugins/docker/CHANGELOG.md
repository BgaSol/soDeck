# 0.11.1

- 容器详情页签栏沿用公共工具底色和当前项底色，多巴胺使用绿底与黄色选中项；名称和数量分开排版，晨曦与星夜使用各自主题角色。
- 统一操作栏、字段、更多信息和结果区的间距、描边、对齐与交互反馈；200/220px 极窄侧栏将页签排成两行，保持入口和数量可读。
- 修正树内通用按钮样式优先级，详情按钮按组件尺寸呈现；不改变请求、导航、确认目标或端口生命周期，未新增依赖。

包来源：[033f478d851927f801829054646774777b871794](https://github.com/BgaSol/soDeck/commit/033f478d851927f801829054646774777b871794)；同一来源的 [Check](https://github.com/BgaSol/soDeck/actions/runs/45408889308) 全部选中任务通过，包含五平台 Docker 源码、格式/脚本及真实 OpenSSH/隔离 Engine 业务与 Docker UI/进程回归。核心完整浏览器、桌面和 netem 未选中，不计为通过。

本机类型/格式、216 项工程脚本、35 项 Docker Rust、完整 Docker UI 与本地独立打包通过。详情覆盖三主题和 200/220/280/320/400/900px、四页签与兄弟行测量、键盘、稳定操作/确认 DOM、端口订阅、冻结及候选重挂载导航。绿栏文字对比度 7.53:1、当前项文字 12.29:1、绿栏焦点 3.50:1；机械检查仅提示原有 14px 标题字号，不改无关区域。

发行：[Docker 0.11.1](https://github.com/BgaSol/soDeck/releases/tag/plugin-docker-v0.11.1)，按用户要求设为仓库 Latest。[原始独立打包](https://github.com/BgaSol/soDeck/actions/runs/45408947876) 的 Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 原生构建、架构/资源、包内挂载/释放与退出全部通过；本轮没有失败重试。Windows x86_64 的同一 CI 原归档另在本机通过包内程序验收。

直接发布同一草稿的五份程序归档，六个公开附件匿名大小与 SHA-256 核对，更新清单 notes 与 Release 一致。[永久证据](release-evidence/v0.11.1-assets.json) 保存固定源码、原始运行、实际包报告、检查结果与公开摘要。更新已启用插件后按提示重启生效，仍需核心 2.3.0。

本轮未重跑原生 WebView 人工体验、物理 IME/DPI/读屏、正式旧版线上升级、netem 或全部 Windows Server/ARM64 远端 Engine 矩阵；实际包启动不代替这些专项。

# 0.11.0

- 容器树内详情采用概览、端口、挂载和网络四个页签，固定操作栏与结果/确认区域；保留三主题与紧凑布局，窄侧栏自动换行。
- 普通打开默认概览，端口菜单直达端口；页签及实际分页游标随 View 导航保存。切页保留按钮和确认目标，离开端口退订展示状态，原隧道请求仍可查询。
- 将对象/集合读取与呈现分离，只投影当前页签集合，同一观察复用当前页缓存；旧对象、范围和观察的迟到结果不能覆盖新页面。
- 修复集合前进 20 条、返回仅减 5 条的问题：每页最多 5 条，返回记录的实际起点，兼容字节预算短页、数据减少及重挂载。
- 删除旧详情模块并同步包内资源检查；镜像、网络、卷、日志、终端和隧道沿用原业务边界，未新增生产依赖或核心 API。仍需核心 2.3.0。

包来源：[cccbb99c9b3246ab8f8ba4c1e3631d46aa066f85](https://github.com/BgaSol/soDeck/commit/cccbb99c9b3246ab8f8ba4c1e3631d46aa066f85)；同一来源的 [Check](https://github.com/BgaSol/soDeck/actions/runs/45395237974) 全部选中任务通过，覆盖五平台核心/插件源码、类型/格式/脚本、完整浏览器和桌面回归、网络及真实 OpenSSH/Engine 业务，netem 未选中。

本机 216 项工程脚本、35 项 Docker Rust、四页签三主题及 280/320/400/900px 浏览器回归通过；覆盖按需请求、缓存、实际游标短页/返回、数据减少、迟到响应、确认 DOM、只读候选、冻结与重挂载导航。隔离 Engine 24.0.9 的资源维护、终端、端口隧道字节与来源关闭/释放通过；Windows 原生 WebView2 的页签/键盘、稳定 DOM、60 秒固定日志阅读、查找/回到最新、20 次开关回收及退出通过。

前一次 Engine 联调在停止后等待分类刷新超时，未稳定复现，原样重跑通过；首次原生验证误用依赖开发服务器的 debug 核心，改用已有 release 核心后通过，未放宽断言。包内启动与真实 WebView/Engine 业务分别验收；本轮不代表新增 Windows Server Engine、ARM64 远端、物理 IME/DPI/读屏、netem 或正式旧版线上升级已验。

发行：[Docker 0.11.0](https://github.com/BgaSol/soDeck/releases/tag/plugin-docker-v0.11.0)，按用户要求设为仓库 Latest。[原始独立打包](https://github.com/BgaSol/soDeck/actions/runs/45395272728) 的 Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 原生构建、架构/资源、包内挂载/释放与退出全部通过；Windows x86_64 的同一 CI 原归档另在本机通过启动和退出。macOS 首次因 GitHub hosted runner 容量不足未执行，仅在原运行重试失败任务，保留四个成功平台的包；第二次及后续五平台 verify 全部通过。

直接发布同一草稿的五份程序归档，六个公开附件匿名大小与 SHA-256 全部核对，更新清单 notes 与 Release 一致。[永久证据](release-evidence/v0.11.0-assets.json) 保存固定源码、原始运行、五份实际包报告、Check 和公开附件摘要。当前实现行为见 [Docker 功能](../../docs/features/docker.md#容器页签详情)；更新已启用插件后按提示重启生效。

# 0.10.0

- 日志会话、原字节解析、有界窗口、查找/历史搜索及冻结导出统一由插件进程持有；重挂载/移窗只读取投影，不重开命令。需要核心 2.3.0，先升级核心并重启。
- 最新尾部不限制最近 15 分钟；长时间无输出仍显示最后内容。固定阅读保持选区与位置，新增计数独立于近期保留窗口。
- 依据实际记录时间补读更早历史，核对纳秒边界与真实重复行；提供时间定位、最近启动范围、指定时段搜索与上下文，明确部分覆盖及未完成搜索。
- 移除日志缩略和相关设置，集中使用补读、时间定位和匹配导航；详情和导出分块保留原文，旧目标由核心保存事务保护。
- 读取恢复清除暂时错误，重挂载恢复所选时段与查找；正文、状态和详情串行查询，卸载后的排队读取不发往新页面。
- 历史进程及时释放，记录由 View 会话有界持有；观察失败不伪造进程完成，旧任务与源身份分开校验。未新增生产依赖。

发行：[Docker 0.10.0](https://github.com/BgaSol/soDeck/releases/tag/plugin-docker-v0.10.0)，保持独立非 Latest；核心 2.3.0 为默认发行入口。包来源 [cf3aaae](https://github.com/BgaSol/soDeck/commit/cf3aaae8e8e0392a314d322fe5ba012eb5059b34)；[原始独立打包](https://github.com/BgaSol/soDeck/actions/runs/45377730480)的 Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 原生构建、架构/资源、实际包挂载/释放及退出全部通过。直接发布同一草稿，六个公开附件匿名大小与 SHA-256 全部核对，[永久证据](release-evidence/v0.10.0-assets.json)保存原包报告和公开摘要。

本机 Rust 34 项、207 项工程脚本、POSIX/Windows 执行环境和 Linux/Windows Engine 进程夹具、真实插件会话、三主题浏览器、隔离 OpenSSH/Engine 24.0.9 百万行尾部及维护/终端/隧道回归通过。Windows 原生 WebView/CSP/正文查询、60 秒固定阅读与查找/回到最新、20 次回收及退出通过；单次首正文至绘制约 4.8 ms，核算数据约 9.1 MiB。读取暂时失败恢复、历史时段重挂载、并发查询上限和迟到绘制均补回归；样本及早期失败见[本机记录](../../docs/releases/v2.3.0-local.json)。

包来源的五平台源码及普通业务通过；首轮总 UI 在设置夹具缺少日志入口时失败，修正夹具与旧缩略开关断言后，生产源码不变的完整 Linux UI 和脚本补验通过，详见[检查收据](../../docs/releases/v2.3.0-checks.json)。本轮不代表其他系统日志 WebView/设备、全部远端 Engine/驱动及生命周期组合、8 MiB/s 端到端压力、30 次分位数或一小时无调试连接内存已验；netem 与其余[既有专项](../../docs/plans/docker-log-viewer.md)仍未完成。

# 0.9.0

- 迁移 API 2.0 统一 SDK，日志及拉取使用通用 process，删除旧 session.stream 控制路径和页面桥。
- 保留资源浏览和管理、日志原字节恢复、拉取与终端/隧道协作；远端退出和资源释放分别确认。
- 插件包携带自身 SVG 图标和 SDK 资源，独立分发；需要核心 API 2.0，API 1 核心无法加载。

发行：[docker 0.9.0](https://github.com/BgaSol/soDeck/releases/tag/plugin-docker-v0.9.0)，保持非 Latest；核心 2.0.0 为仓库默认入口。包来源 [e973109dcfc21d1f55a4ab1ba62245cf7406b0f4](https://github.com/BgaSol/soDeck/commit/e973109dcfc21d1f55a4ab1ba62245cf7406b0f4)；[原始独立打包](https://github.com/BgaSol/soDeck/actions/runs/45009316241)的 Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 原生构建、架构/资源、包内挂载/释放与退出均通过。直接发布同一草稿，六个公开附件匿名下载大小和 SHA-256 全部核对。

[源码及业务检查](https://github.com/BgaSol/soDeck/actions/runs/45010797487)通过；类型/格式、211 项脚本、核心 UI 和六插件回归、本地终端输入/十万行历史、文件编辑、状态图表、三插件协作、真实 SSH/Docker Engine/MCP pipe/PTY 与 1 GiB 上传覆盖受影响链路。本组件未参与终端/文件/网络的五平台原生远程组合，包内进程验收与真实远端/Engine 业务分别记录。

旧版 0.8.0 → 0.9.0 的[原始五平台升级](https://github.com/BgaSol/soDeck/actions/runs/45013665152)中 Linux 两架构与 macOS 通过；Windows 安装夹具假定临时目录导致安装后找不到程序，修正为明确当前用户安装并核对登记后，[x64](https://github.com/BgaSol/soDeck/actions/runs/45014582403)与[ARM64](https://github.com/BgaSol/soDeck/actions/runs/45014588439)补测通过。五平台确认旧 API 程序不执行、登记/启用意图/设置和数据标记保留，下载后切换到新版本并运行。先升级核心并重启，再更新插件并按提示重启。

[永久证据](release-evidence/v0.9.0-assets.json)保留包来源、五份原包报告、公开附件摘要及逐平台升级来源。netem、物理 IME/DPI/读屏、已认证 Codex CLI、全部远端系统/Engine 组合未执行；深度编辑器等既有专项继续单独维护。

# 0.8.0

- 资源树采用单层展开：同时查看一个对象，容器的操作、镜像、实际端口、挂载和网络就地显示；镜像、网络、卷展示必要摘要与引用，引用定位实际资源。
- 取消容器和通用资源详情页，Docker 仅日志可打开中央页面；容器终端继续交给终端插件。连接设置、公开镜像拉取、维护确认和任务结果全部留在树内。
- 统一有界对象观察和集合分页，固定资源行加单个实测行内区域，保留滚动、输入与焦点。端口隧道从树内办理，切换选择后原请求仍属于原容器；复用既有组件通信与网络规则生命周期。
- 精简信息范围，不再提供环境变量整表、全部标签及完整 inspect 字段浏览；完整 ID、运行时间、重启策略和手动指标按需查看。
- 日志独立入口保留原正文／Worker／导出流程；修复保持阅读位置时，窗口变宽后剩余正文不足一屏导致的底部滚动限制造成的锚点偏移。
- 无新增生产依赖、核心 API 或网络插件版本。本版独立发布的来源、五平台实际包和验证／跳过范围见下方交付记录。
包来源：[1fc1eb40123caecf0b9b6d459da0db1b6a0bb001](https://github.com/BgaSol/soDeck/commit/1fc1eb40123caecf0b9b6d459da0db1b6a0bb001)；[最终来源 Check](https://github.com/BgaSol/soDeck/actions/runs/44906968463)；[修正后 Docker UI 复验](https://github.com/BgaSol/soDeck/actions/runs/44907742085)；[原始独立打包与五平台包验收](https://github.com/BgaSol/soDeck/actions/runs/44906932529)；[正式 Release](https://github.com/BgaSol/soDeck/releases/tag/plugin-docker-v0.8.0)。

Windows x86_64／ARM64、Linux x86_64／ARM64、macOS ARM64 五平台原生归档、包内程序初始化、生命周期和退出均通过；最终 Check 的源码、浏览器、桌面、网络及 Docker 集成通过，netem 按专项范围跳过。修正后复验只等待日志缩略导航投影完成，产品代码和发行包来源保持不变。真实 Linux Engine 24.0.9／OpenSSH 完整流程通过；Windows Docker Desktop CLI/Engine 29.9.2 的信息、隧道 HTTP 数据、终端和维护流程通过，公开 Docker Hub 拉取因 TLS handshake timeout 未计为通过，直接 docker pull 复现相同外部故障。核心版本保持 1.11.0，仓库 Latest 保留给核心。

公开匿名更新清单及五份归档的大小与 SHA-256 已核对一致；固定附件、来源、平台报告和跳过范围保存在[本版交付证据](release-evidence/v0.8.0-assets.json)。真实 IPv6／指定非回环网卡、rootless、Windows Server Windows Engine、原生可访问性／IME／混合 DPI 和跨窗设备交互仍由[专项计划](../../docs/plans/docker-port-tunnels.md)与[原生验收计划](../../docs/plans/native-verification.md)维护。

# 0.7.0

- 容器端口分区支持一键创建或复用 TCP 隧道，自动填写实际发布目标、使用本机回环随机端口；运行后复制入口或定位网络隧道规则。
- 通过既有核心命令、共享状态与网络插件后台协作，关闭 Docker 来源后隧道继续运行；提交前核对 Engine、容器和端口绑定，结果未知不自动补建。
- 端口与协作任务使用明确类型，容器终端沿用同一 Ticket 办理路径；仅支持运行中容器实际发布的 TCP 端口。

源码验收覆盖端口推导、旧绑定拒绝、三种模拟远端执行环境、原容器终端、端口 UI 三主题与窄屏，以及真实核心／OpenSSH／隔离 Linux Engine 的 HTTP 字节、复用、来源关闭与端口释放。Windows OpenSSH + Docker Desktop（CLI/Engine 29.9.2）已通过同一真实流程；Windows Engine、rootless、真实 IPv6 发布入口与原生设备交互未由这些结果替代；本版独立归档的来源及平台验收见下方交付记录。

包来源：[57e41f1e013cb4f0fa5f2cdd607c531115d00edc](https://github.com/BgaSol/soDeck/commit/57e41f1e013cb4f0fa5f2cdd607c531115d00edc)；[来源 Check](https://github.com/BgaSol/soDeck/actions/runs/44875135645)；[测试器修复后复验](https://github.com/BgaSol/soDeck/actions/runs/44876235305)；[原始独立打包与实际包验收](https://github.com/BgaSol/soDeck/actions/runs/44875173595)；[正式 Release](https://github.com/BgaSol/soDeck/releases/tag/plugin-docker-v0.7.0)。

Windows x86_64／ARM64、Linux x86_64／ARM64、macOS ARM64 五平台原生归档、架构／资源和包内程序初始化、生命周期、退出均通过；网络归档另验证协作命令、真实监听及清理。来源 Check 的五平台源码、脚本、浏览器、Windows 桌面／SSH、两种 Linux 架构网络及真实 Docker 联通通过；末尾 Docker UI 测试存在异步断言竞态。测试提交 ad0ea8f 仅修复测试等待，两个插件及共享运行／打包输入保持原树，修复后的 Docker UI、进程与真实业务复验通过。原始失败和复验任务均保留在交付证据中；netem 未运行。核心版本保持 1.11.0，仓库 Latest 保留给核心。

公开匿名下载的更新清单及五份归档，其大小与 SHA-256 均已核对一致；保留原始打包附件，未重建或覆盖程序包。固定附件 ID、大小、SHA-256、来源任务及验证／跳过范围保存在[本版交付证据](release-evidence/v0.7.0-assets.json)。真实 IPv6／指定非回环网卡、rootless、Windows Server Windows Engine 及原生设备交互的剩余范围见[专项记录](../../docs/plans/docker-port-tunnels.md)。

# 0.6.0

- Windows 远端通过当前 SSH 账号的原生 Docker CLI 访问本机命名管道；自动识别当前 Context/环境覆盖，手动设置端点及远端 CLI 路径。旧端点配置迁移为手动方式并保留备份。
- 固定不可变端点/Engine 目标；列表、维护、日志、拉取和容器终端统一提交程序与参数，平台 shell 编码由核心提供。Engine 切换后要求重新识别，未知操作不自动重发。
- 区分 Client 和 Engine 系统，保留实际 Docker Root；Windows Engine 默认 cmd.exe，保护系统网络并隐藏暂停/恢复。通用资源删除与容器身份分开，慢速索引读完后再安排自动刷新。
- 需要核心 1.8.1 的 session.program.v1；旧核心明确提示升级。无新增生产依赖。

包来源：[94ca2cad441016f80f066be731e9031641e2c407](https://github.com/BgaSol/soDeck/commit/94ca2cad441016f80f066be731e9031641e2c407)；[来源完整 Check](https://github.com/BgaSol/soDeck/actions/runs/44514505907)；[原始独立打包与实际包验收](https://github.com/BgaSol/soDeck/actions/runs/44514526683)。全部选中源码、脚本、浏览器、Windows 桌面与 SSH/真实业务检查通过，netem skipped。

Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 五平台原生归档、架构与资源、包内程序握手、视图生命周期及退出均通过。核心与插件独立打包，本插件保留同一原始 run 的五份程序归档，配套[核心 1.8.1](../../docs/releases/v1.8.1.md)。

发行：[Docker 0.6.0](https://github.com/BgaSol/soDeck/releases/tag/plugin-docker-v0.6.0)，按用户要求设为 GitHub Latest。草稿阶段只同步 Release 说明和更新清单 notes，不重建或覆盖程序归档。[永久发行证据](release-evidence/v0.6.0-assets.json)保存五份原生报告、来源、附件摘要及匿名公开下载核验。

本机通过 17 项 Docker 单元测试、POSIX/Linux、Windows/Linux、Windows/Windows 的实际插件进程模拟回归，以及完整界面/连接表单回归。进程测试包含发现期间新开详情导致连接提交失败并保留旧配置，防止候选替换在途目标。

真实回环 OpenSSH/Docker Desktop 4.96.3（原生 Windows amd64 CLI 29.9.2，WSL2 Linux amd64 Engine 29.9.2，desktop-linux 的 npipe）完整流程通过：四分类、详情/指标、中文历史日志、真实 PTY、关闭来源后终端独立运行、容器维护、卷/网络删除、多标签镜像拒绝、公开镜像拉取与断线日志保留。原核心候选联调 377.48 秒、最终核心 1.8.1 联调 394.95 秒均通过。Linux 隔离 Engine 24.0.9/OpenSSH 的同一流程通过，自定义 Root 和挂载卷保持。

早期 Windows 联调曾在 7 秒初始化等待时超时，之后没有稳定复现；夹具补充失败快照与日志，未放宽等待时间或绕过断言。

Windows Server Engine、ARM64 Docker 远端及全部版本/rootless/生产环境矩阵尚未真实验收；对应边界与剩余要求见[功能](../../docs/features/docker.md)和[待验收计划](../../docs/plans/docker-windows.md)。netem、正式旧版升级和新增 IME/DPI/读屏专项未执行。

# 0.5.4

- 资源树区分主动操作和后台/滚动刷新；资源菜单复用锚点定位和统一关闭。日志详情所有关闭入口衔接实际宽度，复制提供文字确认；状态文字不再反复淡化。
- 复用现有 DOM、CSS 与 WAAPI，未新增生产依赖或核心 API。

包来源：[d88a62f05a841cde078dfa38e8f6ebb55303b7a5](https://github.com/BgaSol/soDeck/commit/d88a62f05a841cde078dfa38e8f6ebb55303b7a5)；[来源完整 Check](https://github.com/BgaSol/soDeck/actions/runs/43914640450)；[原始独立打包与实际包验收](https://github.com/BgaSol/soDeck/actions/runs/43914675724)。同一最终提交完整 Check（all，不含 netem）通过，覆盖五平台核心/插件源码、工程脚本、Linux 浏览器、Windows 桌面及 SSH、真实 SFTP/PTY、网络、系统状态、Docker 和组件通信。

Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 五平台原生独立归档、架构/资源及包内程序握手、生命周期与退出通过。

发行：[docker 0.5.4](https://github.com/BgaSol/soDeck/releases/tag/plugin-docker-v0.5.4)，非仓库 Latest。发布原始 run 的同一草稿归档，不重建或覆盖正式版本程序包；更新清单 notes 与 Release 说明一致。[永久发行证据](https://github.com/BgaSol/soDeck/blob/main/plugins/docker/release-evidence/v0.5.4-assets.json)保留来源、原生报告、附件摘要和匿名公开下载核验。

netem、正式旧版到本版的线上升级、原生文件剪贴板/默认应用/深度编辑器专项，以及新增真实 IME、DPI、读屏、触控板和多屏设备验证未执行。浏览器中间帧与原生业务通过不等于五平台人工逐帧体验验收。 Docker 未重做全部真实 Engine/远端系统矩阵，不属于原生远程包组合入口。 GitHub hosted Runner 容量不足导致任务未启动时，只重试失败平台及后续汇总，保留其他成功产物；具体次数见发行证据。

# 0.5.2

- 资源树分支与资源状态有短暂反馈，日志详情面板使用实际宽度展开和收起，保留既有阅读锚点处理。日志正文、操作票据、结果待确认与候选页面输入权不受动效改变。
- 统一控件和浮层节奏，减少动态效果与隐藏页面停止效果，未新增生产依赖。

包来源：[33f001452488c7d57f8014a98fee587599a46478](https://github.com/BgaSol/soDeck/commit/33f001452488c7d57f8014a98fee587599a46478)；[来源 Check](https://github.com/BgaSol/soDeck/actions/runs/43847374906)；[原始独立打包与实际包验收](https://github.com/BgaSol/soDeck/actions/runs/43847504901)。

Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 五平台原生归档、架构/资源、包内程序握手、生命周期与退出均通过；来源 Check 的五平台源码及相关业务和 UI 回归通过。

发行：[docker 0.5.2](https://github.com/BgaSol/soDeck/releases/tag/plugin-docker-v0.5.2)。使用上述原始 run 的同一草稿归档，不重建或覆盖正式版本的程序包；更新清单的 notes 与 Release 说明保持一致。永久来源、原生报告、附件摘要和公开下载核验见[发行证据](release-evidence/v0.5.2-assets.json)。

netem、旧版到本版的线上升级、原生文件剪贴板/默认应用/深度编辑器专项，以及新增真实 IME、DPI、读屏、触控板、多屏设备验证未执行，不计为通过。 Docker 未重做全部真实 Engine/远端系统矩阵；不属于原生远程包组合入口。

macOS ARM64 的实际包验收首次因 GitHub hosted runner 容量不足未启动，只重试失败任务和后续汇总，原始 run 与成功平台保留；不是代码或业务测试失败。

# 0.5.1

- 侧栏只保留宿主 Docker 标题，连接状态与 Engine 信息分层显示；全部折叠收进“更多”，菜单关闭后返回触发入口。
- 分类和资源关系的加载状态并入对应行，后台更新保留资源与数量；移除“正在读取”假资源行，区分空结果、无匹配与失败重试。

本机 Windows x86_64 已通过格式/类型、12 项 Docker Rust 测试、183 项工程脚本和完整 Docker UI 回归。覆盖三主题与 200/320/400/900px、分类/关系加载、已有资源保留、无假资源行、空结果、无匹配、失败重试、菜单和焦点恢复及减少动画。独立完成复审检查了最终源码和 15 张实际截图，没有需要追加修复的问题。

包来源：[d94d425a546de5cb2acea23886d290253b2939b5](https://github.com/BgaSol/soDeck/commit/d94d425a546de5cb2acea23886d290253b2939b5)；原始独立打包/验包：[43819344737](https://github.com/BgaSol/soDeck/actions/runs/43819344737)。Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 五平台的原生构建、归档来源/大小/SHA-256、原生架构、UI 资源、初始化、视图创建/释放与包内程序退出全部通过。Windows x86_64 的同一 CI 原归档另在本机通过实际包验收。

[包来源 Check 43818403311](https://github.com/BgaSol/soDeck/actions/runs/43818403311) 首次运行的全部选中任务通过，包含五平台核心/插件源码、格式/类型/脚本、完整浏览器与正式前端、Windows SSH、普通业务，以及隔离 Engine/OpenSSH 的真实 Docker 联调和 Docker UI/进程回归。netem skipped；未选中的 Windows 完整 UI 对照和旧版升级专项未执行，不计为通过。

发行：[Docker 0.5.1](https://github.com/BgaSol/soDeck/releases/tag/plugin-docker-v0.5.1)，非 Latest；同期核心 1.2.3 为仓库 Latest。五份程序归档沿用上述原始 run，草稿阶段只同步说明与更新清单 notes。六个公开附件的实际大小、SHA-256 和五平台更新地址均核对一致，来源及摘要留在[本轮附件记录](release-evidence/v0.5.1-assets.json)。

未新增生产依赖或改变核心 API 主版本；启用中的升级在重启后生效。未新增目标应用 IME/DPI/读屏、rootless、生产服务器和全部 Engine 版本专项，仍以[设备验收](../../docs/plans/native-verification.md)与[远端验收](../../docs/plans/docker-windows.md)为准。

# 0.5.0

- Docker 侧栏改为当前 Engine 的四类资源树，包含 Compose 折叠组、独立容器、镜像仓库/标签、网络和卷；Docker Root 展示实际远端数据目录，支持完整路径查看与复制。
- 按需读取容器与镜像/网络/卷的关系，引用定位实际对象；支持对象菜单、键盘、搜索恢复、状态/健康筛选和稳定分页刷新。容器详情、日志及终端沿用现有工作区。
- 增加公开镜像拉取、任务结果和原始输出。镜像/网络/卷删除前重新核对身份与完整引用，包含停止容器；系统网络禁止删除，多标签镜像保留 Docker 原生拒绝。取消或连接中断不会自动重发未知操作。
- 索引、关系、UI 与全部 View 缓存有界；连接切换拒绝旧响应，独立拉取期间禁止修改 endpoint。日志工作区沿用原有实现；回归测量在同一次 DOM 读取中定位当前行，避免重绘替换后测量已经脱离页面的旧节点。

本机 Windows x86_64 的格式/类型、183 项工程脚本、10 项 Docker Rust 测试、核心 Docker 夹具编译、进程、完整 UI 和正式前端产物回归通过。资源树覆盖三主题及 200/320/400/900 px、700 行分页、只读候选、键盘/菜单/固定目标确认、搜索恢复、Root 长路径与缺失字段、拉取去重和资源回收；进程回归覆盖停止容器引用、同名卷重建、系统网络、多标签镜像原生拒绝、拉取并发及 endpoint 约束。日志重排另验证 30 次宽度切换，保持原偏移标准。

包来源：[a5aab175233e428c874966f466966796239bfd4a](https://github.com/BgaSol/soDeck/commit/a5aab175233e428c874966f466966796239bfd4a)；原始独立打包/验包：[43797146613](https://github.com/BgaSol/soDeck/actions/runs/43797146613)。Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 五平台使用原生归档，来源、大小、SHA-256、原生架构、UI/DOM 资源、初始化、视图创建/释放与包内程序退出全部通过；六个草稿附件与原包逐字节核对一致。Windows x86_64 的 CI 原归档另在本机通过实际包验收。

[包来源 Check](https://github.com/BgaSol/soDeck/actions/runs/43797144813) 全部选中检查通过，包含格式/类型/脚本、Linux x86_64 核心源码、完整浏览器、Docker 进程/UI 与隔离 OpenSSH/Engine 24.0.9 联调。真实联调覆盖自定义 data-root、四分类、无引用卷/网络删除、多标签镜像拒绝、独立 Registry 实际拉取、既有容器维护/日志/指标/终端及挂载卷保留。Windows desktop 与 netem 本轮 skipped，不计为通过。

首轮 [43795744398](https://github.com/BgaSol/soDeck/actions/runs/43795744398) 的五平台 Docker 源码测试通过，浏览器与真实 Docker 夹具因旧介绍断言、关闭重开后等待尚未加载分类而失败；夹具按新行为修正后，在上述包来源 Check 通过。本机日志测量曾抓取到已经脱离 DOM、矩形为零的旧行，已改为同次读取当前节点；缩略取消等待实际恢复完成，未放宽偏移断言或改动日志业务实现。

发行：[Docker 0.5.0](https://github.com/BgaSol/soDeck/releases/tag/plugin-docker-v0.5.0)，按用户要求设为 GitHub Latest；沿用原始 run 的五份程序归档，只同步发行说明和更新清单 notes。未新增生产依赖或变更核心 API 主版本，继续要求核心 1.1.0 的通用命令流/内容导出能力；启用中的插件升级在重启后生效。

尚未完成新资源树的目标应用真实 IME/DPI/读屏、多屏、rootless、生产服务器和全部 Engine 版本专项，见[设备验收](../../docs/plans/native-verification.md)与[远端验收](../../docs/plans/docker-windows.md)。卷无不可变唯一 ID 或原子比较删除，外部同名重建的并发风险无法完全消除，删除会永久移除数据。

# 0.4.1

- 容器日志改为连续工作台：跟随最新、固定阅读同时后台接收、按时间补读、搜索/筛选/上下文/详情、右侧缩略与原文导出。停止很久的容器默认显示最后保留尾部；长时日志采用有限预览与滚动窗口，不一次下载全部历史。
- Docker保留命令/解析/查询/UI，核心提供通用命令流原字节捕获与本机保存；新增要求session.stream.v1和content.export.v1。旧日志分页与行数设置移除，容器维护与终端入口保持原操作。
- 固定阅读统一按实际可见记录与行内偏移恢复，普通重绘也在完成测量后校准；Worker 请求等待期间不先滚动旧正文。4000 条折行记录的测量差异回归在旧版稳定偏移 96px，新版 40 轮接收重绘、搜索、缩略、移窗和三主题检查通过。

本机格式/类型、工程脚本、Docker源码/进程、三主题四宽度UI及真实OpenSSH/隔离Engine24.0.9联调通过；百万行容器默认只返回最后2001条给Worker，UI预览限制为2000条/2MiB。连续接收、固定阅读、缩略取消、原捕获恢复、维护与终端共存回归通过。

固定阅读在提示条/正文高度变化时保留折行测量，宽度或系统缩放改变时保留实际可见记录及偏移；显示、筛选、更多菜单互斥打开。Windows WebView2、真实核心Channel、Docker归档、隔离OpenSSH/Engine24.0.9的持续接收、固定阅读、隐藏恢复和20万条完整历史搜索已通过短测。微软拼音真实候选、中文搜索、F3/Shift+F3，以及系统保存框取消/重试和中文空格路径原文字节对照已实测。100%/125%/150%/200%实际系统缩放的固定记录最大偏移0.67px，已恢复原150%。

0.4.1 最终包来源为 [5002491b7bd17dbc3346d540a6648998a556c755](https://github.com/BgaSol/soDeck/commit/5002491b7bd17dbc3346d540a6648998a556c755)，原始独立打包/验包为 [43668647795](https://github.com/BgaSol/soDeck/actions/runs/43668647795)。Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 五平台原生归档的来源、摘要、架构、资源、初始化和退出全部通过；六个发行附件与原包大小、SHA-256 核对一致。

[来源 Check](https://github.com/BgaSol/soDeck/actions/runs/43668621670) 的全部选中源码、脚本、UI 和 Docker 普通业务通过，其他组件 UI/desktop 和 netem skipped。最终 CI Windows 原归档配合核心原 NSIS 解出程序的实际 WebView2、IME/F3、系统保存框取消/重试和 4436559 字节冻结原文对照通过；30 次近期/20 万条搜索 P95 为 182.7/334.4ms，20 次日志打开/固定/关闭的 Surface、Worker、捕获目录回收和退出通过。核心能力、设备条件及早期失败统一保留在[1.1.0 记录](../../docs/releases/v1.1.0.md)。未采用的 0.4.0 候选来源为 e7745d9，原始打包 [43663678732](https://github.com/BgaSol/soDeck/actions/runs/43663678732)，缺少本次重绘修复，不发布。

发行：[Docker 0.4.1](https://github.com/BgaSol/soDeck/releases/tag/plugin-docker-v0.4.1)，非 Latest，需配套核心 1.1.0；沿用上述五份原始归档，草稿阶段只同步说明及更新清单 notes。Windows 一小时持续接收 746.3 MiB、约 23.32 轮容量、175 次固定位置采样最大偏移 0px，以及 100 次实际开/固定/关与退出通过。耐久使用实际核心与本机构建的 Docker，UI 与 CI 仅有 CRLF 差异、插件程序 SHA 不同；最终实际 CI 归档另验短测及十分钟对照。输入、内存走势与早期失败统一保存在[1.1.0 记录及证据](../../docs/releases/v1.1.0.md)。

[正式版 0.3.0→0.4.1 升级](https://github.com/BgaSol/soDeck/actions/runs/43674763640) 五平台通过，宿主固定为核心 1.1.0；生产发现、公开附件字节校验、准备阶段旧进程保留、重启激活、服务器资料及自定义 30 秒刷新间隔保留、退出和清理均通过。旧 logTail 设置按新版声明退出，新 showLogMinimap 使用默认 true。首次五平台因夹具要求新旧设置对象完全相同而失败，修正为按新版声明逐值检查并增加回归后，复用原正式包通过；首轮失败与执行提交保存在[1.1.0 记录及证据](../../docs/releases/v1.1.0.md)。

五平台新增日志 WebView 专项、其他系统真实 IME/保存框、多屏、全部 Engine/故障范围及长期内存稳定性尚未完成，详见[日志工作台计划](../../docs/plans/docker-log-viewer.md)。未新增生产依赖，不修改已有容器维护、终端或文件插件的操作规则。

# 0.3.0

- Docker 侧栏采用行内操作：选择容器后直接维护；日志、终端、详情由明确入口进入中央页面。状态选择常驻，健康/Compose 选项和 socket 编辑收起；默认右侧与用户保存的停靠继续沿用。
- 启动和恢复直接提交，停止、重启、暂停和删除在对象行内确认；状态决定可用动作，结果保留对象来源。浏览视图维护固定连接和容器身份，去重包含完整目标；终端同编号不能换参数，自动刷新不推迟票据查询。
- 后台刷新保留行内菜单、焦点和 Shell 输入选区；Esc 关闭菜单或取消确认后返回可见入口。

本机 Windows x86_64 已通过相关格式/类型检查、Docker Rust 测试、脚本测试、前端构建、侧栏进程/UI 回归及真实隔离 Linux Engine 24.0.9/OpenSSH 联调。联调覆盖侧栏容器终端、关闭来源后继续输入、暂停/恢复/停止/启动/重启/删除、日志与指标、断线快照及挂载卷保留；本机归档的资源、摘要、原生架构、初始化、视图创建/释放和实际程序退出通过。界面覆盖三主题及 200/320/400/900 px，刷新焦点与键盘取消回归通过。

包来源：[ed1ba41e2187a5723e4b5b27e4612284a7cc94aa](https://github.com/BgaSol/soDeck/commit/ed1ba41e2187a5723e4b5b27e4612284a7cc94aa)；原始独立打包与实包验收：[43539625780](https://github.com/BgaSol/soDeck/actions/runs/43539625780)。Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 五平台均使用原生产物，归档摘要、架构、UI 资源、初始化、视图创建/释放和程序退出全部通过，报告与原包字节绑定通过。

[来源 Check](https://github.com/BgaSol/soDeck/actions/runs/43539593225)全部选中检查通过，包含五平台 Docker 源码测试、Linux x86_64 核心夹具测试、格式/工程脚本/文档，以及 Docker 进程/UI 和真实隔离 Linux Engine/OpenSSH 集成。核心完整浏览器、Windows desktop 和 netem 按本次改动范围跳过，不计为通过。

本机另下载本次原始五份归档、更新清单及五平台验收报告，核对提交、版本、SHA-256、大小与草稿附件；Windows x86_64 原归档实际进程验收再次通过。发行：[docker 0.3.0](https://github.com/BgaSol/soDeck/releases/tag/plugin-docker-v0.3.0)，按用户要求设为 GitHub Latest。六个附件沿用原始 run；发布前只同步发行说明和更新清单 notes，五份程序归档不变。开发期间的 0.2.0 本机测试包不作为正式候选。

未新增生产依赖或变更核心 API；继续需要核心 0.24.0 的 commands.v1 与终端 0.9.0 的 terminal.open.v1。启用中的升级在重启后生效。未执行正式版实际升级、目标应用的人工 DPI/输入法/读屏、多屏、生产服务器、rootless、代理路径或全部 Engine 版本专项；设备复核见 [原生设备验收](../../docs/plans/native-verification.md)。已有手动停靠继续恢复，不静默覆盖保存布局。

# 0.2.0

- 运行中容器详情增加“打开终端”，通过核心命令请求终端插件。固定 socket、Engine、完整容器 ID 与 shell，关闭目录集成；关闭 Docker 来源页后，已登记终端继续独立运行。当前要求核心 commands.v1。

未新增生产依赖。容器终端需要核心 0.24.0 的 commands.v1 与终端 0.9.0 的 terminal.open.v1；缺少能力或处理者时明确提示，不自动启用或替换插件。本版独立发行，启用中的升级在重启后生效。

包来源：[36a0dc5f26830659264fc8ce3f357f9088ac3b9f](https://github.com/BgaSol/soDeck/commit/36a0dc5f26830659264fc8ce3f357f9088ac3b9f)；原始独立打包与验收：[43415192099](https://github.com/BgaSol/soDeck/actions/runs/43415192099)。Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 五平台原生归档的摘要、架构、UI 资源、初始化、视图创建/释放和退出全部通过。[来源 Check](https://github.com/BgaSol/soDeck/actions/runs/43415020177) 的五平台插件源码、进程/UI、Linux 真实 Engine/OpenSSH 回归通过。

Windows x86_64 另下载本次 Docker 0.2.0 与终端 0.9.0 原始归档，核对摘要后配合相同核心源码及隔离 Engine 24.0.9/OpenSSH 联调通过，覆盖容器终端、环境值、目录集成关闭、关闭来源后继续输入及原有容器操作。该测试使用核心源码夹具；核心安装包验收和原生远程组合见[核心发行记录](../../docs/releases/v0.24.0.md)，不把它算作五平台 Docker 远端业务全部通过。

发行：[docker 0.2.0](https://github.com/BgaSol/soDeck/releases/tag/plugin-docker-v0.2.0)，非 Latest。六个附件沿用原始 run；发布前仅同步说明与更新清单 notes，五份程序归档字节不变。来源与摘要保留在[本轮附件记录](../../docs/releases/v0.24.0-assets.json)。未执行正式版安装升级、生产服务器、rootless、代理路径或全部 Engine 版本专项。

# 0.1.4

- 插件客户端使用标准库有界逐行读取，保留超长控制帧立即退出，Interrupted 自动重试；远端操作和视图生命周期保持原有契约。

未新增或升级生产依赖，核心 API 与 RPC 字段保持兼容。

包来源：[90d49f4c7ef509f89025696a71ddddebfec99d51](https://github.com/BgaSol/soDeck/commit/90d49f4c7ef509f89025696a71ddddebfec99d51)；原始独立打包与验收：[43083474805](https://github.com/BgaSol/soDeck/actions/runs/43083474805)。Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 五平台原生归档的架构、摘要、资源、初始化、视图创建/释放与退出全部一次通过。

[来源 Check](https://github.com/BgaSol/soDeck/actions/runs/43083351828) 的五平台源码、真实 Docker Engine/进程和真实 CSSOM 三主题界面回归通过。

发行：[docker 0.1.4](https://github.com/BgaSol/soDeck/releases/tag/plugin-docker-v0.1.4)，非 Latest。六个附件沿用原始 run，发布前只同步说明与更新清单的验收记录，五份程序归档字节保持不变。未执行本版历史版本升级、五平台远程包组合、netem、生产服务器或真实 IME/DPI/Wayland 专项。

# 0.1.3

- 构建与实际归档验收分任务执行，失败复验保留原包。
- 原生归档统一检查资源、初始化、视图生命周期和退出；发布前绑定来源与摘要，拒绝覆盖不同内容的同版本附件。
- 插件 API 与在线更新协议保持兼容。

包来源：[a52157346409d4457c4b00b5bbd3364df86df36c](https://github.com/BgaSol/soDeck/commit/a52157346409d4457c4b00b5bbd3364df86df36c)；原始打包与验收：[42914278659](https://github.com/BgaSol/soDeck/actions/runs/42914278659)。Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 全部原生构建，最终归档的摘要、资源、初始化、视图释放和退出通过；上传与验收独立执行。

[来源提交 Check](https://github.com/BgaSol/soDeck/actions/runs/42914202961) 全部通过，包含五平台核心/插件/共享客户端源码测试及相关浏览器、SSH/SFTP、终端、网络、Docker、通信回归。首次草稿索引延迟导致收尾失败，重跑 release 成功，未重编译或覆盖包。

发行：[docker 0.1.3](https://github.com/BgaSol/soDeck/releases/tag/plugin-docker-v0.1.3)，非 Latest。[真实正式版升级 0.1.2 → 0.1.3](https://github.com/BgaSol/soDeck/actions/runs/42919823620) 五平台通过，宿主固定为核心 0.23.1，验证下载校验、升级前原进程保留、重启激活及设置/服务器资料保留。没有将源码或包生命周期测试算作生产服务器、真实 IME/DPI/Wayland 或所有设备兼容验收。

# 0.1.2

命令输出使用标准严格 Base64 解码；非法输入明确报错，失败后释放原任务，不重复执行 Docker 操作。

独立分发；启用中的插件升级在重启后生效，核心安装包不包含插件。

包来源：[0abaf3e8ccc2045e437dac723d898eca87759f6c](https://github.com/BgaSol/soDeck/commit/0abaf3e8ccc2045e437dac723d898eca87759f6c)。原始五平台打包：[42779070485](https://github.com/BgaSol/soDeck/actions/runs/42779070485)，均使用原生产物，包内进程启动、生命周期和退出通过。

[Check 42807748154](https://github.com/BgaSol/soDeck/actions/runs/42807748154) 复验相同插件业务源码的五平台测试和真实业务；后续核心修复不修改这份插件程序。下载原五份归档及清单，核对版本、原生架构、SHA-256 和更新地址；Windows x86_64 原归档再次通过实际包进程测试。直接发布原六个附件。

未重复历史版本在线升级、用户生产服务器或全平台人工界面操作。正式发行：[docker 0.1.2](https://github.com/BgaSol/soDeck/releases/tag/plugin-docker-v0.1.2)，非 Latest。

2026-11-11 随核心 0.23.0 发行再次核对：0.1.2 继续为 Docker 最新正式版本，运行源码、共享客户端与打包输入未变，保留原始包来源和五平台验收。下载五份正式归档并核对 SHA-256、更新清单、包内版本、UI 与原生架构；原 Windows x86_64 程序的实际进程回归通过。[Check #42885519869](https://github.com/BgaSol/soDeck/actions/runs/42885519869) 与 [Docker 补测 #42887442456](https://github.com/BgaSol/soDeck/actions/runs/42887442456) 再验相同 Docker 源码的五平台测试及 Linux Engine/OpenSSH 普通业务。没有重新构建、覆盖或另升空版本；Docker 不在本轮四组件远程包组合入口内。

# Docker 变更记录

## 0.1.1（2026-11-10）

- Docker 默认固定在右侧工具栏，仍按需展开；已有布局偏好保留，可通过“恢复推荐布局”回到右侧。
- 工作台入口、插件列表和组件设置使用标准鲸鱼图标；容器详情和日志页使用宿主支持的对应图标。请配套更新核心至 0.20.1。

发行来源：

- 插件版本：0.1.1，核心 API 1、UI API 2；配套核心 0.20.1。
- 包来源提交：[`e6aa6c1c35ca0d67f10bcec5e4ca65de0f7bdd32`](https://github.com/BgaSol/soDeck/commit/e6aa6c1c35ca0d67f10bcec5e4ca65de0f7bdd32)。
- 原始打包：[Package plugin #42748870815](https://github.com/BgaSol/soDeck/actions/runs/42748870815)，五个原生目标的构建、归档解包、包内进程协议与退出全部通过。
- 源码与业务：[Check #42748827332](https://github.com/BgaSol/soDeck/actions/runs/42748827332)，五平台源码、公共前端/脚本、Windows 完整 UI/SSH、Linux 普通网络及 x86_64 Docker Engine/OpenSSH、UI 和归档回归全部通过；netem 跳过。
- Release：[Docker 0.1.1](https://github.com/BgaSol/soDeck/releases/tag/plugin-docker-v0.1.1)，按本次要求设为 GitHub Latest。

本机下载本次五份草稿归档，核对实际文件 SHA-256、GitHub 摘要和更新清单，检查各包内 0.1.1 版本、右侧默认与鲸鱼图标声明，并运行下载的 Windows x86_64 程序验收通过。没有重建已通过的包，也没有覆盖 0.1.0 正式附件。原生 WebView 人工交互、多屏/DPI、用户业务服务器和历史版本升级不在本次专项范围。

## 0.1.0（2026-11-10）

- 新增容器侧栏、主区域详情和最近日志三个独立视图，支持现有工作台的分屏与移窗协议。
- 支持容器搜索、状态/健康筛选、Compose 项目排序、隐藏环境变量值、日志快照搜索与复制。
- 支持单容器启动、停止、重启、暂停、恢复及停止后的删除，保留挂载卷。
- 支持按需资源快照；复用现有短命令能力，中断操作不自动重发。

远端先支持 Linux Docker Engine，不包含容器终端、实时日志跟随、文件导出、批量操作和镜像/网络/卷管理。日志两路输出不保证严格全局顺序；取消本地等待不会回滚已发送操作。本机行为与详细边界见[实施记录](https://github.com/BgaSol/soDeck/blob/8115fdcd5bed4108021a729da605fbde77afc2af/docs/implementation-docker-plugin.md)。

发行来源：

- 插件版本：0.1.0，核心 API 1、UI API 2。
- 包来源提交：[`d32b60a798f593df5a3b89d4a5807cc12d1bc546`](https://github.com/BgaSol/soDeck/commit/d32b60a798f593df5a3b89d4a5807cc12d1bc546)。
- 原始打包：[Package plugin #42741187586](https://github.com/BgaSol/soDeck/actions/runs/42741187586)，五个原生目标的构建、归档解包、包内进程协议和退出全部通过。
- Release：[Docker 0.1.0](https://github.com/BgaSol/soDeck/releases/tag/plugin-docker-v0.1.0)，本次按用户要求标记 GitHub Latest。
- 源码与业务：[Check #42741108087](https://github.com/BgaSol/soDeck/actions/runs/42741108087)。五平台核心/插件源码及公共检查通过；Linux Docker Engine、UI 与归档业务验证通过。首轮 Windows 主界面因官方名单固定为四项失败，`369000d` 修正测试预期。[desktop 补测 #42742706703](https://github.com/BgaSol/soDeck/actions/runs/42742706703) 两次暴露既有本地添加流程与后台刷新重叠时的焦点竞态；`1c71367` 增加三行核心前端焦点恢复和稳定回归，[Check #42745621259](https://github.com/BgaSol/soDeck/actions/runs/42745621259) 的 Windows 完整 UI 和公共检查全部通过。Docker 插件运行代码未改变，继续复用原始包。

本次另外下载草稿的五份归档和更新清单，核对大小、GitHub 摘要、清单五平台 URL/SHA-256，并在 Windows x86_64 运行下载归档内的真实进程验收。原生 WebView 手势、多屏/DPI、用户业务服务器及安装升级没有在本次验证。核心仍为既有 0.20.0，没有重发核心安装包。

上述本地添加焦点修正已进入 main，将在下次核心发行生效；当前 0.20.0 极快完成本地选择时可能需要重新按 Tab 或点击详情控件。官方插件安装流程和 Docker 业务不依赖该修正。

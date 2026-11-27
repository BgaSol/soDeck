# 0.6.1

## 0.6.2

使用唯一当前 API 2.1 SDK，与核心文件传输契约更新保持一致，业务行为沿用。

要求核心 2.10.0 的 API 2.1。独立五平台包与发布验收待完成。

- 保存规则使用同目录独占临时文件，同步后原子替换，兼容 Windows 长路径。
- 写入失败保留配置和原有运行/回执；损坏配置仍禁止覆盖。

已发布为 [network-proxy 0.6.1](https://github.com/BgaSol/soDeck/releases/tag/plugin-network-proxy-v0.6.1)，保持非 Latest。包来源提交为 [`22694e2`](https://github.com/BgaSol/soDeck/commit/22694e2530a1ebe622e312f602183292f31473d1)，[五平台原生打包与实际包验收](https://github.com/BgaSol/soDeck/actions/runs/45724546631)全部通过。

# 0.6.0

- 统一 API 2.0 监听票据、网络流、命令及共享状态入口，移除旧能力检查和页面桥。
- TCP/SOCKS5 与规则管理继续由插件实现；取消接纳和实际资源清理分别保留事实。
- 插件包携带自身 SVG 图标和 SDK 资源，独立分发；需要核心 API 2.0，API 1 核心无法加载。

发行：[network-proxy 0.6.0](https://github.com/BgaSol/soDeck/releases/tag/plugin-network-proxy-v0.6.0)，保持非 Latest；核心 2.0.0 为仓库默认入口。包来源 [e973109dcfc21d1f55a4ab1ba62245cf7406b0f4](https://github.com/BgaSol/soDeck/commit/e973109dcfc21d1f55a4ab1ba62245cf7406b0f4)；[原始独立打包](https://github.com/BgaSol/soDeck/actions/runs/45009307561)的 Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 原生构建、架构/资源、包内挂载/释放与退出均通过。本次原包另验隧道命令接纳与回执。直接发布同一草稿，六个公开附件匿名下载大小和 SHA-256 全部核对。

[源码及业务检查](https://github.com/BgaSol/soDeck/actions/runs/45010797487)通过；类型/格式、211 项脚本、核心 UI 和六插件回归、本地终端输入/十万行历史、文件编辑、状态图表、三插件协作、真实 SSH/Docker Engine/MCP pipe/PTY 与 1 GiB 上传覆盖受影响链路。核心 2.0.0、终端 0.11.0、文件 0.12.0、网络 0.6.0 的固定原包五平台基础远程/原生剪贴板通过。Windows 深度编辑器自动化在调试端口连接前失败，限制和重放来源见[核心记录](../../docs/releases/v2.0.0.md#发布与验证)。

旧版 0.5.0 → 0.6.0 的[原始五平台升级](https://github.com/BgaSol/soDeck/actions/runs/45013657747)中 Linux 两架构与 macOS 通过；Windows 安装夹具假定临时目录导致安装后找不到程序，修正为明确当前用户安装并核对登记后，[x64](https://github.com/BgaSol/soDeck/actions/runs/45014565212)与[ARM64](https://github.com/BgaSol/soDeck/actions/runs/45014569184)补测通过。五平台确认旧 API 程序不执行、登记/启用意图/设置和数据标记保留，下载后切换到新版本并运行。先升级核心并重启，再更新插件并按提示重启。

[永久证据](release-evidence/v0.6.0-assets.json)保留包来源、五份原包报告、公开附件摘要及逐平台升级来源。netem、物理 IME/DPI/读屏、已认证 Codex CLI、全部远端系统/Engine 组合未执行；深度编辑器等既有专项继续单独维护。

# 0.5.0

- 提供普通 TCP 转发的创建／复用与管理定位命令，按完整连接上下文发布规则和实际运行状态；后台 manager 无需打开侧栏即可接受请求。
- 手动操作与跨插件请求共用业务服务、规则类型及启动／清理路径；配置写入在状态锁外完成，停止和状态读取不等待磁盘操作。
- 同目标复用、候选歧义、配置与运行差异、未知状态及原请求回包丢失均明确处理；规则文件沿用 schema 1，重连后仍手动启动。

源码验收覆盖规则修订、目标匹配、并发复用、丢失创建／状态回执、存储失败、实际监听清理，以及原有 OpenSSH、SOCKS5 协议／生命周期与 UI 回归。核心未修改产品代码，插件继续独立分发；本版独立归档的来源及平台验收见下方交付记录。

包来源：[57e41f1e013cb4f0fa5f2cdd607c531115d00edc](https://github.com/BgaSol/soDeck/commit/57e41f1e013cb4f0fa5f2cdd607c531115d00edc)；[来源 Check](https://github.com/BgaSol/soDeck/actions/runs/44875135645)；[测试器修复后复验](https://github.com/BgaSol/soDeck/actions/runs/44876235305)；[原始独立打包与实际包验收](https://github.com/BgaSol/soDeck/actions/runs/44875170213)；[正式 Release](https://github.com/BgaSol/soDeck/releases/tag/plugin-network-proxy-v0.5.0)。

Windows x86_64／ARM64、Linux x86_64／ARM64、macOS ARM64 五平台原生归档、架构／资源和包内程序初始化、生命周期、退出均通过；网络归档另验证协作命令、真实监听及清理。来源 Check 的五平台源码、脚本、浏览器、Windows 桌面／SSH、两种 Linux 架构网络及真实 Docker 联通通过；末尾 Docker UI 测试存在异步断言竞态。测试提交 ad0ea8f 仅修复测试等待，两个插件及共享运行／打包输入保持原树，修复后的 Docker UI、进程与真实业务复验通过。原始失败和复验任务均保留在交付证据中；netem 未运行。核心版本保持 1.11.0，仓库 Latest 保留给核心。

公开匿名下载的更新清单及五份归档，其大小与 SHA-256 均已核对一致；保留原始打包附件，未重建或覆盖程序包。固定附件 ID、大小、SHA-256、来源任务及验证／跳过范围保存在[本版交付证据](release-evidence/v0.5.0-assets.json)。真实 IPv6／指定非回环网卡、rootless、Windows Server Windows Engine 及原生设备交互的剩余范围见[专项记录](../../docs/plans/docker-port-tunnels.md)。

# 0.4.6

- 规则进入详情与返回方向一致，状态与检查文字稳定更新；修复公共菜单的中途退出和隐藏后迟到动效。
- 复用现有 DOM、CSS 与 WAAPI，未新增生产依赖或核心 API。

包来源：[d88a62f05a841cde078dfa38e8f6ebb55303b7a5](https://github.com/BgaSol/soDeck/commit/d88a62f05a841cde078dfa38e8f6ebb55303b7a5)；[来源完整 Check](https://github.com/BgaSol/soDeck/actions/runs/43914640450)；[原始独立打包与实际包验收](https://github.com/BgaSol/soDeck/actions/runs/43914667034)。同一最终提交完整 Check（all，不含 netem）通过，覆盖五平台核心/插件源码、工程脚本、Linux 浏览器、Windows 桌面及 SSH、真实 SFTP/PTY、网络、系统状态、Docker 和组件通信。

Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 五平台原生独立归档、架构/资源及包内程序握手、生命周期与退出通过。

[五平台原生远程组合](https://github.com/BgaSol/soDeck/actions/runs/43921785675)通过。固定使用本次原始核心 1.3.3、终端 0.9.3、文件 0.11.4、网络 0.4.6 包，验证原生 WebView/Channel 下的 PTY、文件读写/下载、网络转发、移窗、重开与退出；入口不包含系统状态或 Docker。

发行：[network-proxy 0.4.6](https://github.com/BgaSol/soDeck/releases/tag/plugin-network-proxy-v0.4.6)，非仓库 Latest。发布原始 run 的同一草稿归档，不重建或覆盖正式版本程序包；更新清单 notes 与 Release 说明一致。[永久发行证据](https://github.com/BgaSol/soDeck/blob/main/plugins/network-proxy/release-evidence/v0.4.6-assets.json)保留来源、原生报告、附件摘要和匿名公开下载核验。

netem、正式旧版到本版的线上升级、原生文件剪贴板/默认应用/深度编辑器专项，以及新增真实 IME、DPI、读屏、触控板和多屏设备验证未执行。浏览器中间帧与原生业务通过不等于五平台人工逐帧体验验收。 GitHub hosted Runner 容量不足导致任务未启动时，只重试失败平台及后续汇总，保留其他成功产物；具体次数见发行证据。 原生远程组合的 macOS ARM64 首次因 GitHub hosted Runner 容量不足未启动，仅重试该平台及后续汇总，四个成功平台与固定包输入保留；重试记录见发行证据。

# 0.4.4

- 规则列表与详情切换、启动和停止状态、连通性检查和复制结果有短暂反馈。状态待确认、正在停止和失败继续保留原文字，界面不会提前显示成功。
- 统一控件和浮层节奏，减少动态效果与隐藏页面停止效果，未新增生产依赖。

包来源：[33f001452488c7d57f8014a98fee587599a46478](https://github.com/BgaSol/soDeck/commit/33f001452488c7d57f8014a98fee587599a46478)；[来源 Check](https://github.com/BgaSol/soDeck/actions/runs/43847374906)；[原始独立打包与实际包验收](https://github.com/BgaSol/soDeck/actions/runs/43847496934)。

Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 五平台原生归档、架构/资源、包内程序握手、生命周期与退出均通过；来源 Check 的五平台源码及相关业务和 UI 回归通过。

[五平台原生远程组合](https://github.com/BgaSol/soDeck/actions/runs/43851035246)通过。固定使用原始核心 1.3.0、终端 0.9.1、文件 0.11.2、网络 0.4.4 包，验证原生 WebView/Channel 下的 PTY、文件读写/下载、网络转发、移窗、重开与退出。该入口不包含系统状态或 Docker。

发行：[network-proxy 0.4.4](https://github.com/BgaSol/soDeck/releases/tag/plugin-network-proxy-v0.4.4)。使用上述原始 run 的同一草稿归档，不重建或覆盖正式版本的程序包；更新清单的 notes 与 Release 说明保持一致。永久来源、原生报告、附件摘要和公开下载核验见[发行证据](release-evidence/v0.4.4-assets.json)。

netem、旧版到本版的线上升级、原生文件剪贴板/默认应用/深度编辑器专项，以及新增真实 IME、DPI、读屏、触控板、多屏设备验证未执行，不计为通过。

macOS ARM64 的打包首次因 GitHub hosted runner 容量不足未启动，只重试失败任务和后续汇总，原始 run 与成功平台保留；不是代码或业务测试失败。

# 0.4.3

- 规则列表、详情与表单使用独立打包的 morphdom 资源，删除重复递归 diff，附 MIT 许可证。
- 按压队列随挂载容器结束；轮询结果核对挂载代次，卸载后的快照不再绘制页面。
- 表单切换自动端口时保留其他输入节点；网络监听与业务生命周期保持原规则。

包来源：[2a510727c8fba367a638b9e79fc8468323f68256](https://github.com/BgaSol/soDeck/commit/2a510727c8fba367a638b9e79fc8468323f68256)；原始独立打包与验收：[43373715746](https://github.com/BgaSol/soDeck/actions/runs/43373715746)。Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 五平台原生归档的架构、摘要、资源、初始化、视图创建/释放与退出全部一次通过，包含独立 DOM 运行资源和许可证。

[来源 Check](https://github.com/BgaSol/soDeck/actions/runs/43373679922) 全部相关检查通过，包含五平台原生源码及 Linux x86_64/ARM64 的真实 SSH/SOCKS/curl、进程与网络界面回归。本地真实网络专项同时通过。与本轮核心 0.23.11、terminal 0.8.5、files 0.11.1 的[原包远程组合](https://github.com/BgaSol/soDeck/actions/runs/43374667881)五平台一次通过，包含真实网络通道、移窗与重开退出。

发行：[network-proxy 0.4.3](https://github.com/BgaSol/soDeck/releases/tag/plugin-network-proxy-v0.4.3)，非 Latest。六个附件沿用原始 run；发布前只同步说明与更新清单中的说明，五份程序归档字节保持不变。摘要与本轮来源见[发行附件记录](../../docs/releases/v0.23.11-assets.json)。系统 IME/触控板/DPI、生产服务器、正式版实际升级和 netem 不计为已验证。

# 0.4.2

- 插件客户端使用标准库有界逐行读取，保留超长控制帧立即退出，Interrupted 自动重试；网络通道和清理策略保持原有契约。

未新增或升级生产依赖，核心 API 与 RPC 字段保持兼容。

包来源：[90d49f4c7ef509f89025696a71ddddebfec99d51](https://github.com/BgaSol/soDeck/commit/90d49f4c7ef509f89025696a71ddddebfec99d51)；原始独立打包与验收：[43083461261](https://github.com/BgaSol/soDeck/actions/runs/43083461261)。Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 五平台原生归档的架构、摘要、资源、初始化、视图创建/释放与退出全部一次通过。

[来源 Check](https://github.com/BgaSol/soDeck/actions/runs/43083351828) 的五平台源码及 Linux x86_64/ARM64 真实 SOCKS/curl、进程与网络界面回归通过。[原包组合](https://github.com/BgaSol/soDeck/actions/runs/43088114019) 配合核心 0.23.5、terminal 0.8.4、files 0.10.3 在五平台通过远程流程与重开；组合脚本来自仅增加诊断的 330dc90，程序来源仍为上述提交。

发行：[network-proxy 0.4.2](https://github.com/BgaSol/soDeck/releases/tag/plugin-network-proxy-v0.4.2)，非 Latest。六个附件沿用原始 run，发布前只同步说明与更新清单的验收记录，五份程序归档字节保持不变。未执行本版历史版本升级、netem 或生产服务器专项。

# 0.4.1

- 构建与实际归档验收分任务执行，失败复验保留原包。
- 原生归档统一检查资源、初始化、视图生命周期和退出；发布前绑定来源与摘要，拒绝覆盖不同内容的同版本附件。
- 插件 API 与在线更新协议保持兼容。

包来源：[a52157346409d4457c4b00b5bbd3364df86df36c](https://github.com/BgaSol/soDeck/commit/a52157346409d4457c4b00b5bbd3364df86df36c)；原始打包与验收：[42914263101](https://github.com/BgaSol/soDeck/actions/runs/42914263101)。Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 全部原生构建，最终归档的摘要、资源、初始化、视图释放和退出通过；上传与验收独立执行。

[来源提交 Check](https://github.com/BgaSol/soDeck/actions/runs/42914202961) 全部通过，包含五平台核心/插件/共享客户端源码测试及相关浏览器、SSH/SFTP、终端、网络、Docker、通信回归。首次草稿索引延迟导致收尾失败，重跑 release 成功，未重编译或覆盖包。

发行：[network-proxy 0.4.1](https://github.com/BgaSol/soDeck/releases/tag/plugin-network-proxy-v0.4.1)，非 Latest。[真实正式版升级 0.4.0 → 0.4.1](https://github.com/BgaSol/soDeck/actions/runs/42919809565) 五平台通过，宿主固定为核心 0.23.1，验证下载校验、升级前原进程保留、重启激活及设置/服务器资料保留。没有将源码或包生命周期测试算作生产服务器、真实 IME/DPI/Wayland 或所有设备兼容验收。

[正式包组合](https://github.com/BgaSol/soDeck/actions/runs/42919779115) 与核心 0.23.1、terminal 0.8.2、files 0.10.1、network-proxy 0.4.1 在五平台通过 SSH/PTY/SFTP、网络、工作台与重开；系统桌面测试的具体范围及 macOS 默认关联收据跳过见[核心记录](../../docs/releases/v0.23.1.md)。

# 0.4.0

保存只保存规则，删除需先确认监听清理完成，移除保存并重启和停止后自动删除。查询失败保留原监听并显示待确认；后台 RPC 不占用整个应用状态锁，停止等待实际 socket 和子任务退出。

依赖核心 `net.listenerLifecycle.v1`。 API 主版本保持 1；缺少能力时初始化明确拒绝，不回退到旧流程。本版已独立发行；核心需先升级至 0.23.0 并重启，再升级插件。尚未补齐的故障组合见[剩余验收](../../docs/plans/operation-outcomes.md)。

包来源：[`fcf4f44`](https://github.com/BgaSol/soDeck/commit/fcf4f446d0e588c16da2187473c2c22a63d31f7c)；原始独立打包：[Package plugin #42883702381](https://github.com/BgaSol/soDeck/actions/runs/42883702381)；发行：[network-proxy 0.4.0](https://github.com/BgaSol/soDeck/releases/tag/plugin-network-proxy-v0.4.0)，本插件最新正式版本。五平台均为原生产物，下载原五份归档并核对 SHA-256、更新清单、包内版本、UI 入口及架构，直接发布同一草稿的六个附件。[Check #42885519869](https://github.com/BgaSol/soDeck/actions/runs/42885519869) 与 [Docker 补测 #42887442456](https://github.com/BgaSol/soDeck/actions/runs/42887442456) 对相同产品源码的五平台 Rust 及相关普通业务复验通过，后续仅修正测试，插件程序未改变。

[Verify native remote plugins #42885589898](https://github.com/BgaSol/soDeck/actions/runs/42885589898) 配合核心 0.23.0、终端 0.8.1、文件 0.10.0 和网络隧道 0.4.0，完成五平台实际包的启动、对应远程业务、移窗、重开及退出。下载的 Windows x86_64 进程另验查询失败、停止不被查询阻塞、明确保存/启动/删除与真实端口清理。未重复历史版本在线升级、netem、生产服务器或 IME/DPI 等深度原生专项。

发布标签触发的[重复打包 #42888179780](https://github.com/BgaSol/soDeck/actions/runs/42888179780) 已取消，不是发行来源；原六个附件的 ID、大小与摘要保持不变。

# 0.3.1

共享客户端改用固定 base64 0.23.1；保留标准编码、严格解码、原始字节分页和既有 API。本版同步锁文件并重新独立打包，插件业务和核心 API 主版本不变。

实现见[接入记录](https://github.com/BgaSol/soDeck/blob/8115fdcd5bed4108021a729da605fbde77afc2af/docs/implementation-base64-opener-adoption.md)；核心与插件继续独立发行，核心包不含业务插件。

包来源：[`0abaf3e8ccc2045e437dac723d898eca87759f6c`](https://github.com/BgaSol/soDeck/commit/0abaf3e8ccc2045e437dac723d898eca87759f6c)；原始独立打包：[Package plugin #42779058143](https://github.com/BgaSol/soDeck/actions/runs/42779058143)；正式发行：[network-proxy 0.3.1](https://github.com/BgaSol/soDeck/releases/tag/plugin-network-proxy-v0.3.1)，非 Latest。

五个原生目标独立构建、解包、包内进程启动与退出通过；ARM64 使用原生程序。[Check #42807748154](https://github.com/BgaSol/soDeck/actions/runs/42807748154) 复验相同插件运行源码的五平台测试与普通网络业务。实际包配合核心 0.22.1、终端 0.8.0 和文件 0.9.0，在 [Verify native remote plugins #42814180548，attempt 2](https://github.com/BgaSol/soDeck/actions/runs/42814180548/attempts/2) 完成五平台 TCP/SOCKS5 正文、移窗、端口释放与退出；首轮输入解析失败未启动业务。另下载原五份归档，核对 SHA-256、清单、更新地址和原生二进制架构，直接发布原六个附件。netem、历史版本在线升级与用户生产服务器未重复；不重建已通过包或覆盖正式附件。

正式发布时 GitHub 创建插件标签，触发的[重复打包 #42817942627](https://github.com/BgaSol/soDeck/actions/runs/42817942627) 已取消；不是发行来源。正式六个附件的 ID、大小与摘要、公开更新清单仍与原始草稿一致。

# 0.3.0

接入核心 0.17.0 的工作台与 UI v2。网络隧道工具可停靠中央、左右侧栏或独立窗口；移动和隐藏复用原规则与监听，局部关闭按统一协调器返回仍有效的工作台，全服务器关闭时结束连接并释放端口。

逻辑创建、候选只读、冻结同步和激活分开，避免新旧表面同时输入。请先升级核心，再升级插件并重启；核心 API 主版本保持 1，没有新增生产依赖。

包来源：[`7a3747e`](https://github.com/BgaSol/soDeck/commit/7a3747e009bf0ee51ec5be5d1ba8c72dc678cf2d)；原始独立打包：[Package plugin #42573205282](https://github.com/BgaSol/soDeck/actions/runs/42573205282)；发行：[network-proxy 0.3.0](https://github.com/BgaSol/soDeck/releases/tag/plugin-network-proxy-v0.3.0)，非 Latest。五平台原生包与包内进程生命周期验收通过；源码与普通网络任务、实际包远程组合、Windows 四插件原生交互以及未验证范围统一见[核心 0.17.0](../../docs/releases/v0.17.0.md)。插件业务源码与原包一致，复用同一份归档。

# 0.2.2

加深多巴胺主题的辅助文字，连接入口说明在黄色背景上达到小字对比度要求。

规则列表与详情刷新保留节点、键盘焦点、展开状态和检查输入，按住启停按钮跨刷新只执行一次，移出后取消。复制实际入口给出就地反馈并跨刷新保留，分页有明确的可访问名称。客户端连接失败与整条规则状态分开显示，移除界面中写死的旧版本号。

# 0.2.1

输入框文本菜单在缩窗和对话框内自动避让，保留选区、键盘操作和复制粘贴。菜单资源随插件独立打包，使用时无需联网加载。

SOCKS5 报文改用固定版本协议库。问候未完成或无效时直接关闭，不发送 CONNECT 阶段错误帧；认证方式拒绝只回复一次。入口检查支持三种绑定地址响应，并限制每个逻辑响应的总读取时间。目标连接和正文转发继续复用核心会话。

# 0.2.0

新增网络隧道独立设置页，配置新规则使用的监听地址、端口策略和 SOCKS5 认证默认值；修改偏好保留现有规则和正在运行的监听。页面使用共享三主题滚动条及应用文本菜单。组件设置需要核心 0.9.0，请先升级核心，再升级插件并重启；TCP/SOCKS5 仍复用核心连接并独立管理生命周期。

# 0.1.0

- 新增独立网络隧道插件：固定 TCP 转发和 SOCKS5 CONNECT，复用当前服务器及 SSH/SOCKS5 混合跳板路径。
- 新增按服务器管理的规则、宿主编辑弹窗、立即启停、实际入口复制、连通性检查、连接数和流量统计。
- 收起和移窗保留原运行，断线、关闭和插件退出后清理资源；重开后手动启动。
- 支持浅色、深色和多巴胺主题。核心须提供 `net.tcp.v1` 与 `ui.dialog.v1`。

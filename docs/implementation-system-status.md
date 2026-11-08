# 系统状态插件实施记录

日期：2026-11-08。基线：`main` 的 `455a55c`。本次在原仓库实现，没有创建分支、工作树或 PR。系统状态 0.1.0 独立发行；核心 0.10.0 的发布与实际包记录维护在[版本说明](releases/v0.10.0.md)。

## 实现行为

外置 `plugins/system-status` 首版 0.1.0。默认右侧固定并展开，复用用户已保存的布局。逻辑 view 与页面分离；隐藏、切服务器、最小化和移窗不决定采集开关，关闭工作区或插件进程后释放任务。核心不包含业务插件程序。

核心新增可选 `ui.background.v1` 和 `session.exec.v1`。后台协调独立于页面，最多两个初始化 worker；短命令复用当前认证连接，拥有独立取消、deadline、输出预算、分页读取、归属和票据检查。相同请求不重放，重连后旧 Context 不再启动命令，取消单个任务不结束共享连接。SDK 保留旧 call，并添加结构化错误接口。只读 UI 缓存查询不再广播整个工作台。

插件进程使用一个调度器和四个短 RPC worker，每工作区串行、全插件最多四个远端任务。动态默认 5 秒，可设置 3/5/10/30；拓扑 30 秒、容量与 uptime 校准 60 秒，规格按连接/手动刷新读取。暂停、合并刷新、重连隔离、退避与未确认退出后的停止自动重试均在插件中。历史使用有界数值结构，最多五分钟/100 点，按保守内存估算缩短缓存，保留当前概况。

Linux/OpenWrt 使用 procfs 和 POSIX sh，不 source os-release、不依赖 Python/SFTP；容量先从 mountinfo 筛选明确的本地文件系统，再引用路径查询。Windows 使用 UTF-16LE EncodedCommand、UTF-8 JSON 和 CIM，计数在整数中做差。网络性能计数器未映射物理网卡时按实例显示。macOS、各 BSD 和 Unix 有独立命令选择；无可靠内存总使用率/磁盘 I/O 来源的字段明确不可用。

页面沿用宿主主题、字体和共享文本菜单，无新增图表或 UI 框架依赖。显示 CPU、内存、uptime、接口速率、文件系统，展开设备/磁盘/采集详情，保留五分钟 SVG 趋势和键盘可读数据点；可见时轮询只读缓存，隐藏停止页面轮询。

## 本次验证

- 核心 Rust 全套回归通过，包含后台无 iframe 挂载、未展示 view 不阻塞移窗、已有终端默认打开、票据重试不重放和重复 poll。新增命令集成验证 stdout/stderr 分离、分页完整退出、非零退出、缺少退出状态、输出上限、deadline 和取消后复用同一认证连接。修正了早期后台协调在无后台声明时提前建立普通工作区导致终端默认入口丢失的问题。
- 插件 14 项计数/协议/挂载/周期/缓存预算测试通过，包含暂停时刷新仅采一轮、Windows 数据源失败显示与整组失败重试；SDK 两项错误和二进制编码测试通过。Docker 专项分别执行 GNU sh 和 BusyBox。
- 实际 Windows x86_64 插件包通过架构/静态 CRT 检查。解开本次 zip 后运行包内 initialize、重复 mount、pause、dispose、shutdown，UI 资源齐全。
- 本机隔离 Windows OpenSSH + 实际插件进程完成采集、默认右侧展开、无页面初始采集、隐藏继续采集、暂停、恢复与关闭；不使用用户服务器。
- 本机 Linux 容器分别执行 Debian Bookworm GNU sh 和 Alpine BusyBox 采集脚本，验证 CPU、内存、挂载容量及 uptime。该项不是 OpenWrt/Dropbear 实测，也不是 Linux 客户端原生包验收。
- 固定数据 UI 验证浅色、深色、多巴胺的 160/280/480px、键盘焦点、刷新/暂停/恢复、30 秒间隔的趋势连续性、隐藏轮询和断线旧值；截图位于 `artifacts/system-status-ui/`。宿主完整 UI 套件通过，含隐藏后台 view 不创建 iframe。对实际包 UI 运行设计检测，无报告项。
- `npm run check`、`npm run plugin:check`、SDK 格式检查、77 项脚本测试及已跟踪/新增 Markdown 和 JSON 示例检查通过。
- `npm run build -- --bundles nsis -- --locked` 生成 Windows x86_64 本地核心安装包；从该 NSIS 解开包内 `sodeck.exe` 后，通过原生架构、启动/退出、多窗口移动、持久化重开及隔离 OpenSSH 验证。未执行系统安装登记或正式更新签名验收；应用版本仍为 0.9.4 的本地源码构建，不是新的正式发行。
- 共享 SDK/宿主回归另外运行 `npm run terminal:test`、`npm run files:test`、`npm run network:test`，真实包的 OpenSSH 业务、终端和文件界面、SOCKS 协议/连接清理及网络界面均通过。
- 实现提交的 GitHub Check 完成五目标原生源码、Windows 完整界面及 Linux 普通业务回归；Linux x86_64 另运行实际系统状态进程与隔离 OpenSSH，后台采集通过。Linux ARM64 的该系统状态专项未选择，不以其其他插件业务通过代替系统状态验收；运行链接维护在[插件 CHANGELOG](../plugins/system-status/CHANGELOG.md)。

以上结果对应此次工作区源码与本地包，不复用历史发行验收。独立插件包位于 `artifacts/plugins/sodeck-system-status_0.1.0_x86_64-pc-windows-msvc.zip`；核心本地包位于 `src-tauri/target/release/bundle/nsis/SoDeck_0.9.4_x64-setup.exe`。

## 发布接入与剩余范围

复用 `npm run plugin:build -- --plugin system-status` 和 Package plugin 的五个原生 runner。该插件打包工作流额外解开实际归档验证包内程序/资源，Check 的源码与普通业务矩阵已经加入 system-status；核心仍可独立打包。

系统状态 0.1.0 的五目标 GitHub 原生归档均已完成构建和包内程序/资源验收；发布前另下载真实 GitHub Windows x86_64 归档核对摘要并重复生命周期验证。包来源提交、原始打包 run、Release 及平台验证边界统一维护在[插件 CHANGELOG](../plugins/system-status/CHANGELOG.md)。核心 0.10.0 登记官方安装入口；核心源码和实际包验证结果维护在对应[版本说明](releases/v0.10.0.md)。

OpenWrt/Dropbear、Windows Server/不同默认 SSH shell、真实 macOS/FreeBSD/OpenBSD/NetBSD 及其他 Unix 尚待设备验证。macOS/BSD 总内存百分比与部分磁盘 I/O 未提供，Windows 网卡身份映射待补；不能把编入采集器等同已验证兼容。慢容量/CIM 任务会使动态采样延迟，界面展示实际观察窗口；本轮没有远端成本 p50/p95 或一小时真实运行报告。

周期采样保留原计划时间，完成后跳过错过的周期；手动刷新使用合并标记，不重置慢组周期。补充了暂停时清理回复不阻塞恢复的回归测试。

容量保留每项最后成功批次的接收时间；Linux 分批读取其他挂载不会刷新已有项的时间，拓扑移除已卸载的条目。

与完整编码方案相比，本次后台协调使用合并唤醒与一秒重试循环；未实现全套真实系统 fixture及完整设备截断计数。这些差异仍需后续验收，不以当前通过的局部测试替代。

## 0.2.0 界面改版

将用户批准的原型接入正式插件：服务器摘要、CPU/内存数值与近期趋势、双向网络图例、各挂载已用/总量与剩余容量组成连续分区，固定刷新/暂停与状态页脚；设备、磁盘 I/O 和数据来源按需展开。使用宿主三主题及现有文本菜单，160px 收紧布局但保留至少 11px 的功能文字。这里只修改插件 UI 与插件版本，采集和会话接口继续沿用首版。

`charts.js` 按客户端当前时间画最近五分钟窗口，断线与采样缺口留白；CPU、内存和网络曲线支持指针、方向键、Home/End 和 Escape 读数。网络复用已有 first/second 历史，不新增远端查询。容量仍保留整数计算，超过 `2^53` 的值不先转为浮点数；使用率达到 85% 仅显示注意色和文字，不发布重要消息。

错误保存在页面状态中，不被网卡/磁盘导航或普通快照轮询清除；成功控制请求才清除对应提示，采集组失败继续显示后台给出的原因。新鲜度取各组与挂载的真实成功时间；初次读取失败可见且自动轮询能恢复。重挂载后的滚动位置等列表恢复后应用，切换 Context 先清旧曲线。

独立视觉复核提出并修正两项数据边界：Context 改变立即清空旧身份、读数与列表，拒绝后台尚未切换的旧 Context 快照，按当前连接各组有效观测恢复展示；最近五分钟内没有有效网络样本时显示“等待有效采样”，不把过期历史或全缺口算成零峰值。回归覆盖切换后首读失败、后台旧 Context 回复、新快照无观测及逐组恢复，以及六分钟前历史和全缺口。

本次本机通过 TypeScript/核心格式、正式前端构建、78 项宿主脚本回归、14 项插件源码回归、独立 Windows x86_64 包架构及进程生命周期、隔离 Windows OpenSSH 的真实采集/后台/暂停/关闭与包内 UI。UI 回归覆盖三种宿主真实主题、160/280/320/400/480px、键盘焦点和历史、双向曲线、暂停时刷新、失败提示保持、容量分页、大整数长路径、30 秒采样缺口、隐藏页面停止轮询、断线旧值、部分指标不可用和首读失败恢复。截图仍在 `artifacts/system-status-ui/`。

正式五目标原生打包与包内启动退出由现有 `Package plugin` 执行，运行、来源提交与实际执行/跳过范围统一记录在[插件 CHANGELOG](../plugins/system-status/CHANGELOG.md)。本次没有重复 Docker GNU/BusyBox、远端平台设备或核心包验收；采集器及其既有平台限制没有扩展，不用界面回归替代这些实测。

## 0.2.1 远端 CPU 架构

在既有服务器摘要的系统名旁和设备详情中加入 `identity.architecture`，沿用现有辅助字号与自动换行。初读、连接切换后无有效 identity 观测时显示待读取；采集成功但缺少架构时明确未识别。

Unix 的 identity 命令增加 `uname -m`，macOS 同时读取 `hw.optional.arm64`，确认 Apple Silicon 时显示 ARM64。Windows 在既有 `Win32_Processor` 查询中保留 Architecture 数值并映射名称，不使用客户端安装包架构或 CPU 型号推断。实际 Windows 采集发现 PowerShell 单项数组会被展开成对象，导致原有 CPU 型号、核心数与新增架构无法解析；CPU 来源保留数组边界后恢复单/多处理器统一结构。

常见别名归一化为 x86_64、ARM64、x86、ARMv7/ARMv6；其他有效的短机器架构值保留原文，缺失、unknown 或多行异常值不作为已识别架构。该字段只扩展插件自己的快照数据，不修改核心或 RPC 主版本，不增加动态周期查询或生产依赖。

验收覆盖 Unix 各分支与 Windows 数值映射、macOS 翻译进程场景、未知数据和已有型号不能替代架构、实际本机身份采集、GNU/BusyBox 采集、三主题 160–480px、ARM64、架构缺失和 Context 无观测。五目标原生归档与同一提交的执行范围维护在[插件 CHANGELOG](../plugins/system-status/CHANGELOG.md)。

本机 TypeScript/格式与正式前端构建、78 项脚本回归、17 项插件测试、Windows 实际身份采集、独立 Windows x86_64 包内程序生命周期，以及隔离 Windows OpenSSH 集成和包内 UI 通过。GNU 与 BusyBox 分别执行容器专项并通过，新增架构随实际命令读取和解析。截图仍位于 `artifacts/system-status-ui/`，包含 ARM64 三主题、展开详情、未识别和当前连接待读取状态；独立局部复核通过，原型与既有插件设计规范保持不变。真实远端 Rosetta 会话、OpenWrt/BSD/其他 Unix 设备尚未验证，本机映射和容器结果不替代这些设备验收。

返回[文档索引](README.md)

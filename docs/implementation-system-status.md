# 系统状态插件实施记录

日期：2026-11-08。基线：`main` 的 `455a55c`。本次在原仓库实现，没有创建分支、工作树或 PR；尚未发行核心或插件。

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

以上结果对应此次工作区源码与本地包，不复用历史发行验收。独立插件包位于 `artifacts/plugins/sodeck-system-status_0.1.0_x86_64-pc-windows-msvc.zip`；核心本地包位于 `src-tauri/target/release/bundle/nsis/SoDeck_0.9.4_x64-setup.exe`。

## 发布接入与剩余范围

复用 `npm run plugin:build -- --plugin system-status` 和 Package plugin 的五个原生 runner。该插件打包工作流额外解开实际归档验证包内程序/资源，Check 的源码与普通业务矩阵已经加入 system-status；核心仍可独立打包。

尚未运行五目标 GitHub 原生包构建/验收，没有原始打包 run 或 Release 链接，也未开放尚无发行包的官方在线来源。当前本地包需在新版核心中从目录添加，正式来源在首次 Release 可下载安装后登记。

OpenWrt/Dropbear、Windows Server/不同默认 SSH shell、真实 macOS/FreeBSD/OpenBSD/NetBSD 及其他 Unix 尚待设备验证。macOS/BSD 总内存百分比与部分磁盘 I/O 未提供，Windows 网卡身份映射待补；不能把编入采集器等同已验证兼容。慢容量/CIM 任务会使动态采样延迟，界面展示实际观察窗口；本轮没有远端成本 p50/p95 或一小时真实运行报告。

周期采样保留原计划时间，完成后跳过错过的周期；手动刷新使用合并标记，不重置慢组周期。补充了暂停时清理回复不阻塞恢复的回归测试。

容量保留每项最后成功批次的接收时间；Linux 分批读取其他挂载不会刷新已有项的时间，拓扑移除已卸载的条目。

与完整编码方案相比，本次后台协调使用合并唤醒与一秒重试循环；未实现全套真实系统 fixture及完整设备截断计数。这些差异仍需后续验收，不以当前通过的局部测试替代。

返回[文档索引](README.md)

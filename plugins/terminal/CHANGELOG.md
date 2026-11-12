# 0.8.5

- 修复查找完成后高亮闪烁：取消无变化的定时请求，分批高亮完整后一次显示；后台输出保留仍有效的历史高亮，文字、视口或尺寸变化后更新画面。
- 搜索输入和按钮使用独立行，计数与后台更新提示不会挤动输入。隐藏、冻结和旧回包停止绘制、导航与自动保存；恢复、取消移窗和检查点失败后重新查询，移窗保留最新搜索草稿。

行为和限制统一见[终端功能](../../docs/features/terminal.md)。核心 API、UI 元数据和匹配规则保持兼容，未新增生产依赖；本版独立发行，启用中的插件升级在重启后生效。

2026-11-13 本机验证：类型/格式、151 项工程脚本、终端输入、完整历史入口、真实 OpenSSH/PTY 与三插件联动通过。实际 xterm/Worker 回归覆盖静止 3 秒无透明高亮帧或周期请求、无关输出时输入宽度/节点/焦点稳定、完整批次、旧/隐藏/冻结回包、恢复、草稿、检查点失败及索引重试，并保留 10 万行、Unicode、IME 组合事件、软换行、重排、三主题和窄页测试。

Windows x86_64 本机归档通过架构、资源、初始化、挂载、释放和退出验证，真实 SSH/联动测试使用同一份终端包。另用核心 0.23.9 的本机构建目录程序与该包运行原生 WebView2 157.0.4341.46：静止 3.1 秒采样 190 帧，透明高亮帧和输入宽度变化均为 0；标签隐藏/恢复、移出/返回独立窗口保留查询，返回后的原 PTY 可继续输入，核心正常退出。这不是核心安装包验收。

上述本机验证使用版本调整前的开发包，基线为 `97d6e8cf9e2043057ae4bf36822a03c634ed8d10` 加本次搜索修复，不是正式 0.8.4 的附件；正式候选重新绑定以下确定的版本与源码。

包来源：[03f9c2bcf70e1a97916ef63db411d280a0cd2010](https://github.com/BgaSol/soDeck/commit/03f9c2bcf70e1a97916ef63db411d280a0cd2010)；原始独立打包与验收：[43331154991](https://github.com/BgaSol/soDeck/actions/runs/43331154991)。Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 五平台原生构建及实际归档的架构、摘要、字体与 UI 资源、初始化、视图创建/释放、资源清理和退出全部一次通过。下载原始归档和验收报告再次绑定来源、版本与 SHA-256；不是仅完成交叉编译。

[来源 Check](https://github.com/BgaSol/soDeck/actions/runs/43331133219) 全部选中检查通过：五平台终端源码测试、格式/类型与 151 项脚本、浏览器搜索/输入/历史回归，以及 Linux x86_64 的真实 OpenSSH/PTY、Bash/Zsh 和三插件通信。Windows desktop 与 netem 未选中，记为 skipped。

发行：[terminal 0.8.5](https://github.com/BgaSol/soDeck/releases/tag/plugin-terminal-v0.8.5)，按用户要求标为 Latest。六个附件沿用上述原始 run；发布前更新说明和更新清单的验证记录，五份程序归档字节保持不变，清单的五平台地址与归档 SHA-256 核对通过。不覆盖已发行的 0.8.4 附件；核心版本和组件更新发现规则保持原状。

本机性能样本使用 Windows x86_64、Core Ultra 5 250K Plus、Node 24.22.0 与 Chromium 158.0.8127.18。实际 UI 的 100 个不同查询、100031 行/102 列，输入到完成计数 P95 为稀有 89.7 ms、无命中 95.8 ms、常见 95.0 ms、短查询 96.0 ms，包含输入合并。独立 Node 索引样本为 100000 行/120 列、11360000 文本字节，附加堆 62374592 字节、ArrayBuffer 200064 字节；这些数字不包含 xterm、准备画布、核心或完整进程内存。冻结快照回归通过，最大生成片段 10.5 ms，快照共 8093254 字节。不同夹具不能合并为端到端内存或性能保证。

尚未执行其余四平台的本次原生 WebView 搜索界面专项、正式包远程组合、历史版本升级、真实系统 IME/DPI/设备专项，以及 1/5 MiB/s、超宽/巨型逻辑行、逐字 true color 和四终端并行压力。剩余范围保留在[终端历史计划](../../docs/plans/terminal-history.md)，浏览器及包生命周期通过不代表这些目标通过。

# 0.8.4

- 插件客户端使用标准库有界逐行读取，保留超长控制帧立即退出，Interrupted 自动重试；终端业务和持久状态保持原有契约。

未新增或升级生产依赖，核心 API 与 RPC 字段保持兼容。

包来源：[90d49f4c7ef509f89025696a71ddddebfec99d51](https://github.com/BgaSol/soDeck/commit/90d49f4c7ef509f89025696a71ddddebfec99d51)；原始独立打包与验收：[43083454023](https://github.com/BgaSol/soDeck/actions/runs/43083454023)。Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 五平台原生归档的架构、摘要、资源、初始化、视图创建/释放与退出全部一次通过。

[来源 Check](https://github.com/BgaSol/soDeck/actions/runs/43083351828) 的五平台源码、PTY/Bash/Zsh、终端界面、输入与历史回归通过。[原包组合](https://github.com/BgaSol/soDeck/actions/runs/43088114019) 配合核心 0.23.5、files 0.10.3、network-proxy 0.4.2 在五平台通过远程流程与重开；组合脚本来自仅增加诊断的 330dc90，程序来源仍为上述提交。

发行：[terminal 0.8.4](https://github.com/BgaSol/soDeck/releases/tag/plugin-terminal-v0.8.4)，非 Latest。六个附件沿用原始 run，发布前只同步说明与更新清单的验收记录，五份程序归档字节保持不变。未执行本版历史版本升级、netem、生产服务器或真实系统键盘/IME/DPI/Wayland 专项。

# 0.8.3

- 全选使用 Windows/Linux Ctrl+A、macOS Cmd+A；查找使用 Windows/Linux Ctrl+F、macOS Cmd+F。两项快捷键无需 Shift，在本地处理，菜单显示对应提示。
- 启用中的插件升级在重启后生效；核心独立发行，不需要与本插件同步升级。

包来源：[c9e01423c43131e38172141dbde7ff0ec9ae692b](https://github.com/BgaSol/soDeck/commit/c9e01423c43131e38172141dbde7ff0ec9ae692b)；原始打包与独立验收：[42930260373](https://github.com/BgaSol/soDeck/actions/runs/42930260373)。Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 五个平台的原生归档均一次通过资源、初始化、视图生命周期和退出验收。

[包来源提交 Check](https://github.com/BgaSol/soDeck/actions/runs/42930211841) 全部通过，包含五平台核心/终端源码，以及相关浏览器、终端输入、历史与普通 OpenSSH/PTY 业务回归；本地真实 xterm 的 Windows/Linux/macOS 平台快捷键分支验证通过。原生系统键盘、IME/DPI/Wayland、旧版升级、原生实际包远程组合和 netem 本次未验证。

发行：[terminal 0.8.3](https://github.com/BgaSol/soDeck/releases/tag/plugin-terminal-v0.8.3)，非 Latest。正式六个附件保留原始归档，插件更新清单的五平台 URL 与归档 SHA-256 核对通过。

# 0.8.2

- 构建与实际归档验收分任务执行，失败复验保留原包。
- 原生归档统一检查资源、初始化、视图生命周期和退出；发布前绑定来源与摘要，拒绝覆盖不同内容的同版本附件。
- 插件 API 与在线更新协议保持兼容。

包来源：[a52157346409d4457c4b00b5bbd3364df86df36c](https://github.com/BgaSol/soDeck/commit/a52157346409d4457c4b00b5bbd3364df86df36c)；原始打包与验收：[42914249356](https://github.com/BgaSol/soDeck/actions/runs/42914249356)。Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 全部原生构建，最终归档的摘要、资源、初始化、视图释放和退出通过；上传与验收独立执行。

[来源提交 Check](https://github.com/BgaSol/soDeck/actions/runs/42914202961) 全部通过，包含五平台核心/插件/共享客户端源码测试及相关浏览器、SSH/SFTP、终端、网络、Docker、通信回归。首次草稿索引延迟导致收尾失败，重跑 release 成功，未重编译或覆盖包。

发行：[terminal 0.8.2](https://github.com/BgaSol/soDeck/releases/tag/plugin-terminal-v0.8.2)，非 Latest。[真实正式版升级 0.8.1 → 0.8.2](https://github.com/BgaSol/soDeck/actions/runs/42919794542) 五平台通过，宿主固定为核心 0.23.1，验证下载校验、升级前原进程保留、重启激活及设置/服务器资料保留。没有将源码或包生命周期测试算作生产服务器、真实 IME/DPI/Wayland 或所有设备兼容验收。

[正式包组合](https://github.com/BgaSol/soDeck/actions/runs/42919779115) 与核心 0.23.1、terminal 0.8.2、files 0.10.1、network-proxy 0.4.1 在五平台通过 SSH/PTY/SFTP、网络、工作台与重开；系统桌面测试的具体范围及 macOS 默认关联收据跳过见[核心记录](../../docs/releases/v0.23.1.md)。

# 0.8.1

预览阅读框改为透明底、细轮廓和窄位置标记，保留底下的彩色内容与搜索分布；悬停、拖动及键盘聚焦提供清晰的边缘提示。

搜索条输入框和按钮统一高度，文字、图标与计数居中对齐；窄分屏保持方形点击区域，极窄窗口中的按钮均匀排列。

本版独立发行，启用中的插件升级在重启后生效。

包来源：[`fcf4f44`](https://github.com/BgaSol/soDeck/commit/fcf4f446d0e588c16da2187473c2c22a63d31f7c)；原始独立打包：[Package plugin #42883692933](https://github.com/BgaSol/soDeck/actions/runs/42883692933)；发行：[terminal 0.8.1](https://github.com/BgaSol/soDeck/releases/tag/plugin-terminal-v0.8.1)，本插件最新正式版本。五平台均为原生产物，下载原五份归档并核对 SHA-256、更新清单、包内版本、UI 入口及架构，直接发布同一草稿的六个附件。[Check #42885519869](https://github.com/BgaSol/soDeck/actions/runs/42885519869) 与 [Docker 补测 #42887442456](https://github.com/BgaSol/soDeck/actions/runs/42887442456) 对相同产品源码的五平台 Rust 及相关普通业务复验通过，后续仅修正测试，插件程序未改变。

[Verify native remote plugins #42885589898](https://github.com/BgaSol/soDeck/actions/runs/42885589898) 配合核心 0.23.0、终端 0.8.1、文件 0.10.0 和网络隧道 0.4.0，完成五平台实际包的启动、对应远程业务、移窗、重开及退出。未重复历史版本在线升级、netem、生产服务器或 IME/DPI 等深度原生专项。

发布标签触发的[重复打包 #42888169182](https://github.com/BgaSol/soDeck/actions/runs/42888169182) 已取消，不是发行来源；原六个附件的 ID、大小与摘要保持不变。

# 0.8.0

当前终端保留 10 万行滚动历史，新增浮动子串搜索、大小写开关、命中计数/前后定位和常驻彩色预览。独立 Worker 维护索引与每行计数，支持 Unicode 与软换行；移窗恢复查询和有效的当前命中，退出页仍可查找。

依赖核心新增 `terminal.history.v1`，以临时恢复日志、分片冻结检查点和独立 UI 元数据支持大历史。需要核心 0.22.1；没有新增生产依赖。本机验证、性能数字及原生专项边界统一见[实施记录](https://github.com/BgaSol/soDeck/blob/8115fdcd5bed4108021a729da605fbde77afc2af/docs/implementation-terminal-search.md)。

包来源：[`febdd109cb56ec13836dd954d219f9dfcd3e0f5f`](https://github.com/BgaSol/soDeck/commit/febdd109cb56ec13836dd954d219f9dfcd3e0f5f)；原始独立打包：[Package plugin #42807888458](https://github.com/BgaSol/soDeck/actions/runs/42807888458)；同一提交的源码与业务：[Check #42807748154](https://github.com/BgaSol/soDeck/actions/runs/42807748154)；正式发行：[terminal 0.8.0](https://github.com/BgaSol/soDeck/releases/tag/plugin-terminal-v0.8.0)，非 Latest。

五平台均使用原生产物，独立包的启动、生命周期和退出通过。实际包配合核心 0.22.1、文件 0.9.0 与网络隧道 0.3.1，在 [Verify native remote plugins #42814180548，attempt 2](https://github.com/BgaSol/soDeck/actions/runs/42814180548/attempts/2) 完成五平台 PTY/原生显示、移窗、重开和退出；首次输入解析失败未启动业务。另下载原五份归档，核对 SHA-256、清单、更新地址和原生二进制架构，直接发布原六个附件。系统 IME/DPI、多屏、四终端压力和深度搜索的原生专项未执行，不把基本包验收外推为这些专项通过。

正式发布时 GitHub 创建插件标签，触发的[重复打包 #42817936348](https://github.com/BgaSol/soDeck/actions/runs/42817936348) 已取消；不是发行来源。正式六个附件的 ID、大小与摘要、公开更新清单仍与原始草稿一致；配套核心 0.22.1 为仓库 Latest。

## 0.8.0 本机性能样本

硬件：Intel Core Ultra 5 250K Plus，Windows x86_64；Node v24.22.0，Playwright Chromium 158.0.8127.18。报告保存在本机 `artifacts/terminal-history/`，均由测试脚本重生成。不同宿主、系统 WebView 与并行负载需重新测量。

`search.json`：10 万行、11,360,000 UTF-8 文本字节，按 120 列预算生成，100 个不同关键词；直接使用正式扫描器测完整计数，包含协作式让出任务，不含 UI 合并、终端网格或真实 Worker 传输。颜色摘要包含普通色和稀疏强调色。

| 查询类别 | 本次 P95 |
| --- | --- |
| 稀疏命中 | 134.1 ms |
| 无命中 | 116.7 ms |
| 密集重复子串 | 136.2 ms |
| 单字符 | 137.1 ms |

行补丁/颜色块初建约 36.2 ms。GC 后新增 JS 堆约 62.2 MB、外部 ArrayBuffer 约 0.2 MB，合计约 **59.5 MiB**。这是扫描器及文本/摘要样本的附加内存，不是整个终端、Worker 进程、核心或移窗峰值。

`ui-search.json`：102 列、100,031 个保留屏幕行，主要是短 heartbeat 行，100 个不同查询。输入事件到最终计数/有效当前序号，包括约 60 ms 合并与真实 Worker 消息：稀疏/无命中/常见片段/单字符 P95 分别约 **96.8 / 95.7 / 100.9 / 101.1 ms**。没有逐帧测量高亮完成、键盘输入响应或原生 WebView；短行结果不能代替更长文本负载。

`serialization.json`：120 列、100,024 保留行，中文/emoji/颜色/软换行约 7.94 MB 官方序列化内容。同步完整序列化约 328 ms；分片冻结快照约 549 ms，单次 iterator.next 等待最大约 17.2 ms，过程中另写入 159,200 行。恢复后的文字、颜色模式、宽度、样式、软换行、光标与官方完整序列化一致；小样本同时覆盖备用屏幕、终端模式与彩色空单元格。

上述数字支持第一版直接扫描的选择；没有启用 Bloom 过滤、通用倒排索引或额外搜索 addon。不是所有查询都能常数时间完成，也未据此宣布所有设计目标通过。

# 0.7.3

共享客户端改用固定 base64 0.23.1；保留标准编码、严格解码、原始字节分页和既有 API。本版同步锁文件并重新独立打包，插件业务和核心 API 主版本不变。

本版为未发布候选，最终按要求仅推送 main。实现见[接入记录](https://github.com/BgaSol/soDeck/blob/8115fdcd5bed4108021a729da605fbde77afc2af/docs/implementation-base64-opener-adoption.md)；核心与插件继续独立发行，核心包不含业务插件。

包来源：[`0abaf3e8ccc2045e437dac723d898eca87759f6c`](https://github.com/BgaSol/soDeck/commit/0abaf3e8ccc2045e437dac723d898eca87759f6c)；原始独立打包：[Package plugin #42779045378](https://github.com/BgaSol/soDeck/actions/runs/42779045378)；[草稿 440083889](https://github.com/BgaSol/soDeck/releases/tag/untagged-2f48279889d5db0c3017)，保留原五平台归档与更新清单，共六个附件，未正式发布。

五个原生目标独立构建、解包、包内进程启动与退出通过；ARM64 使用原生程序。最终源码 CI、实际核心候选失败及未执行的新版本包组合/升级范围见[核心候选记录](../../docs/releases/v0.21.1.md)。源码 main 的后续测试和 CI 修正不改变这些归档来源，没有重建或覆盖附件。

# 0.7.2

清单取消侧边固定与定位入口推荐。终端仍是中央多实例页面，保留默认“新建终端”行为、自动首开、分屏、移窗及原 PTY 生命周期；没有提高核心 API/UI 主版本或修改业务实现。新版核心也会忽略旧终端清单的侧边定位字段。

本轮代码与本机验证见[工具停靠实施记录](https://github.com/BgaSol/soDeck/blob/8115fdcd5bed4108021a729da605fbde77afc2af/docs/implementation-tool-docking.md)。本版独立打包，配合核心 0.21.0；终端仍负责 PTY 与自身界面，位置、排序和持久化由核心管理。

包来源：[`066df9ec5ed6a0d7fea9c4a4d41e25472995d32e`](https://github.com/BgaSol/soDeck/commit/066df9ec5ed6a0d7fea9c4a4d41e25472995d32e)；原始独立打包：[Package plugin #42763703046](https://github.com/BgaSol/soDeck/actions/runs/42763703046)；源码检查：[Check #42763617373](https://github.com/BgaSol/soDeck/actions/runs/42763617373)；五平台实际包：[Verify native remote plugins #42764368478](https://github.com/BgaSol/soDeck/actions/runs/42764368478)。

五目标均使用原生产物，独立归档的包内程序启动与退出全部通过。实际包验收配合核心 0.21.0、既有正式文件 0.8.0 和网络隧道 0.3.0，验证真实终端输入输出、显示恢复、原生窗口接续、资料重开与正常退出，五平台全部通过。另下载原始五份归档，核对 0.7.2 清单取消侧边推荐、原生架构、GitHub 草稿摘要及更新清单；直接发布同一份六个附件。

多屏混合 DPI、用户业务服务器、netem 和历史版本升级未在本轮专项中验证，完整范围见[核心 0.21.0](../../docs/releases/v0.21.0.md)。

Windows 旧工作台测试未等待异步布局完成导致 CI 断言失败；仅修正测试等待和 CI 镜像配置后的 [Check #42766195900](https://github.com/BgaSol/soDeck/actions/runs/42766195900) 完整 Windows UI、公共检查、五平台原生源码和 Linux 真实业务全部通过。插件及核心产品代码与上述原始包来源保持一致。

正式发行：[terminal 0.7.2](https://github.com/BgaSol/soDeck/releases/tag/plugin-terminal-v0.7.2)，保持非 Latest；本轮仓库 Latest 为配套[核心 0.21.0](https://github.com/BgaSol/soDeck/releases/tag/v0.21.0)。

发布后核对标签源码、六个附件与公开更新清单，附件 ID、大小与 SHA-256 保持原始草稿一致。GitHub 补建版本标签触发的[额外打包 #42768600594](https://github.com/BgaSol/soDeck/actions/runs/42768600594) 已取消，不计为发行来源。

# 0.7.1

终端首次打开前先应用宿主主题，滚动视口的初始背景也跟随主题，消除每次打开时默认黑色背景形成的短暂矩形。保留原 PTY、输入与显示恢复流程，没有新增依赖。

Windows x86_64 类型/格式检查、95 项脚本回归、真实 xterm 三主题延迟挂载与输入回归、实际包 OpenSSH 多页/重挂载/跨窗接续通过。[本次源码 Check #42752536107](https://github.com/BgaSol/soDeck/actions/runs/42752536107) 的五平台终端源码、公共检查、Linux 终端及组件协作全部通过；完整 Windows desktop 和 netem 按改动范围跳过。

包来源：[`2c0d37190868c2961b489f7552bf3973c779f4d0`](https://github.com/BgaSol/soDeck/commit/2c0d37190868c2961b489f7552bf3973c779f4d0)；原始独立打包：[Package plugin #42752591628](https://github.com/BgaSol/soDeck/actions/runs/42752591628)；发行：[terminal 0.7.1](https://github.com/BgaSol/soDeck/releases/tag/plugin-terminal-v0.7.1)，按本次要求设为 GitHub Latest。

五目标均使用原生包；[实际包验收 #42753141416](https://github.com/BgaSol/soDeck/actions/runs/42753141416) 配合既有核心 0.20.1、文件 0.8.0，验证真实终端输入输出、显示恢复、原生窗口接续、资料重开和正常退出，五平台全部通过。本机下载原始五份归档，核对包内 0.7.1 版本及启动修复、原生架构、GitHub 草稿摘要和更新清单，沿用同一份附件，不重复打包。

正式发布草稿后 GitHub 补建版本标签触发的[额外打包 #42753954350](https://github.com/BgaSol/soDeck/actions/runs/42753954350) 已取消，不计为发行来源。发布后核对 Latest、标签源码、六个附件和最终更新清单；五份包附件 ID 与 SHA-256 保持原始草稿一致。

网络插件、原生 WebView 人工闪烁观察、多屏/DPI、用户业务服务器、netem 和历史版本升级不在本次专项范围。

# 0.7.0

接入核心 0.17.0 的工作台与 UI v2。终端页面参与嵌套分屏、移组、单页或整组拆窗和返回来源；跨窗复用原 PTY，接续输出与输入草稿，候选恢复完成后才接管输入和远程尺寸。共享目录状态跟随最近实际操作的终端。

逻辑创建、界面读取、冻结同步和激活分开；普通同步继续允许输入，迁移取消后恢复输入。请先升级核心，再升级插件并重启；核心 API 主版本保持 1，没有新增生产依赖。

包来源：[`7a3747e`](https://github.com/BgaSol/soDeck/commit/7a3747e009bf0ee51ec5be5d1ba8c72dc678cf2d)；原始独立打包：[Package plugin #42573183178](https://github.com/BgaSol/soDeck/actions/runs/42573183178)；发行：[terminal 0.7.0](https://github.com/BgaSol/soDeck/releases/tag/plugin-terminal-v0.7.0)，非 Latest。五平台原生包与包内进程生命周期验收通过；修正测试后的源码检查、实际包远程组合、Windows 四插件原生交互以及未验证范围统一见[核心 0.17.0](../../docs/releases/v0.17.0.md)。插件业务源码与原包一致，复用同一份归档。

# 0.6.0

通过清单声明默认“新建终端”行为，由终端插件处理事件并主动申请页面。创建参数支持标题、登录 shell 或指定命令、目录集成策略；指定命令只使用通用 PTY 启动能力，不探测主机 shell、不注入目录脚本。命令退出后“重新打开终端”沿用原配置，创建新页面和 PTY。

同 viewId 的首次挂载只初始化一次，重挂载复用原 PTY，失败不自动重复启动；释放时清理迟到的 PTY。off 模式忽略主机目录报告，避免容器路径带动主机文件侧栏。默认行为需要核心 0.16.0；保留旧核心页面入口与旧清单字段，API 主版本仍为 1，没有新增生产依赖。

源码、原始打包运行、正式发行与本次验证范围统一见[核心 0.16.0](../../docs/releases/v0.16.0.md)。0.5.0 的旧草稿属于已撤销的另一轮整改，本次不复用其归档。

包来源：[`c022271`](https://github.com/BgaSol/soDeck/commit/c02227168791dd2cef4b959474dc5cd8873b17ad)；原始独立打包：[Package plugin #42552644774](https://github.com/BgaSol/soDeck/actions/runs/42552644774)；发行：[terminal 0.6.0](https://github.com/BgaSol/soDeck/releases/tag/plugin-terminal-v0.6.0)，非 Latest。五平台原生源码、独立包、实际包远程组合通过；macOS 首次窗口坐标检查失败及同包重试结果、跳过范围见上述核心记录。

# 0.4.0

新终端默认自动识别远端 Linux Bash（4+）/Zsh 的目录。保留用户启动配置和提示符钩子，通过本次 shell 的目录/忙碌报告联动文件侧栏；进入嵌套环境时暂停，返回后恢复。自动模式不需要主机名，不永久修改远端配置；兼容已有 OSC 7 和关闭模式只影响新终端。

需要核心 0.14.0 与文件 0.6.0 完成自动跟随。旧核心、其他远端 shell 或冲突钩子降级为普通终端并显示原因。无新增生产依赖。源码及本轮发布验收统一见[核心版本记录](../../docs/releases/v0.14.0.md)。

包来源：[`b2ee37f`](https://github.com/BgaSol/soDeck/commit/b2ee37f850c53806bc986e5213b7d11810e17033)；原始独立打包：[Package plugin #42516550281](https://github.com/BgaSol/soDeck/actions/runs/42516550281)；发行：[terminal 0.4.0](https://github.com/BgaSol/soDeck/releases/tag/plugin-terminal-v0.4.0)。五平台原生包与实际包远程组合通过，自动报告的远端范围为 Linux Bash 4+/Zsh；完整源码检查、重试原因和未验证范围见上述核心记录。

# 0.3.0

通过核心可选 `sharedState.v1` 发布交互式终端的可靠目录报告，文件侧栏可以跟随当前活动终端。目录报告主机名在设置的“目录联动”分组配置，留空关闭上报；需要正确配置 OSC 7 的 shell。恢复输出快照不重新发布旧目录，退出、输出缺口、断线与配置改变使目录失效；重连后的旧终端不报告新连接的位置。

完整联动需核心 0.13.0 与文件 0.5.0。原 PTY、输入、外观与独立升级保持兼容。主机名配置只适用于与文件浏览相同的文件系统；同主机名容器、chroot 或嵌套 shell 无法自动识别，进入前关闭跟随。没有修改远端 shell 配置或新增生产依赖。

本机真实 PowerShell、三插件 RPC/PTY/SFTP、隐藏/移窗/重连及原终端回归通过。五平台原生源码、独立包和实际包 SSH/PTY/文件组合通过；Linux x86_64 实际 bash 目录报告通过。另从草稿下载 Windows 包，验证 WebView2 拖动、关闭取消与最终释放。

包来源：[`dfdb826b0cedbe07323450692acc21bdc657bfca`](https://github.com/BgaSol/soDeck/commit/dfdb826b0cedbe07323450692acc21bdc657bfca)；原始打包：[Package plugin #42506418843](https://github.com/BgaSol/soDeck/actions/runs/42506418843)；源码：[Check #42506353705](https://github.com/BgaSol/soDeck/actions/runs/42506353705)；实际包：[Verify native remote plugins #42507409785](https://github.com/BgaSol/soDeck/actions/runs/42507409785)；发行：[terminal 0.3.0](https://github.com/BgaSol/soDeck/releases/tag/plugin-terminal-v0.3.0)，非 Latest。DPI/多显示器、macOS/zsh 实际目录报告、netem 与历史升级未重复。

# 0.2.3

终端在连续调整面板宽度时按当前容器逐帧适配，远程尺寸通知保留一个在途请求和一个可替换等待位置，持续拖动不会积压旧尺寸任务或阻挡本地绘制。隐藏、断线、退出和卸载会清理尺寸任务，恢复后重新测量；保留原终端输出、PTY 和输入顺序。

修正 FitAddon 测量容器的盒模型和终端外框高度，消除横纵裁切及底部黑条。建议同时升级核心 0.12.3，以取得面板拖动预览、保存确认与取消的完整整改。插件 UI v1 与现有设置格式保持兼容，没有新增生产依赖。

本机真实 xterm、阻塞尺寸请求、输入、外观、隐藏与卸载回归，以及隔离 OpenSSH 和实际 Windows x86_64 包内 WebView2 流程通过。Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 的原生源码、独立包构建，以及本次核心 0.12.3 + 终端 0.2.3 + 文件 0.4.2 + 网络隧道 0.2.2 的实际包远程业务验收全部通过。另从本次草稿下载 Windows 核心与终端包，摘要核对、拖动、完整行高度、取消关闭和最终退出复测通过。

包来源：[`d7095e910d298989431913f2a2c1d80df6b552c1`](https://github.com/BgaSol/soDeck/commit/d7095e910d298989431913f2a2c1d80df6b552c1)；原始独立打包：[Package plugin #42492560868](https://github.com/BgaSol/soDeck/actions/runs/42492560868)；源码检查：[Check #42492522325](https://github.com/BgaSol/soDeck/actions/runs/42492522325)；五平台实际包：[Verify native remote plugins #42493134683](https://github.com/BgaSol/soDeck/actions/runs/42493134683)；正式发行：[terminal 0.2.3](https://github.com/BgaSol/soDeck/releases/tag/plugin-terminal-v0.2.3)，不设为仓库 Latest。

发布核对五个原生归档、插件更新清单与 SHA-256，沿用原打包附件。实际 DPI/跨显示器和长时间拖动帧率、用户远端大型交互程序、netem 与历史版本对的在线升级未执行，不计为通过。核心发布和完整验收范围见[核心 0.12.3](../../docs/releases/v0.12.3.md)。

GitHub 正式发布时补建插件标签，自动触发的[额外打包 #42494474875](https://github.com/BgaSol/soDeck/actions/runs/42494474875) 被正式版本不可覆盖检查阻止上传，不计为发行来源；正式六个附件的 ID、大小与摘要仍与原始草稿一致。

# 0.2.2

加深多巴胺主题的辅助文字，保持彩色背景上的阅读对比度。

终端退出后可新建一个独立 shell，原页保留输出；重复点击只创建一次。连接断开时给出工作台重连指引，新建动作等待连接恢复。连接状态提示由核心 0.9.3 的可选 UI 增量提供；旧核心仍可使用终端，无法提供完整断线状态。

# 0.2.1

终端菜单与输入框文本菜单在缩窗时自动避让，长内容只在菜单内部滚动。键盘菜单跟随终端，右键菜单在来源滚动时关闭；保留复制粘贴、选区和鼠标报告行为。菜单资源随插件独立打包，使用时无需联网加载。

# 0.2.0

新增独立的终端外观设置，支持三款离线预置字体及本机字体、字号、行高、底部留白和光标。默认 14px、1.2 倍行高、24px 底部留白；设置在同类终端间共享，应用外观保留 PTY。滚动条跟随核心三主题，保持终端输入、复制粘贴、业务右键和鼠标跟踪行为。组件设置需要核心 0.9.0，请先升级核心，再升级插件并重启。字体及许可证随插件独立打包，使用时无需联网下载。

# 0.1.2

统一工作台主题、页签、菜单和状态栏样式。新增原生剪贴板复制/粘贴与平台快捷键；有选区时复制，无选区时 Ctrl+C 继续中断。防止重复粘贴和异步结果输入到已切换的终端。剪贴板功能需要核心 0.7.0，请先升级核心再升级插件并重启。

# 0.1.1

终端改为紧凑内容区域和底部状态栏，跟随核心的状态区域配色及浅色/深色方案。

# 0.1.0

真实远程终端，通过核心复用工作台连接。支持独立 PTY、多终端、输入/输出、尺寸、退出状态和跨窗显示快照。需要核心的 terminal.v1 与 viewData.v1 能力。

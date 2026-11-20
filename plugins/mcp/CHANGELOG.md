# 0.8.0

- 侧栏改为双向任务气泡：右侧保留客户端实际命令或远程路径，左侧合并状态、输出预览、取消和保存事实；移除重复的“正在执行”区块。
- 长输出只在气泡中显示有界预览，详情使用限高文本阅读器、游标分页和“最新输出”；定时刷新只更新事实和水位，不改写正在阅读的页。
- 配置收进右上角“配置”浮层；复制、端口冲突、Token 清除、连接换代、隐藏和迟到回复继续受原生命周期约束。下载详情改为文件凭据。
- UI 内部增加无正文事实读取和输出水位投影，复用现有任务、证据与分页能力；无新增生产依赖或 MCP 对外工具。


要求核心 1.10.0 的现有 MCP 能力；升级后重启 SoDeck。行为见[功能说明](../../docs/features/mcp.md)，字段见[MCP API](../../docs/mcp-api.md)。

发行：[Agent MCP 0.8.0](https://github.com/BgaSol/soDeck/releases/tag/plugin-mcp-v0.8.0)，按用户要求标为仓库 Latest。包来源 [0aec2ece22ac1b401bb1711e685e43f10648e825](https://github.com/BgaSol/soDeck/commit/0aec2ece22ac1b401bb1711e685e43f10648e825)；[来源 Check](https://github.com/BgaSol/soDeck/actions/runs/44766561773)首次通过；[原始五平台打包与实际包验收](https://github.com/BgaSol/soDeck/actions/runs/44766553713)第 3 次尝试收束成功。

Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 五份原生归档均在第 1 次尝试完成打包；实际包启动、十一工具发现、任务查询/交互输入/连接控制进程回归、挂载、销毁与退出通过。Windows ARM64 前两次 verify 均在 30 次短命令中的默认 1000ms 启动等待 done 断言失败，确切 Runner 延迟原因未定位；只重跑失败 verify 及后续汇总，第 3 次同一原包通过，未修改等待参数或放宽断言。所有归档摘要继续匹配原生报告与草稿附件。

本地最终类型/格式、194 项工程脚本、23 项插件 Rust 单元、MCP 进程与连接、正式 Web 构建通过。最终 UI 在锁定 Playwright 的 Chrome for Testing 159 上通过，覆盖三主题 220/280/390px、矮窗、24,000 行输出有界分页、冻结阅读、重试、退出信号、接纳未确认、文件交付、端口冲突、密钥清除、迟到回复与隐藏/卸载。审查发现的气泡截断、启动文案、信号退出、下载恢复与旧提示问题均已修复并补回归。

来源 Check 的源码、浏览器、实际 MCP/SSH 业务和两架构网络回归通过；desktop 与 netem 为 skipped。未执行五平台真实 WebView 的可见交互、已安装核心与插件的五平台真实 SSH 组合、实际旧版升级、新的已认证 Codex CLI 或物理 IME/DPI/读屏专项。原生进程与 Chromium 回归不替代这些专项；相关限制如实保留。

直接发布同一草稿的五份已验证归档，发布前只同步说明与 plugin-update.json 的 notes；未重建归档或推同版本标签再次构建。[永久发行证据](release-evidence/v0.8.0-assets.json)保留原始来源、五份报告、失败与重试、公开附件和摘要核验。

# 0.7.0

- exec_start 增加 batch/pipe/pty，task_input 使用原 taskId 继续输入文字、原始字节、pipe EOF 和 PTY resize；不自动补换行、升级模式或重跑。
- 最近输入与连续序号去重，回包丢失保留原输入身份；接纳、发送、未知结果和实际消费分开。记录只保留最近参数及一项未确认尝试。
- Collector 使用核心通用进程契约；类型化启动参数是唯一来源，输入封闭和实际 RPC 收尾后固定结果。PTY 输出为 v2 terminal，metadata 为 schemaVersion=2。
- 原十工具、连接发现/打开、下载与 batch 输出继续使用；新增为第十一个工具。无新增生产依赖，SSH 认证仍复用核心。

要求核心 1.10.0 的 session.process.v1；缺少能力时初始化提示升级。行为见[功能说明](../../docs/features/mcp.md)，字段见[MCP API](../../docs/mcp-api.md)。更新核心和已启用插件后重启 SoDeck；端口、Token、客户端配置与原侧栏位置沿用。

发行：[Agent MCP 0.7.0](https://github.com/BgaSol/soDeck/releases/tag/plugin-mcp-v0.7.0)，独立正式插件发行，仓库 Latest 保持[核心 1.10.0](https://github.com/BgaSol/soDeck/releases/tag/v1.10.0)。包来源 [c59f4786a0ce66efb6a5c88300d5b3d452245c56](https://github.com/BgaSol/soDeck/commit/c59f4786a0ce66efb6a5c88300d5b3d452245c56)；[原始五平台打包与实际包验收](https://github.com/BgaSol/soDeck/actions/runs/44740814071)第 1 次尝试通过，五份原生归档继续使用同一批原始字节。

Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 均验证原包程序启动、十一工具发现、输入/回包丢失/去重/查询与打开控制进程回归、挂载、销毁及退出，版本、架构、来源、UI 与图标检查通过。控制测试使用匹配 session.process.v1 的受控宿主。五份报告、原始包摘要、Release 与匿名下载核验保存于[永久发行证据](release-evidence/v0.7.0-assets.json)。

[包来源 Check](https://github.com/BgaSol/soDeck/actions/runs/44740795514)的五平台 MCP 源码检查通过，业务组取消，不将整体运行记为通过。之后只修改核心终端屏障与 Linux 验收夹具，MCP 生产输入保持一致；[最终 Check](https://github.com/BgaSol/soDeck/actions/runs/44751914413)的五平台核心/插件源码及全部选中界面、SSH、网络和业务回归通过，netem 未选择。

本机 191 项工程脚本通过。Windows x86_64 的原始 MCP 归档与 afc8948 核心夹具、三个隔离 OpenSSH 服务再次联调，两个流程在 16.49 秒通过，覆盖跨块 pipe、多轮输入、EOF、PTY 与 resize。Linux 最终回归另验证原始 NUL/控制/方向键字节、无回显、持续 shell、Ctrl+C/Ctrl+D，以及真实 sudo 错误密码后继续同一任务，核对 root 文件副作用、退出码与完整输出。夹具曾在真实流程已通过后因临时账号残留 PAM 进程而清理失败；修正后实际 SSH 服务、用户、规则和目录均清理，完整 CI 与隔离活跃会话清理通过。此修正没有重建已通过的插件包。

直接发布同一草稿附件，五份原始归档保持；仅同步发行说明和更新清单 notes。公开六附件的匿名下载大小和 SHA-256 与原报告及清单一致。未执行五平台已安装核心与 MCP 的真实 SSH 组合、旧版实际升级、netem、新的已认证 Codex CLI 调用、真实超过 300 秒持续等待或新增物理 IME/DPI/读屏专项。

# 0.6.0

- 新增保存服务器发现及打开、查询、取消四工具。默认名称/IP 选择包含未打开和断开的资料，分页仍保留完整匹配数，不按当前页猜唯一。
- 外部打开复用核心连接、认证和工作区服务，ready 直接返回准确 target；待用户处理沿用 SoDeck 提示，后台创建 ACK 前不提前成功。
- 原 requestId 去重与固定终态；取消不误断共享或已接纳会话，延迟首次 open 受原身份取消占位约束。重连、迟到销毁与旧收据保留准确归属。
- 运行资源从任务模块移到 App，字面匹配由资料和目标发现共用；无新增生产依赖，无另一套 SSH 或插件调用路由。

要求核心 1.9.0 的 servers.read.v1、connections.v1 和已有 session.target.v2。原六工具、Token/端口、插件 ID、证据 schemaVersion=1 与原侧栏继续兼容；已启用插件升级后重启生效。行为见[功能说明](../../docs/features/mcp.md)，字段与额度见[MCP API](../../docs/mcp-api.md)。

本机类型/格式、21 项插件单元、190 项工程脚本、完整核心套件、实际插件进程、三主题侧栏/窄屏/焦点、真实三服务器 OpenSSH 的新打开流程及原任务流程通过。Windows x64 独立归档的启动、挂载、销毁、十工具、打开/查询/取消进程回归与退出通过。新的已认证 Codex 调用未执行（CLI 未登录）；未执行五平台已安装核心与 MCP 的真实 SSH 组合、旧版升级、netem 或新增物理 IME/DPI/读屏专项。

发行：[Agent MCP 0.6.0](https://github.com/BgaSol/soDeck/releases/tag/plugin-mcp-v0.6.0)，按用户要求标为仓库 Latest。来源 [90ee7b63472f2eb7f76a17760811716ad6fa7c10](https://github.com/BgaSol/soDeck/commit/90ee7b63472f2eb7f76a17760811716ad6fa7c10)；[来源 Check](https://github.com/BgaSol/soDeck/actions/runs/44692897602)及[原始五平台打包/实际包验收](https://github.com/BgaSol/soDeck/actions/runs/44692964311)首次通过。五个原生目标的包内启动、十工具、打开/查询/取消与原任务进程回归、挂载/销毁/退出通过。来源 Check 的选中核心/UI/SSH/网络及 MCP 等业务回归通过；本次最终 Check 未选择五平台 MCP 单元 job，不将 skipped 计为通过。原始 Windows x64 归档再次与同来源核心夹具及三台隔离 OpenSSH 服务联调，两个新旧流程在 16.25 秒通过。

发布复用同一草稿五份原始归档；公开六附件匿名下载的大小、SHA-256 与原报告及清单一致，Latest 返回同一 Release。[永久发行证据](release-evidence/v0.6.0-assets.json)保留来源、原始 run、五份报告、实际 SSH 范围及初次候选失败事实。

首次候选 [44691660189](https://github.com/BgaSol/soDeck/actions/runs/44691660189)五平台构建通过，Windows x64 原包分页回归未在默认 1000 毫秒内完成文件保存，返回 pending 使测试的完成断言失败；没有生成草稿或正式发行。分页用例现在显式等待 5000 毫秒，原三十次短任务的默认等待断言保留，产品默认和输出契约没有改变。连同核心终态发布修正，最终候选按新的固定来源重新打包。

# 0.5.1

- 侧栏保留断开后的最后真实连接摘要并标注来源；一次目标观察失败显示实际原因，保留当前任务详情、分页和阅读位置，恢复成功不切换记录。真实连接或运行换代会清除旧阅读状态，迟到回复不能覆盖新连接。
- 发现、采集和任务保存统一使用类型化目标事实，移除内部旧 JSON 转换。侧栏快照始终带本地登记身份，观察期间换代的回复明确拒绝，不拼接旧观察与新身份。
- 等待配额归接纳层，可信任务错误归事实模型，侧栏组装独立；证据模块独立维护 metadata v1。原生业务字段统一选择，删除侧栏 full 再解码分支，侧栏只接受实际文本分页/最近页参数。

要求核心 1.6.0 的 session.target.v2。六工具公开契约、原请求去重与恢复、Token/端口、证据 schemaVersion=1、等待/IO/发现额度和原侧栏容器保持；没有新增生产依赖或核心 API。行为见[功能说明](../../docs/features/mcp.md)，字段与限制见 [MCP API](../../docs/mcp-api.md)。

发行：[Agent MCP 0.5.1](https://github.com/BgaSol/soDeck/releases/tag/plugin-mcp-v0.5.1)，按用户要求标为仓库 Latest。核心继续使用 [1.6.0](https://github.com/BgaSol/soDeck/releases/tag/v1.6.0)，已启用插件更新后重启 SoDeck 生效，无需重新配置端口、接入密钥或客户端；界面继续位于原侧栏。

包来源：[1d44c344544af4bd41a7c2a88b68c79447cae2a1](https://github.com/BgaSol/soDeck/commit/1d44c344544af4bd41a7c2a88b68c79447cae2a1)；[来源 Check](https://github.com/BgaSol/soDeck/actions/runs/44370973475) 在第 1 次尝试通过；[原始独立打包与实际包验收](https://github.com/BgaSol/soDeck/actions/runs/44371019314) 在第 2 次尝试完成。Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 五平台均为原生归档，实际包启动、挂载、销毁、六工具发现、任务/目标查询、断开进程回归和退出通过。逐包核对同一来源、版本、原生架构、唯一 status 侧栏、UI 与图标；发布复用同一草稿五份原始归档，不推同版本标签再次构建。[永久发行证据](release-evidence/v0.5.1-assets.json)记录五份报告、原始包数据、来源 CI、Release 与公开六附件的核验结果。

macOS verify 首次排队约 15 分钟后被 GitHub 取消，没有执行验收步骤；任务注释明确标注 ARM64 Runner 容量不足、多次分配失败。只重试该项验收及后续汇总，五份原始归档的 Artifact ID 与摘要保持，四个平台成功的验收不重复执行；所有平台仅实际构建一次。

本地 21 项插件单元、类型/格式、190 项脚本、完整核心套件、实际插件进程、Windows x86_64 release 包启动/挂载/工具查询/销毁/退出、侧栏三主题/窄屏/焦点及真实三服务器 SSH 联调通过。两个缺陷已在旧实现上稳定复现并加入回归；补充目标观察失败/恢复、断开摘要、无效身份快照、跨连接迟到回复、观察期间重绑、组合发现/等待/UI 负载、独立元数据格式和长错误尾部变化。来源 Check 的五平台 MCP 单元以及 Linux 核心、三服务器 SSH、实际进程与界面回归通过；browser/desktop/netem 未选中，不计为本次通过。

Actions 原始 Windows x86_64 归档再次与真实核心测试程序、三条隔离 OpenSSH 会话联调，14.45 秒通过；使用固定来源归档，不以本地提交前测试包代替正式包。该项覆盖目标发现、断开/重连与原任务、任务读取及证据下载，不作为已安装核心或真实生产服务器 187 验收。

未执行新的已认证 Codex 客户端调用、旧版实际安装升级、五平台已安装核心与 MCP 的真实 SSH 组合、netem 或物理 IME/DPI/读屏专项。包来源之后仅同步发行证据和删除完成计划，程序及五份原始归档保持固定。

# 0.5.0

- targets_list 增加名称、主机、完整端点和规范 IPv4 尾段查找，返回 matchedBy、resolution、registryChanged、unresolved 与可用性；离线候选参与歧义，不按排序或焦点猜测。
- 目标身份和核心 v2 摘要完整校验；无效重新登记不破坏旧目标。查询最多四个实际作业，无排队/缓存/自动连接，取消后位置随实际 RPC 结束释放。
- 普通目标结果精简，detail=true 提供完整 Context 与错误，超限明确拒绝；旧目标换代后原任务和未知接纳继续按原身份查证，不自动重放。

要求核心 1.6.0 的 session.target.v2；旧核心明确初始化失败。六工具、Token、端口、证据 schemaVersion=1、任务分页/等待和侧栏布局保持；无新增生产依赖。迁移见 [MCP API](../../docs/mcp-api.md)，行为见[功能说明](../../docs/features/mcp.md)。

发行：[Agent MCP 0.5.0](https://github.com/BgaSol/soDeck/releases/tag/plugin-mcp-v0.5.0)，按用户要求标为仓库 Latest。先更新[核心 1.6.0](https://github.com/BgaSol/soDeck/releases/tag/v1.6.0)，再更新已启用插件并重启 SoDeck。端口、接入密钥、客户端配置和原侧栏位置保持。

包来源：[50062c22566eaf8d9b4378e14a82115a4416064b](https://github.com/BgaSol/soDeck/commit/50062c22566eaf8d9b4378e14a82115a4416064b)；[来源完整 Check](https://github.com/BgaSol/soDeck/actions/runs/44349827850) 第 2 次尝试通过；[原始独立打包与实际包验收](https://github.com/BgaSol/soDeck/actions/runs/44349929560) 在第 3 次尝试完成。Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 五平台均使用原生归档，实际包启动/挂载/销毁/六工具/目标查询/进程回归/退出通过。逐包核对来源、版本、原生架构、唯一 status 侧栏及 UI/图标；发布复用同一草稿五份原始归档，不推同版本标签再次构建。[永久发行证据](release-evidence/v0.5.0-assets.json)保存五份报告、原始运行、附件摘要及公开核验。

macOS package 首次排队取消，没有执行步骤；只重试该目标后成功，其余四份归档保留。macOS verify 在第 2 次尝试排队约 16 分钟后取消，没有执行验收；只重试该项验收及后续汇总，第 3 次尝试通过。各平台只实际构建一次，没有重建成功归档。来源 Check 首轮两个界面断言失败及取消/重跑范围见[核心发行记录](../../docs/releases/v1.6.0.md)；后续只修改两个测试夹具的[对应 CI](https://github.com/BgaSol/soDeck/actions/runs/44356075286)首次通过，发行程序与归档字节保持固定。

本机类型/格式、190 项脚本、19 项插件单元、完整核心套件、独立 release 构建、实际插件进程与侧栏三主题/窄屏/焦点/生命周期回归通过。覆盖匹配与歧义、断线摘要、无效 mount 保留旧目标、前后登记变化、四个实际作业与取消占位、首个通信故障停止后续 RPC、完整/普通投影及旧/未知任务换代后不重放。真实 SSH 核心回归验证资料编辑后旧摘要、断开保留、拒绝变更保持原事实、成功重新认证才换代和 v1 兼容。

[目标发现对比证据](release-evidence/v0.5.0-discovery.json)使用实际 0.4.0 插件与受控 v1 协议复现断开摘要缺席及无效重新登记损坏旧目标；同一批三条真实回环 SSH 会话的旧全量回包为 1336 B，新 query 定位为 594 B，耗时分别为 125/139 ms。此比较只说明回包缩小，不宣称延迟改善、生产服务器 187 或 token 测量。

三条真实 OpenSSH 会话及实际 Codex 的 B/C 流程，本机 release 版本 154.71 秒通过；本次 Actions Windows x86_64 原归档再次验收 166 秒通过。B 短命令一次启动，在默认窗口内 290 ms 稳定完成；长命令的状态等待、三页、full 和诊断均通过。C 的 query=187 匹配测试服务器名称，一次查询加一次启动，短命令在默认窗口内 336 ms 完成；query=1 匹配三条回环端点，客户端询问而未执行；重连恢复只查询原任务，没有重放。[永久客户端证据](release-evidence/v0.5.0-client.json)保留该原归档的四类调用轨迹。真实 IPv4 尾段 187 的匹配另由受控值与进程回归覆盖；实际 SSH 使用 127.0.0.1 的三个端口。

未执行旧版实际安装升级、五平台已安装核心与 MCP 的真实 SSH 组合、netem 或物理 IME/DPI/读屏专项。mcp:test 使用真实核心测试程序和 MCP 原包，不冒充已安装核心；现有 remote-integration 入口只支持核心/终端/文件/网络。没有新增生产依赖或主窗口 UI。

# 0.4.0

- exec_start 默认等待稳定完成最多 1000 ms，并返回普通任务事实与第一文本页。waitMs 可指定 0–20000；零等待立即交接，长任务到期继续执行，多页用原任务和游标接续。
- 启动解析、任务接纳与结果读取完整交接；新正等待先预留位置再登记和派发，等待额度满时无新任务/原生操作/证据目录。重复请求可以改变 waitMs，实际执行参数、身份和期限保持。
- 启动与 task_get 共用一个 watch 等待及捕获/正文链路，分别等待稳定 done 与既有变化。请求取消只结束调用，读盘/额度/容量错误保留原任务身份，最终保存和清理规则继续沿用。

这是 0.3.x 启动契约升级，参数及迁移见 [MCP API](../../docs/mcp-api.md)，使用流程见[功能说明](../../docs/features/mcp.md)。六工具、Token、端口、数据、下载行为、证据 schemaVersion=1 和原侧栏保持；无新增生产依赖或核心 API，最低核心版本仍为 1.5.0。已启用插件升级在重启后生效。

本机类型/格式、16 项插件单元、189 项脚本、实际进程、独立 release 构建与原归档验收、侧栏三主题/窄屏/焦点回归通过。新正等待额度满不执行、重复请求窗口变化、持续输出、固定期限、HTTP 中断恢复、正文读取失败、两页与全部 A 进程回归通过。[响应与调用证据](release-evidence/v0.4.0-response.json)记录同一受控代表性 CPU 文本，旧流程 3 次调用/1777 字节，新流程 1 次/1046 字节，实际关键事实与输出一致；30 次受控短命令各一次调用完成，平均 HTTP 结果耗时 13.88 ms，不作为真实服务器 187 或 token 测量。

三目标真实 SSH、150000 字节下载、侧栏生命周期和实际 Codex 首次本机构建验收通过，完整用例 202.79 秒；短系统查询默认窗口内 317 ms 稳定完成，客户端未再查该任务，长命令继续状态等待、三页文本、full 与诊断。原始 Actions Windows x86_64 归档再验同流程 98.97 秒通过，短查询 352 ms、一次启动、无补查；[永久客户端证据](release-evidence/v0.4.0-client.json)记录该原包调用。首次进程检查命中旧测试程序、首次脚本检查缺少本次 shell 的 Cargo PATH、默认本地打包目录有旧 sidecar，分别重建测试产物、配置当前 shell 工具路径和使用本次独立输出目录后通过；不修改产品为环境问题兜底。

发行：[Agent MCP 0.4.0](https://github.com/BgaSol/soDeck/releases/tag/plugin-mcp-v0.4.0)，按用户要求标为仓库 Latest。包来源：[26e40c4999ce260a949f2f22f914417f852c2b8f](https://github.com/BgaSol/soDeck/commit/26e40c4999ce260a949f2f22f914417f852c2b8f)；[来源 Check](https://github.com/BgaSol/soDeck/actions/runs/44324675559) 首次通过；[原始独立打包与实际包验收](https://github.com/BgaSol/soDeck/actions/runs/44324725576) 五平台 package 只执行一次，verify 与汇总在第 3 次运行尝试收束成功。Windows x86_64 首次 verify 的默认短命令 done 断言失败，同包单任务重跑通过；确切 Runner 延迟原因未定位。macOS verify 首次排队约 15 分钟后取消、未执行任何步骤，单任务重跑通过，不计取消为验收。没有改默认等待、放宽断言或重建已成功包。

Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 五平台原生源码、独立原包启动/挂载/销毁/六工具/进程回归/退出通过；来源 Check 的 Linux 核心与三服务器 SSH、进程、界面回归通过，browser/desktop/netem 为 skipped。逐包核对同一来源、原生架构、唯一 status 侧栏和 UI/图标，正式六附件匿名下载与原报告、清单和包摘要一致，Latest 查询确认同一 Release。全部复用同一草稿原包，不推标签再次构建；[永久发行证据](release-evidence/v0.4.0-assets.json)保留来源、原始运行、五份报告、附件与失败/取消恢复事实。包来源之后仅同步最终证据和删除完成计划。

未执行旧版实际安装升级、五平台原包与真实 SSH 组合、netem 或物理 IME/DPI/读屏专项；20 秒到期边界使用模块的固定起点测试，不宣称本次执行了完整 20 秒实际网络等待。

# 0.3.0

- 六个 MCP 工具统一返回一份 JSON 文本，删除 structuredContent 副本；客户端只解析 content[0].text。
- task_get 默认普通事实与一页 text；detail=true 取得原参数、完整 Context、时间、目录与故障详情。full 只返回原信封，none 不读取正文。有正文时不重复保存水位。
- 普通结果保留真实目标、远端退出、证据与资源清理事实；错误摘要标识截断与省略详情，未知结果保留一层历史观察。下载通过 file.remotePath/localPath 交付，只有稳定完成且证据完整时提供本地文件。
- Task 只保存原始 Record，读取/取消/列表从同次捕获生成结果；正文 reader 明确 text/full 页，响应预算计入外层转义，超过 256 KiB 明确返回 resourceLimit，不修改任务或游标。

这是 0.2.x 客户端读取契约升级，迁移见 [MCP API](../../docs/mcp-api.md)。工具名称、启动与目标选择、去重、有限等待、核心 API 和 schemaVersion=1 证据文件保持；侧栏继续在原位置完成接入/监测/详情，连同仓库 0.2.3 的插件自带图标一并交付。最低核心版本仍为 1.5.0；没有新增生产依赖，已启用插件升级在重启后生效。

本机类型/格式、10 项插件单元、189 项脚本、实际进程与侧栏三主题回归通过；[响应对比证据](release-evidence/v0.3.0-response.json)保留实际 0.2.2 原包与新生产程序对同一代表性 CPU 文本的完整 MCP result，3256 → 977 字节，减少 69.99%，输出及退出/保存/清理事实一致。该测量是固定夹具，不是本次实查 187。真实核心三目标 SSH、150000 字节下载、默认/full/none/诊断与侧栏生命周期通过；[实际 Codex 验收](release-evidence/v0.3.0-client.json)确认正等待、默认读取、三页文本、原信封和完整诊断可用。首次客户端 180 秒超时未计通过，缩减远端小块写入后 103.14 秒通过。核心全套本机测试通过，ignored 专项未计通过。合并仓库既有图标更新后，最终来源三服务器 SSH 与 Codex 再验 109.73 秒通过；GitHub Actions 原始 Windows x86_64 归档再验同流程 80.16 秒通过。

发行：[Agent MCP 0.3.0](https://github.com/BgaSol/soDeck/releases/tag/plugin-mcp-v0.3.0)，按用户要求标为仓库 Latest。包来源：[8354e43b6517ea211d9afb1ba6f8f5d435050847](https://github.com/BgaSol/soDeck/commit/8354e43b6517ea211d9afb1ba6f8f5d435050847)；[来源 Check](https://github.com/BgaSol/soDeck/actions/runs/44309996583) 与[原始独立打包及实际包验收](https://github.com/BgaSol/soDeck/actions/runs/44310000363) 均首次通过。五平台 Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 原生构建和独立原包启动/挂载/销毁/退出、带密钥六工具与进程回归通过；来源 Check 的五平台插件单元及 Linux 核心/MCP SSH、进程、三主题界面回归通过，browser/desktop/netem 为 skipped。正式附件复用同一草稿的五份原始归档，不再次构建；逐包检查原生架构、status 唯一侧栏、三个 UI 源资源和插件自带图标，公开附件与报告/清单摘要一致。[永久发行证据](release-evidence/v0.3.0-assets.json)保留来源、五份报告、原包检查与公开 Latest 核验。

包来源之后仅同步实际发行证据和删除完成计划；程序、版本与原归档保持固定。

本轮只实现 A 的响应/读取契约；短命令 start/wait 与目标解析保持既有行为。未执行旧版实际安装升级、五平台原包和真实 SSH 组合、netem 或物理 IME/DPI/读屏专项。

# 0.2.3

- 机器人图标随自身包携带，默认、侧栏和组件设置引用同一 SVG。在核心 1.5.3 起，更换图标只需发行插件。

名称、技术 ID、端口、密钥、数据及 MCP 工具协议继续沿用；旧核心按原 bot 名称展示。无新增生产依赖，核心与插件独立发行。当前行为见[功能说明](../../docs/features/mcp.md)，声明契约见[插件 API](../../docs/plugin-api.md#插件图标核心-153)。

发行：[Agent MCP 0.2.3](https://github.com/BgaSol/soDeck/releases/tag/plugin-mcp-v0.2.3)，独立正式插件发行，仓库 Latest 留给核心。包来源：[584212337bd908a6f8d64dd05b8caaf001d35f3b](https://github.com/BgaSol/soDeck/commit/584212337bd908a6f8d64dd05b8caaf001d35f3b)；[来源 Check](https://github.com/BgaSol/soDeck/actions/runs/44277800228) 与[原始打包及实际包验收](https://github.com/BgaSol/soDeck/actions/runs/44277927242) 首次通过。发布使用同一草稿和五份已验证归档，不重新构建。包源码之后仅纠正核心发行版本与相应说明；插件产物保持原始提交。

Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 五平台原生构建、实际原包启动/挂载/销毁/退出、带密钥的六工具发现及任务查询回归通过。逐包核对原生架构、默认/侧栏/设置的 bot 引用、自带 SVG 与源码图形一致；Windows 与其他平台的文本换行差异保留原包字节。正式六个附件及更新清单核验记录见[永久发行证据](release-evidence/v0.2.3-assets.json)。

本机通过类型/格式、189 项脚本测试、完整核心测试、5 项插件单元、实际进程、正式前端与三主题窄屏界面、独立插件打包及实际包验收。来源 Check 的核心与插件五平台源码、Linux 浏览器和三服务器 SSH/业务回归通过。核心包的原生图标显示另由核心发行单独验收。

本次未执行旧版安装升级、新的实际 Codex CLI 调用、五平台原包与真实 SSH 组合、netem 或物理 IME/DPI/读屏设备专项。其余插件仍沿用旧图标兼容声明，未在本次迁移；其他边界审查的字体预览及共享资源构建整改留在各自任务。

# 0.2.2

- 插件列表、侧栏标题、组件设置与页面名称统一为 Agent MCP。
- 删除内容区重复标题，刷新操作放在当前服务器信息右侧；监测、接入与任务详情继续在原侧栏完成。

当前行为见[功能说明](../../docs/features/mcp.md)。插件 ID、数据目录、端口、密钥和 MCP 客户端配置沿用；已启用插件升级在重启后生效。最低核心版本仍为 1.5.0，没有新增生产依赖或核心 API。

发行：[Agent MCP 0.2.2](https://github.com/BgaSol/soDeck/releases/tag/plugin-mcp-v0.2.2)，独立正式插件发行，按用户要求标为 GitHub Latest。包来源：[a54acb31dff9bd2bf3a5903f4fca876d6b4412f8](https://github.com/BgaSol/soDeck/commit/a54acb31dff9bd2bf3a5903f4fca876d6b4412f8)；[来源 Check](https://github.com/BgaSol/soDeck/actions/runs/44266783239) 与[原始打包及实际包验收](https://github.com/BgaSol/soDeck/actions/runs/44266788778) 均首次通过。发布复用同一草稿的五份原始归档，不重新构建。

Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 五平台原生构建、实际原包启动/挂载/销毁/退出、带密钥的六工具发现和进程回归通过。逐包核对原生架构、Agent MCP 插件名称/侧栏标题/设置名称、唯一 status 侧栏声明、内容区重复标题缺席、刷新与服务器信息同排，以及三个 UI 源资源。附件摘要与原生报告、更新清单一致；[永久发行证据](release-evidence/v0.2.2-assets.json)保留五份原生报告、原包检查、公开下载与 Latest 核验。

本机通过类型/格式、187 项脚本测试、5 项插件单元、实际进程和独立插件构建。先在含侧栏壳标题的夹具稳定复现内容区重复 h1，再删除标题节点及其样式。界面回归覆盖三主题、200–1024px 宽度、400px 矮面板、主区域输入/布局/焦点、详情返回位置、分页、迟到读取、设置冲突和隐藏/卸载；同时检查元数据与运行时标题为 Agent MCP。来源 Check 的五平台原生源码与 Linux MCP 三服务器 SSH、进程、界面回归通过。

本轮未执行实际旧版安装升级、新的实际 Codex CLI 调用、五平台原包与真实 SSH 组合、netem 或物理 IME/DPI/读屏专项；通用 browser/desktop/netem 为 skipped，MCP 界面由业务任务验收。插件 ID、配置及任务/证据契约不变；任务仍不跨重启恢复，证据保留。

# 0.2.1

- 移除中央操作页及新建入口，接入设置、实时监测、执行记录和详情全部在原有侧栏完成。
- 点击记录或“查看详情”只切换侧栏内容；返回恢复列表位置与焦点，刷新和迟到回包保留当前详情及输出页。
- 接入设置默认折叠，收起设置、进入详情或隐藏侧栏时清空配置与密钥显示；端口保存仍复用组件设置与冲突机制。
- 删除中央页专用代理分支，后台目标、MCP 六工具、证据保存和取消机制继续使用同一实现，没有新增生产依赖或核心 API。

当前行为见[功能说明](../../docs/features/mcp.md)。最低核心版本仍为 1.5.0，已启用插件升级在重启后生效；接入密钥、端口和侧栏定义 ID 沿用。六个 MCP 工具的客户端契约保持兼容。

发行：[机器人接口 0.2.1](https://github.com/BgaSol/soDeck/releases/tag/plugin-mcp-v0.2.1)，独立正式插件发行，按用户要求标为 GitHub Latest。包来源：[8dc26768df5421fb402194f221cafff62578c232](https://github.com/BgaSol/soDeck/commit/8dc26768df5421fb402194f221cafff62578c232)；[来源 Check](https://github.com/BgaSol/soDeck/actions/runs/44263058929) 与[原始打包及实际包验收](https://github.com/BgaSol/soDeck/actions/runs/44263062592) 均首次通过。发布复用同一草稿的五份原始归档，不重新构建。

Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 五平台原生构建及实际原包启动/挂载/销毁/退出、带密钥的六工具发现、任务查询/等待/断开与清理回归通过。逐包核对原生架构、唯一 status 侧栏声明、中央创建入口缺席及三个 UI 源资源；Windows x86_64 原包另复验超大详情明确拒绝、摘要和取消回执有界。六个公开附件逐字节匹配原包/清单，更新清单 notes 与 Release 说明一致。[永久发行证据](release-evidence/v0.2.1-assets.json)保存来源、五份原生报告、UI 摘要、附件哈希及 Latest 核验。

本机通过类型/格式、187 项脚本测试、5 项插件单元、实际进程、独立插件构建和三服务器 SSH。界面回归覆盖三主题、200–1024px 宽度、400px 矮面板、列表位置/焦点返回、迟到读取、分页、设置冲突及收起/详情切换/隐藏后的秘密清理，同时核验主区域内容、输入、宽度和轮询中的焦点。真实核心验证中央贡献及旧入口缺席，侧栏隐藏/再显示保留同一后台目标和任务，中央标签组及选择不变。来源 Check 的五平台原生源码及 Linux MCP 三服务器、进程和界面回归通过。

本机首次 SSH 验收因正在使用的 0.2.0 占用默认端口而返回 401；夹具改为先保存独立端口再启动自己的插件，修正后通过，不改变产品默认端口。本轮没有执行实际旧版安装升级、新的实际 Codex CLI 调用、五平台原包与真实 SSH 组合、netem 或物理 IME/DPI/读屏专项；来源 Check 的通用 browser/desktop/netem 为 skipped，MCP 界面由业务任务验收。核心通用手动停靠能力保留，插件不主动创建或迁移中央页面。

# 0.2.0

- 接入设置与执行记录合并到中央操作页：上方配置服务端口、复制配置或显示密钥，下方选择记录查看固定详情和分页输出。
- 侧栏保留当前服务器、实际命令和最近输出，点击“打开操作页”查看完整记录；同一工作区只打开一个操作页，关闭页面后后台任务继续。
- 窄窗口选择记录后定位到详情，定时刷新不会丢失待定位的操作意图；刷新保留选择、分页、焦点和阅读位置。端口保存保留其他窗口更新时的冲突草稿，并说明重启生效。
- 新增 sodeck_tasks_list，按目标/服务器、requestId、种类及 active/done 查找当前运行任务，使用接纳上限和稳定游标分页。
- task_get 增加 full/text/none 和最多 20 秒的变化等待；有效 revision 排除无进展轮询，读取断开释放等待而保留远端任务。
- 任务身份包含运行编号，注册、去重、事实快照和正文水位统一；最终保存及清理完成后才发布稳定 done。UI 读取和取消使用同一 Context 归属检查。
- 接纳处理中或未知时明确拒绝新的同域请求；最终保存失败和各项清理失败保留真实结果。列表摘要及详情按传输预算处理，超大详情明确提示使用证据或 MCP。

当前行为见[功能说明](../../docs/features/mcp.md)，客户端字段和错误见 [MCP API](../../docs/mcp-api.md)。最低核心版本仍为 1.5.0；没有新增生产依赖或变更核心 API。默认立即 full 和原五工具保留；客户端应按不透明字符串使用 taskId，不能依赖旧数字后缀。启用插件的升级在重启后生效。

发行：[机器人接口 0.2.0](https://github.com/BgaSol/soDeck/releases/tag/plugin-mcp-v0.2.0)，独立正式插件发行，按用户要求标为 GitHub Latest。包来源：[e37bd7beb338e5ba25d1b348e00c9390219e55ee](https://github.com/BgaSol/soDeck/commit/e37bd7beb338e5ba25d1b348e00c9390219e55ee)；[来源 Check](https://github.com/BgaSol/soDeck/actions/runs/44256795333) 与[原始打包及实际包验收](https://github.com/BgaSol/soDeck/actions/runs/44256829168) 首次通过。此前未发行的 0.1.4 界面合并在本版，重新构建；发布复用上述原始 run 的同一草稿和归档，不再次构建。

Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 五平台原生构建、包内程序启动/挂载/销毁/退出、带密钥的六工具发现和任务查询回归通过。每份实际原包均验证分页、full/text/none、等待到期/唤醒、HTTP 中断后配额释放、24 活动额度、未知接纳不重发、最终保存失败和两项清理失败。原包架构与三个界面源资源逐一核对；六个公开附件大小和 SHA-256 与原包/清单一致，Release 说明与更新清单 notes 对齐。[永久发行证据](release-evidence/v0.2.0-assets.json)保留固定源码、五份原生报告、原始 run、附件摘要及 Latest 核验。

Windows x86_64 本机格式/类型、187 项脚本测试、核心测试与正式构建、5 项 MCP 单元、实际进程、三服务器 SSH 和独立原包验收通过；原包另验证超过界面预算的完整详情被明确拒绝，列表与取消回执仍有界。界面回归覆盖三主题、200–1024px 宽度、400px 矮页面、设置冲突、隐藏密钥、历史选择/分页/焦点、摘要与完整详情、迟到读取及刷新替代读取的受控交错。来源 Check 的 Linux 三服务器、进程、MCP 界面及相关核心源码回归通过。

首轮未发行候选 [ca0b707](https://github.com/BgaSol/soDeck/commit/ca0b7074e7b4140ab64f96d5a77d302d71837802) 的 [Check](https://github.com/BgaSol/soDeck/actions/runs/44254243976) 在 MCP 窄窗口详情定位断言失败；定位意图与具体读取请求耦合，以及测试在记录重排后等待了错误对象，已修正并增加受控回归。首轮草稿和标签撤销，旧候选不用于本次发行；最终包来自上方新 run。

本次未执行正式旧版安装升级、新的实际 Codex CLI 调用、五平台原包与真实 SSH 的组合、netem 或额外物理 IME/DPI/读屏设备专项。最终来源 Check 未选中的通用 browser/desktop/netem 任务为 skipped，MCP 自身界面在业务任务中通过。没有跨重启任务恢复、证据自动删除或新远端协议；正文仍为每页有损 UTF-8，精确字节读取原信封。

# 0.1.3

- 机器人接口侧栏增加实时操作监测：显示当前服务器、实际命令/远程路径、用时和最新输出，接入配置收进设置。
- 下方操作列表优先保留进行中的任务；点击查看原命令、服务器、退出结果、分页输出和本地证据，刷新不切换已选记录或输出页。
- 任务快照兼容追加原参数、目标与提交/结束时间。当前功能见[功能说明](../../docs/features/mcp.md#侧栏实时监测)。

发行：[机器人接口 0.1.3](https://github.com/BgaSol/soDeck/releases/tag/plugin-mcp-v0.1.3)，独立正式插件发行，按用户要求标为 GitHub Latest。包来源：[27f9491f303f8676debb59137d0a3263ce50cb39](https://github.com/BgaSol/soDeck/commit/27f9491f303f8676debb59137d0a3263ce50cb39)；[来源 Check](https://github.com/BgaSol/soDeck/actions/runs/44240065627)；[原始打包与实际包验收](https://github.com/BgaSol/soDeck/actions/runs/44240103234)。两次运行均首次通过；发布复用同一草稿的五份原始归档，未重新构建。

Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 五平台原生构建、包内程序启动/挂载/销毁/退出、带密钥的 MCP HTTP 发现和资源清理均通过。五份原包的架构、实时监测、操作列表、行内详情和输出分页资源已逐一核对；六个正式附件的公开下载大小与 SHA-256 均匹配，Release 说明与更新清单 notes 一致。[永久发行证据](release-evidence/v0.1.3-assets.json)保存包来源、原始 run、五份原生报告、附件摘要和 Latest 核验。

来源 Check 的五平台源码及 Linux 三服务器调查/进程/界面回归通过。Windows x86_64 本机格式与类型检查、单元、187 项脚本测试、正式前端构建、实际进程故障注入、三服务器 SSH、三主题侧栏界面和实际插件包验收通过。接入配置沿用，最低核心版本仍为 1.5.0，机器人图标需要核心 1.5.1；已启用插件升级在重启后生效。

本次未执行旧版升级、五平台实际包 SSH 组合、新的实际 Codex CLI 调用、netem 或额外原生 IME/DPI/读屏专项。来源 Check 未选中的 desktop/browser/netem 任务为 skipped，不作为专项通过证据。

# 0.1.2

- 插件列表、工作区侧栏、页面标题和设置入口统一显示为“机器人接口”，采用单色机器人图标。
- 插件 ID、证据目录、端口、接入密钥和 Codex 配置继续沿用；已有安装无需重新配置。机器人图标需要核心 1.5.1。

当前行为见[功能说明](https://github.com/BgaSol/soDeck/blob/main/docs/features/mcp.md)。

发行：[机器人接口 0.1.2](https://github.com/BgaSol/soDeck/releases/tag/plugin-mcp-v0.1.2)，独立正式插件发行；核心 [1.5.1](https://github.com/BgaSol/soDeck/releases/tag/v1.5.1) 为 GitHub Latest。包来源：[6c7ebce87d297c1b3e1a1735d7ab12b3e8a36ef9](https://github.com/BgaSol/soDeck/commit/6c7ebce87d297c1b3e1a1735d7ab12b3e8a36ef9)；[来源 Check](https://github.com/BgaSol/soDeck/actions/runs/44081547713)；[原始打包与实际包验收](https://github.com/BgaSol/soDeck/actions/runs/44081596688)。五平台原生源码与 Linux 三服务器调查/进程/界面检查通过；来源 Check 首次 Windows x86_64 核心宿主测试初始化超时，仅重跑失败任务及汇总，同一提交第二次通过，插件发行包未重建。

Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 五平台原生归档、包内程序启动、带密钥的 MCP HTTP 发现、生命周期与退出均通过。五份原始归档的架构、哈希、显示名称、侧栏和设置机器人图标、页面标题已逐一核对；Windows 原始发行包运行时标题为“机器人接口”。六个正式附件摘要与原生报告及更新清单一致，Release 说明与清单 notes 对齐。[永久插件发行证据](https://github.com/BgaSol/soDeck/blob/main/plugins/mcp/release-evidence/v0.1.2-assets.json)保存来源、五份报告、包内显示检查、附件摘要与公开下载核验。

Windows x86_64 本机格式、单元、进程故障注入、三主题窄屏界面、独立发行构建与实际归档启动/挂载/销毁/退出通过。本次未执行旧版升级、五平台实际包远程组合、netem、新的实际 Codex CLI 工具调用或额外原生 IME/DPI/读屏/多屏设备专项。

# 0.1.1

- 插件列表、工作区侧栏、页面标题和设置入口的显示名称统一为 `MCP`。

当前名称见[功能说明](https://github.com/BgaSol/soDeck/blob/main/docs/features/mcp.md)。插件 ID、证据目录与 Codex 接入配置继续沿用。

发行：[MCP 0.1.1](https://github.com/BgaSol/soDeck/releases/tag/plugin-mcp-v0.1.1)，独立正式插件发行。包来源：[ea93b0a61f32102ade26095b8e7fffd44a430a1c](https://github.com/BgaSol/soDeck/commit/ea93b0a61f32102ade26095b8e7fffd44a430a1c)；[来源 Check](https://github.com/BgaSol/soDeck/actions/runs/44058348231)；[原始打包与实际包验收](https://github.com/BgaSol/soDeck/actions/runs/44058371144)。五平台原生源码与 Linux 三服务器调查/进程/界面检查通过；Windows x86_64 本机格式、单元、进程、三主题窄屏界面、独立发行构建和实际归档启动/挂载/销毁/退出通过，包内插件名称、侧栏、设置标题和版本均已核对。

Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 五个平台原生归档、包内程序启动、MCP HTTP 发现、生命周期与退出均通过。正式附件沿用原始 run 的已验证字节，六个公开附件摘要与原生报告及更新清单一致；Release 说明和清单 notes 对齐。[永久发行证据](https://github.com/BgaSol/soDeck/blob/main/plugins/mcp/release-evidence/v0.1.1-assets.json)保存固定来源、五份报告、附件摘要与公开下载核验。

本次未执行旧版升级、五平台实际包远程组合、netem、额外原生 IME/DPI/读屏/多屏设备专项或新的实际 Codex CLI 验收；0.1.0 的实际 Codex 验收保留在其版本记录中。

# 0.1.0

- 新增本机 MCP 接入、固定目标上的命令采集、输出分页、取消与单文件下载；五工具复用核心会话。
- 保存原字节信封和来源元数据，重复请求复用原任务，未知接纳查询原票据。提供当前状态页、配置复制和证据目录入口。
- 最低核心 1.5.0。使用官方 Rust MCP SDK rmcp 3.10.0、HTTP 服务 axum 0.8.9 和 Token 随机数 rand 0.10.3；均为维护中的 MIT/Apache-2.0 项目，依赖版本和传递依赖已锁定。SDK 负责协议协商，axum 使用 SDK 的 HTTP 服务，rand 生成访问密钥；替代方案为手写协议/HTTP/系统随机数适配，会增加维护与跨平台差异。

发行：[MCP 0.1.0](https://github.com/BgaSol/soDeck/releases/tag/plugin-mcp-v0.1.0)，独立正式发行、非仓库 Latest。包来源：[65d9928e7da7aba5b28f032d32a359230cce326c](https://github.com/BgaSol/soDeck/commit/65d9928e7da7aba5b28f032d32a359230cce326c)；[原始打包与实际包验收](https://github.com/BgaSol/soDeck/actions/runs/44048408465)。Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 五个平台原生归档、包内程序启动、Token 验证的 MCP HTTP 发现、资源清理与退出均通过；五个归档 SHA-256 与原生报告一致。

[来源 Check](https://github.com/BgaSol/soDeck/actions/runs/44048361204) 的五平台插件源码与公共检查通过，文件业务组暴露核心新数据端点的 Linux 路径校验缺陷，后续 MCP 业务未执行。核心修正后的 [完整 Check](https://github.com/BgaSol/soDeck/actions/runs/44050460466) 验证同一插件源码，三服务器首次采集/补采、原字节下载、进程故障注入、三主题窄屏界面、取消与关闭全部通过。修正只涉及核心，插件正式归档继续复用原始独立 run。

Windows x86_64 本机单元、实际进程和三服务器 OpenSSH 验收通过，覆盖请求去重/冲突、接纳回包丢失、原字节与非零退出、截断/坏输出、取消、下载、换代及退出。实际 Codex CLI 已调用目标列表、命令和结果工具，确认退出码 0、完整证据及资源释放，返回 MCP_ACCEPTANCE_OK；使用隔离测试配置，未修改用户持久配置。

正式附件保持原始已验证的归档字节；Release 与更新清单 notes 一致。[永久插件发行证据](https://github.com/BgaSol/soDeck/blob/main/plugins/mcp/release-evidence/v0.1.0-assets.json)保留来源、五平台报告、附件摘要、真实 Codex 验收范围和公开下载核验。未执行五平台实际包远程组合、正式旧版升级、netem、完整远端系统/服务矩阵及额外原生 IME/DPI/读屏/多屏设备专项。

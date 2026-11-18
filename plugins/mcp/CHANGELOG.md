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

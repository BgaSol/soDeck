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

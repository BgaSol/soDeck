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

# 终端单行工作条：实施与验收

日期：2026-10-22。

## 实施结果

- 正常 SSH 工作区只显示一条 40 CSS px 基准工作条，字号放大时随 rem 扩展；包含终端标签、新建、历史、更多操作。长名称省略，只有实际溢出时显示全部终端菜单。
- 服务器标签新增同级菜单按钮，主窗口、独立窗口共用工具连接更新、取消验证和端口转发入口。验证组件持续挂载，切换菜单不会卸载认证流程。
- 当前终端菜单包含复制、粘贴、清理显示、全局历史搜索、连接详情、重命名、断开和关闭；后台标签菜单不提供其他终端的剪贴板动作。
- 断开、连接失败、等待验证和自动重连时显示对应恢复操作；历史模式显示只读提示和返回实时入口。历史保存异常与清理反馈保留在底栏。
- 历史模式切换保持同一 xterm 和连接实例，返回实时、复制、粘贴、清理后恢复适当焦点。剪贴板仍经过原有目标身份和多行确认逻辑，展示状态不包含粘贴内容。
- 容器终端通过默认展示模式保留原工具栏。SSH 后端协议、历史存储和迁移快照格式保持原有实现。

## 验证

| 检查 | 结果 |
| --- | --- |
| `npm run build` | TypeScript 与生产构建通过；保留既有的大体积 chunk 提示 |
| `npm test` | 202 项通过 |
| `npm run test:connection -- --max-failures 3` | 185 项通过，1 项跳过；跳过项为 WebKit 不支持的 forced-colors 仿真 |
| 单行工作条、12 个长标签与工具菜单专项 | Chromium / WebKit 共 6 项通过，包含 320/480/640/900px、14/20px 字号、菜单焦点、历史只读、取消验证后继续连接和清理动作目标 |
| `npm run test:terminal-history` | 12 项通过 |
| `npm run test:terminal-clipboard` | 26 项通过 |
| `npm run test:workspace-tools` | 8 项通过 |
| Docker / NetBird 入口回归 | 各 2 项通过，覆盖迁移后的工具更新入口与既有操作 |
| Windows 原生剪贴板与工作条 | 7 组检查通过 |
| Windows 原生工作区迁移 | 11 组检查通过 |

浏览器测试使用 IPC 夹具和真实 xterm。原生检查使用独立标识 `com.sshworkspace.headerqa`、WebView2 profile、本地 SSH/SFTP 夹具 `127.0.0.1:22239` 和 CDP `9239`，没有连接用户保存的服务器。

原生检查核对实际 SSH 输入：Ctrl+V 不补回车、不重复输入；菜单复制保留选区；菜单粘贴走多行确认；历史返回后恢复焦点且节点不变；独立容器窗口保留工具栏。工作区迁移覆盖多个真实连接、混合连接状态、冻结回滚、接收窗口关闭、主/独立窗口往返、零终端工具更新与历史身份连续性。

原生脚本：`tests/terminal-clipboard-native.mjs`、`tests/workspace-native.mjs`。运行方法沿用 [剪贴板验证说明](terminal-clipboard-verification.md) 和 [工作区验证说明](server-terminal-tabs-verification.md)。本轮工作区脚本新增 `SODECK_WORKSPACE_FIXTURE_PORT`，可与剪贴板检查共用 loopback 夹具。

本机截图和结果保存在 `.qa/terminal-header-native.png`、`.qa/server-terminal-tabs-native.png`、`.qa/terminal-clipboard-native-result.json`、`.qa/server-terminal-tabs-native-result.json`。已检查深浅色浏览器截图和原生窗口截图。

本轮未运行整个旧 `test:desktop` 套件；其旧标题和断开入口断言已同步修改。本次原生证据来自上述两个专用脚本。macOS / Linux 原生窗口未实测。

测试客户端、前端服务和 SSH 夹具已关闭，默认应用标识的调试构建已恢复；未安装或发布应用。

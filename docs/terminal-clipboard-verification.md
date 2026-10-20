# 终端复制粘贴实现与验证记录

日期：2026-10-20

## 已实现

- 实时 SSH 与容器终端统一复制粘贴服务、快捷键、工具栏和右键菜单。
- Windows 默认有选区 Ctrl+C 复制、无选区 Ctrl+C 发往远端，Ctrl+V 粘贴；支持传统快捷键和 macOS Cmd 映射。
- 多行、末尾换行、超过 16 KiB 的文本预览；可编辑、移除末尾换行、显式移除控制字符。含控制字符时默认显示符号预览。
- 按最终 UTF-8 数据和 bracketed paste 包装检查 1 MiB 上限，不分片、不截断、不额外补 Enter、不自动重发。
- 在读取、预览、提交之间校验会话代次；切历史、切标签、断开、重连、迁移、窗口失活或外部模态界面使未提交操作失效。
- 用户 DOM 输入与自动协议回复分开处理；后台终端仍可回复 DSR。失效时清空输入法缓冲，防止迟到的组合输入泄漏。
- 复制成功/失败反馈；复制失败不变成中断。预览取消后恢复终端焦点，用户已切走时不抢焦点。
- 选中即复制、右键直接粘贴、便捷快捷键持久化，并通过系统设置事件在各窗口同步。
- 历史正文选区优先于记录范围复制；范围复制继续从历史 API 获取。历史屏幕回放仅绑定复制能力，不持有输入 IPC。
- 桌面端使用 Tauri clipboard-manager；实时窗口只授权文本读写，独立历史窗口仅授权文本写入。文件面板的远端文件剪贴板保持独立。

实际实现以 `src/terminalClipboard/` 为公共模块，`TerminalPane.tsx` 保留 xterm 与 SSH 的单一提交桥。`useTerminalClipboard.tsx` 同时负责 React 弹层展示，不再单独拆出只转发参数的组件。目标身份依靠每个实例独立控制器、真实 sessionId 和失效 epoch；会话标签优先使用后端 ready 返回的实际目标。

## 验证结果

| 检查 | 结果与范围 |
| --- | --- |
| `npm test` | 134 项通过，含快捷键、粘贴分析、状态竞争、复制队列测试 |
| `npm run test:terminal-clipboard` | 26 项，Chromium / WebKit 各 13 项；真实 xterm，剪贴板与 Tauri IPC 使用测试适配 |
| `npm run test:terminal-history` | 12 项通过，覆盖历史阅读、搜索、范围操作及只读回放 |
| 系统设置 Playwright 套件 | 12 项通过，涵盖原开发者模式行为、保存失败、跨窗口更新与键盘处理 |
| Rust `system_settings` | 6 项通过；包含旧配置兼容、偏好保存、revision 冲突、写盘失败及开发者模式重启语义 |
| Rust 编译 | `cargo check --lib` 与桌面 debug 构建通过 |
| 前端构建 | `npm run build` 通过；仍有既有的 bundle 大小提示 |
| Windows 原生 | 使用独立应用标识、真实 WebView2、真实系统剪贴板与本地 SSH 夹具完成 5 组核心检查 |

Windows 原生检查内容：真实 SSH 连接；Ctrl+V 只发送一次且不补回车；右键和 Ctrl+C 复制不夹带中断；多行预览、目标来源和末尾换行处理；独立容器窗口的剪贴板权限与输入隔离。

原生验证读取 SSH 夹具实际收到的命令，验证没有重复输入或意外控制字符；没有模拟 Tauri IPC。夹具不会执行宿主命令，Docker 元数据及容器 shell 响应由夹具模拟。未据此声称真实 Docker、Vim/tmux 或物理中文输入法的全部组合已验收。

视觉检查覆盖浅色常规窗口、深色 640×580 窗口及原生主窗口预览。测试截图位于 `.qa/terminal-clipboard-preview.png`、`.qa/terminal-clipboard-dark-narrow.png`、`.qa/terminal-clipboard-native.png`；原生检查摘要位于 `.qa/terminal-clipboard-native-result.json`。

macOS / Linux 尚未进行原生桌面实测。WebKit 浏览器测试不是 macOS 原生验收；键盘策略的纯函数测试覆盖对应平台映射。

## 本机验证环境的处理

本机 crates.io 索引直连超时，验证时在 cargo 命令上临时使用官方 crates.io-index 的 GitHub sparse 地址，下载仍来自官方 crate 分发地址。没有修改仓库或全局 Cargo 镜像配置。

```powershell
cargo --config 'source.crates-io.replace-with="github-sparse"' --config 'source.github-sparse.registry="sparse+https://raw.githubusercontent.com/rust-lang/crates.io-index/master/"' check --manifest-path src-tauri/Cargo.toml --lib
```

本机 `cargo test --lib system_settings` 生成的测试 EXE 没有应用清单，启动时报 `0xc0000139`；检查定位为默认 comctl32 缺少 TaskDialogIndirect。将已编译测试程序复制到 `.qa/`，通过 Windows SDK 的 mt.exe 加入 Common Controls 6 清单后，同一套 6 项测试全部通过。改动仅作用于测试 EXE 副本，没有修改系统 DLL 或应用实现。

系统设置套件的 browser-policy-fixture 示例程序遇到同一启动问题，也以测试副本加清单方式导出原始插件脚本，再运行原有 12 项 Playwright 用例。因此记录为“Rust/设置用例通过，原始启动命令需要本机兼容处理”，而不是宣称未处理的 cargo test 命令直接成功。

## 重复执行 Windows 原生检查

原生脚本：`tests/terminal-clipboard-native.mjs`。运行环境使用以下独立配置，避免连接正常使用的数据目录。

1. 安装 `tests/requirements.txt`，在一个终端启动本地 SSH 夹具：

```powershell
$env:SSH_WORKSPACE_FIXTURE_PORT='22239'
$env:SSH_WORKSPACE_FIXTURE_DIR=Join-Path (Get-Location) '.qa\clipboard-ssh'
python tests/ssh_fixture.py
```

2. 在另一个终端启动专用前端：

```powershell
npm run dev -- --port 1469 --strictPort
```

3. 启动使用独立标识和 WebView2 数据的调试客户端：

```powershell
$env:TAURI_CONFIG='{"identifier":"com.sshworkspace.clipboardqa","build":{"devUrl":"http://127.0.0.1:1469"}}'
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS='--remote-debugging-port=9239'
$env:WEBVIEW2_USER_DATA_FOLDER=Join-Path (Get-Location) '.qa\clipboard-webview'
cargo run --manifest-path src-tauri/Cargo.toml
```

4. 客户端就绪后运行：

```powershell
npm run test:terminal-clipboard:native
```

脚本只使用本地夹具账号，在空的测试配置中创建 Clipboard native fixture；从 SSH 夹具记录核对实际输入。测试会临时使用系统文本剪贴板，结束时若未被其他应用改写，则恢复测试前的文本。支持 `SODECK_CLIPBOARD_CDP_URL` 与 `SODECK_CLIPBOARD_FIXTURE_LOG` 覆盖调试地址和夹具日志路径。

本次执行后已关闭专用测试客户端、前端服务和 SSH 夹具，并恢复默认应用标识的桌面构建。没有提交或推送 Git 变更。

# 工作台样式与终端剪贴板实施记录

2026-11-05。设计依据为 [工作台设计方案](plan-workbench-design.md)；采用 apple-design 中的清晰层级、即时反馈、空间一致性与克制原则，保留旧版多巴胺的配色和粗描边。核心和两个业务插件的代码已实施，尚未发布新版。

## 可验收行为与缺陷原因

- 服务器底栏为 32px，操作热区为 28px；终端页签、追加操作、侧栏标题、菜单和分割线使用同一套样式。浅色/深色绘制 1px 线，多巴胺绘制 2px 线；主题切换不改变终端行列、选区和滚动。
- Windows 有选区时 Ctrl+C 复制、无选区时仍中断；Linux 使用 Ctrl+Shift+C/V；macOS 使用 Cmd+C/V，Ctrl+C 仍中断。Windows/Linux 另支持 Shift+Insert，Windows 支持 Ctrl+Shift+C/V。
- 粘贴只发送一次，不追加 Enter，交由 xterm 处理换行和 bracketed paste。异步读取期间切页、失焦、换连接、关闭或隐藏视图，旧结果不得输入到其他目标。
- 右键菜单按操作读取剪贴板，取消菜单不读取；远端鼠标报告模式保留右键行为，Shift+右键可打开本地菜单。断线仍可复制已显示内容，禁止粘贴。

原样式实测底栏 26px、按钮 32px，并混用按钮 2px 与分割线 1px。原终端依赖 xterm/WebView 的默认输入，没有明确的复制动作和异步目标约束。对仓库修改前的终端 view.js 配合实际 xterm 复现：Windows 选中文字后 Ctrl+C，远端收到 `[3]`（ETX），没有调用原生复制，选区被清除。修复后的同场景只写入剪贴板，无远端输入；无选区场景仍发送 ETX。复现脚本和运行截图保存在本地 `artifacts/workbench-design/`，持续回归入口为下述测试。

## 实现边界

`src/themes.css` 是运行时主题定义，集中管理色板、结构/控件描边、字重、交互状态和终端 ANSI16 色。`src/ui.css` 提供共享的 tab、icon、quiet、status、control、menu、notice 和 edge 规则；组件保留布局与业务状态。固定 2px 描边占位、按主题绘制实际线宽，避免多巴胺切换造成尺寸跳动。共享 CSS 随插件独立打包，核心构建不依赖插件。

多巴胺使用奶油底、粉色导航、薄荷绿工具栏、黄色选中页签、紫色终端选区和近黑结构线，不使用阴影。底栏不再给每个动作套黑框，连接状态和地址仅作展示，不提供连接弹出菜单或断开入口；断线显示重新连接。成功连接后隐藏终端内部重复状态条，保留恢复、错误和退出提示。

`themeTokens()` 通过既有 UI v1 桥向插件增量传递主题。旧颜色字段保留；新核心兼容旧插件，新插件在缺少剪贴板能力时提示先升级核心。

两级 Tab 采用快捷创建交互：一级加号打开服务器搜索浮层，复用连接/定位工作区和主窗口资料表单；二级加号直接新建默认组件，旁边的箭头选择类型并创建，列表按钮独立切换已打开页面。默认类型由核心验证后保存到 `ui-layout.json`，旧文件无此字段时优先终端；不可用类型自动回退，没有可创建类型时禁用。两级加号始终留在标签滚动区域之外，按钮沿用公共工具样式。

用户已明确同意新增 Tauri 官方 `tauri-plugin-clipboard-manager = 2.5.0`，由 Tauri plugins-workspace 维护，Apache-2.0/MIT。它替代各 WebView 支持和焦点限制不一致的 Clipboard Web API；没有新增 npm 生产依赖。版本及传递依赖锁入 Cargo.lock。原生读写在核心阻塞任务中执行，避免 Linux 事件循环上的剪贴板死锁；终端和文件插件不依赖系统剪贴板实现。

核心通用 `clipboard_text` 命令只读写文本；文件路径、连接地址和终端复用该能力。终端按键、选区、菜单、IME 和 PTY 输入策略留在终端插件。剪贴板正文不进入日志、持久化、普通插件 RPC 或界面事件。总文本上限 256 KiB UTF-8，UI 桥每块最多 2048 个 UTF-16 代码单元，控制帧按 UTF-8 字节计量；写入完整校验后一次提交。无文本粘贴不发送，忙碌/超限/系统失败明确提示。

## 验证记录

已通过本机 Windows x86_64：

- `npm run check`：TypeScript 与 Rust 格式。
- `npm run scripts:test`：10 项，包含分块 Unicode/控制字符、字节上限、错误顺序、未完成写入、重复请求、失效目标和失败复制保留选区。
- `npm run test`：60 项通过，5 项需独立 OpenSSH 的测试由专用入口执行。
- `npm run ui:test`：目录、工作区、连接和生产构建页面；三主题与 720×480 小窗口，无页面错误。工作区另检查 32px 底栏、无边框动作、1/2px 分割线、连接状态仅展示且点击不弹出菜单，以及 125%/150% 显示比例。
- `npm run terminal:input:test`：实际打包的 xterm、终端脚本和核心分块桥；Windows/Linux/macOS 按键语义，中文/emoji/换行、bracketed paste、Ctrl+C、菜单、重复粘贴、失效读取、IME 不拦截、鼠标报告与主题切换保留行列/选区/滚动。系统剪贴板与远端接收端在此测试中使用确定性适配器。
- `npm run terminal:test`：独立插件包与临时回环 OpenSSH，快捷键粘贴进入真实 PTY、多 Tab、WebView 重建后输出恢复。
- `npm run files:test`：独立文件插件、真实 SFTP 与界面，文件路径通过核心剪贴板桥复制；草稿、保存、关闭、移窗、冲突、下载及三主题/窄侧栏回归。
- `npm run build -- --bundles nsis -- --locked`：Windows x86_64 核心安装包构建成功。终端、文件插件也分别完成原生独立打包。

Windows 实际 NSIS 安装到独立的 `artifacts/native-workbench/` 后已通过原生剪贴板往返、超限拒绝、重开、启动/退出和多 WebView 移窗；同一安装包加载独立 terminal/files 包连接临时 OpenSSH，真实 PTY、SFTP、移窗、断开和重开验证通过。报告位于 `artifacts/native-workbench-smoke/smoke-result.json` 与最终 `artifacts/native-workbench-remote-final/smoke-result.json`，均包含 `clipboardNative:true`，后者包含 `sshNative:true`。WebView2 退出打印 `Failed to unregister class Chrome_WidgetWin_0, Error 1412`，程序退出码为 0，测试报告完成；保留该原生运行时诊断，不将其描述为零控制台消息。

`SODECK_TEST_CLIPBOARD=1 node scripts/smoke.mjs <已安装程序>` 为显式原生文本往返检查，要求测试桌面已有文本，验证中文、emoji、CRLF/LF 和超限拒绝后恢复原有文本；只用于已准备好剪贴板的验收桌面。普通烟测不改剪贴板。原生命令往返与 Chromium 键盘回归分别验证，不声称已覆盖原生 WebView 中所有外部编辑器互拷或输入法场景。

## 尚未完成与已知边界

v0.7.0 的五个正式目标已在各自原生 GitHub runner 完成编译、打包、包内启动及 [真实插件组合验收](https://github.com/BgaSol/soDeck/actions/runs/42026635490)。核心与终端/文件/网络插件已分别正式发布，核心设为 Latest；版本、打包运行和附件核对结果见 [发行记录](releases/v0.7.0.md)。本机 Chromium 中模拟 Linux/macOS 键位仍只证明事件策略，原生包业务通过不能替代各系统的剪贴板外部编辑器互拷和复杂输入法设备验收。

一次并发构建/验收期间，源码 OpenSSH 终端 fixture 在插件初始化阶段超时；随后的串行重跑及最终实际包验收均通过，尚未确认该单次超时的根因。没有更改初始化超时或加入自动重试掩盖失败。

以上为开发阶段的本机验收记录，工作台任务本身未发布版本或升级用户现有安装。后续统一发行使用核心 0.7.0、终端 0.1.2 和文件 0.2.1，状态见 [v0.7.0 发行记录](releases/v0.7.0.md)。同次发行包含独立实现的网络插件，其功能与验收单独记录。

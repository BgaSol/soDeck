# 一体化窗口顶部栏

主窗口与六类辅助窗口共用 `WindowTitlebar`。主窗口和独立 SSH 窗口通过 `WorkspaceTitlebar` 将会话标签合入 48px 顶部栏：品牌图标、会话标签、新建／移回主窗口按钮、至少 72px 的窗口拖动空白、工具按钮和窗口控制。主窗口会话统计位于底部。历史和日志入口使用图标，保留可访问名称和悬停提示；窗口控制按钮不收缩。

标签高 34px，宽度为 120–220px，长名称省略。标签较少时“＋”紧跟标签；溢出时提供滚轮横向滚动、左右翻页和“全部会话”菜单。选择、关闭、重排标签或调整窗口尺寸后，当前标签保持可见；手动滚动不会因打开菜单而被重置。右键菜单通过 Portal 放到页面顶层，避免被滚动容器裁切，支持方向键、Home／End、Escape 和焦点恢复。

拖动标签时按实时位置计算插入点，靠近标签可视区两端会持续滚动。顶部拖动空白也接收标签并追加到末尾，因此没有标签的主窗口仍可接收独立会话。窗口按钮和工具按钮不作为放置目标。标签迁移继续使用既有 Rust 协议与会话状态转移逻辑。

## 实现位置

- `src/WindowTitlebar.tsx`：顶部布局、窗口按钮与加载状态。
- `src/WorkspaceTitlebar.tsx`：主窗口与独立 SSH 窗口的统一顶部栏。
- `src/WorkspaceTabs.tsx`、`src/WorkspaceTabMenu.tsx`：标签、快捷操作、右键菜单与全部会话菜单。
- `src/useTabStrip.ts`：溢出测量、滚轮／按钮滚动与当前标签可见性。
- `src/useWorkspaceTabDrag.ts`：标签拖放、空白追加目标与边缘自动滚动。
- `src/useDesktopWindow.ts`：读取 Rust 提供的平台模式，订阅尺寸／焦点事件，调用最小化、最大化和关闭 API。卸载时释放监听。
- `src/window-chrome.css`：深浅主题、终端色调、失焦状态、macOS 原生按钮留白和可访问性样式。
- `src-tauri/src/window_chrome.rs`：辅助窗口创建时应用平台规则，并提供 `window_chrome` 查询命令。
- `src-tauri/tauri.windows.conf.json`、`tauri.macos.conf.json`：主窗口平台配置。`app.windows` 是数组，平台配置保留完整主窗口条目；调整初始尺寸或最小尺寸时须同步基础与平台配置。
- `src-tauri/capabilities/desktop.json`：窗口按钮和拖动所需权限。

## 平台行为

| 平台 | 窗口装饰 | 窗口控制 |
| --- | --- | --- |
| Windows | `decorations: false`、`shadow: true` | 右侧自定义按钮 |
| macOS | `decorations: true`、`titleBarStyle: Overlay`、隐藏原生标题文字 | 原生红黄绿按钮；非全屏预留 88px |
| Linux | 原生窗口装饰 | 原生按钮 |
| 浏览器 | 普通页面 | 无桌面按钮及拖动区域 |

Windows 阴影、系统支持的圆角及极细边线由系统绘制；不使用透明整窗或 CSS 裁切。首版未实现 Windows 11 最大化按钮悬停的 Snap Layouts 原生命中测试。

标题文字和空白区域使用 Tauri 的 `data-tauri-drag-region`，由 Tauri 处理拖动和双击。按钮及会话标签不在拖动区域内。窗口边缘缩放使用原生能力。无需修改终端、文件树与会话标签的坐标或拖放协议。

窗口关闭统一调用 `close()`，主窗口的 `onCloseRequested` 检查活动终端、日志、传输和未保存文件；编辑器保留保存／取消流程。标题栏不直接调用 `destroy()` 或 `quit_app`。各页面已有原生标题仍保留，供任务栏和 Alt+Tab 使用。

## 验证（2026-10-15）

本次标签栏合并的前端构建通过，70 项单元测试通过；Chromium／WebKit 的窗口交互与连接回归共 46 项通过，一起运行：

```sh
npm run test:connection
```

本次 Windows debug 和指定 `x86_64-pc-windows-msvc` 的 release 构建均被系统应用程序控制策略阻断：Rust 依赖构建脚本无法执行（`os error 4551`，分别发生在 `quote` 与 `parking_lot_core`）。因此未生成包含本次标签栏改动的新可执行文件或安装包，合并后的原生窗口拖动、跨窗口标签拖拽和 650px 独立窗口仍需在构建恢复后实测。

窗口用例覆盖最大化状态同步、最小化调用、主窗口关闭确认、编辑器取消关闭及保存后关闭、窄窗口、深浅主题、macOS 留白、Linux 和浏览器模式。本次另增 8 项跨浏览器检查，覆盖 20 个标签的溢出与键盘操作、滚轮滚动、菜单焦点与排序、零／单标签布局、实时拖放目标、持续边缘滚动、空白追加和窗口按钮排除。浏览器用例明确模拟 IPC，不代表原生系统行为。

上一阶段自定义标题栏的真实 Windows 验证使用独立 `com.sshworkspace.chromeqa` 标识、WebView2 数据目录与本机 SSH/SFTP 服务（22229 端口，Docker 为测试响应）。以下结果来自标签仍为独立一行的版本：

- 七类窗口均确认 `is_decorated() == false`，共用标题栏已显示。
- 双击最大化、自定义按钮还原、原生顶部边缘缩放通过；本机显示缩放为 175%。
- 本地日志窗口最小化后，可通过主窗口入口恢复。
- 会话经标签右键菜单移到独立窗口再移回，SSH 会话与终端输出保留。
- 编辑器修改后关闭出现保存提示，取消后内容保留；撤销修改不产生远端写入。
- 主窗口关闭出现活动终端和日志提示，取消与确认退出均可执行。

本轮没有在 macOS 或 Linux 实机运行，也没有完整实测鼠标跨窗口标签拖拽、四边四角缩放、贴边分屏、多显示器或其他 DPI。它们不能用浏览器测试代替。

严格 Clippy 检查被已有模块中的 `too_many_arguments` 和 `single_element_loop` 警告阻断（`desktop_drag.rs`、`terminal.rs`、`transfers/mod.rs`、`transfers/same_server.rs`）；本次窗口模块未报告警告。

本次标签栏截图：`.qa/titlebar-tabs-many-light.png`、`.qa/titlebar-tabs-many-dark.png`。上一阶段截图：`.qa/window-chrome-light.png`、`.qa/window-chrome-dark.png`、`.qa/window-chrome-editor-overlay.png`、`.qa/window-chrome-desktop.png`、`.qa/window-chrome-desktop-dark.png`。

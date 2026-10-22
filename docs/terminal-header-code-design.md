# 终端顶部单行工作条：代码修改方案

日期：2026-10-22。状态：用户已确认实施，单行工作条已实现；验证结果见 [实施与验收记录](terminal-header-verification.md)。

功能依据：[终端顶部收敛：功能设计](terminal-header-design.md)。本方案固定采用其中的“单行工作条 · 推荐”，不实现双行备选或密度设置。

## 1. 目标和改动边界

服务器 SSH 工作区正常连接时，中央顶部从四条常驻区域收为一条约 40 CSS px 的工作条：终端标签、按需出现的“全部终端”、新建、历史、更多。窗口顶部服务器标签继续承担服务器身份；终端底栏继续承担连接摘要、通信和资源详情。

此次修改 React 组件及样式，不改变 SSH / PTY、Rust 工具上下文、数据库、历史持久化协议、剪贴板安全策略或工作区迁移 JSON 结构。连接和历史异常仍可按需显示上下文条，它们不是常驻第二条工具栏。

容器独立终端同样复用 `TerminalWorkspace`，但没有服务器二级标签；它保留现有本地工具栏。改造仅在服务器工作区通过显式展示模式开启，不能靠祖先 CSS 选择器猜测场景。

## 2. 现有结构与需要解开的依赖

| 当前事实 | 代码上的后果 |
| --- | --- |
| `ServerWorkspace` 顺序渲染 `.workspace-tool-status` 和 `TerminalTabs` | 服务器信息和工具连接占据中央第一行；需把工具连接入口移至顶端服务器标签 |
| `TerminalSession` 自己渲染 `.terminal-toolbar` | 连接状态、断开/重连、错误和认证状态局限在子组件；移除标题行前要显式暴露动作和状态 |
| `TerminalWorkspace` 自己渲染 `.terminal-history-toolbar` | 历史、复制粘贴、清屏及保存状态需要上报给工作条；单纯隐藏 CSS 会丢功能 |
| `TerminalPane` 的清理会关闭连接 | 切换标签、开关菜单、历史模式不得卸载 `TerminalSession` 或 xterm |
| `WorkspaceToolConnection` 在卸载时取消验证 | 必须持续挂载在工作区层；菜单只调用它的动作并读取展示状态 |
| 主、独立窗口都使用 `WorkspaceTitlebar` / `WorkspaceTabs` | 服务器菜单要在两种窗口中一致，动作必须绑定对应 workspace ID |
| `WorkspaceTabMenu` 已实现锚定、方向键、Esc 与焦点恢复 | 服务器菜单、终端更多菜单和溢出列表优先复用其交互模型和样式 |
| `useTabStrip` 已能检测真实溢出并显露活动项 | 终端标签可沿用其测量思路，避免仅按标签数量判断 |
| 当前测试有 `.terminal-toolbar strong` 等旧结构断言 | 实施时需改为语义角色与新工作条断言，而不是保留旧隐藏节点 |

## 3. 组件与状态归属

### 3.1 顶端服务器菜单

在 `WorkspaceTabs` 的每个服务器标签里，选择按钮与关闭按钮保持同级，再增加一个独立的“服务器菜单”按钮。该按钮显示 `ChevronDown`，但不嵌套在 tab 按钮内；`onPointerDown` 不触发服务器标签拖动。右键服务器标签继续打开对应服务器的上下文菜单，并共享“工具连接 / 端口转发”动作定义。

`WorkspaceTitlebar` 扩展服务器菜单所需的只读展示和动作回调，由 `App` 与 `DetachedApp` 都传入。菜单包含当前服务器名称、账号和地址、工具状态、配置已更新提示、连接 / 更新 / 取消验证、端口转发及现有服务器操作。先保留主窗口右上角现有的全局端口转发按钮，避免改变全局入口语义；独立窗口也能通过服务器菜单打开当前服务器的转发。菜单打开时按 `tab.id` 固定作用对象；切换、关闭或迁移该服务器后立刻关闭菜单。

`WorkspaceToolConnection` 的验证流程和认证弹窗继续由 `ServerWorkspace` 持有并常驻挂载。它改为“无常驻视觉的控制器”：提供只读 `{ busy, error, promptPending }` 变化通知与 `start/cancel` 命令，工作区原有 `tab.tools` 仍是共享工具连接状态的唯一业务来源。控制器动作由 `ServerWorkspace` 暴露到已有 `WorkspaceController`，`App` / `DetachedApp` 用工作区 ID 找到对应控制器，再向标题栏菜单传递操作；不在标题栏再创建第二套验证 hook。

工具验证的 busy 与错误不写入工作区迁移快照。迁移前已有“验证中不能移动”约束，失败后错误继续留在工作区状态，菜单关闭不清除。菜单被关掉时认证对话框仍按 `promptActive` 和原有队列呈现。任何菜单操作先校验目标 workspace 仍存在且 `lifecycle === active`，再调用控制器。

### 3.2 中央单行工作条

`ServerWorkspace` 成为唯一工作条的组装点，保留 `TerminalTabs` 的已有标签节点、排序和重命名行为。`TerminalTabs` 增加尾部 action slot 或等价明确 props，用于渲染“历史 / 更多 / 状态恢复”操作；不把这些业务行为塞进标签组件内部。工作条对应的 `TerminalSession` 继续按终端 ID 稳定挂载，只用 `hidden` / `inert` 控制可见和交互。

活动终端的状态来自 `tab.terminals` 与该 `TerminalSession` 上报的瞬时视图，按 `terminalId` 保存；不会把一个活动终端的状态误给其他标签。瞬时视图仅含工具栏所需字段，例如连接阶段、自动重连秒数、错误摘要、历史模式、记录异常、剪贴板可用性、清屏反馈。它不存认证内容、终端文本或剪贴板内容，也不进入迁移 JSON。视图上报回调使用稳定引用并比较变化，避免历史轮询、状态播报造成整个工作区频繁重排。

`TerminalSessionController` 继续提供 `disconnect/reconnect/resumeRecovery`，新增或者封装明确的 `cancelConnection`、`presentChallenge` 能力；具体调用现有 `useConnection` 与验证请求，不能新建第二个 SSH 连接尝试。标签状态为等待连接/连接中时显示取消；等待验证时显示“继续验证”并聚焦或重呈现现有对话框；等待重连时显示“立即重连”和可达的“停止重连”；断开或失败时显示“重新连接 / 重试连接”。已连接时不显示恢复按钮。

`TerminalSession` 移除重复的标题行。现有 `connection-recovery` 和 `connection-error` 合并成一个按需出现的状态区域；错误详情仍能通过 `DiagnosticErrorInfo` 展开。连接成功后该区域消失。底部 `SessionConnectionDetails` 和 `ResourceStatusBar` 保留。后台终端异常只标记其标签，不抢占当前终端内容和焦点。

### 3.3 历史与剪贴板

`TerminalWorkspace` 为服务器 SSH 路径增加 `presentation="integrated"`（或同义显式 prop），在该模式下不渲染原 `.terminal-history-toolbar`，但仍持续挂载 `LiveTerminalPane` 和历史轮询。默认独立模式保留容器终端原工具栏与行为。

给 `TerminalWorkspace` 的控制器增加历史切换、全局历史搜索、清屏等明确命令；经 `TerminalSessionController` 转发给 `ServerWorkspace` 的活动终端动作。`TerminalWorkspace` 同时上报历史模式、是否有新实时输出、记录错误、已清理提示，以及 `ClipboardViewState` 中非敏感的 `hasSelection/canPaste/busy/unavailableReason`。服务器工作条历史按钮可直接切换，并用 `aria-pressed` 表示历史模式。记录正常时不占一段“正在保存历史”文字；错误在历史按钮和终端底栏同时可辨认。

“更多”菜单使用独立 `TerminalActionMenu` 或复用 `WorkspaceTabMenu` 的菜单骨架，标题明确为当前终端名称，条目含复制、粘贴、清理显示、搜索全部终端历史、历史状态、连接详情、重命名、断开/重连。菜单命令在打开时固定 `terminalId`，执行前再次核对它仍属当前 workspace 且为可交互的活动终端。右键后台标签只提供该标签安全适用的动作，不能用活动终端控制器替它执行剪贴板动作。

**菜单焦点与剪贴板是重点风险**：现有 `useTerminalClipboard` 以窗口焦点、真实终端会话 ID 和 epoch 判断粘贴资格。打开菜单本身不能调用 `invalidate`、不能清空选区；点击粘贴后先关闭菜单，再用绑定的终端控制器走现有 `requestPaste()`，沿用原生剪贴板读取和多行确认，不自行拼接或发送文本。复制沿用 `copySelection()`；如果菜单的焦点变化使 `canPaste` 临时为 false，不将其误判为断线，应按当前终端目标和已有 `unavailableReason` 验证。切换标签、关闭或迁移时保持现有 epoch 失效机制，阻止晚到的剪贴板操作发送到新终端。

历史只读时，“粘贴 / 清理显示”禁用并说明原因；历史视图已有的选择和复制逻辑照旧。返回实时后恢复对应终端输入焦点。清屏反馈使用现有 `onCleared` 信号转到底栏短提示；不再常驻工作条第二排。

### 3.4 标签溢出与菜单语义

`TerminalTabs` 参照 `useTabStrip` 使用 `ResizeObserver` 和实际 `scrollWidth/clientWidth` 判断溢出，避免当前 `tabs.length > 1` 就显示“全部终端”。测量时需保留溢出按钮的预留宽度，防止按钮显隐后宽度来回振荡；活动项在新建、选择、关闭、重命名及窗口缩放后进入可见范围。

“全部终端”采用有名称、状态、选中项的菜单，不只用无法显示完整长名称的原生 select；复用 `WorkspaceTabMenu` 的键盘导航、Esc 和焦点规则。单个标签也显示新建按钮；无终端时保留新建和空态，“历史 / 更多”禁用。长名称做省略并提供完整 `title` 与可访问名称。终端标签既有方向键、Home/End、F2、Delete、拖动排序以及关闭确认保持可用。

菜单锚定触发按钮打开，按下即时反馈；再次点击可关闭，快速反向点击不锁输入。细微动效只作用于菜单自身，不能逐帧改变终端高度。`prefers-reduced-motion` 使用即时切换或短淡入，`prefers-reduced-transparency` 和高对比模式使用清晰实色表面。

## 4. 文件级修改清单

| 文件 | 修改内容 |
| --- | --- |
| `src/App.tsx`, `src/DetachedApp.tsx` | 向标题栏传递服务器工具展示、按 workspace ID 路由的工具动作与当前服务器端口转发；沿用已有工作区控制器登记 |
| `src/WorkspaceTitlebar.tsx`, `src/WorkspaceTabs.tsx` | 服务器标签增加同级菜单按钮和语义；主/独立窗口共用；控制拖动与菜单焦点 |
| `src/WorkspaceTabMenu.tsx` | 在必要时支持菜单分组、状态文本和动作后焦点策略；维持现有可访问菜单键盘规则 |
| `src/ServerWorkspace.tsx` | 删除 `.workspace-tool-status` 常驻行，持续持有工具验证，组合单行工作条，汇总每个终端瞬时视图与动作路由 |
| `src/WorkspaceToolConnection.tsx` | 将现有状态和命令显式暴露给工作区；保留验证生命周期、认证弹窗和卸载清理 |
| `src/TerminalTabs.tsx` | 真实溢出、活动项显露、尾部操作区、后台标签菜单、无终端规则；保留重命名/拖排 |
| `src/TerminalSession.tsx` | 移除重复标题行；暴露连接动作与瞬时状态；异常区域按需显示、底栏继续保留 |
| `src/TerminalWorkspace.tsx` | 服务器集成模式、历史与剪贴板状态上报、命令入口；默认模式保留容器终端工具栏 |
| `src/terminal-tabs.css`, `src/terminal-history.css`, `src/styles.css` | 统一 40px 基准、标签和图标命中区、溢出、菜单与状态条；删除仅服务于旧 SSH 顶部的占位规则 |
| `src/menus.css`, `src/workspace-windows.css` | 必要的菜单定位、标题栏按钮、深浅主题和无障碍样式 |
| `tests/*` | 调整旧结构断言，增加集中交互、布局、焦点和生命周期回归 |

`src/PortForwardButton.tsx` 原按钮及全局统计逻辑可继续复用；服务器菜单通过现有 `forwardApi.open(serverId)` 打开，无需新后端命令。`src/TerminalPane.tsx`、`src-tauri` 与持久化模型原则上不改；只有原生验证发现现有能力不足时再据实缩小补丁范围。

## 5. 实施顺序和验证

1. **动作与状态接口**：先打通工具连接、终端连接和历史/剪贴板的只读视图与命令，保证每个动作绑定 workspace ID、terminal ID 和现有会话身份；此阶段暂不删除旧 UI，便于对照。
2. **单行工作条与菜单**：完成服务器菜单、终端标签溢出与右侧操作；复用现有菜单导航规则。然后移除三条重复常驻行，并处理条件状态条、底栏反馈。
3. **焦点与响应式**：处理点击/键盘/右键、长名称、无终端、后台终端异常、主/独立窗口、窗口缩放及辅助功能偏好。
4. **回归验证**：运行 `npm run build`、相关 Vitest、`test:connection`、`test:terminal-history`、`test:terminal-clipboard`、`test:workspace-tools` 等适用 Playwright 测试；更新并新增覆盖旧结构改动的断言。之后在真实 Windows Tauri 中验证 SSH、认证、剪贴板、多行粘贴确认、断开/重连和工作区移窗。

测试重点：1、4、12 个终端，320/480/640/900px 中央宽度与放大字体；标签长名称、溢出显隐、切换后菜单对象、关闭确认取消；已连接、排队、验证、自动重连、手动断开、失败及后台异常；历史只读与回实时、记录异常；菜单打开时复制选区和粘贴确认；工具连接更新/取消验证不受菜单关闭影响；文件、Docker、NetBird 和端口转发仍可访问；切换、菜单和历史模式不重建 `.terminal-surface` 或产生多余 SSH 连接。

现有 `tests/desktop.spec.ts` 的 `.terminal-toolbar strong` 断言和 `tests/connection-ui.spec.ts` 的 `.workspace-tool-status` 高度测量需要同步更新。容器终端的 `tests/terminal-history-ui.spec.ts` 则应保留原工具栏断言，作为展示模式隔离的回归证据。

## 6. 完成判定

正常 SSH 状态下中央上方只有一条工作条；所有现有动作有明确可达入口；异常时能直接恢复；不同终端和服务器的操作目标不串台；历史和剪贴板安全规则不退化；主窗口、独立窗口、容器终端都符合各自布局；原生 Windows 验证完成后才认定实施结束。

本文件保留已确认的修改方案和验收约束。实施采用 `integrated` 展示开关，主窗口与独立窗口共用 `useWorkspaceToolActions` 路由服务器菜单动作。

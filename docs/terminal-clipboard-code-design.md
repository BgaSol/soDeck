# 终端复制粘贴代码设计

日期：2026-10-20  
状态：用户已确认，已按本方案实施；实际验证范围见 `docs/terminal-clipboard-verification.md`。  
功能依据：`docs/terminal-clipboard-design.md`

## 1. 实现决策

采用“系统剪贴板适配 + 每个终端实例的交互控制器 + 纯函数策略 + React 展示组件”。保留 xterm 与现有 SSH 输入链路，不重写终端，不增加远端粘贴协议。

所有粘贴入口汇入一个 requestPaste；最终通过 term.paste 触发现有 onData，再调用 terminal_input。只有这条链路发送粘贴数据，不另外调用一次 terminal_input。

确认后按本文完整范围实施，包括三个偏好项、历史复制和独立窗口。分阶段便于检查，每个阶段不是独立交付的缩水版本。

## 2. 已核对的代码约束

1. `LiveTerminalPane` 在 useEffect 内创建 xterm，保存 session、transferring、active 和 interactive 的 ref，onData 调用 terminal_input；没有公共复制粘贴控制器。
2. `TerminalWorkspace` 包装 LiveTerminalPane 并转发迁移控制器。它当前把 interactive 重设为 `mode === "live" && (props.active ?? true)`，遗漏了上层的 props.interactive，应改为三者共同约束。
3. xterm 公开 onData 只给字符串，不提供“键盘输入 / 粘贴 / 终端协议回复”的来源。当前安装版本的 disableStdin 会阻止所有 triggerDataEvent，包括协议回复。不能通过隐藏时设置 disableStdin 或统一过滤 onData 来实现后台输入隔离。
4. 本地 xterm 6.0.0 的 paste 同步产生 onData，内部处理 LF/CRLF 到 CR 的转换，并按 bracketedPasteMode 添加包装。公开 modes 可读取当前 bracketedPasteMode 和 mouseTrackingMode，无需读取 `_core`。
5. Rust terminal_input 验证窗口归属、限制 1 MiB，再送入现有终端队列。返回成功只表示入队。
6. 系统设置 JSON 版本为 1；有原子保存、全局 revision、跨窗口事件和“读取失败不覆盖”的规则。开发者模式重启生效，终端偏好应即时生效。
7. 正常模式通过 browser_behavior 禁用浏览器原生右键菜单；终端需自己的 React 菜单，不能依赖 WebView 默认菜单。开发者模式下终端区域仍以应用菜单为准。
8. 容器窗口入口在 `src/App.tsx` 的 ContainerApp；container_info 目前只返回服务器名和容器名，需要补充预览所需的非敏感来源信息。

## 3. 文件与职责

建议新增 `src/terminalClipboard/`，只拆分有明确责任的模块：

| 文件 | 职责 |
| --- | --- |
| `types.ts` | 公共 action、目标快照、状态和可见能力类型 |
| `policy.ts` | 快捷键映射、粘贴分类、UTF-8 大小、控制字符检测，均为纯函数 |
| `clipboardService.ts` | 桌面插件 / 浏览器适配；统一错误；异步复制写入顺序 |
| `controller.ts` | 单实例操作状态、目标校验、原生事件绑定、请求取消与提交协调 |
| `useTerminalClipboard.ts` | React 生命周期桥接，订阅 controller 状态，挂载/卸载资源 |
| `TerminalContextMenu.tsx` | 菜单呈现、键盘导航、边界定位 |
| `TerminalPasteDialog.tsx` | 预览编辑、统计、控制字符处理、确认与取消 |
| `terminal-clipboard.css` | 菜单、预览和局部反馈样式 |

复用已有终端工具栏与 DialogHeader。不为此引入通用命令总线、状态机库、全局快捷键插件或额外编辑器依赖。预览首版使用受控 textarea，最大输入受 1 MiB 限制；统计不为每个字符创建 DOM。

现有文件改动：

| 文件 | 变更 |
| --- | --- |
| `src/TerminalPane.tsx` | 挂载 controller；复制快照；受控粘贴提交；连接代次和失效通知 |
| `src/TerminalWorkspace.tsx` | 工具栏与弹层；转发操作；修正 interactive 合成 |
| `src/SessionWorkspace.tsx` | 从当前会话的来源快照提供预览标签 |
| `src/App.tsx` | ContainerApp 传入容器来源信息 |
| `src/TerminalHistoryView.tsx` | 统一复制服务；正文选区与记录范围优先级 |
| `src/TerminalReplayView.tsx` | 复用只读复制绑定；不创建粘贴提交能力 |
| `src/SystemSettingsDialog.tsx` | 增加终端偏好；支持直接定位该设置分区 |
| `src/systemSettingsApi.ts` | 新增偏好类型、平台字段与保存方法 |
| `src/useSystemSettings.ts`（新增） | 每窗口共享快照订阅、按 revision 丢弃旧结果、焦点刷新 |
| `src-tauri/src/system_settings.rs` | 新偏好字段与局部保存命令，复用原子保存机制 |
| `src-tauri/src/lib.rs` | 注册剪贴板插件、新设置命令；扩展 ContainerInfo |
| `src-tauri/capabilities/terminal-clipboard.json`（新增） | main / workspace-* / container-* 的文本读写权限 |
| `src-tauri/capabilities/history-clipboard.json`（新增） | terminal-history 的文本写权限 |
| 依赖清单及锁文件 | 添加 Tauri clipboard-manager 对应 JS / Rust 稳定兼容版本 |

实时终端 capability 指定 `clipboard-manager:allow-read-text` 和 `clipboard-manager:allow-write-text`；独立历史 capability 只指定后者。现有 `desktop.json` 不直接给所有工具窗口添加剪贴板读权限。文件面板的远端文件剪贴板保持独立，不接入本功能的文本服务。

## 4. 公共接口

以下为拟实现的接口草案，字段名可在实现时按现有风格微调，语义保持一致。

```ts
type TerminalPlatform = "windows" | "linux" | "macos";
type ShortcutMode = "platform" | "convenient" | "traditional";

interface TerminalClipboardPreferences {
  shortcutMode: ShortcutMode;
  copyOnSelect: boolean;
  rightClickPaste: boolean;
}

interface TerminalSourceLabel {
  serverName: string;
  host: string;
  port: number;
  username: string;
  containerName?: string;
}

interface PasteTarget {
  instanceId: string;        // xterm 实例 UUID
  sessionId: string;         // 真实已就绪 session
  connectionEpoch: number;   // 每轮连接/attach 生命周期递增
  eligibilityEpoch: number;  // 切走再切回来也使旧操作失效
  windowLabel: string;
  source: TerminalSourceLabel;
}

type PasteOrigin = "shortcut" | "native-event" | "menu" | "toolbar";
type PasteRequest =
  | { origin: PasteOrigin; kind: "clipboard"; forcePreview?: boolean }
  | { origin: "native-event"; kind: "text"; text: string; forcePreview?: boolean };

interface TerminalClipboardViewState {
  hasSelection: boolean;
  canPaste: boolean;
  unavailableReason?: string;
  busy: boolean;
  notice?: { kind: "success" | "error" | "info"; text: string };
}

interface TerminalClipboardActions {
  copySelection(): Promise<void>;
  requestPaste(request: PasteRequest): Promise<void>;
  selectAllBuffer(): void;
  sendControl(key: "C" | "V"): Promise<void>;
}
```

将 actions 作为现有 TerminalController 的一个 `clipboard` 子接口，保留 prepareMove / cancelMove / clearDisplay。通过 onClipboardState 回调把轻量状态推送到 TerminalWorkspace，避免工具栏自行读取 xterm 内部状态或轮询。

预览和菜单状态可以由 hook/controller 管理并通过渲染模型交给 Workspace，文本只在当前实例内存中保存，不进入顶层工作区 store、会话迁移快照或日志。

只读回放使用独立窄接口 `{copySelection, selectAllBuffer}`；类型上不持有 session submit 函数，避免靠按钮禁用保证只读。

来源标签在建立本次连接时固定，不能直接引用随后被用户编辑的服务器配置。重连生成新标签和 epoch。ContainerInfo 从已有 pending spec 的 target 取 host/port/username，复用连接目标，不返回密码、私钥或整份 spec。来源信息尚未准备好时禁用预览/粘贴并说明原因。

## 5. 剪贴板适配与权限

桌面使用官方 `@tauri-apps/plugin-clipboard-manager` 与 `tauri-plugin-clipboard-manager`，仅使用 readText / writeText；浏览器开发预览使用 navigator.clipboard。

```ts
interface TextClipboard {
  readText(): Promise<string>;
  writeText(text: string): Promise<void>;
}
```

桌面插件失败不静默降级到浏览器 API，保持可诊断行为。错误只输出操作类型与整理后的错误，不带原文。

原生 paste 事件带文本时直接用 event.clipboardData 的 `text/plain`，不再异步读取一次，避免剪贴板在两次读取间变化。没有可用事件文本时，只有明确的菜单/快捷键动作才读系统剪贴板；图片或文件格式不转换成终端内容。

同一窗口的复制动作由服务维护顺序：先登记请求序号，再异步取得文本（历史范围可能较慢），按登记顺序执行写入，失败不能卡住后续请求。复制选区时已同步截取文本。服务接受有界队列，选中即复制的待处理自动请求可合并为最新一次；显式复制不合并。

跨窗口按各次真实写入顺序更新系统剪贴板，不承诺跨窗口全局提交顺序。取消逻辑不能撤销已经完成的系统剪贴板写入，因此失效检查在写入开始前执行，完成后只取消过期 UI 反馈。

## 6. 事件所有权：一次动作只处理一次

### 6.1 快捷键

`policy.resolveKeyAction(event, platform, preferences, hasSelection)` 返回 copy / paste / clearSelection / passthrough。

- 只在焦点属于该终端且事件不处于 IME composition 时解析。
- 处理 keydown。属于应用快捷键的自动重复也阻止默认行为，但不创建新操作。
- 命中后同步 preventDefault + stopPropagation，并让 attachCustomKeyEventHandler 返回 false；不在异步剪贴板操作之后才阻止默认行为。
- 相应 keypress/keyup 不再派生复制粘贴动作；确认不会被 xterm 二次解释。
- Ctrl+A、常规远端按键保持 passthrough；窗口级监听不拦截其他输入框。

### 6.2 原生 copy / paste

在 xterm 宿主上使用原生 capture 监听，安装早于终端目标节点处理器。只处理 event target 属于当前 xterm 的事件，不接管预览 textarea。

- paste：同步阻止默认动作并 stopImmediatePropagation，取纯文本，调用 requestPaste。这样 xterm 自带 paste listener 不会同时发送。
- copy：有效终端选区走同一复制快照规则，阻止默认处理，避免 WebView 和插件各写一次。
- 键盘入口已经 preventDefault，正常不再产生原生 paste；显式原生 paste 路径不额外发键盘动作。用真实 DOM 测试验证，不按“500ms 内相同文本”去重。
- 首版不劫持普通 Ctrl+X，不提供剪切终端输出操作。

### 6.3 右键和选择

右键是否由本机处理，依据 modifier、当前 mouseTrackingMode 和偏好决定。在 pointerdown/mousedown 捕获阶段确定归属，防止已经发给远端后又执行本机菜单。处理 contextmenu 时只呈现既定动作，不再发送第二次粘贴；键盘触发的 contextmenu 没有前置鼠标事件，单独按本机菜单处理。

鼠标跟踪开启时普通右键交给远端；Shift+右键仅开本机菜单。设置 rightClickSelectsWord=false，避免右键改选区；macOS 强制本机选择按已安装 xterm 的公开选项设置。

选中即复制记录真实 pointer 选择手势，在 pointerup 后读取最终选区；双击/三击使用同一结算路径。onSelectionChange 只更新按钮能力，不直接写剪贴板。键盘全选、恢复快照、程序性选区变化不触发自动复制。

## 7. 粘贴状态机与取消

```text
idle → reading → analyzing → preview → submitting → idle
                  └───────────────→ submitting
任一提交前状态 → cancelled → idle
读取/分析/提交失败 → error → idle（保留轻量错误反馈）
```

每次请求登记 operationId、PasteTarget、偏好快照。controller 只允许一份在途粘贴；额外请求给出“正在处理粘贴”，不排队跨会话执行。

目标有效需同时满足：

- 同一个 instanceId / sessionId / connectionEpoch / eligibilityEpoch / windowLabel。
- active=true、interactive=true、非迁移、连接已 ready。
- 当前原生窗口仍激活；页面未进入 hidden；没有外部模态界面占用输入。

从实时切历史、活动标签切换、面板隐藏、窗口失活、连接关闭、重连开始、prepareMove 或 unmount 时同步增加 eligibilityEpoch 并取消提交前操作。不能只在完成时比较布尔值，否则“切走又切回来”会错误恢复旧操作。

窗口失活使用 Tauri onFocusChanged 为桌面主来源，window blur / visibilitychange 为辅助；不把 textarea blur 当窗口失活，否则打开自己的菜单/预览会误取消。原生 focus 订阅异步建立，卸载后返回的 unlisten 也立即释放。

异步读取不能真正终止时，逻辑取消并丢弃迟到结果；旧 finally 只能结束自己的 operationId，不能清掉新操作。预览确认前、读取后和实际提交前分别检查有效性。

进入 submitting 后已经发出的输入不能撤回。失活或断开只禁止未来提交并清理界面，不把已发输入报告成“已取消且未发送”，也不自动重试。

## 8. 文本策略与边界

```ts
interface PasteAnalysis {
  utf8Bytes: number;
  normalizedBytes: number;
  lineCount: number;
  hasLineBreak: boolean;
  hasTrailingLineBreak: boolean;
  controlCount: number;
  needsPreview: boolean;
}

analyzePaste(text: string): PasteAnalysis;
removeBlockedControls(text: string): string;
removeTrailingLineBreaks(text: string): string;
```

- CRLF 作为一次换行计数；孤立 CR、LF 都触发预览。只有末尾换行也触发。
- 空字符串表示无文本；空格和 Tab 不被 trim 掉。
- 首版阻止未经处理的 U+0000–U+001F（保留 Tab / LF / CR）与 U+007F。不擅自清理其他 Unicode 内容。
- 大于 16 KiB 的文本触发预览；以原文本 UTF-8 字节数判定预览阈值，提交限制以最终规范化数据计算。
- 最终大小 = `TextEncoder(normalizeLineEndings(text)).length` + 当前 bracketedPasteMode 对应的 12 字节包装开销。等于 1 MiB 允许，大于拒绝。
- normalizeLineEndings 的策略与已安装 xterm 保持一致，只用于预算；实际文本交给 term.paste，由 xterm 转换一次。提交时实际 onData 字节再验一次，Rust 保留最终限制。
- 预览编辑后重算；控制字符未处理或超限时禁用确认。移除控制字符是显式操作，实际内容变化可见。取消不回写系统剪贴板。
- 不分块，不静默截断，不自动补 Enter，不自动去掉提示符、缩进或末尾换行。

## 9. 与现有输入链路衔接

为了得到粘贴 IPC 的真实成功/失败，不能让 term.paste 调用完就显示成功。设计一个很小的同步提交桥，位于 LiveTerminalPane，复用现有 onData 订阅：

```ts
// 概念流程；不是最终可直接粘贴的实现代码。
async function submitPaste(target, text) {
  assertCurrentTarget(target);
  assertFinalSize(text, term.modes.bracketedPasteMode);
  const ticket = createPasteTicket(target);
  currentPasteTicket = ticket;
  try {
    term.paste(text); // 当前安装版本同步触发唯一一次 onData
  } finally {
    currentPasteTicket = null;
  }
  assertExactlyOneEmission(ticket);
  assertCurrentTarget(target);
  await sendTerminalInput(target.sessionId, ticket.data);
}

term.onData(data => {
  if (currentPasteTicket) {
    // 只捕获本次粘贴的最终数据，不在回调中先行发送。
    captureTicketData(currentPasteTicket, data);
  } else {
    forwardExistingTerminalData(data);
  }
});
```

ticket 仅在同步调用栈内有效，没有 await 间隙，不会把后来的按键或协议回复归入粘贴。捕获结束后先验证恰好一次发射、实际字节数与目标，再调用现有 sendTerminalInput 包装函数一次；该函数复用 terminal_input IPC。不使用 xterm 私有属性或 monkey patch。没有 onData、出现额外发射或验证失败时明确失败，不用另一条 IPC 路径补发。已有输入流不新增自动重试。

该行为依赖已安装 xterm 的同步 paste 语义，用集成测试固定；依赖升级导致变化时先修改适配设计，再升级。term.paste 返回 void 不能被当作远端确认。

“发送按键”通过同一个受控用户输入提交函数发固定的 `\x03` / `\x16`，不读取剪贴板、不使用 paste 包装，仍校验当前目标。

后台输入处理边界：

- 把 interactive 合成为 `mode === "live" && (props.active ?? true) && (props.interactive ?? true)`。
- 隐藏实时层使用 inert、失效时 blur；用户快捷键、鼠标、原生 paste 与受控菜单入口进行资格检查。阻止针对非交互终端的用户 DOM 输入事件进入目标处理器。
- 不给实时终端统一设置 disableStdin，不在无来源的 onData 上简单按 active 丢弃所有数据，以免破坏后台 DSR 等终端协议回复。
- IME 输入切换需要专项测试，尤其 compositionend 与 xterm 延迟处理；不能假设所有用户输入都经过 keydown。若发现残留组合输入，需在 DOM 输入适配层解决并重新验证，不以屏蔽全部协议回复规避。
- 历史回放仍保持 disableStdin=true，且不注册任何 onData → IPC 桥。这与后台实时终端是不同场景。

Rust 端继续校验窗口归属，不增加客户端可以绕过的替代粘贴命令；本轮不重构 terminal.rs 的输入队列或 SSH 发送机制。

## 10. 设置与跨窗口一致性

在版本 1 设置文件中新增带 serde(default) 的 terminalClipboard 对象，兼容旧文件；其他字段仍保留。推荐持久化：

```json
{
  "version": 1,
  "developerMode": false,
  "terminalClipboard": {
    "shortcutMode": "platform",
    "copyOnSelect": false,
    "rightClickPaste": false
  }
}
```

platform 模式在 Windows 解析为 convenient，在 Linux 解析为 traditional；macOS 固定使用 Cmd+C/V，界面不展示无意义的 Ctrl 快捷键开关。平台值由 Rust cfg 返回到 SettingsSnapshot，浏览器测试通过 fixture 注入。

保留 `set_developer_mode` 以兼容现有调用，新增：

```ts
set_terminal_clipboard_settings({
  preferences: TerminalClipboardPreferences,
  expectedRevision: number
}): Promise<SystemSettings>;
```

Rust 两个保存入口复用一个锁内“校验 revision → 更新对应字段 → 原子写文件 → revision+1”的实现。更新终端偏好保留 developerMode，反之亦然；不允许用旧快照覆盖整份配置。无实际变化不增加 revision。

保存成功广播已有 system-settings-changed。终端偏好立即应用；developerMode 的 active 状态仍按进程启动时固定，不能因提取公共保存函数改变其语义。加载错误保持禁止保存，错误文案改为覆盖全部设置的通用说明。

`useSystemSettings` 每窗口只建立一份订阅，先监听再取快照，忽略迟到的低 revision，窗口聚焦时补刷新；最后一个订阅者卸载时清理。保存冲突刷新显示已提交值，提示用户核对重试，不自动覆盖他人修改。

三个设置开关均立即保存；正在保存时禁用本分区，避免连续提交制造冲突。复制粘贴设置入口直接打开已有 SystemSettingsDialog 并定位终端区，无需新增独立设置窗口。

## 11. 历史、回放与反馈

历史阅读按焦点和选区决定：

1. 焦点是 input / textarea / contenteditable，保持原生编辑行为。
2. 有属于正文区域的非空 DOM Selection，复制选区纯文本。
3. 无正文文本选区，但存在选定记录范围，调用原 api.copy 获取完整范围再走统一写入服务。
4. 两者都没有，不覆盖剪贴板。

历史范围异步请求固定 sessionId / fromId / toId；切会话后迟到结果不写入。跨虚拟列表未加载部分仍由历史 API 读取，不能以 DOM 文本代表完整范围。

回放 xterm 使用同一键盘策略中的只读子集与菜单；提供复制、全选，不提供粘贴或发送按键。禁止把回放内容中的控制序列解释为剪贴板操作，不新增 OSC 52 处理器。

复制成功反馈保留约 1.5 秒，通过 role=status 宣读，失败保留直到用户关闭或新操作替换。普通粘贴不显示“命令执行成功”。菜单/预览关闭后只有目标仍有效才恢复焦点。

## 12. 测试与实施步骤

### 12.1 纯函数与控制器测试

新增 `src/terminalClipboard/policy.test.ts`、`controller.test.ts`：

- 各平台快捷键、有无选区、便捷模式关闭、IME、repeat、修饰键冲突。
- CR / LF / CRLF、只有末尾换行、空字符串/空白、控制字符、中文与 emoji 大小边界。
- 1 MiB 的最终包装边界及 16 KiB 预览边界。
- 延迟读取时断线、切走再切回、重连、迁移、销毁；旧结果不能提交或清除新操作。
- 预览编辑/取消、重复确认、读取失败、IPC 失败；不自动重发。
- 复制写入顺序，失败后队列继续；历史会话变化使待写入请求失效。

Rust 在已有 system_settings 测试模块中验证旧文件默认值、偏好持久化、两个保存入口互不覆盖、revision 冲突、写入失败不改内存、坏文件不覆盖，以及开发者模式仍重启生效。

### 12.2 浏览器集成

新增 `tests/terminal-clipboard-ui.spec.ts` 与独立 Playwright 配置，沿用项目已有模拟 Tauri IPC 的方式，但使用真实 xterm 渲染和事件处理。

重点断言实际 terminal_input 的次数与字节：

- 每个入口只提交一次，有选区复制不会发送 Ctrl+C。
- 原生 paste 不绕过预览，也不会被 xterm 默认监听器重复发送。
- 模拟远端开启/关闭 bracketed paste，断言准确包装。
- 工具栏、右键、选择和预览焦点行为。
- 隐藏实时终端不能接收用户粘贴；同时远端 DSR 仍有正确协议回复。
- 回放和历史正文粘贴产生 0 次 terminal_input，搜索输入框仍可粘贴。
- composition 期间切换历史/标签，确认没有延迟用户输入泄漏。
- 开发者模式开/关都能显示终端自定义菜单。

现有 terminal-history 与 system-settings 套件做针对性回归；更新新增字段所需 fixture，避免仅为测试通过放宽真实类型。

### 12.3 原生桌面验收

先在当前 Windows 打包环境验证真实剪贴板读写、Ctrl+C/V、右键、中文 IME、多窗口、系统剪贴板忙和断线。再验证 macOS Cmd 操作与强制选择、Linux 快捷键及鼠标行为。

浏览器模拟测试不能宣称原生平台通过。无可用环境的平台记录“待验证”，不编造完成结果。

### 12.4 实施顺序

1. 设置数据结构、平台信息、剪贴板插件与权限；补对应 Rust 测试。
2. policy 和实例 controller；接入 LiveTerminalPane 的单次提交桥及状态失效。
3. 菜单、工具栏与预览；确认真实 xterm 事件不重复。
4. 历史/回放只读复制、容器来源、全部偏好与设置同步。
5. 运行相关 Vitest、Playwright、Rust 测试与前端构建；完成 Windows 原生实测并记录范围。

完成标准：功能方案中的操作规则都有对应实现和可观察验证；没有自动执行、内容截断、重复粘贴或过期目标重投；既有终端协议响应与会话迁移保持可用。

## 13. 参考与确认范围

本文依据当前仓库源码与本地 xterm 6.0.0 实现；官方文档用于核对公开接口及权限命名：

```text
https://xtermjs.org/docs/api/terminal/classes/terminal/
https://v2.tauri.app/plugin/clipboard/
```

用户确认本文后，授权范围是上述应用代码、依赖、权限配置、测试与必要文档的本地实现。发布、合并与推送不属于这份代码设计的动作。


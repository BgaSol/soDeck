# 文件编辑页面：代码修改方案

状态：**用户已确认，前端实现与浏览器验证已完成（2026-10-21）；原生实机验收待补。** 下文保留实施前的方案与接口示意，实际结果和限制见[实施记录](C:/Users/bgaso/Documents/works/soDeck/docs/file-editor-ui-implementation.md)。

对应设计：[输入体验整改方案](C:/Users/bgaso/Documents/works/soDeck/docs/file-editor-ui-refinement-plan.md)。

## 1. 技术决策与范围

采用 **编辑器外观优化 + 查找展示层局部重构**，不重写文件编辑器。

- 保留 CodeMirror、basicSetup、语言加载、撤销历史与现有文档读写流程。
- 正文继续是连续画布，不做大型表单卡片；遵循 apple-design 的内容优先、熟悉性、克制、即时反馈和可访问性。
- 使用 CodeMirror 官方 `search` 的 `createPanel` 接口替换默认面板，搜索及替换算法仍由官方命令负责。
- 不增加 UI 框架、搜索引擎、动效库或依赖升级。
- 不改 Rust、SFTP、IPC 数据格式、1 MB 保存限制、窗口标题栏与公共主题存储。
- 不顺带重构当前 `load` / `save` / 关闭确认 / 冲突处理，不覆盖工作区已有的诊断系统等并行改动。

### 本次核实的两个关键边界

1. 日志页面的根节点也是 `container-logs-app remote-editor-app`，且使用 `.editor-error`。因此不能把这些共享基础样式整体从公共样式文件迁走。
2. 当前 `makeState → load → useEffect(load)` 存在依赖链。把主题作为 `makeState` 的依赖可能触发重新读取远端文件，导致未保存内容被替换。主题必须走独立 Compartment 更新。

## 2. 文件级改动清单

### 业务实现

| 文件 | 改动 | 不承担的职责 |
| --- | --- | --- |
| [FileEditorApp.tsx](C:/Users/bgaso/Documents/works/soDeck/src/FileEditorApp.tsx) | 增加文件编辑页专属根类、接入主题和搜索扩展、订阅主题更新、补充保存快捷键作用域、状态栏布局标记 | 不重写加载、保存、冲突或关闭流程 |
| [file-editor.css（新增）](C:/Users/bgaso/Documents/works/soDeck/src/file-editor.css) | 文件编辑页布局、搜索字段/按钮、状态栏、主题变量、响应式及可访问性样式 | 不定义全局 input/button/select，不影响日志页 |
| [editorTheme.ts（新增）](C:/Users/bgaso/Documents/works/soDeck/src/editorTheme.ts) | 提取现有语法高亮与 EditorView 主题，增加明确的 light/dark 配置 | 不管理应用主题偏好，不订阅 localStorage |
| [editorSearchPanel.ts（新增）](C:/Users/bgaso/Documents/works/soDeck/src/editorSearchPanel.ts) | 搜索面板 DOM、事件、焦点、中文提示、官方搜索命令接入、统计任务生命周期 | 不持有文档副本，不调用文件 API |
| [editorSearchStats.ts（新增）](C:/Users/bgaso/Documents/works/soDeck/src/editorSearchStats.ts) | 使用官方查询游标统计匹配、提供有上限的结果快照与当前项定位辅助 | 不自写正则/整词匹配规则，不进行替换 |
| [files.css](C:/Users/bgaso/Documents/works/soDeck/src/files.css) | 仅迁出已确认属于文件编辑页的工具栏、正文、状态栏、语言控件等规则，移除被替代的默认搜索覆盖 | 保留共享根布局、错误提示和其他功能样式，不清理无关历史规则 |

新根节点保留旧类，增加专属作用域：

```tsx
<div className="remote-editor-app file-editor-app">
```

所有新增外层 CSS 使用 `.file-editor-app …`；CodeMirror 内部规则由 `EditorView.theme` 自动限定作用域。文件编辑页继续导入公共文件样式，再导入专属样式。共享的确认弹窗与错误提示不在本次重设计范围内。

### 测试

| 文件 | 改动 |
| --- | --- |
| [editorSearchStats.test.ts（新增）](C:/Users/bgaso/Documents/works/soDeck/src/editorSearchStats.test.ts) | 统计、上限、查询语义与选择范围测试；使用 EditorState，无需新增 DOM 测试依赖 |
| [editorTheme.test.ts（新增）](C:/Users/bgaso/Documents/works/soDeck/src/editorTheme.test.ts) | 验证暗色 facet 与主题重配置不改变文档/选区/历史状态 |
| [file-editor-fixture.ts（新增）](C:/Users/bgaso/Documents/works/soDeck/tests/file-editor-fixture.ts) | 独立浏览器 IPC fixture，支持加载延迟、失败、保存版本冲突与调用记录；不接真实服务器 |
| [file-editor-ui.spec.ts（新增）](C:/Users/bgaso/Documents/works/soDeck/tests/file-editor-ui.spec.ts) | 搜索、替换、输入法事件、焦点、主题、布局和保存安全回归 |
| [window-chrome-ui.spec.ts](C:/Users/bgaso/Documents/works/soDeck/tests/window-chrome-ui.spec.ts) | 强化主题变化不额外触发 editor_load、不丢撤销历史及关闭保护断言 |
| [system-settings-ui.spec.ts](C:/Users/bgaso/Documents/works/soDeck/tests/system-settings-ui.spec.ts) | 保留现有快捷键保护测试，补充查找字段有焦点时保存可用、Esc 返回正文 |
| [playwright.connection.config.ts](C:/Users/bgaso/Documents/works/soDeck/playwright.connection.config.ts) | 将新增编辑器 UI 测试纳入已有 Chromium/WebKit 项目，不新增端口、脚本或运行框架 |

不计划修改依赖清单和锁文件。已有测试定位若可保持就不改，仅在新增明确无障碍语义时优先使用语义定位。

## 3. 正文外观与主题接入

### 3.1 样式责任划分

`editorTheme.ts` 负责编辑器内部的正文、行号、选区、光标、匹配标记、提示层和面板基础色。原有语法高亮映射原样提取，不趁此更换整套语法配色。

```ts
// 拟定接口；实现阶段以实际类型检查为准。
export const editorSyntaxHighlighting: Extension;
export function createEditorAppearance(theme: Theme): Extension;
```

`createEditorAppearance` 使用 `EditorView.theme(spec, { dark: theme === "dark" })`，让 CodeMirror 的内建提示层和默认规则认识实际主题，而不是只改变 CSS 背景。

专属 CSS 引用现有全局语义变量，必要时补充仅属于文件编辑器的 token：

```css
.file-editor-app {
  --editor-font-size: 0.929rem; /* 保持当前约 13px 的阅读密度 */
  --editor-line-height: 1.65;
  --editor-search-control-height: var(--control-height);
  --editor-search-radius: var(--radius-control);
  --editor-search-gap: 0.571rem;
  /* 字段底色、匹配/当前匹配、边界和焦点色按明暗主题定义 */
}
```

- 正文横向间距以约 12px 为初始值，保持现有垂直留白与行距；不人为加宽行号区。
- 行号与正文低色差，不添加厚框、浮雕、持续发光或大阴影。
- 搜索控件默认约 32px 高，使用 `min-height` 而非锁死高度；圆角沿用项目 token。
- 查找字段用 UI 系统字体；正文保持平台等宽字体。选区与搜索匹配分开表达，不让匹配底色遮蔽文字。
- 输入框仍必须容易辨认；“弱化边框”不能变成完全看不出可输入区域。
- 焦点环、hover、active、disabled、invalid 分别定义；按下即时反馈，不给输入和光标增加动画。
- 增强对比度时提升边界和焦点对比；减少透明度时使用实底。面板本轮默认直接展开/收起，不引入弹跳、滑动或高度动画。

### 3.2 主题生命周期

在组件内新增稳定的 appearance Compartment。`makeState` 创建文档时直接读取 `getTheme()`；保持其稳定依赖，不加入主题 state。

```ts
// 接入示意，非本轮已实现代码。
appearance.current.of(createEditorAppearance(getTheme()));

// 独立 effect 订阅现有主题源。
return subscribeTheme(() => {
  const view = editor.current;
  if (!view) return;
  const next = getTheme();
  if (view.state.facet(EditorView.darkTheme) === (next === "dark")) return;
  view.dispatch({
    effects: appearance.current.reconfigure(createEditorAppearance(next)),
  });
});
```

- 初次加载还没有 EditorView 时不补造实例；之后 `makeState` 读取最新主题。
- 系统、手动和跨窗口主题变化共用既有 `getTheme` / `subscribeTheme`。
- 同一实际颜色下改变主题偏好不重复重配置。
- 只允许真实读取/重新加载流程使用现有 `setState`；主题、展开面板、切换匹配选项不得调用它。
- 组件卸载取消订阅；主题更新不聚焦编辑器、不抢走正在输入的查找字段焦点。

## 4. 查找面板：保留引擎，只替换展示层

### 4.1 接入与状态所有权

```ts
export function createEditorSearchPanel(view: EditorView): Panel;
export const editorSearchExtension: Extension;

// 扩展示意
search({ top: true, createPanel: createEditorSearchPanel });
```

自定义 Panel 本身也必须返回 `top: true`；不能以为仅配置 `search.top` 就会替自定义面板设置位置。`basicSetup` 继续保留，其已有搜索快捷键不用复制一份。

| 状态 | 唯一来源 |
| --- | --- |
| 文档、选区、撤销记录 | EditorState |
| 查找词、替换词、大小写/正则/整词设置 | CodeMirror SearchQuery / setSearchQuery |
| 面板是否打开 | CodeMirror 搜索扩展 |
| 替换行和匹配选项是否展开、输入法组合中的文本 | Panel 局部展示状态 |
| 数量与当前项 | 当前文档/查询派生的、有版本关联的统计缓存 |
| 文件版本、dirty、保存和冲突 | 现有 FileEditorApp |

不为面板再挂载 React root，不用 React state 复制整套查询，更不维护第二份正文。Panel 只通过官方命令及 effect 影响 EditorState。

### 4.2 DOM 与生命周期

- 根节点保留 `.cm-search` 以兼容现有测试，同时增加 `.file-editor-search` 和 `role="search"`、中文 accessible name。
- 主查找输入保留 `main-field="true"`，确保官方 `openSearchPanel` 能正确定位。
- `mount` 只在首次打开时聚焦并选择查找文本；`update` 更新数值、状态与禁用规则，不反复重建 DOM、不重复 select 文本。
- 输入事件使用 `input`，支持键入、粘贴、清空；组合输入期间保留原始字段值，compositionend 后提交最终查询。
- 处理外部 `setSearchQuery`：例如选中正文后 Ctrl/⌘ F，按照官方规则带入选中文本。构造新查询时保留既有 `literal` 等设置，不偷偷改变 `\\n` 或正则替换的语义。
- `update` 不直接 dispatch 新 transaction，避免在 CodeMirror 更新过程中重入；事件回调负责查询提交，异步统计只更新 DOM。
- `destroy` 清理事件、统计任务和待播报提示；Panel 关闭后迟到结果不得修改新面板。
- 用户输入与文件内容仅通过 value/textContent 展示，不插入 innerHTML，不把查询或文件内容写入日志、localStorage。

### 4.3 交互与可访问性

默认显示“展开替换 / 查找字段 / 数量 / 上一处 / 下一处 / 匹配选项 / 关闭”。替换行和匹配选项采用面板内展开区，不新增浮层或模态窗口。

- 替换区包含替换字段、“替换”和“全部替换”。先前示意的“替换当前”在实现中用“替换”，沿用 `replaceNext` 的原生语义：未选中匹配时先定位，已选中时替换并前进；不暗改为一键替换光标附近任意文本。
- 匹配选项区保留三个有中文 label 的原生 checkbox，以及“选中全部匹配”。展开按钮使用 aria-expanded/aria-controls；收起时仍显示已开启的匹配条件数量，避免隐藏当前模式。
- 图标按钮均有 accessible name 和 title。普通按钮组不伪装成需要方向键导航的 ARIA toolbar/menu。
- 使用 `getSearchQuery` / `setSearchQuery`、`findNext` / `findPrevious`、`replaceNext` / `replaceAll`、`selectMatches`、`closeSearchPanel`。不做字符串 replaceAll、不手动覆盖整篇文档。
- 空查询不报错；非空无效正则才设置 aria-invalid，并通过 aria-describedby 关联明确提示。无结果是普通状态，不显示错误红色。
- 当前文档/查询的统计确认无结果时禁用匹配操作；统计还未完成时不将旧的“无结果”用于新查询。替换还必须检查 `state.readOnly`，事件执行时再检查一次。
- 加载期间沿用现有只读状态；保存期间是否允许继续编辑沿用当前行为，不能简单以 busy 禁用所有输入。
- 替换可以撤销；“全部替换”一次撤销恢复整次事务，验证不与此前普通输入意外合并。

快捷键规则：

| 输入位置/事件 | 行为 |
| --- | --- |
| 正文 Ctrl/⌘ F | 继续调用 openSearchPanel，打开并聚焦 |
| 查找字段 Enter / Shift+Enter | 下一处 / 上一处 |
| 替换字段 Enter | 官方 replaceNext 行为；其他按钮上的 Enter 保持原生激活 |
| Panel 中 F3、Ctrl/⌘ G 等 | 使用 runScopeHandlers(view, event, "search-panel") 复用现有 keymap |
| Panel 中 Ctrl/⌘ S | 把现有保存 binding 的 scope 扩展为 "editor search-panel"，仍调用 saveRef.current |
| Panel 中 Ctrl/⌘ Z | 普通输入字段保留原生撤销，不把其转发为正文撤销 |
| Esc 且匹配选项展开 | 先收起选项并回到展开按钮 |
| Esc 关闭整个查找面板 | 交给 closeSearchPanel，返回正文，不回滚用户已导航到的匹配位置 |
| 输入法 composition 中的 Enter/Esc | 不执行查找、替换或关闭；检查 isComposing，兼容实际 WebView 的组合状态 |

不注册 document/window 全局键盘拦截。替换行收起而焦点仍在其中时，先移回展开按钮。重新打开默认折叠辅助区；搜索查询按官方生命周期保留或以正文选择更新，不增加持久化。

读屏结果状态使用独立且有名称的 polite live region；不在每次光标变化时重复播报。人工跳转的详细播报由 CodeMirror 负责，通过 EditorState.phrases 补充中文。自动计数播报可做短延后，但不延迟实际输入、查询更新或导航。

## 5. 匹配统计：有上限，不阻塞输入

不为显示“2 / 8”额外维护第二套搜索算法。`editorSearchStats.ts` 使用 `query.getCursor(state)`，处理空查询、无效查询、完整统计和截断统计。

```ts
type MatchRange = { from: number; to: number };
type MatchSummary =
  | { kind: "empty" }
  | { kind: "invalid" }
  | { kind: "complete"; ranges: readonly MatchRange[] }
  | { kind: "capped"; ranges: readonly MatchRange[]; lowerBound: number };
```

- 缓存按不可变文档引用和影响匹配的查询条件失效。只改变替换文本不重新扫描；只移动光标只在缓存中定位，不重新遍历全文。
- 建议最多缓存 1000 项，再多探测一项区分“恰好 1000”与“1000+”。截断时显示“1000+ 个匹配”，不显示虚假的完整总数或当前序号。
- 当前选择恰好对应一个匹配时才显示“i / N”；手工把光标移动到别处时显示“N 个匹配”。
- 使用可取消的小批次遍历，查询或文档变化递增 generation；Panel 关闭、文档重载或组件销毁后丢弃旧结果。统计排队不会挡住查询高亮和查找命令。
- 统计暂态显示“正在统计”，不能沿用旧查询的计数；一旦发现匹配即可更新有结果状态，不必等待完整计数。
- 统计上限不限制下一处/上一处或全部替换。已安装版本的官方 `selectMatches` 自身有 1000 项上限：超过上限时明确说明无法一次选中全部，不能按钮无反应，也不擅自提高该限制。
- 正则单次游标执行仍可能耗时，分批统计不等于可中断 JavaScript 正则。测试常见及大文件情形，不宣称解决任意复杂正则阻塞；若发现需要更换搜索引擎或增加 Worker，另行评估并再次确认，不在视觉整改中暗中扩大范围。

## 6. 响应式与状态栏

- 搜索主体用 CSS grid，输入列为 `minmax(0, 1fr)`，按钮列按内容占位。
- 普通窗口搜索一行，替换另起一行；窄窗口改为“主输入/关闭”一行、“计数/导航/选项”一行，替换区同样分行。明确 grid areas，不依赖默认 inline 自然换行。
- 展开区隐藏时不参与布局和 Tab 顺序；展开不定位为覆盖正文的浮层。
- 极短窗口或 200% 字号时允许搜索面板自身纵向滚动并限制其占编辑器的比例，保留可访问的正文区域；输入与按钮通过键盘自动滚入可见区。
- 正文保留内部横向滚动，不新增自动换行设置，不让长代码行撑出页面。
- 底部语言选择保留原生 select、原有 value 和 onChange，仅改为低强调度外观。悬停/键盘聚焦时边界清楚，不能只是一段看不出可操作的文字。
- 状态栏只增加布局 class，保持保存状态、语言、光标/编码/大小三组含义；小窗口允许次要信息换行，不截掉未保存状态。
- 不修改公共标题栏或模仿 macOS 控件。出现公共标题栏的独立缩放问题时单独记录，不以本任务名义扩改其他窗口。

## 7. 验证矩阵与完成标准

### 7.1 自动化

1. 搜索语义：普通文本、中文、大小写、整词、正则、无效正则、空查询、零宽匹配、多行/转义与 CRLF、替换捕获组；空替换用于删除时仍可用。
2. 统计：0/1/1000/1001 项；只移动光标/只改替换词不重扫；切换查询或重载时旧结果不能覆盖新结果。
3. 键盘：工具栏和快捷键打开、Enter/Shift+Enter、输入法组合、选项区 Esc、关闭后焦点归还、查找字段内 Ctrl/⌘ S、输入字段原生撤销、正文 Tab 缩进与可离开路径。
4. 数据安全：主题/面板/语言改变不额外调用 editor_load；未保存文本、选区和撤销记录保留。替换、全部替换可撤销重做；保存中继续编辑后仍正确标记 dirty。
5. 生命周期：加载中的只读替换保护、加载失败、重载确认取消、保存冲突、关闭继续编辑/保存并关闭；重复打开关闭面板不重复监听或触发多次保存。
6. 视觉与布局：浅/深色、普通/高对比、减少透明度/动态效果；1000×700、560×360、200% 文字缩放；长路径、长正文行、长查询、展开替换和选项。
7. 其他页面：日志页共享根布局及错误提示不退化；确认弹窗的公共样式不变。

正文保持滚动位置的含义是“不主动重置 scrollTop/选区”；打开顶部面板会改变可视高度，必要的焦点可见性调整不应被误判为跳动。初次查询、用户导航到结果或真实重新加载属于有意移动。

### 7.2 执行命令

先跑新增单测及定向 UI；实现完整后运行既有套件。以下为待执行命令，不表示当前已通过：

```text
npx vitest run src/editorSearchStats.test.ts src/editorTheme.test.ts
npx playwright test --config playwright.connection.config.ts file-editor-ui.spec.ts window-chrome-ui.spec.ts
npm test
npm run build
npm run test:connection
npm run test:settings
npm run test:logs
git diff --check
```

`test:settings` 现有全局准备调用 Rust browser-policy-fixture，实施时按环境准备 Cargo PATH；本任务不因此修改后端。浏览器 fixture 使用模拟文档，不通过用户真实服务器做写入验证。

最终补充 Windows Tauri 的中文输入法、窗口焦点、快捷键与布局验证；如需保存闭环，使用隔离的本机 SSH/SFTP fixture。Chromium/WebKit 通过不等于 macOS/Linux 实机已验收，受限项必须明确列出。

对比度以实测为准：正常文字目标至少 4.5:1，必要控件边界与焦点至少 3:1；不能只靠颜色或截图主观判断。记录实际截图、命令结果及未覆盖环境，不沿用上一轮 UI 优化的测试数字。

## 8. 实施顺序与最终确认边界

| 阶段 | 工作 | 退出条件 |
| --- | --- | --- |
| A | 建立独立 IPC fixture 和编辑安全回归基线 | 现有保存/关闭/主题基线可复现 |
| B | 样式作用域隔离、提取主题、Compartment 同步、正文与状态栏 | 明暗主题一致，未保存状态及日志页不回归 |
| C | 自定义搜索面板、中文语义、替换展开、键盘与焦点、受控统计 | 查找/替换/撤销及面板生命周期通过 |
| D | 小窗口与可访问性校验、全套回归、实机复核 | 实际结果记录完整，限制明确，无已知编辑数据丢失回归 |

每阶段只操作本方案列明的文件；若必须跨出边界，先说明原因与新增影响，不顺带做大重构。用户未要求时不自动提交、推送或发布。

**本次按用户最终确认实施以上有限的前端改动和测试；未自动提交、推送或发布。**

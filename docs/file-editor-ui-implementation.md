# 文件编辑页面整改实施记录

日期：2026-10-21。状态：前端实现完成，浏览器验证完成；原生实机验收待补。

依据：[设计方案](C:/Users/bgaso/Documents/works/soDeck/docs/file-editor-ui-refinement-plan.md)、[代码方案](C:/Users/bgaso/Documents/works/soDeck/docs/file-editor-ui-code-plan.md)。用户已最终批准实施。

## 实际改动

- **正文保持连续画布**：统一正文留白、等宽字体、轻量行号区、选区和匹配高亮，不添加大型圆角卡片、阴影或输入动画。
- **顶部中文查找面板**：统一字段尺寸、边界和键盘焦点；替换与匹配选项按需展开。保留上一处/下一处、大小写、整词、正则、捕获组替换及选中全部匹配。
- **清晰反馈**：区分空查询、无结果、非法正则与统计中状态；提供当前项/总数和独立读屏播报。按钮保留即时按下反馈，不引入装饰性动效。
- **主题与状态栏**：CodeMirror 内部浅/深色主题同步；语言选择保留原生 select，降低视觉强调；窄窗口允许状态信息换行。
- **响应式与可访问性**：验证 1000×700、560×360、200% 字号、明暗主题、高对比和减少动态效果；超高搜索面板自身滚动，保留正文空间。字段与匹配文字对比度至少 4.5:1，必要边界至少 3:1；焦点色对白/暗色背景分别约 5.27:1/7.39:1。

遵循 apple-design 的内容优先、熟悉性、克制、即时反馈和可访问性。不仿造 macOS 窗口按钮，不为了视觉整改更换编辑引擎。

## 关键防回归设计

1. `FileEditorApp.tsx` 使用稳定的 appearance Compartment，只重配置主题，不因主题变化触发文件重新加载或替换 EditorState；未保存内容、选区和撤销历史保持。
2. `editorSearchPanel.ts` 使用官方 `search({ top: true, createPanel })` 和搜索/替换命令；Panel 本身声明 `top: true`。不另建 React root、正文副本或搜索引擎。
3. 统计使用官方查询游标，按批次执行；最多缓存 1000 项，多探测一项区分精确总数和 `1000+`。光标移动、替换词变化不重新扫描；generation 和清理回调阻止迟到任务更新已关闭面板。统计上限不截断逐项查找或全部替换。超过 1000 项禁用官方本就有上限的“选中全部匹配”，并解释原因。
4. 查找字段支持 Ctrl/⌘ S；保留输入框原生撤销；Esc 返回正文，选项展开时先收起选项。组合输入期间不执行查找、替换或关闭。点击关闭时显式归还正文焦点，兼容 WebKit 不聚焦被点击按钮的行为。
5. 替换执行时再次检查只读状态；沿用原保存、冲突、关闭保护与加载流程，保留 CRLF 保存行为。没有改变 1 MB 限制。
6. 新 CSS 限定 `.file-editor-app`，保留日志页使用的 `.remote-editor-app` 和 `.editor-error` 共享样式。未修改后端、依赖清单、公共主题存储或窗口标题栏。

主要新增文件：`editorTheme.ts`、`editorSearchPanel.ts`、`editorSearchStats.ts`、`file-editor.css`，以及统计/主题单测、编辑器 IPC fixture 和 UI 测试。另在既有窗口及设置测试中补充编辑安全断言。

## 已执行验证

| 命令/范围 | 结果 |
| --- | --- |
| `npx vitest run` | 29 个文件、185 项通过，含本轮新增 9 项 |
| `npx tsc -b --pretty false` | 通过 |
| `npm run build` | 通过；仍有主 bundle 超过 500 kB 的体积警告 |
| `npx playwright test --config playwright.connection.config.ts file-editor-ui.spec.ts window-chrome-ui.spec.ts` | Chromium/WebKit 共 40 项通过：编辑器 24 项、窗口 16 项 |
| `npx playwright test --config playwright.connection.config.ts` | 122 项通过、4 项范围外失败，详情见下 |
| `npx playwright test --config playwright.logs.config.ts` | 15 项通过 |
| `npx playwright test --config .qa/file-editor-settings.config.ts` | 设置前端 14 项通过；此备用配置跳过 Rust 全局准备，不能替代正式设置流程 |
| 本轮代码文件 `prettier --check` / `git diff --check` | 通过；Git 有常规 LF→CRLF 提示 |

编辑器覆盖内容包括：加载失败重试、重载取消、保存冲突、保存期间继续编辑、保存后关闭、只读替换保护、主题/语言切换保留历史、中文和正则查询、无效表达式、替换撤销、组合输入事件、局部保存快捷键、输入框原生撤销、CRLF、键盘离开正文，以及约 936 KB 文件的有上限计数。截图已实际打开检查；修复了替换行压缩裁切、匹配文字对比不足和 WebKit 关闭焦点问题。

### 完整连接回归的 4 项失败

`tests/connection-ui.spec.ts` 中两个测试分别在 Chromium/WebKit 失败：

- `titlebar tab drop targets use live geometry, auto-scroll, and exclude window buttons`（断言第 1336 行）。
- `titlebar tabs accept empty-window drops and reserve macOS controls`（断言第 1362 行）。

实际 IPC 参数比旧的精确对象断言多一个 `diagnosticTraceId`，`token` 和 `beforeId` 均符合预期。这来自工作区既有诊断改动，不涉及本轮编辑器实现；没有顺带修改相关业务或放宽这些断言。完整套件**不是全绿**。

### 正式设置测试的环境阻塞

使用 Cargo PATH 执行正式 `playwright.settings.config.ts` 时，准备阶段下载 `rustls-platform-verifier/0.7.1` 超时，未进入浏览器测试。

备用配置位于忽略目录 `.qa/file-editor-settings.config.ts`，复用既有配置并跳过全局准备，使用已生成的 `.qa/system-settings/browser-policy.json`。相关 `browser_behavior.rs` 和 `browser-policy-fixture.rs` 与 HEAD 无差异。14 项通过仅证明该策略 fixture 下的前端行为，**不证明当前 Rust 生成流程通过**；正式测试配置未改。

## 未覆盖环境与保留限制

- 未启动原生 Tauri QA 应用，未连接真实或本机 SSH/SFTP，也未向用户配置数据库写入测试连接。
- Windows 真实输入法候选过程、原生窗口焦点/快捷键，以及 macOS/Linux 实机仍待验。浏览器 composition 事件测试不等于真实输入法验收。
- 原生隔离构建所需 Rust 依赖下载受阻，后续应在依赖可用后补原生验收；不要直接启动默认 identifier 的应用向用户数据库写入 QA 配置。
- 分批计数不能中断一次同步 JavaScript 正则运算，不宣称解决任意复杂正则的性能风险；更换引擎或引入 Worker 不在本轮范围。
- 工作区已有的诊断系统等暂存改动已保留。未提交、推送或发布。

## 浏览器效果预览

以下均为模拟 IPC 数据，不是原生实机截图。

浅色：

![编辑器浅色浏览器预览](C:/Users/bgaso/Documents/works/soDeck/.qa/file-editor-chromium-light-1000.png)

深色窄窗口：

![编辑器深色窄窗口浏览器预览](C:/Users/bgaso/Documents/works/soDeck/.qa/file-editor-chromium-dark-560.png)

同目录另有 Chromium/WebKit 的浅/深色、宽/窄窗口及 200% 字号预览；`.qa` 为忽略目录，截图不作为待提交源码。

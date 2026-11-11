# 标准库与现有依赖复用：剩余改动设计

状态：七项代码已实施，本地完整回归进行中；五平台原生包验收与发行待完成。核对日期：2026-11-12；设计基线：`7331c9eec75c20b88d888ba6e5d210a4dd4a2f11`。待交付版本见[核心 0.23.5](../releases/v0.23.5.md)及各插件 CHANGELOG。

本文统一维护本轮需求、代码方案、实施顺序和待验收范围。原“表单与工作台局部刷新”计划的有效要求已合入第 9 节，不再维护第二份同主题计划。

上一轮已交付的 SemVer、Markdown 检查、三个工程 CLI、JSON/JSONC 着色及服务器选择器定位不重复实施，交付记录见[核心 0.23.4](../releases/v0.23.4.md)和[文件插件 CHANGELOG](../../plugins/files/CHANGELOG.md)。

## 1. 目标、范围与复杂度预算

目标是修复已确认的交互问题，并减少仍在重复维护的标准实现。采用浏览器、Node、Rust 标准 API 和已有 Floating UI、Monaco；本轮不新增或升级生产依赖。

| 编号 | 完整改动 | 可验收结果 | 复杂度 |
| --- | --- | --- | --- |
| A | 核心表单选区恢复 | 同实体刷新保留支持选区的输入框与 textarea 的焦点、选区和方向 | 低；只修恢复逻辑 |
| B | 文件编辑器菜单定位 | 窄编辑区、缩窗和视口边缘仍可访问全部菜单项 | 中；包含定位、事件与清理 |
| C | 文件编辑器偏好变更 | 修改字体、行高、tabSize、换行和 minimap 时保留本实例选区及视图 | 低到中；一个临时视图快照 |
| D | RPC 有界读取 | 标准库完成字节读取，帧边界、内存上限和各调用方失败策略得到验证 | 低到中；需边界差分 |
| E | 剩余测试 CLI | 严格解析脚本参数，无效参数在资源启动前失败，启动器参数原样透传 | 低；具名解析函数 |
| F | 主题测试读取 | 状态与 Docker 测试使用真实 CSS 级联和已有主题接口 | 低；测试夹具内复用 |
| G | 升级测试流式下载 | 包体流式落盘，完整校验后提交；超时与失败可清理 | 中；两处真实调用共用 writer |

每项独立实施、验证和提交；A/B/C 优先。D 在边界回归确定后实施，E/F/G 独立推进。源码完成、实际包验收和发布分别记录。

复杂度预算：保留现有调用关系；新增成员只用于实际菜单或隐藏编辑器生命周期；新增工程 helper 只服务已存在的调用者。不得扩展为 DOM 框架、统一编辑状态协议、RPC 框架、CLI 注册器或发布状态机。

## 2. 职责、依赖与兼容

| 层 | 负责的代码 | 允许复用 | 必须保留的边界 |
| --- | --- | --- | --- |
| 核心前端 | `src/ui.ts` 与表单调用方 | 浏览器焦点及选区 API | 草稿和实体身份仍由表单控制器管理 |
| 文件插件 | `plugins/files/editor-src/editor.ts` | Monaco 公共 API、随插件打包的菜单定位器 | 编辑、剪贴板、键盘和视图生命周期留在文件插件 |
| 核心 RPC | `src-tauri/src/plugins/rpc.rs` | Rust 标准库 | 核心仍控制帧上限、输出分类、故障与日志策略 |
| 共享客户端 | `crates/plugin-client/src/lib.rs` | Rust 标准库 | 客户端不链接核心，不引入传输或权限框架 |
| 工程脚本 | 参数、主题测试、下载 writer | Node、Playwright 和既有工程代码 | 不进入产品运行资源，不成为核心构建的插件依赖 |

不变更 RPC 字段、核心 API 版本、插件清单或持久 `EditorState.version = 1`。核心与插件继续独立构建、打包和发行。类型引用、测试引用和插件自带 UI helper 不构成插件调用核心前端的运行时接口。

锁文件与依赖清单在本轮应保持不变。若实施发现必须增加生产依赖，先退出当前方案并重新说明用途和成本，不将其混入本轮提交。

## 3. A：核心表单选区恢复

### 现状与原因

[replace](../../src/ui.ts) 销毁并重建 DOM，捕获了选区，但仅对 text/search 恢复。当前源码的 Chromium 探针确认：text/search 保留 `[1,4]`；password、tel、url、textarea 刷新后变为 `[0,0]`，焦点保留但节点均重建。该结果不证明 IME 组合输入得到保留。

### 功能代码计划

1. 在 `src/ui.ts` 中用局部类型判断识别 `HTMLTextAreaElement`，以及 text/search/url/tel/password 的 `HTMLInputElement`；数字、日期等不支持选区的输入不调用 `setSelectionRange`。
2. 仅保存聚焦控件的 id、非空选区起止与 `selectionDirection`；不复制控件值或密码，草稿仍来自调用方。
3. 保留已有渲染去重、details 展开状态、滚动恢复和 summary 焦点逻辑。
4. 刷新后在当前容器内按 id 查找新控件；恢复焦点，再在目标仍支持选区时恢复范围与方向。目标被删除、改为不支持的类型或禁用时不强行恢复。
5. 核对 [服务器编辑器](../../src/servers.ts)、[应用层刷新](../../src/main.ts)及[组件设置](../../src/settings.ts)的真实调用。只调整必要调用，不将所有 `innerHTML` 更新统一改写。
6. 同实体刷新仍使用当前草稿；切换实体、新建、保存、放弃和关闭沿用控制器自己的状态，不把上一对象输入补回新对象。

核心接法示意：

```text
捕获当前容器内聚焦控件
  → 若支持选区，保存 start/end/direction
  → 按现有规则渲染及恢复 details/滚动
  → 查找当前容器的新控件并恢复焦点
  → 若目标支持选区，setSelectionRange(start, end, direction)
```

### 验收

在 [workbench interactions](../../scripts/test-workbench-interactions.mjs) 的已有浏览器夹具中加入参数化回归：五种支持的 input 与 textarea、正向/反向选择、光标、值缩短、控件消失、input 类型改变、无 id、容器外焦点、select/checkbox/number 不抛异常。至少用服务器真实表单流程验证一次刷新或异步返回；保存、放弃、实体切换回归在对应现有 UI 测试完成。

本项完成只代表选区恢复缺陷修复；第 9 节的 DOM/IME 目标仍需另外验收。

## 4. B：文件编辑器菜单复用 Floating UI

### 现状与原因

[showMenu](../../plugins/files/editor-src/editor.ts) 按固定宽度手算坐标，菜单挂在 `overflow:hidden` 的编辑区内。探针中 100×70 编辑区的菜单为 136×140，超右 40 px、超下 74 px，粘贴与全选无法点击；先打开再缩小后四项均被裁切。

独立插件 [index.html](../../plugins/files/ui/index.html) 已在业务脚本前加载 `text-menu.js`，其中包含 [attachMenuPosition](../../src/menu-position.js)。探针将菜单挂到 body 并调用该 API 后，四项均可命中；这不是迁移后的完整键盘、清理或原生验收。

### 功能代码计划

1. 只以 type import 使用现有 [MenuPosition 声明](../../src/menu-position.d.ts)。运行时调用插件自带的 `window.SoDeckMenuPosition.attachMenuPosition`，不从核心运行目录加载模块。
2. `FileEditor` 增加一个 `menuPosition: MenuPosition | undefined`。菜单节点继续由 `contextMenu` 管理，不增加菜单服务。
3. 菜单使用现有 `.sd-menu` 外层和 `.sd-menu-list` 内部滚动层，标记 `data-text-menu="custom"`。保留四个动作、role、禁用规则和视觉样式；去掉依赖编辑区坐标的定位规则。
4. 创建菜单时先关闭旧菜单、聚焦编辑器并完成按钮及事件注册，再将节点挂到当前插件 document 的 body。
5. 鼠标入口使用 `{ x, y, contextElement: host }`；键盘入口保留原 host 左上角加 40 px 的默认点，同样设置 contextElement，由定位器完成视口避让。定位器只负责几何和锚点有效性。
6. 菜单自身以捕获阶段 keydown 绑定现有 `this.key`，保留方向键、Home/End、Esc/Tab；宿主原有编辑快捷键入口保留，Enter/Space 使用按钮原生点击。导航需要滚动菜单内部列表，使聚焦项可见。
7. 为定位器提供“当前菜单仍是此节点、编辑器未销毁且可见”的 `isValid`，失效时仅关闭同一菜单。
8. 等待 `ready` 为 true 后才聚焦第一个可用按钮，并再次检查菜单与 handle 身份；旧 ready 回调不能聚焦新菜单或抢回焦点。
9. `closeMenu` 先清空当前引用，再 dispose 定位 handle、移除菜单监听和节点；操作幂等，防止失效回调重入。关闭、重开和销毁均使用这一条清理路径。
10. Monaco 的 scrollTop/scrollLeft 实际变化时关闭菜单，因为内部滚动不一定触发 DOM 祖先 scroll；隐藏、只读切换、blur 和 dispose 同样关闭。外部关闭不抢焦点，只有菜单动作和 Esc/Tab 明确返回编辑器焦点。
11. 菜单动作仍先关闭、聚焦编辑器，再调用原剪贴板操作。保留模型版本、主选区、活动控件、document 焦点和销毁检查，不更改剪贴板授权或时效语义。

本项主要修改 `editor.ts`、[插件 CSS](../../plugins/files/ui/style.css)和测试；共享定位器或打包入口仅在真实测试暴露缺口时修改，不因为这个消费者重构已有 API。

### 验收

扩展 [file editor UI](../../scripts/test-file-editor-ui.mjs)：夹具通过已有 [packageMenuAssets](../../scripts/plugin-menu-assets.mjs) 在临时目录生成并服务真实菜单 bundle，在编辑器模块前加载，不把产物写回源码。覆盖 100×70 编辑区、开菜单后缩窗、视口四角、极矮视口内部滚动、host 移动、内容滚动、鼠标/键盘打开、禁用项跳过、Home/End、Enter/Space、Esc/Tab、动作后焦点返回；连续重开、ready 前关闭、隐藏、只读、dispose 后无残留节点或旧回调聚焦。保留剪贴板过期和候选只读回归。

独立包中确认 helper 实际存在、加载先于编辑器、CSP/Worker 正常；浏览器夹具不能代替实际包验收。

## 5. C：偏好变更使用 Monaco 公共视图状态

### 现状与边界

[preferencesChanged](../../plugins/files/editor-src/editor.ts) 经自有 `state/restore` 仅恢复主选区。探针确认两个选区在修改字体后变为一个；公开 `saveViewState/restoreViewState` 可保留两个。

跨窗/重开使用的自有 `EditorState` 还含搜索、语言和逻辑滚动锚点，并受[插件 Rust 校验](../../plugins/files/src/main.rs)约束。该格式继续保留；本项只处理同一个 Monaco 实例的偏好变更。

一个等待布局的临时字段有真实用例：隐藏 live 实例收到偏好变化后，显示前宽度还可能改变。补充探针中，原逻辑顶部为 19 行 1 列、偏移 −12 px；隐藏时立即恢复，再从 330 px 缩到 230 px 显示，顶部漂移到 10 行 128 列；有效布局后恢复公共快照则保留原锚点及两个选区。因此选定延迟恢复，不用 raw scrollTop 代替逻辑阅读位置。

### 功能代码计划

1. 使用公开 `ICodeEditorViewState`，不得导入 Monaco 内部实现或把公共 API 返回值拆成另一套私有序列化格式。
2. 同实例首次偏好变更前捕获视图，再更新 model 的 tabSize、编辑器字体/行高/wordWrap 和现有 minimap 布局。
3. 正尺寸、可见的实例在布局后恢复公共视图；不调用会收窄选区的自有 `restore` 覆盖公共恢复结果。
4. 隐藏或零尺寸的 live 实例需要等待有效布局时，最多保存一个私有 `pendingViewState`，配合已有 `pendingState` 保留等待期间的自有状态。重复偏好变更保留首个有效锚点，不从已变动的零尺寸布局反复捕获。
5. 若当前已有跨窗输入的 `pendingState`，以它为恢复来源，不捕获空实例的公共视图覆盖它；继续执行已有搜索、语言和主选区恢复。
6. 有效 `layout` 完成后将两个字段取到局部变量并清空：本实例公共快照用公共恢复，跨窗快照用原自有恢复。沿用现有 position 通知，不新建恢复状态机。
7. `state()` 对外始终返回版本 1 的自有结构；公共快照不进入 JSON、RPC、磁盘或跨窗 payload。dispose 清理临时引用。
8. 保持顶部可见文本的逻辑位置及行内像素偏移、合法范围内的横向滚动、搜索开关与查询、语言、撤销栈、只读和焦点语义；公共视图不负责插件自有搜索和语言状态。

```text
preferencesChanged：
    若没有 pendingState：
        pendingState = 当前自有 EditorState
        pendingViewState = saveViewState()，null 归为无公共快照
    更新 model/options/preferences
    layout()

layout：
    隐藏、已释放时返回；保留等待状态
    执行原 minimap/overview 调整及 editor.layout()
    宽高为正且有 pendingState 时：
        取出并清空 pendingState/pendingViewState
        有公共快照 → restoreViewState
        无公共快照 → 原 restore(EditorState)
```

### 验收

在现有编辑器回归中覆盖单选区、反向选择、两个及多个选区，分别调整字体、行高、tabSize、wordWrap、minimap；比较正文、撤销结果、逻辑顶部位置及横向滚动。覆盖搜索栏聚焦、隐藏期间多次偏好变更、零尺寸后首次布局、跨窗候选加载与回滚、重复显示和 dispose。

跨窗格式继续按当前契约验收；本项不承诺跨窗持久化多选区。

## 6. D：RPC 字节读取改用 Rust 标准库

### 代码范围与选定语义

修改 [核心 read_frame](../../src-tauri/src/plugins/rpc.rs)和[客户端输入循环](../../crates/plugin-client/src/lib.rs)。[storage 日志读取](../../src-tauri/src/storage.rs)作为 read_frame 调用方参与回归。不共享核心 crate、不提取协议框架。

标准 `read_until/skip_until` 会自动重试 `Interrupted`，旧 `fill_buf()?` 立即返回该错误。本方案采用标准行为，单独写入回归；不为逐错误等价增加拦截适配器。其他 I/O 错误继续传播。

核心算法示意：

```text
cap = limit.checked_add(1)，失败时在读取前返回 InvalidInput
临时借用 reader，take(cap)，read_until(LF, bytes)
若 bytes.len() > limit：
    若尚未读到 LF，skip_until(LF) 排空本行；非 Interrupted 错误传播
    返回 InvalidData
若未读到字节，返回 None
否则返回完整 bytes，包括已有的 LF/CRLF 或合法 EOF 尾帧
```

客户端提取本模块私有 `read_control_line(&mut impl BufRead) -> io::Result<Vec<u8>>`，以便直接测试实际读取路径。固定 MAX 使用 `take(MAX + 1).read_until`，run 保留空输入结束、超长立即退出及后续 JSON/信封/分派检查；不排空等待超长行。核心为了 stderr 可继续读取而排空，不能把客户端停机策略统一成核心策略。

take 限制累计读取字节为 limit+1，不保证 Vec 的分配容量严格等于协议上限；核心仍可能等待超长行排空，此次不新增读取超时或改变停机策略。

| 情形 | 预期行为 |
| --- | --- |
| 空 EOF | 无帧，结束读取 |
| 字节数恰好 limit，含 LF 或无 LF 的 EOF | 接受 |
| limit+1，最后一个字节是 LF | 拒绝，下一行未被消费 |
| limit+1，尚未到 LF | 核心排空本行后拒绝；SDK 立即失败 |
| CRLF | 两个字节均计入上限，不提前 trim |
| UTF-8 或非法 UTF-8 | 按字节计数；JSON/诊断转换仍归原调用方 |
| stderr 超长后有正常行 | 记录省略提示，继续读取正常行 |
| stdout 或 SDK 超长 | 停止当前通信，沿用故障/退出逻辑 |
| 日志文件超长或读取错误 | 结束当前文件，沿用原文件遍历行为 |
| Interrupted 后可继续 | 标准库重试；单独覆盖，不能宣称与旧代码逐错误相同 |
| 其他 I/O 错误 | 返回原错误，保留已消费前缀事实 |
| limit=0 / 加一溢出 | 非空帧拒绝 / 读取前 InvalidInput |

### 实施与验收

先为原实现建立小 buffer、多分片、连续帧、EOF、超长、CRLF、错误注入的差分基线，再替换。使用 BufReader 包裹可脚本化分片和错误的 Read，比较返回值、错误种类/文本及未消费尾部；Interrupted 和不可达的极限 limit 单独按上表验收。SDK 另断言超长后不会观察后续错误事件。旧循环仅作为开发时差分参照，提交长期行为回归，不保留另一套生产或长期测试实现。

扩展核心 rpc 单元测试、客户端单元测试及 [lifecycle](../../src-tauri/tests/lifecycle.rs) 的超长进程回归；增加 stderr 超长后正常行和日志文件截断范围的测试。保留现有初始化、shutdown、写入队列与进程故障测试。

## 7. E/F/G：工程代码计划

### E. 剩余测试 CLI

在范围明确的 `scripts/test-cli-arguments.mjs` 中提供三个纯具名解析函数，服务 [OpenSSH](../../scripts/test-openssh.mjs)、[终端输入](../../scripts/test-terminal-input.mjs)和[正式 UI](../../scripts/test-production-ui.mjs)。这不是通用注册器；固定位置参数和其他 launcher 透传入口不迁移。

函数边界为 `parseOpenSshTestArgs(args) -> { flags, launcher }`（launcher 为 null 或原样参数数组）、`parseTerminalInputTestArgs(args) -> { desktopScript }`、`parseProductionUiTestArgs(args) -> { updatesOnly }`。默认分别为无开启 flags/无 launcher、无脚本路径、false；运行时模式推导继续留在调用脚本中。

- OpenSSH：按第一个字面量 `--packaged` 分割；仅前段交给 `util.parseArgs`，后段至少有一个 launcher/executable，并逐项保留。禁止重拼 shell 字符串或解释后段的 `--`、`-a`、同名脚本开关。
- 布尔开关保留 terminal、files、network、system-status、communication、files-ui-only、network-ui-only、file-editor-native、close-native。保持 communication 推导、既有普通测试优先级和 packaged 中 file-editor-native 优先于 close-native；不新增互斥限制。
- 终端输入：`desktop-script` 为字符串，缺值或空值明确失败，重复值保持第一个值策略。
- 正式 UI：`updates-only` 为布尔值，替换全局 includes；此次追加纳入同一 CLI 阶段。
- 所有解析发生在临时目录、SSH key、sshd、Vite/浏览器和 fixture 构建之前。未知开关或非预期位置参数立即失败；模块导入无 I/O 和构建副作用。

扩展 [cli arguments 回归](../../scripts/cli-arguments.test.mjs)：默认与全部合法开关、组合、重复、缺值、未知值、中文/空格路径，以及完整 launcher tail。用真实脚本无效输入验证“资源启动前失败”。

### F. CSSOM 主题快照

新增仅供测试的 `scripts/test-theme-tokens.mjs`，两个真实消费者是 [system status UI](../../scripts/test-system-status-ui.mjs)与[Docker UI](../../scripts/test-docker-ui.mjs)。

helper 直接导入自包含的 [themeTokens](../../src/theme.ts)；锁定 Node 24.22.0 可读取此 TypeScript 文件，无需新增转译器。创建短命 Playwright page，依次加载 layer 声明、[ui.css](../../src/ui.css)、[themes.css](../../src/themes.css)，切换 light/dark/dopamine 后用 `page.evaluate(themeTokens)` 读取三份快照，finally 关闭页面。

对外只提供 `readTestThemeTokens(browser)`，返回按三个主题名索引的 token 快照；浏览器由现有测试创建与释放，helper 只拥有其短命页面。

两个测试继续使用现有插件静态 HTTP 夹具；Docker 初始 mount 使用 light 快照，状态测试保留初始空 tokens 的默认样式覆盖，再发送主题变化。不增加核心构建或插件运行时对源 CSS 的依赖。

在现有浏览器测试验证三主题颜色与 color-scheme、几何 token 非空及级联读取；保留宽度、菜单、候选与重挂载回归。合法紧凑 CSS、var() 和带分号的值通过浏览器处理，不再以文本正则猜测。根 `scripts:test` 的无浏览器任务不新增 Chromium 前提。

### G. 共用流式校验 writer

从 [nativeGithub.download](../../scripts/native-github.mjs) 提取具名 `writeVerifiedDownload(response, file, expected, { signal } = {})`，仍放在现有工程模块；[release upgrade](../../scripts/test-release-upgrade.mjs)成为第二个调用者。

writer 仅处理 response body → 同目录 .part → size/SHA-256 校验 → 关闭文件 → rename。调用方继续负责 fetch、HTTP 状态、认证、Release/tag/附件身份和请求重试。

1. 写文件前验证 size 为正安全整数、sha256 为 64 位小写十六进制；提前拒绝时释放未消费响应资源。
2. 使用 `Readable.fromWeb`、Transform、`pipeline`，逐块计数与更新 hash。超量立即失败，EOF 后确认完整大小和摘要，禁止整包 arrayBuffer 或为下载摘要再读整包。
3. 失败清理本次 .part，保留已有最终文件；清理失败也报告。沿用同一目标串行写入约束，不增加锁服务或恢复状态机。
4. nativeGithub 继续强制 token、使用 API asset route 和当前 GET 重试规则；不得为了复用 writer 放宽认证。
5. 升级测试保留公开 Release 无 GH_TOKEN 的行为。每次 fetch 前创建 120 秒 signal，并传给 fetch 与 pipeline，期限覆盖响应头和正文；不在 response 返回后才开始计时。
6. 保留精确 Release 关联、GitHub digest、插件 manifest SHA-256 及安装字节验证。其他安装后摘要读取不属于本次下载缓冲清理。

扩展 [native package 回归](../../scripts/prepare-native-packages.test.mjs)，或在同目录增加有明确 CI 映射的 writer 回归：多分片成功、提前 EOF、超量、错摘要、中途读错、abort/正文停滞、写盘失败、旧最终文件不变及 .part 无残留。使用假 Response 和临时目录，不联网。

匿名请求由升级调用方另验：将现有 `download(asset, directory, request = fetch)` 作为可导入的工程函数，沿用项目已有 direct-run guard 包住升级运行流程；导入不解析升级环境或创建目录、安装程序。用 fake request 记录 URL、headers 和 signal，确认无 token 时公开附件请求不发送 Authorization，且 fetch 与 writer 共用期限。writer 不负责请求头，不能把 fake Response 的流测试算作匿名请求验收。

## 8. 实施顺序、检查与交付

| 步骤 | 代码与回归 | 必须完成的验证 |
| --- | --- | --- |
| 1 / A | ui.ts、真实表单必要调用、interactions/服务器回归 | npm run check；npm run interactions:test；node scripts/test-server-ui.mjs；npm run settings:test；npm run ui:production:test |
| 2 / B | 文件编辑器菜单、CSS、编辑器及独立包夹具 | npm run check --prefix plugins/files；npm run files:editor:test；必要时实际文件 UI/OpenSSH 与原生编辑器专项 |
| 3 / C | preferencesChanged/layout 临时快照、编辑器回归 | 文件插件类型、UI 构建、编辑器测试；隐藏与跨窗恢复回归 |
| 4 / D | 核心/SDK 读取、storage 调用回归 | 核心与 SDK 格式、npm run test、cargo test --manifest-path crates/plugin-client/Cargo.toml --locked；五平台相关源码与插件进程回归 |
| 5 / E | 三个 CLI、纯解析模块、参数与选择回归 | npm run scripts:test；相关 OpenSSH/终端输入/正式 UI；实际 launcher 透传由既有原生组合入口验证 |
| 6 / F | 主题快照 helper、两个 UI 消费者 | npm run system-status:ui:test；npm run docker:ui:test；插件静态夹具和现有窄布局继续通过 |
| 7 / G | 工程 writer、两个下载消费者、下载回归 | npm run scripts:test；既有 Verify published release upgrade 按明确版本执行真实升级 |

命令的环境、fixture 准备和完整检查范围以[开发指南](../development.md#检查与测试)为准；上述表列出本次相关入口，不代替其前提。Windows 共用 target 的构建与集成按顺序执行。

实施时维护 [select-checks](../../scripts/select-checks.mjs)及其回归：新参数、主题或下载 helper 必须映射到真实消费者，不能因遗漏映射而漏测；不新建工作流或调用不存在的 npm 命令。沿用 [Check](../../.github/workflows/check.yml)、[独立插件打包](../../.github/workflows/plugin.yml)、[原生组合](../../.github/workflows/remote-integration.yml)和[实际升级](../../.github/workflows/release-upgrade.yml)。

产品源码涉及的核心、共享客户端和文件插件按照现有规则分别验收。核心使用 `npm run build`，文件插件使用 `npm run plugin:build -- --plugin files`；五平台原生编译、打包、包内启动退出及相关专项要求见[分发文档](../distribution.md#验证)。工程工具改动单独完成脚本与必要专项，不因此重打产品包。

D 修改共享客户端时，五个生产插件均直接依赖它，必须分别完成五平台原生编译、独立打包和包内进程验收；对 terminal、files、network-proxy、system-status、docker 使用既有 `plugin:build` 入口及独立工作流。按组件保留结果，不要求同步升版、共同标签或联动发行，核心包仍不依赖业务插件构建。

不预定新版本号或自动发布。后续发行各自升版；核心版本仅修改 tauri.conf.json，文件插件独立版本与 CHANGELOG。相同来源的相关检查、包来源、原始 run、Release 和跳过范围按现有交付规则记录。

## 9. 保留的 DOM 目标与暂缓范围

### 原 DOM 计划的未完成要求

异步刷新应保留当前实体的有效草稿、焦点、选区和 IME 组合；实体切换不能带入旧对象输入，保存、取消、关闭和输入所有权保持清楚。这些要求仍有效，A 的小修不代表已全部实现。

后续先在普通表单真实流程复现连续输入、组合输入、异步返回、select/checkbox 及实体切换。根据实际失败选择最小接法；工作台 [patchChrome/patchChildren](../../src/ui.ts) 保留 iframe、按压按钮、指针捕获、菜单与输入权，并覆盖按压期间刷新、保存/放弃焦点及实体切换。原生 IME、真实窗口缩放和 DPI 分别记录设备验收。

整体 DOM morphing 和核心 overlay 改用原生 dialog 尚未选定；不安装 morphdom、不全局替换工作台渲染。旧试验只作历史背景，见[固定提交中的原记录](https://github.com/BgaSol/soDeck/blob/8115fdcd5bed4108021a729da605fbde77afc2af/docs/implementation-standard-library-adoption.md)，不能替代当前回归。

### 其他暂缓项

- 子进程 `events.once`：低收益清理，保留 exit/close、监听注册顺序和等待失败处理的区别；不扩进 RPC pending map 或 shutdown。
- 语言扩展映射：大部分是 Dockerfile、dotfile、systemd、Nginx 和限定 shebang 规则；公共元数据替换收益不足。
- Monaco 内部 tokenizer、远端 TOML/YAML 文本读取：涉及稳定 API、语言差异、workspace 语义或新工具成本，另行权衡。
- SOCKS、终端历史/输入、远端路径、HTTP 线路和文件操作规则：已有库承担通用部分，余下产品语义继续保留。

七项交付后，计划只保留本节仍有效且未完成的 DOM 范围和必要上下文；若用户取消或替代这些要求，先安置仍有效约束，再删除计划。

## 10. 当前证据、正式文档与收束

### 当前验证

- A：旧实现的密码/多行输入选区丢失已复现；新实现通过支持输入类型/方向与异常目标浏览器回归，并通过真实服务器资料验证失败刷新及设置流程。原生 IME 未验。
- B/C：旧实现菜单裁切、多选区收窄和隐藏视图漂移已复现；新生产编辑器构建、类型与完整浏览器回归通过，含真实共享菜单 bundle、剪贴板过期、隐藏后改宽度和已启用 minimap。五平台实际包待验。
- E/F：参数早失败、启动器原样透传和两套完整三主题 UI 回归通过；主题 helper 的实际 CSSOM、页面清理已验证。
- D/G：标准读取临时差分与永久边界/错误/存储/进程回归通过，旧循环已删除；writer 多分片、异常、abort、清理及升级调用方匿名请求回归通过。真实正式版本对升级待验。
- A-G 代码已实施；本轮完整源码、包来源与实际包验收由新版本记录承接。以前发布或 CI 的通过结果不计为本轮验收。

### 文档归属与完成条件

| 行为或约束 | 实现时回写的唯一正式归属 |
| --- | --- |
| 表单焦点/选区与实体切换 | [连接](../features/connections.md)、[设置](../features/settings.md)中各自相关流程；工作台输入权归[工作台](../features/workbench.md) |
| 编辑器菜单与偏好恢复 | [文件](../features/files.md) |
| RPC 字节上限与故障语义 | [插件 API](../plugin-api.md)的已有协议约束；确有新增设计理由才更新[架构](../architecture.md) |
| CLI、测试 helper、检查选择 | [开发指南](../development.md)，不把测试步骤复制进功能文档 |
| 实际发行与跳过证据 | 核心 docs/releases/ 与文件插件 CHANGELOG |

逐项完成代码、相关格式/类型/构建/回归，并同步正式文档；将可长期复现的缺陷转为测试。实际包或专项未验收时明确保留待验范围。README 不追加普通修复、规划或发行进度；临时计划通过[唯一文档索引](../README.md)的 plans/ 入口进入。

所有有效要求完成或妥善安置后，修复引用并删除本文及无用探针附件；历史通过 Git 查询，不新建完成计划归档。

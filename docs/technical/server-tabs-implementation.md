# T-004：顶部服务器 Tab 与公共工作台代码修改方案

更新日期：2026-10-25 · 修订：r1 · 状态：已获编码确认，实施中（确认原意见 T-004）
任务与确认：[T-004 r4](../tasks/T-004-server-tabs.md) · 功能及测试：[F-CORE-006 r4](../core/server-tabs.md)
现有实现基线：`codex/t-003-core-terminal` / `b1da221`，仅有本次设计文档未提交改动。

## 1. 本次交付与不变量

用户本轮要求先提供具体代码修改方案，确认后再开始改。本文件覆盖应用、SDK、插件、布局迁移、原生适配及测试代码的建议修改；本轮仅写文档。

完成后：Windows 顶部一行容纳服务器 Tab 和窗口控制；每个 Tab 是完整服务器工作区，可以拖出、跨窗口移动、单个或全部合并；内部保留工具面板、双区域和插件页面。主窗口关闭仍收起到托盘，关闭服务器 Tab/工作区子窗口结束相应会话。详细交互以 F-CORE-006 为准。

固定以下约束：

1. 一个 App 中只有一个长期运行的终端业务实例；窗口变化不重启 worker、不重连 SSH、不重放命令。
2. serverId/workspaceId/viewId 和会话身份稳定；改变的是窗口归属与表面挂载代次。
3. 每个逻辑视图只有一个有效输入表面；移动可以重建渲染表面，不能以 DOM 卸载触发会话关闭。
4. 核心负责公共布局、窗口、资源和交互服务；终端业务、终端渲染和会话编排留在插件。
5. 本次不新增业务插件、插件互调、权限审批、任意嵌套分屏、会话镜像或退出后后台服务。

维持当前 Tauri/Rust、React/TypeScript、QuickJS 和 raw Wry/WebView2，不更换技术栈或引入大型停靠库。UI 继续采用已读取的 apple-design：即时反馈、跟手且可取消的拖动、明确的空间来源和去向、克制动效及键盘等价路径。

## 2. 修改地图

以下是计划中的模块；标为“新增”的文件目前不存在，具体拆分可在确认范围内调整，不另建空框架。

| 位置 | 计划修改 | 完成判断 |
| --- | --- | --- |
| `apps/desktop/src-tauri/src/app.rs` | 保留启动、统一来源绑定和调用记录入口；将窗口、命令、迁移及视图生命周期编排拆到职责模块；移除运行路径中对 `sessions.*` 的业务分支 | 外壳不解析终端业务结果来更新页面，全部公开调用仍留痕 |
| 同目录 `workbench.rs`；新增 `window_registry.rs` | 从全局 activeWorkspace 改为每窗口自己的活动工作区和顺序；通用视图、停靠及双区域状态 | 两个窗口互不切换对方服务器；同工作区只归一个窗口 |
| 同目录新增 `workspace_operations.rs`、`view_lifecycle.rs`；复用 `view_operations.rs` | 批次迁移、关闭准备/提交、去重、取消和查询；单视图迁移复用同一协调机制 | 失败不半搬，关闭不误触迁移回调，无重复释放 |
| 同目录新增 `plugin_runtime.rs`、`command_registry.rs` | 按宿主登记的插件身份路由命令/生命周期；保持实际只加载终端一个业务包 | 不以窗口创建 worker；插件自己的 UI 调自己的 worker，不开放插件互调 |
| 同目录 `native.rs`；新增 `window_chrome.rs` | 无独立标题栏、原生命中/窗口操作、跨窗拖动坐标、焦点与 DPI；保留原生视图主线程队列 | 真机拖窗、缩放、Snap、IME、关闭和托盘可用 |
| `crates/core/src/lib.rs`、`model.rs`、必要的 `ssh.rs` | 补充运行资源与视图的归属/绑定、批次绑定提交和源上下文；SSH/PTY 协议行为不变 | 布局层不解释终端会话，失效表面无法继续输入；关闭只释放所属租约 |
| `crates/plugin-manager/src/package.rs`、`lib.rs`；必要的 `store.rs` | 命令/工作区入口声明、API 与能力版本验证、新旧包升级路径 | 不兼容包明确拒绝，兼容包仍独立构建与激活 |
| `crates/plugin-host/src/` | 扩展宿主到 worker 的命令上下文与通用生命周期调用，保持有界队列/超时/故障回收 | 生命周期不会阻塞核心管理入口，晚到结果有确定归属 |
| `packages/sdk/src/`；新增 `contracts/workbench-v2.schema.json`、契约生成脚本 | 公共类型、方法与事件、上下文订阅；从同一 schema 生成 TS/Rust 契约结构，边界运行时验证；两端共享正反例 | 类型检查之外还验证未知字段、错误组合、版本和身份 |
| `apps/desktop/src/app.tsx`、`types.ts`、`surface.tsx`、`surface-queue.ts`；新增 `server-tabs.tsx`、`server-picker.tsx`、`window-controls.tsx` | 顶部服务器 Tab、选择器与系统控制，按窗口快照渲染；去掉重复顶栏、服务器大标题和常驻服务器列表；故障外框改用通用动作 | 外壳不再提供硬编码“新建终端”业务回退；终端入口来自插件声明 |
| `apps/desktop/src/styles.css`、认证/管理对话框 | 44 逻辑像素顶部初始尺寸、溢出、焦点、减少动态效果；对话框跟随明确来源窗口 | 原生子视图不能盖住选择器、菜单和模态操作 |
| `plugins/terminal/manifest.json`、`src/worker.ts`、`session-list.tsx`、`terminal-view.tsx` | 注册命令和列表入口，接入通用视图/元数据/资源/生命周期；保留终端业务和屏幕快照 | 新建、重命名、重连、关闭及移窗经公共契约完成 |
| 测试、`scripts/check-boundaries.mjs`、版本/打包配置 | 增加批次/多窗口/原生测试，替换已被新契约替代的旧断言；独立开发包和版本兼容验证 | 新功能与原终端流程均有对应证据，原生/模拟分开记录 |

运行宿主可用按 pluginId 的内部注册表替代当前单一 `loaded` 字段，以使路由有明确来源；本次只注册并交付终端，第二个插件的真实并行运行不在本次完成声明内。代码中终端专用字样只允许出现在终端包、旧布局适配、随附包配置及针对它的测试；工作台运行编排不按终端命令名分支。边界检查结合契约测试与代码审查，不单凭字符串扫描判断解耦。

## 3. 核心状态与保存格式

### 3.1 运行状态

| 对象 | 必要字段/语义 |
| --- | --- |
| WindowRecord | windowId、role（main/workspace/view）、workspaceIds、activeWorkspaceId、bounds、displayState；单视图窗另有 viewId；主窗口 ID 固定 |
| WorkspaceRecord | workspaceId、serverId、ownerWindowId、内部 Tab 组、split、dockSlots、focusTarget、revision；保持最多两个中央区域 |
| ViewRecord | viewId、pluginId、viewType、workspaceId/全局作用域、instanceKey、placement、title、displayStatus、stateVersion、可恢复的 pluginState、revision |
| RuntimeBinding | viewId、插件实例代次、surfaceId、mountGeneration、windowId、阶段及资源引用；不保存到布局 |
| OperationRecord | operationId、来源上下文、涉及工作区/视图、预期修订、阶段、结果、持久化状态、取消标记 |

通用布局移除 sessionId/ptyId 和 `sessionsSide/sessionsVisible` 专属字段；前者归终端业务模型与核心运行绑定，后者映射为通用 dockSlots。旧文件中的上述字段仅由旧格式转换器读取。终端正文、秘密及有效资源句柄不得进入布局。

### 3.2 布局 schema 2 与升级

- 新布局明确 `schemaVersion:2`，保存窗口、工作区、视图和修订。已有无 schemaVersion 文件按旧格式解析。
- 先复制原文件到独立、不可覆盖的迁移备份，再在内存转换和校验；成功才原子替换。失败保留原件并显示恢复入口，不用空工作台覆盖坏文件。
- 原打开工作区按旧保存顺序归主窗口，保留旧活动项；旧独立终端继续独立。旧文件没有保存用户顺序时使用其实际存储顺序，不虚构此前拖动顺序。
- 旧 `terminal.sessions` 的停靠位置映射为该声明视图的 dockSlot；已结束终端转为插件可识别的恢复占位，标题及位置保留。所有旧运行句柄失效，不自动建连。
- 缺失插件/不兼容状态保留故障占位与可用配置。局部条目损坏隔离处理并报告，合法条目不能被整体清空；整个文件不能解析时使用备份恢复入口。
- 首次新格式保存前的回退使用原布局副本和旧核心/终端组合；新格式启用后的业务配置变化不通过旧快照静默覆盖。旧程序不得直接读取并改写 schema 2 数据。

布局与运行资源的事务边界分开：迁移已经提交但磁盘写入失败时，新位置继续可用，显示未保存并提供重试；不声称移动失败，也不回滚成双重输入。

## 4. 公共接口与调用上下文

### 4.1 版本与来源

RPC envelope 继续使用 `apiVersion:1`；它只表示传输格式。插件 API 计划升为 `2.0.0`，工作台能力为 `workspace.v2`，终端新包声明 `pluginApi:^2.0.0` 并要求该能力。保留已验证的服务器、认证和 PTY 方法，不更改它们的业务语义。

宿主从原生窗口/插件表面/worker 通道绑定实际来源，生成 operationId 对应的 `pluginId/instanceId/windowId/workspaceId/viewId` 上下文。前端传目标 ID 用于定位，但不能覆盖真实来源。windowId 不采用“主窗口当前服务器”的默认替代。旧表面发送过期几何、隐藏、输入或业务请求均被拒绝，不因 viewId 相同获得新挂载身份。

工作区和窗口是核心负责的布局对象；插件不直接调用平台窗口 API。所有已公开能力仍按 P-14 平等调用并留痕，以下身份与归属检查只用于正确路由和生命周期。外壳可执行已注册插件命令；插件来源执行命令时只能路由到自身业务实例，拒绝借 commandId 间接调用另一插件，保持 P-13 的阶段边界。

### 4.2 本次公共签名基线

下表字段为拟实现契约的必要部分。ID 均为非空字符串；revision/generation 为非负整数；枚举和额外字段由 schema 校验。确认后在该语义内补充实现字段、错误细节及类型，不改变使用行为。

| 方法/事件 | 请求或内容 | 结果/约束 |
| --- | --- | --- |
| `workbench.snapshot` | 默认读取实际来源窗口；定位其他窗口时传 targetWindowId | 窗口级快照、布局 revision、公开工作区位置及进行中操作；不广播终端正文 |
| `workspaces.open` | serverId、target（currentWindow/newWindow）、operationId | workspaceId、ownerWindowId、created；已打开则定位原位置，显式移动用 move |
| `workspaces.activate` | workspaceId | 激活其所属窗口内的工作区，不影响其他窗口 |
| `workspaces.reorder` | windowId、workspaceIds、expectedRevision、operationId | 必须是该窗口当前 ID 的完整排列；返回提交修订 |
| `workspaces.move` | workspaceId、targetWindowId 或 newWindow、index、expectedRevision、operationId | 批次操作结果，不复制工作区；源/目标由宿主复核 |
| `windows.merge` | sourceWindowId、targetWindowId（菜单默认 main）、expectedRevision、operationId | 源窗口全部工作区按序一次迁移；结束后回收空子窗口 |
| `workspaces.close` / `windows.close` | 目标 ID、operationId | 返回逐对象终态；main 关闭走托盘，其余执行相应关闭范围 |
| `windows.control` | action（minimize/toggleMaximize/show/systemMenu）、必要目标 | 默认当前真实来源窗口；关闭另走生命周期服务，不能绕过清理 |
| `views.open` | 声明的 viewType、workspaceId、instanceKey、初始元数据、可恢复状态 | viewId；pluginId 来自宿主实例；按视图单/多实例规则创建或定位 |
| `views.update` | viewId、expectedRevision、title/displayStatus/badge/公开动作引用 | 新修订；不能通过此方法改服务器/资源归属或窗口布局 |
| `views.bindResources` | viewId、运行资源引用列表及替换代次 | 核对实例、工作区、代次；运行绑定不写布局；PTY 适配由核心资源层完成 |
| `views.activate/move/close/recover` | viewId，移动另含 placement/group/目标及 operationId | 按声明处理；recover 只恢复表面，重新建会话是插件注册的业务动作 |
| `commands.execute` | commandId、workspaceId/viewId、args、operationId | 从注册表确定所属插件，附可信执行上下文；无需外壳解析命令结果 |
| `plugin.invoke` | command、args | 插件自身 UI 到自身 worker 的业务路由；不新增跨插件目标参数 |
| `workbench.operation.get/cancel` | operationId | 返回当前阶段/结果；已提交不能假装取消成功；超时先查询，不自动重发业务 |
| `workbench.changed` | revision、影响的 windowIds/workspaceIds/viewIds、changeKind | 订阅端刷新其范围；漏事件/旧修订取快照，不以消息顺序猜权威状态 |

`views.update` 的状态是通用展示状态（loading/ready/ended/error 等），不是核心解释插件的业务状态机；文字、提示和动作由插件提供。资源引用由核心按资源类型校验及处理，工作台不解析 sessionId 或拼接 PTY 命令。新接口不提供任意脚本、远端命令或其他插件调用的旁路。

常见错误至少覆盖 INVALID_PARAMS、UNSUPPORTED_API、SOURCE_INVALID、STALE_MOUNT、LAYOUT_CONFLICT、WORKSPACE_CLOSED/VIEW_CLOSED、MOVE_IN_PROGRESS、MOVE_TIMEOUT、CANCELED、RESULT_UNKNOWN、PLUGIN_UNAVAILABLE 和 STORAGE_ERROR。操作返回明确区分 `committed:false`、`committed:true,persisted:true` 与 `committed:true,persisted:false`。

契约文件集中描述方法、请求/结果、事件和标准错误；生成 TS/Rust 契约结构和字段约束，边界完成运行时校验，生成产物过期纳入 check 检查。SDK/Rust 各自消费同一组正反例，检查 required 字段、枚举、未知字段、版本和上下文冲突。生成范围仅本次工作台契约，不顺带重建其他协议工具链；终端业务命令的 args 仍归终端验证，不把业务 schema 放入工作台核心。

## 5. 插件注册、业务生命周期与终端改法

### 5.1 最小公共扩展点

清单保留 contributes.views，增加 commands 与 workspaceEntries：命令有稳定 id、标题、scope 和入口位置；工作区入口引用声明过的视图/命令，支持首次默认展开的工具面板。宿主启动后注册，插件不可自报覆盖其他来源。没有终端插件时，服务器工作区仍能打开并显示通用空态。

通用 worker 生命周期约定如下，方法属于插件公开接入契约，不是外壳根据业务名字猜测：

- command：传命令、参数与宿主上下文；真正的终端业务留在 worker。
- prepareClose：按视图 ID 与 operationId 收集未保存/可关闭结果，只检查，不提前结束远端资源。
- close：准备全部通过后提交，按相同 operationId 幂等释放该视图拥有的业务；列表视图不关闭其他终端。
- restore：接收恢复状态与版本，返回可恢复 UI 或结束/不兼容占位；不自动重建会话。
- 表面 prepareMove/ready/commit/rollback：复用现有视图协议并扩展 batchId；这些回调不调用业务 close。

宿主跨 worker 调用期间不持有工作台/资源互斥锁。工作区迁移和关闭按稳定 ID 顺序登记操作占用，避免多个视图互相等待；其他工作区继续响应。worker 回调再调用核心时可以正常进入公共网关。

关闭先收集所有准备结果，任何取消都发生在释放之前；提交后的远端释放不能承诺原子撤销。若部分释放失败，保留剩余资源与恢复外框，返回逐项结果、允许幂等重试或从核心停止；不把已结束的会话显示为仍运行。正常关闭某工作区不停止共享的插件 worker。

### 5.2 终端具体迁移

1. 新建命令由终端注册；worker 经核心 acquire/open 创建资源，再经 views.open/bindResources/update 建立页面。任何一步失败或关闭/取消后迟到完成，按 operationId 逆序清理本次创建的资源，不停止其他消费者共用连接。
2. 重命名由 worker 修改会话后调用 views.update；核心统一更新各容器标题。重连由 worker 更新资源并换绑运行引用、更新状态，外壳不识别 `sessions.reconnect`。
3. 会话列表继续按自身 workspaceId 查询；打开页面经注册命令，激活/关闭经通用视图接口。manifest 的 multiple 仍表示视图数量，不表示 worker 数量。
4. 终端 xterm 渲染、粘贴、控制键、字号和屏幕快照保留。迁移后按宿主新的挂载身份接回同一资源，源表面的迟到请求失效。
5. 终端结束占位的“新建终端”由插件声明恢复动作；外壳 NativeSurface 只展示通用外框与已登记动作，不读 ptyId 判断业务。
6. 主机信任/凭据交互继续由核心处理。通用命令执行登记 operationId 与来源窗口，核心按结构化认证错误进入已有对话框；用户处理后明确重试对应注册命令，参数和执行上下文由宿主保存。秘密不传给 worker。取消抑制迟到认证，移窗遇到进行中的认证按功能规格先完成或取消。

会话状态改变后，终端通过公共元数据/通知接口更新其列表和外框；取消依赖全 App 无差别刷新业务列表的做法，事件丢失可取快照。不顺便改终端业务功能或实现新的会话托管。

## 6. 工作区迁移与并发

统一状态机：`created → preparing → ready → committing → committed`；提交前可进入 canceled/rolledBack，提交后持久化单独记录。响应丢失从 operationId 查询同一结果。

1. 验证源/目标/修订，登记本批所有工作区与视图的操作占用，保留源占位。独立弹出的单视图不加入搬移表面集合，但它的归属仍随工作区可查询。
2. 对已挂载表面发 prepareMove；完成/取消 IME 组合后冻结新输入，等待已受理输入和已消费输出确认，收集有界快照。未挂载的视图只迁移模型和可恢复状态，不为搬移临时建连。
3. 创建候选窗口/表面，恢复快照、等待 ready；候选表面不能读/确认实时流、resize PTY 或输入。目标窗口准备失败则销毁本批候选并恢复源。
4. 在统一提交关口重新验证修订、资源代次及取消状态。核心资源层批次验证并更新有效绑定，布局归属与挂载注册表一次发布；提交期间阻断这批表面的读写和几何更新，不能逐个公开半完成结果。
5. 撤销源挂载后启用目标，恢复当前有效焦点，关闭旧表面；空子窗口此时回收。最后保存布局，保存失败保留已完成的运行位置并提示。

实现批次资源重绑定时，必须补齐“第 N 个资源验证失败”的故障测试。提交前失败不改变任何有效绑定；若底层适配需要补偿，补偿用更高代次重新绑定源，不能回退或复活旧代次。目标在提交后失败走新位置的表面恢复，不能误判为未提交并复制工作区。

移动、关闭、单视图返回及同工作区新建/重连等改变视图或绑定的操作协调串行；正在进行的连接/认证不允许开始移窗。迁移中关闭只排队一次，迁移提交或回滚后执行；App 退出先停止接收新操作、取消未提交批次，再按全局退出策略清理。锁只覆盖状态核对/提交，不覆盖网络或 WebView 等待。

## 7. 原生窗口与前端改法

- 所有窗口由 WindowRegistry 创建和识别，标签分 main、workspace-*、detached-*；RPC 校验和 Tauri `capabilities/shell.json` 同步接受实际登记窗口。Tauri 原生能力配置与 SoDeck 插件权限是不同层，本次不增加插件授权。
- Tauri 隐藏独立装饰栏，Windows 适配层处理顶部空白/窗口边缘/系统按钮的非客户区命中，保留系统菜单、最大化、缩放、贴靠和 Snap。使用现有 Windows crate 按需补 features；不单凭 CSS 宣称无标题栏窗口已完整支持。
- 命中矩形由外壳布局报告并关联窗口/DPI/修订，原生复核客户区和实际指针。交互控件排除拖窗；原生最大化命中与前端 click 去重，处理 WebView 子窗口的实际输入路径。窗口销毁时移除原生 hook/订阅。
- 顶部拖动由窗口内 Pointer Events 提供即时预览，跨窗口阶段由原生指针/捕获跟踪和已登记落点完成，不依赖两个网页互相传 HTML 拖放。跟踪仅在拖动期间存在；Esc、失焦/系统取消及目标销毁有明确终止。
- 服务器 Tab 落在内容区回原位；窗口外按预览拆出；目标服务器 Tab 行按插入线合并。内部终端 Tab 拖动仍使用视图协议，不能混作服务器 Tab。
- 顶部目标高 44 逻辑像素，系统文字放大允许自适应；保留至少约 48 像素拖窗空白与系统按钮。服务器 Tab 单行溢出，名称完整信息可由悬停/键盘获取。
- 复用现有选择器/菜单/对话框层叠机制，扩展每窗口的原生子视图遮挡区域；关闭弹层恢复原有焦点，不给后台终端发送按键。
- `surface-queue` 的键扩展为 windowId/viewId/mountGeneration；旧组件卸载产生的隐藏或旧尺寸不能覆盖新窗口的挂载。非活动工作区保留会话，展示刷新与缓冲按现有预算管理。

原生机制先以菜单完成正确迁移，再实现指针路径，两者都属于本次交付。若无法满足 Snap、物理跨窗拖动或原生输入，任务保持未完成并修正适配；不能自行删去该行为。

## 8. 运行预算、性能与边界

不新增工作区子窗口的产品数量上限；本次最低验证规模为主窗口 + 两个完整工作区子窗口 + 两个单视图窗口。窗口增加仍受操作系统资源约束，创建失败保持源位置。保留已有 App 总计 8 个终端/PTY、最多 2 个单视图独立窗口限制，拆窗不绕过它们；20 个已打开服务器的无会话 Tab 用于溢出验证。

| 项目 | 本次预算与验证 |
| --- | --- |
| 迁移表面 | 每逻辑视图最多一个有效源与一个候选；仅为已有表面创建候选，批次完成后候选/源残留为零 |
| 快照 | 单视图继续最多 4 MiB，单批累计最多 32 MiB；流式逐表面准备，超额在提交前明确失败并回源，不截断状态冒充成功 |
| 时间 | 单视图迁移沿用 10 秒；整工作区/整窗批次最长 30 秒，整个准备期可取消；超过界限回滚并显示原因 |
| 输入/输出 | 沿用 T-003 每 PTY 与全局有界队列、输入序号与确认规则；不为每个窗口复制无限输出队列 |
| 性能 | 本机隔离协议服务，4 PTY 各 256 KiB/s 输出 60 秒；标记往返及核心查询/停止 P95 ≤ 500 ms，记录 RTT/设备/WebView2 版本；迁移期间的输入冻结单列，不混入稳定状态延迟 |
| 回收 | 10 次拆合、20 次创建/关闭后核对窗口、表面、worker、PTY、连接、订阅及快照计数回到相应基线；同时记录进程内存峰值，不能用协议队列值冒称 WebView 总内存受同一限额 |

本轮先执行已获用户要求的本机验证；Linux/OpenSSH 实机、多屏不同 DPI 或完整辅助技术缺少环境时逐项保留未验证，不以 loopback 或浏览器代替。受影响必需项未验证时，不把任务标成全部工程完成。

## 9. 包、版本和回退

- 新开发包计划 `2.0.0-dev.2`，交付终端配套包 `0.2.1`，插件 API 2 / workspace.v2；`0.2.0` 仅作为下述独立更新测试的前一版本。RPC envelope 与 SSH 能力版本保持。版本字段及兼容校验同步到真实发布/运行来源，不能只改 UI 文案。
- 本次属于核心与终端一次配套升级。旧 `0.1.x` 的工作台接入不在新宿主承诺兼容范围；新终端在旧宿主上明确拒绝。新宿主不能保留伪 workspace.v1 声明让旧包进入不支持路径。
- 新开发产物使用独立 `.local/dev/SoDeck-v2-T004/`，保留原开发包；验证使用独立数据目录及旧数据副本，不改 v1 日常数据。实际用户升级前保留完整旧版本/数据的可恢复组合。
- 对已有旧终端激活记录，使用随附的新签名终端包走现有插件管理器的准备/启动激活事务；成功后再启动 worker 与迁移布局。失败进入核心恢复界面，保留旧包、原数据和诊断；不修改旧包的清单、不连外部市场。
- 首次整体回退要使用相匹配的旧核心、旧终端及布局副本；不能在新核心里热换旧 worker。后续兼容的终端独立更新继续重启生效。
- ST-17 的独立更新使用同一 API 2 下的 `0.2.0 → 0.2.1` 实际终端包验证：将已有 UPD-01/02 的两份真实功能基线接入新契约，继续以“恢复默认字号”入口及 worker 处理的既有差异验证 UI/业务一起更新。前一版只作为隔离测试输入，交付采用包含全部既有功能的后一版；不新增终端功能，也不向用户交付删减功能的测试版本。两次插件构建之间核心哈希必须相同。

本次只交付开发包，不包含正式签名、安装器发布或公开渠道发布。额外的兼容承诺或产品限制变更按任务重新评审，不由实现后补写。

## 10. 实施顺序与阶段产物

用户对本方案一次确认覆盖下列步骤；步骤不是逐段请求批准。

1. 固定接口与 schema、建立正反例和版本校验；补齐布局 schema 2、备份/转换及运行绑定类型。
2. 完成通用命令、视图、元数据与生命周期，适配终端，先在原有主窗口/单视图模式跑通新建、认证、取消、重命名、重连和关闭。
3. 接入窗口注册表和每窗活动工作区，改顶部服务器导航、选择器及无独立标题栏外壳；完成原生控制、层叠和来源路由。
4. 复用单视图协议建立整工作区/整窗批次迁移，先菜单后物理拖动；覆盖失败、取消、关闭竞争和持久化异常。
5. 完成旧布局/旧包升级、恢复与跨窗口故障回收；运行本次完整回归，生成独立 Windows 开发包、实际截图和结果记录。

不顺带重构 SSH 认证协议或数据层，也不修改 T-003 历史通过记录。确认后创建专用 `codex/t-004-server-tabs` 工作分支并保留已有设计文档；不覆盖其他任务变更。

## 11. 测试映射与交付条件

功能预期以 F-CORE-006 的 ST-01 至 ST-17 为唯一来源；下表确定测试代码与实际运行入口。

| 测试层 | 计划覆盖/文件 | 对应功能 |
| --- | --- | --- |
| Rust 状态与资源单元/契约 | workbench、window_registry、workspace_operations、view_lifecycle 的测试；core 批次绑定、manager manifest/版本与迁移测试 | ST-02/03/06/07/09/10/11/12/15/16/17 |
| SDK/worker 契约 | SDK 公共正反例、来源/代次/事件恢复；终端 worker 创建补偿、关闭幂等、元数据及列表作用域 | ST-03/05/09/10/11/13/15/16/17 |
| 浏览器交互 | 扩展 `tests/ui/workbench.spec.ts`、session-list/terminal；新增 `tests/ui/server-tabs.spec.ts` | ST-01/02/03/04/08/13/14 的界面部分，不能代替原生 |
| Windows 原生 | 复用 `tests/desktop/harness.mjs`；新增 `tests/desktop/server-tabs.mjs` 与批次故障用例，扩展 windows.ps1 的窗口 ID/命中测试 | ST-01 至 ST-15 适用场景；菜单、物理拖出/拖回、Snap、焦点、IME 分别留证 |
| 独立包/升级 | 扩展 `tests/desktop/updates.mjs` 与管理器测试，记录核心哈希、激活版本及失败恢复 | ST-12/17，回归 UPD-01 至 UPD-05 |

已有检查：`npm run check`、`npm run test:contracts`、`npm run test:core`、`npx playwright test -c playwright.ui.config.ts`、`npm run build`、`npm run desktop:build`、`npm run test:terminal:native`、`npm run test:updates:native`。本次拟新增 `npm run test:workbench:native` 调用新原生用例，尚不可运行。`npm run check` 不等于完整 lint。

源码回归还包含旧 TERM-01 至 TERM-12 的受影响路径、工作台 WB-03 至 WB-14，以及核心来源、调用记录、取消、旧绑定拒绝和资源释放。旧用例若依赖已被替代的签名，先以新规格调整调用，再保留原业务结果断言；不能为通过测试删去认证、IME、真实 SSH 协议或故障检查。

专项必测：多窗口迟到隐藏不会遮住新表面；批次第 N 个失败无半搬；取消后迟到新建不泄漏；同一输入只受理一次；任一窗口操作只影响对应工作区；普通关闭不结束共享 worker；worker 崩溃按真实范围报告；无终端命令名分支的通用夹具可完成开页/元数据/关闭/移窗。

清理沿用 F-CORE-006：停止测试 App/服务器和远端受控进程、撤销注入、恢复显示/输入设置，保留脱敏证据。结果写入拟新增 `docs/tasks/T-004-results.md`，证据放 `.qa/t-004/`，不提交秘密和测试数据。

交付包含开发包入口、实际界面截图、关键用户验收步骤、源码/构建版本及分层结果；通过本机检查不等于用户验收，也不消除未完成的实机条件。

## 12. 本轮审阅与确认

本次需确认的是：F-CORE-006 r4 的交互 + 本文件 r1 的核心/终端公共接口改造 + 一次配套升级及布局迁移 + 上述测试与开发包交付范围。特别关注关闭范围和 API 2 的配套升级；独立更新复用既有测试功能差异，不扩展终端产品功能。

方案提交轮仅有只读源码核实和文档修改。2026-10-25 用户随后答复“你可以开始做了。”，本方案已确认；实际实施、检查与验收由 T-004 记录。

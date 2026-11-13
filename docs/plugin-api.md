# 外置插件 API v1

本文定义当前核心的插件清单、RPC、可选能力和兼容契约。功能操作见[功能索引](README.md)，开发与独立打包见[开发指南](development.md#打包)。插件使用可选能力前先检查能力声明，旧核心继续按兼容范围加载旧插件。

插件是独立原生程序，核心不需要识别具体插件 ID。插件按可信扩展使用，没有权限清单、授权或签名流程。

核心 RPC 保持 API v1，可选界面支持 UI v1 和 UI v2。核心 0.17.0 起提供 UI v2，当前五个官方插件均使用它进行分屏、页面/标签组移窗与输入交接。旧无 UI 插件无需修改；只接受 UI v1 的旧核心不能加载 UI v2 包。


## 文件编辑 SDK

SDK 是随插件编译的客户端与 UI 源码，不是新增核心 RPC。它复用 files.editor.v1、files.v1、viewData.v1、ui.workbench.v2 和 ui.openView.v1；初始化调用 EditorHost::check_capabilities，缺少任一能力时明确拒绝并提示升级。功能与限制见[通用文件编辑](features/file-editor.md)。

### 插件进程

从 sodeck_plugin_client::file_editor 导入 EditorHost。每个插件实例保留一个 EditorHost::new(view_type)，view_type 对应插件自己声明的中央 perResource 视图。来源事件调用 source_event(client, params)，返回值包装在普通 UI 响应的 data 中。来源最终释放时调用 dispose_source(client, view_id)，不在 Surface 迁移时释放。

| 来源事件 data | 字段与效果 |
| --- | --- |
| fileEditor.resource | expectedContext 必须与 RPC Context 完全一致；为来源 viewId 懒创建本组件的文件资源，返回 id；创建中再次请求明确失败，可稍后重试 |
| fileEditor.open | expectedContext、requestId、viewType、path；viewType 必须匹配构造参数，path 是来源资源 stat 返回的规范路径；调用 ui.openView，以 path 作为 resourceKey，params 为 {path} |

将编辑视图的 ui.createView/getView/event/beforeClose/save/disposeView 交给 handle(client, method, params, before_write)。返回普通 {data,metadata}，错误为 Result<Value,String> 的 Err；不包装另一层 RPC。before_write(Context,path) 在创建与保存前执行短业务检查，不能把数据正文放在这里。编辑事件为 save、editorState、dirty、staged、saved、busy、position、error；下载等组件业务事件仍由组件分派。

data 默认包含 fileId、path、cursor、scroll、revision、editRevision、editorState；metadata 包含 title、dirty、busy。保存事件的 data 为核心保存操作回执。contains(view_id) 识别编辑实例，view(view_id) 返回状态快照，entries(Context) 返回同 workspaceId 的实例供组件检查路径冲突；projection(view_id) 读取最近事实，refresh(client,view_id) 向核心刷新事实。远端 RPC 和 before_write 不占用 SDK 状态锁。

ui.save 仅在 dirty 且不 busy 时提交完整暂存版本；未暂存正文、未知提交结果和业务路径阻断明确失败。修订 R 的保存不会清除 R+1 输入的 dirty。旧的状态观察不会覆盖新观察。ui.disposeView 关闭该页文件资源；创建后迟到的资源也会关闭。来源资源和编辑资源分别持有，关闭来源不关闭编辑页。

position.editorState 版本为 1，序列化最多 8192 字节；anchor/active/topLine 为 0–1048577 的整数，topOffset 绝对值最多 100000000，left 为 0–100000000，language 最多 64 字节；search 包含 open/matchCase 布尔值和最多 1024 UTF-16 单元、无换行的 query。正文仅走核心绑定当前 Surface 的文件数据桥。

### UI 与打包

包内入口 ui/file-editor/open.js 导出 openRemoteFile({path,mount,fileId?,viewType?,request?})，默认 viewType 为 editor，request 为 sodeck.request。mount 是当前来源 mount 快照。已有文件资源时传 fileId；否则请求来源进程懒创建。入口先 stat，拒绝非普通文件和超过 1 MiB 文件，再以同一 requestId 重试一次丢失的打开回执。Context 变化、卸载或 flush 使尚未完成的打开失效；不规范化远端路径为本地路径。

加载 ui/file-editor/editor.css 后，从 ui/file-editor/index.js 导入 mountRemoteFileEditor({root,mount,configureDownloads?})，返回 {dispose}。例如组件在自己的 mount 处理器中执行：

```js
const {mountRemoteFileEditor} = await import('./file-editor/index.js');
const editor = mountRemoteFileEditor({root: document.querySelector('#root'), mount});
```

同一 iframe 只能有一个标准编辑页。SDK 接管 prepared、激活、flush、设置、主题和卸载；组件不要重复发送回执。可选 configureDownloads(bindings) 获得 path、fileId、data、event、request、message、host；返回组件原有传输对象（download/visibility/invalidate/dispose），并自行处理其任务事件和 busy。没有此接点时仍提供编辑、保存、取消保存和草稿导出。

release.json 声明 build.fileEditor:true；设置页使用 {id,viewTypes,fileEditor:true}。打包展开 packages/file-editor/settings.json，复制 SDK 的 JS/CSS/Worker/许可证及设置字体；模板标记不进入 plugin.json。SDK 与使用插件同包更新，不另建运行时版本协商；已有核心 API major 1 和 files.editor.v1 契约不变。不依赖文件插件的 ID、目录或进程。

## 可选 UI v2：工作台与显示交接

声明保留已有 `views/actions` 结构，将 `ui.apiVersion` 设为 `"2"`。初始化结果须返回 `uiApiVersion:"2"`；核心声明 `ui.workbench.v2` 能力。`sidebar/editor/dialog` 分别映射为可停靠工具、中央页面和短暂弹窗，不按插件 ID 分支；工具可放入中央组，编辑弹窗不单独移动。

服务器 Context 和业务 view 在核心登记。一个工作区可以在多个原生窗口中拥有显示部分，同一 view 只归一个显示位置；新建实例、同窗移组与跨窗移动各有明确行为。PTY、文件任务和网络监听仍归插件实例/view/Context，窗口或布局变化不重建业务资源。

| 插件进程方法 | 契约 |
| --- | --- |
| `ui.createView` | 逻辑实例初始化；同 view/Context 幂等，创建 PTY、文件句柄或插件状态。核心按当前实例与 Context 缓存结果，失败不会靠反复挂载重试创建 |
| `ui.getView` | 读取已存在实例的显示数据；保持原 PTY/草稿/任务，不创建资源，不替代视图创建。大正文和终端输出继续走既有数据流 |
| `ui.event`、`ui.beforeClose`、`ui.save`、`ui.disposeView` | 继续使用原业务与关闭契约；迁移期间不调用 dispose，真正关闭时才清理实例 |

界面消息通道为 `sodeck-ui-v2`。宿主 `mount` 附带 `surfaceId`、`readOnly`、Context、数据及 `capabilities.workbench:2`；每次挂载取得新的输入身份。插件用 `prepared`（兼容 `mounted`）报告显示准备完成，并声明 `flush:true` 以参加输入同步。准备完成只说明可显示，插件须等待 `surfaceReady` 后才启用编辑、输入、PTY resize 和状态发布。

跨窗交接依次为 preparing → draining → restoring → 提交：目标先建立只读候选，源端 `flush` 同步草稿/输入并以 `flushed` 回应；核心冻结旧表面，等待已接纳调用和终端队列排空，目标重新读取最终状态并接续终端显示检查点及输出水位。目标就绪后同一次状态提交改变位置和输入权，旧表面失效。任一步失败保留原位置和实例，向源端发送 `transferCancelled` 恢复输入。输入法组合尚未完成时插件可以拒绝 flush。

UI v2 的 `flush.freeze` 为 true 时表示迁移或关闭的输入交接，源界面在同步后保持冻结，直到卸载或收到 `transferCancelled`。普通同步的 freeze 为 false，仅等待已经接纳的输入和草稿队列，完成后保持可编辑。插件不能把普通同步变成永久只读。

同窗布局保留 iframe，不需要重新 get/create。切换标签、隐藏工具和紧凑显示不关闭后台业务。剪贴板、正文写入、终端 resize/checkpoint 和共享状态发布使用真实 Surface 身份检查；候选可以读取输出，不能输入或替旧表面提交检查点。

侧边入口仅由 `container:"sidebar"` 生成，跟随核心 Host 的实际位置与完整顺序。未创建工具以稳定定义 key 的空槽位参与移动和记忆，不分配 viewId/Surface，不调用插件创建方法。中央及其他窗口中的工具从更多入口定位，`editor/dialog` 不进入工具条；旧 `locateEntry` 清单字段继续兼容读取，但不再生成侧边按钮。核心自行处理停靠、按钮拖动、排序与保存回执，插件不新增左右位置设置、停靠 RPC 或权限声明；同窗移动继续保留原输入身份和业务资源。

核心桌面命令 `workbench_move` 以 `requestId`、Context、`scope:{kind:"view"|"group"|"host",id?}` 和目标窗口/组/侧栏表达移动。`component_ui.moveStatus` 查询原回执；相同编号和参数复用结果，更换参数拒绝。已结束回执五分钟后返回 expired；进程内最多保留 4096 个请求，达到额度后明确拒绝新编号，重启清理。超时只查询原结果，不自动再次移动或创建窗口。

`workbench_move.destination.returnToSource:true` 请求核心按运行期书签定位原窗口、标签组、相邻标签或工具侧栏。`workbench_return_window({requestId})` 返回调用窗口的全部 Host；核心先统一保留，再准备所有工作区的候选，全部恢复后在一次状态锁内提交。任一成员失败均不释放其他成员的来源 Surface。窗口投影增加 `incomingWorkspaces` 与 `windows`；原 `incoming` 保留为首项兼容字段，候选挂载使用各自的 Context 和迁移编号。

UI v1 仍映射到同一 Host/分屏树，可以同窗排序、分屏和移组；页面、标签组、整工作台的跨窗迁移均要求其中页面支持 UI v2。含 UI v1 工具的局部关闭若需要跨窗口返回工具，会提示先升级插件或关闭服务器全部工作区。新插件安装在旧核心中会在清单检查时被拒绝；独立 Release 应说明须先升级支持 UI v2 的核心。

焦点由实际页面交互更新。`workspace.presentation.v1` 增加 `focusedViewId/focusedWindowId` 与 `lastInteractedLocationViewId`，旧 `activeEditorViewId` 继续用于兼容投影。终端可通过可选 `locationProvider:"terminal.location.v1"` 声明目录来源；文件跟随使用本服务器最近操作的目录提供页，经核心共享状态协作，不直接调用终端插件。

布局文件只保存服务器的容器结构与工具定义位置；不保存 Context、viewId、Surface、正文或命令。当前交互见[工作台](features/workbench.md)，资源与交接职责见[架构](architecture.md)。

## 可选 UI v1

关闭的准备、决定、清理和原生收尾由核心统一管理。宿主快照查询不会创建 view；后台 view 只由工作区/插件状态变化触发，关闭保留期间暂停创建，取消恢复原身份。关闭页面集合、工作区、窗口和应用共用同一操作。插件仍使用已有 `ui.*` 方法，无需增加权限声明或生命周期专用必填字段。

`ui.save` 已接受后，重复关闭决定只查询原保存结果，不自动再发保存。`ui.disposeView` 的超时表示真实结果尚未确认；核心继续跟踪同一请求，迟到成功可以完成该步骤，明确失败后才由用户重试。全局关闭通过进程监督器停止全部受管插件，再确认核心原生资源与连接结束。`files.close`、`net.close` 等取消接口仍快速接受；接受取消不代表实际端口、句柄或 worker 已释放。

清单可添加 `ui: { apiVersion: "1", entry: "ui/index.html", views: [...] }`，完整示例见 `plugins/files/release.json`。视图 ID 在插件内稳定，核心用 `pluginId:localId` 标识定义、另分配 viewId 标识实例。每个视图必须声明 `id/title/icon/container/instancePolicy`：

- `container`：`sidebar` 或 `editor`。侧栏第一版只支持 `perWorkspace` 单例；中央还支持 `multiple` 和 `perResource`。资源标识按远程文件系统语义由插件提供，不按本机 Windows 路径转换。
- `defaultSide`：left/right，默认 left；`defaultPinned`：初始固定入口，默认 false；`openOnWorkspace`：none/reveal/createOne，默认 none；`startupOrder`：初始动作顺序。首次出现的定义保存推荐值，已有偏好保持；新工作区按这些偏好选择未占用的栏位。
- `backgroundOnWorkspace: true`：可选，仅适用于 sidebar + perWorkspace。需要 `ui.background.v1`。核心在工作区存在时确保逻辑 view 并幂等调用 ui.mount，无需 iframe；`uiRequested=false` 的 view 不创建页面，也不阻塞移窗。显示后沿用普通页面生命周期。隐藏只影响页面，采集和 disposeView 仍由插件进程处理。
- `createAction`：旧核心使用的视图创建名称；当前工作台“＋”使用下述 `ui.actions` 默认行为。`locateEntry: true`：可固定的中央定位入口，优先定位最近使用的实例，没有时创建一个。文件编辑器需要 resourceKey，不声明空页面创建动作。

插件包附带已构建的 HTML/JS/CSS，核心通过本地 `sodeck-plugin` 协议加载，不依赖开发服务器或用户 Node 环境。打包脚本自动复制插件的 `ui/`，核心安装包不包含它们。

核心 → 插件进程的 JSON-RPC 方法：`ui.mount`、`ui.event`、`ui.beforeClose`、`ui.save`、`ui.disposeView`。共同参数为 `{viewId, viewType, context, resourceKey, params, data}`，其中 Context 由核心固定绑定。`ui.mount` 首次创建逻辑状态，之后同 viewId 重挂载读取原状态，必须幂等；返回 `{ data, metadata: { title?, dirty?, busy? } }`。`ui.event` 的 data 是 UI 事件；beforeClose 同步报告最终状态；save 保存或返回错误；disposeView 在完成关闭决定后释放实例，必须幂等。该最小协议没有额外的 ui.createView 或运行时注册系统。

插件进程可主动调用 `ui.updateView`，参数 `{viewId, metadata:{title?,dirty?,busy?}}`，供后台任务报告状态。核心附加调用插件和本次进程身份，只接受其自有视图；汇总关闭期间冻结更新。

页面与宿主使用 `postMessage`，信封固定 `channel: "sodeck-ui-v1"`。页面先发 `{type:"ready"}`，宿主回 `mount`（含 viewId、viewType、surfaceId、context、data、tokens、visible），页面初始化后发 `{type:"mounted",surfaceId}`。`request` 带 surfaceId、id、method、params；宿主以同 id 的 `response` 返回 data 或 error。支持：

- `event`：交给所属插件进程的 ui.event，结果更新页面元数据。
- `openView`：`{viewType,resourceKey?,params?}`，只打开本插件在同一工作区中声明的视图。核心分配动作 requestId，IPC 重试复用，真实多次新建允许多个实例。

宿主还通知 `visibilityChanged/themeChanged/focus/unmount`。`mount.connected` 可选布尔值表示工作区当前连接状态；连接状态变化时通知 `connectionChanged`，携带 `connected`。旧 UI 可忽略这两个增量字段；它们不恢复原终端或自动启动业务任务。核心校验真实页面 sender、挂载 token、插件进程实例与工作区 Context。移窗时先等目标页面 mounted，再提交窗口归属；旧表面事件因此失效。同窗切换只隐藏已挂载页面，不能重写其 DOM。示例 bridge 位于 `plugins/files/ui/bridge.js`。

UI 事件最多 24 KiB，是控制交互接口，不能用来传终端输出或文件内容流。RPC 保留 64 KiB 帧上限和 5 秒等待期限；v0.6.0 的真实业务正文另走下述有界视图数据桥。可信插件不增加权限表或授权流程。

### 命令、默认行为与进程主动创建页面

此命令扩展已在当前源码实现，尚未发行；本轮验证与剩余包验收见[待验收范围](plans/component-communication.md)。

可选能力 `commands.v1` 在现有双向 RPC 上提供一次动作的接纳、路由和回执。共享状态仍表达当前事实；命令用于“请执行一次动作”。没有持久消息队列、离线重放、广播执行、自动启动插件或失败转投。插件按可信扩展运行，命令声明不构成权限列表。

插件清单可声明：

```json
{
  "commands": {"apiVersion":"1","handlers":["terminal.open.v1"]},
  "ui": {
    "apiVersion":"2",
    "actions":[{"id":"new-terminal","title":"新建终端","defaultCreate":true,"dataVersion":1,"command":"terminal.open.v1"}],
    "views":[{"id":"terminal","title":"终端","icon":"terminal","container":"editor","instancePolicy":"multiple","createAction":"新建终端","createActionId":"new-terminal","openOnWorkspace":"createOne"}]
  }
}
```

每插件最多声明 32 个唯一命令。名称最多 128 字节，由 ASCII 字母、数字、下划线和短横线分段，以点分隔，最后是规范的 `.v<正整数>`（u32；无前导零）。`commands.v1` 是通道能力版本，`terminal.open.v1` 是业务契约版本，不兼容业务变化使用新的主版本名称。

初始化结果必须返回 `commandsApiVersion:"1"` 和与清单完全一致的 `commandHandlers` 数组；不一致时启动失败。处理者只在实例 Running 且握手确认后可路由，停止即撤销，不依赖 iframe。零个处理者返回 noHandler，多个返回 ambiguousHandler；提交时固定唯一的处理实例，不按加载顺序选择，不转交替代实例。resolve 只查询当前可用性，submit 仍重新检查。

`ui.actions` 保留入口的标题、图标、默认选择和 dataVersion，不另建执行队列。`command` 必须是本插件声明的命令；`createActionId` 引用本插件带命令的 action，首版只允许 editor + multiple 的独立新建页面；单例恢复和 perResource 资源打开保持既有契约。命令处理者仍可显式申请本插件其他已声明视图。默认页面创建、旧核心操作入口 `action.op:open` 和中央“＋”均适配同一命令。`createAction` 继续是兼容显示名称。默认选择仍保存到 ui-layout.json；已有选择不因提供者停止自动切换。

| 方向 | 接口及必要参数 | 返回 |
| --- | --- | --- |
| 插件 → 核心 | `commands.resolve({expectedContext,sourceViewId,name})` | `{available:true,name}` 或结构化错误；不执行 |
| 插件 → 核心 | `commands.submit({expectedContext,sourceViewId,requestId,name,payload?})` | 立即返回同一命令回执，不同步等待另一个插件 |
| 插件 → 核心 | `commands.get({expectedContext,sourceViewId,requestId,commandId?})` | 原回执；不重新路由、分发或创建 |
| 宿主 → 核心 | component_ui `command`，参数含 `op:resolve/submit/get`，其余同上，但无 sourceViewId | `{result}` 或 `{error:{code,message}}`；submit 可指定 groupId/actionKey |
| 核心 → 处理插件 | `commands.handle({commandId,name,context,payload})` | 对象结果；业务处理者决定是否登记自己的页面 |
| 处理插件 → 核心 | `ui.openView({expectedContext,commandId,requestId:"open:<commandId>",viewType,resourceKey?,params?})` | `{viewId,closed?}`；每命令最多一个关联页面 |

核心绑定真实进程实例、来源逻辑 view、Host 和完整 Context（workspaceId/serverId/connectionId/generation），不信任调用方提供的插件身份。插件请求的目标是来源 view 当前所在的 Host/Group；宿主提交时捕获目标组。整 Host 移窗后跟随该 Host，目标组关闭或交接中明确拒绝，不回退到当前活动组。迟到创建不覆盖用户后续改变的焦点。

回执包含 `commandId,requestId,name,context,status,actionKey,result,error,viewId,hostId,groupId`。未产生结果的字段为 null；error 为 `{code,message}`。

| status | 含义 |
| --- | --- |
| pending | 已接纳，等待分发或原处理结果 |
| completed | 处理者按业务成功契约返回对象；打开终端仅表示逻辑页面登记成功，PTY 和容器连接另行报告 |
| failed | 已知失败，保留已经登记的 viewId，不暗示回滚 |
| invalidated | 关闭、断线、来源结束等撤销继续创建资格；取消关闭不复活命令 |
| unknown | 已发送但通信结束，处理结果无法确认；保留已观察到的页面，不重放 |

方法拒绝使用 JSON-RPC error.message 和 error.data.code；接纳后的失败保存在回执 error。常见 code：invalidParams、staleContext、sourceStopped、workspaceClosing、targetBusy、targetClosed、noHandler、ambiguousHandler、requestConflict、resourceLimit、notFound、notSent、resultUnknown、handlerError、protocolError、pageRejected、startupSuperseded。notSent 表示能确认尚未发送；resultUnknown 不证明没有副作用。处理插件通过 SDK 的 RpcError 返回结构化错误；旧字符串错误映射为 handlerError。

去重身份为完整 Context + 真实来源 + requestId。来源区分逻辑 Host、Host/定义默认创建、插件实例/逻辑 view。requestId 为 1–128 字节；可选 groupId 为 1–128 字节、startupDefinition/actionKey 为 1–256 字节，已知字段类型错误拒绝。commandId 由核心生成。相同身份及输入返回原回执，不同命令/参数/显式目标/入口引用返回 requestConflict。指纹只规范化 JSON 对象键顺序，不改写路径、命令或业务默认值。超时后保存原票据继续 get；若 get 为 notFound，可用原完整输入和原编号重交，不能生成新编号自动补执行。

页面登记和命令关联在同一工作台锁中提交。重复 ui.openView 返回原 viewId，页面已经关闭时附 `closed:true`，绝不复活。处理者返回的 viewId 必须是该命令已登记的页面。来源页关闭会撤销未登记的创建；已登记的终端独立归终端 view，关闭 Docker 来源不关闭它。关闭保留覆盖零页面命令；逻辑 invalidated/unknown 与真实 RPC 在途任务分开，实际响应或进程确认结束后才释放在途额度。重连不把旧票据带到新代次。

| 预算 | 上限 |
| --- | --- |
| payload、页面 params、结果 | 各 16 KiB |
| 命令信封、RPC 帧 | 24 KiB、既有 64 KiB |
| 同来源、同处理实例在途命令 | 各 32 项（失效但未实际结束的调用仍计入） |
| 命令与自主页面申请的回执 | 每 Context 合计 4096 项、全局 16384 项、合计 16 MiB，含结果与在途预留 |

满额先拒绝新编号，不淘汰现存回执以重做动作。连接代次结束后回收旧记录，未结束调用保留至实际结束。极长期工作区可关闭后重新连接。查询只刷新回执，不启动业务。

SDK `run_with_commands(&["terminal.open.v1"], handler)` 使用既有四个 worker，初始化时确认名称；处理函数返回 `Result<Value,RpcError>`。旧 `run` 与 `Client.call` 保留字符串错误接口。`commands::Ticket` 保存 Context、sourceViewId、requestId、name 和 payload，提供 submit/get/resolve；通信超时保留票据。没有新增 iframe 命令桥，UI 事件先交给所属插件后台。

旧 `ui.actions.v1` 的 invokeAction/actionStatus 适配同一命令服务，内部使用固定提供者的 ui.action.v1。旧插件收到的 ui.action.requestId 是核心 commandId，使用它调用旧 ui.openView 即可关联原命令；它不等于发起方编号。旧回执保留字符串错误和 pending/completed/failed/invalidated 形状（unknown 兼容映射为 invalidated）。未声明 command 的旧插件无需升级，不再限制一个工作区只能有一个在途行为。

未关联命令的自主 ui.openView 继续限定为调用插件自己的视图，以实例/Context/requestId 去重；原页面关闭返回旧 viewClosed 错误。带 createActionId 的新视图必须通过显式命令关联创建，不能绕过命令申请。旧 UI openView 桥保持兼容；`mount.capabilities.uiOpenView===1` 仍表示支持进程入口。

### 终端创建参数

终端插件 0.6.0 起支持以下创建配置。

动作 data 和终端视图 params 使用相同契约，null/空对象默认登录 shell。示例：

```json
{"version":1,"title":"api · 容器终端","launch":{"kind":"command","command":"docker exec -it api /bin/sh"},"directoryIntegration":"off"}
```

version 可省略，仅支持 1；title 可省略，非空白、最多 128 字符且不含控制字符。launch 缺省或 `{kind:"loginShell"}` 使用登录 shell；`{kind:"command",command:string}` 要求非空白且不含 NUL。已知字段类型错误、未知版本/kind 拒绝；未知可选字段忽略。总参数最多 16 KiB。

loginShell 缺省目录策略在创建时读取设置，允许 auto/manual/off；command 缺省 off，首版只接受 off。command 只要求 terminal.launch.v1，不做主机探测或目录脚本注入，不回退登录 shell。自动登录 shell 集成仍要求 terminal.launch.v1 和 session.exec.v1，旧核心或不支持的 shell 保留原普通终端降级。

首次挂载保存已解析配置，同 viewId 重挂载复用原 PTY，初始化失败不自动重跑。mount.data.createParams 含可再次使用的配置；command 退出后“重新打开终端”创建新 view/PTY。支持 commands.v1 时，现有 ui.event 的 `{action:"createTerminal",requestId,params}` 由终端后台保存票据并提交 terminal.open.v1；terminalCommandStatus 查询原票据，Surface 重挂载继续查询。缺少该能力的旧核心沿用旧创建接口；新协议业务失败不降级到旧路径。新页面使用当前挂载 Context，不复用原 streamId、输入序号或集成 token。off 不采纳 OSC7/633 的 known 目录，不将嵌套环境路径传给主机文件跟随。

## 会话状态与有界短命令

可选能力 `session.exec.v1`。系统识别、命令文本、指标和调度由插件负责；核心提供当前认证会话上的非交互任务，不暴露凭据或传输类型。

| 方法 | 参数及结果 |
| --- | --- |
| `session.state` | `{viewId}` → `{context,state,closing,capabilities}`；断线/关闭冻结仍可查询，在线 capabilities 包含 `exec` |
| `session.exec.start` | `{viewId,expectedContext,operationId,sequence,command,timeoutMs,maxOutputBytes?}` → `{taskId,state}` |
| `session.exec.poll` | `{taskId,stdoutOffset,stderrOffset}` → 两路 `{dataBase64,nextOffset}`，以及 state/outputComplete/exitCode/exitSignal/error/remoteExitConfirmed/execSent/elapsedMs |
| `session.exec.cancel` | `{taskId}` → `{state}`；取消本任务，不关闭共享连接 |
| `session.exec.release` | `{taskId}` → `{released:true}`；丢弃结果，运行中同时取消 |

Context 使用完整 `workspaceId/serverId/connectionId/generation`。command 最大 16 KiB、不得包含 NUL，无 PTY、无 stdin；timeoutMs 为 1,000–30,000，输出两路合计默认 256 KiB、最大 1 MiB。全局最多 8 个在途任务，每插件进程最多 4 个、64 项记录；按请求预留输出额度，核心 16 MiB、单插件 8 MiB。poll 每路最多 8 KiB；偏移、sequence 使用十进制字符串，同偏移可重读，不重复计量。

同 view/Context 的 sequence 严格递增。相同 operationId 和参数重试返回原任务，参数改变返回 operationConflict；输出回收后旧序号返回 operationExpired，不能重新执行。终态结果 30 秒后回收，正常插件读完即 release。关闭 view、进程退出及应用退出主动回收；重连后旧结果仍可由所属实例清理，不能启动旧 Context 的新任务。

completed 表示收到远端退出状态，非零退出码仍可能 completed。EOF 缺少退出状态为 failed/missingExitStatus，超时为 timedOut，取消为 cancelled。取消尽力终止并关闭自身通道，额外等待最多 1 秒；remoteExitConfirmed 仅表示收到了远端退出确认，不保证已经脱离的子进程结束。

方法错误沿用 JSON-RPC error，`error.data.code` 区分 invalidParams/staleContext/workspaceClosing/resourceBusy/operationConflict/operationExpired/taskExpired/invalidOffset/unsupported。任务错误位于 poll.error。SDK `call_detailed` 保留 code/message/data，原 `call` 的字符串错误接口不变。使用前检查能力，旧插件继续使用原有 API。

## 目录与清单

解压后选择含 `plugin.json` 的目录：

```json
{
  "id": "sodeck.terminal",
  "name": "终端",
  "version": "0.1.0",
  "apiVersion": "1",
  "target": "x86_64-pc-windows-msvc",
  "entry": "bin/sodeck-terminal.exe"
}
```

每个包包含一个目标的程序，五个 target 见分发文档。Unix 入口需要执行位。核心只做必需字段、版本、API 主版本、目标及目录内入口的基本检查。同一 ID 只能有一个活动实例。启用/关闭在重启后生效，核心启动时加载已启用插件。

`version` 按 SemVer 校验，本地目录可使用合法的预发布和构建信息，核心保留原始字符串。`apiVersion` 与插件版本独立，仍按核心 API 主版本检查。官方打包和在线更新的正式版本限制见[分发文档](distribution.md)；当前非正式本地版本不能使用在线更新通道。

清单可选 `updateSource: { "repository": "BgaSol/soDeck", "tagPrefix": "plugin-terminal-v" }`。未声明来源的本地插件可在设置中显式配置。插件仍为 API v1，不增加运行时更新 RPC。发布维护与重启切换见分发文档。

## 通信

stdin/stdout 每行一个 UTF-8 JSON-RPC 2.0 对象，以 LF 结尾，最大 64 KiB（含 JSON 转义和末尾换行；LF 占一个字节，接收 CRLF 时 CR 与 LF 均计入上限，双向均检查）。stdout 只输出协议，stderr 作为诊断日志。请求带字符串或数字 ID，响应使用相同 ID；通知省略 ID。参数错误返回 `-32602`，未知方法返回 `-32601`，持久化等执行失败返回 `-32000`，核心请求队列已满返回 `-32001`，核心处理超时返回 `-32003`。调用方应检查响应，不根据日志推测调用成功。

逐行读取遇到 `Interrupted` 时继续读取当前行，保留已经读到的前缀；其他 I/O 错误沿原有失败路径处理。控制帧超过上限时停止通信。stderr 每行最多 8 KiB（含换行）；超长行读至 LF 或 EOF 后省略正文，记录“诊断日志行过长，已省略”，再继续读取后续诊断。

当前源码的控制通信已改为有界双向分派：每插件最多 4 个处理线程、32 个未完成的插件 → 核心调用；核心 → 插件调用队列及在途表各最多 64 项。接收循环不等待业务处理，响应按请求 ID 匹配，完成顺序可以不同。停止请求有独立队列，写入端优先发送响应、初始化与 shutdown，输出或请求压力不会无限堆积。队列或写入通路持续不可用时明确失败，不静默丢成功响应。

停止插件时取消等待中的核心 → 插件请求；通信结束后旧实例的排队工作失效、迟到结果不再回写。5 秒超时是等待期限，不承诺撤销已经开始的持久化副作用；长远端操作使用单独任务/取消机制。插件自身也必须持续读取控制帧，不能同步等待核心响应时停止读取核心发来的请求。正文不经此控制协议传送。

核心发起：

```json
{"jsonrpc":"2.0","id":"initialize","method":"initialize","params":{"coreApiVersion":"1","coreVersion":"0.6.0","capabilities":["logging.write","messages.publish","ui.v1","rpc.concurrent","terminal.v1","viewData.v1","files.v1"],"dataDirectory":"<系统应用数据目录>/plugin-data/sodeck.terminal"}}
{"jsonrpc":"2.0","id":"initialize","result":{"apiVersion":"1"}}
```

核心等待初始化 5 秒。插件回应 initialize 后可调用以下方法。coreVersion/capabilities 是可选协议信息，旧插件忽略即可；能力表示当前核心实际实现，不是授权清单。真实业务插件检查所需能力后调用 PTY 或文件原语，复用核心会话。插件不能自行声称来源，核心附加 pluginId、instanceId、receivedAt。

```json
{"jsonrpc":"2.0","id":1,"method":"logging.write","params":{"level":"info","message":"服务已启动","fields":{"port":8080}}}
{"jsonrpc":"2.0","id":1,"result":{"accepted":true}}
{"jsonrpc":"2.0","id":2,"method":"messages.publish","params":{"title":"服务就绪","body":"现在可以连接","level":"success","notify":true,"dedupeKey":"service-ready"}}
{"jsonrpc":"2.0","id":2,"result":{"id":"<核心分配的消息 ID>"}}
```

日志级别：debug/info/warn/error。全局每秒最多接收 200 条插件日志，超量返回 accepted=false，并合并记录丢弃数量。单条日志最多 8 KiB，超长正文截断，过大 fields 移除。日志文件每份最多 10 MiB，保留 5 份；内存回退最多 5000 条，界面查询最多 500 条。普通 error 日志不自动弹通知。

消息级别：info/success/warning/error，notify 默认 false；消息持久化成功才返回 ID。最多保留 1000 条。同一插件非空 dedupeKey 在 5 分钟内合并，更新内容、实例、时间和次数并标为未读；合并期间不重复弹提示。标题最多 1024 字节，正文最多 16 KiB。

应用退出或重启停止插件时核心发送：

```json
{"jsonrpc":"2.0","id":"shutdown","method":"shutdown","params":{}}
{"jsonrpc":"2.0","id":"shutdown","result":{}}
```

插件应结束任务、发送收尾日志并退出。核心在 3 秒内继续处理收尾 RPC；超时结束进程组/Windows Job 中的进程，并确认主进程结束。停止期间不能创建新业务任务。stdin 关闭时插件也应自行退出。

API v1 通过 capabilities 扩展真实 PTY、SFTP 与正文桥。插件检查 terminal.v1 / files.v1 / viewData.v1 后使用；旧核心拒绝不支持的方法。不兼容变化增加 API 主版本。

## 真实远程能力

- terminal.open({viewId,cols,rows}) 返回 id；terminal.close({id}) 释放 PTY，结束状态走正文通道。资源固定创建时会话，不自动重放命令。
- files.request({action:"open",viewId}) 返回 id；close/status/save 传 id。save 返回后台任务，status 为 running/completed/failed；关闭协调可等待完成，不阻塞接收线程。
- metrics.networkSnapshot() 返回服务器 socket 量、插件有效载荷、约 3 秒速率与质量。累计为十进制 u64 字符串，不以目录 JSON 代替网络字节。
- iframe 经 sodeck.request("data",request) 调用正文桥。终端 op 为 attach/ack/write/resize/checkpoint/detach，支持 `terminal.history.v1` 的核心另提供 metadata；文件 op="files"，action 为 home/list/stat/read/describe/chunk/stage/save/status/download/cancel/exportDraft。每块最多 16 KiB；按真实 view/实例/Context 校验，不通过普通 ui.event 传正文。download/exportDraft 由核心打开本地保存对话框；导出草稿不要求远端在线。
- 页面以 mounted 消息 flush=true 声明移窗/关闭前需要同步。收到 flush，完成在途同步后回复同 id 的 flushed；携带 error 可阻止丢失输入。旧插件不声明则保留原行为。

所有权见[架构](architecture.md)，业务的缓冲、保存与下载规则见[终端](features/terminal.md)和[文件](features/files.md)。不引入跨插件直调或插件授权流程。

### 终端大历史恢复

initialize 新增 `terminal.history.v1`。新版终端插件初始化时要求该能力，缺失时明确提示升级核心；核心 API 主版本仍为 1，旧插件原 op 保持兼容。

- `data({op:"metadata",id,metadata})` 独立保存当前 view 的小界面状态，最多 8 KiB；要求已有有效订阅和当前 Surface 写入身份，不推进显示快照 offset。恢复 reset 帧的 `metadata.ui` 返回该状态，核心不解释搜索词、阅读位置或选择。
- `checkpoint` 保持 begin/done 和已消费 offset 校验，每块最多 16 KiB，整份最多 128 MiB。done 原子替换旧快照，未完成上传不作为恢复来源；detach 清理该 Surface 的上传。已提交与正在读取的旧快照共享不可变字节，恢复读取完即释放读取引用。
- 正常 ACK 释放已解析的实时队列；原始输出另写入每流最多 128 MiB 的临时分段日志。仅清理已提交快照覆盖、且所有读者均不再需要的日志。日志与所有显示快照分配共用 1 GiB 全局额度；不保证每个并行终端均能同时用满各自额度。
- `kind:"recovery"` 的正文帧以 `metadata.available/error` 报告恢复可用状态。写盘或额度失败时，活动页继续接收有界实时输出；慢读者使用 ACK 背压。新检查点覆盖失败区间后可恢复。重新挂载所需字节出现缺口时明确失败，不能重复执行命令来补数据。
- 自然退出的流保留到用户关闭 view，允许退出页重挂载、搜索和复制。关闭、插件停止及应用退出清理当前资源；启动清理可识别且所属进程已退出的临时目录。不提供关闭终端或应用重启后的搜索档案。

搜索、索引、颜色摘要和预览均属于终端插件；核心不提供搜索 RPC、不读取 shell history，也不接收搜索结果数组。功能及维护边界见[终端](features/terminal.md)。

### 目录树扩展（核心 0.6.1）

新增能力 `files.directoryCursors.v1`。旧版 `list({path})` / `list({next:true})` 保持单目录游标行为；新版文件插件先检查该能力，再使用以下可选参数：

- `list({path,independent:true})` 返回 `{path,entries,more,cursor}`，每页最多 100 项。多个目录可以同时分页，`cursor` 仅在还有后续页时返回。
- `list({independent:true,next:true,cursor})` 继续指定目录，`path` 不参与选择。刷新传 `path,independent:true,cursor` 并省略 `next`，先释放旧游标再读取首屏。
- `closeDirectory({cursor})` 主动释放未完成的独立游标。每个文件资源最多保留 32 个独立游标，超出时释放最早打开的游标；已失效的游标需要重新刷新。读完、关闭页面、连接更换或失效时也会释放。

界面挂载及 `themeChanged` 的 `tokens` 增加 `color-scheme`、`good`、`error`、`shadow`、`file-tree-bg`、`status-bg`、`folder-color`、`folder-fill`、`tree-selection-border`。旧插件可忽略新增字段，新插件提供基础色回退。文件导航状态由插件进程按视图保存，只保存有界路径、选择和滚动元数据，目录内容不进入控制 RPC。

### 工作台外观与文本剪贴板

UI v1 的 `mount.capabilities.clipboard === 1` 表示宿主提供原生文本剪贴板。它是 UI 桥能力，不是插件权限声明。插件 UI 通过 `sodeck.request("clipboard", params)` 使用：

| op | 参数与返回 |
| --- | --- |
| `read` | 原生读取一次，返回 `{id,length}`，length 为 UTF-16 代码单元数 |
| `chunk` | `{id,offset}`，返回 `{text}`；最多 2048 个代码单元，读到末尾释放 |
| `beginWrite` | `{length}`，返回 `{id}`，尚不修改系统剪贴板 |
| `writeChunk` | `{id,offset,text}`，必须按顺序且每块不超过 2048 个代码单元 |
| `commitWrite` | `{id}`，校验完整 UTF-8 文本后一次原生写入并释放 |

每个 surface 最多一个传输，15 秒过期；总上限 256 KiB UTF-8，单请求预留 24000 字节。宿主检查当前可见 surface、焦点和 Context；隐藏、移除和更换 Context 时使旧操作失效。共享 `clipboard-bridge.js` 负责分块与能力检查；旧核心缺少能力时明确提示升级。剪贴板正文不经插件进程或普通控制事件，也不记录和持久化。

`tokens` 保留原颜色字段，新增 `sd-stroke-structure/control`、`sd-icon-stroke`、`sd-control-weight`、`sd-title-weight`、`sd-radius-control`、`sd-tab-height`、`sd-status-height`、`sd-hit-size` 及导航、工具栏、主操作和反馈状态语义色；具体名称以 `src/theme.ts` 为准。终端提供 `terminal-background/foreground/cursor/selection` 与 ANSI16（`terminal-black` 至 `terminal-brightWhite`）。插件将字段设置为同名 `--` CSS 变量，配合打包的 `shared.css` 使用；旧插件可忽略新增项，新插件保留基础色回退。主题变更不重建视图或会话。

### 统一桌面行为和滚动条

桌面一致性改动向 UI v1 增量提供 `sd-scrollbar-thumb`、`sd-scrollbar-hover`、`sd-scrollbar-active`，分别表示滑块默认、悬停及拖动颜色。DOM 页面使用随插件打包的 `shared.css`；终端使用 xterm 的 `scrollbarSlider*Background`，无需替换滚动机制。旧插件可忽略这些字段，自绘滚动条的插件需要主动适配。

核心在所有 WebView frame 中取消浏览器默认动作，保持事件传播；插件应继续处理自己的业务快捷键和菜单，不能将 `defaultPrevented` 自动解释为业务事件已处理。文件与网络插件在 `clipboard-bridge.js` 前加载共享的 `text-menu.js`，通过既有剪贴板能力提供文本菜单；终端沿用自己的菜单。新增字段和共享资源不改变 API/UI 主版本，不改变核心与插件的独立打包边界。

## 通用 TCP 与弹窗（核心 0.7.0 新增）

核心通过 `initialize.capabilities` 声明 `net.tcp.v1` 和 `ui.dialog.v1`，API 主版本保持 1。`net.*` 仅供插件进程调用；业务协议及规则由插件实现。监听持有创建时的已认证会话，域名原样交给最终远端解析，断线后不会自动接到新会话。

| 方法 | 参数 | 返回与行为 |
| --- | --- | --- |
| `net.listener.open` | `viewId,operationId,bind:{host,port}` | `{listenerId}`；后台绑定，`port:0` 自动分配，host 必须是本机 IP |
| `net.poll` | `viewId` | `{items:[{listenerId,streamId}]}`；每次最多领取 16 个本地客户端，不等待 I/O |
| `net.tcp.open` | `listenerId,clientStreamId,operationId,host,port` | `{streamId}`；后台建立远端 TCP，同一本地客户端只允许一个目标 |
| `net.stream.status` | `streamId` | `{state,error}`；connecting/ready/relaying/closed/failed，error 为字符串或 null |
| `net.stream.read` | `streamId,maxBytes` | `{pending:true}` 或 `{data:[字节],eof}`；非阻塞轮询，最多 512 字节，不预读后续正文 |
| `net.stream.write` | `streamId,bytes` | `{queued:true}`；最多 512 字节，按序入队，错误通过状态呈现 |
| `net.stream.finish` | `streamId` | `{}`；排空已排队响应后关闭，用于协议拒绝 |
| `net.stream.splice` | `listenerId,leftId,rightId,operationId` | `{}`；将该客户端及其目标移交原生双向复制，保留半关闭；同一配对重复调用成功 |
| `net.listener.snapshot` | `listenerId` | 监听状态、实际 address、path、error、连接数、sent/received、约 3 秒速率、clients 控制摘要 |
| `net.listener.probe` | `listenerId,operationId` | `{probeId}`；以真实本机入口连接，接受端按预登记端点关联；clients 含对应 probeId |
| `net.close` | `id` | `{}`；取消监听及全部子资源，或指定流；本地流连带其目标，重复关闭成功 |

协商读写合计最多 1024 字节，控制队列 8 项；正常正文只经原生流。每监听最多 64 活动连接、16 待协商连接，每插件实例最多 32 监听，核心最多 256 本地客户端；协商期限约 15 秒，远端连接 10 秒。probe 最多 4 个并发、10 秒期限，检查完成应关闭匹配的客户端流。旧监听 open 的 operationId 在资源存活期间保留，完成后按原创建时间的 60 秒期限回收，最多 4096 项；窗口重挂载应复用 listenerId，不重复创建。tcp open 以本地客户端及 operationId 去重，参数变化拒绝。

`snapshot.error` 为 `{stage,message}`（listen/sessionLost）；单流失败不使整条监听失败。累计量为 u64 十进制字符串，计入远端有效载荷，并归属原有服务器/插件统计；不含本地 SOCKS 协商，不将 SSH 加密开销重复归属插件。关闭视图、服务器断开、插件进程退出或应用退出会清理资源，收起及移窗保持原资源。

清单的视图可声明 `container:"dialog",instancePolicy:"perWorkspace"`，禁止 startup/pin/create/locate 组合。宿主提供标题、关闭、三主题和可滚动容器；按原有 ui.mount/event/beforeClose/save/disposeView 协议工作。iframe 的 `sodeck.request("closeView",{})` 请求关闭当前逻辑视图，仍经过 flush 和保存/放弃/取消；关闭编辑弹窗不关闭同插件的管理侧栏资源。

## 文件批量操作与本地输入

初始化能力包含 `files.operations.v1` / `files.localInputs.v1`。核心只执行单项 IO；递归、冲突选择、失败重试由文件插件进程执行，用户行为见[文件管理](features/files.md)。

- `files.request({action:"batch.begin",id,batchId,context})` 登记已接受批次；`batch.end` 释放。关闭冻结新输入时允许原 Context 的已接受批次继续或取消。
- `op.start` 参数为 `id,requestId,sequence,expectedContext,batchId?,operation`，立即返回 `{id,state}`。sequence 是资源内递增十进制字符串，同 requestId/sequence/Context/参数重复返回原操作，低序号拒绝重做。`op.lookup({id,requestId})` 查询超时请求；`op.poll/result/cancel/release({id,operationId})` 查询、取消和释放，release 只接受终态。
- operation.kind 为 `inspect/list/next/closeCursor/mkdir/createEmpty/copyFile/rename/removeFile/removeDirectory`。端点为 `{side:"remote",path}` 或 `{side:"local",inputId,rootId,relative:[独立分量]}`。单文件保存目标可用 rootName 在原选定父目录改名；上传输入不允许。
- list 返回 `{entries,more,cursor?}`，next/closeCursor 传 cursor。inspect 返回 `{attrs}`，不存在为 null；attrs 含 kind、十进制 size、mtime、stamp，远程目录含 resolved。copyFile 传 source/destination/sourceStamp/destinationGuard，成功返回目标 stamp；guard 为 `{kind:"absent"}` 或 `{kind:"match",stamp}`；其他变更用 endpoint、可选 sourceStamp、guard，rename 另传 destination 且要求目标不存在。
- 状态为 running/committing/completed/failed/cancelled/unknown。completed 的 result 为元数据，失败的 error 为 `{code,message,effect,retryable}`。unknown 表示远程未确认变更结果，不可自动重试。received/total 为十进制字符串。每资源 16 项、核心全局 128 项，结果页最多 100 条且不超过 16 KiB。
- iframe 的文件 data 桥增加 `input.revision/clipboard/drop/chooseUpload/chooseDownload/release/reveal`。chooseUpload 传 directory，chooseDownload 传 single/name；立即返回 waitingInput 操作票据，以 op.poll 等待 inputId 或 cancelled，再 op.release。clipboard 仅解析原生文件格式，drop 传宿主事件 dropId。input.release 传 inputId，reveal 只打开已登记下载目录。
- `input.revision` 兼容增加 `hasFiles:boolean`，返回 `{revision,count,hasFiles}`。该查询只检查本地剪贴板的受支持文件格式，不枚举路径或登记 input；因此此时 count 仍可能为 0，不能据此判断有无文件。缺少 hasFiles 的旧核心按状态未知处理，显式 `input.clipboard` 仍沿用既有行为。远程复制/剪切来源及其可用性由文件插件维护。
- 插件进程用 `input.claim/list/release` 领取/分页读取/释放输入。list 返回 rootId、name、next，最多 4096 顶层项。claim 的 renew:true 仅允许同工作区已领取输入在用户明确重试失败项时重新绑定当前 Context。本地 PathBuf 留在核心，普通文本路径不是上传源。
- mount.capabilities.localFiles 为 1。页面以 `mounted` 消息 `localFiles:true` 声明接收原生拖入；宿主向可见页面发送 localFileDrop（dropType/dropId/x/y，坐标为 iframe CSS 像素），页面领取 drop 后决定业务。此声明不构成插件权限。
- UI 可向宿主发送 `{channel:"sodeck-ui-v1",type:"interaction",surfaceId}` 通知实际指针/键盘交互，供宿主关闭旧菜单和取消过期的自动聚焦。宿主按已有 iframe 来源、origin 与 surface token 绑定核对；不携带按键、文件或终端输入内容。旧宿主忽略此增量消息，旧 UI 仍按原协议加载。
- iframe `request("closeViews",{viewIds})` 立即返回 ticket，宿主异步发送 viewsClosed（ticket/accepted/error）。只允许同插件同工作区页面，将整个集合交给一次核心关闭操作，统一同步输入并决定保存/放弃/取消，全部决定后才释放页面。

旧 files.v1、旧目录游标和文本剪贴板协议保持兼容。文件插件 0.3.0 引入这些扩展并要求核心 0.8.4；当前插件的 UI v2 要求另见本文开头。平台验证范围由插件 CHANGELOG 记录，不属于接口支持推定。

## 文件和监听结果确认（核心 0.23.0 起）

新增能力 `files.operations.v2`、`files.editor.v1`、`net.listenerLifecycle.v1`，核心 API 主版本保持 1。当前文件/网络插件初始化检查对应新能力；缺失时明确要求升级核心，不使用旧接口代替。旧插件仍可使用旧能力，其自身的业务决策不在新协议保证内。

### 文件原语 v2

插件调用 `files.operations({action,id,...})`，action 为 start/lookup/poll/cancel/release。start 必须传固定 requestId、递增十进制 sequence、完整 expectedContext、已登记的 batchId 和 operation；其他端点/配额沿用文件原语。lookup 传 requestId 和 batchId，其余控制传 operationId。重复 start 先返回已登记回执；相同身份不同参数拒绝，release 后旧序号不能重新执行。

新入口拒绝旧 rename，提供独立的 renameNoReplace 原语。当前适配器返回 unsupported，不发送复制或删除作为替代。旧入口的 rename 为兼容保留，不属于新版插件使用路径。

任务失败和方法拒绝保留 `{code,message,effect,retryable}`，方法拒绝位于 JSON-RPC error.data。effect 为 none/unknown，成功通过 completed/result 表达。明确成功提交之后的附属 stat/cleanup 失败记录为 warning/cleanupWarning，不改成可重试失败。通信超时没有这些提交事实，只能继续查询原请求。

`files.request({action:"batch.cancel",id,batchId})` 先封闭批次，再取消已登记操作。lookup 在已封闭且从未接纳的请求上可返回 cancelled/effect:none；取消 ACK 本身不证明已接纳操作未修改目标。batch.end 与新接纳按同一短锁顺序互斥，结束后的批次不能接纳迟到 start。终态落盘后才 release，不淘汰在途或待确认回执来腾出容量。

### 草稿、保存和导出

正文继续走 iframe `data({op:"files",id,action,...})`。新版 action 成功返回 `{ok:true,value}`，拒绝返回 `{ok:false,error:{code,message}}`；code 为 draftRejected/saveRejected/exportRejected，外层挂载或通信失败仍可能直接拒绝 Promise。插件控制面 `files.editor({id,action,...})` 提供 draft.describe、save.start/status/cancel/release 及 export.status/cancel/release；导出 start 必须来自 UI 对话框入口。

| action | 参数与结果 |
| --- | --- |
| draft.describe | 返回 path/size/version、draftRef、savedRevision、writeBlock、saveSequence/exportSequence、exportState 和 source（最近提交上传的身份） |
| draft.chunk | draftRef、offset；返回 data/draftRef，修订不同拒绝 |
| draft.begin | documentId、uploadId、uploadSequence、editRevision、size、sha256；返回 pending/nextOffset 或原 committed 回执 |
| draft.append | documentId、uploadId、uploadSequence、offset、data；每片最多 16 KiB，重复片必须完全相同，不允许乱序或混片 |
| draft.commit | documentId、uploadId、uploadSequence；完整大小/摘要吻合后原子替换草稿，返回 committed、draftRef、上传身份、surfaceId、editRevision、sha256、size |
| draft.status | uploadId；返回最近 committed、当前 pending 或 expired，expired 不能证明保存结果 |
| draft.abort | documentId、uploadId、uploadSequence；封住序号并放弃此上传，不改变完整草稿 |
| save.start | requestId、sequence、expectedDraftRef、expectedContext；接受时固定字节，立即返回任务与 request/draft |
| save.status | requestId 可省略以读取当前投影；状态 idle/running/committing/committed/notCommitted/unknown，附 request/draft，终态含 result 或 error |
| save.cancel/release | 原 requestId；取消尚未接纳的请求还须原 sequence 与完整 start 参数以封住意图。release 仅接受终态，保留退役序号 |
| export.start | requestId、sequence、expectedDraftRef、path（建议文件名）；立即返回 waitingInput/request/draftRef，只有首次接纳才弹框 |
| export.status/cancel/release | 原 requestId，取消未接纳意图还需完整 start 参数；状态 waitingInput/writing/completed/failed/cancelled。取消不结束仍可见的原生对话框，release 只接受实际终态 |

DraftRef 为 `{documentId,revision}`。revision、sequence 和 editRevision 用十进制字符串。草稿上限 1 MiB；每个 Surface 一个上传和最后提交身份，每个文件资源各保留一个保存及导出回执，下一任务前先确认并 release。上传序号在 Surface 内单调，保存/导出序号在文件资源内单调，不随移窗重置。

候选 Surface 只能读草稿和状态；提交在最终输入归属锁下复核。保存与导出固定接受时的 DraftRef，后续输入不能更改已固定的字节。保存提交发出后继续等待协议结果，取消不抹去真实提交；成功 ACK 后 stat 失败仍为 committed，并以 baselineUnavailable/writeBlock 阻止继续原位保存。unknown 禁止新保存，release 不解除 writeBlock。导出不访问 SSH，选路径后复核原 Surface 和取消状态再写固定快照；本地覆盖不保证防住外部并发写入。

同一文件只维护一份草稿。首次使用新版上传/保存/导出后，旧 stage/save/exportDraft 拒绝混用；旧 describe/chunk/status 读取同一事实，legacy status 把 committing 映射为 running、unknown 映射为 failed 并保留说明。旧导出也在弹框前固定字节，但同步接口仍受原桥等待期限限制；新版插件使用后台任务。

### 监听生命周期

插件调用 `net.listener.create/lookup/cancel/release`，均传 viewId、expectedContext、固定 operationId、递增十进制 sequence；create 还传 bind。按插件实例/view/Context 隔离，保留退役序号，活跃回执不按时间过期。回执/旧 open 合计最多 4096 项，scope 也有 4096 项上限；原归属退役且实际任务完成后回收。

create/lookup/cancel 返回 observed（listenerId/snapshot/cleanupComplete）、notObserved（cleanupComplete:false）或 retired（cleanupComplete:true）。相同身份不同绑定参数拒绝；cancel 先封住序号，先于 create 到达也不能再创建。retired 只表示意图封闭且没有活跃资源，或核心确认原归属退役且资源全部退出。RPC 错误、Context 失效本身不能作为清理证明；方法错误 data.code 为 networkRequestRejected。

snapshot 增加 cleanupComplete；只有监听、客户端流、远端通道、转发和探测任务全部退出才为 true，closed/failed 字符串不能替代它。release 只接受已确认清理的回执，低序号重放不能创建；net.close 仍仅接受取消请求。规则保存/删除不进入核心协议。

## 组件设置（可选 settings.v1）

组件设置从核心 0.9.0 提供，页面保存所属设置页从核心 0.13.0 提供。插件在清单增加可选 `settings: {apiVersion:"1", schemaVersion:1, pages:[...]}`，一个插件可声明多个组件页，一个页可关联多个 `viewTypes`。同一视图最多关联一个页；稳定身份为 pluginId/pageId/key，不含服务器、Tab 或程序版本。字段、默认值和生效方式由插件声明，核心提供基础校验、存储、版本选择、通用表单和变化通知。正式示例见各插件的 `release.json`，功能与维护约束见[设置](features/settings.md)。

页面包含 id/title/icon/order/viewTypes/groups；分组包含 id/title/fields。字段为 key/type/title/default/apply，可带 description。首版类型为 boolean、number（min/max/step/unit）、string（maxLength）、select（options 的 value/title）、font（presets 的 id/title/regular/bold）；apply 为 live 或 newInstance。`visibleWhen:{key,equals}` 只引用同页无条件的 boolean/select 字段。隐藏字段仍需完整保存并校验。

font 值为 `{source:"preset",id:"jetbrains-mono"}` 或 `{source:"local",family:"Consolas"}`。预置字体路径相对插件安装目录，要求包内 WOFF2；核心只按当前页声明读取对应资源。本机字体按名称加载，失败显示说明并回退；不传字体文件、CSS 表达式或安装绝对路径。

initialize 的 capabilities 含 `settings.v1`，settings 数组只列 pageId/definitionToken/revision。插件返回 `settingsApiVersion:"1"` 后核心才向其发送变化请求；没有该声明的旧插件不接收新方法。初始化期间也可读取设置，读取不等待插件生命周期锁。

| 方向与方法 | 参数 | 结果与语义 |
| --- | --- | --- |
| 插件 → 核心 `settings.get` | `{pageId}` | `{pluginId,pageId,definitionToken,revision,values}`；限所属当前实例，返回有效值 |
| 核心 → 插件 `settings.changed` | `{pageId,definitionToken,revision,values}` | 使用带 ID 的普通 RPC 请求并等待回复；插件通过标准 RPC error 报告应用失败 |
| UI mount/settingsChanged | settings 单页快照、`capabilities.settings:1` | 宿主转发当前 surface 对应的页，隐藏组件可在显示时应用 |
| UI → 宿主 `sodeck.request("settings",...)` | `{op:"get"}`、`{op:"applied",pageId,definitionToken,revision,error?}` 或 `{op:"save",requestId,definitionToken,baseRevision,values}` | 当前 surface 读取、回报或保存其唯一关联页；保存不接受页面自报的插件身份，返回 persisted/conflict/snapshot |

revision 为十进制字符串，definitionToken 为插件当前程序版本、schemaVersion 和规范化页面声明的稳定摘要。核心保存命令接受 definitionToken/baseRevision/requestId/完整 values；旧页面返回冲突及最新快照，相同请求 ID 与正文的近期重投不会重复保存。保存成功与运行应用分别报告；应用失败可重投当前修订，插件应允许同修订重试并忽略过期更新。

每插件最多 8 页，每页 8 组/64 字段，每字段最多 32 个选项或字体。单页保存值和运行快照各限 24 KiB，RPC 帧继续限 64 KiB；设置文件限 4 MiB。用户保存严格校验，载入旧非法值则保留原值、提供有效默认和提示。存储独立于业务规则、凭据和工作区；临时文件原子替换、备份与失败保留沿用核心存储机制。

升级待重启时继续使用旧进程的声明及资源，实际激活新版后才更新；新增字段补默认，兼容的用户值保留，结构变更历史最多保留两份。新版启动失败恢复旧程序时设置随之恢复适用定义，修订不倒退。首版不提供任意迁移脚本；插件作者改变 key/type/单位含义时需单独设计兼容迁移。

## 共享运行状态（可选 sharedState.v1）

核心 0.13.0 起可用。initialize 的 capabilities 含 `sharedState.v1`；页面 mount 的 `capabilities.sharedState` 为 1。旧核心没有能力时保留原业务并禁用联动。核心保存内存中的当前事实，进程与页面经接口访问；不持久化，不提供插件之间直接调用或历史事件日志。

每条记录由完整 Context、topic、producer、key 标识。Context 包含 workspaceId/serverId/connectionId/generation；producer 为 `{kind:"core"}` 或 `{kind:"plugin",pluginId,pluginInstanceId,viewId}`。核心推导调用者身份，put/remove 只能修改自己的来源。修订号为规范 u64 十进制字符串；JSON null 是存在的值，删除用 `present:false` 墓碑，读取未存在的版本为 `"0"`。

进程参数必须带 viewId/expectedContext，页面使用 `sodeck.request("state",{op,...})`，宿主绑定真实 surface。页面返回 `{result}` 或 `{error:{code,message,data?}}`；进程沿用 JSON-RPC error，恢复代码在 error.data.code。

| 方法 | 参数（省略公共身份） | 结果 |
| --- | --- | --- |
| `state.get` | topic/key、可选 producer（默认自己） | scope 与 record；record 含 present/revision/value?/receivedAt? |
| `state.put` | topic/key/expectedRevision/value | revision/changed；整条替换，重复相同值不刷新时间 |
| `state.remove` | topic/key/expectedRevision | revision/changed；CAS 删除 |
| `state.list` | filter，或 cursor/release? | 固定 snapshotId/scope/revision/records/nextCursor |
| `state.subscribe` | clientSubscriptionId/filter | 初始快照页、subscriptionId/leaseMs |
| `state.subscribe` | clientSubscriptionId/filter/acknowledgedRevision | 完整读完后确认初始快照，或续订；返回 confirmedRevision/leaseMs |
| `state.unsubscribe` | subscriptionId 或 clientSubscriptionId | 幂等释放；先取消再迟到初始化也不能重建同 ID |
| 页面 `state.ack` | subscriptionId/batchId/part/appliedRevision? | 中间帧确认不推进游标，最终帧必须确认完整 toRevision |

filter 为 `{topics:[...] ,producer?,key?}`，需要 1–8 个明确主题。快照页可重读，包括最后一页；游标绑定原来源和 Context，空闲 30 秒到期。初始快照确认前缓存增量，确认后才开始投递。订阅租约 60 秒，客户端应每 30 秒续订。

核心发送带 ID 的 `state.changed` 请求，或向目标页面发送 `sodeck:stateChanged`。帧含 subscriptionId/clientSubscriptionId/context/batchId/fromRevision/toRevision/part/parts/changes。change 为 `{op:"put"|"remove",record}` 或 `{op:"scope",scope}`；part 从 0 开始，全部帧原子应用后推进 toRevision。进程回复 `{accepted:true,batchId,part,appliedRevision?}`；同一帧重试内容不变，最近一批重复确认幂等。未确认的进程投递最多重试三次，通知等待按到期回收；不会阻塞生产者或保留无限 RPC。

错误代码包含 invalidParams/revisionConflict/workspaceClosing/staleContext/sourceStopped/scopeClosed/resourceLimit/snapshotExpired/subscriptionExpired/resetRequired。冲突需重新读取并判断最新意图；不得拿新版本重放旧目标。收到 resetRequired，或本地租约到期，应弃用投影并用新客户端 ID 重同步。旧 Context/source 停止不自动恢复。冻结或断线后既有读取/续订/ACK/释放仍可执行，写入拒绝；scope.connected/frozen 独立。

额度：记录 16 KiB，进程帧 32 KiB、页面帧 24 KiB（含路由余量）；范围数据 4 MiB，全局当前与临时数据各 32 MiB，每范围 4096 个记录/墓碑，每实例 32 个订阅，单订阅待发与在途共 1 MiB。超限保留旧事实或要求重新同步。view/实例停止清理其事实，surface 销毁只释放页面订阅，移窗保留逻辑事实，重连创建空的新 Context。

页面使用随插件打包的 [state.js](../plugins/shared/state.js)：`new SharedState(mount)`，`request(op,params)`，`watch(filter,onProjection)`，最终 `dispose()`。watch 会分页、原子拼批、确认、续订及重同步；onProjection 收到 null 表示当前不可用。Rust 使用 [plugin-client::shared_state](../crates/plugin-client/src/shared_state.rs) 的 request/Watch::open/snapshot/renew，持有 Watch 时每 30 秒调用 renew；snapshot 为 None 或 renew 失败时由调用者在有效 Context 内重建。两者使用同一组线协议样例测试。

首批业务契约：Core 发布 `workspace.presentation.v1/current` 的 activeEditorViewId；终端发布 `terminal.location.v1/current` 的 state/path/pathStyle/filesystemView/terminalId/outputOffset/observedAt；系统状态发布 `filesystem.mounts.v1` 的 mount 与 scan-status，以及同源同 mountId 的 `filesystem.capacity.v1`。容量字节数使用十进制字符串，业务新鲜度由 observedAt/staleAfterMs 表达。扫描 coverage 与 publishState 分开：complete/partial/unsupported 描述采集覆盖，publishing/synced/limited 描述共享副本。没有跨记录事务。

## 远程位置解析（可选 files.location.v1）

initialize 宣告 `files.location.v1`，页面 `capabilities.fileLocation:1`。现有 files 数据通路增加 `action:"resolveLocation"`，传入 path/pathStyle（posix 或 windows）/filesystemView（当前只支持 session-default），返回经当前文件服务 realpath/stat 验证的 path/attrs。原生 Windows 盘符绝对路径转换后仍需服务端验证；UNC、相对路径、容器或未知视角返回 unsupportedLocation，不用客户端 OS 猜测。返回和应用前继续校验 Context。

## 可选 PTY 启动与目录对应（核心 0.14.0）

initialize 增加 `terminal.launch.v1`。`terminal.open` 可传 `command` 字符串（非空、无 NUL、UTF-8 不超过 32 KiB）；在同一 PTY 执行该启动命令，不传时保持默认登录 shell。命令解释属于插件，核心继续统一管理资源、取消和 30 秒打开期限，不重放失败启动。

`workspace.presentation.v1/current` 增加可选 `activeEditorPluginId`。`terminal.location.v1/current` 保持 known/unknown，新增可选 activity（starting/ready/busy/unavailable）、reasonCode、title、shell。busy 的 state 为 unknown，消费者不得把旧路径当作当前事实，但应保留已处理的目录以避免手动导航回跳。

页面能力 `fileLocationProof:1` 允许 `resolveLocation` 接受 `filesystemView:"shell-reported"` 和 `witness:{path,value}`。核心通过当前文件服务检查标记为普通文件、内容完全匹配（上限 128 字节），再执行目标 realpath/stat；失败返回 unsupportedLocation。此标记只说明文件系统对应，不是认证或授权。旧文件插件只接受 session-default，因此不会误用新报告；新插件在旧核心上明确降级。原 session-default 及手动 OSC 7 行为保留。

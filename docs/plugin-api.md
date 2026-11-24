# 外置插件 API 2.0

本文定义当前核心与插件的唯一 SDK 基线。功能操作见[功能索引](README.md)，工程与打包见[开发指南](development.md)。插件是可信扩展，分进程用于生命周期与故障恢复；不声明权限列表，不引入授权或沙箱流程。

## 版本与 SDK 边界

清单必须声明 version、apiVersion:"2.0"、target 和包内入口。API 使用规范 major.minor：主版本相同且核心 minor 不低于插件要求才可加载。API 1 插件停止执行，但已安装登记、启用意图、数据和更新来源保留，可在管理页升级；不加载历史 UI、能力或 RPC 适配。未来变更通过显式 API 版本与插件升级处理。

- [plugin-protocol](../crates/plugin-protocol/src/lib.rs) 是核心与 Rust 客户端共同使用的数据契约；[Request](../crates/plugin-protocol/src/requests.rs) 定义当前控制方法、参数、Context 和错误。业务 payload、共享状态值及插件 UI data 仍由插件定义。
- [plugin-client](../crates/plugin-client/src/lib.rs) 统一初始化、有界并发收发、命令票据和文件编辑生命周期。插件通过 `client.request(api::call::Target(api::ViewContext { ... }))` 等封闭请求入口调用服务；稳定的会话、程序、exec/process 和资源回执由关联类型解码，领域快照及插件自定内容保留 JSON。底层原始方法名发送为私有实现。初始化前或重复初始化失败，不执行业务回调。反向调用先按共享 `Callback` 校验方法、Context 与核心字段，插件业务状态机可以投影已校验的回调内容。
- [页面 SDK](../packages/plugin-sdk/src/index.js) 与[声明](../packages/plugin-sdk/index.d.ts) 随插件独立打包到 ui/sdk。每个 Surface 只有一份 pending 表与事件分派；更换或卸载时拒绝全部等待并清理定时器。64 个等待请求上限，data 等待 45 秒、其他请求 8 秒；已发送超时标为结果未知。
- 标准[文件编辑组件](features/file-editor.md) 独立打包 Monaco；终端、Docker 日志等业务依然归所属插件。核心安装包不包含业务插件资源。

## 清单与初始化

```json
{"id":"example.plugin","name":"示例","version":"1.0.0","apiVersion":"2.0","target":"x86_64-pc-windows-msvc","entry":"bin/example.exe","updateSource":{"repository":"BgaSol/soDeck","tagPrefix":"plugin-example-v"}}
```

version 按 SemVer 校验；API 与组件版本独立。每包对应一个正式支持的平台架构，Unix 入口保留执行位。ui、commands、settings 可按实际使用声明，但它们不再分别声明 apiVersion。

stdin/stdout 使用 UTF-8 JSON-RPC 2.0，一行一帧、LF 结尾，上限 64 KiB 含转义与换行。stdout 只输出协议，stderr 进入诊断。核心提供 4 个处理线程、32 个插件发起的未完成请求；核心发往插件的队列及在途表各 64 项。读循环持续接收嵌套请求和乱序完成；5 秒等待不撤销已开始副作用。

初始化请求为 {coreApiVersion,coreVersion,dataDirectory,settings}。SDK 核对基线并调用插件初始化处理，返回 {apiVersion,commandHandlers}；commandHandlers 必须与清单一致。初始化中仅允许 settings.get。coreVersion 用于展示，不参与历史功能探测。退出通过 shutdown 及进程监督收尾，超时可强制结束所属进程树。

RPC 参数错误 -32602、未知方法 -32601、领域执行失败 -32000、未接纳/队列满 -32001、已发等待超时 -32003。error 保留 {code,message,data}，data.code 是领域恢复码，data.dispatch 区分 notSent 和 unknown。页面成功直接返回结果，失败抛出带 code/data 的 Error。日志、错误文本与百分比都不证明操作成功；超时查询原票据，不盲目重发变更。

## 插件图标

icon 与视图/设置图标名只在自身 icons 映射解析；未映射或加载失败使用通用插件图标。最多 32 项，名称 1–64 位 ASCII 字母/数字/短横线；SVG 路径相对包根、最多 256 字节，每文件最多 32 KiB，不允许绝对或上级路径、反斜线、冒号、百分号、查询和片段。SVG 作为单色外部遮罩，不插入宿主 DOM。

打包携带映射资产，运行按实际包版本与路径检查；图标不依赖进程或 UI 存活。待重启包不提前显示，官方目录预览不覆盖已安装包。

## 会话目标、短命令与终端

session.target.describe({viewId,expectedContext}) 返回完整 context/state/closing/capabilities/target/targetSource。target 为 null 或 {name,protocol,host,port,endpoint,username}，来自实际接纳会话，不从后来编辑的服务器目录回填。targetSource 为 current/lastConnected/unknown；失败重连保留原事实。capabilities 中 exec/files 是当前会话事实，不能证明远端程序或 SFTP 存在。

session.exec.start/poll/cancel/release 提供有界短结果，session.environment.start 提供环境采集；参数见共享 Request。session.program.prepare 只编码给定环境下的程序、参数、检查和退出策略，不执行。terminal.open/close 管理 PTY；Surface 输入、Barrier 与 Replay 恢复保留专有语义。三个执行服务共用执行器，不共用一张万能任务表；传输实现和认证只归核心。



## 通用远端进程

核心为插件提供同一远端执行资源的启动、原字节捕获与顺序输入。它复用核心连接与生命周期；不承担密码提示识别、业务命令生成或终端屏幕仿真。

| RPC | 参数与结果 |
| --- | --- |
| session.process.start | viewId、expectedContext、operationId（1–128 UTF-8 字节）、sequence（u64 十进制字符串）、command（1–16 KiB，无 NUL）、maxOutputBytes（1 KiB–64 MiB）、retention（fixed/rolling）、timeoutMs?（1000–3600000 或 null）、programOutput?（startup-v1，仅 pipe）、io?；返回 processId/status |
| session.process.lookup | 原完整启动参数；known、已知时 processId/status；只查原票据 |
| session.process.read | processId/offset（完整信封边界的十进制字符串）；chunks/nextOffset/watermark/earliestAvailableOffset/status，每页最多 48 KiB JSONL |
| session.process.status | processId；原执行、输入与输出水位事实 |
| session.process.input | processId/inputSequence/action；write 带 dataBase64（解码后最多 16 KiB、编码最多 24 KiB），eof 无内容，resize 带 cols/rows；返回原 status |
| session.process.cancel | processId；只取消原通道，返回事实，不能证明全部远端子进程退出 |
| session.process.release | processId；封闭并取消，停止订阅，等待在途资源；releaseRequested/resourceReleased/status |

io 固定为 `{kind:"pipe",stdin:"closed"|"open"}` 或 `{kind:"pty",cols,rows}`，缺省 pipe/closed；尺寸 cols 2–1000、rows 1–500。核心缺省没有总 deadline，由调用服务决定期限；启动握手仍最多 30 秒。PTY 不接受 startup-v1 前导或独立 eof，pipe 不接受 resize；未运行资源不能升级为 PTY。

status 分别保留 execSent（启动发送已请求）、launchQueued（本地 SSH 库已排入消息）、launchAccepted（远端 Success）、phase（opening/running/finishing/finished）、remoteExitConfirmed、exitCode/exitSignal/error、finished、workerFinished、outputComplete。队列成功不等于服务器接受。输出同原捕获：pipe 用 v1 stdout/stderr，PTY 用 v2 terminal，数据块最多 8192 字节，Base64 保留原字节。退出尾部和写盘完成后才标 workerFinished。

input 为 state/available/nextInputSequence/last；last 带 sequence/action/state/sentBytes/error。1 起的连续序号只保留最近一项操作和 oneshot 完成回执；重复相同参数先返回原事实，冲突 inputConflict、旧号 inputExpired、跳号 invalidSequence。新输入先检查原 Context、资源接纳和模式；启动未接受 inputNotReady、pending 或队列满 resourceBusy、封闭 inputClosed。参数拒绝不推进序号。接纳后结果 pending/sent/failed/unknown 固定归原序号；sent 为 SSH 写调用成功，failed 为确定未发，unknown 为开始发送后未确认。EOF 接纳即封闭后续输入，重复 EOF 仍返回原回执；取消后未发输入失败，正在发送的结果如实未知。


实现：[session_process.rs](../src-tauri/src/session_process.rs)、[remote_execution.rs](../src-tauri/src/remote_execution.rs)、[execution_output.rs](../src-tauri/src/execution_output.rs)。原始启动 RPC 去重沿用 view/Context/operationId 与完整规范化参数，release 后旧 sequence 不能重启；实例停止或工作区关闭清理原资源。

### 进程正文订阅

UI使用`sodeck.request('data', {op:'processOutput', action, id, ...})`。attach可提供完整信封边界offset（十进制字符串），收到reset/data/caughtUp/gap/end的Frame，offset是块末尾绝对偏移。每包按packet确认ack后继续发送；重复同一ack可重试。gap要求丢弃未完成信封/记录并按新offset重新建立解析状态。detach仅结束该Surface订阅，不关闭远端命令。

read提供offset和固定watermark，返回至多16KiB的data与nextOffset；跨缺口只返回可用连续部分，缺失起点明确失败。pin提供start/end，返回pinId；unpin释放窗口。候选Surface只读attach/read，激活Surface才可pin/unpin。原捕获可在新Context中只读恢复，不能将其Owner改成新连接。

全核心最多24条在运行进程，每插件12条、每View3条；每插件最多64条保留资源。每View滚动额度64MiB，固定捕获与滚动pin合计64MiB；pin最多8段/资源，重复范围保守计入额度。恢复journal与终端共用1GiB全局磁盘额度，每journal128MiB，分段边缘另受全局额度约束。关闭View/插件/核心取消执行和订阅，实际任务完成后回收；Surface消失由订阅检查或宿主detach结束。缓存只用于当前进程恢复，不是持久日志档案。

### 本地内容导出

UI使用`sodeck.request('data', {op:'contentExport', action, ...})`，由核心打开本机保存对话框；插件只传建议文件名和字节，不提供本地路径，不借用远程文件编辑资源。

| action | 参数与效果 |
| --- | --- |
| begin | requestId（1–128字符）、name（无路径分隔符/控制字符，至多256字符）、byteLength（u64十进制字符串，至多128MiB）；返回id、started、status；同Surface同票据同参数复用原对话框 |
| status | id；state为waitingInput/receiving/committing/completed/cancelled/failed，receivedBytes为十进制字符串，失败附error |
| append | id、offset（十进制字符串）、data（1–16KiB字节数组）；严格连续，只允许最后一块相同offset/字节的回执重试，冲突失败 |
| finish | id；完整接收后同步同目录临时文件并替换；completed才能报告成功；相同完成请求可复查 |
| cancel | id；清理自己的临时文件，保留原目标；不逆转已经完成的提交 |

每View最多一个活动导出，全核心4个；只允许最初激活Surface提交。关闭或移动时先排空导出/明确取消；提交前再次核对输入权，目标在选择后变化时拒绝覆盖。连接断开不影响本地导出。正文等待60秒、对话框等待300秒，status/新请求及保存框回调会检查期限；迟到的路径选择不重启已超时或取消的任务。写入失败同时保留临时文件清理错误，清理失败的路径留到资源释放时再尝试删除。现有目标按长度和修改时间检查，不提供外部应用同时改写目标的事务隔离；崩溃后临时文件不是已保存结果。

实现：[process output](../src-tauri/src/session_process/output.rs)、[capture.rs](../src-tauri/src/streams/capture.rs)、[content_export.rs](../src-tauri/src/content_export.rs)。

## 文件编辑 SDK

SDK 是随插件编译的客户端与 UI 源码，不是新增核心 RPC。它使用当前文件、视图和页面数据接口。功能与限制见[通用文件编辑](features/file-editor.md)。

### 插件进程

从 sodeck_plugin_client::file_editor 导入 EditorHost。每个插件实例保留一个 EditorHost::new(view_type)，view_type 对应插件自己声明的中央 perResource 视图。来源事件调用 source_event(client, params)，返回值包装在普通 UI 响应的 data 中。来源最终释放时调用 dispose_source(client, view_id)，不在 Surface 迁移时释放。

| 来源事件 data | 字段与效果 |
| --- | --- |
| fileEditor.resource | expectedContext 必须与 RPC Context 完全一致；为来源 viewId 懒创建本组件的文件资源，返回 id；创建中再次请求明确失败，可稍后重试 |
| fileEditor.open | expectedContext、requestId、viewType、path；viewType 必须匹配构造参数，path 是来源资源 stat 返回的规范路径；调用 ui.openView，以 path 作为 resourceKey，params 为 {path} |

将编辑视图的 ui.createView/getView/event/beforeClose/save/disposeView 交给 handle(client, method, params, before_write)。返回普通 {data,metadata}，错误为 Result<Value,RpcError> 的 Err；不包装另一层 RPC。before_write(Context,path) 在创建与保存前执行短业务检查，不能把数据正文放在这里。编辑事件为 save、editorState、dirty、staged、saved、busy、position、error；下载等组件业务事件仍由组件分派。

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

release.json 声明 build.fileEditor:true；设置页使用 {id,viewTypes,fileEditor:true}。打包展开 packages/file-editor/settings.json，复制 SDK 的 JS/CSS/Worker/许可证及设置字体；模板标记不进入 plugin.json。SDK 与使用插件同包更新，不另建运行时版本协商；核心 API 基线为 2.0。不依赖文件插件的 ID、目录或进程。

## 工作台与显示交接


服务器 Context 和业务 view 在核心登记。一个工作区可以在多个原生窗口中拥有显示部分，同一 view 只归一个显示位置；新建实例、同窗移组与跨窗移动各有明确行为。PTY、文件任务和网络监听仍归插件实例/view/Context，窗口或布局变化不重建业务资源。

| 插件进程方法 | 契约 |
| --- | --- |
| `ui.createView` | 逻辑实例初始化；同 view/Context 幂等，创建 PTY、文件句柄或插件状态。核心按当前实例与 Context 缓存结果，失败不会靠反复挂载重试创建 |
| `ui.getView` | 读取已存在实例的显示数据；保持原 PTY/草稿/任务，不创建资源，不替代视图创建。大正文和终端输出继续走既有数据流 |
| `ui.event`、`ui.beforeClose`、`ui.save`、`ui.disposeView` | 继续使用原业务与关闭契约；迁移期间不调用 dispose，真正关闭时才清理实例 |

界面消息通道为 `sodeck-ui-v2`。宿主 `mount` 附带 `surfaceId`、`readOnly`、Context、数据；每次挂载取得新的输入身份。插件用 `prepared`报告显示准备完成，并声明 `flush:true` 以参加输入同步。准备完成只说明可显示，插件须等待 `surfaceReady` 后才启用编辑、输入、PTY resize 和状态发布。

跨窗交接依次为 preparing → draining → restoring → 提交：目标先建立只读候选，源端 `flush` 同步草稿/输入并以 `flushed` 回应；核心冻结旧表面，等待已接纳调用和终端队列排空，目标重新读取最终状态并接续终端显示检查点及输出水位。目标就绪后同一次状态提交改变位置和输入权，旧表面失效。任一步失败保留原位置和实例，向源端发送 `transferCancelled` 恢复输入。输入法组合尚未完成时插件可以拒绝 flush。

UI v2 的 `flush.freeze` 为 true 时表示迁移或关闭的输入交接，源界面在同步后保持冻结，直到卸载或收到 `transferCancelled`。普通同步的 freeze 为 false，仅等待已经接纳的输入和草稿队列，完成后保持可编辑。插件不能把普通同步变成永久只读。

同窗布局保留 iframe，不需要重新 get/create。切换标签、隐藏工具和紧凑显示不关闭后台业务。剪贴板、正文写入、终端 resize/checkpoint 和共享状态发布使用真实 Surface 身份检查；候选可以读取输出，不能输入或替旧表面提交检查点。

侧边入口仅由 `container:"sidebar"` 生成，跟随核心 Host 的实际位置与完整顺序。未创建工具以稳定定义 key 的空槽位参与移动和记忆，不分配 viewId/Surface，不调用插件创建方法。中央及其他窗口中的工具从更多入口定位，`editor/dialog` 不进入工具条；核心自行处理停靠、按钮拖动、排序与保存回执，插件不新增左右位置设置、停靠 RPC 或权限声明；同窗移动继续保留原输入身份和业务资源。

核心桌面命令 `workbench_move` 以 `requestId`、Context、`scope:{kind:"view"|"group"|"host",id?}` 和目标窗口/组/侧栏表达移动。`component_ui.moveStatus` 查询原回执；相同编号和参数复用结果，更换参数拒绝。已结束回执五分钟后返回 expired；进程内最多保留 4096 个请求，达到额度后明确拒绝新编号，重启清理。超时只查询原结果，不自动再次移动或创建窗口。

`workbench_move.destination.returnToSource:true` 请求核心按运行期书签定位原窗口、标签组、相邻标签或工具侧栏。`workbench_return_window({requestId})` 返回调用窗口的全部 Host；核心先统一保留，再准备所有工作区的候选，全部恢复后在一次状态锁内提交。任一成员失败均不释放其他成员的来源 Surface。窗口投影增加 `incomingWorkspaces` 与 `windows`，候选挂载使用各自的 Context 和迁移编号。


焦点由实际页面交互更新。`workspace.presentation.v1` 增加 `focusedViewId/focusedWindowId` 与 `lastInteractedLocationViewId`。终端可通过可选 `locationProvider:"terminal.location.v1"` 声明目录来源；文件跟随使用本服务器最近操作的目录提供页，经核心共享状态协作，不直接调用终端插件。

布局文件只保存服务器的容器结构与工具定义位置；不保存 Context、viewId、Surface、正文或命令。当前交互见[工作台](features/workbench.md)，资源与交接职责见[架构](architecture.md)。

### 命令、默认行为与进程主动创建页面


命令服务在双向 RPC 上提供一次动作的接纳、路由和回执。共享状态仍表达当前事实；命令用于“请执行一次动作”。没有持久消息队列、离线重放、广播执行、自动启动插件或失败转投。插件按可信扩展运行，命令声明不构成权限列表。

插件清单可声明：

```json
{
  "commands": {"handlers":["terminal.open.v1"]},
  "ui": {
    "actions":[{"id":"new-terminal","title":"新建终端","defaultCreate":true,"dataVersion":1,"command":"terminal.open.v1"}],
    "views":[{"id":"terminal","title":"终端","icon":"terminal","container":"editor","instancePolicy":"multiple","createActionId":"new-terminal","openOnWorkspace":"createOne"}]
  }
}
```

每插件最多声明 32 个唯一命令。名称最多 128 字节，由 ASCII 字母、数字、下划线和短横线分段，以点分隔，最后是规范的 `.v<正整数>`（u32；无前导零）。`terminal.open.v1` 是业务契约版本，不兼容业务变化使用新的主版本名称。

初始化结果必须返回与清单完全一致的 `commandHandlers` 数组；不一致时启动失败。处理者只在实例 Running 且握手确认后可路由，停止即撤销，不依赖 iframe。零个处理者返回 noHandler，多个返回 ambiguousHandler；提交时固定唯一的处理实例，不按加载顺序选择，不转交替代实例。resolve 只查询当前可用性，submit 仍重新检查。

`ui.actions` 保留入口的标题、图标、默认选择和 dataVersion，不另建执行队列。`command` 必须是本插件声明的命令；`createActionId` 引用本插件带命令的 action，首版只允许 editor + multiple 的独立新建页面；单例恢复和 perResource 资源打开保持既有契约。命令处理者仍可显式申请本插件其他已声明视图。默认页面创建和中央“＋”使用同一命令。默认选择仍保存到 ui-layout.json；已有选择不因提供者停止自动切换。

| 方向 | 接口及必要参数 | 返回 |
| --- | --- | --- |
| 插件 → 核心 | `commands.resolve({expectedContext,sourceViewId,name})` | `{available:true,name}` 或结构化错误；不执行 |
| 插件 → 核心 | `commands.submit({expectedContext,sourceViewId,requestId,name,payload})` | 立即返回同一命令回执，不同步等待另一个插件 |
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

方法拒绝使用 JSON-RPC error.message 和 error.data.code；接纳后的失败保存在回执 error。常见 code：invalidParams、staleContext、sourceStopped、workspaceClosing、targetBusy、targetClosed、noHandler、ambiguousHandler、requestConflict、resourceLimit、notFound、notSent、resultUnknown、handlerError、protocolError、pageRejected、startupSuperseded。notSent 表示能确认尚未发送；resultUnknown 不证明没有副作用。处理插件通过 SDK 的 RpcError 返回结构化错误；业务处理错误使用 handlerError。

去重身份为完整 Context + 真实来源 + requestId。来源区分逻辑 Host、Host/定义默认创建、插件实例/逻辑 view。requestId 为 1–128 字节；可选 groupId 为 1–128 字节、startupDefinition/actionKey 为 1–256 字节，已知字段类型错误拒绝。commandId 由核心生成。相同身份及输入返回原回执，不同命令/参数/显式目标/入口引用返回 requestConflict。指纹只规范化 JSON 对象键顺序，不改写路径、命令或业务默认值。超时后保存原票据继续 get；若 get 为 notFound，可用原完整输入和原编号重交，不能生成新编号自动补执行。

页面登记和命令关联在同一工作台锁中提交。重复 ui.openView 返回原 viewId，页面已经关闭时附 `closed:true`，绝不复活。处理者返回的 viewId 必须是该命令已登记的页面。来源页关闭会撤销未登记的创建；已登记的终端独立归终端 view，关闭 Docker 来源不关闭它。关闭保留覆盖零页面命令；逻辑 invalidated/unknown 与真实 RPC 在途任务分开，实际响应或进程确认结束后才释放在途额度。重连不把旧票据带到新代次。

| 预算 | 上限 |
| --- | --- |
| payload、页面 params、结果 | 各 16 KiB |
| 命令信封、RPC 帧 | 24 KiB、既有 64 KiB |
| 同来源、同处理实例在途命令 | 各 32 项（失效但未实际结束的调用仍计入） |
| 命令与自主页面申请的回执 | 每 Context 合计 4096 项、全局 16384 项、合计 16 MiB，含结果与在途预留 |

满额先拒绝新编号，不淘汰现存回执以重做动作。连接代次结束后回收旧记录，未结束调用保留至实际结束。极长期工作区可关闭后重新连接。查询只刷新回执，不启动业务。

SDK `run_with_commands(&["terminal.open.v1"], handler)` 使用既有四个 worker，初始化时确认名称；处理函数返回 `Result<Value,RpcError>`。客户端只接受共享的类型化 Request，底层方法名收发为 SDK 私有实现。`commands::Ticket` 保存 Context、sourceViewId、requestId、name 和 payload，提供 submit/get/resolve；通信超时保留票据。没有新增 iframe 命令桥，UI 事件先交给所属插件后台。


未关联命令的自主 ui.openView 继续限定为调用插件自己的视图，以实例/Context/requestId 去重；原页面关闭返回 viewClosed 错误。带 createActionId 的新视图必须通过显式命令关联创建，不能绕过命令申请。页面也可经宿主 openView 请求自己的普通视图。

### 终端创建参数

终端插件提供以下业务创建配置。

动作 data 和终端视图 params 使用相同契约，null/空对象默认登录 shell。示例：

```json
{"version":1,"title":"api · 容器终端","launch":{"kind":"command","command":"docker exec -it api /bin/sh"},"directoryIntegration":"off"}
```

version 可省略，仅支持 1；title 可省略，非空白、最多 128 字符且不含控制字符。launch 缺省或 `{kind:"loginShell"}` 使用登录 shell；`{kind:"command",command:string}` 要求非空白且不含 NUL。已知字段类型错误、未知版本/kind 拒绝；未知可选字段忽略。总参数最多 16 KiB。

loginShell 缺省目录策略在创建时读取设置，允许 auto/manual/off；command 缺省 off，首版只接受 off。command 不做主机探测或目录脚本注入，不回退登录 shell。自动集成按真实远端 shell 探测；不支持的 shell 保留普通终端并报告原因。

首次挂载保存已解析配置，同 viewId 重挂载复用原 PTY，初始化失败不自动重跑。mount.data.createParams 含可再次使用的配置；command 退出后“重新打开终端”创建新 view/PTY。现有 ui.event 的 `{action:"createTerminal",requestId,params}` 由终端后台保存票据并提交 terminal.open.v1；terminalCommandStatus 查询原票据，Surface 重挂载继续查询。新页面使用当前挂载 Context，不复用原 streamId、输入序号或集成 token。off 不采纳 OSC7/633 的 known 目录，不将嵌套环境路径传给主机文件跟随。

### 网络隧道协作契约

网络隧道提供以下业务契约，使用核心命令、共享状态和后台视图，不新增核心 RPC、权限或直接插件调用。manager 为 sidebar + perWorkspace + backgroundOnWorkspace；后台初始化不启动保存规则。

| 命令 | payload | 结果 |
| --- | --- | --- |
| `network.tcpForward.ensure.v1` | `{version:1,target:{host,port},suggestedName,selectedRule:null或{id,revision}}` | `{version:1,outcome,disposition?,ruleId?,ruleRevision?,runId?,state?,candidateCount?,target?,reasonCode?,message?}` |
| `network.rule.reveal.v1` | `{version:1,ruleId}` | 核心已关联的 `{viewId,closed?}`，定位本插件 manager 与规则分页；不修改编辑草稿或移动跨窗单例 |

ensure 的 target.host 必须是明确 IPv4／IPv6，拒绝通配、组播和无效地址；port 为 1–65535。suggestedName 为字符串，最多 128 字符，不含控制字符；selectedRule 非空时 id 为字符串、revision 为 u64。已知字段类型错误或未知 version 拒绝，未知可选字段忽略；payload／结果沿用 16 KiB 限制。服务器与完整 Context 从核心信封取得，调用方不传凭据或自报来源身份。

匹配同服务器、规范化 IP 与目标端口的回环 TCP 规则，优先当前 Context 的运行项，其次已登记启动，最后已停止项；同优先级多个候选返回 selectionRequired，selectedRule 提交时再次核对最新 revision。仅名称修改不改变转发语义；保存的 type/bind/target 与固定运行配置不一致、其他 Context 持有、查询未知、停止／清理未完时拒绝。不自动复用或启动非回环规则。不同来源的同目标串行决策由网络插件服务负责，不能以核心 requestId 去重替代。

outcome 为：

- `accepted`：规则与启动意图已登记；disposition 为 created／started／reused／joined。返回 ruleId、ruleRevision、runId 及当前 state；不返回顶层 viewId，也不打开侧栏。commands completed 不表示监听已经运行或 TCP 已经可达。
- `selectionRequired`：返回 candidateCount 与 target，无副作用；调用方从共享规则摘要展示候选，用户明确选择后产生新意图。
- `savedNotStarted`：保存已发生而启动未接纳，返回 ruleId、ruleRevision、reasonCode／message；不隐含回滚或自动重试。

无副作用拒绝使用错误 code/message，包括 invalidParams、managerNotReady、changeBusy、ruleConflict、tunnelBusy、tunnelStateUnknown、storageUnavailable、resourceLimit，其他处理失败使用 handlerError。变更门繁忙时明确拒绝，不排无限等待任务；停止与关闭标记不等待磁盘。通信超时查询原 Ticket，结果未知不换编号自动补执行。核心尚未分发的请求可随来源关闭撤销；处理者已接收办理后，不承诺关闭来源能够回滚副作用。

网络 manager 使用共享状态发布两个 topic，完整 producer 由核心绑定；消费者同时核对 Context、pluginId、pluginInstanceId、viewId，不能只用 ruleId 认领其他来源。读取／订阅不启动或检查隧道：

| topic／key | value |
| --- | --- |
| `network.tunnelService.v1`／current | `{version:1,ready:true,publishState:"synced"或"limited",storageError:null或string,observedAt}` |
| `network.tunnelRules.v1`／ruleId | `{version:1,rule,run:null或{runId,ruleRevision,rule,ownedElsewhere,canStart,released,stopRequested,queryError,observedAt,snapshot:{state,cleanupComplete,address,error}},check:null或{checking,listenerId,runId,observedAt?,ok?,message?}}` |

rule 是网络插件既有 schema 1 的普通规则配置。run.rule 固定启动配置，与当前保存配置分别表达；runId 是 opaque 关联标识，不是核心资源操作凭证。其他 Context 持有时 ownedElsewhere=true、address=null。snapshot.error 与 queryError 只提供有界说明，不含客户端流、凭据或完整网络路径。日期时间为 Unix 毫秒，生产者每约 5 秒更新服务 observedAt；消费者超过 15 秒没有有效服务观测，或具体 Run 观测过期时停止把入口当作当前可用事实。

单条记录与整个范围沿用 sharedState 预算，不增加跨记录事务。发布失败为 limited；尚未取得同步副本不能把缺少记录解释为可以安全重复启动。UI 候选只是提示，真正命令由 Service 重读事实决定。删除移除记录，进程／逻辑 view／Context 结束由核心回收其事实；Surface 变化不结束监听。新启动自动检查一次实际入口到目标的 TCP，joined／reused 不重复检查，TCP 可达不表示应用健康。用户行为和失败恢复见[网络隧道](features/network-proxy.md#其他插件的快捷入口)及[Docker](features/docker.md#容器端口隧道)。

## 服务器资料与连接

servers.list 提供非敏感资料、目录修订及 profileToken；create/update/delete 使用 catalogRevision 进行比较写入，认证保持 keep/set/clear 的明确意图。目录写入没有可恢复远程 taskId；回包丢失后读取确认，不以新修订自动重建。参数类型见[服务器契约](../crates/plugin-protocol/src/servers.rs)。

connections.open/get/cancel 使用固定 requestId。open 带 serverId/expectedProfileToken；原 receipt 为 attached/accepted/failed/cancelled，终态固定；current 单独反映原 Context 当前事实。取消先封住未接纳意图，不断开已接纳共享会话。后台 view ACK 不等于插件业务可执行，MCP 再核对目标。完整业务出口见 [MCP API](mcp-api.md)。

## 文件资源、目录与复制

files.open({viewId}) 返回 id；files.close({id}) 清理所属资源。files.batch.begin/cancel/end 与 files.input.claim/list/release 分别管理批次及原生输入，所有接纳绑定实例、View、Context；取消与实际清理是不同事实。

目录数据使用 home/stat/list/closeDirectory，list 每个 cursor 独立分页并显式关闭；不再有共享单目录或 independent 开关。文件复制支持 remote、原生 inputId/rootId/relative、localPath 及 pluginData 各自的端点约束。localPath 是只读本地来源，不伪造原生文件选择；pluginData 只在本组件数据目录内独占创建。路径按本地/远端分别处理。

copyFile 使用 destinationGuard 的 absent/match/capture，capture 只允许远端目标并在准备时固定；提交前复核，不能重捕获掩盖变化。metadataPolicy=contentOnly 不传播来源属性。transfer 保留 commitState/commitAttempted/sourceVersion/temporaryPath，提交 ACK 后附属 stat/cleanup 失败保持 committed，未知提交不自动重试或删除未决临时文件。



### 文件操作

插件调用 `files.operations({action,id,...})`，action 为 start/lookup/poll/cancel/release。start 必须传固定 requestId、递增十进制 sequence、完整 expectedContext、已登记的 batchId 和 operation；其他端点/配额沿用文件原语。lookup 传 requestId 和 batchId，其余控制传 operationId。重复 start 先返回已登记回执；相同身份不同参数拒绝，release 后旧序号不能重新执行。

renameNoReplace 当前适配器返回 unsupported，不发送复制或删除作为替代。

任务失败和方法拒绝保留 `{code,message,effect,retryable}`，方法拒绝位于 JSON-RPC error.data。effect 为 none/unknown，成功通过 completed/result 表达。明确成功提交之后的附属 stat/cleanup 失败记录为 warning/cleanupWarning，不改成可重试失败。通信超时没有这些提交事实，只能继续查询原请求。

`files.batch.cancel({id,batchId})` 先封闭批次，再取消已登记操作。lookup 在已封闭且从未接纳的请求上可返回 cancelled/effect:none；取消 ACK 本身不证明已接纳操作未修改目标。batch.end 与新接纳按同一短锁顺序互斥，结束后的批次不能接纳迟到 start。终态落盘后才 release，不淘汰在途或待确认回执来腾出容量。

### 草稿、保存和导出

正文继续走 iframe `data({op:"files",id,action,...})`。成功直接返回结果，失败拒绝 Promise 并保留结构化错误；修订冲突使用 error.data.code=revisionConflict。插件控制面 `files.editor({id,action,...})` 提供 draft.describe、save.start/status/cancel/release 及 export.status/cancel/release；导出 start 必须来自 UI 对话框入口。

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

同一文件只维护一份草稿。首次正文由非候选 Surface 的 document.load({path}) 读入；它允许在 prepared 前执行，避免正文与准备互相等待。候选和重挂载仅用 draft.describe/draft.chunk；每次读取固定 DraftRef，发生 revisionConflict 后重新读取描述，不拼接不同修订。

## 通用网络流

net.poll({viewId}) 每次最多领取 16 个本地客户端；net.tcp.open 固定 listenerId/clientStreamId/operationId/host/port，同客户端只接纳一个目标。net.stream.status/read/write/finish/splice 处理协商和转交，net.stream.close({id}) 只关闭流；监听使用下述票据生命周期。默认绑定回环，可传其他本机地址；远端 DNS 由最终会话解析。

协商每次最多 512 字节，累计最多 1024 字节，控制队列 8 项。每监听最多 64 活跃/16 待协商连接，每实例 32 监听、全核心 256 本地客户端；协商约 15 秒、远端连接 10 秒。probe 最多 4 并发、10 秒。正常正文原生转发；资源停止需等待真实端口、流和 worker 退出。

## 日志与重要消息

logging.write 使用 debug/info/warn/error，最多 8 KiB 正文；全局每秒 200 条，过量 accepted=false。日志普通 error 不自动通知。日志文件 10 MiB × 5，内存 5000 条，界面查询 500 条。

messages.publish 使用 info/success/warning/error，title 最多 1024 字节、body 16 KiB，notify 默认 false。持久化成功才返回 id；保留 1000 条，同插件 dedupeKey 五分钟合并。核心附加插件/实例/接收时间，记录启动、停止与异常退出。

## 页面公共服务与远程位置

页面请求支持 read、event、openView、closeView、data、settings、state、clipboard 和 localFiles。数据、剪贴板及本地输入始终验证真实 Surface；通用文件和终端正文走原有数据通道；插件自有记录可经有界 read 分页读取。剪贴板分块上限 256 KiB UTF-8、单 Surface 一个传输、15 秒期限。公共 SDK 负责分块、清理和文本菜单接入，不检测历史核心能力。

主题 tokens 名称由[token-names.ts](../packages/ui/token-names.ts)定义，页面应用为同名 CSS 变量；shared.css 随插件打包。主题变化不重建会话。文件编辑、终端和 Docker Worker 业务留在各自组件。

files 数据的 resolveLocation 接受 path/pathStyle/filesystemView。session-default 仍需真实 SFTP realpath/stat；shell-reported 还需 witness:{path,value}，检查普通文件、内容完全相同且最多 128 字节。此证据说明终端与文件视角对应，不是认证或授权。失败返回 unsupportedLocation；不按客户端 OS 猜测远端路径。


### 监听生命周期

插件调用 `net.listener.create/lookup/cancel/release`，均传 viewId、expectedContext、固定 operationId、递增十进制 sequence；create 还传 bind。按插件实例/view/Context 隔离，保留退役序号，活跃回执不按时间过期。回执最多 4096 项，scope 也有 4096 项上限；原归属退役且实际任务完成后回收。

create/lookup/cancel 返回 observed（listenerId/snapshot/cleanupComplete）、notObserved（cleanupComplete:false）或 retired（cleanupComplete:true）。相同身份不同绑定参数拒绝；cancel 先封住序号，先于 create 到达也不能再创建。retired 只表示意图封闭且没有活跃资源，或核心确认原归属退役且资源全部退出。RPC 错误、Context 失效本身不能作为清理证明；方法错误 data.code 为 networkRequestRejected。

snapshot 增加 cleanupComplete；只有监听、客户端流、远端通道、转发和探测任务全部退出才为 true，closed/failed 字符串不能替代它。release 只接受已确认清理的回执，低序号重放不能创建；流关闭使用 net.stream.close，监听始终通过原票据 cancel/release。规则保存/删除不进入核心协议。

## 组件设置

插件在清单增加可选 `settings: {schemaVersion:1, pages:[...]}`，一个插件可声明多个组件页，一个页可关联多个 `viewTypes`。同一视图最多关联一个页；稳定身份为 pluginId/pageId/key，不含服务器、Tab 或程序版本。字段、默认值和生效方式由插件声明，核心提供基础校验、存储、版本选择、通用表单和变化通知。正式示例见各插件的 `release.json`，功能与维护约束见[设置](features/settings.md)。

页面包含 id/title/icon/order/viewTypes/groups；分组包含 id/title/fields。字段为 key/type/title/default/apply，可带 description。首版类型为 boolean、number（min/max/step/unit）、string（maxLength）、select（options 的 value/title）、font（presets 的 id/title/regular/bold）；apply 为 live 或 newInstance。`visibleWhen:{key,equals}` 只引用同页无条件的 boolean/select 字段。隐藏字段仍需完整保存并校验。

font 值为 `{source:"preset",id:"jetbrains-mono"}` 或 `{source:"local",family:"Consolas"}`。预置字体路径相对插件安装目录，要求包内 WOFF2；核心只按当前页声明读取对应资源。本机字体按名称加载，失败显示说明并回退；不传字体文件、CSS 表达式或安装绝对路径。

initialize 的 settings 数组列出 pageId/definitionToken/revision；核心向声明设置的当前插件发送 settings.changed。初始化期间允许 settings.get，不等待插件生命周期锁。

| 方向与方法 | 参数 | 结果与语义 |
| --- | --- | --- |
| 插件 → 核心 `settings.get` | `{pageId}` | `{pluginId,pageId,definitionToken,revision,values}`；限所属当前实例，返回有效值 |
| 核心 → 插件 `settings.changed` | `{pageId,definitionToken,revision,values}` | 使用带 ID 的普通 RPC 请求并等待回复；插件通过标准 RPC error 报告应用失败 |
| UI mount/settingsChanged | settings 单页快照 | 宿主转发当前 surface 对应的页，隐藏组件可在显示时应用 |
| UI → 宿主 `sodeck.request("settings",...)` | `{op:"get"}`、`{op:"applied",pageId,definitionToken,revision,error?}` 或 `{op:"save",requestId,definitionToken,baseRevision,values}` | 当前 surface 读取、回报或保存其唯一关联页；保存不接受页面自报的插件身份，返回 persisted/conflict/snapshot |

revision 为十进制字符串，definitionToken 为插件当前程序版本、schemaVersion 和规范化页面声明的稳定摘要。核心保存命令接受 definitionToken/baseRevision/requestId/完整 values；旧页面返回冲突及最新快照，相同请求 ID 与正文的近期重投不会重复保存。保存成功与运行应用分别报告；应用失败可重投当前修订，插件应允许同修订重试并忽略过期更新。

每插件最多 8 页，每页 8 组/64 字段，每字段最多 32 个选项或字体。单页保存值和运行快照各限 24 KiB，RPC 帧继续限 64 KiB；设置文件限 4 MiB。用户保存严格校验，载入旧非法值则保留原值、提供有效默认和提示。存储独立于业务规则、凭据和工作区；临时文件原子替换、备份与失败保留沿用核心存储机制。

升级待重启时继续使用旧进程的声明及资源，实际激活新版后才更新；新增字段补默认，兼容的用户值保留，结构变更历史最多保留两份。新版启动失败恢复旧程序时设置随之恢复适用定义，修订不倒退。首版不提供任意迁移脚本；插件作者改变 key/type/单位含义时需单独设计兼容迁移。

## 共享运行状态

核心保存内存中的当前事实，进程与页面经接口访问；不持久化，不提供插件之间直接调用或历史事件日志。

每条记录由完整 Context、topic、producer、key 标识。Context 包含 workspaceId/serverId/connectionId/generation；producer 为 `{kind:"core"}` 或 `{kind:"plugin",pluginId,pluginInstanceId,viewId}`。核心推导调用者身份，put/remove 只能修改自己的来源。修订号为规范 u64 十进制字符串；JSON null 是存在的值，删除用 `present:false` 墓碑，读取未存在的版本为 `"0"`。

进程参数必须带 viewId/expectedContext，页面使用 `sodeck.request("state",{op,...})`，宿主绑定真实 surface。页面成功直接返回结果，失败拒绝 Promise；进程和页面的恢复代码均在 error.data.code。

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

页面使用随插件打包的 [state.js](../packages/plugin-sdk/src/state.js)：`new SharedState(mount)`，`request(op,params)`，`watch(filter,onProjection)`，最终 `dispose()`。watch 会分页、原子拼批、确认、续订及重同步；onProjection 收到 null 表示当前不可用。Rust 使用 [plugin-client::shared_state](../crates/plugin-client/src/shared_state.rs) 的 request/Watch::open/snapshot/renew，持有 Watch 时每 30 秒调用 renew；snapshot 为 None 或 renew 失败时由调用者在有效 Context 内重建。两者使用同一组线协议样例测试。

首批业务契约：Core 发布 `workspace.presentation.v1/current` 的 activeEditorViewId；终端发布 `terminal.location.v1/current` 的 state/path/pathStyle/filesystemView/terminalId/outputOffset/observedAt；系统状态发布 `filesystem.mounts.v1` 的 mount 与 scan-status，以及同源同 mountId 的 `filesystem.capacity.v1`。容量字节数使用十进制字符串，业务新鲜度由 observedAt/staleAfterMs 表达。扫描 coverage 与 publishState 分开：complete/partial/unsupported 描述采集覆盖，publishing/synced/limited 描述共享副本。没有跨记录事务。

### 只读页面查询与视图入口

核心 2.3.0 的挂载消息提供 capabilities.viewRead=true。页面使用 sodeck.request('read', query)，query 必须是至多 24 KiB 的对象；核心以 ui.getView 的 data={read:query} 转发，返回 data 至多 24 KiB。默认挂载/后台 ui.getView 的 data=null 行为不变。插件须先分流只读查询，只返回已有本地状态与有界页面，不执行远端任务或业务变更。

read 不修改宿主 metadata、不触发宿主整体刷新；允许已挂载的只读候选 Surface，仍核对 window/Surface/View/实例及 Context，登记在途调用，View 封闭后拒绝新查询。回复返回时再次核对原挂载和 View 身份，过期结果不得作用到新挂载。页面每 Surface 最多两个并发查询，核心 View 在途上限 16。真正的业务意图继续使用 event 和原输入权检查；不引入插件授权流程。

ui.views[].entry 可选，使用与 ui.entry 相同的包内相对资源验证；声明时该视图使用自己的入口，省略则沿用 ui.entry。持久安装清单保留此字段，注册投影只输出一个解析后的 entry。旧插件不受影响，新插件通过可选挂载能力检测提示升级；不增加 initialize 字段或历史数据通路。

# 外置插件 API v1

通用 TCP 与弹窗扩展已实现，见本文末尾接口和 [网络隧道设计](plan-network-proxy.md)。插件须先检查 `net.tcp.v1` 和 `ui.dialog.v1`，旧核心仍可加载不使用新能力的旧插件。

插件是独立原生程序，核心不需要识别具体插件 ID。插件按可信扩展使用，没有权限清单、授权或签名流程。

当前源码为 API v1 增加可选 UI v1，旧无 UI 插件无需修改；已发行 v0.4.0 不支持该扩展。界面方案与验收见 [组件 UI 设计](plan-component-ui.md)，实际协议如下。

后续终端/文件能力的调用关系、流式数据桥、异步保存/长任务和带宽归属设计见 [远程工作台提案](plan-remote-workbench.md)。其中方法名为提案，不是当前可调用的 RPC；实现时以保持旧插件兼容的扩展同步本协议。

## 可选 UI v1（当前源码）

关闭的准备、决定、清理和原生收尾由核心统一管理。宿主快照查询不会创建 view；后台 view 只由工作区/插件状态变化触发，关闭保留期间暂停创建，取消恢复原身份。关闭页面集合、工作区、窗口和应用共用同一操作。插件仍使用已有 `ui.*` 方法，无需增加权限声明或生命周期专用必填字段。

`ui.save` 已接受后，重复关闭决定只查询原保存结果，不自动再发保存。`ui.disposeView` 的超时表示真实结果尚未确认；核心继续跟踪同一请求，迟到成功可以完成该步骤，明确失败后才由用户重试。全局关闭通过进程监督器停止全部受管插件，再确认核心原生资源与连接结束。`files.close`、`net.close` 等取消接口仍快速接受；接受取消不代表实际端口、句柄或 worker 已释放。

清单可添加 `ui: { apiVersion: "1", entry: "ui/index.html", views: [...] }`，完整示例见 `plugins/files/release.json`。视图 ID 在插件内稳定，核心用 `pluginId:localId` 标识定义、另分配 viewId 标识实例。每个视图必须声明 `id/title/icon/container/instancePolicy`：

- `container`：`sidebar` 或 `editor`。侧栏第一版只支持 `perWorkspace` 单例；中央还支持 `multiple` 和 `perResource`。资源标识按远程文件系统语义由插件提供，不按本机 Windows 路径转换。
- `defaultSide`：left/right，默认 left；`defaultPinned`：初始固定入口，默认 false；`openOnWorkspace`：none/reveal/createOne，默认 none；`startupOrder`：初始动作顺序。首次出现的定义保存推荐值，已有偏好保持；新工作区按这些偏好选择未占用的栏位。
- `backgroundOnWorkspace: true`：可选，仅适用于 sidebar + perWorkspace。需要 `ui.background.v1`。核心在工作区存在时确保逻辑 view 并幂等调用 ui.mount，无需 iframe；`uiRequested=false` 的 view 不创建页面，也不阻塞移窗。显示后沿用普通页面生命周期。隐藏只影响页面，采集和 disposeView 仍由插件进程处理。
- `createAction`：中央“＋”中的创建动作名称；目标就是本视图。`locateEntry: true`：可固定的中央定位入口，优先定位最近使用的实例，没有时创建一个。文件编辑器需要 resourceKey，不声明空页面创建动作。

插件包附带已构建的 HTML/JS/CSS，核心通过本地 `sodeck-plugin` 协议加载，不依赖开发服务器或用户 Node 环境。打包脚本自动复制插件的 `ui/`，核心安装包不包含它们。

核心 → 插件进程的 JSON-RPC 方法：`ui.mount`、`ui.event`、`ui.beforeClose`、`ui.save`、`ui.disposeView`。共同参数为 `{viewId, viewType, context, resourceKey, params, data}`，其中 Context 由核心固定绑定。`ui.mount` 首次创建逻辑状态，之后同 viewId 重挂载读取原状态，必须幂等；返回 `{ data, metadata: { title?, dirty?, busy? } }`。`ui.event` 的 data 是 UI 事件；beforeClose 同步报告最终状态；save 保存或返回错误；disposeView 在完成关闭决定后释放实例，必须幂等。该最小协议没有额外的 ui.createView 或运行时注册系统。

插件进程可主动调用 `ui.updateView`，参数 `{viewId, metadata:{title?,dirty?,busy?}}`，供后台任务报告状态。核心附加调用插件和本次进程身份，只接受其自有视图；汇总关闭期间冻结更新。

页面与宿主使用 `postMessage`，信封固定 `channel: "sodeck-ui-v1"`。页面先发 `{type:"ready"}`，宿主回 `mount`（含 viewId、viewType、surfaceId、context、data、tokens、visible），页面初始化后发 `{type:"mounted",surfaceId}`。`request` 带 surfaceId、id、method、params；宿主以同 id 的 `response` 返回 data 或 error。支持：

- `event`：交给所属插件进程的 ui.event，结果更新页面元数据。
- `openView`：`{viewType,resourceKey?,params?}`，只打开本插件在同一工作区中声明的视图。核心分配动作 requestId，IPC 重试复用，真实多次新建允许多个实例。

宿主还通知 `visibilityChanged/themeChanged/focus/unmount`。`mount.connected` 可选布尔值表示工作区当前连接状态；连接状态变化时通知 `connectionChanged`，携带 `connected`。旧 UI 可忽略这两个增量字段；它们不恢复原终端或自动启动业务任务。核心校验真实页面 sender、挂载 token、插件进程实例与工作区 Context。移窗时先等目标页面 mounted，再提交窗口归属；旧表面事件因此失效。同窗切换只隐藏已挂载页面，不能重写其 DOM。示例 bridge 位于 `plugins/files/ui/bridge.js`。

UI 事件最多 24 KiB，是控制交互接口，不能用来传终端输出或文件内容流。RPC 保留 64 KiB 帧上限和 5 秒等待期限；v0.6.0 的真实业务正文另走下述有界视图数据桥。可信插件不增加权限表或授权流程。

## 会话状态与有界短命令（当前源码）

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

清单可选 `updateSource: { "repository": "BgaSol/soDeck", "tagPrefix": "plugin-terminal-v" }`。未声明来源的本地插件可在设置中显式配置。插件仍为 API v1，不增加运行时更新 RPC。发布维护与重启切换见分发文档。

## 通信

stdin/stdout 每行一个 UTF-8 JSON-RPC 2.0 对象，以 LF 结尾，最大 64 KiB（含 JSON 转义和末尾 LF，双向均检查）。stdout 只输出协议，stderr 作为诊断日志。请求带字符串或数字 ID，响应使用相同 ID；通知省略 ID。参数错误返回 `-32602`，未知方法返回 `-32601`，持久化等执行失败返回 `-32000`，核心请求队列已满返回 `-32001`，核心处理超时返回 `-32003`。调用方应检查响应，不根据日志推测调用成功。

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

## 真实远程能力（v0.6.0）

- terminal.open({viewId,cols,rows}) 返回 id；terminal.close({id}) 释放 PTY，结束状态走正文通道。资源固定创建时会话，不自动重放命令。
- files.request({action:"open",viewId}) 返回 id；close/status/save 传 id。save 返回后台任务，status 为 running/completed/failed；关闭协调可等待完成，不阻塞接收线程。
- metrics.networkSnapshot() 返回服务器 socket 量、插件有效载荷、约 3 秒速率与质量。累计为十进制 u64 字符串，不以目录 JSON 代替网络字节。
- iframe 经 sodeck.request("data",request) 调用正文桥。终端 op 为 attach/ack/write/resize/checkpoint/detach；文件 op="files"，action 为 home/list/stat/read/describe/chunk/stage/save/status/download/cancel/exportDraft。每块最多 16 KiB；按真实 view/实例/Context 校验，不通过普通 ui.event 传正文。download/exportDraft 由核心打开本地保存对话框；导出草稿不要求远端在线。
- 页面以 mounted 消息 flush=true 声明移窗/关闭前需要同步。收到 flush，完成在途同步后回复同 id 的 flushed；携带 error 可阻止丢失输入。旧插件不声明则保留原行为。

所有权、缓冲、保存与下载约束见 [实施记录](implementation-remote-plugins.md)。不引入跨插件直调或插件授权流程。

### 目录树扩展（核心 0.6.1）

新增能力 `files.directoryCursors.v1`。旧版 `list({path})` / `list({next:true})` 保持单目录游标行为；新版文件插件先检查该能力，再使用以下可选参数：

- `list({path,independent:true})` 返回 `{path,entries,more,cursor}`，每页最多 100 项。多个目录可以同时分页，`cursor` 仅在还有后续页时返回。
- `list({independent:true,next:true,cursor})` 继续指定目录，`path` 不参与选择。刷新传 `path,independent:true,cursor` 并省略 `next`，先释放旧游标再读取首屏。
- `closeDirectory({cursor})` 主动释放未完成的独立游标。每个文件资源最多保留 32 个独立游标，超出时释放最早打开的游标；已失效的游标需要重新刷新。读完、关闭页面、连接更换或失效时也会释放。

界面挂载及 `themeChanged` 的 `tokens` 增加 `color-scheme`、`good`、`error`、`shadow`、`file-tree-bg`、`status-bg`、`folder-color`、`folder-fill`、`tree-selection-border`。旧插件可忽略新增字段，新插件提供基础色回退。文件导航状态由插件进程按视图保存，只保存有界路径、选择和滚动元数据，目录内容不进入控制 RPC。

### 工作台外观与文本剪贴板（增量能力，尚未发布）

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

### 统一桌面行为和滚动条（增量能力，尚未发布）

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

协商读写合计最多 1024 字节，控制队列 8 项；正常正文只经原生流。每监听最多 64 活动连接、16 待协商连接，每插件实例最多 32 监听，核心最多 256 本地客户端；协商期限约 15 秒，远端连接 10 秒。probe 最多 4 个并发、10 秒期限，检查完成应关闭匹配的客户端流。监听 open 的 operationId 在同实例内保留 60 秒，最多 4096 项；窗口重挂载应复用 listenerId，不重复创建。tcp open 以本地客户端及 operationId 去重，参数变化拒绝。

`snapshot.error` 为 `{stage,message}`（listen/sessionLost）；单流失败不使整条监听失败。累计量为 u64 十进制字符串，计入远端有效载荷，并归属原有服务器/插件统计；不含本地 SOCKS 协商，不将 SSH 加密开销重复归属插件。关闭视图、服务器断开、插件进程退出或应用退出会清理资源，收起及移窗保持原资源。

清单的视图可声明 `container:"dialog",instancePolicy:"perWorkspace"`，禁止 startup/pin/create/locate 组合。宿主提供标题、关闭、三主题和可滚动容器；按原有 ui.mount/event/beforeClose/save/disposeView 协议工作。iframe 的 `sodeck.request("closeView",{})` 请求关闭当前逻辑视图，仍经过 flush 和保存/放弃/取消；关闭编辑弹窗不关闭同插件的管理侧栏资源。

## 本地构建业务插件

### 文件批量操作与本地输入（未发布增量）

初始化能力增加 `files.operations.v1` / `files.localInputs.v1`。具体约束见 [实施记录](implementation-file-management.md)。核心只执行单项 IO；递归、冲突选择、失败重试由文件插件进程执行。

- `files.request({action:"batch.begin",id,batchId,context})` 登记已接受批次；`batch.end` 释放。关闭冻结新输入时允许原 Context 的已接受批次继续或取消。
- `op.start` 参数为 `id,requestId,sequence,expectedContext,batchId?,operation`，立即返回 `{id,state}`。sequence 是资源内递增十进制字符串，同 requestId/sequence/Context/参数重复返回原操作，低序号拒绝重做。`op.lookup({id,requestId})` 查询超时请求；`op.poll/result/cancel/release({id,operationId})` 查询、取消和释放，release 只接受终态。
- operation.kind 为 `inspect/list/next/closeCursor/mkdir/createEmpty/copyFile/rename/removeFile/removeDirectory`。端点为 `{side:"remote",path}` 或 `{side:"local",inputId,rootId,relative:[独立分量]}`。单文件保存目标可用 rootName 在原选定父目录改名；上传输入不允许。
- list 返回 `{entries,more,cursor?}`，next/closeCursor 传 cursor。inspect 返回 `{attrs}`，不存在为 null；attrs 含 kind、十进制 size、mtime、stamp，远程目录含 resolved。copyFile 传 source/destination/sourceStamp/destinationGuard，成功返回目标 stamp；guard 为 `{kind:"absent"}` 或 `{kind:"match",stamp}`；其他变更用 endpoint、可选 sourceStamp、guard，rename 另传 destination 且要求目标不存在。
- 状态为 running/committing/completed/failed/cancelled/unknown。completed 的 result 为元数据，失败的 error 为 `{code,message,retryable}`。unknown 表示远程未确认变更结果，不可自动重试。received/total 为十进制字符串。每资源 16 项、核心全局 128 项，结果页最多 100 条且不超过 16 KiB。
- iframe 的文件 data 桥增加 `input.revision/clipboard/drop/chooseUpload/chooseDownload/release/reveal`。chooseUpload 传 directory，chooseDownload 传 single/name；立即返回 waitingInput 操作票据，以 op.poll 等待 inputId 或 cancelled，再 op.release。clipboard 仅解析原生文件格式，drop 传宿主事件 dropId。input.release 传 inputId，reveal 只打开已登记下载目录。
- `input.revision` 兼容增加 `hasFiles:boolean`，返回 `{revision,count,hasFiles}`。该查询只检查本地剪贴板的受支持文件格式，不枚举路径或登记 input；因此此时 count 仍可能为 0，不能据此判断有无文件。缺少 hasFiles 的旧核心按状态未知处理，显式 `input.clipboard` 仍沿用既有行为。远程复制/剪切来源及其可用性由文件插件维护。
- 插件进程用 `input.claim/list/release` 领取/分页读取/释放输入。list 返回 rootId、name、next，最多 4096 顶层项。claim 的 renew:true 仅允许同工作区已领取输入在用户明确重试失败项时重新绑定当前 Context。本地 PathBuf 留在核心，普通文本路径不是上传源。
- mount.capabilities.localFiles 为 1。页面以 `mounted` 消息 `localFiles:true` 声明接收原生拖入；宿主向可见页面发送 localFileDrop（dropType/dropId/x/y，坐标为 iframe CSS 像素），页面领取 drop 后决定业务。此声明不构成插件权限。
- UI 可向宿主发送 `{channel:"sodeck-ui-v1",type:"interaction",surfaceId}` 通知实际指针/键盘交互，供宿主关闭旧菜单和取消过期的自动聚焦。宿主按已有 iframe 来源、origin 与 surface token 绑定核对；不携带按键、文件或终端输入内容。旧宿主忽略此增量消息，旧 UI 仍按原协议加载。
- iframe `request("closeViews",{viewIds})` 立即返回 ticket，宿主异步发送 viewsClosed（ticket/accepted/error）。只允许同插件同工作区页面，将整个集合交给一次核心关闭操作，统一同步输入并决定保存/放弃/取消，全部决定后才释放页面。

旧 files.v1、旧目录游标和文本剪贴板协议保持兼容。文件插件 0.3.0 需要核心 0.8.4；五目标原生包构建和既有远程业务组合烟测通过，原生系统粘贴/拖入的设备互操作待验收。

## 组件设置（可选 settings.v1）

组件设置从核心 0.9.0 提供，页面保存所属设置页从核心 0.13.0 提供。插件在清单增加可选 `settings: {apiVersion:"1", schemaVersion:1, pages:[...]}`，一个插件可声明多个组件页，一个页可关联多个 `viewTypes`。同一视图最多关联一个页；稳定身份为 pluginId/pageId/key，不含服务器、Tab 或程序版本。字段、默认值和生效方式由插件声明，核心提供基础校验、存储、版本选择、通用表单和变化通知。正式示例见各插件的 `release.json`，实现范围见 [组件设置记录](implementation-component-settings.md)。

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

## 独立插件打包

```sh
npm run plugin:build -- --plugin terminal
# 指定目标时仍需要相应编译环境
npm run plugin:build -- --plugin terminal --target aarch64-apple-darwin
# 正在运行的插件不能原位覆盖；关闭并重启后构建，或使用另外的输出目录
npm run plugin:build -- --plugin terminal --out-dir artifacts/another-build
```

产物位于 `artifacts/plugins/`，包含清单和原生入口的目录与压缩归档。构建脚本在所选插件工程目录调用 Cargo，使其 Windows 静态运行库配置生效；归档前检查原生架构和额外的 Visual C++ 运行库依赖。每个插件自己的版本只在 `plugins/<名称>/Cargo.toml` 中修改，独立于核心版本；用户运行时不需要 Rust、Node.js 或额外安装 Visual C++ 运行库。

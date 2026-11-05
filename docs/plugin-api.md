# 外置插件 API v1

通用 TCP 与弹窗扩展已实现，见本文末尾接口和 [网络隧道设计](plan-network-proxy.md)。插件须先检查 `net.tcp.v1` 和 `ui.dialog.v1`，旧核心仍可加载不使用新能力的旧插件。

插件是独立原生程序，核心不需要识别具体插件 ID。插件按可信扩展使用，没有权限清单、授权或签名流程。

当前源码为 API v1 增加可选 UI v1，旧无 UI 插件无需修改；已发行 v0.4.0 不支持该扩展。界面方案与验收见 [组件 UI 设计](plan-component-ui.md)，实际协议如下。

后续终端/文件能力的调用关系、流式数据桥、异步保存/长任务和带宽归属设计见 [远程工作台提案](plan-remote-workbench.md)。其中方法名为提案，不是当前可调用的 RPC；实现时以保持旧插件兼容的扩展同步本协议。

## 可选 UI v1（当前源码）

清单可添加 `ui: { apiVersion: "1", entry: "ui/index.html", views: [...] }`，完整示例见 `plugins/ui-example/release.json`。视图 ID 在插件内稳定，核心用 `pluginId:localId` 标识定义、另分配 viewId 标识实例。每个视图必须声明 `id/title/icon/container/instancePolicy`：

- `container`：`sidebar` 或 `editor`。侧栏第一版只支持 `perWorkspace` 单例；中央还支持 `multiple` 和 `perResource`。资源标识按远程文件系统语义由插件提供，不按本机 Windows 路径转换。
- `defaultSide`：left/right，默认 left；`defaultPinned`：初始固定入口，默认 false；`openOnWorkspace`：none/reveal/createOne，默认 none；`startupOrder`：初始动作顺序。首次布局采用当前插件推荐值，以后新增插件进更多入口，不覆盖已有偏好。
- `createAction`：中央“＋”中的创建动作名称；目标就是本视图。`locateEntry: true`：可固定的中央定位入口，优先定位最近使用的实例，没有时创建一个。文件编辑器需要 resourceKey，不声明空页面创建动作。

插件包附带已构建的 HTML/JS/CSS，核心通过本地 `sodeck-plugin` 协议加载，不依赖开发服务器或用户 Node 环境。打包脚本自动复制插件的 `ui/`，核心安装包不包含它们。

核心 → 插件进程的 JSON-RPC 方法：`ui.mount`、`ui.event`、`ui.beforeClose`、`ui.save`、`ui.disposeView`。共同参数为 `{viewId, viewType, context, resourceKey, params, data}`，其中 Context 由核心固定绑定。`ui.mount` 首次创建逻辑状态，之后同 viewId 重挂载读取原状态，必须幂等；返回 `{ data, metadata: { title?, dirty?, busy? } }`。`ui.event` 的 data 是 UI 事件；beforeClose 同步报告最终状态；save 保存或返回错误；disposeView 在完成关闭决定后释放实例，必须幂等。该最小协议没有额外的 ui.createView 或运行时注册系统。

插件进程可主动调用 `ui.updateView`，参数 `{viewId, metadata:{title?,dirty?,busy?}}`，供后台任务报告状态。核心附加调用插件和本次进程身份，只接受其自有视图；汇总关闭期间冻结更新。

页面与宿主使用 `postMessage`，信封固定 `channel: "sodeck-ui-v1"`。页面先发 `{type:"ready"}`，宿主回 `mount`（含 viewId、viewType、surfaceId、context、data、tokens、visible），页面初始化后发 `{type:"mounted",surfaceId}`。`request` 带 surfaceId、id、method、params；宿主以同 id 的 `response` 返回 data 或 error。支持：

- `event`：交给所属插件进程的 ui.event，结果更新页面元数据。
- `openView`：`{viewType,resourceKey?,params?}`，只打开本插件在同一工作区中声明的视图。核心分配动作 requestId，IPC 重试复用，真实多次新建允许多个实例。

宿主还通知 `visibilityChanged/themeChanged/focus/unmount`。核心校验真实页面 sender、挂载 token、插件进程实例与工作区 Context。移窗时先等目标页面 mounted，再提交窗口归属；旧表面事件因此失效。同窗切换只隐藏已挂载页面，不能重写其 DOM。示例 bridge 位于 `plugins/ui-example/ui/bridge.js`。

UI 事件最多 24 KiB，是控制交互接口，不能用来传终端输出或文件内容流。RPC 保留 64 KiB 帧上限和 5 秒等待期限；v0.6.0 的真实业务正文另走下述有界视图数据桥。可信插件不增加权限表或授权流程。

## 目录与清单

解压后选择含 `plugin.json` 的目录：

```json
{
  "id": "sodeck.hello",
  "name": "Hello 插件",
  "version": "0.1.0",
  "apiVersion": "1",
  "target": "x86_64-pc-windows-msvc",
  "entry": "bin/sodeck-hello.exe"
}
```

每个包包含一个目标的程序，五个 target 见分发文档。Unix 入口需要执行位。核心只做必需字段、版本、API 主版本、目标及目录内入口的基本检查。同一 ID 只能有一个活动实例。启用/关闭在重启后生效，核心启动时加载已启用插件。

清单可选 `updateSource: { "repository": "BgaSol/soDeck", "tagPrefix": "plugin-hello-v" }`。未声明来源的本地插件可在设置中显式配置。插件仍为 API v1，不增加运行时更新 RPC。发布维护与重启切换见分发文档。

## 通信

stdin/stdout 每行一个 UTF-8 JSON-RPC 2.0 对象，以 LF 结尾，最大 64 KiB（含 JSON 转义和末尾 LF，双向均检查）。stdout 只输出协议，stderr 作为诊断日志。请求带字符串或数字 ID，响应使用相同 ID；通知省略 ID。参数错误返回 `-32602`，未知方法返回 `-32601`，持久化等执行失败返回 `-32000`，核心请求队列已满返回 `-32001`，核心处理超时返回 `-32003`。调用方应检查响应，不根据日志推测调用成功。

当前源码的控制通信已改为有界双向分派：每插件最多 4 个处理线程、32 个未完成的插件 → 核心调用；核心 → 插件调用队列及在途表各最多 64 项。接收循环不等待业务处理，响应按请求 ID 匹配，完成顺序可以不同。停止请求有独立队列，写入端优先发送响应、初始化与 shutdown，输出或请求压力不会无限堆积。队列或写入通路持续不可用时明确失败，不静默丢成功响应。

停止插件时取消等待中的核心 → 插件请求；通信结束后旧实例的排队工作失效、迟到结果不再回写。5 秒超时是等待期限，不承诺撤销已经开始的持久化副作用；长远端操作使用单独任务/取消机制。插件自身也必须持续读取控制帧，不能同步等待核心响应时停止读取核心发来的请求。正文不经此控制协议传送。

核心发起：

```json
{"jsonrpc":"2.0","id":"initialize","method":"initialize","params":{"coreApiVersion":"1","coreVersion":"0.6.0","capabilities":["logging.write","messages.publish","ui.v1","rpc.concurrent","terminal.v1","viewData.v1","files.v1"],"dataDirectory":"<系统应用数据目录>/plugin-data/sodeck.hello"}}
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

## 通用 TCP 与弹窗（新增能力，尚未发布）

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

## 本地构建 Hello

```sh
npm run plugin:build
# 指定目标时仍需要相应编译环境
npm run plugin:build -- --target aarch64-apple-darwin
# 正在运行的插件不能原位覆盖；关闭并重启后构建，或使用另外的输出目录
npm run plugin:build -- --out-dir artifacts/another-build
```

产物位于 `artifacts/plugins/`，包含清单和原生入口的目录与压缩归档。构建脚本在 Hello 工程目录调用 Cargo，使其 Windows 静态运行库配置生效；归档前检查原生架构和额外的 Visual C++ 运行库依赖。Hello 自己的版本只在 `plugins/hello/Cargo.toml` 中修改，独立于核心版本；用户运行时不需要 Rust、Node.js 或额外安装 Visual C++ 运行库。

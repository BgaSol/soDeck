# 外置插件 API v1

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

UI 事件最多 24 KiB，是控制交互接口，不能用来传终端输出或文件内容流。RPC 原有 64 KiB 帧上限仍在，超时 5 秒；真实业务的数据通道、背压和恢复策略后续单独实现。可信插件不增加权限表或授权流程。

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

停止插件时取消等待中的核心 → 插件请求；通信结束后旧实例的排队工作失效、迟到结果不再回写。5 秒超时是等待期限，不承诺撤销已经开始的持久化副作用；长远端操作将使用单独任务/取消机制，不能通过延长全局超时实现。插件自身也必须持续读取控制帧，不能同步等待核心响应时停止读取核心发来的请求。正文数据通道尚未实现。

核心发起：

```json
{"jsonrpc":"2.0","id":"initialize","method":"initialize","params":{"coreApiVersion":"1","coreVersion":"0.4.0","capabilities":["logging.write","messages.publish","ui.v1","rpc.concurrent"],"dataDirectory":"<系统应用数据目录>/plugin-data/sodeck.hello"}}
{"jsonrpc":"2.0","id":"initialize","result":{"apiVersion":"1"}}
```

核心等待初始化 5 秒。插件回应 initialize 后可调用以下方法。新增 coreVersion/capabilities 是可选协议信息，旧插件忽略即可；能力表示当前核心实际实现，不是授权清单。核心现在可以建立 SSH 会话，但尚未向插件开放会话、PTY 或 SFTP RPC，所以此处不声明这些能力。插件不能自行声称来源，核心附加 pluginId、instanceId、receivedAt。

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
- iframe 经 sodeck.request("data",request) 调用正文桥。终端 op 为 attach/ack/write/resize/checkpoint/detach；文件 op="files"，action 为 home/list/stat/read/describe/chunk/stage/save/status/download/cancel。每块最多 16 KiB；按真实 view/实例/Context 校验，不通过普通 ui.event 传正文。
- 页面以 mounted 消息 flush=true 声明移窗/关闭前需要同步。收到 flush，完成在途同步后回复同 id 的 flushed；携带 error 可阻止丢失输入。旧插件不声明则保留原行为。

所有权、缓冲、保存与下载约束见 [实施记录](implementation-remote-plugins.md)。不引入跨插件直调或插件授权流程。

## 本地构建 Hello

```sh
npm run plugin:build
# 指定目标时仍需要相应编译环境
npm run plugin:build -- --target aarch64-apple-darwin
# 正在运行的插件不能原位覆盖；关闭并重启后构建，或使用另外的输出目录
npm run plugin:build -- --out-dir artifacts/another-build
```

产物位于 `artifacts/plugins/`，包含清单和原生入口的目录与压缩归档。构建脚本在 Hello 工程目录调用 Cargo，使其 Windows 静态运行库配置生效；归档前检查原生架构和额外的 Visual C++ 运行库依赖。Hello 自己的版本只在 `plugins/hello/Cargo.toml` 中修改，独立于核心版本；用户运行时不需要 Rust、Node.js 或额外安装 Visual C++ 运行库。

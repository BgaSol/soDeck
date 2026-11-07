# 系统状态插件编码方案

日期：2026-11-08。本文保留实施前的代码目标；实际实现、偏差与验证范围见[实施记录](implementation-system-status.md)，正式接口契约见[插件 API](plugin-api.md)。本文的验收要求不代表已通过。

基线：原仓库 `main`，提交 `455a55ca54e8602ff06ac54fb9c1baa4b012ec18`；本次 fetch 后与 `origin/main` 一致。功能与指标口径见[功能设计](plan-system-status.md)，本文负责代码拆分、接口契约、并发边界和实施验收。沿用原仓库工作区，不创建分支、工作树或 PR。

## 1. 实施结论

新增外置插件 `sodeck.system-status`，代码放在 `plugins/system-status/`。核心只补两组通用能力：不依赖页面的工作区逻辑实例，以及会话状态查询和有界远程命令任务。系统识别、采集脚本、数据解析、采样频率、指标口径、趋势和 UI 全部属于插件。

保留已经明确的行为：默认右侧固定并展开；首次连接各类数据尽快读取一次；动态指标默认 5 秒、状态与拓扑 30 秒、容量和运行时长校准 60 秒，规格按连接或显式刷新读取；隐藏、换面板、切服务器、最小化和移窗继续采集。休眠期间无法采集，恢复后断开曲线并校准。

不把采集放在 iframe 的定时器内，不借用终端插件，不在远端安装程序，不引入监控数据库、统一任务框架或新的第三方生产库。插件按现有方式使用仓库内 SDK 和已锁定的 JSON/序列化、Base64 库；实现前核对现有版本并提交自己的 Cargo.lock。SDK 的同步调用和现有 4 秒超时保持不变。

## 2. 当前代码证据与必须覆盖的位置

以下是对当前源码的核对，不将功能设计里的拟议能力当作现有接口。

| 当前位置 | 已有行为 | 本功能需要的变化 |
| --- | --- | --- |
| [component_ui.rs](../src-tauri/src/component_ui.rs) | Definition、View、WorkspaceUi；侧栏限定 perWorkspace | 增加可选后台声明及页面展示标记，保持旧清单默认值 |
| [layout.rs](../src-tauri/src/component_ui/layout.rs) 的 ui_snapshot/open_view | 在页面查询时登记插件、建立工作区；创建 view 同时选择面板 | 抽出可由核心调用的状态协调；拆开确保逻辑 view 与选择面板 |
| 同文件 target_ready | 等待工作区所有 view 在目标窗口挂载 | 排除从未请求页面的后台 view，仍等待已展示页面 |
| [host.rs](../src-tauri/src/component_ui/host.rs) 的 ui_attach/ui_event | ui.mount 在页面 attach 时发出；事件处理后广播 workspace_changed | 后台幂等 mount；只读快照不造成每秒全工作台广播 |
| [components.ts](../src/components.ts) 的 paint | 为每个逻辑 view 创建 iframe | 后台 view 未请求展示时不创建 iframe；既有页面继续沿用 |
| [connections/admission.rs](../src-tauri/src/connections/admission.rs) | 接纳连接、重连递增 Context、rebind_context 作废旧 surface | 接纳后唤醒后台协调；新旧连接的采集结果隔离 |
| [connections/route.rs](../src-tauri/src/connections/route.rs) | 最终认证会话可开 session channel | 新执行服务使用此入口，SSH 类型不进入插件 |
| [connections/validation.rs](../src-tauri/src/connections/validation.rs) | 测试探测合并 stdout/stderr、5 秒超时 | 只作为传输参考；不能直接改名公开成生产接口 |
| [resource_owner.rs](../src-tauri/src/resource_owner.rs) | 校验插件实例、view、关闭冻结 | 启动复用严格检查；结果读取和清理允许所属旧任务收尾 |
| [plugins/dispatch.rs](../src-tauri/src/plugins/dispatch.rs)、[process.rs](../src-tauri/src/plugins/process.rs) | 有界并发 RPC、能力声明、退出前后两次资源清理 | 加 session 路由、能力与两处执行任务清理 |
| [core.rs](../src-tauri/src/core.rs)、[workspaces.rs](../src-tauri/src/workspaces.rs)、[close.rs](../src-tauri/src/component_ui/close.rs) | 核心退出、工作区变化、视图关闭与释放 | 新任务覆盖这些入口，关闭取消时不能永久停掉采集 |
| [plugin-client](../crates/plugin-client/src/lib.rs) | 独立响应读取线程，4 个处理线程，call 等待最多 4 秒，64 KiB 帧 | 直接复用；业务 handler 不等待采集，锁内不调用 call |
| [settings.rs](../src-tauri/src/settings.rs) | settings.changed 可直接送到插件进程 | 后台调度应用设置，不依赖页面 settingsChanged |
| [package-plugin.mjs](../scripts/package-plugin.mjs)、[plugin.yml](../.github/workflows/plugin.yml) | 按目录读取 Cargo/release 元数据，五目标独立打包 | 新增元数据即可使用；补实际包验收与相关检查映射 |

关键发现：仅在 ui.mount 中启动线程不够，因为从未展开的插件未必有逻辑 view；仅增加逻辑 view 也不够，因为当前前端会给所有 view 创建 iframe，移窗还会等待所有页面。这三层必须一起改。

## 3. 目录与依赖方向

以下新增路径均为实施目标，尚不存在。

```text
src-tauri/src/
  session_exec.rs                 # 通用任务登记、参数/Owner 校验、poll/cancel/release
  session_exec/ssh.rs             # SSH exec channel、字节流、退出与取消
  component_ui/background.rs     # 后台 view 协调、初始化与重试

plugins/system-status/
  Cargo.toml / Cargo.lock
  release.json / README.md / CHANGELOG.md
  src/
    main.rs                      # SDK RPC handler、初始化及共享状态
    model.rs                     # 组状态、快照、能力、设备身份、原始计数
    session.rs                   # 插件侧 session RPC、任务票据和结果拼接
    scheduler.rs                 # 单调时钟调度、限流、公平、取消与退避
    metrics.rs                   # 纯函数：CPU/内存/速率/身份与基线检查
    history.rs                   # 有界时间序列、缺口、容量预算
    collectors/
      mod.rs                     # Collector 枚举、探测与批次选择
      linux.rs / windows.rs
      darwin.rs / freebsd.rs / openbsd.rs / netbsd.rs / unix.rs
    scripts/                     # 与采集器对应的固定 .sh/.ps1 脚本，include_str!
  ui/
    index.html / bridge.js / view.js / style.css
    charts.js                    # 数值到 SVG 路径、缺口和键盘数据点
  tests/fixtures/                # 原始输出、来源说明、期望结果

src-tauri/tests/session_exec.rs
src-tauri/tests/fixtures/system_status_flow.rs
scripts/test-system-status-ui.mjs
scripts/test-system-status.mjs
```

使用具体枚举 `Collector::{Linux, Windows, Darwin, FreeBsd, OpenBsd, NetBsd, Unix}` 分派。每个采集器提供探测结果、构造某组任务及解析结果三个操作；只有第二处真实重复出现时才抽公共解析函数。不建立 Collector 动态注册、插件内插件、依赖注入框架或采集 DSL。

所有远端家族模块都编入五种客户端产物，不能用 `cfg(target_os)` 选择远端采集器。Windows 客户端必须能读 OpenWrt，Linux 客户端也必须能读 Windows；只在核心处理本机进程与路径差异。

```mermaid
flowchart LR
    UI[侧栏 iframe] -->|只读缓存与用户动作| P[插件进程：调度、解析、趋势]
    H[核心：工作区逻辑 view] -->|幂等 ui.mount / disposeView| P
    P -->|session.state / exec| E[核心：有界命令任务]
    E --> S[核心已认证会话]
    S --> R[远端只读一次性脚本]
```

## 4. 宿主：后台实例和展示分开

### 4.1 清单与默认布局

release.json 使用下列 view；插件 UI 与主 API 版本仍为 1。

```json
{
  "id": "overview",
  "title": "系统状态",
  "icon": "activity",
  "container": "sidebar",
  "instancePolicy": "perWorkspace",
  "defaultSide": "right",
  "defaultPinned": true,
  "openOnWorkspace": "reveal",
  "startupOrder": 40,
  "backgroundOnWorkspace": true
}
```

Definition 新字段为 `#[serde(default)] background_on_workspace: bool`，只允许 `sidebar + perWorkspace`。新增初始化能力 `ui.background.v1`；旧清单默认 false。现有网络隧道默认右侧但 startup=none，正常新工作区不会抢占系统状态；用户已保存其他同侧展开偏好时沿用现有优先顺序。

在 View 增加内部/投影字段 `uiRequested: bool`：后台确保实例时为 false；任何显式选择或启动布局选择时设为 true，直到 view 销毁。普通插件创建的 view 始终为 true。后台存在和左右栏 active ID 是两个状态。

1. 将 open_view 内“寻找或建立逻辑 view”提取为 ensure_view；该函数不选择栏位、不更新 lastUsed、不抢焦点。
2. open_view 调用 ensure_view 后执行现有选择逻辑，并将 uiRequested 设为 true。
3. 前端只为 uiRequested 的 view 建立 surface；已存在的隐藏 surface 沿用当前保留行为。
4. target_ready 只等待 uiRequested 的 view。首次默认展开的系统状态仍需挂载就绪；从未显示的后台 view 不阻塞移窗。
5. 后台标记不加入用户布局偏好，用户的收起/移侧/固定/启动偏好仍由现有格式保存，不进行全局布局迁移。

### 4.2 不依赖前端的协调入口

将 ui_snapshot 中插件登记、默认偏好、工作区初始化的纯状态部分收敛到 `reconcile_workspaces`，输入为全部有效工作区与运行插件的只读快照；ui_snapshot 调用后只按窗口过滤投影。该函数同样供后台协调使用，避免两份登记规则。

background.rs 维护一个有界的待协调通知（重复唤醒合并）和按 `(pluginInstanceId, viewId, generation)` 保存的初始化状态。在核心服务启动时建立一个协调循环，持有 Weak<AppCore>；只在通知或已有初始化重试到期时运行。首次启动、连接接纳、插件进入 Running/停止、工作区关闭与重连后唤醒。新调用必须在释放 connections/workspaces/component_ui 锁后发生。

协调先确保所有后台 view，再恢复首次工作区布局；已有工作区只补缺失实例，不突然替换用户当前面板。仍按当前“插件启动阶段结束后落默认布局”的策略处理启动竞态。已关闭或冻结工作区不创建新资源；插件异常恢复必须替换旧 instance 的后台 view 归属，清空原采集缓存，沿用 definition 偏好。

需要初始化的条目交给最多 2 个固定执行 worker，每 worker 一次 ui.mount；候选直接来自 view 状态，不建立无界任务队列。核心监督插件 stdout 的循环不能同步等待向同一插件发出的 mount，否则嵌套 RPC 无法得到响应。

ui.mount 不新增协议变体，无 surfaceId，返回普通 metadata 与空/缓存数据。插件按 viewId 幂等 upsert，并校验 Context；之后页面 attach 再次 mount 仅取当前缓存。后台初始化失败按 1/5/30 秒重试，三次仍失败则等待新连接、插件恢复或用户打开重试；迟到回复只有 instance/view/generation 都仍匹配才接受。关闭后的回复不能复活实例。

### 4.3 关闭、断线与移窗

所有后台 view 进入既有关闭清单。系统状态始终 `dirty=false, busy=false`；beforeClose 只返回快照，不提前销毁。其他插件使关闭流程停在确认框时，冻结阻止新 exec，已在途只读任务可结束；若用户取消关闭，调度下一轮恢复，暂停标记不受影响。确认关闭后 disposeView 清除插件条目，并由核心兜底取消任务。

移窗不触发 disposeView，不改变采集 Context；旧窗口 surface 按原规则失效。重连会改变 connectionId/generation，后台通过 session.state 得知；旧任务输出不得写入新快照。插件崩溃后核心清理资源，新进程重建历史，不能声称补回丢失样本。

## 5. 核心 RPC：会话状态与短命令任务

### 5.1 能力与参数

新增 `session.state.v1`、`session.exec.v1`；只供插件进程调用，不给 iframe 增加任意命令接口。新插件 initialize 检查这两项及 ui.background.v1、settings.v1；缺少时明确提示升级核心，不能降级为只在面板打开时采集。

```text
session.state({viewId})
  -> {context, connected, closing, capabilities:["exec.v1"]}

session.exec.start({viewId, expectedContext, operationId, sequence,
                    command, timeoutMs, maxOutputBytes})
  -> {taskId, state}

session.exec.poll({taskId, stdoutOffset, stderrOffset})
  -> {state, stdout:{dataBase64,nextOffset}, stderr:{dataBase64,nextOffset},
      outputComplete, exitCode, exitSignal, error, remoteExitConfirmed,
      elapsedMs, outputLimitExceeded}

session.exec.cancel({taskId}) -> {state}
session.exec.release({taskId}) -> {released:true}
```

Context 沿用当前完整字段 `workspaceId/serverId/connectionId/generation`。session.state 按当前 plugin instance + view 查所属工作区，无凭据、无路径跳板秘密；断线和关闭冻结仍能返回状态。`capabilities` 表示传输可提供 exec，并不表示远端账号一定允许执行或具备采集工具。

command 是远端非交互命令文本，最大 16 KiB、禁止 NUL。当前 SSH 以无 PTY exec 实现，不提供 stdin 写入、环境变量列表和交互式 shell；发送 stdin EOF，防止命令等待输入。未来协议不能实现同样语义时返回不支持，不让产品插件直接使用 SSH。

timeoutMs 为 1,000–30,000，包含开 channel、exec 确认和读取；超时清理额外最多等待 1 秒。maxOutputBytes 为两路合计预算，默认 256 KiB、最大 1 MiB；任务全局 8 个在途、每插件进程 4 个。参数无效直接拒绝，不悄悄扩大或缩减请求。

### 5.2 幂等、隔离和资源有界

start 先取得 Owner，再核对 expectedContext 与 view 当前 Context，并通过 connection_session 固定这一次最终会话；登记时再次验证实例/关闭状态，登记后检查取消令牌以覆盖并发关闭。任务 ID 由核心生成，不用用户可控名称当文件或全局资源键。

operationId 长度最多 128，sequence 为 view 当前 Context 内递增 u64 的十进制字符串。首次原子登记时记录 `(instance,view,context,operationId,sequence,参数)`；相同请求重试返回同一 taskId，参数变化报 operationConflict。保留每 view/Context 已接受的最高 sequence：结果回收后，低序号或同序号未知请求返回 operationExpired，绝不重新执行。校验失败或资源繁忙的请求不消耗 sequence；同一 view 串行 start。

该补充避免只有短期 operationId 缓存时，迟到的重试在缓存过期后再次运行。插件 call 超时后只重试原票据；拿到明确拒绝、原任务或过期结果前，不换新 ID 启动同组命令。过期且远端结果未知时按未确认终止处理。

任务保存 Owner 和独立取消令牌，父会话取消立即唤醒任务；取消单个任务只关自身 channel。poll/cancel/release 校验调用者与任务的 plugin/instance 归属，允许断线、重连、关闭冻结期间读取/清理旧任务；不能复用 resource_owner 的“必须可启动”检查来阻止收尾。

每插件最多 64 项在途及终态记录，输出预留预算每插件 8 MiB、核心 16 MiB；start 按请求的最大值预留，失败或释放归还。终态输出 30 秒后过期；正常插件读完即 release。幂等元数据最多 4,096 条/插件、保留最多 120 秒，不存第二份输出，旧条目驱逐仍由 sequence 水位防重放。view 销毁后清掉水位；旧 view/Context 本来就不能再次 start。

poll 每路最多 8 KiB 原始字节、Base64 返回，offset 为非负十进制字符串；相同 offset 可重读，越过已产生字节报 invalidOffset。每个响应校验完整 64 KiB 帧。poll 不阻塞等待远端，也不因读取结果重复计量。outputComplete 只在任务终态且两路本次返回后都读到尾部时为 true，不能看到 exitCode 就丢弃后续输出。

release 在运行态等同请求取消并丢弃结果，立即返回；后台关闭 worker 完成后归还额度。重复 release/cancel 不产生新任务，已释放查询返回 taskExpired。关闭 view、插件异常/正常退出、应用退出时主动清理对应任务和缓存，不依赖 30 秒回收期限兜底。

### 5.3 执行、结果与错误

任务状态为 running、completed、failed、cancelled、timedOut。completed 表示收到完整退出结果，即使 exitCode 非零；是否采集成功由插件根据退出结果和分段数据判断。无 ExitStatus/ExitSignal 的关闭为 failed/missingExitStatus；远端 EOF 不是完整结束，不提前认定成功。

执行循环在现有 Tauri/Tokio runtime 中 select：取消、父会话取消、绝对 deadline、channel 消息。分别处理 stdout、stderr、退出状态、退出信号和关闭；输出超额立即失败并清理，不能把半截 JSON 当成功。绝不持有 AppCore 全局 mutex 等待网络 I/O。

取消/超时尽力向该 channel 发终止信号并关闭，限定清理等待，不关闭共享 SSH 连接。remoteExitConfirmed 仅在得到远端退出确认时为 true，关闭 channel 本身不算确认。不能保证杀死已经脱离的子进程；插件脚本禁止后台化，未确认退出的数据源暂停自动重试，避免反复堆积卡住的 df/CIM。

方法错误通过 JSON-RPC error.data.code 给出 invalidParams、staleContext、workspaceClosing、unsupported、resourceBusy、operationConflict、operationExpired、taskExpired；任务错误保存在 poll.error 的 `{code,message}` 中，包括 execRejected、sessionLost、outputLimit、missingExitStatus。消息有界且不包含整段命令输出，插件据 code 决定重探测、等待或局部失败。

现有 SDK 的 Client::call 会把 RPC error 压成字符串；为本插件可恢复分支增加一个小的 `call_detailed` 返回结构化 RpcError，原 call 继续映射为 String，既有插件不改调用。只在 SDK 保存 error 的 code/message/data，不加入 session/监控业务类型。补响应错误与旧 call 兼容测试；不能靠匹配中文错误消息调度。

核心以 record_business 记录实际发出的命令文本及收到的两路有效载荷，重复 poll 不重复计入；SoDeck 通信量与远端网卡累计量保持独立。源码/日志不保存采集原文、认证资料或默认打印完整 IP 清单。

## 6. 插件内存模型与计算边界

内部类型用普通 Rust struct/enum；示意字段不是完整实现。

```text
WorkspaceState {
  viewId, context, localEpoch, userPaused, connectionState, collector,
  groups: {dynamic, topology, capacity, uptime, identity, capability},
  inFlight: Option<CollectionTicket>, latest, baselines, history, navigation
}
GroupState { nextDue: Instant?, lastSuccess: Instant?, observedAt,
             failureCount, retryAt, manualPending, quarantined, sourceStates }
CollectionTicket { taskId?, operationId, sequence, context, localEpoch,
                   group, stdoutOffset, stderrOffset, outputBuffer }
Metric<T> { value: Option<T>, state, source, observedAt, windowMs?,
            estimated, reason? }
```

localEpoch 在暂停、恢复、Context 更换、销毁时递增，所有 worker 完成消息携带该值；迟到结果必须先检查 instance/view/context/epoch，再更新数据。采集失败不清空最近成功值，而是保留时间与原因。sourceStates 允许同组不同来源独立失效；没有某项性能计数器不使整个 dynamic 组停止。

各组自己的 revision、时间和可用性独立。全局 snapshotRevision 仅在缓存/状态变化时增加，不把运行时长的页面递增视作新的远端数据。公开状态区分 ok、warmingUp、unsupported、unavailable、stale；不可用值为 null，不是 0 或空字符串。

分批容量和列表项还保留自己的 observedAt/lastSuccess；某一批成功不能把未刷新项目一并标新。组摘要需要区分“全部已更新”和“部分过期”。主机/设备显示名最多 256 字符、地址最多 128、路径最多 4,096、单条错误最多 512；按实际序列化字节预算分页，省略内容须有标记。

原始计数、容量和高精度时间用 u64/u128 检查运算；JSON 使用十进制字符串。差值先在整数中计算，再转有限 f64 速率；浏览器只绘制已计算的数值，必要格式化使用 BigInt，禁止先把两份大计数转 Number 再相减。

基线键至少包含 Context、系统启动标识、指标来源和设备身份。设备身份建议 Linux 的 ifindex/可见硬件地址与设备代次、块设备 major:minor；Windows 的 GUID/实例身份；BSD 的可验证接口索引。身份资料读不到时，一旦观测到消失/新出现即增加插件本地代次；无法识别同名快速替换是需明确记录的限制，不能保证仅凭名称永不串接。

CPU 使用各系统计数器规定的时间分母；网络/磁盘优先用远端 uptime 或性能时钟。只能取得本地时间时用任务开始/结束的单调时刻中点并标 estimated。任意计数倒退、启动/身份变化、时间差非正或采样缺口超过有效周期三倍：舍弃基线、断开曲线，本轮 warmingUp。Linux 总时间不重复加 guest；磁盘 sector 乘 512；Windows inverse timer 不是直接百分比。

history 使用 VecDeque，每序列最近 5 分钟且最多 100 点，CPU/内存与最多各 32 个接口/磁盘序列。当前选中设备优先保留；被截去的历史不能补造。列表上限接口/块设备各 256、文件系统 512；超限显示 truncated 和已知的省略数量，无法计数则省略数量为 null。每工作区 2 MiB、插件总样本预算 64 MiB，按真实结构和字符串占用计账；达到预算统一缩短历史并显示实际起点，保留每个工作区当前值。

## 7. 调度器：一份状态，多种频率

### 7.1 调度表

| 组 | 正常触发 | deadline | 调整动态周期是否影响 |
| --- | --- | --- | --- |
| dynamic | 5 秒，可设 3/5/10/30 秒 | 10 秒 | 是 |
| topology | 30 秒；设备增删时提前一次 | 10 秒 | 否 |
| capacity | 60 秒，仅本地文件系统 | 20 秒 | 否 |
| uptime | 60 秒；首次、恢复和重连立即 | 5 秒 | 否 |
| identity | 首次、重连、手动刷新、已确认相关规格变化 | 分步，每步 5 秒 | 否 |
| capability | 首次探测；缺失能力 10 分钟复核 | 每步 5 秒，总预算 30 秒 | 否 |

所有组第一次可执行时立即到期，identity/probe 拆成小任务，基础可读就先展示，不等所有系统信息完成才显示 CPU。Linux dynamic 廉价读取 meminfo/uptime 时可顺带校准总量和时间；Windows 不因这种便利重复执行全部 CIM 查询。

每工作区最多一个远端任务在途，插件全局最多 4 个。一条调度线程维护全部 WorkspaceState；最多 4 个 RPC worker，每次只执行一次短 session RPC，随后把结果发回调度线程。worker 不整段等待一个 20 秒远端任务，避免占满后连本地 session.state 都无法查询；长任务通过 poll 状态机推进。队列容量固定 32，每工作区同一动作至多一个待办，不排逐字段任务。

本地 session.state 目标每工作区 5 秒查询一次，开始新远端任务前再校验；其 RPC 与 poll/cancel 优先于新 start。核心父会话取消负责即时停止远端任务，因此不用等待这次查询才清理。高负载/IPC 超时时展示延迟，不声称严格 5 秒响应保证。

正常远端 poll 从 100 ms 逐步到 250 ms，不 busy loop；无在途任务时调度器通过 recv_timeout 等到最近期限/控制消息。UI handler 只短锁读取已发布快照或投递动作，不在锁内调用核心、解析大输出或等待 worker。

### 7.2 到期顺序与失败处理

工作区之间 round-robin，首次开始按工作区稳定次序在一个动态周期内错峰；同一工作区优先到期 dynamic，但连续两次 dynamic 后有到期慢组就先执行最早到期慢组。未完成任务遇到定时点只保留“已到期”，绝不补跑每个漏掉的时隙；完成后将 nextDue 推进到原单调时间轴上严格晚于当前时刻的下一周期边界。手动刷新不重置慢组的周期，实际采样窗口如实显示。

慢组可阻塞该工作区最多一个 deadline，期间保留快照，dynamic 超过自己的新鲜度阈值即 stale。不能同时承诺“每工作区串行”和“容量挂起时 CPU 仍严格每 5 秒”；目标是有界延迟，慢任务结束/取消后恢复动态任务。

手动刷新把各组 pending 位设为 true，多次点击合并。暂停取消在途、清掉定时待办并递增 epoch；暂停下手动刷新允许一次各组巡回，然后仍暂停。恢复保留设备显示，但清速率基线、先校准并恢复调度。暂停标记仅内存保存，移窗/重连保留，关闭工作区清除。

临时失败按数据源以 max(本组周期, 5/10/30/60 秒) 退避；无周期的首次 identity/probe 最多三次。明确 unsupported 不逐周期重试，只在手动/重连或低频复核时探测。已提交远端执行的任务 timeout/cancel 且 remoteExitConfirmed=false 时，将该命令包含的来源置 quarantined；未发出 exec 的取消无需隔离。后续快速任务排除这些来源，其他独立来源继续。手动刷新或恢复采集允许用户明确触发的一次再试；仍未确认退出则继续隔离，不改成新的自动循环。

settings 页 `sampling` 只提供 `dynamicIntervalSeconds`，select 值 3/5/10/30、默认 5、apply=live。initialize 后通过 settings.get 读取，settings.changed 按 revision 校验并把新的周期写入后台状态后确认；不能只把值缓存到没有读取接口的 SettingsCache 就返回“已应用”。不改变慢组到期、不重建 view、不发并发命令。页面沿用既有设置桥显示同一值。

新鲜度根据 lastSuccess 的单调时刻计算：动态两周期，topology 60 秒，capacity/uptime 120 秒；退避和排队不延长阈值。运行时长仅在在线、未暂停、校准有效时用远端基准加本地经过时间；睡眠恢复先校准，显示推算不用于任何速率公式。

调度线程至少每秒记录一次本地 Instant 与 SystemTime 的对应值，仅用于检测时间不连续。唤醒间隔明显超预算，或墙钟与单调时钟的增量偏差超过 2 秒时，先清速率基线并要求 uptime 校准；墙钟校时也允许保守地断开曲线。远端 uptime/启动标识和采样窗口继续做独立检查，不依赖可见页面发送“恢复”事件，也不拿 SystemTime 差值计算速率。测试分别注入单调时钟包含休眠、不包含休眠和墙钟跳变。

## 8. 采集器：固定脚本、可验证输出

### 8.1 探测和封装

先执行小型正向探测：uname 识别家族后确认 sh；无法确认时分别探测 powershell.exe/pwsh 的 Windows CIM 能力，失败不等于另一种系统成功。探测不把 && 等运算符寄希望于未知默认 shell。CYGWIN/MSYS 再试 Windows 原生路径；WSL 按其 Linux 登录环境处理。未知 POSIX 系统进入 Unix 回退，RouterOS/受限 CLI 没有正向结果就显示执行环境不支持。

脚本随插件编译，不下载运行时脚本。Unix 固定 POSIX sh + LC_ALL=C，采用版本化分段文本：请求 nonce、段名、BEGIN/END、段退出码；每段边界独占一行、输出末尾补换行。nonce 只来自插件生成的 ASCII 票据，不能使用远端文本。解析器限制 banner、段数、行长和总量，拒绝重复段/错误 nonce/缺 END；单段失败保留其他完整段。

Windows 使用 UTF-16LE EncodedCommand，-NoProfile -NonInteractive，显式 UTF-8 输出压缩 JSON，带 schemaVersion 和 nonce。每数据源 try/catch，区分不存在和权限/运行错误；计数先转 invariant 十进制字符串，再 ConvertTo-Json，数组即使零个/一个元素也固定为数组。不启用 WinRM，不调用 WMIC，不修改 execution policy。

成功条件为协议标记完整、字段合法、对应段结果可接受，而不是只看 exitCode=0。核心限流造成整体输出不完整时不发布本次数据；完整但某段明确失败时只保留其他成功段。命令输出只作数据，不 source os-release，不执行远端给出的命令，也不用 innerHTML 渲染字符串。

### 8.2 每个家族落实的来源

| 采集器 | dynamic 的具体内容 | 慢数据及首轮验证 |
| --- | --- | --- |
| Linux | /proc/stat、meminfo、net/dev、diskstats、uptime；原始整数本机计算 | os-release/cpuinfo/sysfs 为规格；loadavg/接口身份/IP 为 topology；mountinfo 为挂载枚举 |
| OpenWrt/Alpine/BusyBox | 复用 Linux；缺 diskstats/CPU 型号不影响其他段 | 验证 ash、无 GNU 工具/无 SFTP；ubus 只作为首轮型号增强；/rom 与 overlay、tmpfs 分开展示 |
| Windows | PerfRawData_PerfOS_Processor 的 _Total 和 Timestamp_Sys100NS、Memory.AvailableBytes；可用网卡原始字节；PerfDisk_PhysicalDisk 原始读写与性能时间戳 | OS/Processor/ComputerSystem 按连接；IP/接口表 30 秒；Volume/LogicalDisk 60 秒；PageFileUsage 若昂贵单独来源，不拖垮整组 |
| Darwin | top 非交互双样本取得 CPU、vm_stat 读取实际页大小、netstat link 层字节 | sw_vers/sysctl/ifconfig/本地 df；明确 CPU 约 1 秒观察窗口；内存没有可靠 available 时仅展示分类，总使用率/相应趋势保持不可用 |
| FreeBSD | kern.cp_time、对应 vm.stats.vm 字段、netstat link 层字节 | boottime/loadavg/ifconfig/本地 df；固定 FreeBSD 版本样本，设备 I/O 无可靠来源先标缺失 |
| OpenBSD / NetBSD | 各自独立模块，只有真实样本确认的 CPU/内存/网卡字段进入 dynamic | 先落实系统、运行时长、负载、本地文件系统；不复制 FreeBSD 的 sysctl 键冒充适配 |
| Unix 回退 | 没有可信动态来源则不启动高频任务 | 独立识别 uname/hostname/uptime，容量须能确认本地挂载；无可靠格式就逐项 unsupported |

Windows CPU 从 inverse timer 的前后计数与对应时间差计算；实现时把使用的类/属性/单位写入 fixture README，与低/高负载、中文/英文和多处理器组样本对照。网卡只使用能与 GUID/ifIndex 可靠关联的来源，不能按本地化名称猜关联；未映射计数器实例不假称某块物理网卡。内存缓存 OS 可见总量，明确发现总量变化时才更新规格。

运行时长优先远端单调 uptime/性能来源；只有 wall-clock boottime 时标明估算，并保留来源，不能用它制造精确速率。macOS/BSD 的内存比例、磁盘 I/O、特殊 CPU 汇总以对应系统验证为前置；尚未落实的字段标不可用，不在编码时自行编公式填满 UI。

### 8.3 文件系统与异常数据

容量任务先读不触发 statfs 的挂载元数据，筛除 NFS/CIFS 等远程及无法识别的挂载，再查询已确认本地目标。按平台验证 df 的本地选项或分批传入已引用的路径；不能先执行全盘 df 再过滤输出。每批命令仍受 16 KiB 和时间预算限制，最多 16 个目标，余项跨调度轮转并展示各页真实时间；网络挂载首版仅列挂载信息。

Linux mountinfo 先按字段/分隔符解析，再解码八进制转义；路径作为 POSIX 单引号参数整体转义，绝不拼接成代码。Windows 容量为空的离线卷保留 null，卷无盘符不遗漏。Unix used/available/reserved 不强制满足 total=used+available，使用比例统一说明为 used/total；UI 同时保留 available，避免把预留空间称为用户可用。

解析样本必须含非 ASCII、空格、引号、换行转义、只读固件、bind/overlay、重复接口、缺字段和超长输入。计数器宽度未知时倒退清基线；已知的 32 位回绕也先以清基线实现，除非第二份真实样本证明需要专门补偿。没有完整设备身份时在采集详情说明范围。

## 9. 页面协议与交互落点

沿用文件/网络插件的原生 DOM、bridge、随包 shared.css、文本菜单与主题 token，不增加 UI 框架或图表依赖。已有 network-panel 参考图只用于核对紧凑列表和层次，尺寸/字体/主题最终以当前 src/ui.css、theme.ts 和插件样式为准；不复制参考图中的历史版本文字。

页面只通过现有 ui.event 发送以下 action；其名称属于插件业务，不加入核心路由。

| action | 内容 | 返回 |
| --- | --- | --- |
| snapshot | knownRevision | 未变化仅 revision；变化返回概况、分组时间与状态、选中设备摘要 |
| list | kind, cursor?, revision? | interfaces/disks/filesystems，每页最多 32 项并受字节限制；游标过期重新取第一页 |
| history | seriesKey, knownRevision? | 该序列最多 100 点、起止范围和缺口；只允许已知指标/设备键 |
| refresh | 无远端命令参数 | accepted 和当前刷新状态；仅置调度待办 |
| pause / resume | 无 | 当前暂停与连接状态 |
| navigation | interfaceId, diskId, expandedGroups, scrollTop | 有界内存保存的页面偏好，跨窗复用 |

每个业务响应最大 24 KiB、完整 RPC 帧小于 64 KiB；分页同时考虑项数和编码后的字节数。正文字符串各自有长度上限，错误也有界；取曲线分请求，不把 66 条趋势一次塞进 mount。导航只允许已知字段与设备 ID，过期选择显示“已移除”，不能自动接到同名设备。

可见页面最多 1 秒读一次快照，revision 不变不重绘；隐藏/unmount 停止页面轮询和动画。恢复显示立即取快照，再按需取当前曲线/列表；每个请求带页面 epoch，迟到响应不能覆盖新 Context、选择或 surface。主题变化只更新变量，不重启采集。

host.rs 的 update_metadata 返回是否真的改变 title/dirty/busy；ui_event 仅在元数据改变时 workspace_changed，返回普通缓存不广播。用普通测试插件证明此通用优化，而不是在核心识别 action=snapshot 或系统状态 ID。

界面采用功能设计的单列顺序：动态值时间/刷新 → CPU 与内存/运行时长 → 选中接口速率与趋势 → 文件系统 → 折叠设备与采集详情。保留宿主 280px 默认、160–480px 宽度，不改全局尺寸。160px 时换行、不横向裁切；长 IPv6/挂载点可完整复制，刷新不重建全部 DOM、不跳焦点和滚动位置。

用 SVG 绘制简单趋势，按实际时间横轴，缺口分 path，不补零。CPU/可比较内存比例固定 0–100%，吞吐图标单位并同时保留当前文本数值；键盘可查看选中序列的数据点。状态不只用颜色，常规数字刷新不设全区域 aria-live，错误和用户操作反馈才适度播报。使用系统字体/等宽数字，不加载 CDN 字体、不做无意义的持续动画。

## 10. 测试：先证明行为，再写业务实现

现有测试结构足够承载大部分验证。新增夹具只编入 test-fixtures，不进入核心发行依赖。以下全部是实施要求，目前尚未执行。

| 批次 | 放置位置 | 必须证明的行为 |
| --- | --- | --- |
| RPC 任务 | session_exec 单元 + src-tauri/tests/session_exec.rs | stdout/stderr 分离、分页重读、完整退出、非零退出、缺退出状态、输出上限、deadline、单任务取消不伤共享会话 |
| 幂等与竞态 | 同上，真实插件进程 + 可控 channel | start 响应丢失只执行一次；release/过期后旧序号不重做；关闭/重连/退出与 start 并发没有遗留资源；两次进程清理均覆盖新服务 |
| 后台生命周期 | 扩展 component_ui.rs/workspaces.rs/fixtures/workspace_ui.rs | 从未调用 ui_snapshot/attach 就有后台 mount；普通插件不变；同侧占用仍采集；从未显示 view 不阻塞移窗；初始化晚回不能复活已关实例 |
| 展示协议 | 扩展 test-workspace-ui/test-workbench-interactions | 未请求展示不创建 iframe；默认右侧展开、偏好保留、移窗前后 viewId 一致、旧 surface 失效；snapshot 不广播整个工作台 |
| 调度 | scheduler 内纯状态测试，显式传入 now | 5 分钟远端调用序列符合不同组周期；改为 3 秒不触发慢组；20 工作区公平；无补跑/重叠；慢组不会饿死；冻结取消关闭可继续 |
| 指标解析 | 插件单元 + 原始 fixture | 大于 2^53、u64 边界、重启/重置/回绕、CPU guest、Windows timer、macOS 16 KiB 页、缺 MemAvailable、身份更换、局部坏段 |
| 资源与超时 | 插件压力测试 + 核心集成 | 1 小时运行缓存有界；多次重连无泄漏；未确认退出的来源不自动重启；其余来源继续；日志只记录状态变化 |
| 页面 | test-system-status-ui，真实插件 UI 资源 | 三主题、160/280/480px、缺失/过期/暂停/断线、长名称、分页、键盘、图表缺口、隐藏两分钟历史连续 |
| 真实传输 | system_status_flow + test-system-status | OpenSSH/Dropbear/Windows OpenSSH 无 PTY 采集、普通账号、混合跳板、断线取消及终端/文件/隧道并行 |

远端 fixture 记录系统版本、SSH 服务、shell、权限、命令、原始输出和人工确认的单位/含义；敏感主机名/IP 可脱敏但保留编码及结构特征。手写坏输入用来验证拒绝行为，不能当成该系统已经兼容的证据。

远端实测范围：Debian/Ubuntu、一个 RPM 系、Alpine、OpenWrt+Dropbear、Windows Server 与 Windows 11、Apple Silicon macOS、FreeBSD，以及 OpenBSD/NetBSD 基础路径。Linux 同家族衍生/NAS 可以复用采集逻辑，仍按验证结果列明；其他 Unix 只承诺已验证的基础字段。没有设备时登记未验证项目，不能用模拟样本勾选支持。

客户端测试与远端矩阵分开：五个正式客户端目标均需原生构建、实际包启动/加载/退出；每条远端路径至少一个真实环境，Windows 至少测 cmd 与 PowerShell 默认 SSH shell。无需机械扩成五乘所有远端系统，但不能互相替代。

性能报告记录各组 p50/p95、输出大小、远端新增 CPU 时间、插件内存及多工作区等待。普通环境 dynamic p95 目标小于 2 秒，低配 Windows/OpenWrt 单列。若不能满足默认间隔，先缩减重复查询，依据实测再决定常驻通道；首版不预建远端 daemon。

## 11. 按完整问题拆提交

| 顺序 | 一个提交/阶段解决的问题 | 进入下一步的条件 |
| --- | --- | --- |
| 1 | 采集来源和原始样本可信 | 各系统基础探测成功；可提供/不可提供字段、默认命令成本与退出行为有记录 |
| 2 | 生产通用短命令能取消和收尾 | session.state/exec + SDK 结构化错误；真实进程验证幂等、限额、隔离，旧插件兼容 |
| 3 | 工作区后台插件无需 iframe 即可运行 | 声明、核心协调、uiRequested、移窗及关闭回归一并完成 |
| 4 | Linux/OpenWrt 可在默认侧栏连续观察 | 插件元数据、采集器、调度、缓存、最小完整 UI/设置；隐藏/重连/暂停可验收 |
| 5 | Windows 原生采集完整接入 | CIM/编码/计数公式/普通账号/不同 shell 用真实环境验证，共用同一 UI |
| 6 | macOS/BSD 与 Unix 回退按字段工作 | 逐家族独立提交，来源与缺失提示匹配实际证据 |
| 7 | 作为独立官方插件交付 | 五客户端实际包验收、官方来源、更新路径、文档和版本记录齐全 |

这些是可验证增量，不把只实现 Linux 当作整个需求完成。核心通用能力可以先独立发行；插件正式兼容列表必须在各家族验收后确定，未满足的项继续列为待完成。每阶段按实际改动运行格式、类型、构建和相关测试，不只跑最小新用例。

## 12. 构建、检查与文档接入

新插件版本来源为 Cargo.toml，首版 0.1.0；release.json 提供 binary=sodeck-system-status、tagPrefix=plugin-system-status-v，UI 与 sampling 设置声明。UI 无打包依赖，直接随插件复制；固定采集脚本 include_str! 编入程序，不能依赖开发目录或远端脚本文件存在。

实施阶段可使用现有入口 `npm run plugin:build -- --plugin system-status`。新增插件格式检查追加到 plugin:check；新增 status 单元/集成/UI 脚本创建后才接入 package.json 和工作流。本次不放调用不存在命令的 workflow。

必须核对并同步这些实际调用方：

- scripts/select-checks.mjs 里的 pluginNames、路径正则、集成选择，以及 select-checks.test.mjs；只加目录却不加映射会落入未知输入的全量检查，仍缺专属行为验收。
- .github/workflows/check.yml 的插件/集成矩阵与命令分支，现有 test-openssh/实际包脚本中枚举的组件；扩展为接受 system-status 的独立检查，不强制要求其他插件包。
- scripts/package-plugin.mjs 的帮助文字、release-metadata/verify-binary/原生输入与结果脚本的组件枚举；已有通用路径保持通用，只有实际白名单/断言才改。
- Package plugin 原工作流复用五个 runner 与插件缓存；实际包验证必须解开本次归档，以包内程序和 UI 启动 initialize/mount/采集/dispose/shutdown，不只对 target/release 二进制测试。
- official_plugins.rs 新增固定来源并更新其测试期望；等插件包可正常下载安装时与核心能力版本一起交付，不把插件程序加入核心包。

核心格式/类型 `npm run check`、源码测试 `npm run test`、脚本 `npm run scripts:test`、前端 `npm run web:build`、核心 `npm run build` 按改动阶段执行；插件使用自己的 cargo fmt/test --locked 与独立打包。改 SDK/共享宿主时运行既有终端、文件和隧道回归。netem 不因只读状态插件自动加入日常矩阵。

发布沿用[分发流程](distribution.md)：核心和插件独立版本、独立包、独立 Release，复用原始有效打包 run，同版本正式附件不覆盖。首版插件验证首次安装/启用/重启，升级验证要有真实旧版候选后再执行；不得捏造不存在的旧版本升级证据。记录来源提交、原始 run、Release、实际/跳过范围。核心不依赖本插件构建或全远端验收完成才可打包。

本文和功能设计维护目标；实现后 plugin-api.md 写正式能力契约，usage.md 写操作，development.md 写真实命令/CI 选择，distribution.md 只补发行变化，新增 implementation-system-status.md 记录证据并补索引。普通进度不写根 README，不把未实现能力混入现有 API 文档。

## 13. 本次验证范围与剩余风险

本次仅迁入功能设计、编写此编码方案并补文档索引。已读取现有核心、SDK、插件、前端、检查选择和打包源码，核对 main 与 origin/main；方案中的路径与行为以该基线为准。

文档改动按[开发指南的文档检查范围](development.md#版本与持续集成)验证：现有 Markdown 链接/换行/冲突检查，同时显式检查新未跟踪文档、JSON 示例和 diff 空白。实际执行结果在本次交付说明中报告；不将源码阅读等同运行验证。

未实现采集器或新 API，未构建产品、运行远端采集、原生包验收或发布。最先要消除的风险是 Windows CIM 的权限/耗时和 CPU 口径、OpenWrt 裁剪环境、BSD/macOS 原始字段、远端命令取消不能确保子进程结束，以及后台 view 对移窗与关闭的影响。上述风险均已有对应样本任务和验收项，不能靠扩大支持列表消除。

返回[文档索引](README.md)

# 组件通信与共享状态：代码实施方案

状态：已整体实现，本机验收与支持限制见[实施记录](implementation-component-communication.md)。基于[功能设计](plan-component-communication.md)和当前仓库代码核对，本文确定模块、协议、关键算法、调用链及验收入口。

按完整用户流程统一验收；组件版本只在实际发布时修改。

## 1. 实现决策

采用核心内存 StateHub，提供当前快照、条件写入和有界订阅。继续使用 JSON-RPC、Tauri 命令和现有 iframe bridge。共享状态记录归逻辑 view，页面订阅归 surface；生产者身份由核心附加，插件共同贡献同一主题的数据。

首批具体安排：

| 项目 | 实现选择 |
| --- | --- |
| 系统状态发布 | 插件进程在现有采集完成后提交挂载与容量记录 |
| 终端目录发布 | 终端 UI 解析明确的目录报告，经有 sender 校验的共享状态桥提交；进程提供原 PTY Context，恢复旧终端不能发布新连接目录 |
| 文件组件消费 | 文件页面直接订阅核心投影，处理跟随和挂载展示；不在文件进程再复制一份相同 store |
| 后台插件消费 | Rust 客户端提供同一协议的订阅收件箱，测试插件验证；以后需要后台协作的插件可直接使用 |
| 当前活动页面 | 从 `WorkspaceUi.editor` 派生核心记录，不使用 `Workspace.active_view_id` 保留字段 |
| 大列表传输 | 快照分页；变化封成有版本边界的批次，再拆成有界帧，整批完成后推进订阅游标 |
| 写入冲突 | 单记录 CAS，禁止盲目读新版本后重放旧意图；不引入跨记录事务和通用 reducer |
| 生命周期 | 复用当前 Context、view、surface、插件实例及关闭保留规则，在既有提交点同步更新 StateHub |
| 依赖 | Rust 标准库和已有 serde_json/Tauri；不增加 Vuex、数据库、消息中间件或生产依赖 |

两项协议细化同步回功能方案：初始快照需要确认完成后再投递增量；大变化批次不能因为分帧而提前推进修订号。两者用于保证已经承诺的“不漏状态”。

## 2. 当前代码约束

| 当前落点 | 核对结果与实施约束 |
| --- | --- |
| [core.rs](../src-tauri/src/core.rs) | AppCore 已集中持有服务，`event` 经 listener 交给桌面；`changed/workspace_changed` 会触发界面协调，不能每次状态写入都调用它们 |
| [plugins/mod.rs](../src-tauri/src/plugins/mod.rs)、[process.rs](../src-tauri/src/plugins/process.rs) | `Instance::request_observed` 同步等待；超时请求仍留在 supervisor 的 requests 表直到实际回应或进程结束。可重建状态的通知需要独立到期回收规则，不能改变保存/释放的真实结果跟踪 |
| [plugin-client](../crates/plugin-client/src/lib.rs) | 四个 handler 线程，核心来信按请求回应；不能直接使用无 ID 通知，也不能在 handler 内等待订阅消费结束 |
| [resource_owner.rs](../src-tauri/src/resource_owner.rs) | Owner 已包含 plugin/instance/view/Context；现有 admission 拒绝关闭中的调用，不能直接拿它处理允许在关闭等待中执行的读取、续订和释放 |
| [component_ui.rs](../src-tauri/src/component_ui.rs)、[host.rs](../src-tauri/src/component_ui/host.rs) | 已有统一 `component_ui` 命令、真实窗口与 surface 校验；在这里增加 `state` 操作，不新增任意插件调用入口 |
| [layout.rs](../src-tauri/src/component_ui/layout.rs) | 活动中央页面在 open/select/locate/remove_views 等路径改变；一个点击处理器不足以覆盖全部变化 |
| [connections/admission.rs](../src-tauri/src/connections/admission.rs) | 重连在锁内递增 generation，并对原 view 执行 rebind；StateHub 必须在同一提交边界更换 Context |
| [connections/mod.rs](../src-tauri/src/connections/mod.rs) | 连接任务结束时会先发布工作区断线；需在这个边界冻结旧共享事实，不只监听前端事件 |
| [components.ts](../src/components.ts) | 普通 `event` 会走 `ui.event` 并刷新工作台；共享状态不能用高频普通 event 代替。关闭期间存在 deferred 队列，ack/续订/释放需要单独允许通过 |
| [终端构建](../plugins/terminal/build.mjs) | `ui-src/` 才是源码，`ui/` 被 .gitignore 忽略；不能只修改生成文件 |
| [文件树](../plugins/files/ui/tree.js) | `locate/reveal/load` 多次 await 后会修改展开、选中、根目录和滚动；需要给全过程加导航有效性检查 |
| [系统状态调度](../plugins/system-status/src/scheduler.rs) | take_work/complete 已分离 RPC 与状态锁，有四个 worker；共享发布应加入有界工作计划，不在 complete 的锁内调用 RPC |
| [挂载采集](../plugins/system-status/src/collectors.rs) | Linux 当前只返回经过类型筛选的路径；Windows 目前主要在 capacity 读取固定卷；不能直接把这两份内部数据视为完整挂载拓扑 |
| [remote_files.rs](../src-tauri/src/remote_files.rs) | 现有 stat 用服务端 realpath 规范化，并没有完整的原生 Windows/UNC 到文件浏览视角转换 API；需要明确新增能力及不支持的结果 |
| [workspaces.rs](../src-tauri/src/workspaces.rs) | 当前会复用相同服务器的已有工作区；协议隔离测试可构造同 serverId 的不同 Context，不要求修改产品的工作区创建策略 |

## 3. 文件修改地图

新增文件只在对应实现阶段创建，不预先生成空目录、接口层或通用框架。

| 文件 | 代码责任 |
| --- | --- |
| 新增 `src-tauri/src/shared_state.rs` | 记录模型、CAS、范围状态、分页快照、订阅与批次、预算、清理，以及 AppCore 状态请求门面；单元测试随模块 |
| 新增 `src-tauri/src/shared_state/delivery.rs` | 单个分发循环，处理唤醒、RPC 回执、页面 ACK、有限重试和租约；业务提交不等待 IO |
| `src-tauri/src/core.rs`、`lib.rs` | 注册服务、装配、启动分发器、退出清理；不加入持久化 store 或公开总快照 |
| `src-tauri/src/plugins/dispatch.rs` | `state.*` 路由与结构化错误 |
| `src-tauri/src/plugins/mod.rs`、`process.rs` | 非阻塞 enqueue 入口、状态通知到期回收、实例激活/停止 hooks、capability |
| `src-tauri/src/component_ui.rs`、`component_ui/host.rs` | 页面 `state` 请求、真实 surface/readiness/归属校验、结构化状态返回 |
| `component_ui/layout.rs`、`workspaces.rs`、`connections/admission.rs`、`connections/mod.rs` | 活动页面事实、view 清理、Context 建立/替换、断线及移窗接续 |
| `lifecycle.rs`、`lifecycle/close.rs`、`lifecycle/host.rs` | frozen/resume、实际 view/scope 释放、宿主丢失；不另建关闭状态机 |
| `src-tauri/src/main.rs` | 对共享页面批次按真实 windowId 定向 emit，沿用当前 listener；无新发布或全局广播策略 |
| 新增 `src/component-state.ts`，修改 `src/components.ts` | 页面请求适配、定向批次转发、ACK、挂载变更及销毁，不保存业务权威值 |
| 新增 `crates/plugin-client/src/shared_state.rs` | 基于现有 Client 的条件发布辅助、订阅收件箱、批次拼装、初始确认、续订和恢复 |
| 新增 `plugins/shared/state.js` | 供终端和文件共同使用的页面 SDK，返回 Promise/订阅 disposer；复用既有 bridge，不复制整套 bridge |
| `plugins/terminal/src/main.rs`、`ui-src/view.js`，新增 `ui-src/location.js` | PTY 初始 Context、目录报告解析、输出追赶隔离、条件发布 |
| `plugins/system-status/src/main.rs`、`scheduler.rs`、`collectors.rs`、`metrics.rs`、`scripts/windows.ps1`，新增 `src/shared_state.rs` | 可选能力、挂载模型、采集覆盖、增量发布计划和失败恢复 |
| `plugins/files/ui/view.js`、`tree.js`、`style.css`，新增 `ui/shared-state.js` | 消费四个主题、来源选择、跟随控制、挂载/容量展示、精准更新 |
| `plugins/files/release.json`，`src-tauri/src/settings.rs`、`main.rs` 和现有 settings 页面桥 | 跟随偏好及本页面关联设置的保存入口，不新增第二份共享数据缓存 |
| `src-tauri/src/remote_files.rs`，新增 `remote_files/location.rs` | 通用远程位置解析与文件服务验证，用于终端位置和挂载路径 |
| `plugins/terminal/build.mjs`、`scripts/package-plugin.mjs`、相关 UI HTML | 把公共 state.js 随需要它的插件打包；不依赖核心安装目录 |
| 核心/插件测试、`scripts/test-*.mjs`、`scripts/select-checks.mjs` | 接入协议、页面、真实组合及 CI 选择测试，详见第 12 节 |

核心不依赖 `sodeck-plugin-client` 或插件业务 crate。SDK 当前只有 serde_json，辅助类型使用手工字段检查和 Value，不为 derive 增加新依赖；公开线协议最终维护在 `docs/plugin-api.md`，fixture 校验 Rust 与 JS 两种客户端的一致性。

## 4. 数据结构与状态归属

以下是结构示意，省略 serde 标注和普通方法体。协议数字的转换集中实现，不能到处使用 `as_f64()`。

```rust
// 新增；Context 继续使用现有完整结构。
pub(crate) struct StateHub {
    inner: Mutex<Store>,
    wake: SyncSender<()>, // 容量 1，只合并唤醒信号
}

struct Store {
    scopes: HashMap<String, Scope>, // workspaceId -> 当前 Context
    active_instances: HashSet<String>,
    subscriptions: HashMap<String, Subscription>,
    snapshots: HashMap<String, Snapshot>,
    retained_bytes: usize,
    transient_bytes: usize,
}

struct Scope {
    context: Context,
    connected: bool,
    frozen: bool,
    revision: u64,
    records: BTreeMap<RecordKey, VersionedRecord>,
}

enum Producer {
    Core,
    Plugin { plugin_id: String, plugin_instance_id: String, view_id: String },
}

struct RecordKey {
    topic: String,
    producer: Producer,
    key: String,
}

struct VersionedRecord {
    revision: u64,
    received_at: u64,
    value: Option<Arc<Value>>, // None 为删除标记；JSON null 仍是 Some(Null)
    encoded_bytes: usize,
}

enum DeliveryTarget {
    Process { instance_id: String, view_id: String },
    Surface { window_id: String, surface_id: String },
}
```

`RecordKey/Producer` 实现排序与相等比较。Scope 用 workspaceId 查找后比较完整 Context，不必为了 map key 改造既有 Context。

record、snapshot、批次引用不可变值；使用 Arc 避免为每个订阅克隆全部 JSON。所有仍被快照/批次引用的旧值都计入预算，不因当前记录被替换就停止计费；引用索引、墓碑和编码缓冲也有上限。

`connected` 与 `frozen` 分开保存：工作区可能在等待关闭决定时又断线，取消关闭只清除 frozen，不能误改为在线。对外可返回派生 state，同时保留两个布尔值。

业务值中的 observedAt 由生产者使用本机 UTC 毫秒生成，staleAfterMs 为该业务的有效期；业务时间不能从远端时钟或 receivedAt 推断。核心附加接收时间，租约和快照期限使用 Instant。数值超界、远未来时间或未知字段形状由消费契约拒绝，不把异常值当最新观测。

### 4.1 订阅与快照最小状态

Subscription 保存 owner、target、filter、clientSubscriptionId、Context、phase、confirmedRevision、leaseDeadline、初始快照引用、pendingByKey、可选 activeBatch。phase 为 `initializing/live/resetRequired`，终止时从注册表移除。

Snapshot 保存固定 Context、范围状态、revision、排序后的记录引用、cursor 位置和 idleDeadline。快照只冻结某一时刻，不会跟随新写入改变。快照来自订阅初始化或一次 list；get 不建立快照。

activeBatch 冻结 `fromRevision/toRevision` 与该区间记录的最终值，分成若干帧；pendingByKey 继续接收之后的新变化。该设计只保留当前投影与有限待送变化，不保存历史事件日志。

## 5. RPC 和页面协议

### 5.1 能力发现

initialize 的 capabilities 新增 `sharedState.v1`；后续位置适配新增 `files.location.v1`。插件将共享能力记为可选，不加入原有强制能力检查列表。只有实际订阅才接收 `state.changed`，旧插件不需要新增初始化回执字段。

页面 mount.capabilities 增加 `sharedState: 1`，只在当前核心支持时提供。新页面在旧核心上禁用这两项增强，保留原有工作。控制帧仍受现有 64 KiB 限制。

### 5.2 请求与响应

进程公共参数为 `viewId/expectedContext`；surface 请求不接受自报的 owner，而由真实 window、surface 推导。过滤器至多 8 个明确 topic，可选 producer 和 key，不支持正则和任意查询语言。

| 方法 | 业务参数 | 结果与行为 |
| --- | --- | --- |
| `state.get` | `{topic,key,producer?}`，省略 producer 表示自己 | `{scope,record:{producer,topic,key,revision,present,value?,receivedAt?}}`；从未存在 revision=0 |
| `state.put` | `{topic,key,expectedRevision,value}` | `{revision,changed}`；写入自己的 producer，不接受来源覆盖 |
| `state.remove` | `{topic,key,expectedRevision}` | `{revision,changed}`；删除标记保留版本，相同当前墓碑重复删除为无变化 |
| `state.list` | 首次 `{filter}`，后续 `{cursor}`；可带 `{cursor,release:true}` | 固定快照 `{snapshotId,scope,revision,records,nextCursor}`；显式释放幂等 |
| `state.subscribe` | 初次 `{clientSubscriptionId,filter}` | `{subscriptionId,snapshotId,scope,revision,records,nextCursor,leaseMs}` |
| `state.subscribe` | 续次 `{clientSubscriptionId,filter,acknowledgedRevision}` | 确认初始快照或续订；返回 `{subscriptionId,confirmedRevision,leaseMs}` |
| `state.unsubscribe` | `{subscriptionId}` 或本 owner 的 `{clientSubscriptionId}` | 幂等释放订阅、快照和批次；初次响应超时也能按客户端 ID 取消 |

get 可读到删除标记的版本，list 和初始订阅只返回当前存在记录。未知 topic 是空结果，不是方法不存在。业务 value 可为 JSON null，不能与缺失混淆。

游标是核心生成的不透明标识，绑定 owner、Context、原过滤器和页位置；禁止调用者修改游标偏移或换过滤器翻页。同一个游标允许在有效期内重读同一页。最后一页响应丢失后仍可重读：订阅快照在客户端确认 R 后释放；一次 list 的快照在显式释放或空闲 30 秒后释放。

初次 subscribe 重试相同 clientSubscriptionId 返回同一订阅和仍有效的初始快照。已完成或失效的初始化不重新生成快照，返回明确状态；SDK 新建 ID 重同步。订阅初始化期间也有 60 秒租约；分页空闲超时或初始化不能按时完成时整次重建。

### 5.3 变化批次

```json
{
  "subscriptionId": "sub-42",
  "clientSubscriptionId": "files-browser-1",
  "context": {"workspaceId":"w1","serverId":"s1","connectionId":"c1","generation":1},
  "batchId": "batch-18",
  "fromRevision": "100",
  "toRevision": "140",
  "part": 0,
  "parts": 2,
  "changes": [
    {"op":"put","record":{"topic":"filesystem.capacity.v1","key":"mount-1","producer":{"kind":"plugin","pluginId":"sodeck.system-status","pluginInstanceId":"p1","viewId":"v1"},"revision":"125","receivedAt":1794153600000,"value":{"mountId":"mount-1","totalBytes":"1000000000","availableBytes":"600000000","observedAt":1794153600000,"staleAfterMs":120000}}}
  ]
}
```

此处 Context/时间为示例。change 另有 `remove`（含完整记录身份、revision、reason）和 `scope`（connected/frozen、reason）。删除后又创建可合并为最终 put；同一批内不要求回放所有中间状态。

核心 → 进程使用带 ID 的 `state.changed` 请求。回执包含 `{batchId,part,accepted:true,appliedRevision?}`。中间帧只确认已进入有界暂存区，最后一帧拼装并更新本地投影后才返回 appliedRevision=toRevision。业务回调在投影提交后异步处理，ACK 不等待 UI 绘制或远程目录请求。

核心 → 页面使用定向 `component-state-changed`，宿主转为 `sodeck:stateChanged`。页面以同一个状态桥的 `op: "ack"` 回应 batchId/part/appliedRevision，核心校验实际 surface；`ack` 是页面传输操作，不新增插件业务 RPC。

重复帧核对 batchId、part 和内容后重回执；乱序、缺帧、Context 改变、batch 超限或 fromRevision 不匹配则丢弃暂存并重新订阅。消费者不能在收到第一帧后就把本地游标跳到 toRevision。

### 5.4 SDK 使用形状

以下是待实现的页面 SDK 用法，watch 返回时已经完成初始快照确认，随后 onChange 接收最新只读投影：

```js
const shared = createSharedState(sodeck, mount);
const stop = await shared.watch({
  topics: [
    'workspace.presentation.v1',
    'terminal.location.v1',
    'filesystem.mounts.v1',
    'filesystem.capacity.v1'
  ]
}, projection => updateFileEnhancements(projection));

const current = await shared.get({topic: 'terminal.location.v1', key: 'current'});
await shared.put({
  topic: 'terminal.location.v1', key: 'current',
  expectedRevision: current.record.revision,
  value: verifiedLocation
});
// unmount/contextChanged: stop(); shared.dispose();
```

SDK 不自动替业务解决 revisionConflict。提供结构化错误和当前 revision；生产者重新确认最新业务意图后再提交。Rust SDK 使用相同语义，不引入独立权威 store；镜像只用于当前订阅投影。

### 5.5 错误透传

后端沿用 JSON-RPC `error.data.code`，至少区分 invalidParams、staleContext、workspaceClosing、sourceStopped、revisionConflict、resourceLimit、snapshotExpired、subscriptionExpired、resetRequired、scopeClosed。revisionConflict 返回当前 revision/present，不额外塞入完整值。

现有 component_ui 返回 String 错误，不改所有既有操作的返回类型。新增 `state` 分支返回 `{result}` 或 `{error:{code,message,data?}}` 的内层信封，由 component-state.ts/state.js 解包；旧 bridge 的普通错误行为保留。

## 6. 核心算法、锁与资源约束

### 6.1 提交边界

新增代码的锁顺序固定为：`workspaces → component_ui → shared_state`。只需要其中部分锁时可以取后缀，禁止持有 shared_state 再反向获取其他核心锁。实例查找、输入解析和大小预检在进入这组锁前完成；真实存储预算与编码后的最终大小仍在提交边界核对。

进程写入伪代码：

```text
parse request; validate topic/key/value/frame bound
lock workspaces; require exact Context and connected
lock component_ui; require live own view and non-frozen lifecycle
lock shared_state; require active instance and exact active Scope
compare expectedRevision; validate current budget including retained history references
if equal value: return unchanged
assign next revision; replace one immutable record
coalesce matching subscription changes, or mark subscription resetRequired
unlock all; wake dispatcher; return committed revision
```

scope 只能由连接接纳入口建立，active instance 只能由 supervisor 初始化成功入口登记。请求不能懒创建 scope 或复活 producer。这样插件停止时在同一 hub 锁内撤销实例并移除贡献，之前已提交的写入会被清理，之后的排队请求会被拒绝。

页面写入还在持有 component_ui 锁时检查 surface 所属窗口、ready、逻辑 view、当前工作区窗口归属，以及捕获的 expectedContext。初始挂载尚未 ready、旧窗口已完成交接或 view 已被 rebind，均不得提交；不能只在 components.ts 发送之前检查一次。

读取和续订允许 frozen/disconnected；新订阅允许读取断线历史，但关闭准备后拒绝新建。ack/unsubscribe 和已有续订允许通过关闭等待，不刷新业务值。关闭清理开始后，待删 view 的订阅被撤销，不因续订延长资源释放。

### 6.2 快照与变化不漏项

1. subscribe 在同一 hub 临界区建立过滤器、基准修订 R、快照引用和 initializing 订阅。
2. R 后的匹配变化进入 pendingByKey，初始化期间不投递；超限标记 resetRequired。
3. 客户端收齐快照，调用 subscribe 携带 acknowledgedRevision=R；核对 Context、租约和初始化状态后切到 live，释放快照。
4. 分发器在锁内冻结当前 pendingByKey 和 frontier T 为一个批次，from=R、to=T；之后新写入进入新的 pendingByKey。
5. 已冻结批次按最终编码大小拆帧。每个订阅最多一帧等待回执，所有帧完成后才能 confirmedRevision=T，再发送下一批。

Core 来源的活动页面变化与对应 view 移除在一个 hub 临界区生成，避免文件端看到“已选择不存在的页面”的稳定投影。终端目录仍由插件独立发布，允许短暂没有对应目录，此时停止跟随。

对于无匹配变化的全局修订，不发送空批次；下一批从订阅自己的 confirmedRevision 开始覆盖到新 frontier，不要求数字连续加一。

### 6.3 CAS 与删除

删除不把 key 恢复为 revision=0，而是保留新修订号的墓碑。`get` 返回它，后续重建必须匹配墓碑版本。来源/view/Context 真正释放时才一起丢弃墓碑，旧来源也已不再通过 admission。

put/remove 超时后 SDK 读取当前记录。原版本仍在才允许重试同一意图；已变化则报告冲突。生产者只保留最新待提交目标，不允许一个旧采集结果在重试过程中自动借用新 revision。

### 6.4 预算与清理常量

沿用功能设计的初始预算，集中在一个 limits 常量区域：记录完整编码 16 KiB；RPC 批次帧 32 KiB；页面批次含外层消息 24 KiB；Context 当前数据 4 MiB；全局保留数据 32 MiB；快照/待发旧值/编码缓存另有全局 32 MiB 临时预算；Context 记录与墓碑共 4096 项；每实例订阅 32 个；单订阅 activeBatch 与 pending 合计 1 MiB。

元数据也限长：topic 128 ASCII 字节、key 512 UTF-8 字节、客户端订阅 ID 128 字节；JSON 嵌套深度最多 32。租约 60 秒、30 秒续订，快照空闲 30 秒。常量不足时返回明确错误，不静默截断或 LRU 删除仍有效的事实。

若订阅超过积压预算，立即丢弃其数据队列和未完成批次，只保留小型 resetRequired 控制状态。控制状态及来源删除必须有保留额度，不能因为大值占满预算而无法宣布失效。超限与事件丢弃不回滚已经成功的业务写入。

分发器定时回收租约、游标和已结束目标，同时在 unsubscribe、移窗提交、view 释放、插件停止、Context 替换时主动清理。测试读取计数与预算，不依赖生产日志推测是否已释放。

## 7. RPC 分发与页面通路

### 7.1 不阻塞的核心分发器

把现有 Instance 的“编码、try_send、返回 Receiver”提取为内部 `enqueue_request`，原 request/request_observed 继续在其上同步等待，不改变调用者行为。新增内部回收策略 `RetainUntilReplyOrExit` 和 `ExpireAtDeadline`，默认前者；只有可重建的 state.changed 使用后者。

process.rs 的 pending 请求从 tuple 改为命名结构以保存回收策略。到期的 state.changed 移出 requests 表，迟到回包因未知 ID 被忽略；保存、disposeView 等仍继续保留真实回执。状态通知不得携带原生资源 Task，也不得计入 view.calls 的保存/关闭等待。

一个 delivery 循环持有 Weak<AppCore>，使用容量 1 的 wake 和最近到期时间等待，不为每条记录或每个订阅创建线程。循环在短锁内取出发送计划，锁外调用 enqueue_request 或 core.event，再轮询有界 Receiver 并在锁内提交 ACK。每实例最多 4 个状态帧在途，全局最多 64 个，按订阅轮转；保证 stop/正常 RPC 的现有队列优先级。

单帧等待沿用 5 秒上限，可按 100/500 ms 退避重试至多两次，复用同一 batch/part，不能复用 JSON-RPC request ID。仍未确认则 reset/终止该订阅。客户端租约失效即宣布本地投影不可用；不会依赖下一次业务变化才察觉断流。

SDK 的 state.changed handler 必须在业务 viewId 解析和业务锁之前分派；只验证、拼装及提交本地投影并回应，远程操作和渲染在 handler 返回之后执行。四个 handler 同时忙碌也不应阻止核心提交状态，因为核心不等待消费 ACK 才返回 put。

### 7.2 页面桥只做路由

新增 component-state.ts，持有每 surface 的请求/订阅登记和投递代次，不持有通用业务数据镜像。

页面 `request('state', input)` → components.ts 捕获当前 token/Context → `component_ui(operation:'state')` → StateHub。消息返程同时核对捕获的 token/Context，旧 Promise 不能投递给重新挂载的同名 view。

core.event 发 `component-state-changed`，包含核心生成的 windowId/surfaceId；main.rs 的 listener 对这一事件使用 `emit_to(windowId, ...)`，其他事件沿用原行为。宿主核对已登记 surface/订阅后 postMessage 给目标 iframe；windowId 不由页面提供。

ack、已有续订、unsubscribe 在 closeBlocked 分支前处理，且仍接受后端校验；put/remove 不进入 deferred 队列，冻结时立即返回 workspaceClosing。这样取消关闭后不会自动重放旧 cwd 写入。状态请求也不调用 refresh 或创建页面。

目标窗口准备移入时可先显示原组件，但不建立可写状态通路。移窗正式提交后用现有归属刷新建立目标订阅并取得快照；旧 surface 的订阅在提交点释放。移窗取消保留源订阅并清理目标临时对象。

state.js 负责快照/批次、租约和可销毁监听。隐藏页面继续接收状态并 ACK，只暂停渲染、保留最新待显示状态；如果 WebView 被挂起导致租约过期，恢复可见时必须重同步。宿主可以代为续订当前仍登记的有效 surface，但只能转发页面真实发出的 ACK，不能因 postMessage 成功就认定页面已经消费批次。

## 8. 生命周期调用点

| 现有入口 | 加入的同步动作 | 结束条件 |
| --- | --- | --- |
| `process.rs` initialize 成功、允许 Running 的提交 | `activate_instance(instanceId)`，先登记再允许状态请求 | 相同实例登记幂等 |
| supervisor 开始停止、通信故障和异常退出分支 | `stop_instance(instanceId)`，撤销来源与订阅；真实进程清理仍按原机制等待 | 无该实例贡献，旧 worker 不可重新注册 |
| `connections/admission.rs::adopt_connection` | 持有 workspaces/ui 锁时建立 scope；重连用空 Scope 替换旧 Context | 老快照、订阅及记录全部退出活动集合 |
| `connections/mod.rs` 当前 disconnected 提交点 | `set_connected(context,false)`，发 scope 变化 | 已断线数据不再引发联动 |
| layout 的 open/select/locate/remove_views、reconcile_ui | 在已持有 ui 锁的提交尾部调用 `sync_presentation_locked` | 仅 editor 真变化才写核心记录 |
| `lifecycle.rs::close_prepare_internal`、`close.rs::close_mutate` 和宿主丢失路径 | 同一 ui 临界区按当前生命周期推导 frozen；取消时同步清除 | 不靠 UI 后续刷新才冻结/解冻 |
| `close.rs` 确认清理逻辑 view 的 remove_views | 同一 ui→hub 临界区 remove_view，移除记录/快照/订阅，更新替补 editor | view 删除与状态删除一致 |
| 工作区/窗口/app finalizing | 在原工作区移除边界 remove_scope；全局结束停止分发器并清空预算 | 无活动 scope/订阅/快照 |
| `workspaces.rs::commit_workspace_move` 和 surface 替换/宿主丢失 | 校验并释放旧 surface 订阅，保留逻辑记录 | 只有当前窗口的页面可写/ACK |

新增少量持锁 helper 接受当前 Runtime 引用，避免 helper 内重新获取同一 Mutex。禁止“先复制所有 view，释放锁，过一会儿拿副本清理”的竞态；workspace_changed 可以做一致性检查和唤醒，但不能作为唯一生命周期提交入口。

ScopeClosed/SourceStopped 的最终通知允许有限送达尝试，不阻止真实资源清理；读者最终由事件或租约发现失效。允许被关闭的订阅直接结束，没有接收端时不保留无限期的告别消息。

## 9. 三个插件的具体改造

### 9.1 系统状态插件：把已接受观测发布出去

新增插件私有 shared_state.rs，包含公开挂载投影、按记录 CAS 的待发布目标和 reconcile 逻辑。App 增加共享能力开关，各 View 增加 source revision 缓存、最新 desired records 和同步状态。这里的缓存是生产者的待提交目标，不能供文件插件直接调用。

在 scheduler.complete 成功接受 topology/capacity 结果后生成 dirty keys；pause、错误、连接切换更新 scan-status。新增工作类型 `SharedGet/SharedPut/SharedRemove`，继续通过 take_work → worker RPC → complete 处理，锁内不等待 Client.call。优先保持 exec poll/cancel/release，再轮转共享发布，禁止一次 512 条清单挤满控制队列。

每 view 最多一个共享写在途，队列按 key 合并。完成结果必须核对 view epoch、expectedContext、目标值代次；旧结果只更新必要的已知服务端版本，不把旧目标重新放回发布队列。version conflict/未知提交结果先 get，再决定最新目标；关闭冻结保留 dirty，恢复后重试最新观测。发布退避有上限，不增加远程采样次数。

采集数据调整：

- Linux 从完整 mountinfo 解析 mount ID、挂载路径、设备、文件系统、只读选项和文件系统视角。原有“适合采容量的路径白名单”继续服务 capacity 调度，与完整拓扑分开；遇到截断/解析缺项标为 partial。
- Windows 在 topology 读取可获得的卷身份和挂载路径，保留 CIM 单项数组边界；capacity 关联同一卷身份。缺少无盘符挂载目录或网络卷时描述覆盖范围，不能将当前 DriveType=3 查询称为完整全系统拓扑。
- macOS/Linux/BSD 采集分支分别产出已验证字段；现有 df 可提供部分容量事实，不凭 df 推断只读属性或完整挂载拓扑。尚未支持的字段为 null，不能在客户端 OS 上补猜。
- mountId 组合来源可用的挂载身份和本 Context 的观测代次；发现身份变化就生成新 ID 并丢弃旧容量。采样间隔内发生又恢复的不可观测变化无法保证识别，实施记录必须保留这项限制。

公开 topology/capacity 每条自带观测时间。scan-status 先写 publishing，再提交 puts/removes，全部完成后写 synced；coverage 描述远端采集覆盖，publishState 描述共享副本是否同步，两个字段独立。只在完整扫描或明确卸载证据下删除；部分扫描没有某项时仅允许其过期。

### 9.2 终端插件：目录报告与输出恢复

main.rs 的终端记录从二元组改为命名 TerminalView，增加 openedContext；重复 mount 返回同一 PTY 及其最初 Context。重连后仍保留的旧终端页面只展示历史，不因宿主 Context 被 rebind 就重新获得目录发布资格。

新增 ui-src/location.js，负责注册 xterm OSC handler、解析路径报告、聚合一个输出块内的最后位置，以及向 state.js 提交。ui-src/view.js 在 write 前设置当前帧种类/原始输出 offset，await terminal.write 后将报告交给 location 模块，不能使用解析回调发生时尚未更新的全局 offset。

规则与调用点：

1. `snapshot/reset` 重建终端屏幕时禁用业务报告，恢复画面中的控制序列不能重新发布 cwd。
2. 原始 data 追赶期间只暂存最新有效报告；caughtUp、实际 surface ready、当前窗口归属均满足后发布一次。
3. 每条业务值附 terminalId 和原始输出 offset，移窗读取已有 current 后，不能用较早 offset 的报告覆盖它。相同报告重放不刷新 observedAt。
4. 记录里的路径可用性与 stream end/Context 变化绑定；检测到 end、会话视角不明或输出缺口时发布 unknown，或在已失去 admission 时由核心范围失效处理。
5. 隐藏仅停止展示，不清除逻辑记录；待定位数据在再次可见且追赶完成前不使用。surface 销毁清理 handler、计时器和 Promise；不会删除同 view 的逻辑状态。

首批实现严格 OSC 7 解码：只接受支持的 file URI、有效百分号编码、有限长绝对路径，拒绝 NUL、控制字符、无效主机/路径组合。远端主机名不能拿服务器显示名称替代。报告无法证明来自当前文件浏览视角时保留 unknown，不能仅靠 cwd 字符串就宣称对应当前服务器。

目录来源验证与通用状态服务分开验收：使用预先配置的可靠 shell 报告作为首批接入；真实 bash/zsh/PowerShell 各自记录已验证和未支持范围。自动安装 shell 集成涉及启动行为，首批不把向 PTY 注入命令或改写远端 rc 文件当作隐式实现。若目标要求所有默认 shell 开箱即用，必须补齐独立 shell 集成方案后再宣称该覆盖已经完成。

### 9.3 文件组件：订阅与可取消定位

新增 ui/shared-state.js，以一份 watch 订阅四个首批主题。文件编辑器不订阅，只有 files 侧栏需要；关闭跟随可继续订阅挂载增强。

投影控制器维护 activeEditorViewId、选定挂载来源、当前目录候选和已处理的位置。offset 用于拒绝较旧报告，不作为“目录改变”的依据；同一终端同一路径再次报告、续订、主题刷新均不能触发回跳。切换活动终端、从不可用恢复为可用或重新开启跟随时允许定位当前路径一次。只使用 Core 来源的 presentation；不会用其他插件的同名主题冒充宿主选择。选定来源使用稳定 pluginId 作为偏好，当前 pluginInstanceId/viewId 从新快照重新绑定；多个提供者时沿用功能方案的明确选择，不按最后更新时间抢占。

跟随状态机：

```text
范围不可用 / 非终端 / 目录未知 / 开关关闭 -> 取消当前导航，保留文件位置
可见且目标改变 -> followLocation(candidate)
隐藏 -> 保存最新候选，取消当前导航；显示并确认订阅有效后只定位最后目标
手动导航 -> navigationEpoch++，取消正在应用的跟随，记住当前报告已处理
重新开启跟随 -> 强制处理当前有效候选一次
```

tree.js 增加 `navigationEpoch` 和 `contextEpoch`；locate 改为 options 参数区分 manual/follow，但所有原调用者继续经过同一定位实现。每次 await 返回后、每次改变根目录/expanded/selection/scroll 前检查 captured epoch。

必要调整是把 load 的结果合并与跟随副作用分开：同一 Context 的旧读结果可以安全补充节点数据，但不能因旧跟随继续展开/选中；跨 Context 返回的节点和目录游标全部丢弃并清理。已发出的底层读取无需强制终止整个文件会话，取消指取消该导航的后续应用。用户重命名/上传等文件任务继续使用原机制。

对当前显示的挂载节点，仅修改标识和详情文本，不调用 tree.start 或全树 render；使用按路径定位的 DOM 节点和 textContent，保留 selection/scroll。tree.setMounts 保存当前展示映射，普通 render 创建的新行也调用同一 decorateRow，因此后续展开不会丢失标识。容量用 BigInt/十进制字符串计算显示，缺失显示未知；topology 与 capacity 的时间分别计算。时间过期只更新展示，不再扫描远端。

在 files/release.json 的 browser 页增加 `followTerminal` boolean，默认 true，apply=live。侧栏开关沿用 `request('settings', ...)`，新增 `op:'save'`，输入复用现有 `{requestId,definitionToken,baseRevision,values}`；不能把 sharedState 的 expectedRevision 字段误用到设置 API。

新增 `save_settings_for_surface` 门面及 Tauri 命令，从真实 surface 和 viewType 解析唯一关联设置页，忽略调用方自报的 pluginId/pageId，再进入现有设置保存逻辑。已接受的保存登记到所属 view 的调用生命周期，关闭等待和重复 requestId 沿用当前规则；不得持有 shared_state 锁调用设置服务。现有 settings 页面桥无需另建状态缓存，文件页面从已有设置快照合并 followTerminal 并提交；冲突按现有 persisted/conflict/snapshot 返回处理。文件进程继续接收 settings.changed，不增加私有设置 RPC。

## 10. 远程位置到文件路径的通用适配

新增可选 `files.location.v1`，在现有 `files_data` 控制操作中增加 `resolveLocation`，输入为 `{path,pathStyle,filesystemView}`，沿用当前文件 handle、surface 和 Context，不接受另一个插件的凭据或连接标识。

remote_files/location.rs 的责任只有通用位置解析：

- posix 绝对路径按远端规则处理，保留 Unicode；不使用本机 PathBuf 做规范化。
- windows 盘符路径按远端文件适配的已验证表示转换，再使用当前文件服务 stat/realpath 确认；不能看客户端 OS 决定转换规则。
- UNC、chroot、容器及未知 filesystemView 没有经过验证的映射时返回 unsupportedLocation，不做主机名或路径前缀猜测。
- 返回 `{path,attrs}` 或结构化 location 错误；调用结束前及应用结果前检查 Context，转换成功不代表之后读写一定成功。

终端跟随在定位前调用一次；挂载路径在拓扑变化后按需解析，并按来源/mountId/Context 缓存映射，容量变化不重复解析。挂载解析只使用有界并发（首批 2 个）、优先可见/已展开节点，不为标注 512 条挂载启动 512 个同时 stat。

files 插件只消费规范化结果，不直接依赖 SFTP 细节。旧核心缺少位置能力时，对不能证明兼容的输入禁用自动定位，保留普通文件浏览。

## 11. 实现与验收边界

共享存储、进程 RPC、页面订阅、两个业务联动和生命周期作为一次完整实现交付。以下划分用于检查覆盖，不作为停工或分别交付的边界。

| 范围 | 代码 | 验收行为 |
| --- | --- | --- |
| 内存状态契约 | shared_state.rs、类型、CAS、范围状态、分页、批次、限额 | 确定性测试覆盖当前值、无空档订阅、删除、防复活和有界内存 |
| 进程通信与生命周期 | RPC、SDK、通知到期、连接/关闭/view/实例 hooks | 两个真实测试进程发布/订阅，退出后无残留；原保存/关闭测试通过 |
| 页面消费链 | component-state.ts、state.js、surface 命令与定向发送、打包 | 页面晚打开、隐藏、移窗、销毁、ACK 失步正确接续 |
| 挂载增强 | 规范挂载投影、系统发布计划、路径适配、文件展示 | 真实采集 → 核心 → 文件树；完整/部分/失败/过期/卸载符合设计 |
| 终端跟随 | presentation、目录解析、PTY Context、文件导航取消、设置 | 多终端、手动干预、恢复；至少一个真实可靠 shell 端到端通过 |
| 组合与交付 | 兼容降级、独立包、平台检查、文档 | 明确实际通过及未支持范围；核心与插件独立发布，无业务插件构建前置 |

方案与实现阶段不修改版本；实际发布按既有约定分别更新核心及消费能力的插件，旧组合保持独立功能。本次发行来源和验收见[核心 0.13.0](releases/v0.13.0.md)。

## 12. 验收代码与执行入口

### 12.1 核心与客户端测试

核心新增 `shared_state` 模块测试，用注入 now、可控回执和屏障安排竞态，不能通过长时间 sleep 碰概率。

| 用例组 | 要证明的反例 |
| --- | --- |
| CAS 与墓碑 | 同版本双写只有一个成功；删除后旧 create 不复活；null 与不存在不同；超时重试不覆盖新值 |
| 初始快照 | 写入发生在建立订阅、读取第二页、确认 R 的不同阶段都不会漏；最后一页响应丢失可重读 |
| 大批次 | 512 条数据分多帧；第一帧不能推进 toRevision；最后一帧丢失、重复 ACK、乱序和插入新写入都可恢复 |
| scope/source | 断线与关闭并存，取消只解除 frozen；旧实例 worker 和旧 Context 拒绝写入 |
| 预算与过期 | 超限保留旧值，消费者得到 reset；快照旧版本、墓碑和订阅释放后预算归零 |
| 传输保留策略 | 不回应 state.changed 的插件不会永久占满 requests；ui.save/disposeView 超时后的真实回执继续保留 |
| surface | 旧 window/token、目标未提交、rebind 后的返回值不能写入或 ACK 新订阅 |

扩展[测试插件](../src-tauri/tests/fixtures/plugin.rs)或添加仅 tests 使用的模式，实现 producer/consumer/slow-consumer，使用真实 stdin/stdout JSON-RPC；不把夹具发行。Rust SDK 与 JS SDK 使用相同 JSON 样例验证墓碑、分页、批次、错误与数字精度。

### 12.2 插件与页面测试

- 终端 location.js：OSC 跨数据块、UTF-8/百分号编码、未知主机、控制字符、snapshot 抑制、较早 offset、PTY 原 Context 与重连 Context 不同。扩展 test-terminal-input.mjs 的实际 xterm 环境，不自造另一套终端解析器。
- 文件 tree.js：用可控 Promise 让 A 在 C 后返回，验证选择/expanded/root/scroll；覆盖手动导航、关闭跟随、隐藏和 Context 切换。扩展 test-files-ui.mjs 与 test-workspace-ui.mjs。
- 系统状态：新增公开投影和发布计划测试；检查部分扫描不能删除、单批容量不能更新其他观测时间、共享 RPC 超时不加快采集、暂停/恢复及同路径身份变化。
- 组合新增 `scripts/test-component-communication.mjs`：在既有隔离 OpenSSH 和测试宿主基础上同时加载真实终端、文件、系统状态插件，验证两个联动以及缺插件、新旧能力组合。只有实现脚本后才登记 `npm run communication:test`。

新增组合入口应复用 test-openssh.mjs 的隔离服务和包输入；不把两次独立插件测试当成组合证明。需要传入实际独立包时沿用现有 `SODECK_TEST_*_PLUGIN` 约定，并覆盖文件面板隐藏、重连、取消关闭、拆窗和插件进程异常退出。

### 12.3 格式、构建与 CI

实现后执行现有 `npm run check`、`npm run plugin:check`、`npm run scripts:test`、`npm run test`、`npm run web:build`；修改的插件分别执行 `cargo test --manifest-path plugins/<名称>/Cargo.toml --locked`，客户端运行自己的 cargo fmt/test。页面和真实业务执行对应 terminal/files/system-status/settings/communication 回归。

打包保持现有 `npm run build` 与 `npm run plugin:build -- --plugin <名称>`；不要求核心构建先构建插件。state.js 在终端开发构建和各独立包路径都验证存在，使用相对包内引用；新增状态设置动作要纳入 settings:test。

修改 select-checks.mjs 及其测试：shared_state、共享 SDK、状态桥或公共资源变更选中核心、页面和三个业务插件的通信组合；纯采集字段或文件展示修改仍选择实际相关范围。新组合运行到现有 Check/remote-integration 中；只有脚本实际存在后才引用。共享客户端变更对其他插件的编译/原协议回归沿用既有扩大范围规则。

正式交付仍验证 Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 五种原生产物及启动/退出。客户端平台通过不等于所有远程 shell、文件系统视角和挂载来源通过；远程 Linux、Windows、macOS 与实际 shell 的支持表独立记录。

## 13. 文档同步与完成定义

实施时将已实现协议写入 plugin-api.md；用户开关、目录报告支持范围、挂载过期表现进入 usage.md；新增命令进入 development.md；构建和发布流程有实质变化才更新 distribution.md。实际证据新增对应 implementation 文档并登记索引，普通细节不追加根 README。

完整完成需要同时满足：共享状态机制通过共同契约测试；两个联动使用真实插件跑通；关闭、重连、移窗与错误恢复有证据；旧组件组合能运行；独立包包含全部运行资源；未支持的远端环境被明确列出。

本方案尚未解决所有 shell 的默认集成和所有远程文件系统的映射识别，已明确采用可验证来源并按能力降级。实现中不得用伪造 cwd、提示符猜测、完整性不明的扫描结果或“消息已发送”代替业务验收。

返回[功能设计](plan-component-communication.md) · [文档索引](README.md)。

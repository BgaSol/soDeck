# 远端 NetBird 客户端代码设计

日期：2026-10-18。状态：已确认并完成首版实现；实际支持条件与测试范围见 `netbird-client-implementation.md`。

本方案承接 `netbird-client-feature-design.md` 与 `netbird-client-ui-design.md`，首版范围为 Linux 上原生安装的 NetBird 客户端。Docker 客户端、网络选择、诊断包和独立窗口不进入首批实现。

## 1. 核心决策

### 1.1 建立独立的 NetBird 业务模块

不把 NetBird 状态塞进现有 `MonitorManager`，也不依赖 Docker 管理状态。新增独立的 `NetBirdManager`，仅复用以下公共能力：

- 已验证的 SSH `ConnectionContext`。
- `connection::connect_verified` 创建独立后台连接。
- Tauri Channel、窗口所有权和会话移动规则。
- `Cancellation`、应用退出收尾和 SSH 流量统计。
- 现有错误模型 `AppError`。

原因：NetBird 有独立的目标绑定、轮询频率、写操作、结果核对和“操作可能切断 SSH”的安全语义。把它放进资源监控会使资源采样、Docker 检测和 NetBird 写任务相互耦合。

### 1.2 CLI 是首版唯一适配层

首版通过远端 `netbird` CLI 查询和操作，不直接访问 daemon gRPC socket。前端不拼接命令，也不能传入任意参数。

```text
React UI
  → 固定类型的 Tauri command
  → NetBirdManager
  → 服务端构造受限命令
  → 独立 verified SSH connection
  → 远端 netbird CLI / 系统网络查询
```

CLI 路径、客户端身份和能力检测完成后生成不可伪造的绑定 token。所有写操作、日志读取、原始状态和节点检测都必须携带该 token；执行前后均重新核对绑定。

### 1.3 共享状态按 SSH 身份聚合

相同 `ConnectionContext::identity()` 的多个工作区和窗口共用：

- 一份 NetBird 快照。
- 一套轮询任务。
- 当前客户端绑定。
- 当前写操作锁。
- 最近操作结果。

前端页面状态仍按工作区保存，例如当前页面、搜索、筛选、选中节点和滚动位置。

### 1.4 写操作使用“预检 → 提交 → 核对”协议

任何写操作都不能直接从按钮调用 CLI：

```text
prepare
  检查客户端绑定
  检查当前状态
  检查 SSH 路径依赖
  生成短期 preview
        ↓ 用户确认
submit（带 requestId）
  再次核对 preview、绑定与路径
  执行固定命令
  重新查询状态
  发布最终结果或“结果待核对”
```

该流程参考现有 Docker 的 preview、request ID 和 reconcile 思路，但不直接复用 Docker 类型或 Manager。

## 2. 前端目录与文件

新增目录：

```text
src/netbird/
├── NetBirdPanel.tsx
├── NetBirdOverview.tsx
├── NetBirdPeers.tsx
├── NetBirdPeerDetail.tsx
├── NetBirdDiagnostics.tsx
├── NetBirdOperationDialog.tsx
├── NetBirdProvider.tsx
├── netbirdApi.ts
├── netbirdStore.ts
├── netbirdPresentation.ts
├── netbirdPresentation.test.ts
├── netbirdStore.test.ts
└── netbird.css
```

### 2.1 `netbirdApi.ts`

此文件是前后端契约的单一 TypeScript 来源。主要类型如下：

```ts
type NetBirdCollectionStatus =
  | "collecting"
  | "ready"
  | "stale"
  | "unsupported"
  | "unavailable"
  | "disconnected";

interface NetBirdBinding {
  token: string;
  generation: string;
  kind: "native";
  cliPath: string;
  clientVersion: string | null;
  daemonVersion: string | null;
  profile: string | null;
  capabilities: NetBirdCapabilities;
}

interface NetBirdSnapshot {
  binding: NetBirdBinding | null;
  collectionStatus: NetBirdCollectionStatus;
  updatedAt: number | null;
  stale: boolean;
  error: string | null;
  installation: NetBirdInstallationState;
  service: NetBirdServiceState;
  authentication: NetBirdAuthenticationState;
  connection: NetBirdConnectionState;
  localPeer: NetBirdLocalPeer | null;
  controlPlane: NetBirdControlPlane;
  peers: NetBirdPeer[];
  peersComplete: boolean;
}

interface NetBirdFrame {
  subscriptionId: string;
  revision: number;
  connectionIdentity: string;
  snapshot: NetBirdSnapshot;
  operations: NetBirdOperation[];
  refreshing: boolean;
}
```

安装状态、服务状态、认证状态和网络连接状态必须分开，不能只使用一个 `online: boolean`。

对端统一为宽容模型：

```ts
interface NetBirdPeer {
  id: string;
  name: string;
  fqdn: string | null;
  ipv4: string | null;
  ipv6: string | null;
  state: "connected" | "connecting" | "idle" | "unknown";
  rawState: string | null;
  connectionMode: "direct" | "relayed" | "unknown";
  latencyMs: number | null;
  latencyMeasuredAt: number | null;
  lastHandshakeAt: number | null;
  stateChangedAt: number | null;
  receivedBytes: string | null;
  sentBytes: string | null;
  networks: string[];
}
```

所有数值缺失时使用 `null`，不以 `0` 替代未知。

### 2.2 `netbirdStore.ts`

采用现有 `DockerManagementStore` 的外部 Store 方式，避免把 Channel 生命周期散落在组件中。

Store 负责：

- 建立和关闭订阅。
- 丢弃旧 epoch 的晚到事件。
- 按 `revision` 去重并发送 ack。
- 更新可见性和兴趣级别。
- 手动刷新去重。
- 保存最新 frame；后端断线时保留旧值。

建议兴趣模型：

```ts
interface NetBirdInterest {
  level: "none" | "background" | "visible";
  includePeers: boolean;
}
```

- NetBird 页面可见：`visible`，15 秒轮询。
- 工作区可见但处于监控或 Docker 页：`background`，60 秒轮询。
- 工作区隐藏或无活动窗口：`none`，暂停常规轮询。
- 有写操作时，后端独立进入短期核对，不依赖页面兴趣。

### 2.3 `NetBirdProvider.tsx`

与 `MonitorProvider`、`DockerManagementProvider` 同级，按 `sessionId` 创建 Store。Provider 始终存在，但由 Panel 报告兴趣，避免切换标签时销毁共享状态。

组件嵌套建议：

```tsx
<MonitorProvider>
  <DockerManagementProvider>
    <NetBirdProvider>
      <WorkspaceLayout />
    </NetBirdProvider>
  </DockerManagementProvider>
</MonitorProvider>
```

Provider 之间不能互相依赖。

### 2.4 `NetBirdPanel.tsx`

Panel 只管理展示路由和工作区快照，不管理 SSH Channel：

```ts
interface NetBirdPanelSnapshot {
  page: "overview" | "peers" | "peer" | "diagnostics";
  selectedPeerId: string | null;
  query: string;
  filters: {
    states: string[];
    modes: string[];
  };
  scrollTop: Record<string, number>;
}
```

刷新后若已选节点消失，详情页显示“节点已不在当前快照中”，保留返回路径，不悄悄切换到另一个节点。

### 2.5 现有文件调整

| 文件 | 设计修改 |
| --- | --- |
| `src/resourcePresentation.ts` | `ResourcePreferences.tab` 增加 `netbird`；修正 normalize 逻辑 |
| `src/ServicePanel.tsx` | 增加 NetBird 标签、图标、tabpanel；将键盘索引改为通用三项算法 |
| `src/SessionWorkspace.tsx` | 接入 Provider 和 Panel；`WorkspaceSnapshot` 增加 `netbird`；移动工作区时捕获快照 |
| `src/resources.css` | 只放一级导航需要的共用调整 |
| `src/netbird/netbird.css` | NetBird 页面专用样式，不向 Docker/监控泄漏选择器 |

`ResourcePreferences` 只保存一级页签和监控偏好；NetBird 的页面状态放在独立 `NetBirdPanelSnapshot`，避免继续扩大资源监控类型。

## 3. Tauri IPC

首版命令接口：

```text
subscribe_netbird(sessionId, interest, events) -> subscriptionId
update_netbird_interest(subscriptionId, interest)
ack_netbird_frame(subscriptionId, revision)
unsubscribe_netbird(subscriptionId)
refresh_netbird(subscriptionId)

get_netbird_raw_status(sessionId, bindingToken, detailed) -> string
get_netbird_logs(sessionId, bindingToken, limit) -> LogPage
probe_netbird_peer(sessionId, bindingToken, request) -> ProbeResult

prepare_netbird_operation(sessionId, bindingToken, action) -> OperationPreview
submit_netbird_operation(sessionId, request) -> operationId
reconcile_netbird_operation(sessionId, operationId)
acknowledge_netbird_operation(sessionId, operationId)

set_netbird_cli_path(sessionId, path | null) -> NetBirdBinding
```

所有命令都执行 `workspace_only` 和窗口／订阅所有权校验。`bindingToken` 只能引用当前 SSH 身份下由后端检测出的实例。

写操作请求：

```ts
type NetBirdAction =
  | { type: "up" }
  | { type: "down" }
  | { type: "serviceStart" }
  | { type: "serviceRestart" };

interface NetBirdOperationRequest {
  requestId: string;
  bindingToken: string;
  action: NetBirdAction;
  previewId: string;
}
```

`requestId` 保证 IPC 回复丢失后只能以相同请求重试，并返回同一个 operation ID。

节点检测请求不接受 shell 字符串：

```ts
type NetBirdProbeRequest =
  | { peerId: string; type: "icmp"; timeoutMs: number }
  | { peerId: string; type: "tcp"; port: number; timeoutMs: number };
```

后端从当前快照解析目标地址；前端不能直接提交任意主机或扫描端口范围。

## 4. Rust 模块

新增：

```text
src-tauri/src/netbird/
├── mod.rs
├── commands.rs
├── model.rs
├── manager.rs
├── detect.rs
├── query.rs
├── parser.rs
├── safety.rs
├── operations.rs
├── diagnostics.rs
├── store.rs
└── tests.rs
```

### 4.1 `model.rs`

包含可序列化的 snapshot、peer、binding、frame、preview 和 operation 类型。状态使用有限枚举加 `raw_state`，未知 CLI 枚举不能导致整份解析失败。

`Operation.phase` 统一为：

```text
queued
running
verifying
succeeded
failed
uncertain
```

`uncertain` 表示命令可能已经执行，但没有足够证据确认结果；它不能自动重试。

### 4.2 `detect.rs`

检测顺序：

1. 若服务器有已保存的 CLI 路径，先验证它是绝对路径、无 NUL 且可执行。
2. 否则运行受限的 `command -v netbird`。
3. 获取版本和支持的子命令／参数。
4. 尝试 `status --json`，确认 daemon 访问能力。
5. 生成 `NetBirdBinding` 和 token。

token 至少绑定：

- SSH connection identity。
- 规范化 CLI 路径。
- 当前 NetBird 客户端／daemon 可识别信息。
- 当前 profile 或 daemon 目标（若 CLI 能可靠返回）。
- capability 集合。

不能只根据版本号推断能力；每项能力分别探测。能力检测失败只关闭对应 UI，不让整个概览失败。

首版只支持默认 daemon／profile。若检测到目标不明确，状态仍可只读展示，但禁止写操作，并提示后续选择目标。Profile 切换不进入首版。

### 4.3 `query.rs` 与 `parser.rs`

`query.rs` 负责固定命令和输出限制；`parser.rs` 只处理纯数据，便于对不同版本样本做单元测试。

读取约束建议：

- 状态超时 8 秒。
- JSON stdout 上限 8 MiB。
- stderr 上限 256 KiB。
- peer 数量上限 10,000。
- 日志单次最多 2,000 行或 2 MiB。

JSON 先解析为 `serde_json::Value`，通过小型兼容函数读取多个已知字段路径。允许：字段缺失、`null`、新增字段、未知状态和 IPv6-only 节点。关键身份字段无法确认时，快照可读但 `binding.writable = false`。

若 JSON 不受支持：

- 展示安装、版本和 daemon 可访问性。
- 原始状态允许用户主动查看。
- peers 保持不可用。
- 所有写操作保持禁用，因为无法可靠核对结果。

### 4.4 `manager.rs`

结构建议：

```rust
pub struct NetBirdManager {
    entries: Mutex<HashMap<String, Arc<Entry>>>,
    store: Mutex<Result<Store, AppError>>,
    jobs: AtomicUsize,
}

struct Entry {
    generation: String,
    cancel: Cancellation,
    core: Mutex<Core>,
}

struct Core {
    members: HashMap<String, SharedContext>,
    preferred_session: Option<String>,
    subscribers: HashMap<String, Subscriber>,
    snapshot: NetBirdSnapshot,
    revision: u64,
    refresh_generation: u64,
    collecting: bool,
    next_poll: Instant,
    active_operation: Option<String>,
}
```

Registry key 使用 `ConnectionContext::identity()`。会话关闭时移除 member；只要同身份还有其他会话，就继续工作。没有订阅且没有活动操作时停止采集并释放连接。

Channel 采用最新帧覆盖语义和 ack 背压，与 Docker 管理一致：同一订阅上一帧未 ack 时不无限排队，只保留最新待发送状态。

### 4.5 远端命令执行

只读查询可复用 `connection::connect_verified`，但不直接调用无取消能力的 `ssh::exec`。NetBird 模块提供有界 runner：

```rust
struct ExecLimits {
    timeout: Duration,
    stdout: usize,
    stderr: usize,
}
```

runner 必须：

- 非阻塞读取 stdout 和 stderr，避免远端管道死锁。
- 每轮检查应用状态、Cancellation、窗口／会话绑定。
- 超时后关闭 channel。
- 输出超过限制立即终止。
- 最终恢复 session blocking 状态。

首批不为此功能重构所有旧远端读取代码；NetBird runner 可在稳定后再抽成公共模块。

所有命令由枚举构造，并对 CLI 路径使用 `ssh::quote`。不得接收前端传入的附加参数或完整命令。

### 4.6 `store.rs`

使用独立 `netbird.sqlite`，只保存：

- 每个 server ID 的 CLI 路径覆盖。
- 最近操作回执和状态。
- request ID 到 operation ID 的幂等映射。

状态快照、peers、日志和原始 CLI 输出不持久化。

应用异常退出后，数据库中仍处于 `queued/running/verifying` 的任务恢复为 `uncertain`。用户以相同 SSH 身份重新连接后可以显式核对；不能在启动时自动重放命令。

## 5. SSH 路径安全检查

`safety.rs` 返回三态结果：

```rust
enum PathSafety {
    Independent,
    Dependent,
    Unknown,
}
```

证据结构随 preview 返回 UI：

```ts
interface NetBirdPathAssessment {
  result: "independent" | "dependent" | "unknown";
  checkedAt: number;
  sshClientAddress: string | null;
  sshServerAddress: string | null;
  routeInterface: string | null;
  reasons: string[];
}
```

检查步骤：

1. 在目标服务器读取当前 exec channel 的 `SSH_CONNECTION`，得到本次 SSH 的实际源和目标地址。
2. 将 SSH 目标地址与 NetBird 本机 IPv4／IPv6 比较。
3. 从系统地址表定位承载 NetBird 地址的接口，不假设名称一定是 `wt0`。
4. 对 SSH 来源地址执行 `ip -j route get`；IPv6 使用对应命令。
5. 检查返回接口、prefsrc、策略路由结果是否经过 NetBird 接口。
6. 存在跳板时，额外使用实际 SSH 目标地址判断跳板到目标这一段是否连接到 NetBird 地址。

缺少 `ip -j`、输出无法解析、源地址异常或证据冲突均返回 `Unknown`。不能通过 `100.x` 网段或接口名称猜测。

首版策略：

| 操作 | Independent | Dependent | Unknown |
| --- | --- | --- | --- |
| `down` | 可确认后执行 | 阻止 | 阻止 |
| `serviceRestart` | 可确认后执行 | 阻止 | 阻止 |
| `serviceStart` | 可确认后执行 | 阻止 | 阻止 |
| `up` | 展示可能改变路由的普通确认 | 不适用／按当前证据提示 | 允许确认，但提示连接可能变化 |

提交前必须重新执行关键检查；preview 默认 30 秒过期。若结果由 Independent 变成 Dependent／Unknown，拒绝提交。

## 6. 写操作执行与核对

### 6.1 命令回执

写命令使用一次性 nonce，并在命令结束后输出固定回执：

```text
<固定 NetBird 命令>
code=$?
printf '__SODECK_NETBIRD_<nonce>_%s\n' "$code"
```

只有格式严格匹配且位于输出尾部的回执才能证明命令已经返回。若 SSH 在收到回执前断开，结果为 `uncertain`，不能自动重试。

### 6.2 幂等和串行化

- 同一 connection identity + binding token 同时只允许一个写操作。
- 重复 `requestId` 返回原 operation ID。
- 不同窗口同时提交时，后到请求返回 `busy`。
- preview 绑定窗口 generation、connection identity、binding token、action 和过期时间。
- 不允许用另一个窗口的 preview 提交。

### 6.3 成功条件

| 操作 | 核对目标 |
| --- | --- |
| `up` | 客户端状态成为 Connected；需要登录或超时均不算成功 |
| `down` | daemon 仍可访问，客户端状态进入 Idle／Disconnected |
| `serviceStart` | 服务可访问；连接状态可以是 Connected、Connecting 或 Idle |
| `serviceRestart` | daemon 重新可访问，服务实例和目标绑定重新核对 |

命令返回 0 仅进入 `verifying`。查询成功但未达到目标，记录为 `failed`；查询本身无法完成，记录为 `uncertain`。

建议核对节奏：立即、500 ms、1 s、2 s、4 s，最长约 15 秒；服务重启最长 30 秒。核对阶段不重复写命令。

## 7. 诊断与节点检测

### 7.1 原始状态和日志

使用独立按需 IPC，不进入共享 frame：

- `get_netbird_raw_status` 返回有界文本。
- `get_netbird_logs` 返回行数组、来源、截断标记和读取时间。
- 页面隐藏、窗口移动或组件卸载时丢弃前端内容。

日志路径先从服务／客户端实际配置中获取；无法获取时才尝试官方默认路径。读取权限不足直接返回明确错误，不自动 sudo。

### 7.2 单次节点检测

后端从当前 snapshot 的 `peerId` 找到地址，避免前端把此接口变成任意主机探测器。

- ICMP：调用单次、带 deadline 的系统 ping；命令能力先探测。
- TCP：只允许一个 `1..=65535` 端口；优先使用已有 `nc`／`timeout` 能力组合，能力不存在则返回 unsupported。
- 每次结果记录起点、目标地址、耗时、时间和错误语义。
- 检测不写入 peer 的隧道状态，只在详情页显示独立结果。

## 8. 生命周期接入

`src-tauri/src/lib.rs` 需要：

- `mod netbird;`
- `State` 增加 `netbird: netbird::NetBirdManager`。
- setup 时打开 `netbird.sqlite`。
- 注册 NetBird commands。
- 页面重载和窗口销毁时清理该窗口订阅／preview，但不取消已提交操作。
- `quit_app` 中先停止新任务，再调用 `netbird.shutdown()` 等待后台线程有界收尾。
- `delete_server` 在该服务器存在活动 NetBird 操作时返回 busy；仅有 CLI 路径设置时可随删除清理。

`connection::finish` 在 SSH 会话结束时同时调用 `netbird.close_session(id)`。后端保留旧快照并向其他订阅发布 disconnected／stale，而不是清空数据。

流量统计增加 `TrafficPurpose::NetBirdMonitor` 和 `TrafficPurpose::NetBirdAction`：前者计入 monitoring，后者计入 other，避免把写操作误算为资源采样。

## 9. 错误分类

沿用 `AppError.kind`，NetBird 建议使用：

| kind | 用途 |
| --- | --- |
| `unsupported` | CLI、JSON、service、ping 等能力不存在 |
| `unavailable` | daemon 或依赖服务暂不可用 |
| `authentication` | NetBird 需要登录或认证失效 |
| `permission` | CLI socket、日志或服务操作权限不足 |
| `conflict` | binding、profile、客户端状态或路径证据已变化 |
| `busy` | 已有写操作或采集任务达到限制 |
| `uncertain` | 命令可能执行但无法核对；主要作为 operation phase，必要时映射错误 |
| `limit` | 输出、peer 数量或日志请求超限 |
| `timeout` | 远端查询或核对超时 |

展示文案在 presentation 层映射；Rust 错误仍提供安全、可直接显示的中文消息。

## 10. 测试设计

### 10.1 Rust 单元测试

`src-tauri/src/netbird/tests.rs` 覆盖：

- 多个真实版本的 `status --json` 样本。
- 缺字段、null、新字段和未知枚举。
- IPv4、IPv6-only、直连、中继、Idle 和连接中。
- CLI／daemon 版本不一致。
- 10,000 个 peer 边界和输出过大。
- CLI 路径及所有命令参数的 shell quoting。
- binding token 变化后拒绝写操作。
- preview 过期、跨窗口提交和重复 request ID。
- 操作串行化、收到／未收到回执、核对成功／失败／uncertain。
- 路径判断：直连 NetBird 地址、经 NetBird 路由、普通网卡、IPv6、跳板、无法解析和缺少 `ip -j`。
- session 关闭、窗口移动和应用退出时无孤儿线程。

测试样本放在：

```text
src-tauri/src/netbird/fixtures/
├── status-connected.json
├── status-idle-lazy.json
├── status-relayed.json
├── status-partial-control-plane.json
├── status-ipv6.json
└── status-unknown-fields.json
```

### 10.2 前端单元测试

- `netbirdPresentation.test.ts`：状态、颜色、文案、排序和未知值。
- `netbirdStore.test.ts`：epoch、revision、ack、晚到 frame、断线旧值、刷新合并和兴趣变化。
- `resourcePresentation.test.ts`：旧快照无 `netbird` 时仍归一化正确。

### 10.3 Playwright UI

新增：

```text
tests/netbird-ui.spec.ts
playwright.netbird.config.ts
npm run test:netbird
```

覆盖：

- 概览、节点搜索筛选和详情返回位置。
- Management 异常但隧道仍存在的“连接受限”。
- Idle 和 Relayed 的非错误表现。
- 旧数据、权限不足、需要登录、未安装和无节点。
- 普通刷新不闪空内容。
- 路径 Independent 的确认流程。
- Dependent／Unknown 阻止 down 和 service restart。
- 命令完成后的 verifying、failed 和 uncertain。
- 多窗口共享操作锁及晚到事件。
- 320px、深色、高对比度和减少动态效果。
- 键盘 Tablist、列表 Enter、对话框焦点恢复。

### 10.4 SSH fixture 与真实联调

为 `tests/ssh_fixture.py` 增加独立 `netbird_fixture.py`，模拟固定 NetBird 命令和可变状态，不执行宿主机命令。fixture 支持通过 `.qa` 控制：连接状态、daemon 不可用、权限错误、慢查询、命令回执丢失和 SSH 中断。

此外保留 ignored 的真实 Linux 联调测试，连接隔离 NetBird 测试网络，从不依赖 NetBird 的管理路径执行 `up/down/service restart`。CI 的模拟 fixture 不能替代真实客户端兼容性验收。

## 11. 推荐实现顺序

### 阶段 A：只读闭环

1. Rust model、detect、query、parser 和样本测试。
2. Manager 订阅、轮询、frame/ack 和断线旧值。
3. TS contracts、Store、Provider。
4. ServicePanel 接入、概览和 peers。
5. 工作区快照、搜索／筛选／详情和 UI 测试。
6. 原始状态、基础日志和单次节点检测。

此阶段完成并通过真实 NetBird 样本后，才进入写操作。

### 阶段 B：安全写操作

1. `safety.rs` 及路径样本测试。
2. preview、SQLite 回执、request ID 和串行锁。
3. `up/down` 执行与核对。
4. service start/restart。
5. uncertain 与显式 reconcile。
6. 跳板、IPv6 和真实隔离节点回归。

### 阶段 C：第二批能力

Docker 客户端、诊断包、网络选择和 Profile 选择在独立设计确认后实现，不提前为其污染首版接口。

## 12. 需要确认的实现边界

本方案建议确认以下五点后开始编码：

1. 首版只支持 Linux 原生客户端，Docker NetBird 客户端放第二批。
2. 使用独立 `NetBirdManager`，不加入资源监控 Manager。
3. 先完成只读闭环，通过测试后才开放写操作。
4. `down`、service start/restart 在路径为 Unknown 时直接阻止，不提供强制继续。
5. 写操作回执持久化到独立 SQLite；应用重启后只允许核对，不自动重放。

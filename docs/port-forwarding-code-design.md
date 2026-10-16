# SSH 端口转发与跳板共用内核：代码设计

状态：已授权实施并在独立分支完成代码实现。日期：2026-10-16。

本文保留实施前的代码设计基线；示意接口与最终实现的差异、已执行测试及原生验收限制见 [实施与验证记录](port-forwarding-implementation.md)。

功能基线：[功能需求](port-forwarding-requirements.md)。产品与架构背景：[总体设计](port-forwarding-design.md)。本文细化模块、所有权、接口、状态机、数据存储、并发和验收；不扩展已约定的功能范围。

## 1. 技术决策

| 项目 | 本次选择 | 原因 |
| --- | --- | --- |
| SSH 实现 | 沿用 ssh2/libssh2 | 复用认证、指纹、跳板与既有计量 |
| 共享边界 | 通道调度和字节转发内核 | 跳板与本地转发统一处理 I/O，不互相依赖业务配置 |
| 会话所有权 | 每个 SSH Session 由一个执行循环独占 | 避免多个线程交错驱动 libssh2 的会话状态 |
| 业务隔离 | 每条规则独立 SSH 路径，内部多 channel | 一条规则停止或重启不影响其他业务 |
| 运行管理 | State.port_forwarding 应用级管理器 | 运行不依赖终端或管理窗口存在 |
| 网络等待 | 非阻塞 I/O + polling readiness | 控制空闲 CPU，避免每个客户端一个线程 |
| 连接建立 | 每条运行至多一个独立连接尝试任务 | 认证或网络等待期间仍能处理停止和本地新连接 |
| 配置存储 | 独立 port-forwards.sqlite | 不修改 servers.db 的现有结构 |
| 前端 | Tauri 命令 + 状态快照 + React | 业务字节不进入 IPC |
| 新增直接依赖 | polling 3、socket2 0.6、tauri-plugin-opener 2 | readiness、跨平台 socket 设置、受控网页打开 |

当前锁文件已包含 polling 3.11.0、socket2 0.6.5，拟将它们声明为直接依赖；锁定解析结果以实施时构建为准。opener 仅由 Rust 后端生成并打开回环 HTTP/HTTPS URL，不向前端开放任意 URL/程序启动能力。无需新增前端状态库，也不重写为 Tokio SSH 客户端。

## 2. 当前代码与接入位置

以下为本次代码检查确认的事实，位置以函数/类型名定位，避免正在编辑的文件行号变化。

| 现有位置 | 当前职责/限制 | 计划调整 |
| --- | --- | --- |
| connection.rs / ConnectionPlan、ConnectionContext | 单层跳板、身份与凭据快照 | 保留语义，提供独立于终端的连接入口 |
| connection.rs / InteractiveProvider、Interaction | 通过 TerminalStream 发送认证事件 | 抽出 ConnectEventSink，由终端和转发各自适配 |
| connection.rs / connect_metered、connect_verified | 建立已验证 SSH 路径，结束时设为阻塞 | 保留旧调用行为，转发内核接管时切非阻塞 |
| ssh_transport.rs / forward、pump、TunnelGuard | 为下一跳 SSH 提供单通道桥接 | forward 保留外部接口，内部改接共享内核 |
| ssh_transport.rs / Cancellation、tcp | socket 取消；系统 DNS 使用临时线程 | 复用取消，增加有界 DNS 执行预算及 readiness 句柄 |
| ssh.rs / authenticate | 密码、私钥、Agent；已设置 SSH keepalive | 保留认证语义，重新核验共享内核中的保活调度 |
| lib.rs / State、setup、quit_app、窗口事件 | 应用资源、关闭与退出入口 | 注册管理器，补齐启动/退出/窗口清理 |
| lib.rs / list_servers、delete_server、reset_incompatible_database | 服务器查询目前只允许主窗口 | 新管理窗口使用专用查询接口，删除/重建检查转发引用 |
| traffic.rs | 固定四类通信账目 | 增加端口转发分类并同步前端类型 |
| App.tsx / Root、退出回调 | 按 URL 参数路由独立窗口；组合退出说明 | 增加转发窗口和退出摘要，不调整现有窗口布局 |
| WindowTitlebar、ConnectionDialogs | 共用标题栏、主题、认证弹窗 | 新窗口复用，认证控制逻辑独立 |
| capabilities/desktop.json | 按窗口标签授权 | 加入 port-forwarding 窗口 |

当前工作区存在其他尚未提交的改动，包括主题、标题栏、日志和监控。实施时以当时文件内容为基线做局部接入，不覆盖或回退这些改动。

## 3. 模块与依赖关系

```text
src-tauri/src/
  connection.rs                 现有入口与兼容导出
  connection/
    interaction.rs              挑战、等待、回复、取消
    auth.rs                     通用认证事件接口和 provider
  ssh_transport.rs              Socket / SshConnection / Cancellation 门面
  ssh_transport/
    forwarding.rs               ForwardCore：通道队列、状态与调度
    relay.rs                    单通道双向缓冲、半关闭、关闭过程
    reactor.rs                  readiness 注册、唤醒与资源句柄生命周期
    jump.rs                     既有跳板的 stream-pair 适配
    resolver.rs                 有界系统解析任务
    meter.rs、meter.c           沿用现有传输计量
  port_forwarding/
    mod.rs                      PortForwardManager、注册表、业务协调
    model.rs                    规则、运行、事件、错误与 DTO
    store.rs                    SQLite、版本和 CAS 更新
    commands.rs                 IPC、输入/调用窗口校验
    worker.rs                   单规则监督循环和连接尝试
    listener.rs                 绑定、接入和并发配额
    windows.rs                  管理窗口登记、聚焦、失效处理
    tests.rs                    管理与生命周期测试

src/
  PortForwardingApp.tsx         列表、选择与操作编排
  PortForwardEditor.tsx         规则表单及修改提示
  PortForwardDetails.tsx        路径、诊断、流量与事件
  PortForwardConfirmDialog.tsx  停止/删除/应用确认
  portForwardApi.ts             IPC 与 TypeScript DTO
  usePortForwards.ts            订阅、快照与请求状态
  portForwardPresentation.ts    地址、状态与操作可用性
  port-forwarding.css           新窗口样式
```

依赖方向：

```text
前端 → commands/windows → PortForwardManager → RuleWorker
                                              ├─ connection：构建路径和认证
                                              │    └─ jump adapter → ForwardCore
                                              └─ ForwardCore：最终服务器到业务目标

ForwardCore → SshConnection / ssh2 / relay / reactor
```

ForwardCore 不引用 State、数据库、Tauri、规则或 React。它接收已建立的 SSH 连接及待转发流。跳板将内核产生的字节通道适配为下一跳的传输输入，属于资源组合，不由内核反向发起连接路径构建。

connection.rs 暂不整体拆分；只提取认证交互所需部分，并保留既有类型/函数导出及终端 IPC 事件形状。

## 4. 数据模型与所有权

### 4.1 配置、运行与展示分离

```rust
// 示例字段；真正的 wire 类型另有序列化定义。
struct ForwardRule {
    id: RuleId,
    revision: Revision,
    name: String,
    web_scheme: Option<WebScheme>,
    settings: ForwardSettings,
}

struct ForwardSettings {
    ssh_server_id: String,
    listen_address: LoopbackAddress, // V4 / V6
    listen_port: Option<u16>,       // None = 自动分配
    target: ForwardTarget,          // host + port
    auto_reconnect: bool,
}

struct ActiveRun {
    run_id: RunId,
    settings: ForwardSettings,
    route_snapshot: Arc<ConnectionPlan>, // 仅 Rust，可能含凭据
    verified_context: Option<SharedContext>,
    attempt_id: AttemptId,
    ever_ready: bool,
    phase: RunPhase,
    actual_listen_address: Option<SocketAddr>,
}

// Manager 保存的轻量控制句柄；不持有 listener 或 SSH Session。
struct RunHandle {
    run_id: RunId,
    stop: RunStop,
    controls: SyncSender<RunControl>,
    completion: RunCompletion,
}
```

`ForwardRule` 表示当前已保存配置；`ActiveRun` 表示实际生效设置。两者不同即产生 `pendingChanges`。名称和网页打开方式立即更新，不把它们与运行设置混在同一个相等性判断中。

完整连接计划、凭据和 SharedContext 不实现 Serialize/Debug；前端只得到安全的连接路径展示 DTO。比较路径修改时只在 Rust 内比较相关字段，不将密码哈希作为前端配置版本。

停止后 ActiveRun 释放，只保留不含凭据的 LastRunSummary，供查看累计量和结束原因。重新显式启动创建新 run_id 和新统计；网络重连只增加 attempt_id，延续同一 run_id 的业务累计。

### 4.2 标识与过期结果隔离

| 标识 | 用途 |
| --- | --- |
| rule_id | 持久规则身份 |
| rule revision | 编辑 CAS，防止过期保存/应用 |
| run_id | 一次显式启动的资源归属 |
| attempt_id | 该运行内一次完整 SSH 路径连接尝试 |
| stream_id / probe_id | 独立业务连接或目标测试 |
| window label + generation | 认证、订阅、窗口操作的归属 |
| challenge_id | 一次具体指纹或凭据回答 |
| snapshot epoch + revision | 快照排序、重订阅和旧事件过滤 |

异步结果至少携带 run_id、attempt_id；认证回复还校验窗口代次和 challenge_id。比较失败则丢弃并释放结果内携带的连接，不能只忽略通知却遗留 socket。

UUID 使用字符串；累计字节与长期递增序号通过十进制字符串传给 JavaScript，避免整数精度问题。时间戳使用毫秒，超时/退避基于 Rust Instant，不受系统时钟调整影响。

### 4.3 资源拥有者

| 对象 | 唯一拥有者 | 对外提供 |
| --- | --- | --- |
| 已保存规则、Store、订阅登记 | PortForwardManager | 配置操作和只读快照 |
| 活动运行登记 | Manager 中的 RunHandle | 取消、控制消息和完成信号 |
| ActiveRun、listener、连接尝试和重连计时 | RuleWorker | 不含凭据的运行快照 |
| 正在建立的 SSH 路径 | RouteJob | 成功后一次性移交 SshConnection + SharedContext |
| 已就绪 SSH Session、channel、relay 缓冲 | 所在线程中的 ForwardCore | 事件、状态和流量增量 |
| 父跳板 SSH Session | JumpDriver 中的 ForwardCore | 子连接持有的内部 stream 和 guard |
| 表单草稿、选中项和展开状态 | React | 经校验的配置/操作请求 |

ActiveRun 的可变运行状态只由 RuleWorker 更新；Manager 中的展示快照不能反向成为 worker 的第二份权威状态。停止时先通过 RunHandle 发出取消，再等待 completion；不能仅删注册表条目来表示连接已经释放。

## 5. SQLite 存储

文件：应用数据目录 `port-forwards.sqlite`。第一版 schema 示例：

```sql
CREATE TABLE forward_rules (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  ssh_server_id TEXT NOT NULL,
  listen_address TEXT NOT NULL CHECK (listen_address IN ('127.0.0.1', '::1')),
  listen_port INTEGER CHECK (listen_port IS NULL OR listen_port BETWEEN 1 AND 65535),
  target_host TEXT NOT NULL,
  target_port INTEGER NOT NULL CHECK (target_port BETWEEN 1 AND 65535),
  auto_reconnect INTEGER NOT NULL CHECK (auto_reconnect IN (0, 1)),
  web_scheme TEXT CHECK (web_scheme IS NULL OR web_scheme IN ('http', 'https')),
  revision INTEGER NOT NULL CHECK (revision > 0),
  created_at_ms INTEGER NOT NULL,
  updated_at_ms INTEGER NOT NULL
);
CREATE INDEX forward_rules_server ON forward_rules(ssh_server_id);
PRAGMA user_version = 1;
```

- 仅新库初始化此结构，未知 schema 版本返回明确错误，不自动清空文件。
- SQLite 使用 WAL、有限 busy_timeout 和事务；保存通过 `WHERE id = ? AND revision = ?` 更新并递增 revision，影响行数为零返回冲突。
- 同地址端口不建唯一索引：允许保存不能同时启动的规则，实际绑定由 OS 裁决。
- 跨库引用不使用伪外键。保存规则、删除服务器和重建数据库通过既有 configuration 协调锁串行检查。
- 保存成功后才发布配置更新；写入失败不改变内存中的生效配置或活动连接。
- 存储不可用时，转发功能显示不可用；不因此阻止终端启动，不创建空库覆盖原文件。服务器删除若无法检查规则引用，明确失败。
- 存储中途失败不强行中断已经运行的隧道，但配置写操作和新启动必须验证其所需配置可读取。

## 6. 从终端中提取认证交互

新增内部事件和发送接口：

```rust
enum ConnectEvent {
    Progress { hop: HopView, stage: String },
    AuthRequired { challenge: AuthChallenge },
    HostKeyRequired { challenge: HostKeyChallenge },
}

trait ConnectEventSink: Send + Sync {
    fn send(&self, event: ConnectEvent) -> AppResult<()>;
}

fn connect_interactive(
    state: &State,
    plan: &ConnectionPlan,
    sink: &dyn ConnectEventSink,
    interaction: &Interaction,
    cancel: &Cancellation,
    purpose: TrafficPurpose,
) -> AppResult<(SshConnection, SharedContext)>;
```

`TerminalConnectSink` 转换为原 ConnectionEvent，继续经 TerminalStream 发送；`ForwardConnectSink` 转换为带运行/尝试/窗口代次的认证事件。Interaction.wait 使用通用 sink，保留现有挑战类型核对、认证重试、等待超时与成功后保存密码的保护条件。

转发不用 `open_connection`、不登记 State.terminals、不创建 PTY、shell 集成或终端历史。后台自动重连继续用 connect_verified 校验已验证指纹；需要用户处理时，用户显式恢复操作才建立新的交互尝试。

管理窗口可复用当前会话上下文，但必须由打开窗口的后端调用通过 `state.spec(session_id, caller_window)` 取得，并核对新规则当前路径与该上下文一致。前端不能任意传 identity 获取凭据。没有合适上下文就独立认证；不新增全局凭据缓存。

原 `answer_connection` 继续只服务终端。新增转发认证回复命令，独立检查 rule/run/attempt/window/challenge 归属后调用通用 Interaction。

重新认证仍固定原运行的服务器地址和路径，认证策略额外携带上次 VerifiedRoute。某一跳指纹与原运行不同，即使其他窗口刚更新了全局 host_keys，也必须在本次恢复流程中明确确认；不能仅因数据库现在信任它就静默改变本规则的身份。成功后更新该运行的 verified_context 和认证材料，后续自动重连按新确认的身份校验。修改地址、账号或跳板路径仍走显式停止并应用。

从已连接会话预选时持有的上下文只用于当前管理窗口的短期启动上下文；窗口关闭、预选切换或路径不匹配后释放，不把管理窗口变成另一个长期凭据缓存。真正已启动的规则自行持有它所需的运行凭据。

## 7. 共享转发内核

### 7.1 内核接口与执行方式

ForwardCore 是被单一线程驱动的状态机，本身不创建业务线程或读取数据库。

```rust
impl ForwardCore {
    fn new(connection: SshConnection, limits: RelayLimits) -> AppResult<Self>;
    fn attach(&mut self, id: StreamId, peer: Socket,
              target: ForwardTarget, origin: SocketAddr) -> AppResult<()>;
    fn probe(&mut self, id: ProbeId, target: ForwardTarget) -> AppResult<()>;
    fn cancel_probe(&mut self, id: ProbeId);
    fn drive(&mut self, now: Instant, budget: DriveBudget) -> CoreUpdate;
    fn sync_interests(&self, reactor: &mut Reactor) -> AppResult<()>;
    fn abort(&mut self);
}
```

CoreUpdate 包含进展、单流/探测结果、流量增量、最近截止时间，以及是否发生整个 SSH 传输失效。上层决定是否重连。接口落地时可调整参数承载方式，但保持独占、所有权和非阻塞契约。

`RuleWorker` 使用 core 调度多个业务流；`JumpDriver` 使用同一 core 调度一条内部流。两者共用 reactor、channel-open 和 relay 逻辑，不保留第二套独立 pump。

### 7.2 已确认的库约束

本机锁定版本为 ssh2 0.9.6、libssh2-sys 0.3.3。本次阅读对应依赖源码确认：

1. channel.c 中的 `session->open_state` 和 `direct_state` 是 Session 级状态，同一 Session 不可同时推进多个 direct-tcpip 打开请求。
2. transport.c 的挂起发送要求原始调用参数保持一致；channel 写入的 EAGAIN 恢复不能换掉待写缓冲。
3. ssh2 的 Channel Drop 会调用 channel_free 但忽略其返回值；正常运行不能把“丢弃 Rust 对象”当作已完成协议关闭的证明。
4. libssh2 keepalive_send 的实现可能忽略发送时的 EAGAIN；函数成功不代表已收到保活回应，也不能据此判定发送队列一定为空。

以上支持调度方案选择，尚不代表并发正确性已通过实际测试。

### 7.3 调度规则

- 同时只有一个 pending channel-open，剩余接入进入有界队列；请求中保存固定 target、origin、归属 ID 和截止时间。
- 每个流具有固定分配的双向缓冲及独立读写游标；有部分写入时仅推进游标，不移动尚待重试的数据。
- 对产生 EAGAIN 的 SSH 操作，记录操作归属、参数和立即读取的 block_directions。
- 出站协议写入挂起时优先恢复其拥有者，包括可能由 read/窗口调整/EOF/close 引起的写入，避免其他调用抢占未完成报文。
- 等待某个通道的入站数据不锁死整个 Session；按每轮预算公平推进其他可执行流。需要识别“会话发送挂起”与“该流等待入站”的区别。
- 当前调用等待方向不可由后续调用覆盖后再统一读取；每次调用后保存状态，再合并 readiness 需求。
- 消化 libssh2 内部已有的数据后才等待新 socket 事件。仅在确有待发送数据时订阅可写，避免空闲忙循环。
- Session 所有 SSH 调用都在所属执行线程完成，包括 keepalive、probe、close；其 Session 和 Channel 不克隆给其他工作线程。
- pending open 的客户端断开或测试取消后，仍保留打开操作的归属。迟到 channel 到达后关闭，不能交给下一客户端。
- 挂起打开或协议写入达到时限且无法安全取消时，报告 TransportFailure，结束该 Session；上层重建完整规则路径。禁止强行清空内部状态后继续复用 Session。

控制循环需要真实 socket 部分写入、接收窗口耗尽和并发关闭实验验证；在这些实验通过之前不替换现有跳板生产路径。

### 7.4 半关闭、结束与资源

每个流的状态包含 opening、relaying、closing、finished，另有 local_read_eof、remote_read_eof、eof_sent 和 local_write_shutdown 标记。

本地 EOF → 发送完已有上传缓冲 → send_eof；之后仍可接收远端响应。远端 EOF → 写完下载缓冲 → 本地 shutdown(Write)。正常完成进入可重试的 close/wait-close 流程，再释放 channel。

正常关闭有截止时间，不能长期积累未释放 channel；若库状态不允许单独结束，则升级为该传输失效并明确影响范围。强制停止先撤销 poll 注册、关闭底层 transport，再按 channel、Session、父跳板 guard 的归属顺序释放，防止 Drop 中等待活连接。

reactor 注册必须由拥有 socket 的对象保持有效，先注销后关闭，不使用会被 OS 重用的裸整数作为长期身份。停止通知通过 Poller.notify 和取消信号唤醒，不依赖命令队列尚有空位。

### 7.5 容量、保活与连接建立

初始预算：每规则 64 条业务连接（包括打开队列），等待打开最多 8 条；应用业务连接共 256 条；每方向应用缓冲 64 KiB；每规则同时一个 probe，并计入明确的 channel 配额。配额用 RAII permit，在失败、取消、迟到结果和正常关闭时统一归还。

libssh2 的通道窗口和内部队列也占内存，须压测确认实际 RSS，不能只用应用缓冲容量计算总内存。最终调参保持既定有界要求。

连接/认证等待阶段不能占用唯一监听循环：每次路径构建在一个 RouteJob 中执行，使用专属 Cancellation，结果通过容量为 1 的通道返回。RuleWorker 持有监听器，持续快速关闭未就绪阶段的新接入，并处理取消。旧 RouteJob 完全收尾后才允许下一尝试；旧结果若失效，当场释放。

沿用现有 tcp 的 DNS 语义，但将可阻塞的系统解析限制到固定小型执行池/预算中，防止自动重连不断增加悬挂解析线程。解析任务不持有凭据或业务 socket，超时结果不能再发起连接；系统解析本身不可强制中断的限制需要明确处理，不能让退出无限等待它。

传输活性以实际 I/O、操作期限和平台 TCP keepalive 为依据，通过 socket2 配置平台支持的参数；Linux 可结合 TCP_USER_TIMEOUT，平台不支持的选项有明确降级。不要按业务空闲时间断开合法连接。

TCP 探测只覆盖它实际绑定的网络连接。本机到 A 的 TCP 正常，不能证明 A 到 B 的内部连接正常；子 SSH 使用的本地 stream pair 更不能当作 B 的网络探测。G2 必须单独覆盖 A → B 黑洞和 B 空闲断开，记录最终确认失效的触发与延迟，不能把第一跳探测成功显示为完整路径健康。

相对总体设计的技术细化：SSH keepalive 不直接照搬旧 pump 的定时调用。先验证其非阻塞挂起语义；共享内核的基础方案用 TCP 探测和操作时限，只有证明不会遗留会话发送状态后才启用 SSH 层保活。此处不改变自动重连需求，也不承诺尚未实测的统一断网检测秒数。

无 channel、无待处理操作时，不因 SSH socket 持续可读而空转；必要时仅做非消费式 EOF/错误检查，不自行读取或解析 SSH 报文。空闲会话的协议级失效可能在下一次实际 SSH 操作时被确认，这一点纳入黑洞/空闲场景验证。

本地 listener 直接使用规范化的回环地址绑定：IPv6 显式使用 only_v6，不依赖系统默认双栈行为；不启用允许重复监听同端口的选项。Windows 校验独占绑定行为，必要时在平台适配中设置 SO_EXCLUSIVEADDRUSE；端口是否可用由本次实际 bind 决定。仅 Ready 状态将 accept 结果送入 ForwardCore，其他阶段接受后立即关闭，且每轮 accept 有预算，防止连接突发饿死停止处理。

### 7.6 跳板适配

保留 `ssh_transport::forward(parent, host, port, cancel)` 的调用方式和错误归属：

1. 建立内部 stream pair，启动持有 parent 的 JumpDriver。
2. JumpDriver 用共享 core 打开到目标 SSH 端口的 channel，并与 pair 一端转发。
3. 通道真正打开后才返回 pair 的另一端作为子 SshConnection 的传输；等待期间支持取消和超时。
4. 子连接继续独立完成握手、指纹核对和认证。
5. 子连接持有 guard，释放时停止并回收 JumpDriver；父 Session 不交给子线程直接操作。

Unix 继续使用 UnixStream pair，Windows 继续使用仅用于内部配对的临时回环 socket，不增加用户可配置端口。guard 错误继续通过 transport_error 传播到具体跳板阶段。

## 8. 规则监督循环与状态机

Manager 持有规则注册表与轻量运行句柄；每个活动规则一个 RuleWorker。直连就绪时没有每客户端线程；连接尝试阶段临时增加一个 RouteJob，跳板路径还含一个 JumpDriver。

```text
Stopped/Failed
  → Starting（绑定 listener，启动 RouteJob）
  → WaitingForUser（如需交互）
  → Ready（创建 ForwardCore）

Ready → 网络故障 → Reconnecting → Ready
                             └→ NeedsAttention
NeedsAttention → 用户处理 → 新 RouteJob → Ready / NeedsAttention

首次启动失败 → Failed，并释放 listener
关闭自动重连的运行断线 → Stopped，记录连接中断
任意活动阶段 → Stop → Stopping → Stopped
```

监听器整个 run_id 生命周期归 RuleWorker 持有；重连只替换 RouteJob/ForwardCore，保留实际本地端口。run_stop、attempt_cancel 和 probe_cancel 分开：取消重新认证不会误释放仍需保留的监听器，取消 probe 不直接调用整个连接的 Cancellation。

窗口关闭以 ever_ready 判定：尚未首次成功的运行取消；已经成功过的运行继续后台重连。若正在交互，取消该次交互/连接尝试并回到 NeedsAttention。没有有效交互窗口时不排入隐形认证弹窗。

窗口加载/重载会更换 generation。原始首次启动与旧挑战按窗口失效规则取消；已运行隧道保留，新页面重新订阅快照。规则运行从不依赖 React useEffect 是否仍挂载。

### 8.1 启动时序

1. 校验调用窗口、配置 revision、应用未退出和服务器引用。
2. 在协调锁下取得规则/路径快照，登记 run_id，使重复启动返回同一运行。
3. 释放协调锁，工作线程直接绑定最终 listener；禁止“先找空闲端口再重新绑定”。
4. 启动 RouteJob，处理认证事件；同时监听循环响应停止及未就绪接入。
5. 完成后再次核对 run/attempt 仍有效，再转交 SshConnection 给 ForwardCore。
6. 原子发布 Ready 和实际地址；已经失效的成功结果立即关闭。

### 8.2 保存、应用和删除

普通保存先事务提交，再更新展示字段及 pendingChanges；不修改当前运行参数。底层服务器保存后刷新关联规则的路径修改提示，自动重连继续使用运行快照。

“停止并应用”是独立后端操作，携带 rule revision 和 expected run_id。后端固定本次要应用的快照并登记操作意图，停止旧运行后再启动；停止期间再次编辑/删除/手动停止会使旧应用意图失效。禁止前端简单拼接 stop + start 导致中间配置被换掉。

应用意图同时绑定发起窗口的 generation。新运行尚未首次就绪时窗口失效，取消后续启动；已经停止的旧运行不自动复活。保存配置仍保留，用户下次可以重新启动。

删除同样是一次带版本和运行归属的操作：需要停止则先完成收尾，再删除配置。停止失败保留规则及诊断。新配置启动失败保持失败状态，不自动回退到旧目标。

## 9. 并发、锁和退出

### 9.1 锁约定

- configuration 串行协调规则变更、服务器引用、启动意图和退出门禁。
- 服务器 DB、转发 Store 依次读取/提交，释放前一个锁后再取下一个；不同时持有两库连接去执行事务。
- 窗口 generation/closed 状态的短校验与运行登记采用固定顺序：窗口登记 → manager 注册表 → entry。
- 工作线程只短暂更新自身 entry，不在持有 entry 时反向取得 configuration、数据库或窗口锁。
- 发送 IPC、原生窗口操作、网络、认证等待、线程 join 均在上述锁外执行。
- 删除/停止先标记状态并取得取消句柄，再在锁外唤醒/取消资源。不会因网络超时阻塞整个服务器配置锁。

不得假定预先读取的 window generation 在登记时仍有效；登记关键区再次核对。窗口失效回调按旧 owner 取消，避免误取消同标签新一代窗口的操作。

### 9.2 退出

主窗口取得转发摘要时失败应显示错误，不能把未知状态当作零。摘要包含所有持有资源的规则，而不仅 Ready 数量。

quit_app 设置 stopping 后立刻请求所有转发停止，随后与既有资源收尾流程协调：停止 listener、取消 RouteJob、终止 channel、回收 worker 和跳板 guard、刷新通信账目，最后退出。无终端但存在规则时同样触发退出说明。

等待 worker 有截止时间，超时返回“尚未释放”供重试退出；不把 timeout 当成已成功清理。即使全局 stopping 因现有退出错误恢复，未收尾的规则仍保持 Stopping，不能再创建第二代运行。没有 socket/凭据的不可中断系统解析任务不构成无限退出等待。

## 10. IPC 契约

所有入口由 Rust 检查真实调用窗口，前端传入 window label 不作为权限依据。下表为拟定命令名：

| 命令 | 主要输入/输出 | 允许调用者 |
| --- | --- | --- |
| open_port_forwarding_window | 可选 serverId/sessionId → 聚焦或创建 | 主工作区、独立会话窗口 |
| port_forward_bootstrap | → 服务器选项、预选上下文、窗口会话信息 | 管理窗口 |
| subscribe_port_forwards | 状态 Channel + 认证 Channel → subscriptionId | 管理窗口 |
| unsubscribe_port_forwards | subscriptionId | 订阅所属窗口 |
| subscribe_port_forward_summary | Channel → 数量/需处理摘要 | 工作区 |
| ack_port_forward_frame | subscriptionId + epoch + revision | 订阅所属窗口 |
| port_forward_summary | → 退出所需活动规则/连接数 | 主窗口 |
| save_port_forward | 配置 + expectedRevision → 新规则版本 | 管理窗口 |
| start_port_forward | ruleId + expectedRevision → runId | 管理窗口 |
| stop_port_forward | ruleId + expectedRunId → 操作已受理 | 管理窗口 |
| apply_port_forward | ruleId + expectedRevision + expectedRunId | 管理窗口 |
| delete_port_forward | ruleId + expectedRevision + expectedRunId + stopActive | 管理窗口 |
| resume_port_forward | ruleId + expectedRunId → 新 attempt | 管理窗口 |
| answer_port_forward | rule/run/attempt/challenge + 回答 | 当前挑战所属窗口代次 |
| test_port_forward | ruleId + expectedRunId → probeId | 管理窗口 |
| cancel_port_forward_test | runId + probeId | 测试所属窗口 |
| port_forward_events | ruleId + runId → 最近事件 | 管理窗口 |
| open_port_forward_web | ruleId + expectedRunId | 管理窗口 |

“保存并启动”可以先保存再按返回 revision 启动；启动失败仍保留已保存配置。停止/应用/删除的最终结果通过状态流确认，IPC 已受理不等于已收尾。

打开网页的后端命令读取实际已就绪 listener 和已保存 scheme，自行构造 `http(s)://127.0.0.1:port/` 或 IPv6 形式；不接受任意 URL 或命令字符串。

后端统一验证端口范围、名称/目标长度、控制字符、回环地址和 scheme 枚举。IPv6 方括号只作输入规范化；目标域名不在本机预解析。连接错误使用单独的 ForwardError DTO，包含 kind、stage、hop、target、run/attempt 和时间；不把新的转发专用字段强加给所有既有 AppError 调用者。

### 10.1 快照与认证事件

```typescript
type ForwardFrame = {
  epoch: string;
  revision: string;
  rules: ForwardRuleView[]; // 已保存配置、运行快照、pendingChanges、错误和计数
};

type ForwardAuthEvent = {
  ruleId: string;
  runId: string;
  attemptId: string;
  windowGeneration: string;
  event: ConnectionPrompt; // 沿用现有密码/指纹展示类型
};
```

快照采用可合并的完整状态帧，订阅登记和首帧快照取自同一版本边界。前端按 epoch/revision 丢弃旧帧，重开窗口重新订阅，不根据陈旧本地状态创建运行。

每个订阅最多有一帧已发未确认状态，以及一帧可合并的最新待发送状态。前端接收并合入状态后通过 ack_port_forward_frame 确认，后端才发送下一帧；确认校验订阅归属和精确版本。高频流量更新约每秒一次，低频状态变更在通道可用时及时发布。确认超时注销观察者，重新订阅取得完整最新状态，不积压 Tauri 消息或影响后台运行。

认证挑战使用独立、可重放当前挑战的受控事件通道，不能被统计帧合并丢失。每个活动尝试最多一个挑战；多个规则同时需要认证时，前端一次展示一个，其余保留在有界待处理集合中。重复订阅或展示不延长原认证时限；挑战到期更新状态并从队列移除。

服务器配置变更使安全的服务器选项缓存失效，通过快照中的配置版本通知管理窗口重新取得 bootstrap 数据；刷新选项不覆盖未保存表单或运行快照。预选上下文只在明确入口请求时更新，不因周期刷新切换用户选择。

认证回答不会广播。订阅失败只释放该观察者；管理窗口失效再按生命周期规则处理启动/认证，不能因普通主窗口摘要订阅失败停止隧道。

## 11. 前端与窗口接入

- Root 增加 `?port-forwarding=1` 懒加载分支，使用现有 WindowErrorBoundary、WindowLoading、WindowTitlebar 和主题能力。
- 固定窗口标签 `port-forwarding`，窗口创建过程登记 pending 状态，创建失败回滚，重复入口聚焦已有窗口。重新创建同标签必须清理旧 closed 标记并递增 generation。
- App.tsx、ServerPickerDialog 和会话入口只添加打开动作/摘要，具体规则逻辑集中在新窗口。
- usePortForwards 保存后端快照、选择项和请求状态；表单草稿单独保存，不能被周期状态帧覆盖。
- PortForwardEditor 只使用安全服务器选项；不放宽现有 list_servers 的主窗口权限，新增 bootstrap 专用查询。
- 认证弹窗复用 LoginDialog/FingerprintDialog，但用独立控制器绑定 run/attempt/challenge；不套用以 TerminalPane 为拥有者的 useConnection。
- 地址、状态文案、按钮可用性集中在 portForwardPresentation，前端状态只用于展示，后端仍独立校验。
- 未保存表单关闭仍提示；真正关闭窗口后由后端窗口回调取消尚未首次成功的运行，不能仅依赖 React cleanup。
- 复制地址使用实际运行值，IPv6 带方括号；活动连接数和累计值来自后端，前端不估算业务字节。

## 12. 对既有模块的必要修改

| 文件/模块 | 具体修改范围 |
| --- | --- |
| lib.rs | State 字段、存储初始化、命令注册、窗口失效回调、服务器引用检查、退出收尾 |
| connection.rs + 新 auth/interaction | 通用 sink 和连接入口；原终端命令/事件兼容 |
| ssh_transport.rs + 新子模块 | shared core、jump 适配、句柄注册与有界解析 |
| traffic.rs | 新增 PortForward bucket，将固定容量统一为命名常量/枚举数量，更新总量计算和测试 |
| serverMonitoring.ts、ClientTrafficCard.tsx | 新增 portForwarding 分类；缺省数据兼容测试 fixture |
| App.tsx、ServerPickerDialog、会话入口 | 管理窗口入口、路由、摘要及退出说明 |
| QuitDialog.tsx | 明确包含端口转发及连接中断 |
| capabilities/desktop.json、Cargo.toml/lock | 新窗口与必要依赖；不开放泛用执行权限 |
| connection_tests.rs 及其他 State 测试构造 | 为新字段提供内存/临时 Store，避免访问真实应用目录 |
| package.json、Playwright 配置、测试 fixture | 新增针对转发的测试入口 |

转发账目继续挂到最终 SSH 身份，跳板外层不重复累计；每规则业务字节按成功写入目标 channel/本地 socket 计数。目标 probe 产生 SSH 开销但不计业务载荷。原四类通信保持原语义，合计包含新增类别。

## 13. 必须覆盖的竞争条件

| 交错事件 | 设计保障 |
| --- | --- |
| 连接成功与停止同时发生 | run/attempt 校验后才转交资源；停止优先使旧结果失效 |
| 重连计时到期与停止同时发生 | 独立 stop 标志、唤醒和入口门禁；不依赖普通队列顺序 |
| 同一规则两次启动 | 注册表原子登记，返回已有 run_id |
| 保存后另一个窗口再次编辑 | revision CAS；不能静默覆盖 |
| 停止并应用期间再次修改 | 固定应用意图及版本；失效后保留已停止状态并提示 |
| channel-open 挂起时本地客户端关闭 | 保留请求归属，迟到通道关闭；超时则淘汰会话 |
| 窗口关闭后旧认证回答到达 | window generation、attempt 和 challenge 联合校验 |
| OS 重用 fd/socket 数值 | 先注销再销毁，注册 token 与资源代次绑定 |
| 流量更新晚于新 run 启动 | 按 run_id 归属累计，不污染新运行 |
| 服务器删除与规则保存/启动 | configuration 下校验引用与登记，无网络等待 |
| Store 失败与活动隧道并存 | 保留活动资源，明确配置操作失败，不创建替代空库 |

## 14. 测试与验证计划

### 14.1 先验证的技术关口

G1：当前库版本的多 channel 调度。真实制造部分写入、远端窗口耗尽、pending open、read 引起的窗口调整、关闭和取消，证明其他流可继续且无串流/泄漏。

G2：非阻塞关闭与保活。验证正常关闭不会依赖 Drop 重试；验证空闲、黑洞、强制终止、SSH 保活挂起和平台 TCP 探测的真实行为。

G3：Windows/Unix readiness 与 stream pair。验证注册撤销、唤醒、重复启停和跳板嵌套不会造成空转或句柄复用错误。

上述验证在用户确认并进入实施后进行；本设计阶段只做了源码检查。如果发现必须更换 SSH 库、修改功能语义或引入额外底层补丁，应先更新方案，不静默扩大改造范围。

### 14.2 测试分层

- Rust：CAS 存储、状态转移、取消/迟到结果、引用检查、配额释放、窗口代次及半关闭缓冲。时钟、连接结果和故障注入集中于少量测试边界，避免真实 sleep 驱动所有状态测试。
- 真实协议：新增 `tests/port-forwarding/` 隔离 OpenSSH A/B 与 TCP/HTTP 服务；使用哈希/逐字节检查双向数据、并发和半关闭，真实验证 AllowTcpForwarding/PermitOpen、只允许转发账号、远端 DNS、密钥变化与断线。
- UI：新增 `port-forwarding-ui.spec.ts`、独立 Playwright 配置和 `test:port-forwarding` 脚本；模拟 IPC 验证表单、状态、待应用提示、认证竞争、复制地址和退出说明。
- 原生：Windows/macOS/Linux 验证监听、端口占用、IPv6、默认浏览器打开、窗口关闭/重开、Agent 能力、休眠和退出。
- 回归：现有 connection、database、resources、desktop、文件任务/日志相关用例按实际修改影响执行。

关键映射：A01–A03/A07/A08/A14 → 共享内核和真实 SSH；A04/A05 → 跳板兼容与生命周期隔离；A06 → listener；A09/A10/A11/A13 → worker/manager/window/store；A12 → probe/error；A15 → 资源和计量；A16 → 原生三平台。验收编号来自功能需求文档。

所有 fixture 使用隔离目录、端口和测试凭据；不读取用户服务器配置、不触碰业务服务器。内核协议验证不能由前端模拟替代，Windows 构建成功也不能替代 macOS/Linux 验收。

## 15. 实施批次与完成条件

| 批次 | 内容 | 完成条件 |
| --- | --- | --- |
| 1 | 验证 G1/G2/G3，抽取通用认证 sink | 得到库行为证据；现有认证用例兼容 |
| 2 | shared core 和跳板适配 | 原 forward 调用接入新 core；旧独立 pump 删除；跳板回归通过 |
| 3 | Store、Manager、RuleWorker 和无界面转发 | 真实直连/跳板、多连接、取消、重连及关闭验证通过 |
| 4 | 管理窗口、IPC、诊断、访问操作 | 全部产品操作可用，权限与状态竞态测试通过 |
| 5 | 通信统计、退出/引用整合、三平台验收 | 需求 A01–A16 有记录，未验证项明确列出而非视为通过 |

用户已授权在新目录和独立分支实施。当前代码及验证进度以 [实施与验证记录](port-forwarding-implementation.md) 为准，未完成的原生验收不视为通过。

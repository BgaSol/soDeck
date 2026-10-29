# 通信监控代码修改设计方案

> 2026-10-27 范围调整：用户要求 T-019 快速收尾，问题记录、知悉与通知留待后续独立规划。本文件相关完整设计保留，但不作为本次交付；当前有效范围与检查见 [T-019 r4](../tasks/T-019-connection-metrics.md)。

日期：2026-10-27 · 修订：r1 · 文档就绪：可评审，等待最终实施确认
任务：[T-019 r3](../tasks/T-019-connection-metrics.md)
功能：[实时计量r3](../core/connection-metrics.md)、[历史r2](../core/communication-history.md)、[诊断r2](../core/connection-quality.md)、[测速r2](../core/connection-speedtest.md)
界面：[通信监控r2](../design/communication-monitor.md) · 公共依赖：[T-016](../tasks/T-016-observability.md)

## 1. 本次确认覆盖什么

交付完整的Windows客户端通信监控：底部摘要、单实例工具窗、来源/路径、跨启动历史/比较/导出、连接事件与去重提醒、可选SSH响应探测、主动上下行测速。沿用功能方案的默认值、核心/插件边界与40项功能流程。

本稿进一步固定代码职责、版本与契约、底层适配、数据升级、资源预算、先行实验和正式交付关口。用户最终确认覆盖本稿、关联规格修订、实验/测试代码、实现和独立Windows开发目录包；不包含Git提交/推送、正式发布或使用日常业务服务器测试。本轮仅编写文档，以下标“新增”的文件/接口/脚本均尚未建立。

两项需要在最终确认时一并知悉的具体影响：

1. 为统一调用审计与事件存储，核心数据库schema由4升至5，新观测数据集schema为1。遵循P-52：升级旧库时提示、完整备份旧库及凭据后创建新库；**不自动迁移服务器档案、已保存凭据、信任记录、数据库设置与旧调用记录**。插件独立文件和独立外观偏好按其当前格式保留，但可能引用旧服务器ID，不能自动挂接同名新服务器。新建隔离开发数据不影响日常目录。
2. 对当前锁定的ssh2/libssh2增加范围明确的本地适配：无正文的字节观察和诊断channel请求；加密、认证、主机指纹及SSH线协议保持原行为。先通过E1/E2验证才进入功能接入，不用估算数据或伪RTT替代。

## 2. 已核实的代码基线与改动清单

2026-10-27只读基线：`v2`，HEAD `fc533e8`，本轮开始时工作区干净。应用dev.14、API/SDK4.8.0、UI1.4.0、终端0.8.0、文件0.6.0、网络0.2.2；核心schema4。用户此前快照已包含T-019功能文档，原“存在大量未提交修改”是历史情况。本轮不创建提交。

源码证据：`Core::record`当前向core.sqlite的calls写入，保留7天；`network::Group`累计listener接入侧字节；`ssh/remote.rs`按lease维护独立SFTP子系统；SSH I/O由所属连接线程串行驱动；`Session::keepalive_send`只返回下次发送间隔。SDK在验证state通知前先匹配subscriptionId/feedEpoch，能够隔离新旧订阅；Hub单快照32KiB。主IPC窗口入口目前只认主/工作区/独立视图窗口，新工具窗必须正式注册。

所有路径相对仓库根；下表给出修改目标，不要求一次把全文件重写。

| 位置 | 修改/新增内容与职责 |
| --- | --- |
| `third-party/libssh2-sys/`（新增）、`Cargo.toml` / `Cargo.lock` | vendor锁定0.3.3源码及libssh2子树，保留许可证/上游来源/补丁说明；本地构建不回退到未打补丁的系统库 |
| `third-party/ssh2/src/{session,channel,sftp}.rs`、新增`metrics.rs` | 安全封装字节观察上下文、channel身份/SFTP底层channel及诊断请求状态；管理FFI生命周期，不引用SoDeck模型 |
| `crates/core/src/traffic/`（新增） | `model/registry/counters/sampler/aggregate/query/services`：权威归属、计数、1秒快照、查询与预算 |
| `crates/core/src/ssh.rs`、`ssh/{transport,remote,network}.rs` | 在现有单I/O所有者处注册/结算原语与路径，接入探测/测速驱动；不在WebView统计 |
| `crates/core/src/quality/`（新增） | `probe/scheduler/rules/services`：带响应探测、阈值与连接领域事件；问题生命周期调用公共观测模块 |
| `crates/core/src/speedtest/`（新增） | `task/protocol/driver/adapter/services`及`agents/{memory.py,memory.ps1}`：有界短任务/双向协议/静态远端模板 |
| `crates/core/src/observability/`（新增） | `schema/store/writer/events/calls/history/issues/evidence/health`：公共事实、审计、问题、证据、存储治理；不按通信业务另建私有日志 |
| `crates/core/src/{lib,store,store_bootstrap,route_services,remote_read}.rs` | Core生命周期/方法分派、审计单源、数据集预检重建、归属与连接阶段采集；保留原公开DTO |
| `contracts/{communication-metrics,communication-history,connection-quality,connection-speedtest,observability}-v1.schema.json`（新增） | 请求/结果/错误/类型及核心内置事件目录的唯一来源；另扩展state-feed的受控contract分支 |
| `scripts/generate-traffic.mjs`（新增）、`generate-communication.mjs`、`contracts/capabilities.rs` | Rust/TS生成与检查、API4.9能力声明；不手改生成产物 |
| `packages/sdk/src/{traffic,traffic-client,quality,speedtest,observability}.ts`（新增）、`state.ts` | 运行时校验、BigInt计数、订阅目标匹配/取消、查询/任务客户端；不依赖UI |
| `crates/plugin-host/src/dispatcher.rs` | 新短调用分类、查询/任务专属有界队列与取消保留通道；不把长历史/测速塞进现有控制线程 |
| `apps/desktop/src-tauri/src/{monitor_window,monitor_services,monitor_navigation}.rs`（新增） | 窗口注册、可信来源、范围定位、保存目标令牌及认证续接；无字节统计逻辑 |
| `app.rs` / `surface_endpoint.rs` / `communication_services.rs` / `application_menu.rs` / `window_services.rs` | 新角色验证、state分发、菜单/退出/主题/取消；现有窗口关闭语义保留 |
| `apps/desktop/src/communication-monitor/`（新增） | `root/overview/history/diagnostics/speedtest/settings/client`：四分区、上下文与查询状态；共用SDK/UI |
| `apps/desktop/src/{main,app,connection-status,auth-dialog}.tsx`及应用菜单模块 | 新工具窗渲染入口、底部速率、认证组件的可复用回调；不自动创建服务器工作区/首终端 |
| `packages/ui/src/{metrics,charts}/`（新增）、公共tokens与主题surfaces | 无SDK的MetricPair/TrendChart/ChartLegend/DataCoverage，主题令牌与图表可访问性 |
| 三插件及其manifest | 正确计数无需插件升级；只有通用资源用途标签/定位需要补齐时改SDK调用。该变化另按下文新版本验证；不改代理协议或终端/文件业务 |
| `tests/core/`、`tests/ui/`、`tests/desktop/`、开发打包脚本 | 下文明确的新测试/实验/Windows包入口，最终确认后建立 |

## 3. 底层计量：在实际I/O边界计数

### 3.1 本地libssh2适配方案

锁定`libssh2-sys 0.3.3`（Cargo.lock校验值已有），其头文件版本文本是`1.11.1_DEV`，不虚称正式1.11.1发布。vendor完整可构建源码，记录原包校验值/许可证和窄补丁diff；强制使用此静态库，禁止pkg-config/vcpkg绕过补丁。保留当前OpenSSL构建路径，不换SSH库。

新增观察API名称使用`sodeck_`前缀并只在本地适配公开：会话观察上下文、当前channel创建归属token、channel身份查询、计数终态。观察回调只有稳定ID、token、方向、字节数、生命周期类型；**没有buffer指针、内容或命令字符串**。

- 对外流：在`LIBSSH2_SEND/RECV`对应的会话socket读写包装处，成功返回正数才累加。保留原callback的errno/EAGAIN语义；不统计`LIBSSH2_SEND_FD/RECV_FD`的SSH agent等其他fd。握手banner/认证/保活也覆盖。
- 通道：在`_libssh2_channel_read/_write`的实际正返回/成功数据分支计数，覆盖stdout/stderr及SFTP内部调用。原调用重入/EAGAIN不重复记；关闭/释放前发最终结算。库内部discard行为按已计成功读取的边界记录，不把未读缓冲当已交付。
- 子系统：通过已有`libssh2_sftp_get_channel`查询对应身份；SFTP初始化前设置可信创建token，元数据和初始化交换不会漏记。每lease现有独立SFTP子系统作为归因资源，不伪装能把共用SFTP批量响应精确分配到某次目录命令。
- 桥：内层SSH会话和direct-tcpip中转channel标为`carrier`，用于路径证据但不进入通道业务合计；仅真正连出本机的最外层socket标`external`。

观察上下文拥有独立字段，不复用`session.abstract`；ssh2现有keyboard-interactive会临时替换abstract，复用会造成类型混淆。Rust持有固定生命周期的计数单元，FFI回调只作有界slot查找/原子加法，无分配、锁等待、数据库、RPC或panic穿越FFI；C层无SoDeck业务依赖。最后一个channel/session销毁前观察上下文仍有效，Drop只释放自有资源。

数据热路径只更新计数；单调时间与采样在Rust所有者/采样线程，不能每包读数据库。资源创建时绑定来源token，channel-open现有仲裁保持一次只推进一个在途开通；重入同一SFTP/open操作恢复同token，不能让其他挂起任务串归属。失败初始化及关闭尾包也要结算到原token。

### 3.2 归属、快照与去重

`TrafficOwner`固定包含bootId、目标serverId/配置epoch、workspaceGeneration（可空）、可信pluginId/instanceId、resourceKind、resourceId/generation、routeRunId；`Carrier`另含parentSessionId与externalTransportId。用户可改的标题只是定位信息，不参与主键，不允许插件改写计数身份。

`traffic::registry`拥有对象/路径，业务线程拥有计数器；每秒在各I/O所有者发布计数读数并带observedAt，采样器形成一个samplerRevision。全局/服务器/来源由同一读数集合算出，累计等式成立；明确最多一个采样周期的读取时差，不声称物理上同时采样。

全局通道总量按最终目标/资源相加，transport按externalTransportId去重。共享传输不强行摊到每个服务器；显示相关目标列表。每个资源关闭先提交final counter再撤销，保留按boot/目标/插件合并的已结束量。绑定表满时仍计已知目标/来源的“其他”，完整性变partial；未知目标退入全局“未归属”并明确合计口径，最终正常工作负载验收必须没有此退化。

高基数预算：同时细分256个目标、4096个资源、8192个原语计数slot；超出不拒绝业务连接，先合并明细，连目标预算也超出则聚合“其他目标”，返回coverage和受限数量。每目标≤1KiB、资源≤512B安全元数据；标题不保存远端正文。

实时32MiB分为计数/身份≤4MiB、环数据≤24MiB、索引/临时汇总≤4MiB。1秒数据优先保障全局和活跃目标，再资源；预算不足按明确5秒/1分钟聚合与逐级明细降级，保留字节和最细可知峰值。与CM-10的16服务器/64资源负载相比不能提前降级；每次查询返回实际分辨率与retainedRange。

速率基于单调时间差；睡眠通过系统恢复标记和墙钟/单调漂移检测切段（阈值2秒），缺口不插值、不将尾差压入1秒。u64溢出用checked/saturating加法并标invalid，不回卷。JSON字节/序号统一无前导零十进制字符串，SDK用BigInt；SQLite字节累计用8字节无符号大端值或分片增量解码合并，不转有符号INTEGER后溢出。

## 4. SSH响应探测实现

不采用`keepalive_send`返回值或GLOBAL_REQUEST无标识响应作为RTT。源码显示channel请求使用`_packet_requirev`按本地channel ID匹配SUCCESS/FAILURE，可作为隔离测量基础。

方法固定为`ssh-channel-response-v1`：在已有已认证会话上开一个不启动shell/exec/subsystem的诊断session channel，发固定扩展请求`sodeck-response@probe.invalid`，want-reply=1。正常SSH服务器对未知请求返回CHANNEL_FAILURE；**此拒绝正是有效响应**，不能显示“探测失败”。对应SUCCESS也视为收到响应并记录回复类型，任何其他错误不作成功。服务器拒绝创建诊断channel时显示不可用，不改用业务PTY。

libssh2窄增量提供`probe_send/receive/cancel`非阻塞状态机：send完成后先返回Sent，再由Rust记录Instant并调用receive，剔除开channel/本机排队时间；响应仅匹配此channel且无其他请求在途。每次≤128B请求、≤16B有效回复（SSH封装另计）。空载调度仍泵入站包，保活回复不能匹配成探测回复。

同channel串行复用成功请求；**任何超时或取消后立即退役该channel，下一次必须用新ID**，防止迟到无requestId的响应误中后续探测。关闭排空有2秒本地预算，未清理的channel计入总量并停止该连接后续探测；不能为清理诊断而强制关闭承载业务的session。原正在发送的libssh2缓冲必须先续进或随连接结束释放，不能交叉替换。

沿用5秒周期/3秒超时/全局8在途与8次/秒；只观察现有消费者连接，不持最后租约，不自动重连。单次检查绑定操作/窗口代次，持续开关属于服务器偏好；工作区/连接终结撤销。中位数/p95/波动和触发恢复规则以F-CORE-025为准，事件归核心诊断。

## 5. 测速的执行器与二进制协议

### 5.1 任务/连接与认证

独立`SpeedTestTask`全App最多1个，任务状态为preparing/authRequired/warming/sending/verifying/receiving/succeeded/partial/failed/canceling/canceled/interrupted。操作ID+可信调用方+请求hash确定回执；重复相同ID返回原task，不同参数返回CONFLICT；活跃/终态回执至少保留本次运行，历史关闭也不允许重复启动负载。

核心内部引入`ConnectionOwner::Workspace`与`ConnectionOwner::Task`，提取现有公共建连/逐跳认证流程，旧公开连接接口不变。App级测速可以没有工作区；禁止伪造workspaceId或自动开工作区来借用首终端。若从工作区启动，记录originWorkspaceGeneration并在其结束时取消；全局启动仅受App/任务/目标生命周期约束。

固定输入serverId、路径快照token、预期routeRevision、方向和档位；开始前校验快照未被替换。选“当前运行路径”必须还能引用该真实连接快照及有效凭据，无法复现则明确不可用，不能悄悄改走新配置。配置中途变动不改变已锁路径。

创建purpose=`diagnostic-speedtest`的独立整条SSH链（复用建连算法，单独租约/预算），各层协商压缩none，记录实际方法；不改变业务连接设置。相同host/凭据可复用可信配置，不复用业务数据channel。认证挑战以taskId/attemptGeneration关联；`speedtest.resume`只在现有AuthDialog保存信任/凭据后续接，取消不留下新的会话。后台任务不等待UI线程。

页面`prepare`只查本机已知/过期能力与路径预览，不远程执行；真正能力验证在用户点开始后、通过认证的准备阶段进行。查能力返回unknown不伪装支持；支持范围仍为Linux已有Python3、Windows已有PowerShell5.1/7，缺能力明确unsupported。

### 5.2 内存agent与时限

固定版本模板随核心打包：Linux以`python3 -I -u -c`执行固定代码；Windows以`powershell.exe/pwsh -NoProfile -NonInteractive -EncodedCommand`承载固定模板，直接使用Console二进制标准流。业务字符串、主机名或路径不拼入脚本，参数通过下述有界帧传入；不添加ExecutionPolicy Bypass，不安装工具、不写远端文件。Windows模板不得使用会生成临时程序集的Add-Type。

Python使用标准库二进制I/O、SHA-256与独立watchdog线程；PowerShell使用.NET Stream/IncrementalHash（或同等流式SHA256）及独立runspace watchdog，避免依赖被阻塞的主runspace事件循环。每任务独立60秒硬截止；超时终止本测试进程，不能扫描/kill其他进程。模板在Python版本/PowerShell兼容性不满足时明确拒绝。

实施细化（已确认范围内）：Windows默认cmd的命令长度有限，EncodedCommand只承载固定短loader；loader按编译期长度从stdin读入≤16KiB的固定脚本后在内存执行，后续stdin继续二进制协议。该初始化字节计入控制及实际通道量，不计测速DATA；不使用用户输入生成脚本或文件。预热至多占每方向字节额度的一半，为高速链路保留正式阶段数据；总上限及时间窗保持。

低可压缩度数据使用标准库/OS随机源分块生成，所有SSH层压缩none；分块≤64KiB、每方向数据环≤2MiB，额外工作内存≤16MiB。数据生成与校验开销属于方法条件；不伪称测得纯链路容量。两个方向顺序执行，标准/省流档及预热/时间/字节上限沿用F-CORE-026。

### 5.3 Wire v1与测量公式

专属exec不分配PTY，stdout/stdin用二进制帧：固定16B头为magic(4B)/version(1B)/type(1B)/phase(1B)/flags(1B)/sequence(4B)/payloadLength(4B)，多字节整数使用网络字节序；payload≤64KiB，控制payload≤4KiB、全任务控制负载≤64KiB。保留flags必须为0；非预期帧/越序/超长度/溢出/重复冲突立即停止。stderr单独读，最多8KiB安全错误摘要，不解析为协议，不保存原脚本或正文。

消息为HELLO/CONFIG/READY、PHASE_BEGIN、DATA、ACK、PHASE_END、RESULT、CANCEL。上传receiver每250ms至多回一个累计ACK，结束必须回RESULT（确收字节、SHA256、phase）；下载client同样校验。ACK只报告接收进度，不表示终态校验成功。DATA帧头为协议开销，不计256MiB/32MiB测试负载上限，但计入真实通道流量；64KiB“控制负载上限”不包含每个DATA帧头，界面和结果明示开销。

预热/正式使用独立phase与SHA256；切phase要收到结束回执，不能把预热尾巴算正式数据。上传正式均速=已验证正式payload字节 / 客户端首个正式DATA写入动作至最终RESULT接收完成的单调时长；下载=本地已验证正式字节 / 发出正式阶段请求至校验完成的时长。包含排队/背压/回执/校验，附辅助传输时长，禁止把服务器未同步墙钟相减。

上传1秒峰值来自实际成功写入channel的DATA增量，明确标“发送过程峰值”；下载峰值来自本地成功读出的DATA增量，不能将批量ACK当瞬时收到速度。终态校验失败时过程曲线仍可查看但所有性能结果带unverified，不能进入默认成功结果比较。总RESULT丢失为partial；已排队数据不冒称远端确认。

本机1秒内停发；关闭测试channel/整条专属链唤醒阻塞，正常远端5秒内退出；控制失联由远端watchdog保证最多60秒。本机终态分别报告localStopped/remoteExitConfirmed，不把远端无法确认写成清理成功。取消、窗口关闭、退出、杀进程及两个方向边界必须实测。

## 6. 公共观测依赖与数据库设计

### 6.1 本任务实现的T-016公共子集

实现并公开：类型化内置事件/调用事实、稳定event/trace引用、事件查询/统计、问题聚合/确认/恢复、有限快照/通信指标查询、固定水位证据包/本地导出、留存/健康/启动重建。首批领域采集覆盖连接、通道/租约起止、隧道组起止、测速/探测、通信设置、历史清理和存储健康。

T-016中任意插件自描述日志注册、console/stderr全面接管、可靠历史events.open/ack/close feed、所有插件业务事件目录及AI消费插件仍由原任务后续交付。本任务不发布这些尚未实现的能力位；当前实时用Hub、时间线按固定水位query+revision失效通知更新。公共模块、schema与对象模型同源，后续增加来源无需另换私有数据库。T-016整体仍不能标工程完成。

调用审计一次性切换到公共事实库的call.started/finished+calls投影（同事务）；`calls.list`保留原字段/过滤/游标结构，retentionDays从7改30是明确行为变化。旧core.calls不双写或复制；网络stream开始/结束也走同一CallStore，阶段事实关联callId/traceId，不每帧记录。

新控制调用仍先持久化审计开始，失败不执行；结果写失败返回RESULT_UNKNOWN+operation/task回执查询入口，禁止自动重做测速/清理。取消/结束/退出/健康/已发生任务的结果核对保留豁免路径；观测查询自身审计不进入自动问题规则或触发证据生成，消除反馈环。

### 6.2 数据集与表

路径为独立数据根下`core/`和`observability/`；物理分区共享StoreEpoch和类型目录，不对插件开放文件路径。一个有界writer线程串行提交事实，查询最多2个只读worker；不持连接/工作台锁等磁盘。具体表/索引：

| 库 / 在线额度 | 表与关键索引 |
| --- | --- |
| `facts.sqlite` / 256MiB | meta/storeEpoch/提交sequence；events(eventId,sequence,kind,type,source,boot,target,resource,trace,outcome,payload)；calls兼容投影；operation_receipts；按sequence、target+time、trace、callId索引，64MiB审计保留 |
| `logs.sqlite` / 128MiB | 公共普通日志分区保留；本任务仅需要的有限运行健康事实优先写facts，不因此实现所有插件日志入口；没有生产者时不预占大文件 |
| `traffic.sqlite` / 96MiB | identities/path_segments、buckets_5s/1m/1h、resource_totals、quality_buckets、speedtest_results、gaps；主键含epoch/boot/owner/layer/bucket；时间+目标复合索引 |
| `diagnostics.sqlite` / 32MiB | issues/episodes/issue_changes、projection_checkpoint、snapshots、evidence_jobs/manifests、notification_receipts；issue聚合键唯一，投影和checkpoint同事务 |

临时证据文件也计32MiB诊断额度；50MiB是公共单次输出上限而非预留内存或保证可成功生成50MiB缓存包。直接显式CSV导出流式写用户目标，证据包需要自包含缓存时容量不足返回CAPACITY/明确partial，不能越过在线预算。紧急通道另外8MiB；用户已选导出与版本备份单独报告，不自动删除。

事实到问题的跨库投影不是跨库原子事务：facts先提交，diagnostics按序消费并同事务更新问题/检查点；查询有projectedThrough/pending，崩溃幂等重放。主库不可写时健康写紧急通道，不能递归创建自身问题。证据固定各分区水位并报告读取时差/缺口，不冒称跨库瞬时一致。

保留时间及粒度按F-CORE-024；每桶保存byteDelta、validDuration、sampleCount/gaps、最细可知峰值/粒度。源桶与下一层汇总在traffic同事务替换/登记覆盖，禁止多粒度重复。5秒batch持久化，退出最多2秒排空；尾部不确定区间保留gap。

每库额度含main/WAL/临时：创建时4KiB页、incremental auto_vacuum，main≤80%额度、WAL/回收预留20%；设置max_page_count、禁用长查询事务、写前估算/分批≤256KiB、90%前清理与checkpoint。轮转先细粒度再汇总，保留活动任务最小状态；无法腾空间按数据类别拒绝/降级。禁止用VACUUM整库副本或超大WAL突破预算；CH-04/G4实测物理峰值。

### 6.3 schema4→5与一致备份重建

core schema5移除旧calls物理权威表，保留服务器/凭据/设置的当前建库结构；新增观测数据集schema1。不提供旧数据增量迁移。启动按进程锁→枚举core/观测各库版本→一次提示完整影响→统一备份清单/日志→验证备份→初始化新套→发布→启动服务。

扩展`store_bootstrap`而不是另开一套reset逻辑：日志版本2保存每成员相对路径、旧/新版本、原文件集合、备份hash、发布状态；旧日志版本1用窄只读适配恢复到原已承诺状态后再预检，不将此视作数据库内容迁移。不接受任意路径/软链接越界。SQLite backup读取WAL一致快照，secrets逐文件校验；在全部备份验证前不退役任何旧库。

首次由4升级：备份并重建core+新观测套，旧服务器/凭据/信任/设置/调用不导入；插件store、插件包及外观独立文件保留。只观测库未来版本不兼容时可仅重建观测成员，core同版本保留；只core重建但观测版本相同时保留历史并换配置epoch，旧目标只读且不关联新同名ID。

损坏库、备份失败、hash不符、发布中断停止正常启动并保留可恢复旧套；中断按日志前进恢复，不混合半新半旧服务。降级旧客户端不允许无提示打开新数据；开发包使用新隔离数据根验证，正式对日常目录升级须以同一P-52提示执行。

## 7. 对外API、类型与兼容

目标API/SDK为4.9.0，capabilities增量为`communication.metrics.v1`、`communication.history.v1`、`communication.quality.v1`、`communication.speedtest.v1`、`observability.query.v1`、`observability.issues.v1`、`observability.evidence.v1`、`resources.labels.v1`。能力位仅在全部对应真实实现可用时发布；某台远端不支持测速是prepare结果，不撤销整个产品能力位。

### 7.1 契约与方法清单

共用`Target = all | server(serverId) | resource(resourceId,generation)`、`Layer = channel | transport`、`Range = live | [fromUtc,toUtc)`；所有目标含configEpoch，临时资源另含bootId。schema拒绝未知字段/非法组合；u64字符串长度≤20且按范围校验，不能接受NaN/Infinity/负字节。客户端revision是缓存依据而非权限或可执行资源句柄。

| 方法 | 关键输入 → 结果 / 语义 |
| --- | --- |
| `communication.summary` | target/layer → revision、observedRange、up/down rate/counters、state、coverage、共享关系计数 |
| `communication.sources` / `communication.connections` | target/layer/sort/cursor → items、totals/other、snapshotId、nextCursor、expiresAt；分页固定同一读数集 |
| `communication.history.query` / `.compare` | target、时间/粒度/来源筛选 → 桶/统计/峰值粒度/缺口、水位、nextCursor；比较条件显式返回 |
| `communication.policy.get/set` | expectedRevision+单位/历史/阈值/提醒偏好 → 新revision；并发冲突不覆盖 |
| `communication.history.cleanup.start/get/cancel` | operationId+时间范围 → jobId、已删除范围、终态；确认在UI完成，核心仍校验参数/边界 |
| `quality.summary` / `quality.check.start/get/cancel` / `quality.policy.get/set` | 目标/预期路径/操作 → 方法、样本、状态/错误、影响及探测任务；单次与持续分开 |
| `speedtest.prepare` / `.start/get/resume/cancel/results` | 路径token、档位、方向、operationId → 任务/认证挑战/结果/完整性；prepare不远程执行 |
| `events.query/get/aggregate` / `issues.query/get/acknowledge` | 公共过滤、水位/游标、expectedRevision → 事实/问题/投影水位/缺口；旧calls.list为兼容投影 |
| `evidence.create/get/cancel/export` / `communication.export.start/get/cancel` | 固定范围/格式/可选本机目标token → job、manifest、partial/gaps、输出结果；不上传 |
| `observability.health.get` / `.policy.get` | 当前磁盘/队列、watermark/storeEpoch、预算、失败/丢弃、留存范围；无故障递归 |
| `resources.labels.set` | 调用者自己拥有的handle/generation、受限kind/opaqueKey/liveLabel/定位描述 → revision；不能变更权威归属 |

通用数据错误统一：INVALID_PARAMS、TARGET_ENDED、STALE_GENERATION、CONFLICT、CURSOR_EXPIRED、STORE_EPOCH_CHANGED、STORE_UNAVAILABLE、CAPACITY、CANCELED；协议/探测/测速专属错误沿用功能规格。失败信息包含安全stage/errorCode和可重试性质，不返回原命令/原始异常对象。

来源冻结查询最多8个、每个≤512KiB、TTL5分钟，总4MiB；单快照最多1024个资源明细，超限明确other/coverage，累计不丢。来源页默认50、历史页默认100，均最大200、普通响应≤512KiB；历史最多1200曲线点/30天，查询最长2秒。大结果走受控export job，不用无限分页冻结全内存。

### 7.2 类型化订阅复用现有Hub

扩展已注册contract：`communication.summary.v1`、`communication.quality.v1`、`communication.speedtest.v1`、`observability.issues.v1`；全部App scope含appGeneration，objectId为规范化target或taskId。`state.get/open/ack/close`和现有Hub握手/背压保持，32KiB单快照不扩容；大列表/历史只带revision/摘要，按需分页拉取。

更新SDK的state scope/object匹配为显式contract表；保留旧各分支规则。旧4.8订阅只收到自己的原契约，unknown新contract不会推给它；新增分支不改变旧schema的必需字段。owner采用窗口代次或可信插件实例/表面代次；关闭释放owner，旧通知先按subscriptionId/feedEpoch丢弃。

core sampler每秒发布轻量版本通知，宿主现有pump仅搬运最新快照，不执行历史SQL或大聚合。视图隐藏可退订，采集仍继续。所有公开订阅控制逐次审计；内部sample/update/投影不走公共RPC，查询审计不会自动触发问题/证据，避免自激循环。

## 8. UI、原生窗口与插件接入

### 8.1 专用窗口有正式身份

新增`MonitorWindowRegistry`持有固定label `communication-monitor`及每次创建generation，不把它塞入服务器工作区数组。`app.rs`的core_call来源校验、endpoint_window/caller、主题订阅、窗口控制/缩放、on_window_event及退出遍历统一识别此注册对象；不能只凭label前缀放行任意窗口。

`ui.communicationMonitor.open`为公开宿主动作，登记opId、target、generation和可返回的原焦点；创建在主线程，重复打开聚焦并提交新的导航revision。入口给scope后，旧异步查询无法覆盖新scope。设置`window.__SODECK_MONITOR_CONTEXT__`作为渲染分支提示，真实身份仍由原生IPC绑定，不相信JS声明的windowId。

主界面底部复用ConnectionStatus附近布局；主菜单、托盘恢复和无独占标题行遵守现有规范。监控窗关闭不走“结束工作区”，先取消单次检查/当前测速，再撤销订阅和窗口；本机CSV导出明确为后台任务继续。最小化不取消。明确App退出按先停新任务→取消探测/测速→关SSH→结算统计→2秒排空→关闭全部窗口执行。

摘要/曲线每秒接最新快照；来源/服务器列表第一页每5秒换一份一致快照。光标位于表格、键盘焦点在行内或翻页浏览期间固定该快照，显示采样时间及“刷新”，不在点击时换行。离开交互1秒后第一页恢复下一轮刷新；翻页保持至显式刷新，游标过期提示重新加载。排序末级使用稳定ID，选中项以resourceId/generation保持，资源结束给明确状态，不把选择移到同位置的新行。

认证复用`AuthDialog`与核心挑战字段，提取UI参数/回调而非拷贝第二套认证逻辑；测速的onReady调用speedtest.resume，原终端/隧道继续commands.retry。任务/窗口已结束时关闭对话框并拒绝迟到提交，不能再次自动发起连接。

原生保存选择器由宿主注册一次性输出token（含选择范围/路径/可覆盖结果，10分钟过期）；前端/插件不能把任意字符串路径传给核心当导出目的地。导出后台持有经用户选择的writer句柄/授权目标；临时写/原子完成/取消清理由该job负责，重启不复用旧token。

### 8.2 公共UI和三主题

沿用已读apple-design：即时按下反馈、稳定数字几何、清晰层级、可中断查询/任务、焦点返回与减少动态。公开UI新增MetricPair、TrendChart、ChartLegend、DataCoverage；图形用SVG（每图≤1200点），无额外图表库，图表同时提供键盘游标及文字表格。

普通曲线2px、键盘选中点3px轮廓；发送为实线、接收为虚线并附方向文字，高对比用系统色/不同线型，不能仅靠色；不平滑缺口/不曲线插值抹掉峰值。坐标单位随视图固定，数字格式由SDK无损字节工具提供；UI只接数据、单位、回调。

主题新增语义角色`trafficSend/trafficReceive/trafficGap/trafficEvent`，唯一定义在themes/surfaces并由tokens生成，具体推荐值见[主题规范T-019增量](../standards/theme.md)。新角色不扩展旧presentation.v2必需颜色字段；UI使用静态公共profile变量消费，旧插件主题DTO保持兼容。新公共UI1.5.0，保留现有exports和组件行为。

### 8.3 插件改动最小且可独立更新

被动计数从核心原语自动得到，旧插件照样覆盖。终端PTY→现有view、文件lease→文件view、网络group→网络view的基础定位由核心已有资源关系给出；补充业务名称通过`resources.labels.set`，只有该句柄所有者可描述，view定位必须经宿主核验。liveLabel≤128字符、opaqueKey≤128B；持久历史默认保存类型/稳定key，不持久自由名字或路径。

本批准备终端0.8.1、文件0.6.1、网络0.2.3，分别只补受控资源描述/定位；若规则详情需要直达，网络插件声明带ruleId schema的公开视图命令，由用户点击后宿主路由，不变成插件互调。原bytesRead/bytesWritten及代理UI含义保持，统一监控读新口径字段。

新包先检查resources.labels能力，不支持则跳过增强；清楚写入兼容范围，不强制所有用户更新插件才能有基础统计。原0.8.0/0.6.0/0.2.2的签名包作为回归夹具。插件独立更新仍重启App生效，三插件新增描述不能要求核心每次识别私有协议字段。

## 9. 调度、总预算与失败隔离

| 资源 | 本批硬预算 / 行为 |
| --- | --- |
| 被动计量/实时环 | 32MiB；256目标/4096资源/8192原语slot；超明细预算聚合且声明覆盖 |
| 来源冻结查询 | 4MiB（8×512KiB），5分钟；过期游标显式失败 |
| 观测写入队列 | 8MiB，其中1MiB审计/控制保留；紧急内存1MiB，独立紧急磁盘8MiB |
| 历史查询 | 2 worker、待队列16、单查询2秒/1200点/512KiB；取消不等磁盘长锁 |
| 新Hub负载 | 仍在既有8MiB存储/8MiB交付总额内，新监控最多2MiB快照、2MiB待交付子额；单快照32KiB |
| 探测 | 8在途/8次每秒，每连接1次；待退役channel也计预算；观测额外状态≤1MiB |
| 测速 | 1任务、≤16MiB额外工作区，现有SSH/桥内存依原公共限制并单独计量 |
| 证据/导出 | 2并发、每任务≤2MiB流式缓冲，缓存文件纳入32MiB诊断盘额度 |
| 线程 | 新增1采样、1writer、2query、最多2export、1测速调度；探测在已有SSH owner推进，远端watchdog单独记录 |

这些是分区额度，不能将它们分别满足后就声称进程总开销32MiB。合计新增显式工作内存上限约71MiB（含Hub新增4MiB子额，不含已存在SSH/运行时），测试同时报告RSS/私有内存/线程的实际增量；当未测速/未导出时按需释放缓冲。代码实现应以分配字节和容量而非只看元素数守预算。

取消/任务get/健康/订阅close走保留控制lane，历史SQL/导出不在`dispatcher`3个short线程中等待。每次I/O循环的统计更新固定开销、探测推进有步数限制；业务ready先行，测速自己的连接不饿死主连接。CM-10基准：同环境观测开/关三轮中位吞吐≥95%，控制p95≤200ms，数据校验一致；不可通过跳过审计或扩大无界队列达标。

## 10. 先行实验、失败门槛与实施顺序

最终确认包含以下有界实验和在成功条件下的采用，不需要每过一个实验再问一次；涉及换SSH库、部署agent、改变兼容平台、计量含义或时限/预算时先改方案并确认。实验失败不能把对应产品能力标完成，独立文档与其他不依赖工作可继续。

| 实验 | 候选/准备与操作 | 采用条件 / 失败处理 / 清理 |
| --- | --- | --- |
| E1 真实计量 | 本稿libssh2窄补丁；隔离PTY/SFTP readdir/stat/exec/direct-tcpip、部分写/EAGAIN、多跳、认证取消、100次建销；与同边界受控计数核对 | 已知输入准确、无重复/正文、SFTP初始化/关闭无缺失、回调终止安全、CM-10门槛可达；失败限于同方案修复，不退成JSON长度估算。停止夹具、检查线程/句柄/计数slot归零 |
| E2 SSH响应 | 已有Linux与Windows OpenSSH，未知channel请求返回FAILURE，延迟/失联/晚到回复/保活交错/MaxSessions受限、无PTY业务时读泵 | 与注入延迟相符，失败回复当有效响应，新旧channel不串，取消不关业务；正常OpenSSH必须可用，受限服务器显示不可用。异常不能通过改成ping绕过；清诊断channel/开关 |
| E3 内存测速 | Python3/PowerShell5.1与7二进制模板；大于缓冲数据、EOF、摘要错、两向高速触顶、停读、控制失联、客户端强杀 | 所有负载/时间上限、回执校验、1秒本地停发、正常5秒远端退出与60秒硬截止均满足；无临时文件/监听/长期进程。任一必需方法失败保留失败证据，不悄悄换磁盘或安装工具；确认远端进程已结束 |

实施顺序：

1. 固定schema/生成/版本和E1～E3实验，完成可采用证据；不把未验证实验样例放入发布能力位。
2. 建立公共观测存储/CallStore/备份重建和可信事件；完成异常恢复关口后接入真实原语计量。
3. 交付核心聚合/公开查询/状态订阅、兼容原包，再实现摘要/监控窗/公共图表。
4. 实现历史/问题/证据/策略；接入诊断探测与测速任务及认证/取消，补三插件描述。
5. 按各功能和下文关口验证，提供Windows独立目录包、结果文档和用户验收入口；所有必需关口通过后才进入待验收。

## 11. 代码专属验证、现有回归与交付

功能的CM12+CH9+CQ9+CSPEED10共40项仍全部执行。以下14项MCODE补充实现风险，不替代功能流程；测试代码在确认后建立，当前未运行。

| 编号 | 准备/步骤 | 必须观察到的结果 | 清理 |
| --- | --- | --- | --- |
| MCODE-01 | 对schema生成TS/Rust，u64最大值/错单位/未知字段/旧DTO输入及旧SDK订阅 | 无损或明确拒绝、生成校验通过、原包不解析新contract | 关订阅 |
| MCODE-02 | E1及FFI边界：auth abstract临时替换、SFTP失败初始化、Drop/并发取消 | 计量不串、无panic穿FFI/UAF、正文不入观察、无漏slot | 销毁隔离session |
| MCODE-03 | 同revision多层汇总，超256目标/4096资源及关闭尾包 | 合计可核对，other/coverage明确，旧代次不污染 | 停资源核对基线 |
| MCODE-04 | E2及超时channel晚到响应、占满MaxSessions、持续开关/退出 | 无伪RTT、无互相应答、受限可见、不重连或关业务 | 排空诊断channel |
| MCODE-05 | E3、畸形帧/超序/超预算/摘要错/不同压缩链 | 不越界、不假成功、无输出文本混帧，方法条件明确 | 核查远端进程 |
| MCODE-06 | 无工作区测速、当前旧路径/新路径选择、认证弹窗关闭、任务回执结果不明重试 | 无伪工作区/新终端、固定路由、挑战与回执幂等、取消后不复活 | 关专属链/凭据副本 |
| MCODE-07 | 在call开始/领域提交/终态/问题投影各边界强杀/写失败 | 审计单源，执行前失败不执行，结果未知可查询，投影幂等且pending可见 | 恢复写入/测试库 |
| MCODE-08 | schema4升级5，WAL/凭据/多库备份中断与重启，旧reset日志v1 | 一次影响提示、全部备份校验、无内容迁移、无半新旧套/源数据丢失 | 留脱敏证据后回收副本 |
| MCODE-09 | 轮转/查询/证据并发，满main/WAL/紧急额度/磁盘、拒绝长事务 | ≤声明物理预算，gap/partial真实，清理/停止仍可执行，无递归错误风暴 | 撤销配额故障 |
| MCODE-10 | 双窗口快速打开/关闭监控，伪label/旧generation，睡眠唤醒 | 单实例、可信IPC/主题源、正确路由与缺口，100次无泄漏 | 关闭测试窗 |
| MCODE-11 | 原3插件签名包与新包分别加载；只更新一个插件后重启，伪描述他人资源 | 旧业务/计量可用，描述不改归属，重启更新行为及边界保持 | 恢复包配置 |
| MCODE-12 | 三主题、新图表令牌、1200点/窄窗/缩放/高对比/键盘/原生菜单焦点 | 曲线有文字等价，单位/缺口可辨，公共UI无SDK，旧主题DTO兼容 | 恢复系统偏好 |
| MCODE-13 | 同硬件64资源10分钟、慢状态/历史读者、同时测速/导出、100次关闭 | 各内存/线程/磁盘预算及CM-10成立，取消p95/停止时限达标，无业务饥饿 | 停负载核对资源 |
| MCODE-14 | 新Windows目录包从空数据和旧库副本启动，更新插件/托盘/明确退出/再开历史 | 首终端/文件/代理原流程不回归，目录包资源完整，历史可查但不恢复工作现场 | 退出并回收隔离数据 |

拟建立：`crates/core/tests/traffic.rs`、`observability.rs`、`quality.rs`、`speedtest.rs`、`tests/core/traffic_fixture.py`、`tests/ui/communication-monitor.spec.ts`及`tests/desktop/communication-{monitor,history,quality,speedtest,lifecycle,compatibility,benchmark}.mjs`；E1～E3复用这些夹具与测试模块，不建一套无人维护的产品原型。`scripts/package-communication-dev.mjs`负责本批包及证据清单，新增专属命令在实现时写入开发入口；以上目前不是可运行命令。

已有检查按实影响选取：`npm run check`、`npm run test:contracts`、`npm run test:core`、`npm run build`；浏览器运行受影响Playwright集合；Windows目录构建用既有desktop:build。必须真实验证OpenSSH Linux、Windows和多跳；Python协议夹具不能冒充真实服务器，浏览器也不能冒充原生窗口。

回归重点：终端输入/目录联动，文件SFTP/OpenWrt降级，固定转发/HTTP/SOCKS数据正确性及原NT-08门槛，跳板认证/关闭，调用记录分页，P-52恢复，三主题与窗口焦点。修改底层SSH后需重跑真实原语与吞吐相关集合；通过且无新变化后不无理由重复所有历史测试。

本批版本建议：应用`2.0.0-dev.15`、API/SDK4.9.0、UI1.5.0及三插件补丁版本；构建时若其他任务占用应用dev号顺延并记录实际值，语义API和schema不得静默改变。生成独立Windows目录包（不替代安装验证）；记录源码基线/工作区、工具版本、运行方式、实测证据、限制与5步用户验收。安装/签名/正式发布另按授权，不在目录包通过后自行发布。

## 12. 本轮文档检查与确认状态

本稿只读核实并提出可执行技术路线；E1～E3尚未编码或运行，不能称为已验证采用。代码文件、schema、agent和测试清单均为计划。文档链接/编号/修订及实际检查结果统一记入[T-019](../tasks/T-019-connection-metrics.md)。用户最终确认后，按照本稿顺序持续完成实现、测试、自查和开发包，不为已确认范围内的普通实现细节反复请求确认。

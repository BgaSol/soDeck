# 核心事件与诊断：代码修改设计

日期：2026-10-28 · 修订：r3 · 状态：已确认实施；工程验证中

r1完整方案已由用户明确批准“可以开始做了”。r2只调整性能验收：用户2026-10-27明确选择“移到后续优化；本期确保正常操作、容量上限和异常隔离”，将OCODE-12的128订阅极限延迟/吞吐目标转后续；保留原始测试和失败数据，本期不再继续为该极限目标优化。正常三插件操作、各预算上限和异常时业务/取消/关闭隔离仍须验证。下方“拟实施/当前版本”为r1设计基线，实际实现与结果以任务记录为准。

r3实施细化：历史统计按HQ-03补固定耗时桶/样本范围，远端资源事实移至真实原生分配点，退出先结束导出任务再停writer；均为原确认语义的实现补齐，未扩大产品范围。

任务：[T-016 r6](../tasks/T-016-observability.md) · 产品依据：[P-53、P-52](../product.md) · 功能来源：[核心及插件接入](../core/observability/integration.md)

## 1. 本次交付边界

在当前 API/SDK 4.9.0、业务库 schema 5、观测库 schema 1 上扩展。复用已有调用审计、四分区存储、连接阶段与通信历史，不另建插件日志数据库。以下是拟实施方案，模块名和版本不代表现在已经存在。

一次确认覆盖以下完整交付，分阶段实施不等于只交付第一阶段：

1. 核心/外壳/宿主结构化事件与六级日志、来源身份、操作/阶段关联、采集类型目录。
2. 终端、文件、网络隧道三个独立插件的业务采集、主动快照和有限指标。
3. v2 历史查询、统计、操作时间线，以及可补读、有缺口说明的历史订阅。
4. 本地现场采集任务、自包含证据、公开读取和显式本机导出。
5. 版本化健康/策略、debug 自动到期、留存清理、容量保护、启动备份重建及失败恢复。
6. Windows 真实插件/SSH/原生通信验证和独立开发目录包；使用公开 SDK 的验收读取夹具与脱敏证据。

问题聚合/知悉/通知（F-CORE-020）、AI 插件、模型调用、云端上传、主动测速、自动修复不在本次实施范围。重要事件本身照常记录。保留 T-019 收束结果，不恢复通信监控中已撤下的入口。本次不新增日志查看页、设置页或插件诊断布局；既有 UI 只在语义动作处理处传上下文，证据导出复用原生保存对话框。用户可见的新诊断界面另行设计；本稿没有新 UI 视觉布局；原生交互按仓库要求已读取并使用 apple-design 技能，具体应用见§5.4及§8。

## 2. 已核实的实现与改动理由

| 当前实际位置 | 现状 | 本次修改 |
| --- | --- | --- |
| `crates/core/src/observability/{writer,calls,events,query,services,schema}.rs` | 单写入所有者；调用事实和 calls 投影同事务；已有窄查询；四分区合计 512 MiB | 保留所有者和旧投影，加入完整信封、类型校验、跨分区顺序、附加数据和服务 |
| `crates/core/src/observability/calls.rs::AuditScope` | 线程局部 call/trace，部分 trace 借用 operationId | 引入不可变诊断上下文；旧业务 operationId 单独保留；跨队列显式 capture/resume |
| `apps/desktop/src-tauri/src/app.rs`、`surface_endpoint.rs`、`plugin_registry.rs` | 身份由通道确定，WebView RPC 有请求去重；worker 调用没有诊断 metadata | 增加可信上下文入口和宿主转交；去重指纹包含诊断元数据 |
| `crates/plugin-host/src/{process.rs,bootstrap.js,dispatcher.rs}` | 回调只有 method/params；invoke 两个参数；console 为空；32 个待处理 core 调用 | 协商 metadata，扩展回调/invoke 上下文，增加有界采集调度与 console 摘要 |
| `crates/plugin-manager/src/package.rs`、`scripts/pack-plugin.mjs` | manifest 严格字段；包文件清单签名/哈希验证 | 新增可选描述引用，构建复制描述、验包与注册；不执行描述中的代码 |
| `packages/sdk/src/{index,worker,observability}.ts` | 已有 WebView/无 DOM worker client；observability 仅旧契约调用封装 | 增加无 DOM 生产者/消费者、上下文绑定、批处理及降级回执 |
| 三插件 worker 和业务状态机 | 终端直接调用 sodeck；文件有异步受理；隧道有 200 条内存诊断 | 在实际状态与完成边界插入采集，保持独立构建/更新 |
| `crates/core/src/store_bootstrap/dataset.rs` | 已可只重建观测集合；任一观测分区版本不符即替换四分区 | 沿用集合恢复机制，补 schema 2、观测独立重建及降级回滚验证 |

当前存在的 issues/evidence 表不是已实现服务；旧 `diagnostics.get` 是已有运行诊断入口，不能改成新任务查询。

## 3. 契约、版本与包描述

### 3.1 版本和兼容

| 项目 | 当前 → 本次拟定 |
| --- | --- |
| 核心公开 API / SDK | 4.9.0 → 4.10.0；RPC 外层 apiVersion 仍为 1 |
| 开发目录包 | 2.0.0-dev.15 → 2.0.0-dev.16；不含提交、推送或发布授权 |
| 终端 / 文件 / 网络隧道 manifest 版本 | 0.8.0 → 0.9.0 / 0.6.0 → 0.7.0 / 0.2.2 → 0.3.0 |
| 三个新包要求 | pluginApi `^4.10.0`，保留旧能力要求并增加 events/context/snapshots/metrics 四组 |
| 数据 | core schema 保持 5；四个观测分区统一 schema 2；插件业务 dataSchemaVersion 不因采集而变化 |

编码前再次核对工作区版本；若其他已授权任务已占用上述发行编号，顺延未发布编号并记录，不因此改变接口语义或重新要求功能授权。旧签名插件无需重新打包，可在新核心运行，coverage 为 audit-only；新包在旧核心因不支持的 API/manifest/capability 被拒绝。旧核心严格解析器可能先报包格式不兼容，不能承诺它能识别新能力名称。

能力分组：`observability.events.v2`、`observability.context.v1`、`observability.snapshots.v1`、`observability.metrics.v1`、`observability.history-feed.v1`、`observability.evidence.v1`，另设 `observability.management.v2` 承载新版健康/策略/清理。分组是兼容发现，不是插件权限；只对完成实现和验证的组发布能力位，交付包应完成本稿全部七组。

### 3.2 单一契约来源与方法

新增 `contracts/observability-v2.schema.json`、`observability-descriptor-v1.schema.json`、`diagnostic-context-v1.schema.json`。前者维护请求/响应/通知及公共信封；后两者维护包描述和传输上下文。新增 `scripts/generate-observability.mjs` 生成 Rust 校验数据与 SDK 类型，加入 `npm run check` 的生成一致性检查。当前 `observability-v1.schema.json` 及其生成链保留，不交给两个生成器重复写同一产物。

| 组 | 新公开方法/通知 | 返回及边界 |
| --- | --- | --- |
| 发现/采集 | `observability.describe`、`events.types.list/get`、`events.v2.appendBatch` | 限额/覆盖/描述；批次逐项 persisted/duplicate/filtered/rejected，提交成功才给 persisted |
| 操作 | `traces.begin/stage/end/get`、`observability.targets.resolve` | 不透明 contextRef、只读资源 ref、有限时间线；不执行业务 |
| 读取 | `events.v2.query/get/aggregate` | 固定水位、分页、实际留存、缺口；旧 events.*、calls.list 保持原 DTO/筛选语义 |
| 补读 | `events.v2.open/ack/close`；`events.v2.batch/gap/closed` | 单未确认批次、至少一次；独立于允许合并的 state feed |
| 现场/指标 | `diagnostics.snapshots.report/query/get`、`metrics.report/query` | 已上报安全数据；无插件私有回调、无额外远端命令 |
| 采集任务 | `diagnostics.capture`、`diagnostics.capture.get/cancel` | 返回 jobId；避免覆盖旧 diagnostics.get |
| 证据 | `evidence.create/get/cancel/export` | 自包含记录/描述/清单；get 支持有界分页；export 显式调用原生保存目标选择 |
| 管理 | `observability.v2.health.get`、`observability.v2.policy.get/set`、`events.v2.cleanup.start/get/cancel` | 新 DTO 不扩写旧 strict health/policy；管理操作仍逐次审计 |

清理方法名固定为 `events.v2.cleanup.start`、`events.v2.cleanup.get`、`events.v2.cleanup.cancel`（不存在额外的 `events.v2.cleanup` 方法）。其他表中的斜线仅为列举缩写。

旧事件查询仍读 facts 中原语义的投影，不突然混入 logs 全量记录或扩充严格枚举；新增完整数据用 v2。SDK 校验器按明确的方法集合选择 schema，不能将当前“未知观测接口即报错”的 validator 无条件串入所有业务方法。服务返回同样验证；未知字段和状态组合拒绝。

### 3.3 插件包目录

manifest 新增可选字段，示例仅在本文，确认后才创建真实文件：

```json
{"observability":{"descriptorVersion":1,"path":"diagnostics/observability.json"}}
```

包描述顶层为 `schemaVersion:1`、`pluginId`、`events`、`actions`、`snapshots`、`metrics`；各项带稳定类型、typeVersion、字段/单位/枚举、默认级别、messageKey 与采集分类。快照/指标保留插件规格的短名称，在注册表使用 pluginId + 类别 + 名称 + typeVersion 作为完整键；事件使用 `plugin.<pluginId>.*`。不得跨类别/版本抢占类型。同一类型版本定义不同内容为冲突，修改字段契约必须升 typeVersion。

描述文件最多 1 MiB；四类合计最多 128 项，每项最多 8 KiB；最大嵌套深度 3、每对象 32 字段、数组最多 32 元素、枚举最多 32 项，字符串显式长度且最大 256 字符。仅标量/枚举/有限数组/浅对象，不支持脚本、远程 $ref、正则或任意错误对象。字段必须声明数据分类，首批仅安全 ID、枚举、布尔和有界数值等白名单；不得以“安全文本”承载任意路径/正文。类型说明文字属于包内静态描述。

`pack-plugin.mjs` 构建时把源 `diagnostics/observability.json` 复制到 dist；使用外部 `--source` 时校验已包含正确文件。描述与 manifest 一起进入现有 integrity 清单及签名。manager 在暂存验证时读有界文件、校验身份/命名空间/能力依赖；坏包不替换当前版本。启动时先注册当前类型再 activate，历史类型副本与首次事件在其所在分区同事务持久化；存储失效不误报包不兼容，不触发业务包热回退。注册表超额标 coverage degraded，显式拒绝新增采集，旧历史仍可从盘读取。

## 4. 上下文如何穿过 UI、宿主和 worker

新增纯 Rust 值类型 `DiagnosticContext` 与有界上下文登记表，放在核心观测模块；来源身份继续使用既有 Caller 和 surface endpoint。元数据上限 1 KiB，固定形状：

```json
{"metadata":{"diagnostics":{"version":1,"contextRef":"opaque-core-reference"}}}
```

`RpcRequest`、host core frame、host invoke frame 使用相同可选 metadata。插件不能在其中填 source/verified/traceId 等权威字段。contextRef由核心校验封装boot/实例/端点/阶段/到期信息，配合登记表检查；即使登记已到期，也能区分合法过期引用与伪造引用，不能把任意未知字符串当作可静默忽略的上下文。WebView 初始上下文增加支持版本；worker initialize/ready 增加 `diagnosticContextVersion:1` 协商，未协商时不发送新增字段。宿主内 `CoreCallback` 改收 method、params、可选 metadata；`plugin.invoke(command, params, invocation)` 的第三参数为只读调用上下文，旧 JS worker 可忽略第三参数。

UI 动作调用 SDK 的 beginAction，得到不可变绑定客户端；调用自己的 `plugin.invoke` 时由宿主校验 UI/插件实例/工作区/代次，并签发绑定到同一 worker 的转交引用。worker 从第三参数构建 client，显式传入排队闭包和子任务。外壳命令本来已有宿主 action 时继承，不重复创建“用户点击”；插件自报用户行为始终 reported，只有外壳拥有的明确用户命令为 verified。worker 可完成宿主转交给自己的业务阶段；不得结束核心所有者阶段或其他实例的根操作。

Rust 内部 capture/resume 传完整上下文；短同步作用域仍可使用 RAII，跨线程/延迟任务必须携带值。call_start 保存关联，call_finish 从持久调用上下文取回，不能依赖结束时线程局部值。业务 operationId、callId、actionId、spanId 独立；旧连接路径继续保留 operationId，不能换成诊断 ID 破坏取消/重试。

WebView 请求去重键保留原 caller/source/requestId，指纹改为规范化 method + params + metadata；同 requestId 换上下文拒绝 REQUEST_CONFLICT。业务执行回执仍按原 operationId 与业务参数识别，不因新的诊断上下文重复执行。诊断重送使用新 requestId、原 clientEventId，因此每次 RPC 留痕、领域事实去重。

不合法 metadata 形状或跨实例伪造关联按请求校验拒绝；合法引用因超时/移窗过期时，业务 RPC 继续执行并标 associationMissing，不能为补日志改变业务结果。直接调用 traces/append 的旧 context 则返回 STALE_CONTEXT；SDK 不自动重放业务。beginAction 暂不可用时包装器以 detached/未关联模式继续业务，保留失败计数，不伪造一个已持久化 action。

trace 最长 30 分钟、无活动 10 分钟；每实例 256、App 4096 个活动 span，受总内存更小限额约束。实例结束标中断；单个 surface 结束只撤销其转交前的引用和 feed，已转交 worker 的动作按业务所有者继续或取消，不把移窗等同于业务结束。诊断失效不取消业务，长寿命会话以资源引用串起后续独立操作。

## 5. 核心模块与持久化

### 5.1 模块职责

继续放在 `crates/core/src/observability/`，按职责新增模块，避免把所有分支追加到 services.rs：

| 模块（拟新增，既有另标） | 职责 |
| --- | --- |
| `model.rs`、`catalog.rs`、`validation.rs` | 完整信封、静态核心目录/包目录、白名单与有限字段校验 |
| `context.rs`、`targets.rs` | 操作/阶段、所有者/转交、TTL、诊断资源映射；资源结束不删除历史引用 |
| `ingest.rs`、`sequence.rs` | 配额/批次逐项处理、幂等、提交顺序与回执 |
| `history.rs`、`feed.rs` | v2 合并查询/统计、持久进度及有界交付；旧 events.rs 保持兼容投影 |
| `snapshots.rs`、`metrics.rs` | 最新现场/持久副本、有限序列聚合、sampleId 去重 |
| `capture.rs`、`evidence.rs` | 有界只读任务、包清单/内容/缺口、取消/中断 |
| `policy.rs`、`retention.rs` | CAS 策略、debug TTL、分区轮转、清理任务及完整性标记 |
| 既有 `writer.rs`、`calls.rs`、`services.rs`、`schema.rs` | 单写者、调用事实/投影、版本化分发、schema 2；不另开旁路写库 |

桌面新增 `observation_services.rs`，负责原生 source、feed 推送、宿主动作和保存对话框适配；核心可独立测试，不依赖 Tauri 或插件业务实现。核心公开方法仍统一经 record 审计，内部采集/投影不递归走公共 RPC。

### 5.2 schema 2 与顺序

四分区保持 facts 256 / logs 128 / traffic 96 / diagnostics 32 MiB，总 512 MiB，紧急通道 8 MiB 另计。facts 的 64 MiB 审计保留和 FULL/WAL、main 80% 上限继续有效。

- **facts**：扩展事件列和索引：level/typeVersion、producer/clientEventId、occurred/received/committed、完整 source、关联/错误码/安全字段。新增 action/span 投影、策略、类型描述、上下文最小恢复状态、保留边界；calls 和事实同事务，operation_receipts 保持业务用途。
- **logs**：采用同一完整信封，保存 kind=log；独立索引/去重与类型副本。普通日志仅在此保存，不在 facts 复制正文。log 的 error/fatal 不自动变成另一份领域事实。
- **traffic**：保留 T-019 表、采样与计量语义；仅随集合版本重建，不增加插件业务字节聚合。
- **diagnostics**：最新快照/固定副本、指标分钟桶/去重、采集/证据任务和证据分块；不启用遗留 issues 表。完成包作为分块 BLOB 留在此库，避免额外管理一个不在备份清单内的缓存目录。

常用查询建时间/sequence、source+type、trace、call、operation、target+generation 等有界索引；targets 用有限关联表，所有值参数化。保留时间依 receivedAt/核心时间，不让插件伪造 occurredAt 延长留存。外部 sequence 为十进制字符串，内部检查有符号 64 位溢出。

全局 sequence 由现有唯一 writer 统一分配：在 facts 的独立短事务预留 1024 个号的区间，再依实际串行提交顺序分配给 facts/logs 事务；不从两个库各自的 meta.sequence 自增。号段未能持久预留则不能用；进程重启跳过预留区间剩余号。单分区事务内写事件、去重摘要、类型副本和本分区已提交水位，提交成功后才发布事件/更新内存总水位。调用事实及 calls 投影继续原子。号段留下的空号不是缺失事实，不能靠 sequence 不连续推断丢数据。

writer 不交错执行未完成的跨分区子提交。混合批次按分区分别提交，允许部分成功并逐项回执，绝不宣称跨 SQLite/WAL 原子。崩溃后从两分区已提交水位恢复可读水位，同时从预留上界分配新号，避免晚写入小于已返回水位的记录。旧查询使用 facts 已提交水位；v2 查询在总水位内合并短读，并复核各分区 pruneRevision。

append 的去重键为 pluginInstance/producerEpoch + clientEventId，内容哈希不含每次接收时间；writer 在两个事件分区检查键与内容，防止换 type/kind 绕过去重。唯一索引和事件同事务；相同返回原引用，冲突拒绝，回执丢失可查询核对。记录轮转后不维护无界墓碑，describe/回执明确去重保留范围，不承诺超出该范围的恰好一次。

v2 eventId 为核心生成、带持久 store 密钥校验的不可解析引用，绑定 epoch/partition/sequence/随机量；对当前 epoch 的有效已发引用，记录已淘汰返回 GONE。无效/未知引用为 NOT_FOUND，其他 epoch 为 STORE_EPOCH_CHANGED。旧 UUID 事件投影无足够证明时保持 legacyPartial/未知存在性，不伪造 GONE。此校验服务正确性，不提供抗本机篡改保证。

### 5.3 查询、补读与反馈环

复用 query.rs 的两个只读 worker 和 16 个待处理任务；扩展为一次有界任务内顺序读多个分区，每段短事务，不跨页持锁。v2 每页最多 200 条/256 KiB、最多 30 天/200 组、2 秒截止，排队及解码同计时。页游标固定过滤指纹、epoch、总水位、各 pruneRevision、最后 sequence，5 分钟过期；清理冲突立即返回 CURSOR_EXPIRED，不返回貌似完整的半页。

feed 独立于 state feed：open 暂停 → 安装监听 → ack 激活 → 读取已提交批次 → ack 前最多保留一个批次。每批最多 100 条/64 KiB，单实例 16、App 128 个订阅，但实际填充同时受 512 KiB/实例、8 MiB/App 的更小发送额度约束，其余只保留磁盘进度。无每订阅线程；公共调度器公平轮询，慢消费者不锁住生产者。

历史页游标与补读 checkpoint 是两种对象。checkpoint 使用持久 store 密钥封装 epoch/规范化过滤/已确认 sequence，不依赖进程临时 cursor_key，可跨 App 重启补读；新连接重新绑定实际 caller。握手 10 秒、未 ack 60 秒关闭；轮转/重建明确 gap，继续需显式接受可用起点。acked checkpoint 由消费者保存，核心不冒充已保存用户消费进度。

默认排除 kind=call；即便显式订阅调用事件，也固定排除所有诊断生产/查询/控制/管理方法的 call 审计，以及这些方法仅为记录自身而产生的常规任务进度事件。覆盖 events/traces/observability/metrics/diagnostics/evidence、旧 diagnostics.get/calls.list 及既有通信观测查询；策略显式变更/真实存储故障仍可投递，但不能因读健康再产生新的健康事件。排除表为集中元数据并进入 effectiveFilter，不用零散字符串分支。所有被排除的调用仍可历史查询。真正业务操作相关的 action/domain 不因这条规则消失。

### 5.4 快照、指标与证据

快照 report 校验 producerRevision；同修订同内容返回原回执、冲突拒绝、倒退显式 ignored。只存安全投影，每来源 16 目标/32 KiB/5 秒；常态更新覆盖最新值，显式 capture 把当时版本固定成不可变副本。5 分钟未更新 stale，最长声明 1 小时；实例退出标 ended。report 的 storedSnapshotId 可查询该最新记录，版本被替换时明确 superseded；固定副本不被覆盖。报告持久化完成后才返回 stored。

指标每来源最多 32 种/128 序列，标签最多 4 个有限枚举；10 秒窗口聚合，保留 15 分钟有限内存环，1 分钟桶持久化。SDK 每序列每窗口一份最终样本，counter 为区间 delta，gauge 为窗口最后值，histogram 桶界来自描述。相同 sampleId/内容去重，不同内容冲突；每序列窗口防止换 sampleId 重复结算。持久样本回执/聚合与去重同事务，再更新内存；崩溃丢失 10 秒细节时声明覆盖不足，不重复累计已确认 delta。接受迟到上报最多 2 分钟，超窗口明确拒绝/缺口。更早分钟桶查询不补零。2分钟迟到窗口关闭后可合并/回收sample去重条目，以持久窗口结算边界拒绝旧样本，不在32MiB分区保存无限逐样本墓碑。

capture/evidence 最多共享 2 个活跃任务；create 返回 queued，状态 running/succeeded/partial/failed/canceled/interrupted。显式起点为 trace/event 或时间/资源范围，默认围绕锚点前后各 5 分钟，本期不依赖 issueId。capture 只复制已有安全现场；同来源/范围 5 分钟内的自动故障捕获合并，全 App 每分钟最多 10 次，由事实所有者触发，不建立问题引擎。

证据固定事件水位与各快照/指标版本，分批复制最多 5 分钟；不长期持读事务，清理冲突返回 partial/失败。包含 manifest、事件/调用/阶段、类型描述、快照、指标、版本、引用及 coverage/gaps；数据来自多个时刻，清单如实列出。最多 10000 事件、50 MiB 逻辑输出上限；实际缓存受 diagnostics 32 MiB 中剩余额度约束，预留失败即 CAPACITY 或请求允许的 partial。

包用 JSONL 自包含记录，64 KiB 分块，manifest/内容摘要固定后只读；get 分页读取。取消清理未完成分块，原始数据保留；重启把未完成任务置 interrupted，回收预留，不自动重做远端操作。导出由桌面适配层使用已有 rfd 原生保存框选目标，后台创建目标旁临时文件，完成后不覆盖式发布；用户取消不影响已有包，失败保留原目标并清理临时文件。Windows临时输出采用可在进程退出时删除的文件句柄语义，提交成功才解除删除并发布；强杀导出时也验证无残留半成品，不依赖退出钩子一定执行。公共参数只接受包 ID 和格式，不接受任意文件路径。每次显式 export 才打开保存框；两任务共用 4 MiB 缓冲，无隐式网络发送。export立即返回jobId（kind=export），通过evidence.get/cancel读取/取消，选位置期间phase=selectingDestination，避免把用户选文件的时间卡在35秒RPC内。任务仍计入2个活跃额度，但选位置时不占数据复制worker/缓冲。

原生交互应用apple-design的即时反馈、可取消、来源一致和平台适配原则：保存框绑定经过宿主确认的来源窗口，后台没有可绑定surface时返回UI_REQUIRED，不随意弹到另一插件窗口；同窗口最多一个保存框。Esc/取消只取消本次导出，保留已生成证据，关闭后焦点返回触发控件或其有效父视图；原surface失效时不向新surface发送结果。通过现有任务回执区分选位置、写入、完成/失败，不能在选好路径时就宣称导出成功。使用Windows原生字体/键盘/高对比和系统动效，不增加CSS、过渡或新的确认弹窗。DE-04与OCODE-09覆盖取消、焦点、父窗口关闭及200%缩放/键盘操作。

### 5.5 健康、策略与清理

新版 health 返回各来源覆盖、配额/丢弃原因、实际留存范围、队列/分区/备份占用、策略版本与采集状态；旧 health/policy 继续兼容投影。策略保存在 facts，set 要求 expectedRevision，冲突拒绝；可改普通日志阈值/来源 debug 到期、较短留存，不能关闭调用/操作/关键事实、扩大固定在线预算或启用正文。debug/trace 最多 24 小时，重启仍按到期恢复。

清理 start 接受明确 source/time/kind 范围及期望 storeEpoch，返回 jobId；每 App 一个清理任务，32 条一批让出 writer；取消仅停止后续批次，报告已删除量/partial。活动调用最小状态、活动 span 和任务必要状态受保护但全部计入预算，结束/中断后可回收。清理完成/部分完成的管理事实最后写入；写入也失败时健康明确报告管理尾部缺口。证据不无限钉住原始记录，保留失败可以 partial。

所有分区在 90% 之前维护并预留 WAL/索引/放大空间；扩展当前仅 facts 的压力清理到 logs/diagnostics，并保持 traffic 专用汇总策略。批量淘汰同步去重/类型副本引用计数与 pruneRevision。正常新业务仍遵循“审计开始不可写则不执行”；附加采集失败只退化采集。取消/close/stop/ack/health/get 等现有豁免及新增对应清理控制路径在一个方法表维护，写盘失败不能妨碍释放资源。

## 6. 宿主与 SDK 的资源、退出和安全字段

`@sodeck/sdk/observability` 扩展已有入口；内部拆 `observation-client.ts`、`observation-context.ts`、`observation-feed.ts`。生产者提供声明类型的 logger/event、beginAction/stage/end、snapshot/metric、flush/dispose；消费者提供 describe/query/trace/subscribe/capture/evidence。底层传入 transport、时钟、随机 ID 与调度适配，不在 QuickJS 路径依赖 window/Node/crypto 全局。保留旧 callObservability 导出。

UI 和 worker 不假装共享 JS 内存：宿主按插件实例分配有界采集信用，worker 与多个 surface 合计 256 KiB；SDK 只有获得信用才缓冲，端点消失回收信用。宿主统一串行准入事件/快照/指标上报，每实例最多一个执行中的采集请求，其他端点在各自已获信用内等待。250ms 批量、error/关键终态立即唤醒，32 项/128 KiB、单事件 8 KiB、200 条/秒突发 400；关键事实不采样但超容量仍如实拒绝。业务控制 lane 不被诊断借用。

超时/暂时容量的诊断重送最多 100/300/1000ms 三次、原 clientEventId；明确拒绝/过滤不重送。flush 最多 1 秒，App 退出全局总排空 2 秒，不能每个插件顺序等 2 秒。先停止业务生产/释放资源，再排空已接收安全项；stop 后不开放新生产，强杀未提交尾部明确丢失。

宿主新增诊断专用有界 dispatch（2 worker、最多 32 项及 2 MiB），避免大量查询/上报占住既有 net/state 短调用池；ack/close/cancel 保留既有控制通道。feed 同时扣现有 process writer 的 4 MiB 总额/32 KiB 控制保留，不能再复制一套无账目的通知队列。

以下是整个公共观测子系统的**受控缓冲账目**，含复用额度，不等于整个 App RSS；编码时按排队、在执行、序列化副本合计扣账并记录实际堆峰值：

| 缓冲/缓存 | 全 App 上限 |
| --- | --- |
| 所有 SDK 生产者信用（含 worker/UI） | 8 MiB；每实例合计 256 KiB |
| 现有 writer 入站/执行 | 8 MiB，内含 1 MiB 审计/控制保留，不再另加 1 MiB |
| 紧急内存环 | 1 MiB |
| 当前类型目录 | 4 MiB |
| context/对象映射/覆盖与任务元数据 | 2 MiB；数量上限和字节上限取小者 |
| 最新快照 | 8 MiB |
| 通用指标序列、10 秒环和去重缓存 | 8 MiB；超额拒绝新序列/样本并标缺口 |
| 历史 feed 的发送/未确认批次 | 8 MiB；属于已有宿主发送额度，引用共享，避免双算复制 |
| 诊断 dispatch / 查询输出 | 2 MiB / 2 MiB |
| 两个 capture/evidence/export 任务缓冲 | 合计 4 MiB |
| 现有 4 写连接 + 2×4 读连接 SQLite page cache | 合计 1.5 MiB（每连接 128 KiB），SQLite 其他开销另测 |

合计受控额度 56.5 MiB；不是在原采集队列之外再新增 56.5 MiB。现有通信实时缓存、业务/网络队列、WebView/QuickJS 堆、SQLite 非 page-cache 开销分别列实测，不声称受本表约束。实例/订阅数上限不保证每个都同时拿到最大缓冲；无额度时退化而非扩容。对象/字段个数和序列化大小都校验，不能仅以 JSON 字节数假称总进程内存有硬上限。

console 兼容层只按窗口聚合级别、调用次数和 contentNotCollected，记录类型由宿主绑定；不展开参数/toJSON/异常消息/stack。未处理异常/进程退出仅记录安全错误码、阶段、退出码和资源中断；stderr 不原文入库，stdout 仍是协议帧。logger 也只接受已声明字段，不直接序列化 Error。故障记录不递归生成故障记录；紧急通道恢复归档按稳定 ID 去重。

## 7. 核心自动采集与三个插件的改动点

### 7.1 核心/外壳/宿主

核心事件目录定义在核心静态描述中。`lib.rs` 的 record、服务器/分组存储提交、连接/SSH/PTY/文件/TCP 资源所有者，在真实提交、状态变化、释放处记录。外壳 `workbench_services.rs`、`plugin_registry.rs`、`app.rs` 和窗口相关服务只在逻辑工作区/视图/显式命令完成处记 action/lifecycle；WebView 重挂载不再记业务创建。宿主记录包验证、prepare/activate/stop、异常退出和覆盖状态；启动协调器用紧急通道记录尚未打开主库的阶段。

配置只留对象 ID、changedFields、前后修订和稳定结果；不保存值。共享连接断开由核心一次事实拥有，其影响由各插件分别上报并关联同一 diagnosticRef。原生菜单/外壳事件可 verified；插件 UI 报告仍 reported。任何 get/snapshot 轮询都不凭空制造 state.changed。

### 7.2 终端

- `plugins/terminal/src/worker.ts`：prepare 绑定 observer；open/pendingOpens/session 持不可变 attempt 上下文；create/reconnect/cancel/close 在真实 PTY 和发布结果后结束。waitingAuth/needs-input 不等于成功，失败后的 cleanup 单独记录。
- `terminal-view.tsx`/`main.tsx`：在现有用户命令处传 action，上报 sessions.status 时保留 ui-reported，不把 DOM 挂载记为会话创建。
- `directory-reporter.ts`：只记集成状态变化和稳定失败码，关联 directoryRevision；不记录每个提示符、cwd、nonce、终端输入/输出。
- 新增 `src/observation.ts` 安全投影，以及包内目录；会话快照/指标来自 worker 已有状态，最多 32 项详情，不增加 PTY 读取者。

### 7.3 文件

- `plugins/files/src/worker.ts`、`state.ts`（Explorer）、`follow.ts`：schedule 闭包保存上下文，实际 load/refresh/locate 结束才报告终态。accepted 只结束 RPC；读取成功但结果过期分别记录，superseded 不改写成远端失败。
- `remote.ts`、`mounts.ts`、`capacity-cache.ts`/`linux-collector.ts`：在已有业务结果处采集数量/耗时/稳定码，目录成功与容量退化分别留痕；不为诊断额外执行 ls/df。
- `view.tsx` 的刷新/定位/跟随设置动作绑定上下文；files.snapshot/row、滚动和重复渲染不产生语义事件。
- 新增 `src/observation.ts` 将私有状态投影为安全快照；paths/base64 节点 ID 只用于插件内存映射，随机 directoryRef 每 scope 最多 512 项并受总字节上限约束，超额省略标记。保留 files.diagnostics 兼容，公共核心不调用它。

### 7.4 网络隧道

- `plugins/network-proxy/src/{worker,rules,editor-sessions,manager,run}.ts`：Rules 持久回执后记 saved/deleted；Manager 给每次 Run/attempt 传上下文；状态 setter 仅变化时上报。
- `run.ts` 的 execute/targetOperation/check/stop：延迟任务绑定原 run/revision；test 按 CheckResult.state 判结果，Promise fulfilled 不是成功依据。target→serve→accept 用本地 failureId/终态所有者去重，cleanup 另记。
- `bridge.ts` 保持 12/8/4 lane 及清理额度，诊断使用统一调度；协议解析只输出允许枚举，不把 URL、host、头、握手或正文转成日志。
- 新增 `src/observation.ts` 和包内目录；原 200 条 Run 诊断留作展示缓存，历史以核心为准。运行快照最多 16 项详情，不上报第二份通信字节指标。

三插件各自的精确事件/字段和功能测试仍由 [F-TERM-004](../plugins/terminal/features/observability.md)、[F-FILES-003](../plugins/files/features/observability.md)、[F-PROXY-004](../plugins/proxy/features/observability.md)维护。插件只使用公开 SDK/核心，目录联动沿用既有目录状态服务，不以诊断事件绕过插件互调边界。

## 8. 数据升级影响与回退

按 P-52，不做观测 schema 1→2 迁移或旧历史导入。当前启动协调器只要一个观测分区版本不符，就将四个分区作为一套备份、暂存重建、退役、发布；沿用该行为，不只替换 facts/logs 留下混代数据。

**对当前用户数据的影响：**业务库为 schema 5 时继续使用，服务器、分组、凭据、信任、设置和插件业务文件保留；调用/事件/通信历史及已有观测数据四分区进入一致备份，新在线历史从新 storeEpoch 开始。即使 traffic 表结构没变也一起重建。提示必须清楚显示“仅观测历史重建”、四分区范围及备份位置，不能显示成所有配置已重置。

扩展现有ResetNotice为每个实际重建集合提供旧/新版本；app.rs现有原生提示显示观测1→2，避免只显示业务5→5。只更新既有提示的事实字段与文案，不增加确认步骤或新布局。按apple-design的清晰层次与平台一致原则，先说明实际重建范围和保留项，再列版本/备份位置；沿用系统字体/缩放/键盘行为，验证200%缩放时正文及备份位置可读。

无库正常初始化；全套同版本直接使用。损坏、缺少某个集合成员、备份/校验失败时停止启动并保留旧套。测试 SQLite backup 包含未 checkpoint 的 WAL 数据；备份清单/哈希、目标版本、阶段恢复、重建 notice 的事实记录继续复用，不复制业务 secrets 到不相关的观测备份。遇旧二进制留下且目标版本不同的未完成journal，保留全部原套并停止，明确需由对应版本先恢复；不能改写journal目标冒充已完成新版本恢复。

回退旧应用也会按 P-52 发现更高版本并备份/重建观测库，不宣称可直接读 schema 2。需要旧历史时，在隔离副本和匹配旧版本下恢复完整旧观测集合；本期不提供合并新旧历史功能。备份不是在线留存，不自动删，占用单列。所有失效测试只在专用副本执行。

## 9. 实施顺序与完成关口

| 顺序 | 完成内容 | 进入下一步的证据 |
| --- | --- | --- |
| 1 | 三份契约、生成器、包目录/版本校验及元数据类型 | 旧严格 DTO/旧包不回归；无 DOM 构建、描述非法输入拒绝 |
| 2 | schema 2、统一顺序/去重、writer/审计/轮转、启动恢复 | OCODE-03/04/07/08；调用事务一致，重建只影响观测集合 |
| 3 | 可信上下文、宿主转交、SDK 生产者和自动核心采集 | EV、OCODE-02/06/11；并发不串线，失效不重放业务 |
| 4 | v2 查询/补读、快照/指标、策略/健康 | HQ/DE/ST；跨重启 checkpoint、无自激、预算与控制进展 |
| 5 | 三个插件各自接入和独立包验证 | 18 项插件流程及 INT；真实异步终态、无正文和重复计量 |
| 6 | capture/证据/显式导出、恢复与仅公开消费者 | DE、OCODE-09/14；生产者退出后仍能解释证据 |
| 7 | Windows 集成/容量故障/性能回归、开发目录包 | 完整测试证据、已知限制、3–5 项验收步骤；之后才标工程完成 |

测试期间可分组启用能力，但不交付已声明却只有占位的服务。若必要真实验证受阻，任务保持开发中并记录缺口，不以模拟结果替代。方案确认、工程完成、用户验收、发布分别记录；不自动 Git 提交/推送。

## 10. 代码方案专项验证

功能语义继续执行 EV-01～11、HQ-01～09、DE-01～08、ST-01～10、INT-01～08 和三个插件各 6 项。IS-01～06 留给后续问题服务，不作为本次完成项。以下 OCODE 补实现风险，不取代每个功能自己的测试。

准备：Windows、隔离数据根、真实 SQLite/WAL 和 QuickJS，当前未重编译旧包与新签名包，两台/两个隔离 SSH 目标及受控 HTTP/SOCKS/TCP 服务。使用副本构造满盘/只读/损坏/崩溃；记录机器、构建、包/契约版本及启用模式。以下测试文件/命令尚未建立，不能按表述认定可运行。

| 用例 | 操作 | 通过标准 |
| --- | --- | --- |
| OCODE-01 | 描述漏签名/越界路径/身份冲突/字段超额，同版本改定义；组合新旧核心/包 | 新核心拒绝坏包且保留旧包；旧包无需重编译可运行；新包在旧核明确失败，历史定义仍可读 |
| OCODE-02 | 两 UI+worker 交错操作、移窗、弹窗关闭、异步跨线程，重复 requestId 换 metadata | 可信来源/代次不串；指纹冲突被拒；合法过期关联不重放/阻断业务；业务 operationId 保持原语义 |
| OCODE-03 | facts/logs 混合写、预留号段后/事务前后强杀，重启分页 | 已回执记录不丢；号段不复用，水位内无迟到插入；空号不冒称丢失；跨库部分成功如实返回 |
| OCODE-04 | 丢上报回执，同 ID 同内容/换 type/换内容重送，轮转后重送 | 留存内唯一事实、每次 RPC 独立审计；冲突拒绝，超去重范围如实说明；旧查询 DTO 不变 |
| OCODE-05 | 128 feeds 慢/快消费、包含 call、只做 query/ack/report、重启后 checkpoint 补读 | 无握手漏项；允许重复但无自激；断代/轮转 gap；控制可用，实例结束 1 秒内释放 feed |
| OCODE-06 | 三插件及多 surface 同时灌事件/快照/指标、丢回执，耗尽信用 | 单实例一个执行中采集请求；全部预算内退化、无无限副本；清理 lane 保留，debug 过滤不漏业务审计 |
| OCODE-07 | schema 5业务库+观测1，仅观测重建；各恢复阶段中断、坏/缺成员、高版本回退 | 4 分区一致备份含 WAL 数据；服务器/凭据不变；新 epoch、旧历史只在备份；失败停止、无混套 |
| OCODE-08 | 时间推进24小时/7天/30天，实际填充四分区/WAL，清理中取消且有活动调用/证据 | debug 恢复；在线512MiB/紧急8MiB上限；保留/淘汰准确、partial可查；备份占用单列 |
| OCODE-09 | capture/证据生成中清理/取消/强杀；保存取消、无权限、目标已存在 | 自包含包/水位/缺口准确；任务 interrupted，可清理；无原文件覆盖、无任意路径 API、无额外远端/网络调用 |
| OCODE-10 | 真实两服务器：终端创建/认证取消，文件accepted后失败，隧道检查state=failed，再断开共享连接 | 真实状态决定终态；核心原因与各插件影响可关联，重试独立；原业务与通信计量结果保持 |
| OCODE-11 | 秘密/路径/base64/正文标记穿过参数、console、Error、stderr、快照、导出；采集自身失败 | DB/WAL/紧急通道/导出均无禁止标记；console不调用对象toJSON；无递归日志/诊断任务 |
| OCODE-12 | 本期：真实Windows/Linux SSH三插件的正常操作、故障终态、资源关闭与App正常退出；受控注入诊断写失败/队列满/慢读，核对现有预算和释放。后续保留原16生产者、200事件/秒、128feeds的60秒基线/负载对照 | 本期：业务不因附加诊断重放，明确拒绝/丢弃且无无限队列，关闭/清理仍可用；遵守既定审计失败规则。后续极限目标仍为事件/关闭p95≤1秒、吞吐下降≤10%，原失败不改成通过 |
| OCODE-13 | 各插件单独构建/更新/重启启用，其他包保持旧二进制；Windows release目录包启动 | 各自产生声明事件且独立更新；旧包audit-only；无其他插件/核心业务专用修改依赖；真实包路径/哈希记录 |
| OCODE-14 | 停止全部生产者后，仅公开SDK读取 trace/类型/快照/指标/证据；主库故障后再恢复 | 不依赖私有命令/DB读取；缺口/过期可解释，紧急归档不重复；无AI调用却具备机器读取所需来源与证据引用 |

拟新增 `crates/core/tests/observability.rs` 与模块单测、SDK observation 测试、host/manager 契约测试；扩展已有插件 worker/state/follow/manager/bridge 测试。拟新增 `tests/desktop/observability.mjs`、`observability-package.mjs` 及只使用公开 SDK 的读取夹具，复用现有 native harness；不把测试专用调用入口打入普通发布包。

实现后的既有检查入口为 `npm run check`、`npm run test:contracts`、`npm run test:core`、`npm run build`、`npm run desktop:build`。新增桌面脚本在建立并登记前仅为计划；真实 SSH、QuickJS、原生 WebView/保存框及开发目录包分别记录结果。性能记录全部样本与排除项；故障注入场景不套正常延迟指标，但仍必须验证有界和资源释放。

清理：关闭 feed/任务/插件/SSH/监听端口，撤销时钟/磁盘注入，核对线程、信用、context、资源和临时文件释放；保留脱敏事件 ID、包清单、容量/延迟、备份恢复证据后删除隔离副本。结果归 T-016，当前均未运行。

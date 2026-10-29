# F-CORE-018：统一事件采集与操作关联

修订：r4 · 日期：2026-10-29 · 文档就绪：可评审；历史实施见 T-016，T-027 事件增量待确认
所属：核心事件采集、可信宿主、SDK · 上级：[能力总览](../observability.md) · 任务：[T-016](../../tasks/T-016-observability.md)

T-027 待确认增量：[逐步连接诊断 r1 §3/4](../../technical/connection-diagnostics.md)将执行器阶段开始/完成/等待/失败明确区分，使用版本化 `connections.route.phase` 事件、attempt/round/phase 关联和安全字段。现有 `connections.route.stage` 的无错误 started 通知不能解释为阶段成功；新记录采用真实结果，旧事件不倒改。复用现有 events 表/写入与留存预算，不新增诊断库或迁移；具体实施确认与 DIAG 测试由 T-027 r7 维护。以下 r2/r3 为原设计阶段描述，T-016 当前事实见其任务结果。

r2将核心/SDK和三个插件的接入具体化，组合流程见[接入需求](integration.md)。T-019已建立内部call/连接阶段事件和部分查询，当前仍无通用插件类型注册/批量上报/完整上下文传播；下文的新模型为待实现需求，不把既有局部trace视为完整操作链。

r3代码设计由[实施稿r1 §3～6](../../technical/observability-implementation.md)固定：API/SDK 4.10、包内observability描述引用、协商metadata与contextRef、统一writer分配跨facts/logs的提交序号及独立v2信封。本稿仍未授权实施。sequence允许预留号段空号，空号不能直接解释为事件丢失；批次跨分区可部分提交、逐项回执，不承诺跨库事务。有效v2事件引用可区分GONE/未知引用；旧UUID证据不足时标legacyPartial。合法诊断引用过期不阻断业务RPC，直接诊断上报仍返回STALE_CONTEXT，SDK只降级采集，不重放业务。

当前实施说明（2026-10-28）：用户已确认T-016完整实施及r6性能调整。上方“未实现/待确认”和下方拟建入口是原设计基线；当前代码、检查结果与未覆盖项统一见[T-016结果](../../tasks/T-016-results.md)，不代表工程完成。

## 1. 目标与范围

外壳、核心、插件用同一模型记录用户意图、运行过程、领域变化、API结果和生命周期。核心自动记录公共调用；插件报告其拥有的业务事实。包含结构化分级日志和操作关联，排除终端命令/正文录制、任意系统日志抓取和插件互调。

## 2. 数据模型与真实性

| 字段组 | 规则 |
| --- | --- |
| eventId、storeEpoch、sequence、schemaVersion | 核心分配；sequence为十进制字符串，按本存储提交顺序递增；不表示远端绝对发生顺序 |
| occurredAt、receivedAt、committedAt、bootId | 发生时间可以来自生产者，接收/提交时间来自核心；注明时钟来源/偏差，耗时用单调时钟计算 |
| source | 核心/外壳/插件、模块、插件ID/版本/实例、视图/挂载代次；由真实通道绑定，插件不能覆盖 |
| initiator | user/system/plugin/unknown，与报告者分开；只有可追踪的外壳动作或宿主动作上下文才标 verified，插件自报用户触发标 reported |
| kind、type、typeVersion | kind为action/call/domain/log/lifecycle/health；稳定type例如core.connection.failed、plugin.sodeck.files.directory.readFailed |
| level、outcome | trace/debug/info/warn/error/fatal；outcome为requested/running/succeeded/failed/canceled/timedOut/unknown/interrupted等适用值，不强迫所有事件带结果 |
| traceId、spanId、parentSpanId、causeEventId、callId、operationId | 操作链、阶段、显式因果、调用及业务操作分别标识；缺失时明确unknown，不按时间推定父子 |
| targets | 适用的server/workspace/view/resource稳定标识及代次；全局事件可为空，不保存可使用的资源句柄或秘密引用正文 |
| messageKey、safeArgs、errorCode、attributes | 版本化类型描述固定字段、类型和长度；展示文字可本地化，查询和聚合不依赖文案 |
| provenance、completeness | 来源为core-observed/plugin-reported/derived/imported-emergency，记录截断/脱敏/关联缺失；派生结论不能冒充原始事实 |

fatal表示某项明确作用域的功能无法继续，例如worker启动失败；不自动等于整个App崩溃，不直接决定弹窗。级别不替代调用结果、问题优先级或采集策略。

事件类型描述随插件包签名和版本一起交付，宿主只注册自身命名空间；旧描述快照随历史保留到相关事件到期，卸载插件仍可读取历史。采用受限schema子集：标量、枚举、有限数组、浅层对象，无脚本、正则执行或任意模板求值；未知字段拒绝。每包建议最多128种类型，单描述8KiB，已加载注册表App总量4MiB。包中manifest字段、描述限制和版本协商已由[代码方案§3](../../technical/observability-implementation.md)固定，当前尚未修改实际manifest。

核心只能确认通道身份和本身观察到的事实，不能保证插件上报内容真实。公开描述供新消费者理解含义，不作为插件权限清单。与已有P-14一致，所有已启用插件可调用公开能力；跨插件读已公开诊断数据不新增授权表。

## 3. 采集与操作生命周期

建议公开 `observability.describe`、`events.types.list/get`、`events.v2.appendBatch`、`traces.begin/stage/end/get`；SDK封装 `logger.info/warn/error` 等和操作上下文。新增v2事件信封不改变T-019现有strict事件DTO；这些新增方法尚不可调用。

### 3.1 首批接口的输入与回执

| 方法 | 必要输入 / 结果 | 功能约束 |
| --- | --- | --- |
| describe | 空查询 → capabilities、限额、策略修订、storeEpoch、各来源coverage | coverage区分audit-only/full/degraded/disabled/stale；full仅指声明类型已接入，不保证业务全部可观察或没有缺口 |
| types.list/get | 来源/类型/版本、分页 → 当前或历史描述 | 类型包括owner、字段/枚举/数据分类、默认级别、可用版本；历史不依赖插件仍安装 |
| appendBatch | batchId、items：clientEventId、type/typeVersion、occurredAt、contextRef可选、targets、fields | 宿主补身份、接收/提交时间；每项有persisted/duplicate/filtered/rejected和eventId/sequence/error。duplicate返回原已提交引用，不再次写领域事实；整次调用仍有独立审计 |
| traces.begin | clientOperationId、声明的actionType、trigger、scope/targets、允许的安全字段 | 返回traceId、rootSpanId、actionId及诊断contextRef；同实例/键/内容重复返回原上下文，内容冲突拒绝；只建立记录，不执行业务 |
| traces.stage | contextRef、stageKey、phase=start/finish、适用outcome/安全字段 | start创建子span并返回childContext；finish只结束自己创建的span；SDK包装为beginStage/endStage避免漏配对 |
| traces.end | contextRef、outcome、稳定errorCode及声明结果字段 | 结束所属根操作/阶段；重复同终态幂等、冲突拒绝。未结束子span保留pending/unknown，不能一键推断全链成功 |
| traces.get | traceId、时间/游标、limit | 返回有界span/事件/显式关系和coverage；旧事件只有call级关联时标legacy/partial，不制造不存在的父阶段 |
| observability.targets.resolve | 已支持核心对象类型、当前资源标识/代次 | 校验有效对象后返回只读diagnosticRef；该ref不可用于停止/获取资源，历史查询不要求资源仍活跃，不泄漏可用句柄 |

traces.begin/stage/end重送去重限于实例存活且记录仍在保留范围；上下文已过期时明确STALE_CONTEXT，不自动新建根操作。trace默认最长30分钟、无活动10分钟失效；长时间运行的会话不持有一个永不结束的启动trace，后续断线/重连独立成链并引用原对象。每实例活动span最多256、App4096，超过明确CAPACITY。到期只结束诊断跟踪并标unknown，不取消业务；后续真实结果可用operationId/资源ref关联新记录，但不能复活旧context。到期/退出由宿主统一回收，不由UI判断远端结果。

插件私有目录/目标标识使用SDK内存的随机entityRef，不把路径base64、URL或可逆编码作为安全ID。建议每scope最多512个映射、按对象释放；超限事件省略ref并标targetDetailLimited，不能阻止业务或回退存原始值。核心资源ref由resolve提供；插件侧ref只能说明插件自述的对象关联。

### 3.2 统一采集和去重规则

- 网关自动产生调用开始/结果，沿用每个公开调用留痕。内部追加不再次走公开网关，避免递归；失败请求也记录安全的接收/拒绝摘要。每次重送的调用都有独立callId。
- UI动作经宿主建立上下文，传到自己的worker和核心；后台定时任务单独起链，记录触发类型。异步任务显式传递上下文，禁止使用进程全局“当前操作”导致并发串线。
- 核心签发仅用于诊断关联的不透明上下文标记，校验实例/代次、目标关系和父阶段；不能用它获取业务权限或跨插件转交资源。插件只结束自身阶段，不能伪造核心认证结果。共享连接失败由核心记录资源原因，各消费者链通过关联边引用同一事实。
- begin/end保存事实；同一阶段只接受一个一致终态，重复相同提交返回原回执，冲突终态拒绝。崩溃后未结束阶段标interrupted/unknown，不能推断远端成功或自动重试。
- 插件append以实例和clientEventId去重，在对应事件仍留存时返回原eventId；相同键不同内容拒绝。过期重试返回明确的去重范围，不承诺跨留存期恰好一次。
- append回执逐条区分persisted/duplicate/filtered/rejected，不用accepted表示已落盘；SDK本地短队列只称buffered。批次按项校验，不让一条坏事件吞掉其他结果。超时提交结果不明时在去重范围内用同clientEventId重送，或用新查询的来源/clientEventId精确过滤核对。
- 调试日志可按级别/采样减少，用户操作、调用、关键生命周期、错误与恢复事实不采样；超额时返回限流/缺口并统计，不能默默当作已保存。日志关闭不能关闭API审计。

SDK可按带版本的采集策略在发起RPC前过滤trace/debug，减少无效上报；一旦公开调用已发出，就必须审计该调用，不能以其内容将被过滤为由跳过。策略切换失败保持最近有效策略并报告版本，不能静默关闭事实记录。

普通console不是正式事件契约。后续接入需替换当前空实现为有界兼容适配，但不能将任意对象、异常消息或stderr原文直接持久化。未声明的console只保留调用次数、级别与“内容未采集”的结构化摘要；开发终端的临时原文输出另受显式诊断模式控制。正式业务排错使用声明过字段的logger。宿主崩溃记录退出码/阶段及可安全解析的错误码，不把stdio协议混成日志文本。

## 4. 边界与容量

单事件建议8KiB、每批最多32项且序列化后不超过128KiB；超限字段拒绝或按该类型明确的截断规则返回标记。每实例上报令牌桶200条/秒、突发400条；非审计采集队列每实例256KiB、App8MiB。该预算不限制内部逐次审计；审计用独立保留预算，详见[F-CORE-022](storage.md)。不存在无限排队或“永不丢失”保证。

错误包括INVALID_EVENT、TYPE_UNSUPPORTED、STALE_CONTEXT、CONFLICT、CAPACITY、STORE_UNAVAILABLE、RESULT_UNKNOWN。关闭面板只结束该表面的采集上下文，业务worker仍按实际生命周期记录；实例退出停止新上报，已提交历史继续保留。正常退出按存储规范限时排空，强杀的尾部损失明确报告。

## 5. 验收与测试流程

AC-01来源和模型正确；AC-02操作关联不串线；AC-03每次调用留痕且无递归；AC-04错误、去重、限额和缺口如实可见。

环境：Windows隔离数据目录，真实SQLite与QuickJS宿主；身份/时钟/断线用可控夹具，另用真实终端、文件和隧道包验证采集边界。拟建事件契约/集成测试，当前无可运行入口。适用r1，实际结果只写T-016。

| 用例 / 验收 | 准备和操作 | 预期 |
| --- | --- | --- |
| EV-01 / 01 | 外壳及两个实际插件各上报事件；A伪造B身份、核心类型和verified用户来源 | 来源由宿主绑定；伪造字段/命名空间拒绝；插件自述不升级为核心事实 |
| EV-02 / 01、04 | 六级日志、取消操作、失败调用、恢复事件；降低日志阈值再查询 | 类型/结果/级别独立；debug可过滤，操作/API/关键事实不因阈值消失 |
| EV-03 / 02 | 两服务器同时连接，交错延迟/取消；跨窗口移动；一共享连接断开 | 父阶段/资源代次正确；多个消费者引用同一断开事件，不混链、不合并无证据的根因 |
| EV-04 / 02、04 | begin后强杀worker/App，再启动；end重复及冲突提交 | 中断与结果不明如实显示；重复一致无第二终态，冲突失败，无业务自动重放 |
| EV-05 / 03 | 上报、查询、ack、关闭各100次，含坏参数/未知方法；检查审计数量 | 每个公开调用有一条逻辑调用记录；内部事件写入不递归、不采样漏记 |
| EV-06 / 04 | 32项含坏项/超长项、超速生产、写盘失败、已提交但回执断开后重送 | 按项回执；未保存不报persisted；去重不重复事件但重送调用仍留痕；队列有界 |
| EV-07 / 01、02 | 旧包无事件描述、新包注册升级、卸载后查历史；伪造父标记和过期视图 | 旧包可运行并显示覆盖不足；历史类型仍可解释；非法关联不能冒充可靠因果 |
| EV-08 / 01、04 | 密码/终端/文件/URL正文标记经字段、console、错误对象和stderr路径输入 | 持久存储无标记；类型拒绝/省略可观察，协议stdout保持完整 |
| EV-09 / 01、02、04 | begin/stage/end重复与冲突，旧context、异步跨线程、30分钟/空闲到期、span超额 | 回执/归属正确；上下文到期不取消业务，后续结果标关联不足；不全局串trace |
| EV-10 / 01、04 | 核心资源释放后查询diagnosticRef，512个私有路径映射超额、旧包查询describe | ref无执行能力；超额省略且可见，无可逆路径泄漏；旧包显示audit-only |
| EV-11 / 01、04 | 交错提交facts/logs，号段预留后及事务提交前后强杀，重启查水位/旧投影和有效已淘汰引用 | 提交顺序稳定且不复用序号；空号不冒称丢失；跨分区逐项回执、GONE/legacyPartial准确，调用投影不双写 |

清理：关闭所有实例和测试App，恢复时钟/磁盘注入，核对队列与资源归零；仅保留脱敏计数与证据，删除临时目录。r1建立模型、采集和EV流程，不代表已实施。

# T-016 逐项验证矩阵

日期：2026-10-28 · 修订：r3 · 状态：本期工程检查通过（待用户验收）

按已确认78项逐项核对。通过可由单元、契约、真实服务和原生检查组合支撑；各证据层级分别注明。未运行表示整项尚未完成，其已经通过的子场景保留，不能将单测数量当作78项完成。没有改写原预期；IS六项不在本期。

证据索引（均在本工作区 `.qa/t016/`）：

- A：`workspace-delivery-retry.log`（Rust非ignore整套exit0）、`core-observability-final-retry.log`、`optional-log-isolation.log`；来源为核心/host/manager/desktop单元与契约，含实际SQLite。
- B：`check-final.log`、`contracts-final.log`；SDK55项、终端17+14、文件61+1、网络34。
- C：`native-delivery-run.log`、`native/results.json`；真实Windows/Linux隔离SSH、原生App、公开SDK，证据包明确partial。
- D：`compatibility-delivery.log`、`compatibility/results.json`；旧签名包及三组单独升级。
- E：`export-native/results.json`；Computer Use原生保存/取消/保护/200%缩放及退出修复复验，含原失败。
- F：`performance/measurements.json`、`performance-profile.log`；极限压力观察，延迟指标失败且按用户决定延期。
- G：`capacity-actual.log`；四分区实际写满及钉住WAL，分别低于256/128/96/32MiB。
- H：`remote-owner-final.log`、`resource-owner-test.log`；真实SFTP/exec/PTY，远端资源事实修复。
- I：`cancel-audit-fault-retry.log`；隔离Paramiko协议夹具50次取消与审计失效清理，区别于OpenSSH。
- J：`crash-recovery.log`；独立测试子进程真正强杀后再启同隔离库。
- K：`observation-final-boundaries.log`（31通过，父测试实际启动1个忽略的强杀子测试）、`context-boundaries.log`、`feed-timeout.log`、`history-cursor-boundaries.log`；500次审计精确计数、上下文/指标/订阅限额、到期游标、混合来源快照、核心禁止标记DB/WAL/导出扫描。时间边界用模块内时间数据推进，不修改系统时钟。
- L：`native-worker-crash-retry.log`、`native-boundaries/results.json`；真实Linux OpenSSH，第二服务器认证取消/重试、文件accepted后失败/重试、三插件移窗、正文/路径base64/规则名称扫描12个在线文件及导出、单独来源窗关闭。关闭原70150ms失败保存为`source-close-before-fix.json`，修复后1121ms，新增认证组合后968ms，最终强杀组合1159ms；App继续响应；准确worker强杀中断与PTY释放通过，最终正常退出。
- M：`rebuild-native-pass.log`、`rebuild/results.json`；只读复制dev.15清单文件至新隔离目录，不读取其日常data；原生200%提示、schema1→2四分区备份哈希与服务器保留、旧exe读取另一备份恢复副本22条原调用ID。
- N：`cleanup-cancel-combination-passed.log`；1024条事实清理已发生后取消，已删不回滚、仍有剩余、冻结证据与活动导出不变。首轮夹具错误（超队列单批/缺类型目录）保留。

- O：`files-shared-disconnect.log`（68+1）、`proxy-run-observation.log`（37）、`cleanup-concurrent.log`、`history-compatibility.log`；当前容量/跟随/断开、Rules提交计数、真实Run类迟到/去重、证据清理及配置代次模块验证。
- P：`native-network.log`、`network/results.json`；原生App+隔离Paramiko实际SSH/TCP协议，两规则共享transport、自动/人工重试、32坏HTTP与正常请求、HTTP秘密标记及计量。不是OpenSSH服务。
- Q：`native-disconnect.log`、`disconnect/results.json`；真实Linux OpenSSH三插件，debug提示符/cd、远端exit、共享终端/文件断开与独立代理隔离。原错误成功/ref缺失已修复，失败日志保留。
- R：`privacy-instruction.log`、`sdk-evidence-capability.log`（SDK56）；恶意指令样本经禁止字段无持久化、缺evidence API错误透传。结合A的真实QuickJS console及紧急文件测试，证明分层边界；没有模型/自动执行实现。

- S：`package-final-ordinary.log`、`package-ordinary/results.json`、`package-delivery.json`；Windows release候选普通启动与最终包39项哈希/运行文件一致性。目录包验证，不是安装器验证；`service-cleanup.json`记录准确隔离服务退出。

| 用例 | 结果 | 已有证据与剩余范围 |
| --- | --- | --- |
| [EV-01](../core/observability/events.md) | 通过 | A/B/C：描述归属、命名空间/字段拒绝、metadata验证；真实三插件来源 |
| [EV-02](../core/observability/events.md) | 通过 | A/B/C：六级logger、过滤/CAS、取消/失败/恢复事实 |
| [EV-03](../core/observability/events.md) | 通过 | C/L/Q：双服务器认证取消/重试、操作中移窗及真实共享连接断开分层覆盖；Q终端/文件关联同一核心resourceRef，独立代理不被误归因 |
| [EV-04](../core/observability/events.md) | 通过 | A/J：真实强杀后trace中断、已回执事实保留；重复/冲突终态单测 |
| [EV-05](../core/observability/events.md) | 通过 | K：上报/查询/ack/close/未知方法各100次，含一半非法参数；500次均恰一条已结束审计、每call恰两条事实，无递归 |
| [EV-06](../core/observability/events.md) | 通过 | A/B：混批逐项回执/提交失败、同ID重送、队列/速率限额；G：实际容量 |
| [EV-07](../core/observability/events.md) | 通过 | A/D：旧包audit-only、单独升级、类型持久化、非法上下文/旧挂载 |
| [EV-08](../core/observability/events.md) | 通过 | A/K/L/P/R：核心禁止字段及业务参数扫描DB/WAL/SHM/导出；QuickJS console不序列化参数，宿主stderr固定丢弃；紧急通道固定码及恢复无正文；真实终端/路径/base64/HTTP标记均通过。分层验证，非单次全入口注入 |
| [EV-09](../core/observability/events.md) | 通过 | A/B/K：幂等/冲突/跨线程/旧context；30分钟/空闲到期后业务仍执行一次，256/4096 span上限拒绝 |
| [EV-10](../core/observability/events.md) | 通过 | A/B/D/H：签名ref、资源结束、512个别名/字节上限、旧包覆盖；真实失败open不伪造资源 |
| [EV-11](../core/observability/events.md) | 通过 | A/J：混分区顺序、提交回滚/失败不回执、强杀重启不复用、GONE/旧投影 |
| [HQ-01](../core/observability/history.md) | 通过 | A：v2同时间205项分页、后插入回拨时间记录不挤入固定水位；过滤与缓存等价测试 |
| [HQ-02](../core/observability/history.md) | 通过 | A/K/M：篡改/换条件/清理/旧epoch拒绝，5分钟有效期限边界通过内部签名到期值验证；M实际集合重建，未等待真实5分钟 |
| [HQ-03](../core/observability/history.md) | 通过 | A：计数/耗时固定桶、缺失不计零；限分组/窗口、查询中断有界 |
| [HQ-04](../core/observability/history.md) | 通过 | B监听先于激活，K open前/激活前/激活后事实集合与未ack重复batch；J真实重启checkpoint补读 |
| [HQ-05](../core/observability/history.md) | 通过 | B释放后不ack；K未激活/慢消费超时closed并回收信用，旧epoch拒绝、重复close；实际清理后gap须显式acceptGap |
| [HQ-06](../core/observability/history.md) | 通过 | A：query/ack/report审计过滤不自激，checkpoint跨重启 |
| [HQ-07](../core/observability/history.md) | 通过 | F/A：128feed压力观察、发送预算与轮转公平、关闭可用；极限p95按r6延期，不能视为性能通过 |
| [HQ-08](../core/observability/history.md) | 通过 | A/O/M：同schema2跨config-A/B默认隔离、显式多代读取、v1严格DTO；schema1旧行只留备份并由旧exe读取，不存在旧行导入新表的legacyPartial迁移路径 |
| [HQ-09](../core/observability/history.md) | 通过 | A/J：持久checkpoint重启补读、诊断调用无自激；清理GONE/gap分层覆盖 |
| [DE-01](../core/observability/evidence.md) | 通过 | A/K：同次A有效/B过期/C audit-only，时间与coverage/gap明确，capture后现场更新不改冻结值 |
| [DE-02](../core/observability/evidence.md) | 通过 | A/K：单位/schema/重复样本/重启明细覆盖；128序列可用，第129 CAPACITY，错误单位与动态requestId标签拒绝 |
| [DE-03](../core/observability/evidence.md) | 通过 | A/C：真实失败证据、固定水位/冻结、独立类型/hash；实际大包明确partial/capture-incomplete |
| [DE-04](../core/observability/evidence.md) | 通过 | E/J/G/L：保存、Esc/API取消、不可写/不覆盖、200%缩放、退出/强杀恢复、独立来源窗关闭canceled且主App响应 |
| [DE-05](../core/observability/evidence.md) | 通过 | C：退出全部生产者并重启后只用公开SDK读历史/类型/快照/指标/同一包 |
| [DE-06](../core/observability/evidence.md) | 通过 | K/R：禁止正文及恶意指令样本文本经事件/快照/参数后，在线和关闭后文件及证据无标记；公开SDK读取不触发业务调用。未实现模型或自动执行消费者 |
| [DE-07](../core/observability/evidence.md) | 通过 | A：100次重复故障合并、并发预算和失败不递归；C/E退出 |
| [DE-08](../core/observability/evidence.md) | 通过 | A/C：三来源主动快照/指标、sample去重/冲突、revision冻结/替换、生产者结束后读取 |
| [ST-01](../core/observability/storage.md) | 通过 | A/I：必需审计失败拒绝新业务、清理可用；独立logs分区故障不阻断/重放业务 |
| [ST-02](../core/observability/storage.md) | 通过 | A/J：投影同事务回滚、commit失败、真实强杀/重启中断与序号连续性 |
| [ST-03](../core/observability/storage.md) | 通过 | A/G：24小时/7天/30天留存边界、活动对象保护、实际四分区含钉住WAL上限 |
| [ST-04](../core/observability/storage.md) | 通过 | A/B/J：debug到期、队列/flush上限、真强杀后未完成状态可见、紧急记录有界 |
| [ST-05](../core/observability/storage.md) | 通过 | A/K/L/P/R：入口拒绝、QuickJS console计数、固定码紧急通道与恢复、实际DB/WAL/SHM/导出分层证明；真实HTTP和终端/路径标记无泄漏，非单次全入口联合注入 |
| [ST-06](../core/observability/storage.md) | 通过 | A：初始化/同版/异版/坏库、仅重建单集合、每个发布边界恢复、哈希损坏拒绝 |
| [ST-07](../core/observability/storage.md) | 通过 | A/K/N/O：活动调用保护、GONE/gap、已删除后取消、冻结包/活动导出保留；生成中清理明确partial/CURSOR_EXPIRED/capture-incomplete，仍可导出；问题服务不在本期 |
| [ST-08](../core/observability/storage.md) | 通过 | A：不可写健康/紧急环、恢复后归档去重、并发追加不删除未归档内容 |
| [ST-09](../core/observability/storage.md) | 通过 | A/G/I/C：统一四分区总账、diagnostics预算竞争、队列保留与日志故障隔离；正常通信并行 |
| [ST-10](../core/observability/storage.md) | 通过 | A/M：schema5/1→2、含WAL备份/凭据/每发布边界/坏集合；200%原生提示及旧dev.15 exe回退副本22条原审计ID |
| [INT-01](../core/observability/integration.md) | 通过 | C/L：真实双服务器/三插件操作，加第二目标waitingAuth取消与同session新attempt重试，首目标继续连接；文件accepted后失败及重试不串链 |
| [INT-02](../core/observability/integration.md) | 通过 | P/Q：真实OpenSSH终端/文件共享connection一次核心ended、两个插件通过同resourceRef记录影响；独立代理保持ready。协议夹具另核对两网络Run共享transport受影响 |
| [INT-03](../core/observability/integration.md) | 通过 | A/B/L：代次/迟到、三插件真实移窗无重建、独立来源窗关闭、强杀准确终端worker阶段interrupted/PTY释放且主App继续响应 |
| [INT-04](../core/observability/integration.md) | 通过 | A/D：签名描述拒绝保留旧版、capability兼容、旧二进制及逐插件升级矩阵 |
| [INT-05](../core/observability/integration.md) | 通过 | A/B/F/I：诊断重试不重放、队列/控制保留、日志分区故障、50次取消及审计失效后释放 |
| [INT-06](../core/observability/integration.md) | 通过 | A/C：纯公开SDK在无生产者App读历史/类型/trace/快照；旧DTO保留 |
| [INT-07](../core/observability/integration.md) | 通过 | C/P/R：核心communication计量随实际HTTP增长，插件指标无流量重复；真实URL/头/凭据/body与终端路径扫描；SDK缺evidence API明确METHOD_NOT_FOUND且无回退业务调用 |
| [INT-08](../core/observability/integration.md) | 通过 | A/B/K/L：真实文件accepted后failed、两次尝试不同trace/span，合法定位随后成功；订阅查询无自激 |
| [TERM-OBS-01](../plugins/terminal/features/observability.md) | 通过 | C/L：两服务器成功、第二目标waitingAuth不写成功，取消无PTY、重试同session且新attempt/retryOf，首目标不受影响 |
| [TERM-OBS-02](../plugins/terminal/features/observability.md) | 通过 | B/L/Q：worker迟到及状态回归、实际移窗PTY身份保持且无重复open、真实远端exit后保留观察依据且不编造exitCode |
| [TERM-OBS-03](../plugins/terminal/features/observability.md) | 通过 | B/L：发布/释放失败与取消竞态；真正强杀终端worker后未结束阶段interrupted、核心plugin.failed、PTY释放且主App存活，无成功终态 |
| [TERM-OBS-04](../plugins/terminal/features/observability.md) | 通过 | A/B/L/Q：真实秘密输入/输出和路径扫描；debug下重复提示符与cd不重复上报ready，远端退出状态可查；禁止正文/nonce字段拒绝 |
| [TERM-OBS-05](../plugins/terminal/features/observability.md) | 通过 | B/F/I：上报挂起/队列满有界与一次业务调用；真实负载终端吞吐/关闭、清理审计故障隔离 |
| [TERM-OBS-06](../plugins/terminal/features/observability.md) | 通过 | A/C/D：旧新兼容、只更新终端、生产者结束后历史快照读取 |
| [FILES-OBS-01](../plugins/files/features/observability.md) | 通过 | B/C/L：排队取消、刷新和真实不存在目录accepted→failed→不同trace/span手动重试→合法定位成功 |
| [FILES-OBS-02](../plugins/files/features/observability.md) | 通过 | B/L/O/Q：follow记录旧修订superseded及新修订applied，来源与directoryRevision准确；隐藏/手动/迟到用例、真实移窗和共享连接丢失影响 |
| [FILES-OBS-03](../plugins/files/features/observability.md) | 通过 | B/O/Q：缺procfs保留目录，容量unsupported重复只一次、真实Promise成功后恢复；取消/隐藏/换连接后的迟到available不再上报；共享断开刷新记失败 |
| [FILES-OBS-04](../plugins/files/features/observability.md) | 通过 | A/B/C/K/L：描述拒绝自由文本、别名有界，真实路径及base64未进入12个观测DB/WAL/SHM文件与证据 |
| [FILES-OBS-05](../plugins/files/features/observability.md) | 通过 | B队列/快照合并、follow取消与隐藏；A/I诊断故障隔离；C资源关闭 |
| [FILES-OBS-06](../plugins/files/features/observability.md) | 通过 | A/C/D：只升级文件旧终端无需重编、无生产者历史公开读取、capability拒绝 |
| [PROXY-OBS-01](../plugins/proxy/features/observability.md) | 通过 | B/O/C：规则CAS/停止先于删除；实际Rules存储失败/冲突不发saved/deleted，成功修订与删除各一次，saveDraft同提交重试不重复；真实保存通过 |
| [PROXY-OBS-02](../plugins/proxy/features/observability.md) | 通过 | B/O/P：两规则共享transport断线各一次影响，手动恢复保留run；自动失败→人工恢复有不同trace/attempt与retryOf；旧run迟到不覆盖新run |
| [PROXY-OBS-03](../plugins/proxy/features/observability.md) | 通过 | C/O：真实检查fulfilled且state=failed记失败；真实Run类受控迟到检查保留旧run/revision，新run无check污染，action补齐ruleRevision |
| [PROXY-OBS-04](../plugins/proxy/features/observability.md) | 通过 | O/P：真实Run多层catch同对象100次各一事实、cleanup独立、显示缓存200项；原生32个独立坏HTTP各一失败，穿插正常代理通过，停止释放 |
| [PROXY-OBS-05](../plugins/proxy/features/observability.md) | 通过 | C/P：debug真实HTTP URL/Authorization/头/body标记，在线及退出后观测目录与证据无标记；核心通信计量增长且插件无重复流量指标 |
| [PROXY-OBS-06](../plugins/proxy/features/observability.md) | 通过 | C/D/L：独立更新、停止生产者/重启后历史；原生移窗不记stop，旧包audit-only和新包能力缺失拒绝；快照不恢复监听 |
| [OCODE-01](../technical/observability-implementation.md) | 通过 | A/D：完整签名/篡改/路径/归属/结构、同版本类型冲突、独立旧新包 |
| [OCODE-02](../technical/observability-implementation.md) | 通过 | A/B/L：可信metadata不可变/跨线程/指纹冲突；双服务器认证、移窗/表面重挂载、来源保存框关闭不混上下文，过期业务仍执行一次 |
| [OCODE-03](../technical/observability-implementation.md) | 通过 | A/J：混分区批事务、单项/提交失败、真强杀已回执事实与新序号 |
| [OCODE-04](../technical/observability-implementation.md) | 通过 | A/B：同ID重送/冲突、过期context去重、GONE；旧投影保持 |
| [OCODE-05](../technical/observability-implementation.md) | 通过 | A/B/F/J/K：握手、预算、公平、重启checkpoint，慢消费超时回收、清理gap显式确认、旧epoch拒绝 |
| [OCODE-06](../technical/observability-implementation.md) | 通过 | A/B/F：实例/端点信用及回收、一个在途采集、快照合并、有界发送和控制保留 |
| [OCODE-07](../technical/observability-implementation.md) | 通过 | A/M：集合恢复/原凭据/WAL、200%原生观测重建提示及旧exe读取恢复副本均通过 |
| [OCODE-08](../technical/observability-implementation.md) | 通过 | A/G：留存时钟边界、真实四分区/WAL、活动保护、紧急上限 |
| [OCODE-09](../technical/observability-implementation.md) | 通过 | A/E/J/L/N/O：包hash/partial、取消/退出/强杀中断、来源窗关闭；生成中清理明确partial，冻结包与活动导出保留 |
| [OCODE-10](../technical/observability-implementation.md) | 通过 | C/H/L/P/Q：Windows/Linux双SSH、认证取消/重试、文件accepted后失败、隧道fulfilled失败、真实共享断开resourceRef与实际影响，核心计量保持 |
| [OCODE-11](../technical/observability-implementation.md) | 通过 | A/K/L/P/R：各入口与介质分层覆盖（console不调toJSON、宿主stderr不存储、固定码紧急通道、DB/WAL/SHM及导出扫描），真实HTTP/终端/路径/base64无正文；采集故障无递归 |
| [OCODE-12](../technical/observability-implementation.md) | 通过 | 按r6本期：C/H/I正常双SSH三插件、G实际容量、A/B诊断队列与日志失效隔离、E正常退出。F极限延迟未达标保留延期 |
| [OCODE-13](../technical/observability-implementation.md) | 通过 | D/S：独立旧新包矩阵；最新release及三签名包候选direct/launcher普通启动、忽略开发变量、portable数据、三来源full及正常退出均通过；最终目录与候选运行文件逐项哈希一致 |
| [OCODE-14](../technical/observability-implementation.md) | 通过 | A/C：生产者全部停止后SDK历史/类型/快照/指标/证据可读；紧急恢复归档去重 |

当前汇总：通过78项，未运行0项。此表是执行记录，不是范围变更或用户验收。

# 容器日志代码修改方案

日期：2026-10-20
状态：用户已确认并完成代码实施；本文件保留实施前方案，实际交付与验证边界见 `container-logs-implementation.md`。
依据：`container-logs-experience-plan.md` v2 与 apple-design 技能。

## 1. 目标、范围与交付顺序

沿用现有 React + Tauri + SSH + SQLite + Virtuoso 架构，解决容器重启接续与日志阅读行为不清楚的问题。实现按下面顺序推进；优先级代表实施顺序，不代表只完成第一步就结束。

| 阶段 | 本方案安排的内容 | 交付条件 |
| --- | --- | --- |
| A：状态与交互原型 | 数据契约、阅读状态机、模拟采集驱动的日志窗口 | 可交互演示跟随、暂停、搜索、等待、重连、失败、离线；浅深色和紧凑布局可用 |
| B：核心闭环（P0） | 接入真实采集、错误分类、同 ID 重启恢复、筛选标签、底部主动作、系统事件 | 重启前后的阅读意图保持，关闭与重开行为正确 |
| C：完整排查（P1） | 实时关键词筛选、稳定搜索、范围操作、筛选导出、显示偏好 | 查询、显示、复制和导出范围一致；长行与增量更新正确 |
| D：生命周期（P1） | 本次/上次启动，发现替代容器并显式切换 | 新旧容器来源独立，未知历史不被错误分类 |
| E：收尾验收 | 原有回归、真实 Docker、布局与可访问性检查 | 关键场景达到第 12 节验收标准 |

P2 的历史后台补齐、自动跨重建跟随、后台常驻采集不纳入本轮实现。保留测量与扩展位置；本轮不切换 Docker Engine API、不新增远端常驻程序、不升级 React/Virtuoso/Tauri，也不引入全局状态管理库。

最终确认后按 A→E 连续实施；A 是开发验证产物，不额外增加逐阶段批准流程。实际发现会改变目标行为或存档兼容性的问题时，再明确提出具体调整。

## 2. 已核实的代码基础与约束

| 位置 | 当前事实 | 修改方向 |
| --- | --- | --- |
| `src/ContainerLogsApp.tsx` | 单组件管理事件、查询、缓存、滚动、搜索、范围和导出；同一状态存在 state/ref 双份维护 | 按生命周期拆分，避免多个布尔量互相覆盖 |
| 同文件日志区域 | `onPointerDown={pause}`；任意筛选会阻止实时追加 | 删除通用按下暂停，用阅读意图和查询模式决定跟随 |
| `src/logApi.ts` | 仅有页查询、全部/序号范围导出、范围复制；LogEvent 在组件内 | 集中类型，扩展结构化状态、事件、查询与导出契约 |
| `src-tauri/src/logs.rs` | 5 秒等待检查、网络退避、按 source 串行替换 worker 已存在；错误和状态大量使用字符串 | 保留取消与串行约束，抽离采集状态和分类 |
| `src-tauri/src/log_ingest.rs` | 同一 message 的 fragments/chunks 可能续写；重连可能更新旧 segment 中的未完成消息 | 增量协议必须表达旧记录更新，不能只看 lastSeq |
| `src-tauri/src/log_store.rs` | 分片 SQLite；当前写入格式版本 3；按 seq 固定查询上界 | 保持主存档结构兼容，明确高水位与内容快照的区别 |
| `runs` | 代表采集批次，历史读取、接续都可能新增 | 启动周期另行记录，不复用 runs 作为重启次数 |
| `playwright.logs.config.ts` | 仅匹配 `log-archives-ui.spec.ts` | 新增实时日志窗口的 IPC 模拟测试并纳入命令 |
| `tests/features.spec.ts` | 已有原生日志窗口、重连、重开、停止后启动的测试定义 | 更新断言，补阅读锚点与重启场景 |
| `tests/docker-engine/` | 已有隔离的嵌套 Docker Engine + SSH 环境 | 真实生命周期测试使用此环境，不操作用户日常容器 |

本轮已核对源码、安装的 Virtuoso 类型声明和官方 API 文档；没有运行这些测试，也没有把已有测试定义当作当前通过的证明。

## 3. 文件与模块安排

新增文件名为拟定名称，实施时允许合并小模块，但不扩大职责。

### 3.1 前端

| 文件 | 责任 |
| --- | --- |
| `src/ContainerLogsApp.tsx`（改） | 窗口入口与组合；保留 WindowTitlebar，组装 controller、工具栏、正文和阅读栏 |
| `src/logApi.ts`（改） | IPC DTO、调用封装；集中 LogEvent，不在组件中重复定义 |
| `src/logs/model.ts`（新） | CollectionState、ReadState、QueryState、Anchor、事件 reducer 和状态文案派生 |
| `src/logs/useLogSession.ts`（新） | start/stop/retry、Channel 订阅、代次隔离、窗口卸载清理、存档升级为在线的恢复 |
| `src/logs/useLogQuery.ts`（新） | 查询请求序列、分页、快照句柄、增量扫描游标、缓存上限、上下文返回 |
| `src/logs/LogViewer.tsx`（新） | Virtuoso、阅读锚点、原生滚动和选区、系统事件与正文混合呈现 |
| `src/logs/LogToolbar.tsx`（新） | 查找、筛选标签、显示设置、导出入口；小型弹层可在文件内拆分 |
| `src/logs/LogReadingBar.tsx`（新） | 跟随/暂停/快照主动作、新增提示、选中范围操作 |
| `src/logs/preferences.ts`（新） | 显示偏好的版本化存储、校验和跨窗口同步 |
| `src/logs.css`（改） | 新层级布局、主题 token、紧凑布局、焦点、减少动态效果/透明度 |
| `src/LogText.tsx` / `src/logHighlight.*`（按需改） | 仅为正文稳定性或快照匹配修正；不重写现有高亮系统 |

阅读相关事件由一个 reducer 决定，查询与网络状态分别管理；effects 执行副作用，不能从状态更新函数中启动 IPC。refs 只保存句柄、游标或最新回调，避免再维护一套权威 UI 状态。

### 3.2 后端

| 文件 | 责任 |
| --- | --- |
| `src-tauri/src/logs.rs`（改） | 保留命令、窗口来源、worker 生命周期；委托采集与查询任务 |
| `src-tauri/src/logs/protocol.rs`（新） | 结构化状态、容器观测、错误分类、Channel 事件 DTO |
| `src-tauri/src/logs/collector.rs`（新） | inspect、read、等待、退避、状态探测、取消检查 |
| `src-tauri/src/logs/lifecycle.rs`（新） | 来源级生命周期元数据、系统事件、已观测启动周期 |
| `src-tauri/src/logs/snapshot.rs`（新） | 窗口绑定的历史快照句柄、短期读取视图与可变消息冻结 |
| `src-tauri/src/logs/export.rs`（新） | 导出范围解析、后台任务、取消、完整原文写出 |
| `src-tauri/src/log_store.rs`（改） | 范围/快照查询、读协调、快照原文读取、辅助数据统计 |
| `src-tauri/src/log_ingest.rs`（改） | 提供提交摘要、可变消息集合与已提交锚点；保留原去重算法 |
| `src-tauri/src/lib.rs`（改） | 注册新增命令；窗口销毁/退出时释放查询和导出任务 |
| `src-tauri/src/docker/query.rs` / `compose.rs`（按需改） | 复用或小幅抽取只读检测与来源匹配逻辑，不修改容器管理动作 |

新命令仍由调用窗口解析 source/连接凭据。前端不能通过传入任意 sourceId、路径或 endpoint 获取其他窗口的权限；快照、任务和事件游标都校验所属窗口与来源。

## 4. 状态模型与 IPC 契约

### 4.1 前端状态

拟定核心形状如下，属于方案示意：

```ts
type ReadState =
  | { mode: 'follow' }
  | { mode: 'paused'; reason: 'browse' | 'manual' | 'selection'; anchor: LogAnchor | null };

type QueryMode = 'live' | 'snapshot' | 'context';

interface LogAnchor {
  itemKey: string;      // log:<seq> 或 event:<eventId>
  offsetPx: number;     // 相对视口顶部偏移
}
```

查询状态保存条件、当前快照、返回查询及其锚点。进入 snapshot/context 时保存先前阅读意图；它们不能因为触底回调而自动切回 live。

状态转换的关键约束：

- 单击正文、聚焦、打开空搜索框不发出暂停事件。
- 实际向上翻阅或有用户操作依据的离底才进入 browse；新增行造成高度变化不能误判。
- 文本形成非空选区或选择记录范围进入 selection；有选区期间不替换相关 DOM，不被新数据或重连打断。
- 只有 browse 暂停可以在用户返回底部时自动恢复。manual/selection 需要明确继续。
- 采集状态转换不派发阅读重置事件；恢复连接也不调用“清空筛选”。
- 搜索与时间范围转换使用显式 action，清除条件与继续跟随保持独立。

### 4.2 采集协议

所有采集事件携带 `collectorId` 和单调递增 `eventRevision`；单次连接另有 `attemptId`。区分用户启动的采集任务、一次传输尝试和容器启动周期。

拟扩展的事件：

- `state`：`phase`、`reason`、`retryAt`、容器已知状态、观测时间、原始诊断摘要。
- `saved`：`lastSeq`、`archiveRevision`、已变化的记录范围或 `invalidate` 标志。
- `lifecycle`：持久化事件新增通知，前端按事件游标读取详情。
- `end`：明确区分 userStopped、sourceRemoved、fatalError。

phase 至少包含 connecting、syncingHistory、streaming、waitingContainer、reconnecting、failed、offline。使用结构化原因区分容器不存在、权限/认证、网络、daemon 不可用、驱动不支持和存储失败。

`archiveRevision` 用于表示当前采集代次内存在内容更新；不是跨进程永久一致性凭证。前端更换 collector 后全量刷新必要元信息，不比较不同代次的 revision。

现有组件中的 runEpoch/version 保护思路保留，但分别命名为 collectionEpoch/queryEpoch，旧连接和旧查询的迟到结果不能覆盖新状态。

### 4.3 命令清单

- 保留：`logs_info`、`logs_start`、`logs_stop`、`logs_page`、`logs_copy_range` 和现有存档入口。
- 增加：`logs_retry`，只唤醒当前任务的等待/退避；正在连接或采集中时返回当前状态，不并行启动第二个 worker。
- 增加：`logs_events_page`、`logs_boots`，按调用窗口来源读取事件与启动周期。
- 增加：`logs_snapshot_create`、`logs_snapshot_release`，长操作以 Channel 回报进度并可取消；查询、上下文和导出共享来源绑定的句柄。
- 增加：`logs_export_start`、`logs_export_cancel`，导出事件与采集事件使用独立 Channel；用户取消系统保存对话框不显示失败。
- 增加：`logs_replacements` 与 `open_log_replacement`，只读发现并打开经过后端再校验的新目标。

迁移过程中保留旧 `logs_export(from,to)` 作为委托入口，使现有原生测试和调用点能逐步迁移。前后端随应用一起发布，不建设独立的多版本网络协议。

## 5. 滚动、缓存与查询修改

### 5.1 滚动和锚点

- 利用现有 Virtuoso 的 `scrollerRef`、`atBottomStateChange`、`rangeChanged`、`followOutput` 和定位接口，配合原生滚动事件识别操作来源。
- 在正文附近记录用户滚轮/键盘/指针滚动意图；不能仅凭 atBottom=false 就暂停，不能保留正文级 pointerdown 暂停。
- 稳定 key 使用记录序号或事件 ID。正文与系统事件混合后，firstItemIndex 根据实际混合列表项目数调整，不再直接按日志行数算偏移。
- 翻页、换行、字体与工具栏高度变化前保存首个可见 key+offset，变化后恢复。Virtuoso 的测量快照只在相同数据/尺寸条件下复用，不能跨查询盲目恢复。
- 保留 5000 行 / 16 MiB 的有界缓存策略作为起点；有活动选区时停止后台替换当前正文，只更新未读状态。
- 原生选区在虚拟化边界存在限制：当前可见范围内保持 DOM，跨大量记录的精确选择使用 seq 范围。显式切换查询会结束旧选区；不声称可无限保留被卸载 DOM 上的原生选区。
- 连续输出用已有约 300ms 更新节奏作为起点，后续按测量调整；按钮按压与本地状态反馈不等待该批次。

### 5.2 增量更新

当前 newer 查询从最后显示 seq 回读，对实时筛选存在空页重复扫描问题，也无法覆盖任意旧片段更新。修改为：

1. 为 live 查询保存独立 `scannedThrough`，按后端 nextCursor 推进；零命中也前进，避免从最后匹配行重复扫。
2. 保存范围与已显示范围分开。暂停时只接收摘要，恢复时按当前条件补查最新记录。
3. 对 changedRanges 回读受影响区域，更新或移除不再匹配的记录；范围过大时刷新当前锚点附近，而非只追加。
4. 新增计数只计算新记录，不把同 seq 的内容续写算作新增。筛选匹配量未算完时只显示“有新日志”。
5. 合并通知与请求；每个查询至多一个前台读取任务，旧请求中止/丢弃结果，离线视图不轮询网络。

### 5.3 搜索快照的准确语义

当前 `snapshot: seq` 只是序号高水位，未完成消息会原位续写。因此需要避免把它直接包装成严格内容快照。

- B 阶段保留现有高水位机制，文案使用“搜索范围截至 #N”，已展示结果冻结，明确未完成记录可能续写；不宣称内容不可变。
- C 阶段引入真正的历史快照：固定分片清单、高水位和事件上界；冻结会继续更新的未完成 message 对应 records/chunks，包括旧分片中的待续写消息。
- 完整消息沿用主存档，不复制整份历史。未完成消息以流式方式写入临时 SQLite 快照文件，保持内存有界；主库查询排除这些消息，再合并冻结副本参与匹配、分页、上下文和导出。
- 使用来源级读写协调，在提交边界建立各分片的 SQLite 读取视图；不在网络读取期间持锁。冻结完成后释放读取事务，避免用户长时间查看导致 WAL 持续增长。
- 创建期间回报进度，可取消；长行导致冻结体积或耗时超过预算时明确报错并保留原查询，不能静默降级成不同语义。预算通过长行夹具与实测确定。
- 句柄绑定窗口、source、条件版本；查询更新、来源切换、关闭窗口时释放。执行中的导出持有引用直到结束；异常退出残留缓存在下次启动按专用目录清理。
- 快照文件放在应用缓存的专用目录，不加入永久存档和存档列表。跨启动不恢复旧句柄。

上下文跳转保存原快照、条件和锚点，返回时恢复同一个查询；重新搜索则创建新句柄并释放不再使用的旧句柄。

## 6. 采集与重启恢复

### 6.1 保留现有基础

继续使用固定 endpoint 和完整容器 ID，通过 `docker logs --timestamps --since ... --follow` 接续；保留回读重叠区与已有按出现次数去重。保留 source 级 worker 排他、旧 worker 取消和等待顺序，不使用 UI 重挂载触发隐式双采集。

### 6.2 恢复状态机

```text
connecting → inspecting → syncingHistory / streaming
                                ↓ 正常结束或容器停止
                         waitingContainer → inspecting
                                ↓ 可恢复故障
                         reconnecting → inspecting
                                ↓ 不可恢复故障
                              failed
```

- 容器停止/正常 EOF：等待约 5 秒后 inspect；如果历史同步刚完成，直接转入实时读取。
- 网络/暂态 daemon 故障：指数退避 1/2/4/8/16/30 秒，立即重试通过可唤醒等待机制处理。
- 容器不存在：以成功连接到固定 daemon 后的目标检查确认，不能把 daemon 不可达误报为删除。
- 权限、凭据、主机身份校验、驱动不支持、磁盘写入失败：停止采集并提供明确处理入口，不无限重试确定性错误。
- 未识别的 Docker 错误：先做有限次数的健康与目标复查，仍不可判断时停止并展示诊断；不对所有非零退出无限循环。

错误分类集中于纯函数并覆盖夹具，不能散落为多个组件里的字符串包含判断。现有 Docker 检测会读取上下文，日志恢复必须仍针对已绑定 endpoint；复用格式化/解析逻辑时不能意外跟随用户改变的默认 context。

### 6.3 流未结束时的生命周期观测

不能只依赖 EOF 识别所有重启。采集期间增加低频、可取消的 inspect 观测（初始约 5 秒），读取 State.Status、StartedAt、FinishedAt、RestartCount、TTY 等必要字段。

- 使用独立、受控的探测连接，不在已设为非阻塞模式的日志 SSH session 上并行调用阻塞 exec。
- 每个 source 最多一个在途探测；等待/重连阶段复用已有 inspect，不重复探测。
- 探测失败时状态降为未知并退避，不因探测失败立即破坏仍正常传输的日志流。
- 新 StartedAt 只用于记录已确认启动周期；探测可能错过多次快速启动，因此展示“已观测周期”，不伪造完整历史。
- 首次连接到已经运行的容器仅记录基线，不插入“刚刚重启”的虚假事件。

### 6.4 诊断与应用 stderr

现有 CLI stderr 可能同时包含应用 stderr 和命令诊断，本轮不更换传输协议。增加增量帧/行边界识别与诊断隔离区：

- 带合法 Docker 时间前缀的输出及其连续字节进入正常 ingest，支持时间戳、UTF-8 和 ANSI 跨块。
- 仅对明确可识别的 CLI 诊断隔离；无时间戳不自动等于错误，不能删除应用多行内容。
- 无法区分的内容保留原始字节并标注“来源待确认”，默认不混作普通应用记录；与非零退出、再次 inspect 的结果一起分类。
- 使用有界内存和磁盘暂存处理长内容；诊断区不参与应用日志等级和匹配计数，提供原始诊断查看/导出。
- 驱动不支持、合法 stderr、无换行输出、分块时间戳必须共同通过回归，不能只修一个报错样本。

该分类方案只能改善 CLI 可识别范围；未知格式保持明确的不确定状态，不承诺绝对分离。若实测证明必须完全分离，再单独评估 Engine API 传输改造。

## 7. 存储兼容、系统事件与启动周期

### 7.1 辅助数据库

在每个 source 目录增加 `lifecycle.db`，使用独立 schema 版本，不改主 `segment-*.db` 的版本 3，不修改既有 records.seq、sourceId 与去重身份。

拟包含：

- `events`：event_id、kind、observed_at、actual_at、anchor_seq、boot_id、attempt_id、受限诊断详情。
- `boots`：boot_id、container_id、started_at、observed_at、finished_at、结束时间是否可靠、观测覆盖状态。
- `meta`：辅助格式版本及必要检查点。

旧存档没有辅助库时返回空事件与未知周期，不为打开离线存档强制执行写迁移。新库初始化使用事务；不改变 log_ingest 现有 upgrade 的版本逻辑。

### 7.2 原文与事件边界

- 应用原文提交成功后，事件引用已提交 anchor_seq；不能产生指向未提交记录的事件。
- 主库与辅助库不是跨库原子事务，不承诺崩溃时事件零缺失。原文优先，辅助写失败要显式提示“运行事件保存失败”，不得误报为原文丢失。
- 连续重试只维护状态，去重事件按状态转换或实际恢复写入，不把每秒倒计时写入数据库。
- 每个事件有独立 ID 与分页游标；不会占用应用日志 seq，也不会改变记录总数、序号复制或默认原文导出。
- 采集事件的 anchor 代表本地记录顺序，不是精确的远端发生位置。跨断连补读时标明检测时间，不把事件行冒充精确因果时间线。
- 存档体积统计纳入辅助文件；列表仍只把合法 source 目录作为存档。

### 7.3 启动周期查询

bootId 由 source 和已确认 StartedAt 构成稳定身份。对已观测的起止区间按 Docker 时间查询；停机期间缺失的周期、无时间戳、时钟回退或边界不可信的记录保持未知，不直接归入“上次启动”。当前运行结束时间未知时不能声称历史覆盖完整。

## 8. 筛选导出、原文与任务生命周期

导出请求使用显式 union：all、selection(seqFrom/seqTo)、query(snapshotId)。避免把序号范围和时间范围都命名为 from/to。

- all 在确认导出范围时固定内容视图；query 直接复用搜索快照；selection 校验范围与来源，并固定导出开始时的内容。
- 从 chunks/message fragments 读取原始字节，筛选命中以 records 建立映射。不能把高亮文本、prefix 上下文或截断显示内容拼成“原文”。
- 对只命中长消息中某些片段的筛选，沿用可追溯的记录片段边界，避免无说明地扩成整条消息；需要补完整消息时明确选项并展示范围。
- 无换行尾部不能额外补换行；不同流与长行的原始字节语义需要和旧导出测试对齐。
- 后台分批写入，回报已处理记录/字节、取消与错误。写到目标目录的临时文件，成功后再提交到目标；失败不能留下看似完整的目标文件。
- 确认覆盖由系统保存流程处理；应用不擅自覆盖其他文件。关闭日志窗口取消未完成导出并释放任务，已完成文件不受影响。
- 复制沿用现有 8 MiB 限制并给出导出入口；查询任务与采集任务互不取消。

## 9. 新旧容器来源切换

发现仅使用已有经过验证的连接身份、固定 endpoint、可信 Compose 项目/服务/实例标签。必要时记录 daemon ID；远端 engine 改变时重新验证，不按名称猜测。

- 原 ID 不存在时先保留存档和明确提示，再提供“查找新容器”。
- 后端返回候选的完整 ID、名称、创建时间、匹配依据；多候选由用户选择。
- 点击“查看新容器日志”后后端重新校验候选，调用已有 open_window 创建或聚焦新 source 的窗口，旧窗口继续保留。
- 采用独立窗口复用现有生命周期，避免本轮在一个日志窗口中切换凭据、采集任务和快照；这是功能规划中“显式切换来源”的具体实现选择。
- 从离线存档不能使用陈旧凭据发起探测；须先连接服务器并建立有效上下文。

## 10. Apple Design 的代码落点

- 继续使用 `WindowTitlebar` 与 `theme.css`，不改共享窗口控制布局。局部样式限定在日志窗口作用域，避免影响编辑器和存档列表。
- 工具栏与底部阅读栏作为两个清楚的功能组；搜索输入只按需展开，活动条件保留标签；序号范围输入移入按需操作。
- 菜单/弹层采用按钮锚点定位、窗口边界避让、Esc 关闭和焦点返回；多个弹层按单一打开状态管理，防止堆叠。
- 普通按压反馈用 CSS :active，业务动作在 click 提交；正文 pointerdown 不承担暂停。按钮名称与 aria-pressed/expanded 同步更新。
- 第一版不引入动效依赖：用少量 opacity/transform 过渡或 WAAPI 处理非手势弹层，并能从当前显示状态取消/反向；无日志行入场和长距离滚动动画。
- 搜索栏布局变动保留正文锚点；减少动态效果时静态切换。减少透明度/高对比度时使用实底与可见边界。
- `preferences.ts` 只持久化换行、时间戳、高亮、字号；使用版本化键、值校验和 storage/focus 同步。窗口关闭后不恢复旧查询或暂停状态。
- Ctrl/Cmd+F、Esc、原生选择复制可用；重要状态使用礼貌播报，日志和每秒倒计时不持续进入 live region。
- 原型使用同一批 UI 组件和模拟 adapter，放在开发专用入口或测试 harness；不把模拟重启按钮带进生产界面，不维护两套相互漂移的样式。

## 11. 测试文件与执行计划

### 11.1 单元与浏览器测试

新增拟定文件：

- `src/logs/model.test.ts`：暂停原因、恢复、搜索/上下文返回、旧代次事件丢弃、重连保持阅读意图。
- `src/logs/query.test.ts`：无命中游标推进、旧记录变更、不重复计数、快照句柄释放、乱序请求。
- `tests/container-logs-ui.spec.ts`：可控 IPC Channel 与动态日志数据，验证实际滚动位置/可见序号/选区，不只检查状态文案。
- `tests/log-fixture.ts`：复用的浏览器 IPC 模拟 adapter，区别于真实 SSH 夹具。

更新 `playwright.logs.config.ts`，同时运行存档列表与实时窗口用例。浏览器覆盖常规窗口、650×400、200% 文字、浅深色、减少动态效果、键盘与焦点、搜索条件和导出范围。

### 11.2 Rust 与 SSH 集成

- 在 protocol/collector 模块加入错误分类、退避、取消与唤醒测试；状态转换使用可控时钟/策略，不靠长时间 sleep。
- 扩展 log_store/log_ingest 回归：同 seq 续写通知、跨分片尾部、合法重复、稳定快照、筛选原文导出、冻结失败清理。
- 为 lifecycle 辅助库增加旧存档无写入读取、事务中断、辅助写失败不破坏原文、事件锚点验证。
- 扩展 `tests/ssh_fixture.py`：EOF、restarting/StartedAt、暂态 daemon 错误、权限、删除、恢复、多块 stderr、未换行长行；保留原有 fixture 模式兼容。
- 更新 `tests/features.spec.ts`，保留窗口关闭计数、主窗口关闭后日志独立运行、离线读取等原生回归。

### 11.3 真实 Engine

新增 `src-tauri/src/logs/engine_tests.rs`（显式 ignored 集成测试）复用现有嵌套 Engine 环境，覆盖 restart、stop/start、restart policy、删除重建、不同日志驱动可读性、TTY 与非 TTY。

daemon 故障测试只操作专用嵌套 daemon。若需支持受控重启，修改 `tests/docker-engine/start.sh` 与 environment helper 提供专用启停流程；测试 finally 必须恢复/清理，不操作宿主 daemon。

原生 UI 另补针对真实环境的可选用例或配置，保证真实 restart 与前端阅读锚点一起验证。模拟 IPC 通过不能替代此项。

### 11.4 计划命令

以下只在实现阶段执行，本轮没有运行：

```text
npm run test -- src/logs/model.test.ts src/logs/query.test.ts
npm run test:logs
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml log
npm run test:desktop -- --grep "container logs"
cargo test --manifest-path src-tauri/Cargo.toml logs::engine_tests -- --ignored --nocapture
```

原生与真实 Engine 命令需要桌面调试环境及隔离 fixture。环境缺失时明确记录未执行项目，不把测试降级后称为全部通过。若改到 Docker 公共检测辅助代码，再运行受影响的 Docker 单测与管理回归。

## 12. 验收、风险与收尾

| 风险 | 控制方式 | 验收证据 |
| --- | --- | --- |
| 仅状态恢复，日志或视图未恢复 | 同时断言新记录标识和视口底部/锚点 | 重启前后实时跟随与暂停两组真实记录 |
| 暂停被布局变化误触发 | 用户意图与滚动结果结合，区分程序定位 | 换行、改字号、缩放、插入事件后阅读状态不变 |
| 重连覆盖已冻结尾部 | 真正快照冻结可变 messages | 多流、旧分片、未换行续写后的查询/导出字节一致 |
| 快照造成 WAL 或磁盘增长 | 短期读取事务、流式冻结、取消与缓存回收 | 大尾部、创建失败、窗口关闭、异常退出回收测试 |
| 单来源产生重复 worker | 保留原排他流程，重试只唤醒 | 重试连点、重开、关闭时计数与原文无重复 |
| CLI 诊断误当原文或误删 stderr | 保守分类、未知隔离保留、格式夹具 | 驱动失败与合法多行 stderr 同时通过 |
| 启动周期被误推断 | 仅使用已确认生命周期与可用时间边界 | 首次连接、快速多次重启、时钟异常、缺失周期保持未知 |
| 视觉重做影响可用性 | 原生滚动、token 复用、焦点和降级验证 | 紧凑布局截图与键盘操作记录 |
| 新辅助数据损害旧存档 | 独立 schema，不改原始 seq/bytes | v1/v2/v3 旧归档回归及原文校验 |

完成后交付代码差异、实际执行的检查结果、真实重启验证记录、关键状态截图和仍存在的可确认限制。不把 UI 原型、模拟重连或文字状态正确当作生命周期完整通过。

## 13. 核对参考

- 项目现有类型：`node_modules/react-virtuoso/dist/index.d.ts` 中的滚动/状态接口。
- Virtuoso 官方 API：`https://virtuoso.dev/react-virtuoso/api-reference/virtuoso/`，用于核对滚动与测量接口；实现以项目安装版本为准。
- Docker 官方 logs 文档：`https://docs.docker.com/reference/cli/docker/container/logs/`，用于核对 follow、since 与 timestamps 语义；其说明不等于本应用跨重启恢复保证。
- Apple Design 本地技能：`C:/Users/bgaso/.codex/skills/apple-design/SKILL.md`。

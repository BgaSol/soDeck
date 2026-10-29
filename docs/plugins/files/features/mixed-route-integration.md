# T-027：文件插件混合连接接入计划

文档就绪：可评审 · 修订：r4 · 更新日期：2026-10-29\
所属：[文件插件](../README.md) / [F-FILES-001](server-file-tree.md)\
依赖：[F-CORE-031 r2](../../../core/mixed-route.md)、[代码方案r1](../../../technical/mixed-route-implementation.md)、[T-027 r2](../../../tasks/T-027-mixed-route.md)

## 1. 文件应有的行为

文件树读取最终目标T的SFTP/有界exec结果，不展示A/B跳板的目录。自动观察仍只通过connections.observe/acquireExisting使用已存在目标连接；仅用户点“连接并读取文件”才发起整条链及必要认证。文件插件不依赖打开终端，也不直接调用终端或网络隧道插件。

混合路线改变、断线或迟到读取不能把旧连接的目录、cwd、挂载和容量结果写进新连接。原树展开/隐藏文件/容量等级/目录跟随业务保持，API5兼容路径不在本次实现。

## 2. 代码修改

| 文件 | 计划 |
| --- | --- |
| `plugins/files/src/state.ts` | 当前hasConnection/verify查询route running子集，改为SDK6明确类型和connectionId+generation+routeRevision校验；去除缺路由能力时的旧核回退。显式connect错误采用SDK统一认证分类，preset确认可回到同操作继续；自动观察不暗中acquire。 |
| `plugins/files/src/view.tsx` | “连接并读取”使用核心预算helper，预算/认证/取消错误各自显示；不在文件面板另建代理密码框，仍交宿主公共流程。 |
| `plugins/files/src/follow.ts`、`capacity-cache.ts`及`state.ts`失效路径 | 核实并补齐缓存键的connectionId/generation及请求代次，连接变更时丢弃正在返回的旧cwd/容量/目录查询；不以serverId相同就复用旧结果，不更改容量计算业务。 |
| `plugins/files/src/state.test.ts`、`state-observation.test.ts`、`follow.test.ts` | API6 strict status夹具；同server不同路线的迟到读/观察、无现成连接不拨号、认证继续/取消、文件独立连接覆盖。 |
| `plugins/files/manifest.json`、`package.json`及包锁 | SDK6、`pluginApi:^6.0.0`、`connections.route.v2`；候选版本0.9.0，独立构建打包。 |

远端文件/挂载协议不增加SOCKS字段；网络改动留在核心。仅确有缺口时补失效逻辑，已有正确的generation隔离保留并通过专项证明，不为了迁移重写整个Explorer。

## 3. 插件专属测试

适用r1，对应MR-AC-01/03/04/05。准备独立混合拓扑，T有只读测试目录与文件，A/B放不同标记便于区分；Windows原生文件包与真实SFTP/exec，模拟桥另测竞态。

| 用例 | 步骤 | 预期 |
| --- | --- | --- |
| FILES-MR-01 独立显式连接 | 无终端时打开T文件面板，先观察，再点击连接并完成各节点挑战；读取T目录/大小/挂载 | 初始不发起任何网络链；显式操作后只读取T，代理不变成目录目标；失败保持可重试界面 |
| FILES-MR-02 共享与释放 | 已连T终端后打开文件，展开目录，再关闭文件/一个终端/整个工作区 | acquireExisting使用实际目标lease；自动刷新不重建链，单消费者释放不误关其他连接，工作区关闭全部回收 |
| FILES-MR-03 修改和迟到 | 延迟旧目录/cwd/容量结果，修改混合路径并重新连接同serverId，再放行旧响应 | connection generation/routeRevision隔离，旧结果不污染树/跟随/容量；UI显示新连接实际状态 |
| FILES-MR-04 认证和包 | P preset确认/错误密码/取消后重试；核心预算内慢链；API6环境仅更新文件A→B并重启 | 不误报读取失败或自动重试风暴；预算不提前超时，关闭取消清理完成，其他插件包不变 |

每例取消所有读取，关闭文件view与独立目标连接，清除临时文件/凭据及测试数据；保证日常文件不读写。记录实际SFTP/exec与原生证据，模拟结果单列。

## 4. 状态与结果

已按本计划完成 API6 route status、连接代次、缓存隔离和显式连接行为。实测、包版本、限制和验收见 T-027，不改变原文件树/容量任务结果。

## 2026-10-29 验收修订计划（已实施）

确认期间等待共用目标连接，不触发第二套确认；补确认完成后目录首读仅一次、取消后可重试和旧代次结果隔离。独立构建公共 UI 更新，核对已有目录/选择器控件状态。

来源：[整体修订 r2](../../../design/connection-editor-refinement.md)，对应 REF-01/04～08；与下方诊断增量合并评审。包版本核对在途版本后递增，不能只更新宿主而宣称本插件完成。具体方案已获确认并实施，结果由T-027 r9及实施记录维护，原r2交付记录保留。

## 逐步诊断增量（r4，已实施）

公共行为见[诊断方案 r1](../../../technical/connection-diagnostics.md)。`state.ts` 的显式连接错误、观察状态及快照增加结构化连接故障引用；view 的命令结果和 snapshot.message 使用同一错误来源。`view.tsx` 去除同一连接错误红/黄两条叠加，展示失败摘要及宿主详情入口；按 attempt/round/phase/errorId 去重，不按文本吞掉独立目录权限、挂载、跟随或容量错误。被动观察只读取同工作区/目标/session 用途的相关尝试，已有 lease 优先；不因其他终端失败覆盖仍可用的目录。manifest/SDK/包声明新能力并单独发布。

准备：隔离混合路线，终端+文件同工作区，受控 SOCKS EOF 与 SFTP 拒绝夹具，沿用 FILES-MR 环境；具体执行证据见T-027结果。

| 编号 | 步骤 | 预期 |
| --- | --- | --- |
| FILES-DIAG-01 | 点击连接并读取使 SOCKS 失败，命令 catch 与 worker 状态同时到达；交错后再查看详情 | 同面板只一条连接错误，详情是同一 attempt，未连接目标不显示目录读取失败；独立主题/目录错误不被合并。 |
| FILES-DIAG-02 | 终端发起连接，文件被动等待确认→失败→重试成功；另一个终端失败时已有文件 lease 可用 | 解释对应等待/失败，不再发认证/建连；成功首读一次，已有目录不被无关新尝试清空；旧 generation 结果丢弃。 |
| FILES-DIAG-03 | SSH 成功后目录访问拒绝；编辑路线，打开旧详情；单包更新及三主题原生展示 | 目录错误仍属文件业务，连接阶段为成功；旧路线不被新配置替代，引用过期明确；只有文件包更新且资源释放正常。 |

清理目录读取/跟随任务、lease、观察订阅和测试文件；恢复界面设置。保留截图和故障引用核对证据，实际结果写 T-027-results。

## 本轮实现版本

插件 0.10.0 已独立构建和打包，使用SDK6.1/UI1.8，声明公共诊断与统一确认能力。命令/状态共用引用，同故障只显示一次；按工作区/目标/session观察连接失败，独立SFTP目录错误保留。 本插件定向状态/worker测试、真实混合链原生流程和独立单包更新均有本轮证据，统一见 [T-027结果](../../../tasks/T-027-results.md)。测试候选包只用于隔离更新验证，交付仍使用本段版本；用户验收和发布未发生。

# F-TERM-004：终端事件与诊断接入

修订：r2 · 日期：2026-10-27 · 文档就绪：功能需求可评审，已按T-016确认接入
插件：sodeck.terminal · 总览：[终端](../README.md) · 任务：[T-016](../../../tasks/T-016-observability.md)
公共规则：[核心接入需求](../../../core/observability/integration.md)、F-CORE-018/019/021/022

代码对接固定于[T-016代码方案r1 §3、4、6、7](../../../technical/observability-implementation.md)：新增独立observation安全投影和签名采集目录，API/SDK4.10，经metadata传context，沿实际任务终态采集。版本、修改位置、兼容/资源和验证关口见实施稿；本规格6项功能流程继续适用，并由OCODE-10/13验证真实边界与独立包。已在对应独立插件包接入，执行结果与尚未完整覆盖的流程见[T-016结果](../../../tasks/T-016-results.md)。下方“当前”实现缺口是原设计基线。

## 1. 目标和范围

让一次新建/重连/取消/关闭终端能关联核心调用、真实会话状态和目录集成健康。终端仍拥有会话业务，核心拥有SSH/PTY资源与认证事实；不录制终端按键、粘贴、命令、屏幕或输出正文，不新增终端诊断页面/提醒。

当前源码：`plugins/terminal/src/worker.ts`直接使用runtime.sodeck，维护sessions/pendingOpens及open/close/cancel；`directory-reporter.ts`负责目录集成，UI上报sessions.status。新接入必须覆盖真实业务完成路径，不能仅包装invoke入口。宿主prepare/activate/退出异常由核心记录。

## 2. 动作、事件和字段

所有类型使用`plugin.sodeck.terminal.`前缀、typeVersion=1。下表字段为本插件允许采集的增量；宿主自动补source、时间、trace、workspace/server和代次。errorCode仅稳定码，任何error.message或title原文不直接入库。

| 动作入口 / 事实 | 事件类型及触发点 | 安全业务字段和粒度 |
| --- | --- | --- |
| terminal.new / sessions.create/open | action根由外壳或SDK入口产生；`session.open.started/completed`在open实际开始/结束 | sessionId、attemptId、mode=new/reconnect、durationMs、outcome；首次工作区初始化继承外壳trace |
| terminal.reconnect / sessions.reconnect | 同一session的新attempt，关联旧连接代次；不得覆写旧attempt | retryOf、previousState、newResourceRef；新的人工重试是新动作，自动步骤是子阶段 |
| 等待认证/信任输入 | `session.state.changed`在业务状态确实改变时 | from/to、reasonCode、attemptId；waitingAuth只表示需要输入，不假装最终连接失败/成功 |
| 会话就绪/远端结束/断线 | `session.state.changed`，ready依据PTY建立成功；结束依据核心资源状态 | observationBasis=worker-observed/core-response/ui-reported区分依据；可信source仍由宿主绑定；核心资源ref和代次，未知退出码不编造 |
| sessions.cancelOpen / lifecycle.cancel | action请求与最终取消结果分开；open的对应attempt结束为canceled | targetAttempt、cancelOrigin、outcome；取消迟到已成功时报告真实终态，不能改写已建会话 |
| sessions.close / lifecycle.close | `session.close.started/completed`，以真实资源释放/失败为终态 | sessionId、reason=user/workspace-close/app-exit、durationMs、cleanupCode |
| 清理部分失败 | `session.cleanup.failed` | sessionId、originalCode、cleanupCode、未清理资源ref；与原失败不同事实，不丢原始原因 |
| 重命名、字体/目录集成偏好保存 | `session.metadata.changed`、`settings.changed`，仅保存成功后 | changedFields、旧/新修订或数值枚举；不保存新旧标题和自由文本值 |
| 目录集成启动/健康变化 | `integration.state.changed`、`integration.report.failed` | mode、state=starting/ready/unsupported/degraded/stopped、reasonCode、sessionRef、directoryRevision |

核心已经有call/SSH/PTY生命周期时，插件只记录业务关联和影响，不复制核心认证事件。同一状态重复轮询不产生state.changed。正常目录报告的每个提示符不记业务事件；真实cwd变化只使用已有核心目录契约的修订关联，不采集路径和shell集成nonce。

一次等待输入的open attempt可结束为needs-input（事件outcome=unknown，业务state=waitingAuth），后续输入重试建立新attempt并关联；不得把同一重试记录为新session。会话创建是一次事实，移窗/表面恢复/重新绑定输入不再次产生created。业务状态无新证据时保留last-known，而不是根据UI仍显示Tab断言终端运行。

## 3. SDK对接和生命周期

在prepare绑定采集客户端和类型目录，activate上报一次安全会话快照；prepare/activate错误由宿主兜底记录。命令上下文由宿主→worker传递；`open`与`mutate`的异步任务显式携带绑定client，各session的trace不得互相覆盖。

`sessions.status`来自UI时只产生ui-reported观察记录，核心资源终态仍为权威。已有`refreshStatuses`从核心查询得到状态可以校验实际资源；不能每秒无变化仍生成新事实。按attempt/session代次校验迟到更新；不需要为诊断增加额外SSH连接或PTY读取者。

`open`的cleanup错误和原错误分别留痕；`close`即使上报失败也继续释放。正常stop先结束业务/停止生产再限时flush，插件或UI崩溃交给宿主记录中断及缺口。未持久化的reporter异常不再无痕吞掉，记录稳定失败码和health退化，但不能改变现有目录集成功能的回退语义。

## 4. 公开现场与指标

快照`terminal.sessions.v1`：按工作区主动上报会话计数、各状态计数、目录集成enabled/状态、待处理attempt数、最近稳定错误码；相关session详情最多32项，每项只含opaque sessionId、状态和核心资源诊断ref。超过32项返回total/truncated与覆盖说明，不能塞入所有PTY字段。事件及显式查询可补充单个会话证据。

指标：`session.open.duration`、`session.reconnect.duration`（ms histogram）；`session.open.result`（counter，outcome枚举）；`session.active`（gauge，state有限枚举）。最多5秒一次快照，变化立即在限额内标dirty并合并；初始化/故障可按公共策略触发固定副本。终端字节量/带宽由核心communication服务提供，插件不再重复上报。

日志默认仅状态变化、关键失败、清理失败；debug可记录阶段耗时、计数和验证状态。终端正文、退出时最后屏幕、粘贴内容及包含命令的异常文本无论debug开关均不采集。

## 5. 兼容和功能验收

旧包在新核心保持原会话行为、只有核心审计；新采集包声明必需的事件/context/snapshot/metric能力，缺失时安装兼容校验拒绝，实际API版本在代码方案固定。存储运行故障仅降级附加诊断，既有核心审计失败边界保持；不因日志问题自动重连。

AC-01动作/会话/核心阶段正确关联；AC-02生命周期和失败真实；AC-03无正文、无流量重复且兼容可见。

准备：Windows隔离App与真实终端包、两个SSH目标，可控认证挑战、断线和延迟；测试从终端功能既有入口扩展，新用例尚未实现/运行，结果归T-016。

| 用例 | 操作 | 预期 |
| --- | --- | --- |
| TERM-OBS-01 / 01 | 同时新建两服务器终端，一成功、一等待认证后取消，再重试 | trace/attempt不串，waitingAuth不写成功，重试新attempt但同逻辑session，核心认证不重复上报 |
| TERM-OBS-02 / 01、02 | 连接中移窗/重挂载、sessions.status迟到、远端退出 | 无重复创建，旧代次不改新状态；UI报告和核心事实区分，未知退出码不猜 |
| TERM-OBS-03 / 02 | PTY建立后注入发布/释放失败、取消与完成竞态、强杀worker | 原因和清理错误均可查；实际中断/结果不明正确，无日志触发业务重放 |
| TERM-OBS-04 / 03 | 输入测试秘密命令/粘贴标记、重复提示符与cd、debug启用后查询/导出 | 无正文/路径/nonce，正常重复提示符不刷事件，目录健康变化有证据 |
| TERM-OBS-05 / 02、03 | 上报不可用和队列拥塞时正常打字/关闭/退出，再恢复 | 不阻塞终端和释放，失败/缺口可见；计量只来自核心，flush不超过公共时限 |
| TERM-OBS-06 / 01、03 | 旧包/新包兼容组合，单独升级终端；停止插件再读历史描述和快照 | 原业务不被诊断接入改变，旧包audit-only，新包缺能力明确拒绝；历史仍可解释 |

清理：取消attempt、关闭所有会话和测试App，终止隔离SSH，恢复故障注入；核对PTY/lease/队列归零，保存脱敏trace/事件计数后删除测试数据。

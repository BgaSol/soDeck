# F-PROXY-004：网络隧道事件与诊断接入

修订：r2 · 日期：2026-10-27 · 文档就绪：功能需求可评审，已按T-016确认接入
插件：sodeck.network-proxy · 总览：[网络隧道](../README.md) · 任务：[T-016](../../../tasks/T-016-observability.md)
公共规则：[核心接入需求](../../../core/observability/integration.md)、F-CORE-018/019/021/022

代码对接固定于[T-016代码方案r1 §3、4、6、7](../../../technical/observability-implementation.md)：新增独立observation安全投影和签名采集目录，API/SDK4.10，经metadata传context，沿实际任务终态采集。版本、修改位置、兼容/资源和验证关口见实施稿；本规格6项功能流程继续适用，并由OCODE-10/13验证真实边界与独立包。已在对应独立插件包接入，执行结果与尚未完整覆盖的流程见[T-016结果](../../../tasks/T-016-results.md)。下方“当前”实现缺口是原设计基线。

## 1. 目标与现状

固定端口转发/HTTP/SOCKS内网代理都可追踪规则变更、启停、重连、协议失败和连接检查。当前worker路由到Rules/Editors/Manager/Run，Run.log只保留本次运行最多200条诊断。新增公共事件成为持久历史权威，原数组可暂作“最近诊断”兼容缓存；不要求核心读取它，不新增问题列表、知悉或通知入口。

核心负责SSH/TCP/listener/流生命周期，插件负责代理协议、规则和运行状态。核心communication继续唯一负责通信字节/速率/历史；插件不把自己的bytesRead等再次累加进全局流量。

## 2. 动作与事件目录

类型前缀`plugin.sodeck.network-proxy.`、typeVersion=1。共同上下文ruleId、ruleRevision、runId、server/workspace/代次、trace/attempt、适用核心诊断ref。用户自填规则名、主机/URL、用户名密码、完整错误消息不默认入库。

| 操作 / 事实 | 事件与时点 | 安全字段及粒度 |
| --- | --- | --- |
| network.editor.save / network.save / network.delete | `rule.saved/deleted`，Rules实际持久化提交成功后 | ruleId、revision、kind=port-forward/network-proxy、changedFields、portMode；编辑弹窗关闭不代表保存成功 |
| network.start | action与`run.start.started/completed` | runId、ruleRevision、attempt、outcome、durationMs；命令受理不代表listener已经ready |
| Run实际状态变化 | `run.state.changed` | from/to、reasonCode、routeRevision、资源ref；含starting/ready/reconnecting/needs-attention/stopping/stopped/failed |
| 自动重连/人工重连 | `run.reconnect.started/completed` | trigger=automatic/user、retryIndex、backoffMs、retryOf、outcome；自动重连继承运行关联，人工重试另起action |
| network.stop / lifecycle.close | `run.stop.started/completed` | reason=user/view-close/workspace-close/app-exit、cleanupCode、remainingResourceCount；仅隐藏侧栏不停止/不记stop |
| network.check | `check.started/completed` | checkId、runId、revision、protocol=http/socks/tcp、targetRef、state、durationMs、errorCode |
| 协议协商失败/目标不可达 | `client.failed` | reasonCategory、protocol、stage、errorCode、runId、coreStreamRef/targetRef；不得保存握手、URL路径、认证头或正文 |
| 清理/停止异常 | `cleanup.failed` | stage、originalCode、cleanupCode、资源ref；与源错误分开，不重复上报同一个catch链 |

HTTP/SOCKS错误按预先声明的reasonCode映射，如unsupported-method、malformed-request、target-unreachable；字段来自解析后的枚举，不截取任意请求原文。固定目标/动态目标用不含地址的targetRef与核心诊断资源ref关联；不把host/port拼接字符串作为全局聚合键或指标标签。端口模式和实际本机监听端口可作为数值记录，监听IP默认只表示loopback类别。

每次已处理的客户端失败保存一次事实，不因相同错误而采样丢弃；超过全局上报容量时明确rejected/drop计数。高频正常代理请求和每个数据帧不生成业务事件，只记录核心已有生命周期及插件区间指标。未来问题聚合可利用这些事件，当前不增加自动提醒机制。

## 3. 业务和SDK连接点

采集client在worker.prepare绑定，Manager为规则的每次新Run分配上下文；Run.execute传给核心操作，targetOperation队列闭包必须保存当时上下文。当前网络桥有12/8/4 lane，诊断走公共有界采集调度，不能挤占停止、state.ack和资源清理额度。

Run的status setter只在真实变化时产生state.changed；用一次异常的本地failureId在target/serve/accept的catch链去重，主处理者报告client.failed，cleanup报告独立的失败。Run.log的安全结构可同时更新原200条显示缓存及公共事件队列，但公共事件拒绝/未持久化状态必须可从health/覆盖查询得知，不能称本地缓存已经持久化。

规则保存的成功以Rules存储回执为依据，Editors的accepted或弹窗关闭不替代提交；并发版本冲突保存一条失败action，无rule.saved。删除先停止运行，若停止/删除任一步失败，分别记实际结果，不能宣称全删除成功。

Run.test当前失败可返回`state=failed`而不抛异常；终态必须读取CheckResult，不用Promise fulfilled判成功。手动停止/规则修订/新Run后迟到check结果只关联原run/revision，不改变当前状态。重连不清空旧失败历史，Run对象替换也不删除公共记录。

## 4. 现场、指标和覆盖

快照`network.runs.v1`：每工作区规则/运行数、各状态计数、每个运行的ruleId/revision/runId、protocol、portMode/本地端口、routeRevision、activeChannelCount、retryIndex、最近检查的稳定状态/错误码。最多16个运行详情，其他规则仅计数；运行结束快照明确ended，不能重启后当作仍监听。

指标：`run.start.duration`、`run.reconnect.duration`、`check.duration`（ms histogram）；`client.failure`（counter，protocol/stage/errorCategory有限枚举）；`run.active`和`client.active`（gauge）。不使用host/URL/streamId/requestId标签、不重复report流量；问题优先级不是协议错误计数自动推导的。

上报失败不停止代理、不触发新重连、不影响业务检查结果；既有核心审计不可写的保护保持。事件、快照及指标均使用公共配额/flush，正常停止不为保存日志延长socket生命周期。

## 5. 验收与测试流程

AC-01规则与运行事实准确；AC-02重连/检查/异步归属正确；AC-03数据/计量/容量正确；AC-04兼容与历史可读。

准备：Windows隔离App、真实网络包、隔离SSH跳板/HTTP/SOCKS/TCP测试服务，自动与固定端口，受控断线/超时/坏协议；新增流程拟扩展既有network测试，尚未运行，结果归T-016。

| 用例 | 操作 | 预期 |
| --- | --- | --- |
| PROXY-OBS-01 / 01 | 创建/修改/删除规则，注入保存失败/版本冲突/删除前停止失败 | 提交后才saved/deleted，半完成和失败可查，rule/run与修订分开 |
| PROXY-OBS-02 / 01、02 | 两类规则并行启动，断线自动重试后人工重连，停止期间旧回执返回 | 来源/trigger/attempt明确；状态变化只一次，旧代次不覆盖新Run |
| PROXY-OBS-03 / 02 | 检查成功/失败都正常返回Promise，检查中修改规则或重启Run | 按CheckResult判终态，失败不写成功；迟到结果关联原run/revision |
| PROXY-OBS-04 / 03 | 错误经target→serve→accept多层catch，产生大量坏协议流量并运行正常隧道 | 每个失败一个领域事实，cleanup独立；队列有界、缺口可见，正常业务/停止不饿死 |
| PROXY-OBS-05 / 03 | 测试URL/头/凭据标记和不同目标；开启核心通信监控与debug | 无正文/可逆目标文本；指标无高基数，核心流量不被插件重复累加 |
| PROXY-OBS-06 / 01、04 | 单独升级网络包、隐藏/移窗、关闭再启动App，读公共历史和原最近诊断 | 隐藏/移窗不记stop；旧包audit-only，新包缺能力拒绝；历史不随Run缓存消失，快照不是运行恢复 |

清理：停规则、关listener/stream/group/路由租约，停止测试App与隔离服务、恢复故障注入；确认端口/队列回基线，保留脱敏事件与计量对照后删除测试数据。

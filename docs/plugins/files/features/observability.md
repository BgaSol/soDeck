# F-FILES-003：文件事件与诊断接入

修订：r2 · 日期：2026-10-27 · 文档就绪：功能需求可评审，已按T-016确认接入
插件：sodeck.files · 总览：[文件](../README.md) · 任务：[T-016](../../../tasks/T-016-observability.md)
公共规则：[核心接入需求](../../../core/observability/integration.md)、F-CORE-018/019/021/022

代码对接固定于[T-016代码方案r1 §3、4、6、7](../../../technical/observability-implementation.md)：新增独立observation安全投影和签名采集目录，API/SDK4.10，经metadata传context，沿实际任务终态采集。版本、修改位置、兼容/资源和验证关口见实施稿；本规格6项功能流程继续适用，并由OCODE-10/13验证真实边界与独立包。已在对应独立插件包接入，执行结果与尚未完整覆盖的流程见[T-016结果](../../../tasks/T-016-results.md)。下方“当前”实现缺口是原设计基线。

## 1. 目标与当前路径

记录文件读取、定位、目录跟随和容量采集的真实结果及退化原因，仍保持只读文件业务。当前worker调用Explorer.schedule/load/refresh/locate及FollowController；部分入口立即返回accepted/revision，异步任务继续运行。因此开始/终态必须接在Explorer/FollowController实际任务边界，不能用invoke成功代表目录已读完。

`files.diagnostics`已有跟随耗时、节点/缓存、容量缓存、snapshotTiming；新增公共快照由worker主动上报，核心不调用这个私有方法。当前节点ID可能是路径bytesBase64，严禁直接当安全对象ID持久化；使用SDK为工作区内目录对象分配的随机directoryRef，映射仅在插件内存中、随节点释放，不保存原始路径/文件名/base64或可逆路径指纹。

## 2. 动作与事件目录

类型前缀`plugin.sodeck.files.`、typeVersion=1。共同上下文含工作区/代次、serverId、viewId、action/attempt、适用的核心连接诊断ref；文件节点只用directoryRef。

| 入口 / 实际事实 | 事件 | 字段与粒度 |
| --- | --- | --- |
| files.connect / 首次自动读 | `explorer.connect.started/completed` | trigger=user/workspace-init/visible-resume，durationMs、outcome、errorCode；用户动作继承宿主上下文 |
| files.refresh / files.locate / files.more | action开始/受理；实际队列执行时`directory.operation.started/completed` | operation=refresh/locate/readMore、directoryRef、operationRevision、durationMs、outcome |
| 展开触发真实读取 | `directory.read.completed` | directoryRef、entryCount、pageCount、durationMs、cacheHit、outcome、errorCode；同一次多页读取按attempt串联，不每个条目一事件 |
| 缓存展开/折叠/选择/滚动 | 默认不产生业务事件 | 保持既有调用审计；缓存命中可进入指标，不把虚拟列表渲染当用户活动日志 |
| 切换跟随模式/手动定位一次 | `follow.preference.changed` / action与实际定位终态 | enabled、reason=user、revision；不保存输入路径 |
| 自动跟随目录 | `follow.locate.completed` | trigger=directory-change/source-change、sourceSessionRef、directoryRevision、result=applied/canceled/superseded/unavailable/failed、mappingMs/locateMs |
| shell不支持/目录无法映射/连接中断 | `follow.state.changed` | from/to、reasonCode、sourceRevision；只在状态变化记录，恢复同样有事实 |
| 挂载信息/容量采集 | `mount.collection.completed`、`capacity.state.changed` | method=kernel/df/other固定枚举、mountCount、available/unsupported/failed、errorCode、durationMs；缺系统能力为degraded，不一律error |
| files.cancel / 切换工作区 / 关闭 | 原操作终态canceled/superseded；`explorer.closed` | reason=user/context-change/view-close/app-exit、未完成计数；取消请求不等于远端执行被撤销 |

文件读失败是业务事实；核心files/exec调用失败仍由网关记录，插件不能重新产生一条“核心失败”。同attempt若多个catch处理同一异常，只由拥有终态的任务报告一次；附属挂载/容量失败与目录成功分别保留，不把部分可用改成整个文件树失败。

## 3. 异步与目录联动

用户动作的context随schedule闭包传到实际执行，排队返回只结束调用span，不结束action。被新选择替代的任务记录superseded，已取消工作不写成功；若远端读取已经结束但UI结果过期，记录“读取成功、结果未应用”，不更改真实执行结果。

FollowController继续订阅核心目录状态，不直接订阅终端诊断事件驱动定位。自动跟随形成后台操作，使用已有source session与directoryRevision建立显式关系；需要完整因果时由核心目录所有者附加可验证关联，不在插件之间传trace token。没有因果证据时仅列资源/修订关联。

隐藏视图沿用停止自动读取和订阅的原行为，记录状态变化一次；周期tick、files.snapshot、files.row和files.diagnostics查询不产生新的业务变化事件。切回视图时按当前代次重建上下文，不能将隐藏期间旧定位结果记到新目录。

## 4. 现场、指标与失败处理

快照`files.explorer.v1`：state/visible/followEnabled/followStatus、sourceSessionRef与directoryRevision、节点数/缓存字节/队列长度、挂载能力状态、容量队列active/pending/cacheCount、最近稳定错误码。禁止完整目录树、路径、文件名、符号链接目标和远端exec正文；保留现有私有诊断时也不能将其原对象直接序列化到公共存储。

指标：`directory.read.duration`、`follow.mapping.duration`、`follow.locate.duration`、`capacity.collect.duration`（ms histogram）；`directory.read.result`（counter，outcome枚举）；`explorer.cache.bytes`和`explorer.queue.depth`（gauge）。只允许operation/outcome/method等低基数标签，不以directoryRef、路径、文件名或serverId作为通用指标标签；服务器范围可在有界采集目标中表达。

新诊断不会触发额外ls/df/exec，只消费业务已经取得的结果。快速节点变化按公共快照节流合并；事件终态不采样，超额按公共规则拒绝并显示coverage缺口。只读业务结果不因附加上报失败变成失败，不恢复已取消任务来补日志。

## 5. 验收与测试流程

兼容沿用公共接入：旧包audit-only；新采集包required能力缺失时明确不兼容；采集暂时失效可降级，原核心审计规则保持。AC-01真实异步终态；AC-02联动/取消归属正确；AC-03数据最小化与性能边界；AC-04独立兼容。

准备：Windows隔离App、真实文件与终端包；隔离Linux SSH及OpenWrt/缺能力夹具；多页目录、延迟/权限失败和不可映射cwd。拟扩展现有files/follow测试入口，以下新增流程未运行，结果归T-016。

| 用例 | 操作 | 预期 |
| --- | --- | --- |
| FILES-OBS-01 / 01 | 刷新/定位立即accepted后分别成功、失败、排队取消；多页目录 | 受理不写成功，实际终态与attempt匹配，条目数量不等于事件数量 |
| FILES-OBS-02 / 02 | 快速切终端/cd/手动定位、隐藏视图、重开工作区，交错迟到结果 | 目录修订/来源正确，superseded与实际成功分开，无终端诊断事件驱动业务 |
| FILES-OBS-03 / 01、03 | 目录可读但挂载/容量命令不可用；恢复后再次读取 | 部分可用和unsupported可见，恢复只在真实成功后报告，无重复错误风暴 |
| FILES-OBS-04 / 03 | 秘密标记路径/文件名/链接目标及base64节点ID，debug下查询/导出 | 原文/可逆ID不入库，directoryRef不可据此还原路径；没有额外远端采集命令 |
| FILES-OBS-05 / 01、03 | 高频虚拟列表snapshot/滚动/缓存展开，暂停事件写入后取消/关闭 | 无重复业务事件，指标/快照有界；队列和连接释放不受上报阻塞 |
| FILES-OBS-06 / 04 | 单独更新文件包，新旧核心组合；停止worker后读公开快照/类型 | 能力兼容明确，终端无需重编译；旧快照stale/结束，不需要私有diagnostics命令 |

清理：停止自动跟随/读取及事件生产，取消目录/容量任务，关闭测试视图/SSH/App，恢复注入；检查路径映射/队列/lease释放，保留脱敏ID和计数，删除隔离目录与数据库。

# MCP API

本文定义机器人接口插件 0.2.0 的客户端契约。接入、页面操作和限制见[功能说明](features/mcp.md)；核心通用 RPC 见[插件 API](plugin-api.md)，发行证据见插件 [CHANGELOG](../plugins/mcp/CHANGELOG.md)。客户端先进行 MCP initialize 和 tools/list；新增能力以工具发现结果为准。

## 传输与调用

服务为带 Bearer Token 的回环 Streamable HTTP，端点 `http://127.0.0.1:<port>/mcp`。MCP 请求正文最多 32 KiB，总并发最多 32。Host 与可选 Origin 需匹配该回环地址。HTTP 401/403 为接入失败，429 为容量限制。字符串参数按 UTF-8 字节限制；未知参数或工具返回 MCP 协议 invalid_params。认证资料由核心管理。

工具成功使用 structuredContent，文本 content 表达同一 JSON；业务错误使用 isError=true 与 `{"error":{...}}`。工具声明的只读/破坏性提示不能替代实际远端操作判断。

## 目标与启动

| 工具 | 参数 | 返回 |
| --- | --- | --- |
| sodeck_targets_list | 无 | runId、targets；每项含 targetId、context、连接状态、closing 和真实 target 摘要。不可用目标带 error |
| sodeck_exec_start | targetId、requestId、command；可选 timeoutSeconds，默认 300，1–3600 | runId、taskId、kind=exec |
| sodeck_file_download | targetId、requestId、remotePath | runId、taskId、kind=download |
| sodeck_task_cancel | taskId | taskId、cancelRequested、result（当时任务事实，不含正文） |

targetId/taskId 最长 256 字节，requestId 最长 128，command 最长 16384，remotePath 最长 4096。必需字符串非空；命令和路径拒绝 NUL。timeoutSeconds 为整数。命令使用远端非交互 shell；下载一个普通文件到新建本地证据，不覆盖已有目标。

客户端明确选目标。固定 `(targetId, requestId)` 和规范化参数标识同次运行内的一个启动；同参数重试返回原任务，不同参数或种类为 requestConflict。去重先于容量检查。对应命令/文件接纳域正在处理或结果未知时，新身份返回 resourceBusy，没有新 taskId、worker 或目录。未知接纳只查原生票据，不重发；取消未知任务不能立即解除该域占用。

runId 是插件进程身份，taskId 包含运行身份并按不透明字符串使用。targetId 包含连接代次。重新连接要重新选择目标；旧任务在原插件进程中仍可查询。升级/重启后新进程不扫描恢复历史，也不接受旧短 task-N 别名。HTTP 断开和零匹配列表都不能证明远端未执行。

取消只在 collecting 阶段首次记录请求。finalizing/done 返回实际既有 cancelRequested，不修改事实；ACK 不表示远端或所有子进程已退出，也不撤销副作用。

## sodeck_tasks_list

| 参数 | 默认/范围 | 行为 |
| --- | --- | --- |
| targetId | 可选，1–256 字节 | 精确匹配原目标，可查已关闭目标；与 serverId 互斥 |
| serverId | 可选，1–256 字节 | 匹配保存的 context.serverId，包括不同连接代次 |
| requestId | 可选，1–128 字节 | 精确匹配，可组合其他筛选 |
| status | all / active / done，默认 all | active 为 done=false，包含未知和收尾；done 为稳定终态 |
| kind | all / exec / download，默认 all | 原任务种类 |
| limit | 整数，默认 20，1–50 | 每页条数 |
| cursor | 可选，1–2048 字节 | 只使用上次返回的 nextCursor |

返回 runId、tasks、counts.active、counts.done、hasMore、nextCursor。零匹配正常返回空 tasks、false 和 null。不自动选择跨目标同名 requestId 的首项。

列表项为任务摘要：

| 字段 | 内容 |
| --- | --- |
| taskId/runId/targetId/requestId/kind/context | 原身份、种类及完整 Context |
| target | 接纳时的名称、端点、协议和账号摘要；未取得为 null |
| inputPreview/inputTruncated | 前 256 UTF-8 字节，按完整字符截断，保留空白，并标记是否截断 |
| createdAt/finishedAt | Unix 毫秒时间；未结束的 finishedAt=null |
| phase/done/cancelRequested/revision | 插件阶段、稳定结束、取消请求和版本 |
| native | 实际存在的 state、admission、execSent、exitCode/exitSignal、remoteExitConfirmed、workerFinished、outputComplete、received/total、原生 ID 和 error |
| evidence | state、complete、error，省略本地路径 |
| resourceReleased/cleanupError | 原生清理事实 |

摘要 error 只保留 code/message，message 超过 512 UTF-8 字节时截断并标 messageTruncated=true；完整 details 用 task_get 查询。列表省略 input、output、directory、接入配置和 Token，不读证据文件。

排序按不可变接纳顺序从新到旧。第一页捕获接纳上限，游标绑定 runId、全部规范化筛选、limit、上限及末项序号。后续页保留全部参数；改变筛选/limit 需重新从第一页查询。游标不透明，不得自行构造或修改。跨运行为 runMismatch，结构、参数或边界错误为 invalidCursor。

新任务不插入已有分页序列。status=all 的成员不会因新增或完成而重复、漏项；active/done 使用各页实际状态，不冻结执行，状态变化后从第一页刷新对应筛选。counts 覆盖同一接纳上限与目标/requestId/kind 范围，不受 status、limit 或本页边界限制，是当时观察值。需要一次找全活动任务时可用 status=active、limit=50。

```json
{"serverId":"server-demo","status":"active","limit":50}
```

## sodeck_task_get

| 参数 | 默认/范围 | 行为 |
| --- | --- | --- |
| taskId | 必需，1–256 字节 | 唯一任务身份 |
| cursor | 十进制字符串，默认 "0"，最多 20 位且不超 u64 | 已保存 JSONL 的完整信封边界 |
| outputMode | full / text / none，默认 full | 输出投影 |
| waitMs | 整数，默认 0，0–20000 | 读取等待期限 |
| sinceRevision | 可选十进制字符串，最多 20 位且不超 u64 | 已观察的版本；不得超过当前任务版本 |

三个模式都返回同一任务事实：taskId/runId/targetId/requestId/kind、input（规范化原参数）、target、createdAt/finishedAt/observedAt、native、phase、done、cancelRequested、revision、evidence、directory、resourceReleased、cleanupError 和 outputInfo.savedBytes。command 的 savedBytes 是十进制字符串；download 为 null，下载进度以实际 native.received/total 为准。

| 模式 | output | 正文 IO |
| --- | --- | --- |
| full | chunks、stdout、stderr、nextCursor、savedBytes、hasMore、previewEncoding | 读一页；兼容旧默认 |
| text | 与 full 相同的文本及游标，省略 chunks | 同一 reader 直接投影，不构造 Base64 响应 |
| none | 省略 output；cursor 只能省略或为 "0" | 不打开、解析或校验正文 |

下载无命令正文，任何模式都省略 output。命令文件未建立且保存水位为 0 时，text 返回空页、nextCursor="0"、savedBytes="0"、hasMore=false；full 沿用可能省略 output 的旧形状。

一页最多约 16 KiB 原始 JSONL 完整信封。cursor 超过保存水位或不在完整行边界为 invalidCursor；继续使用 nextCursor。TaskSnapshot 一次捕获 revision、native、保存事实和 committed；outputInfo.savedBytes、output.savedBytes 与 hasMore 使用同一捕获水位，不混入读取过程中新增的输出。水位耗尽不表示任务结束。full/text 可在同一 nextCursor 上切换。

stdout/stderr 为每页有损 UTF-8，previewEncoding="utf-8-lossy-per-page"；它们不表达跨来源顺序，二进制、跨页字符和准确顺序需读取 chunks 的原字节 Base64 与 sequence 或本地证据。none 不校验证据，正文损坏不阻止状态查询，也不因此证明正文可读。

```json
{"taskId":"task-run-example-42","cursor":"680","outputMode":"text","waitMs":20000,"sinceRevision":"8"}
```

## 等待与 revision

waitMs=0 立即返回且不含 wait。正等待按以下优先级检查；唤醒后重新捕获同一 TaskSnapshot：

| 优先级 | 条件 | wait.reason |
| --- | --- | --- |
| 1 | done=true | done |
| 2 | full/text 的 cursor 小于已保存水位 | output |
| 3 | revision 大于 sinceRevision 或请求到达时基线 | changed |
| 4 | 到期且无以上条件 | timeout |

返回 `wait:{requestedMs,elapsedMs,reason}`，elapsedMs 使用单调时钟，包含响应准备的耗时；timeout 为正常回包，不修改 native/error。none 不以已有正文作为立即返回条件。省略 sinceRevision 时以请求初始版本为基线；显式携带上次 revision 可以发现两次请求之间的变化。输出 cursor 与 revision 各自保存，不可互换。

revision 从 1 开始，以十进制字符串返回。接纳、原生运行/退出/错误、实际下载进度、成功提交的新信封、保存/清理状态、第一次取消请求和目标摘要变化会递增。observedAt、elapsedMs、读取次数、HTTP 重试和无新数据轮询不递增。多个事实可合并一次递增，客户端只比较大小。版本属于同一 runId/taskId，不是持久事件日志。

等待为 async 监听已有事实，不创建远端 worker 或专用核心轮询。全插件最多 16 个等待、每任务 2 个，超额 resourceBusy，不排队或悄悄降为即时查询。即刻满足条件的读取不占等待额度。正文 IO 全插件最多 4 个，超额 resourceBusy；permit 随实际阻塞读取完成释放，HTTP 取消不虚假释放仍在执行的 IO。

MCP 请求取消、HTTP 中断或插件 shutdown 释放等待及配额；不调用 task_cancel。正常断开取消可见后的一秒内结束等待；不能抢占已经开始的系统磁盘 IO。目标/工作区关闭按原生命周期处理，插件退出可能直接中断 HTTP，不保证最后回包。读取中断后继续查原任务。

## 状态、证据与错误

phase=collecting 表示接纳/采集，finalizing 表示最终保存和原生清理，done 表示稳定发布。evidence.state 为 collecting/finalizing/saved/incomplete/failed，complete 仅在完整捕获或下载、保存无错误且最终 metadata 保存成功时为 true。非零退出码可有完整证据；done、取消 ACK 或本地保存不能推断全部远端子进程结束。清理失败独立保存于 cleanupError.details，按 step 标识 cancel/status/release/batch.end。

收尾尝试保存释放前事实，执行有界清理，再保存最终候选并一次发布。发布后 native、evidence、revision、取消标记与 finishedAt 保持稳定。最终 metadata 失败保留最后有效磁盘事实与输出，内存证据结果为 failed；无后台无限写盘、跨重启恢复或成功终态回滚。原生事实及本地错误不互相替代。

| 业务错误 code | 场景/后续动作 |
| --- | --- |
| invalidParams | 缺少字段、类型/范围/枚举/组合错误、sinceRevision 非法或为未来版本；修正参数 |
| taskNotFound | taskId 不在本次运行；查列表，不自动重放旧任务 |
| staleContext | 目标失效、关闭、代次改变或 UI 归属不匹配；重新列目标 |
| requestConflict | 固定请求身份参数不同；核对原参数 |
| invalidCursor | 正文/列表游标错误；使用返回的游标或从第一页重读 |
| runMismatch | 列表游标属于旧运行；重新查询，不能自动补执行 |
| resourceBusy | 接纳、活动、记录、等待或 IO 额度满；原任务/游标退避重读 |
| evidenceReadFailed | 正文读取、信封校验或解析失败，error.task 附捕获的事实；可用 none 查状态，核对本地证据 |
| requestCancelled / unavailable | 读取等待取消或插件退出；远端任务按原生命周期运行 |

原生核心 RPC 错误保留实际 code/message/details；native 失败、非零退出或未知结果在正常任务回包中表达。不存在通用 success。兼容保留原五工具、有效参数、默认立即 full 和原事实字段；新唯一编号、稳定 done、取消回执、分类错误及 busy 接纳是明确的行为修正，不保留旧错误执行路径。

返回[文档索引](README.md)。

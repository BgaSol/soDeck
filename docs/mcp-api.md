# MCP API

本文定义Agent MCP插件 0.13.x 的客户端契约。接入、页面操作和限制见[功能说明](features/mcp.md)；核心通用 RPC 见[插件 API](plugin-api.md)，发行证据见插件 [CHANGELOG](../plugins/mcp/CHANGELOG.md)。客户端先进行 MCP initialize 和 tools/list；新增能力以工具发现结果为准。

## 传输与调用

服务为带 Bearer Token 的回环 Streamable HTTP，端点 `http://127.0.0.1:<port>/mcp`。MCP 请求正文最多 32 KiB，总并发最多 32。Host 与可选 Origin 需匹配该回环地址。HTTP 401/403 为接入失败，429 为容量限制。字符串参数按 UTF-8 字节限制；未知参数或工具返回 MCP 协议 invalid_params。认证资料由核心管理。

工具 schema 由参数类型生成，服务器补丁保留与核心省略/null 规则一致的显式结构。schema 与校验器在进程内各编译一次，先校验类型、范围和嵌套未知字段，再检查 UTF-8 字节预算、跨字段组合和资源状态。schema 的字符长度不能代替字节预算。顶层未知参数/未知工具保持协议 invalid_params；已知字段的形状错误返回业务 invalidParams，游标和输入序号错误保留 invalidCursor/invalidSequence。默认值由处理器实际应用，schema 校验不修改输入。

十五个工具的成功与业务错误均只返回一个 JSON TextContent，缺席 structuredContent，不同时附原文副本或 outputSchema；客户端只解析一次 text。业务错误使用 isError=true 与 `{"error":{...}}`，取得任务时另以顶层 task 附一份该次事实。工具声明的只读/破坏性提示不能替代实际远端操作判断。

## 服务器节点写入（0.11.0）

要求核心 1.12.0 的 servers.write.v1，缺少时插件初始化提示升级。原十二工具保留，新增三个写工具；不扩展 servers_list 的现有读取投影。

| 工具 | 参数 | 成功返回 |
| --- | --- | --- |
| sodeck_server_create | catalogRevision、server | serverId、catalogRevision |
| sodeck_server_update | catalogRevision、serverId、patch | serverId、catalogRevision |
| sodeck_server_delete | catalogRevision、serverId | serverId、catalogRevision |

catalogRevision 是最近查询返回的十进制 u64 字符串，必须原样使用。serverId 为稳定 ID，范围沿用资料 ID；三个工具声明 readOnly=false、idempotent=false，update/delete 同时 destructive=true。成功只交付保存身份与新修订，不交付凭据、整份目录、profileToken 或远程任务。后续查询仍走 servers_list。

server 支持 name、host、port、username、tagIds、route、authentication。host/username 必需，name 默认主机，port 默认 22，认证默认 password、无秘密；tagIds/route 默认空。name/host/username 和 ID 为非空、无 NUL、最多 256 UTF-8 字节，privateKeyPath 最多 4096，秘密 value 可为空、最多 16384、无 NUL。正文仍受 32 KiB 限制；不接受 protocol、id、copyFromId、newTagNames 等其他字段。

route 项为 {kind:"ssh",serverId} 或 {kind:"socks5",proxyId}，顺序固定；不接受步骤 id。authentication 以 type=password/privateKey 指明完整配置，privateKeyPath 可省略；secret 可省略或为 {action:"keep",remember:boolean}、{action:"replace",value,remember:boolean}、{action:"clear"}。新增缺省 remember=true；修改省略 secret 沿用原保存选择和兼容秘密。改变认证类型或私钥路径时必须明确 replace/clear。私钥路径按 SoDeck 主机处理；不返回旧路径或秘密供客户端拼草稿。

patch 只能包含以上可编辑字段，至少一项。未提供保持；tagIds/route 用空数组清空，其他数组整体替换；authentication 未提供则完整保留。未知字段、null、错误类型、非法引用和无效端口拒绝。显示名称省略不改名；新建同地址独立节点允许存在，不自动合并。凭据存储与会话影响见[节点管理](features/mcp.md#管理服务器节点)。

核心业务错误放在 error.details.code，保留既有 RPC error.code：invalidParams、notFound、conflict、referenced、inUse、catalogUnavailable、persistenceFailed、staleInstance。通信超时的 code 仍可能是本地 RPC 错误，不能解释为确认未提交。所有错误不包含秘密值。

写入没有 requestId 和管理回执查询。旧 catalogRevision 重试不能再次提交，但 conflict 不证明原调用已成功；结果丢失须查询并核对，不自动用新修订重建操作。写入不接受 task_get、waitMs 或 task_cancel，不承诺 HTTP 断开撤销已派发的保存。

## 保存资料发现与打开（0.6.0）

要求核心 1.9.0；四个新增工具与原六工具共存。调用顺序为 servers_list → 明确选择 → 复用符合保存目标的 ready target 或 connection_open → connection_get（需要时）→ exec_start/file_download。任何发现或 get 都不建连或执行命令。

| 工具 | 参数 | 返回 |
| --- | --- | --- |
| sodeck_servers_list | query?、match?（exact 默认 / contains）、serverId?、detail?（false）、limit?（20，1–50）、cursor? | runId、catalogRevision、resolution、matchCount、servers、hasMore、nextCursor |
| sodeck_connection_open | serverId、expectedProfileToken、requestId；waitMs?（1000，0–20000） | runId、connection、target；正等待附 wait |
| sodeck_connection_get | connectionRequestId；detail?（false）、waitMs?（0，0–20000）、sinceRevision? | 同一原请求事实；正等待附 wait |
| sodeck_connection_cancel | connectionRequestId | 同一事实及 cancelResult=cancelled / alreadyFinished / pending |

query 与 serverId 互斥；match 仅在 query 存在时使用。匹配规则与下节 literal 规则相同，但身份范围为全部保存的 SSH 服务器。SOCKS5 跳板只出现在 detail.route，不作为可打开目标。matchCount 为整个查询范围的数量，resolution=none / unique / ambiguous，不依赖当前页数量或在线状态。游标绑定 run、目录 revision、查询和 limit；detail 可改变。资料编辑导致 catalogChanged；错误不会返回局部结果冒充 unique。

每项普通字段为 serverId/profileToken/name/protocol/host/port/endpoint/username/connection/target。connection 为当前尝试/工作区事实或 null，含 state/endpoint/context/matchesSavedTarget/error；matchesSavedTarget 无法确认时为 null。target 为已完成后台登记的目标摘要或 null，结构与 targets_list 普通候选一致。detail 增加 tagIds、非敏感 route 和完整 Context。观察故障单独提供 connectionError，创建失败提供 targetError，尚未创建提供 targetState=preparing。资料与会话字段分别表示保存目标和真实在用目标；不得用前者覆盖后者。

发现仅返回非敏感选择资料；密码、口令、私钥路径和整份目录不输出。profileToken 是核心既有 Profile.identity 的不透明选择令牌，不是凭据。必需 ID/token 为非空、无 NUL 的 1–256 UTF-8 字节字符串；requestId 最长 128。目录 query 最长 256、无控制字符、去除首尾空白，cursor 最长 2048。

connection 包含 connectionRequestId/requestId/serverId/state/stage/revision/interaction/error。state 为 pending / needsUser / ready / failed / cancelled；stage 在 SSH 接纳后准备目标时为 preparingTarget，取消确认中为 cancelling。interaction 为原提示的 kind/name/nodeIndex/endpoint/message/windowId，否则 null。错误表示已确认失败或最近通信故障，pending 加 error 不证明没有接纳。detail 增加原 input，接纳后的准备失败保留 error.context。ready 的 target 包含 targetId/serverId/workspaceId/target/targetSource/availability/capabilities；detail 增加完整 Context。

open 等待 ready、needsUser、failed、cancelled 或窗口到期，wait.reason 使用对应状态或 timeout；get 等待终态、revision 变化或到期，变化为 changed。revision 为 1 起的十进制 u64 字符串；sinceRevision 不得为未来值。waitMs=0 省略 wait，get 仍刷新本地核心事实。wait.elapsedMs 使用单调时钟；RPC 和响应准备可能超出主动等待窗口。needsUser 应交接用户，避免循环查询同一个问题。

打开的 requestId 在本次插件运行内唯一，与固定 serverId/expectedProfileToken 组成原意图；waitMs 可改变。同输入重试返回原 connectionRequestId，冲突返回 requestConflict。首次接纳未知时，仅显式重试同一 open 可重提原核心请求，get 保持只读。HTTP 中断不会撤销打开。完成收据保持原 target 和 revision；当前可用性需新发现或实际操作接纳确认，重连后旧收据不会指向新 target。

cancel 与 ready 发布决定先后：取消先胜后不再发布 ready，核心取消确认中返回 pending；ready/failed/cancelled 已完成时为 alreadyFinished。只在取消确认后返回 cancelled。核心已接纳会话时，本次尚未 ready 的 MCP 请求仍可取消，但共享会话保持。插件退出取消未接纳且无其他使用者的尝试，已接纳会话继续按工作区生命周期管理；重启后旧 connectionRequestId 为 connectionRequestNotFound。

每运行最多 32 个未完成打开、4096 条打开收据，满时 resourceBusy，不逐出旧收据。每个未完成打开仅一个控制任务；服务器/目标发现与控制共用 4 个实际核心 RPC 位置，全插件正等待共用 16 个、每个打开最多 2 个。新正等待在登记前预留；HTTP 取消不能提前释放已发送同步 RPC 的位置。无额外连接队列或自动重放远端命令。

| 打开/资料错误 | 含义 |
| --- | --- |
| profileChanged | 选择后的端点、账号、路线或认证配置已变化；重查资料，用新 requestId |
| existingSessionMismatch | 在用会话地址、账号或路线与保存资料不同；用户处理原会话 |
| serverNotFound / catalogUnavailable / catalogChanged | 资料删除、目录故障或分页时变动；恢复或重新发现 |
| connectionRequestNotFound | 原打开身份不属于本次运行；不可借新身份重放未知命令 |
| targetInitializationFailed | SSH 接纳后后台目标准备失败；共享会话保留 |

新客户端将默认名称/IP 发现由 targets_list 改为 servers_list。已配置的 Token、端口、插件 ID和原任务 API 沿用；本版元数据为 schemaVersion=3；旧核心缺少能力时初始化明确提示升级，不回退为插件自己的 SSH 连接。

## 目标与启动

| 工具 | 参数 | 返回 |
| --- | --- | --- |
| sodeck_targets_list | 可选 query、match、serverId、workspaceId、targetId、detail | runId、resolution、registryChanged、targets、unresolved；完整契约见下节 |
| sodeck_exec_start | targetId、requestId、command；mode?（batch 默认 / pipe / pty）、timeoutSeconds?（1–3600）、cols?/rows?（PTY 80×24）、waitMs?（1000，0–20000） | 普通任务事实与第一 text 页；正等待附 wait |
| sodeck_file_download | targetId、requestId、remotePath | runId、taskId、kind=download |
| sodeck_task_cancel | taskId | taskId、cancelRequested、result（当时任务事实，不含正文） |

targetId/taskId 最长 256 字节，requestId 最长 128，command 最长 16384，remotePath 最长 4096。必需字符串非空；命令和路径拒绝 NUL。timeoutSeconds/waitMs 为整数。mode=batch 缺省期限 300 秒、stdin 关闭；pipe stdin 开放、pty 分配终端，两者缺省没有总期限。显式 null 期限拒绝。cols（2–1000）、rows（1–500）仅 pty 接受。规范化模式、期限和尺寸属于启动去重参数，waitMs 不属于；下载一个普通文件到新建本地证据，不覆盖已有目标。

exec_start 在本次窗口内等待稳定 phase=done，出现输出、revision、取消请求或 finalizing 只重新观察，不提前结束。到期返回当前任务与已保存正文，wait.reason=timeout，远端继续；完成为 reason=done。默认普通事实与 text 页的字段同 task_get，固定 cursor="0"；不接受 detail/outputMode/分页参数。waitMs=0 不主动等待且省略 wait，同样返回任务与第一文本页。下载不接受 waitMs，继续立即返回凭据。

waitMs 只控制本次主动等待，timeoutSeconds 仍控制实际执行。窗口从取得原任务身份或新任务登记时以单调时钟计算，不因通知延长；elapsedMs 包含后续结果准备，读盘/编码/传输不受 waitMs 限制。done 且 hasMore=false 时可直接使用输出回答，仍须核对退出、证据与清理事实；未 done 或有更多页时用 task_get 与 nextCursor 接续。

客户端明确选目标。固定 `(targetId, requestId)` 和规范化执行参数标识同次运行内的一个启动；同参数重试返回原任务，不同执行参数或种类为 requestConflict。waitMs 不进入执行参数、去重比较或元数据，重试可以调整它，原执行期限不重置。去重先于当前目标存在性和新接纳容量检查；旧目标移除或换代仍可恢复原记录。对应命令/文件接纳域正在处理或结果未知时，新身份返回 resourceBusy，没有新 taskId、worker 或目录。未知接纳只查原生票据，不重发；取消未知任务不能立即解除该域占用。

新正等待请求在任务登记和后台派发前取得全局与每任务等待位置，额度满时拒绝且无新任务、原生操作或证据目录，不自动降为零等待。已完成原任务或显式零等待不申请等待额度；运行中的原任务等待额度不足时，错误附原任务，操作继续。任务登记是本地接纳，native.admission/execSent 才说明远端是否执行。登记后的 HTTP 取消、读盘或编码失败不能撤销任务；可恢复错误保留顶层 task 或容量错误的可信 taskId。响应丢失时以原请求查证，不生成新 requestId 重发。

runId 是插件进程身份，taskId 包含运行身份并按不透明字符串使用。targetId 包含连接代次。重新连接要重新选择目标；旧任务在原插件进程中仍可查询。升级/重启后新进程不扫描恢复历史，也不接受旧短 task-N 别名。HTTP 断开和零匹配列表都不能证明远端未执行。

取消只在 collecting 阶段首次记录请求。finalizing/done 返回实际既有 cancelRequested，不修改事实；ACK 不表示远端或所有子进程已退出，也不撤销副作用。

## 单文件上传（0.9.0）

要求核心 1.11.0 的 `files.copy.v1`。新增第十五个工具 `sodeck_file_upload`；行为、元数据与并发限制见[上传说明](features/mcp.md#上传文件)。沿用目标选择、固定请求去重、当前插件运行内查询与额度。

| 参数 | 范围 |
| --- | --- |
| targetId / requestId | 沿用目标与启动规则；固定为同一次上传，不以重连目标重放 |
| localPath | SoDeck 主机普通源文件绝对路径，1–4096 UTF-8 字节，无 NUL；本机核心处理盘符、UNC 和 POSIX 差异 |
| remotePath | 当前文件服务完整绝对路径，含文件名，1–4096 UTF-8 字节，无 NUL；例如 /tmp/job.sh 或 /C:/Temp/job.ps1 |
| overwrite | 布尔值，默认 false；true 允许替换开始时已有普通文件，仍拒绝后到目标和类型冲突 |

拒绝未知字段；没有 waitMs、内联正文、自动 mkdir 或执行参数。立即返回 `{runId,taskId,kind:"upload"}`，然后用 task_get 等待/读取；task_input 不适用于上传。上传参数和缺省 overwrite=false 参与去重，同身份同参数恢复原 taskId，不重新打开来源；更改参数为 requestConflict。

任务普通结果增加 `file:{localPath,remotePath,overwrite,commitState}`。localPath 在上传中恒指来源，不是下载交付路径；已有下载语义不变。commitState 为 pending/committed/notCommitted/unknown。native.received/total 为十进制字节数字符串（准备期总量可缺省），记录远端已确认写入临时文件的字节；native.transfer.commitAttempted 表示已进入提交尝试。普通结果保留 native.result 的最终大小与附属警告；detail 才含来源版本及未决临时路径。无 output/outputInfo 正文，outputMode 不改变文件读取方式。

提交得到 ACK 后为 committed，最终 stat、本地元数据和清理故障另报，不能改成可重传失败；明确未提交为 notCommitted，未收到提交确认或接纳结果不明为 unknown。done 只表示稳定收尾。开始前额度/参数拒绝可无 taskId，登记后所有失败均保留原身份。重启不恢复任务或跨运行去重；旧任务不存在不能证明远端未提交。

本版落地元数据 schemaVersion=3，包含 upload 种类和 transfer 事实；旧证据不迁移，命令原字节信封不变。工具列表与 kind=all 增量包含上传，严格枚举客户端需识别 upload。

## 交互输入（0.7.0）

要求核心 1.10.0 的 session.process.v1，缺少时插件初始化明确失败。保留原十工具，新增 sodeck_task_input；batch 原调用方式继续使用。不会在运行中改变模式、建立新 SSH 连接或自动重跑原命令。

| task_input 参数 | 范围 |
| --- | --- |
| taskId | 本次运行的原命令任务；下载不支持 |
| sequence | 1 起始的十进制 u64 字符串，以 interaction.nextInputSequence 为准 |
| action=write | text 与 dataBase64 二选一；UTF-8 或解码后原始字节最多 16 KiB，Base64 编码最多 24 KiB；不补换行 |
| action=eof | 仅 pipe；不带内容或尺寸，关闭 stdin，输出采集继续 |
| action=resize | 仅 pty；cols 2–1000，rows 1–500，均为整数 |

未知或不适用字段拒绝。text 可含 NUL、CR、LF；Ctrl+C、Ctrl+D、方向键用实际字节发送。PTY 的 Ctrl+D 由远端终端解释，raw 模式不等于关闭 stdin。

成功返回原任务普通事实与 interaction，不读取正文；错误使用 isError=true，并在取得任务时附原事实。interaction 和 detail.input（原启动参数）分别表达控制状态和启动诊断。interaction 包含 mode、state（open/closing/closed 或尚未知的 null）、available、nextInputSequence、last 和 attempt。last 为核心最近接纳的 sequence/action/state/sentBytes/error；state 为 pending/sent/failed/unknown。sent 只证明本地 SSH 通道发送调用成功，程序消费情况由输出证明。attempt 为 MCP 尚未结束或接纳不明的序号、sending/unknown 状态与通信错误，不含输入原文。

同任务只允许一个未确认输入，最近已接纳序号和同一规范化字节重复返回既有事实；同序号不同内容为 inputConflict。较旧序号 inputExpired，跳号 invalidSequence，未结束输入阻止新输入并返回 resourceBusy。接纳前拒绝不推进序号；已接纳后即使发送失败或未知也保留原回执，不重新发送。原输入 RPC 回包丢失时仅查询或显式重试完全相同输入，后续 read/status 可以恢复它的接纳与完成事实。核心只保留一项最近输入，MCP 保留最近参数及最多一项未确认尝试，不建立输入历史。

启动未收到远端 Success 为 inputNotReady；batch 或不适用动作 unsupported；停止、取消、EOF 或收尾为 inputClosed。陈旧原会话 staleContext，已释放资源 processNotFound，不把输入转给重连后的新会话。HTTP 中断不撤销已接纳输入；在途 RPC 完成前仍占原短 IO 额度。收尾先封闭输入并等待实际 RPC 结束，再保存最近回执和清理资源。

例如启动 `{"targetId":"target","requestId":"sudo-install","command":"sudo command","mode":"pty"}`，读取原 taskId 输出及 interaction 后发送：

```json
{"taskId":"task","sequence":"1","action":"write","text":"password\r"}
```

继续读同一任务；错误密码产生的新提示使用下一序号。普通 input 参数、日志和证据不把密码作为另一种特殊执行协议。

## sodeck_targets_list

范围是本次插件运行中已登记、尚未移除的后台目标，包括断开和关闭中的工作区；不查询服务器目录、其他实例或历史运行。无参数返回全范围，不过滤离线项。

| 参数 | 范围与组合 |
| --- | --- |
| query | 去除首尾空白后 1–256 UTF-8 字节，拒绝控制字符；与 targetId 互斥 |
| match | exact（默认）或 contains；仅有 query 时有效 |
| serverId、workspaceId | 各 1–256 字节、无 NUL，精确比较；可相互组合或与 query 组合 |
| targetId | 1–256 字节、无 NUL；与 query、match、serverId、workspaceId 互斥 |
| detail | 布尔，默认 false；只改变详略 |

显式 null、错误类型、空字符串、非法组合或未知参数拒绝。名称按首尾去空白及 Unicode 小写比较；域名按 ASCII 小写并去除一个结尾点比较。标准库解析 IPv4/IPv6，IPv6 按数值比较、作用域标识精确比较，不做 DNS。带端口的主机:port 或 [IPv6%zone]:port 按结构化主机和端口比较，不带端口的完整主机匹配所有端口。

exact 取全部适用依据的并集：nameExact、hostExact、endpointExact、ipv4LastOctet。IP 尾段仅接受规范十进制 0–255，0187 不作尾段；名称“0187”仍按字面比较。一项只出现一次，matchedBy 保留全部依据。contains 在上述规则上增加 nameContains 和 hostnameContains，不按 IPv4/IPv6 子串匹配，也不自动从 exact 扩大。

| 结果字段 | 含义 |
| --- | --- |
| runId | 当前插件进程身份 |
| targets | 已确认符合筛选的候选，按登记顺序；不可用身份候选仍保留 |
| unresolved | 使用 query 时，摘要未知/失败而无法判断匹配的登记项；不得当成不匹配 |
| registryChanged | 查询前后，同一精确身份范围的 targetId/完整 Context 集合发生变化 |
| resolution | incomplete 优先于其他判断；否则两项以上 ambiguous、零项 none、一项 unique |

registryChanged=true 或 unresolved 非空时 resolution=incomplete。无 query 时成员已由登记身份确定，描述失败项放 targets，unresolved 为空；unique 只说明身份唯一，不保证可执行。没有跨目标瞬时快照，连接事实可能在返回后改变，启动继续验证原 Context。

每项含 targetId/serverId/workspaceId、target、targetSource、availability、capabilities。target 为 name/protocol/host/port/endpoint/username，未知为 null。targetSource 是 current/lastConnected/unknown；只表示会话地址来源，名称使用工作区当前显示名称。availability 为 ready/disconnected/closing/stale/unavailable，capabilities 未读取到时为 null，不伪造空数组。query 的已确认候选另含 matchedBy。普通结果省略 Context、state、closing；detail=true 增加完整登记 Context、实际存在的 state/closing 与完整 error，不附第二份普通副本。

单项 RPC 失败保留实际 code 类型和 message；普通错误复用 512 字节字符边界截断与 detailsAvailable 规则。不同 Context 是 staleContext；畸形事实为 invalidTarget。SDK 本地通信错误 -32003 或通信关闭后停止后续 RPC，剩余项为 notObserved，并保存首个实际原因；不能把剩余项描述为各自发生了核心业务失败。

唯一 ready 且包含所需能力时才能启动；ambiguous 需要用户选择，incomplete 需重新查或精确缩小范围，none 只说明登记范围没有匹配。离线候选参与歧义，不能按在线、焦点或排序消歧。旧任务恢复先查询原身份，不用新 targetId 重放原操作。

采集最多 4 个实际作业，使用一个阻塞 worker 顺序读取该次范围；额度不足立即 resourceBusy，不排队。取消/退出停止后续 RPC，已发送的同步 RPC 仍可能等到 SDK 的 4 秒期限；位置随实际作业结束释放。查询无任务、证据目录或远端操作副作用。实际 MCP result 超过 256 KiB 时明确 resourceLimit，请用身份范围或 detail=false 缩小回包，不截掉候选制造假唯一。

## sodeck_tasks_list

| 参数 | 默认/范围 | 行为 |
| --- | --- | --- |
| targetId | 可选，1–256 字节 | 精确匹配原目标，可查已关闭目标；与 serverId 互斥 |
| serverId | 可选，1–256 字节 | 匹配保存的 context.serverId，包括不同连接代次 |
| requestId | 可选，1–128 字节 | 精确匹配，可组合其他筛选 |
| status | all / active / done，默认 all | active 为 done=false，包含未知和收尾；done 为稳定终态 |
| kind | all / exec / download / upload，默认 all | 原任务种类 |
| limit | 整数，默认 20，1–50 | 每页条数 |
| cursor | 可选，1–2048 字节 | 只使用上次返回的 nextCursor |

返回 runId、tasks、counts.active、counts.done、hasMore、nextCursor。零匹配正常返回空 tasks、false 和 null。不自动选择跨目标同名 requestId 的首项。

列表项为任务摘要：

| 字段 | 内容 |
| --- | --- |
| taskId/runId/targetId/requestId/kind/serverId | 原身份、种类及保存的 Context.serverId，省略完整 Context |
| target | 接纳时的名称、端点、协议和账号摘要；未取得为 null |
| inputPreview/inputTruncated | 前 256 UTF-8 字节，按完整字符截断，保留空白，并标记是否截断 |
| createdAt/finishedAt | Unix 毫秒时间；未结束的 finishedAt=null |
| phase/done/cancelRequested/revision | 插件阶段、稳定结束、取消请求和版本 |
| native | 实际存在的 state、admission、execSent、exitCode/exitSignal、remoteExitConfirmed、outputComplete、received/total 和 error |
| evidence | state、complete；有故障时附 error，省略本地路径 |
| resourceReleased/cleanupError | 原生清理事实 |

普通 native/evidence/cleanup 错误只保留实际 code/message，message 超过 512 UTF-8 字节按完整字符截断并标 messageTruncated=true；省略附加详情时标 detailsAvailable=true。原 code 类型保留，完整 details 用 task_get 的 detail=true 查询。列表省略 input、完整 Context、output、outputInfo、directory 和下载路径，不读证据文件。

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
| outputMode | text / full / none，默认 text | 与 detail 独立的输出格式 |
| detail | 布尔，默认 false | 仅控制任务事实详略，不改变正文、等待或分页 |
| waitMs | 整数，默认 0，0–20000 | 读取等待期限 |
| sinceRevision | 可选十进制字符串，最多 20 位且不超 u64 | 已观察的版本；不得超过当前任务版本 |

普通结果保留 taskId/runId/targetId/requestId/kind、接纳时实际 target、phase/done/cancelRequested/revision、native、evidence 和 resourceReleased；有清理故障时附 cleanupError。省略完整 input/Context、时间、本地目录及原生内部 ID/worker/耗时。target 未确认时为 null，不从当前连接猜历史归属。

交互任务的普通、诊断和侧栏详情均附 interaction。native 只投影实际存在的 state、phase、admission、execSent、launchQueued、launchAccepted、exitCode/exitSignal、remoteExitConfirmed、outputComplete、received/total 与简明 error；缺失不补 false 或 0。unavailable 可附一层 lastResult 的最后观察事实，更深内容标 lastResultDetailsAvailable=true，不把历史退出结果当当前确认。

detail=true 返回同一捕获中的原 input、完整 Context、时间、directory、native 原对象、evidence.path 与完整错误；不另附一份普通任务副本。detail=true、none 不检查正文文件，不能恢复上一次未持久化的临时读取错误；诊断正文故障需重读对应游标。

| 模式 | output | 正文 IO |
| --- | --- | --- |
| text，默认 | pipe/batch 为 stdout、stderr，PTY 为 terminal；另含 nextCursor、savedBytes、hasMore、previewEncoding；不含 chunks | 读一页并按来源解码 |
| full | chunks、nextCursor、savedBytes、hasMore；不含 stdout/stderr/terminal/previewEncoding | 同一 reader 校验原信封，只保存一份原字节正文 |
| none | 省略 output，命令保留 outputInfo.savedBytes；cursor 只能省略或为 "0" | 不打开、解析或校验正文 |

有 output 时不重复 outputInfo.savedBytes。下载任意模式省略 output 和 outputInfo，保留 file.remotePath；file.localPath 仅 done=true、evidence.complete=true 且已提交文件时可用，其他为 null。诊断 evidence.path 可以保留残留证据路径，不能作为完整下载交付。

命令水位为 0 且文件尚未建立时，text/full 均返回格式对应空页，nextCursor=savedBytes="0"、hasMore=false。其他 IO 或损坏错误不能冒充空页。

一页最多约 16 KiB 原始 JSONL 完整信封。cursor 超过保存水位或不在完整行边界为 invalidCursor；继续使用 nextCursor。一次捕获 Record 的 revision、native、保存事实和 committed，output 的水位与 hasMore 使用该次记录，不混入读取中新输出。full/text、detail 可在同一 nextCursor 上切换。水位耗尽不表示任务结束。

stdout/stderr/terminal 为每页有损 UTF-8，previewEncoding="utf-8-lossy-per-page"；它们不表达跨来源顺序，二进制、跨页字符和准确顺序需读取 chunks 的原字节 Base64 与 sequence 或本地证据。none 不校验证据，正文损坏不阻止状态查询，也不因此证明正文可读。

```json
{"taskId":"task-run-example-42","cursor":"680","outputMode":"text","waitMs":20000,"sinceRevision":"8"}
```

## 等待与 revision

两种工具的 waitMs=0 均不主动等待且不含 wait。以下变化等待优先级用于 task_get；exec_start 只以稳定 done 或固定期限到达结束。唤醒后重新捕获原始 Record：

| 优先级 | 条件 | wait.reason |
| --- | --- | --- |
| 1 | done=true | done |
| 2 | full/text 的 cursor 小于已保存水位 | output |
| 3 | revision 大于 sinceRevision 或请求到达时基线 | changed |
| 4 | 到期且无以上条件 | timeout |

返回 `wait:{requestedMs,elapsedMs,reason}`，elapsedMs 使用单调时钟，包含响应准备的耗时；timeout 为正常回包，不修改 native/error。none 不以已有正文作为立即返回条件。省略 sinceRevision 时以请求初始版本为基线；显式携带上次 revision 可以发现两次请求之间的变化。输出 cursor 与 revision 各自保存，不可互换。

revision 从 1 开始，以十进制字符串返回。接纳、原生运行/退出/错误、实际下载进度、成功提交的新信封、保存/清理状态、第一次取消请求和目标摘要变化会递增。observedAt、elapsedMs、读取次数、HTTP 重试和无新数据轮询不递增。多个事实可合并一次递增，客户端只比较大小。输入尝试及接纳/完成回执变化也递增；迟到旧输入回包和原生结束事实不会使控制水位倒退。版本属于同一 runId/taskId，不是持久事件日志。

等待为 async 监听已有事实，不创建远端 worker 或专用核心轮询。全插件最多 16 个等待、每任务 2 个，超额 resourceBusy，不排队或悄悄降为即时查询。新正等待启动预先取得位置；已有任务即刻满足条件的读取不占等待额度。等待结束先归还等待位置，再读取正文。正文 IO 全插件最多 4 个，超额 resourceBusy；permit 随实际阻塞读取完成释放，HTTP 取消不虚假释放仍在执行的 IO。

MCP 请求取消、HTTP 中断或插件 shutdown 释放等待及配额；不调用 task_cancel。正常断开取消可见后的一秒内结束等待；不能抢占已经开始的系统磁盘 IO。目标/工作区关闭按原生命周期处理，插件退出可能直接中断 HTTP，不保证最后回包。读取中断后继续查原任务。

## 状态、证据与错误

output.jsonl 的 batch/pipe 信封为 v1、source=stdout/stderr；PTY 为 v2、source=terminal。读取按任务固定模式校验来源与版本，不把未知来源归为 stdout。终端文本保留 ANSI 等原字节的有损预览，不提供屏幕仿真。metadata.json schemaVersion=3 保留规范化启动 input、interaction 和原生事实。

phase=collecting 表示接纳/采集，finalizing 表示最终保存和原生清理，done 表示稳定发布。evidence.state 为 collecting/finalizing/saved/incomplete/failed，complete 仅在完整捕获或下载、保存无错误且最终 metadata 保存成功时为 true。非零退出码可有完整证据；done、取消 ACK 或本地保存不能推断全部远端子进程结束。清理失败完整信息在 detail=true 的 cleanupError.details，按 step 标识 cancel/status/release/batch.end。

收尾尝试保存释放前事实，执行有界清理，再保存最终候选并一次发布。发布后 native、evidence、revision、取消标记与 finishedAt 保持稳定。最终 metadata 失败保留最后有效磁盘事实与输出，内存证据结果为 failed；无后台无限写盘、跨重启恢复或成功终态回滚。原生事实及本地错误不互相替代。

| 业务错误 code | 场景/后续动作 |
| --- | --- |
| invalidParams | 缺少字段、类型/范围/枚举/组合错误、sinceRevision 非法或为未来版本；修正参数 |
| taskNotFound | taskId 不在本次运行；查列表，不自动重放旧任务 |
| staleContext | 目标失效、关闭、代次改变或 UI 归属不匹配；新操作重新列目标；旧操作仍查原身份 |
| requestConflict | 固定请求身份参数不同；核对原参数 |
| invalidCursor | 正文/列表游标错误；使用返回的游标或从第一页重读 |
| runMismatch | 列表游标属于旧运行；重新查询，不能自动补执行 |
| resourceBusy | 接纳、活动、记录、等待或 IO 额度满；原任务/游标退避重读 |
| evidenceReadFailed | 正文读取、信封校验或解析失败，顶层 task 附一次捕获事实；可用 none 查状态，核对本地证据 |
| resourceLimit | 实际编码 MCP result 超过 256 KiB；改读普通 text/none 或本地证据，任务事实与游标不变 |
| requestCancelled / unavailable | 查询/读取等待取消或插件退出；远端任务按原生命周期运行 |

远端失败、非零退出、证据或清理失败、未知结果与等待 timeout 在正常查询回包中如实表达，isError=false；参数、配额、游标、正文读取失败为工具业务错误，isError=true。未知工具、畸形请求仍走 MCP/JSON-RPC 错误。不存在通用 success。

每份 MCP result 编码后最多 256 KiB，包含 text 的外层转义与实际 SDK 字段；超限明确返回 resourceLimit，不删字段、截正文或改变任务状态。错误只带操作名及可信任务/已验证游标提示，不回显原超大参数。普通与诊断使用同一预算。

0.5.0 将 targets_list 默认形状改为简明候选和 resolution；原 context/state/closing 改用 detail=true，客户端需重新发现工具并处理 ambiguous/incomplete。要求核心 1.6.0 的 session.target.v2，旧核心明确初始化失败，不用目录或缓存补出不可靠摘要。

0.3.0 升级读取契约：默认由 full 改为 text，任务事实默认精简，full 不再附文本，structuredContent 删除。旧客户端需改为解析 content[0].text；原参数、路径、完整错误改用 detail=true。0.4.0 将 exec_start 从立即凭据改为默认有限等待与普通任务/第一文本页；依赖旧形状或极短回包期限的客户端需迁移，waitMs=0 保留立即交接意图但不恢复旧形状。客户端重新发现 tools/list。六工具名、启动/目标身份、task_get 的变化等待、分页、去重保持；0.7.0 元数据使用 schemaVersion=2，保存模式及最近输入摘要，原文件不扫描或迁移。无旧版开关或重复工具。

返回[文档索引](README.md)。

API 2.1 文件任务的 `native.transfer` 增加 `phase`（preparing/transferring/finalizing/committing/finished）和 `stage`；终态保留最终 `received/total`，`native.error` 保留 `stage/osCode`。下载 `file.commitState` 与上传使用相同核心事实，只有稳定交付才提供 `localPath`。正文进度完成不代表校验或提交成功。

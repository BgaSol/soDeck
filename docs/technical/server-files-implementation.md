# T-008：文件插件代码设计与修改规划

更新日期：2026-10-26 · 修订：r2 · 文档就绪：可评审
实施状态与最终确认：[T-008 r4](../tasks/T-008-server-file-tree.md)
功能依据：[F-FILES-001 r4](../plugins/files/features/server-file-tree.md)、[F-CORE-009 r3](../core/remote-read.md)、[文件插件 r4](../plugins/files/README.md)
现有基线：[T-009 代码方案 r2](clean-start-implementation.md)、[T-009 实测](../tasks/T-009-results.md)，核心 dev.4 / 终端 0.4.0 / API 4。

本文件记录先方案、再最终确认的设计基线。用户已在 T-008 r4 后答复“可以开始做了”，随后实施；当前版本、脚本和实际覆盖见 [T-008 结果](../tasks/T-008-results.md)。以下基线核对和拟新增列表保留方案形成时的语义，不能将历史“待确认/尚未建立”理解为当前授权或实现状态。

## 1. 交付目标与工程边界

交付 Windows 开发目录包：用户打开服务器，中央按既有规则自动创建一个终端，左侧显示文件树；连接就绪后按需读取目录和文件大小，选中挂载目录可查看来源、类型、容量及可得高级信息。首终端失败不等于文件功能不可用，文件失败也不阻止终端。完整重启保持空工作台。

首轮远端支持 Linux + OpenSSH SFTP；挂载完整查询以 procfs、POSIX sh、GNU coreutils 的 `stat/readlink/cat` 可用为验证基线，不要求部署远端 Agent、Python 或常驻服务。缺少挂载采集条件时按功能规格降级，目录浏览可独立使用。首轮不实现文件写入、传输产品、挂载/卸载、目录递归大小或 Docker 卷映射。

```mermaid
flowchart LR
  Shell[核心工作台与原生窗口] --> Host[通用多插件注册与路由]
  Host --> Terminal[终端插件 0.4.0]
  Host --> Files[文件插件 UI 与业务]
  Terminal --> Gateway[版本化公共网关与调用记录]
  Files --> Gateway
  Gateway --> Broker[核心连接与资源管理]
  Broker --> SSH[SSH 调度：PTY / SFTP / exec]
  SSH --> Server[隔离验证或用户选定的远端服务器]
```

文件插件持有树、筛选、选择、挂载解析、容量解释与缓存；核心持有协议、租约、句柄、取消、真实来源、窗口及工作区代次。禁止新增 `getDiskTree`、`getMountDetails` 等文件业务专用核心命令。插件之间不调用、不共享内部状态或句柄。

实施直接在 `v2` 当前工作区进行，保留尚未提交的 T-009 改动；不自动建分支、工作树、提交、推送或正式发布。交付使用新目录 `.local/dev/SoDeck-v2-T008/`，不覆盖 T-009 EXE/data。启动仍沿用现有开发包入口，不纳入 T-005 的直接启动设置。

## 2. 已核实事实与文件修改清单

当前 HEAD `d550c76`，实际起点是其上的 T-009 未提交实现，不能按 HEAD 旧代码推导设计。实施前再次记录工作区差异和基线文件哈希。

| 模块/文件 | 已核实的现状 | 计划修改与责任 |
| --- | --- | --- |
| `crates/core/src/lib.rs`、`model.rs` | Registry 只有连接/租约/PTY；租约按实例归属；普通 acquire 复用失败会建连 | 抽出连接协调与 Remote I/O 分派；新增工作区 scope 注册/撤销、目录/文件/执行资源、终态回执和统一清理。扩展真实调用上下文，不把挂载模型放入核心 |
| `crates/core/src/ssh.rs` | 一个连接一个非阻塞 Session 调度线程，现有循环处理 PTY | 保留传输/认证，拆出 `ssh/pty.rs`、`ssh/sftp.rs`、`ssh/exec.rs`、`ssh/scheduler.rs`；按状态机推进并保持 PTY 公平调度，不用网络等待占住全局锁 |
| 拟新增 `crates/core/src/remote_read.rs`、`remote_operations.rs`、`connection_access.rs` | 尚不存在 | 对应文件/执行 API、操作回执/取消、连接状态/仅复用接口；资源数据结构按可复用责任组织 |
| `crates/core/src/store.rs` | 统一记录字段摘要，不记录正文 | 增加安全的资源/操作/数量摘要；关闭/取消仍允许在记录存储失败时尽力执行，不能泄露路径、命令或挂载参数 |
| `third-party/ssh2/`、根 `Cargo.toml`/`Cargo.lock` | ssh2 0.9.6 Windows 路径入口会替换反斜杠，非 UTF-8 readdir 转换可能 panic | 固定 0.9.6 的小范围 vendor 补丁，增加字节路径 API；保留上游路径 API 和现有 PTY 行为、许可证与来源哈希，不改 SSH 协议/加密实现 |
| `apps/desktop/src-tauri/src/app.rs` | 单个 `loaded: Option<Loaded>`；启动、assets、invoke、monitor、shutdown 均选择它 | 抽出 `plugin_registry.rs`/`plugin_services.rs`，按插件 ID 管理实例与错误；调用/表面资源根据所有者选择插件；App 错误与单插件错误分开 |
| `workbench_services.rs`、`workbench.rs`、`view_operations.rs` | 默认视图/初始化动作已有，但只来自单插件；有创建 generation | 聚合各插件贡献，动作按插件限定；默认侧栏失败局部化；关闭/取消/重试携带真实归属；新增内存侧栏宽度和焦点状态，不恢复旧布局 |
| `workspace_move.rs`、`window_services.rs` | 整工作区批量迁移在绑定资源时仍取单个 loaded 实例 | 按每视图 pluginId 分组准备/绑定/回滚；原事务全部成功后提交归属，单组失败回滚所有已准备视图 |
| `native.rs`、`surface.tsx`、`app.tsx`、`types.ts` | 表面显示时可能 focus；sideWidth 是窗口 React 状态 | 显隐与焦点拆开，按核心焦点意图授予；侧栏宽度进工作区内存模型；文件侧栏 minWidth 默认值按声明，不用终端 sessions 特判 |
| `crates/plugin-manager/src/package.rs`、`lib.rs`、`store.rs` | 管理器存储已按插件 ID 分组，宿主仍单实例 | 复用签名/暂存/数据代次/回退；能力目录升级，按插件分别激活/报告失败；集合校验贡献归属，不因为一个插件失败跳过其他插件 |
| `contracts/`、`packages/sdk/`、生成脚本 | 工作台 v4 schema 和严格请求/响应检查已存在；宿主帧上限 1 MiB | 新 Remote I/O schema/类型/验证器；工作台增加兼容字段及能力协商，不放宽成任意 JSON。多插件旧包响应投影与测试见 §3 |
| 拟新增 `plugins/files/` | 尚无文件插件工程 | 独立 manifest、React UI、QuickJS worker、领域解析、数据状态机与测试；只依赖公开 SDK/契约及通用第三方库 |
| `scripts/pack-plugin.mjs`、`package-dev.mjs`、启动资源解析 | 参数有 plugin 名，但版本常量/构建变量/默认包仍偏终端 | 读取每插件 manifest/构建参数；新增多包资源清单，保留已有单终端覆盖参数；交付两个独立签名包和各自哈希 |
| `scripts/check-boundaries.mjs`、测试与文档 | 基础边界检查较窄，已有 T-009 回归 | 增加插件互导入/核心依赖文件业务等检查，补有效负例；增加 FT/RI/双插件与真实性测试。新增脚本见 §10 |

拆模块限于以上责任，不顺带重构终端渲染器、数据库、密码保存或整个工作台框架。每步保持项目可构建；新增 I/O 方法未完成前不发布对应 capability。

## 3. 版本与兼容设计

| 对象 | 本次计划 | 规则 |
| --- | --- | --- |
| 核心开发包 | `2.0.0-dev.5` | 带本次公共能力及两个官方插件；版本号是待构建目标 |
| 插件 API / SDK | `4.1.0`，保持 major 4 | 新增兼容能力，不把单插件改造变成 API 5 升级；当前终端 `^4.0.0` 仍可加载 |
| 文件插件 | `sodeck.files`，`0.1.0`，schema 1 | 要求 `^4.1.0`、`remote.files.read.v1`、`remote.exec.v1`、`connections.observe.v1`、`workspace.multiplugin.v1`、`workspace.presentation.v1` 及已有主题/存储/剪贴板能力 |
| 当前终端包 | `sodeck.terminal` `0.4.0` | 保持已验证包哈希，不为接入文件而改终端业务；用该原包完成回归。API 3 及更早包保持拒绝 |
| 请求封装 | `RpcRequest.apiVersion:1` | 这是传输信封版本，不与 plugin API 4.1 混淆 |
| 工作台契约 | `workspace.v4` + 两项新增能力 | v4 原方法保留；新增可选 pluginId、呈现上下文与独立新方法，生成 TS/Rust 校验 |

旧 SDK 的严格响应验证不能因为 core 支持新字段就自动兼容。按实际包要求/协商能力选择回执投影：终端 0.4.0 收到原 v4 形状；新文件及新外壳可收到带 pluginId/generation 的扩展。不得让旧包收到未知字段后通过“忽略校验”维持运行。新包在 dev.4 上因缺能力/版本明确拒绝，不影响终端。

核心维护唯一公开能力目录，插件管理器兼容检查与 `capabilities.list` 消费同一来源；已有 `pty.v1` 等旧别名保持响应兼容，不顺便撤掉终端使用的契约。计划新增：

- `contracts/remote-read-v1.schema.json` 及正常/异常 examples；生成 `packages/sdk/src/remote-read.generated.ts`、`crates/core/src/remote_read_contract.generated.rs`。
- `scripts/generate-remote-read.mjs` 及 SDK `remote-read.ts` 验证/辅助方法；复用生成逻辑的公共部分，保持旧 workbench 生成结果来源清晰。
- 工作台 v4 的新可选字段、`workbench.contributions`、`views.presentation`、`views.requestFocus`、`workbench.dock.configure` 及对应能力检查；兼容矩阵纳入 RI-06/MP-01。

## 4. 公共接口与数据形状

本节固定本次实现语义和主要字段，schema 是编码阶段的机器可读唯一来源。字段可在不改变本方案行为的前提下补全验证细节；改变范围、平台、兼容保证或预算验收需先更新方案。

### 4.1 身份、路径和资源

- `WorkspaceScope = {workspaceId, workspaceGeneration}`，由核心工作台登记/撤销。新工作区先登记 scope 再创建贡献视图；关闭先撤销再取消所有读取，不能仅依靠稍后 worker 回调。宿主把可信 scope 附到调用上下文，插件自报必须匹配。
- 资源附 `{pluginId, instanceId, scope, leaseId, connectionId, connectionGeneration}`；视图输入还校验 `{viewId, mountGeneration}`。这些代次分别负责工作区重开、SSH 重连、表面重建，不能合并成一个整数。
- 当前终端不传 workspaceGeneration 时，宿主根据正在执行的操作/绑定注入；不得收到一个旧操作后简单取“当前同 ID 的新代次”。文件的新请求必须显式携带 scope 并由宿主比对。
- `RemotePath = {bytesBase64}`，只表示远端路径字节，禁止 NUL；绝对路径上限 16 KiB，条目名上限 4 KiB。UI 文本框 UTF-8 编码，目录返回保留原始字节；显示名与操作目标分离，不调用 Windows Path 规范化。
- 文件长度、时间和块计数使用十进制整数字符串；可选字段缺失用 null/缺失约定，不写 0。目录/文件/链接/FIFO/设备/socket/unknown 类型明确；大小格式化使用 BigInt，不经 Number 丢精度。
- 所有持有资源返回不透明 ID，不返原生指针。资源只存本次内存；目录和挂载数据不进布局、日志、插件 settings。

### 4.2 连接状态与仅复用

| 方法/请求 | 返回及语义 |
| --- | --- |
| `connections.observe({scope, serverId, configRevision, afterRevision?, waitMs})` | 返回状态 `idle/connecting/authRequired/active/failed`、revision、可复用连接摘要；不返回他人的 lease/秘密。最长等 1s 的有界长轮询，仅在面板可见/等待时使用，无变化后退避，不进行 SSH I/O |
| `connections.acquireExisting({scope, serverId, configRevision, operationId})` | 在注册表锁内匹配有效连接并登记本插件租约；返回已有 lease 形状，或 `CONNECTION_NOT_READY`。绝不建连/发送凭据；断线竞争返回失效，不转普通 acquire |
| 已有 `connections.acquire` / `release` | 文件显式“连接并读取”可调用 acquire；对旧终端保持语义。共同连接建立按服务器/账号/凭据修订/scope 合并，消费者取消互不误伤；最后引用释放回收 |

observe 状态只反映核心连接流程，文件不读终端 session 或 initializer 业务内容。自动路径等待或复用，不接入认证弹窗；显式连接走公开 `commands.execute` 及现有核心认证协调，各等待操作带 pluginId/scope/operationId。取消认证后必须由明确新操作才再次提示。并发不同主机/修订的挑战不能只以 serverId 覆盖，改为 challenge ID 与目标修订匹配；旧终端调用仍由宿主绑定原挑战。只修改受本次真实双插件影响的协调部分。

### 4.3 SFTP 只读接口

共同参数：有效 lease、scope、connectionGeneration、operationId；结果可查询且有数量/长度上限。接口不自动建立 SSH。

| 方法 | 主要输入 → 结果 | 特殊规则 |
| --- | --- | --- |
| `files.openDirectory` | path → directoryId | 每打开一次登记资源；失败/响应未交付仍可查询或取消清理 |
| `files.readDirectory` | directoryId、cursor、maxEntries → entries、nextCursor、eof | cursor 是代次内序列；重复同 operationId/cursor 返回同批，不再次推进远端游标；过期游标明确报错 |
| `files.closeDirectory` | directoryId → closed | 幂等，EOF 后自动关闭远端句柄，保留有限回执用于重放 |
| `files.stat` | path、followLinks 默认 false → attributes | 不跟随时为 lstat；follow 必须显式，权限/缺失与类型未知分开 |
| `files.readLink` / `files.realPath` | path → 原始 target/canonical bytes | 不修改原逻辑路径；链接不自动递归遍历 |
| `files.openRead` / `files.read` / `files.close` | path → fileId；fileId/offset/maxBytes → bytes/eof | 顺序或显式偏移只读，无写方法。允许 procfs 这种报告长度 0 但可读的文件；按 EOF/限额终止，不按 stat 长度误判 |
| `remote.operations.get` / `forget` | operationId → queued/running/completed/failed/canceled 及结果，或释放已读终态 | 和现有 `operations.cancel` 同一操作身份；只查询本来源，保留期限见 §8；显式 forget/超时清除回执 |

主要错误固定为 `INVALID_PARAMS`、`STALE_WORKSPACE`、`STALE_HANDLE`、`CONNECTION_NOT_READY`、`SFTP_UNAVAILABLE`、`PERMISSION_DENIED`、`NOT_FOUND`、`CURSOR_EXPIRED`、`TIMEOUT`、`CANCELED`、`LIMIT_EXCEEDED`、`CONNECTION_LOST`、`RESULT_UNKNOWN`；附 phase 与可重试说明，不把远端错误字符串直接用作有秘密的日志。

慢调用允许 15–30s 内完成；UI 命令只登记工作再快速返回，通过 worker 快照查询呈现，不占住渲染主线程。协议回执丢失时按 operationId 查询，不换 ID 自动重发创建/推进操作。资源创建交付失败留可查询终态；scope/实例关闭时立即回收，未被领取的结果 TTL 到期回收。

### 4.4 非 PTY 执行

`exec.start({leaseId, scope, connectionGeneration, operationId, command, stdinBase64?, timeoutMs, maxOutputBytes})` 返回 execId。command 是经过调用方明确处理的 SSH 命令字符串，不伪装原生 argv；输入上限 64 KiB，不允许携带在日志中的正文。

`exec.read({execId, afterSequence, maxBytes})` 返回有序 stdout/stderr 帧、state、exitCode/exitSignal、结束原因和最后序号；`exec.ack` 前移游标释放帧，`exec.cancel`/`exec.close` 可重入。只读是文件插件对该通用能力的使用约束，exec 本身不是权限沙箱。

输出总量超限终止本次采集并明确 `truncated:true/LIMIT_EXCEEDED`，不能当完整挂载表解析。取消停止所属 channel，不关闭同连接的 PTY；断线不能保证远端派生进程已结束。文件插件只运行有限采集命令，不后台化、不 sudo、不改服务器配置。

## 5. SSH 实现与路径适配

沿用已有 ssh2/libssh2 连接、主机校验、凭据及非阻塞 Session。网络操作在每连接单一所有者线程推进，核心注册表锁只用于登记/查询，不跨网络等待。

- 将 PTY、SFTP 请求与 exec 状态机分队列轮转；每轮每类最多推进 8 个就绪操作/64 KiB，PTY 输入优先且每轮获得机会。EAGAIN 返回调度器，不嵌套 retry/sleep 等待完整目录；保留该调用参数/缓冲及 libssh2 要求的续调状态，同一原生状态机禁止交错推进不兼容操作。关闭和取消走独立控制标记，不因普通队列满而无法提交。
- 每个文件租约使用受核心管理的 SFTP 子会话，句柄由调度器持有；目录只调用逐项 readdir，不使用一次性收集 Vec 的完整目录 API。重复 page 请求先查回执再推进。
- exec 通过独立 SSH channel，绝不送入终端 PTY。远端 MaxSessions 拒绝时明确报容量不足，不关闭现有终端腾额度。一个服务器可存在多个核心连接，但本轮不因自动文件查询静默增加 SSH 物理连接。
- 已取消/超时的调用立即禁止交付新数据；非阻塞关闭在清理队列继续执行。SFTP 子系统无法及时恢复时撤销该子会话下所有文件句柄并明确局部失败，关闭其 channel，终端连接保留；不能为清理文件而切断共享 socket。执行与清理上限见 §8。

**字节路径适配采用固定的小范围 ssh2 补丁。** 已读本机 0.9.6 源码，Windows `path2bytes` 会将 `\` 变成 `/`，`mkpath` 对非 UTF-8 用 unwrap。这不是 Linux 文件名的正确语义。vendor 副本只增加 `open_mode_bytes`、`opendir_bytes`、`stat/lstat_bytes`、`readlink/realpath_bytes`、`File::readdir_bytes` 等等价字节入口，直接传长度明确的字节给已有 libssh2；原 API 不改。新增适配覆盖 NUL、EOF、EAGAIN、名称超限和原始字节往返，依赖补丁不扩大到加密或传输重写。

Windows 上不再用 PathBuf 表示远端路径，也不将反斜杠按分隔符处理；路径拼接只使用远端 `/`，保持原始条目字节。显示时将不可打印/无效 UTF-8 字节转义，但不从显示字符串重建请求。

## 6. 多插件平台与工作台

### 6.1 通用注册与来源路由

新增 `PluginRegistry`：pluginId → Loaded/instanceId/process/assets/status/error；贡献索引以 `(pluginId, localCommandId/viewType/initializerId)` 为键。存储管理器已有插件映射继续使用，不再在启动代码用单个 active("sodeck.terminal") 表示全部运行插件。

- `plugin.invoke` 仍只能来自该插件自身有效视图；宿主按 binding 的 pluginId 找 worker，参数不能改来源。保留禁止跨插件调用的检查。
- 核心外壳 `commands.execute` 新增可选 pluginId；有 viewId 则按视图确定并校验，旧终端调用无 pluginId 时按可信调用者解析。只有外壳且 ID 唯一时允许无 pluginId；存在重名返回 `AMBIGUOUS_COMMAND`，不随意选第一个。
- 默认视图、命令按钮、初始化状态由 `workbench.contributions` 返回包含 owner 的数据。文件 UI 不出现在终端包，核心也不 import 文件模型。错误/重试/取消始终保留原 pluginId 与实例代次。
- 单插件 worker 失败只撤销其来源、操作、视图及租约；其他插件继续。文件实例恢复由用户触发核心恢复入口，用新 instanceId 重新初始化本次尚存在的文件视图；数据重新读取，不复制旧句柄；终端业务进程失败仍沿用其原有结束/重连语义。
- pending update 按每插件准备、激活、回退及错误记录；启动中一个文件坏包不阻止终端加载。全局退出先收集各插件关闭意见，确认可结束后统一撤销 scope、取消操作、停止进程和核心资源；避免一半已清理后才发现取消。

### 6.2 默认侧栏与初始化

文件 manifest：`files.explorer`，server scope、multiple false、dock left/right、default left、min 260×240；workspaceEntries openByDefault true；workspaceInitializers 为空。文件菜单命令含 `files.connect`（server、非 primary）、刷新/复制在插件内；首终端的 `terminal.initial` 保持当前声明。

新工作区先在内存建立/登记 generation，再独立创建各默认视图和调度各显式 initializer；一项出错记录到该项，不通过 `?` 提前返回阻止首终端。已有工作区定位、切换、移窗和表面恢复不再调度。用户主动关闭最后终端后文件仍可使用；关闭服务器重开建立新 scope 和默认文件状态。

文件显示需要 worker 初始化/显隐上下文。扩展公开呈现能力 `workspace.presentation.v1`：`views.presentation` 返回本视图 scope、visible、focused、revision；宿主向声明支持的插件发送合并的 `lifecycle.presentation` 通知，失去可见性时停止新增读取。worker/表面初始化禁止远端连接的原规则继续有效；读取必须在工作区绑定有效且面板可见后启动。旧终端未声明此扩展时不发送它不认识的生命周期命令。

### 6.3 焦点、布局和迁移

- `native.present` 只负责显隐/尺寸，不再因“刚显示”无条件抢焦点。核心维护窗口/工作区的焦点意图序列；打开服务器时默认目标是首终端，用户点击/键盘进入文件后前进序列，稍晚就绪的终端或文件不能覆盖该用户操作。
- `views.requestFocus` 只允许有效当前表面及明确用户手势；原生 focus 回报核对宿主前台/模态/视图可见性。后台连接成功不切换服务器或激活窗口。
- `workbench.dock.configure` 按 workspace generation/edge 更新内存宽度；默认 320，最小 260，最大为可用宽度下的 480 CSS px，窄窗保留可达收起入口；拖动实时本地反馈、结束提交，键盘每次步进提交。同窗口不同服务器不共用一个 React sideWidth。
- 批量移窗收集每个 view 的所属插件，分别 prepare/snapshot/bind；全部目标准备成功才提交原生归属与新呈现代次。失败按已准备集合逆序回滚；同工作区多个插件不能只给一个 instance 绑定资源。
- 完整树缓存留在 worker，移窗 snapshot 仅含可见 UI 状态和版本，最大 256 KiB；分页取得数据，不将整棵树塞进已有 1 MiB 宿主帧。停止跨启动工作现场保存的 T-009 行为保持。

## 7. 文件插件内部设计与 Linux 采集

### 7.1 模块与状态

拟建 `plugins/files/src/worker.ts`（生命周期/命令）、`state.ts`（每 scope 状态机）、`directory.ts`（分页/缓存）、`remote-path.ts`（字节路径）、`mounts.ts`（mountinfo/匹配）、`linux-collector.ts`（只读采集与命令编码）、`format.ts`（大小/时间）、`view.tsx`/`tree.tsx`/`details.tsx`/`files.css`。

状态以 `(workspaceId, workspaceGeneration)` 为键，含 lease、connectionGeneration、requestGeneration、selectedPath、expandedPaths、treePages、mountSnapshot、details、visible、autoReadPaused。明确状态：idle → waitingConnection → loading → ready/partialError/disconnected；关闭为 disposed。任一异步回执先核对 scope/实例/请求代次，错代次回收其资源并丢弃。

UI 通过自身 `plugin.invoke` 命令操作 worker；打开/刷新快速返回 operationId，快照以 revision/可见行窗口读取，只有进行中时短轮询，无变化退避。QuickJS 的内存/单次执行时间约束保留，树排序与解析分批让出事件循环。持久存储 schema 1 仅含 showHidden 等已确认偏好。

树使用可见行扁平索引及窗口化渲染，仍提供 tree/treeitem、层级/展开/选中和键盘语义。单个目录分批展示“继续加载”，有资源上限时明确不完整状态。路径定位逐级 stat/realpath，定位节点可单独插入可见树，不扫描 10,000 个兄弟才定位。目录大小保持空，不运行 du。

UI 沿用已读取的 apple-design 与功能规格：即时行反馈、稳定位置、13px 正文/至少 12px 说明、28px 行高、16px 图标/缩进、数字对齐、主题色独立、减少动态与高对比。不新增界面动效库、全局快捷键或玻璃装饰。

### 7.2 采集路径与可信对应

2026-10-26 r2 补充：[COMPAT-FILES-002 / F-FILES-001 r10](../plugins/files/features/server-file-tree.md#compat-files-002openwrt-挂载兼容) 已获用户“可以，你做吧”确认。以 exec 自身保持打开的根 fd/fdinfo + namespace + mount ID/inode + 前后完整表和进程身份替代 GNU stat 根身份依赖；容量优先 stat，失败尝试 df 的明确 KiB 格式，缺失字段不可用。具体实现、OW 测试及交付由轻量条目维护。下文 GNU stat 路线为 r1 历史基线，本增量替代其对应根身份和容量条件，不降低路径视图一致性要求。

实施补充（2026-10-26，原范围内的兼容修复）：真实 Debian 13 / OpenSSH 的 sftp-server 为不可 dump 进程，同账号 exec 无权 readlink `/proc/<sftp-pid>/ns/mnt` 或 stat `/proc/<sftp-pid>/root`。仍保留 PID/starttime、命名空间、根设备/inode 和完整 mountinfo 的全部对应证据：SFTP 通过自身 `/proc/self/ns/mnt` 取得 namespace ID；保持一个 `/` 目录句柄打开，从自身 fd 链接及 fdinfo 取得该根句柄的 mnt_id/inode，并用该 mount ID 的 mountinfo 设备号与 exec 自身 `/` 的 GNU stat 设备/inode 比对。采集前后再次核对进程身份与根句柄，所有证据一致才关联；缺少任一证据仍降级。此适配只使用既定只读接口，不增加权限、远端写入或降低校验保证，替代下文第 2 项受保护的跨进程 readlink/stat 取证方式。

1. SFTP 列目录/属性并保留真实字节。挂载采集按每个连接代次、首次需要、手动刷新执行；普通目录展开不逐项运行远端命令。
2. 读取 SFTP `/proc/self/stat` 获取其进程 PID/starttime，必要系统文件经有界文件接口读取。插件通过同连接 exec 以只读命令核对 `/proc/<pid>/stat` 的 starttime、`/proc/<pid>/ns/mnt` 与执行上下文的命名空间，以及 `stat -L` 所见 `/proc/<pid>/root` 与执行 `/` 的设备/inode；同时比对两侧 mountinfo。取样前后身份改变则重试一次或标不可用。
3. 只有这些证据匹配且标准 OpenSSH 路径语义可核对时，将 SFTP 路径与执行侧挂载目标关联。chroot、强制 SFTP、procfs/hidepid 限制、非标准虚拟 SFTP 根或身份无法核对时，目录仍可浏览，挂载信息标不可用。不能仅靠 uname、几个相同文件名或路径字符串假设两个根一致。
4. 挂载主数据使用 `/proc/self/mountinfo`：逐行按分隔符 ` - ` 分离固定/可选字段，处理内核八进制转义，保留原始路径字节、mount ID/parent/device/root/options/fstype/source。`\040`、`\011`、`\012`、`\134` 的解码在字段拆分后进行，不得先改写分隔符。解析/来源/字段脱敏属于文件插件。
5. 精确目标相等标挂载点；普通后代按路径段边界及最长有效匹配。bind 的源内 root 不等于可唯一还原的源目录；btrfs 子卷不误标 bind。没有充分证据时展示原始文件系统来源与源内路径，不假造 bind 源。叠加挂载无法确认顶层时列候选和限制，不以文件顺序作结论。
6. 选中详情时用 GNU `stat -f` 的固定数值格式采集 fragment size/blocks/free/available，BigInt 计算总量/已用/空闲/当前账号可用；总量 0 不计算百分比。采集前后校验挂载快照身份，变化则刷新/标过期。超时保留来源与类型，容量标不可用。

命令在插件内由固定模板与 POSIX 参数编码构造，`LC_ALL=C`、`--` 结束选项；原始路径以八进制 printf 编码及尾部哨兵保留换行，移除哨兵后作为单个带引号参数传递，禁止 `eval` 或字符串拼接执行文件名。批量数据使用长度/分隔明确的格式，通用 exec 返回原始字节及退出状态。敏感选项按白名单显示/复制，未知参数值默认隐藏，UI 不接收未脱敏原始挂载输出。

不为容量扫描全部挂载，不强行解释物理盘/RAID/容器拓扑，不求和共享挂载容量。Linux 特殊边界和采集能力缺失执行 FT-03/04/05；读取真实远端内核信息的验证不能只靠保存样例。

## 8. 明确预算与超限行为

以下为首轮实现与测试预算，编码前无需再猜常量；性能目标按 §10 的受控环境衡量，不把远端任意网络速度当承诺。

| 项目 | 上限/行为 |
| --- | --- |
| 插件传输 | 沿用 1 MiB 帧/4 MiB 排队上限；本轮单页序列化结果最多 256 KiB，字节 payload 单块 64 KiB，超限拆页，不增加全局帧限额 |
| 目录页 | 默认/最大 256 条，另受 256 KiB 限制；不是全目录原子快照；已读条目稳定排序，未读标明 |
| 单次目录/属性/读取 | 默认 15s、最大 30s，deadline 包含队列等待；空闲目录句柄 60s 过期，继续加载需明确重新读取 |
| exec | 默认 10s、最大 30s；文件挂载快照可取 15s；stdout+stderr 累计最多 4 MiB，缓冲最多 512 KiB，块 64 KiB；超限终止并报告截断 |
| 远端并发 | 每 scope 文件业务最多 4 个正在执行的 I/O；最多 8 个打开目录/文件句柄、2 个 exec；App 新增 I/O 最多 16 个执行、64 个排队；超额返回可恢复容量错误。现有终端 8 个预算保持 |
| 操作回执 | 每实例最多 128 个终态、合计 4 MiB，TTL 60s；只缓存已完成小结果/资源 ID，大 exec 输出走帧；相同 ID 参数冲突拒绝，过期不假装重放成功 |
| 文件树缓存 | 每工作区最多 20,000 条或 12 MiB、文件 worker 总缓存最多 32 MiB；包含路径及已规范化挂载记录。优先淘汰折叠/隐藏分支，选中可见分支固定；超限显示需重新加载/未完整载入，不能无限增长 |
| 挂载采集 | 每次最多 4 MiB 原始文本/8,192 条；解析增量进行，规范化后受总缓存预算；超限明确部分能力不可用，不用截断表假报完整 |
| UI | 视口外仅预留上下各 20 行，默认 DOM 树行不超过 300；详情只加载当前节点；移窗 UI snapshot 最大 256 KiB |
| 连接状态观察 | 有变化马上返回；waitMs 最大 1,000，空闲无变化退避到 5s；不可见停止，断线手动操作优先；不是远端周期扫描 |
| 取消与资源回收 | UI 在下一个渲染周期显示取消中；本机受控场景 1s 内进入不可再交付状态，5s 内关闭/撤销所属资源。异常服务器仍明确 cleanupPending/error，不为达标强关共享 SSH；App 退出核对实际进程/通道 |

用户可见方向、输入、取消等本地操作在受控测试中响应 p95 ≤100ms；本地隔离 Linux/OpenSSH 无注入延迟、单目录 10,000 条、两服务器并开且终端回显时，首批 256 条在连接已就绪后 p95 ≤2s（20 次），终端回显 p95 ≤150ms 且相对空载增量 ≤100ms（100 次）。记录机器 CPU/RAM、虚拟机、版本、样本原始值；未达标先优化并说明瓶颈，不能靠放宽指标标通过。

## 9. 实施顺序与每步完成条件

| 步骤 | 工作 | 进入下一步的依据 |
| --- | --- | --- |
| 0 基线与验证环境 | 用户最终确认后记录版本/差异/包哈希，准备隔离 Linux 和独立 App data，固定依赖版本及来源 | 验证环境可用或明确受影响阻塞；不用日常服务器；本方案所列验收条件可执行 |
| 1 契约与来源 | 保存 schema、生成类型/验证器、兼容投影、scope/资源身份及能力目录 | 正反例契约测试通过，当前终端 0.4.0 可加载，API 3/缺能力包准确拒绝 |
| 2 公共 I/O | 字节路径补丁、非阻塞 SFTP/exec、仅复用/observe、回执/取消/记录与预算 | 完成 RI-01 至 05、RI-07 的核心/协议部分及真实 OpenSSH 原语检查；慢读取时终端仍可操作。RI-06/07 的双插件联调在后续补齐 |
| 3 多插件与工作台 | Registry/路由、默认入口与初始化、认证/故障/移窗分组、焦点及内存宽度 | 正式文件插件的最小视图接入后完成 MP-01 至 03 的平台部分、RI-06/07 和 T-009 受影响回归；完整业务与更新在步骤 4/5 联合验收，不提前记所有 MP 用例通过 |
| 4 文件业务与 UI | 树/大小、路径定位、挂载采集与匹配、错误/空态、偏好/缓存/可访问性 | FT-01 至 14 与核心用例联调，真实 Linux 数据与系统对照一致，原生截图核查 |
| 5 更新与交付 | files 0.1.0/0.1.1 真实版本差异、故障包回退、双包开发目录和清单 | 同一核心/终端哈希下文件独立更新生效；完整重启空台；所有必需检查及清理完成，产物和限制写任务 |

步骤为同一次实施范围，不要求用户逐阶段批准。方案内解析/调度优化与修复自主处理；若需要改变首轮平台、兼容承诺、可见行为、测试目标或引入外部付费资源，先说明并更新方案。最终确认只在用户明确答复后记录，不因本规划完成自动开始第 0 步。

## 10. 验证环境、测试入口与工程完成标准

### 10.1 隔离环境

本次只读检查：已有 Windows 回环 SSH 夹具只回送 PTY；PATH 未发现 docker/podman；WSL 查询提示尚未安装。它们不能证明 Linux 文件系统与挂载语义。用户已选择“由我在确认实施后准备本地隔离 Linux 测试环境”，随后提供 `192.168.1.16` 作为可用端点；凭据不写入文档、命令参数或测试脚本。

已通过系统 OpenSSH 只读核实该端点为 Ubuntu 22.04.5 LTS、Linux 5.15 x86_64，普通账号 SSH exec/SFTP 可登录，GNU coreutils 8.32 和 `sh/stat/readlink/cat` 可用，exec 检查 mountinfo 具有可读权限，SFTP 可查询 `/proc/self/stat` 与 `/proc/self/mountinfo` 属性。此结果只证明基础连接及采集前提；尚未验证同一 SSH 连接下 SFTP/exec 的命名空间对应、真实挂载解析、插件能力或测试夹具。没有修改远端文件/配置或创建挂载；首次主机指纹由系统 OpenSSH 登记到本机 known_hosts。

该端点用于只读兼容与现存挂载对照。新增挂载、权限/网络故障及批量文件夹具仍使用下述本地隔离环境；没有把提供服务器凭据解释为最终编码确认。

推荐不改 Windows 系统特性的本地方案：便携 QEMU x86_64 + Debian 13 官方 cloud 镜像，优先检测可用硬件加速，不可用用 TCG；不安装 WSL/Docker 服务。运行程序、镜像、副本和测试凭据放 `.qa/t008/linux/`，镜像只读基盘加独立 overlay，约 2 GiB 内存、2 vCPU、磁盘预算 12 GiB。下载来源、固定构建版本、官方签名/哈希、许可先记录后使用；后台进程隐藏窗口。SSH 转发只绑定 `127.0.0.1`，测试私钥不进 Git，宿主不共享用户目录。此处是待确认环境方案，未下载或启动。

Guest 内运行两个 OpenSSH 测试端点/账号，建立普通/只读/bind/嵌套/tmpfs/叠加/稀疏/非 UTF-8 路径和受限账号；使用 guest 内 Samba 回环共享验证真实 CIFS 网络挂载，btrfs 子卷用 guest 临时磁盘镜像。测试管理员配置 guest 和夹具，产品始终以普通测试账号只读访问。慢挂载/网络和异常协议另用注入夹具补充，不能把注入叫真实网络挂载。

若便携环境的可信来源/启动/网络条件无法满足，继续可独立完成的实现和现有端点只读验证，明确夹具验证的阻塞；不把模拟通过当工程完成，不默认安装系统服务或在现有端点制造故障。需要用该端点承载测试夹具时，先提出具体目录、挂载和清理范围。凭据由本机受控配置引用，测试准备和清理由 AI 承担。

### 10.2 已有与拟新增入口

| 入口 | 用途与映射 |
| --- | --- |
| 已有 `npm run check` / `npm run test:contracts` / `npm run test:core` | 加入文件 workspace、新生成器/边界和核心测试；不是把类型检查当完整 lint |
| 已有 `npx playwright test -c playwright.ui.config.ts` | 新增 `tests/ui/files.spec.ts`，覆盖 FT-01/02/06/08/10 的桥模拟；保留现有 UI 回归 |
| 已有 T-009 原生脚本 | 在兼容的单插件和双插件配置复验空启动、首终端、取消/重试、托盘/拆合；协议环境证据边界保持 |
| 拟新增 `crates/core/tests/remote_read.rs` 与 `tests/core/remote_read_fixture.py` | RI-01 至 07 故障、代次、分页重放、限额及特殊字节协议测试；有限夹具，不冒充 Linux shell |
| 拟新增 `plugins/files/tests/` | 真实 worker+模拟核心桥状态机、mountinfo/路径/容量/脱敏纯规则测试；覆盖大小精度、特殊路径、bind/子卷/叠加不确定性 |
| 拟新增 `tests/desktop/files.mjs` | Windows 原生+真实隔离 Linux 的 FT-01 至 10、12 至 14；引用私有测试配置，保存截图/系统对照/资源计数 |
| 拟新增 `tests/desktop/files-updates.mjs` | FT-11 和 MP-04：两个实际插件、文件独立更新/回退、进程故障和哈希核对 |
| 拟新增 `tests/desktop/files-load.mjs` | §8 性能/预算：10,000 条目、并发回显、取消、缓存淘汰及 20 次展开关闭循环 |
| 拟新增 `tests/linux/t008/` 环境说明与夹具 | 仅实施确认后编写；部署隔离挂载/文件、记录对照数据、先卸载再清理，不运行在业务服务器 |
| 已有 build/pack/package-smoke | 扩展双包清单、能力及独立目录启动、移动目录、重启空台；产物证据与各版本哈希齐全 |

本表是 r1 规划的入口清单。实际实现将文件单元放在 `plugins/files/src/*.test.ts`，Linux 夹具放在 `tests/linux/t008/`，补充 `files-platform.mjs`、`files-degraded.mjs`、`files-visual.mjs`；核心真实边界入口为 `crates/core/tests/remote_read.rs`，未建立原建议的 `tests/core/remote_read_fixture.py`。准确命令和已运行结果见 T-008 实测，不把规划名称当成现存脚本。

### 10.3 多插件平台专属流程

共同准备：独立 data、原终端 0.4.0 签名包、真实文件包和必要冲突/故障夹具；两个测试服务器，原生主/工作区窗口。下列 MP 用例补充 FT-11/13 与 RI-06，实际双业务插件不可用时保持未运行。

| 用例 | 操作 | 预期 |
| --- | --- | --- |
| MP-01 兼容与归属 | 新核心载入原终端及文件；模拟两个插件使用相同 localCommandId/initializerId；旧包请求/回执逐项比对；让文件伪报其他 view/pluginId | 原终端不因新字段失败；路由靠真实所有者，不歧义选择；旧核心拒绝新文件包、API 3 仍拒绝；不能绕开禁止互调 |
| MP-02 初始化与故障 | 文件默认视图创建失败、慢初始化、worker 崩溃；新建服务器/并发重试，另使终端不可用后从文件入口显式连接 | 文件错误不阻止首终端；终端错误不禁用文件；重复事件不复制视图/初始化；故障仅清理所属实例资源 |
| MP-03 双插件迁移与焦点 | 同工作区终端+目录树拆合；故意在第二插件 prepare/target ready/bind 阶段失败；加载顺序反转、用户改焦点后放行旧完成 | 批量成功保持两插件上下文；失败全体回滚、原 PTY 继续，旧输入绑定撤销；文件/终端晚就绪不抢用户焦点 |
| MP-04 更新与退出 | 两插件运行时准备文件新版/坏包，分别正常重启、失败回退；运行中关闭文件、关闭工作区及退出并注入一项清理失败 | 文件新版 UI/解析规则有真实变化，核心/终端哈希不变；坏文件包不禁用终端；重启为空，关闭/退出不漏子进程、租约/通道；失败如实记录 |

每例结束关闭视图/租约和 App，核对 worker、socket、SFTP/exec 通道实际数量；恢复故障/包副本。guest 测试挂载从管理侧卸载后再删测试目录，不能在仍挂载时递归删除；强杀例单独核对残留。保存脱敏日志/截图/哈希后停止 VM；只清理确认位于 `.qa/t008` 的临时路径，保留可复验资料。

### 10.4 完成条件与交付

必须完成：契约/静态/依赖检查、Rust/worker 关键规则、浏览器、Windows 原生双插件、真实 Linux 文件/挂载、预算与取消、T-009 受影响回归、文件独立更新/回退、双包目录冒烟。未运行或失败的必要项不得把任务置为待验收。记录验证的是工作区内容，模拟/回环协议/真实 Linux/Windows 原生/安装验证分别陈述；本轮不生成安装器或宣称正式安装发布。

交付 `.local/dev/SoDeck-v2-T008/` 的 EXE、两个独立 `.sodeck-plugin`、公钥、主题、现有启动入口、README、build.json、哈希和截图。build.json 记录 core/pluginApi、每个插件 ID/版本/哈希、HEAD、sourceDirty 及工作区补丁指纹；排除运行 data、SSH 凭据和签名私钥。任务结果引用实际证据，不复制用户配置、不替换旧包。

## 11. 风险与停止条件

交付内复查（2026-10-26）：原生视觉脚本按实际 DPI 将逻辑客户区换算成物理尺寸，并记录双侧测量；保持 FT-10 的 900×600 / 1280×800 目标。目录包冒烟扩展为完整白名单核对、复制到含空格新目录并以该目录为 cwd、双插件加载/退出子进程核对。外壳历史 `.sessions-panel` CSS 类是通用停靠容器，双插件期应断言只承载“文件”而没有终端会话导航，不能继续用“任何侧栏数量为 0”的旧单插件断言；此修正遵循既定文件侧栏需求。

小窗截图复查发现旧会话侧栏的响应式 `max-width:210px/200px` 会覆盖工作区的 320px 默认宽度及 260px 最小值。按 F-FILES-001 / 文件插件的既定尺寸移除旧限制，继续由工作台有界宽度控制；FT-10 原生脚本在两种窗口尺寸断言侧栏仍为 320 CSS px，再核对 150% 文字、滚动和控件可达。该修复不改变功能或验收目标；重建外壳后只重跑受影响视觉与目录包流程。

实施内复查（2026-10-26）：按原定 RI/FT/MP 目标补齐后台回执 TTL、未领取资源和租约回收、关闭身份校验及资源诊断；SSH 已开始的非阻塞 channel-open 必须继续推进到可关闭状态，取消不能直接丢弃 libssh2 状态，stdin 重试保留原缓冲。新增回归覆盖取消时并发 PTY/SFTP、回执过期后的资源计数、错误代次关闭、目录刷新删除与选择回退、隐藏/关闭后的迟到回执。文件取消需包含当前详情查询，刷新仅在确认节点不存在后回退选择。上述为已确认范围的内部修复，保持接口兼容、共享终端及验收预算；先完成补充验证再记录通过。

取消取证补充：真实 OpenSSH 上注入 `sleep 20` 后，libssh2 已发送 channel-close，但对端可在命令退出前延迟确认；5 秒内公共句柄/租约已撤销，原生通道仍为 `cleanupPending`。按 F-CORE-009 已声明的“不能保证任意远端派生进程结束”和 §8 的撤销/待清理语义，RI-03/05 分别断言本地撤销时限、待清理计数、PTY 持续回显及命令自然退出后通道归零；普通已完成读取的关闭仍要求 5 秒内实际归零。不得丢弃未关闭的原生对象或强断共享连接，也不得把待清理计数描述为已关闭。

独立更新复查：多包启动路径只安装尚未登记的随附插件，不能在每次启动时用随附旧版本覆盖用户准备的 pending 更新。MP-04 验证 0.1.1 pending 经相同双包入口重启后生效及 0.1.2 初始化失败回退；诊断增加真实插件实例 PID，用于精确故障注入和退出后子进程核对，不增加业务权限或生产故障按钮。

路径呈现补充：有效 UTF-8 路径复制原始文本（包括真实换行），非 UTF-8 字节无法无损放入文本剪贴板时明确标为“复制转义路径”，树的实际读取仍使用字节身份；未编辑的路径输入回车复用该字节身份。控制字符/双向控制符在显示时转义。FT-02/06/10 核对复制、原始路径操作不混用及链接目标文件大小。

1. ssh2 非阻塞 SFTP 的取消/关闭以及字节路径适配先以协议与 Windows 测试验证；若无法在保持终端可用的前提下满足预算，记录结果再调整内部状态机。需换 SSH 库或改变兼容保证时更新方案，不悄悄牺牲终端响应。
2. 当前单插件假设分布在启动、invoke、资源绑定、移窗、认证、monitor/退出；完成清单逐项替换和双插件负例审查，不能只把 loaded 改成 Map 就结束。
3. 挂载来源/命名空间无法证实时宁可显示未知；不通过虚构 bind 源或将整个文件系统容量当目录大小来满足界面。真实 Linux 验证是必要条件。
4. API 4.1 必须实际运行原终端包并检查严格响应。若兼容投影不能保持原结果，先修改代码方案和兼容选择，不能未经确认升级为要求终端同步改包。

最终确认覆盖本文件 r1、T-008 r4、F-FILES-001 r4、F-CORE-009 r3 和文件插件 r4 所列实现、测试准备及独立本地开发包；不包含提交/推送/发布。用户确认后按第 9 节连续实施，无需对已确认范围内的每次编辑重复询问。

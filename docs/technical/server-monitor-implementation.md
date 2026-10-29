# 资源监控插件：代码修改方案

更新日期：2026-10-28 · 修订：r1 · 状态：已获实施确认，实施与验证见T-024
任务：[T-024 r2](../tasks/T-024-server-monitor.md) · 功能：[F-MON-001 r2](../plugins/monitor/features/server-resources.md) · [插件总览r2](../plugins/monitor/README.md)

本稿原响应用户“设计一下代码方案”；用户随后明确“可以开始做了。”，已授权本稿代码组织、公共增量、采集协议、预算、验证及开发包的完整实施，确认见T-024 r3。下文设计阶段的未确认说明保留历史含义；首版功能范围沿用F-MON-001。

## 1. 实际基线与关键取舍

基于2026-10-28当前v2工作区只读核实；存在其他任务未提交修改，不以HEAD或旧设计文字代替当前源码。

设计期间核对到[T-023](../tasks/T-023-settings.md)已获实施确认并占用API/SDK5.1。本稿顺接5.2，保留其设置契约/版本及外观文件变化，不把本任务尚未确认的clock混入T-023授权。源码核实部分仍如实记录当时5.0；集成时以当前工作区复核新旧包组合。

| 已核实来源 | 当前事实 | 本次处理 |
| --- | --- | --- |
| `contracts/capabilities.rs`、`packages/sdk/package.json` | 核实时API/SDK为5.0.0；T-023已授权向5.1实施 | 本任务顺接5.2.0，增加公开worker单调时钟；不新增监控专用核心服务 |
| `packages/sdk/src/remote-client.ts` | 现有Remote默认4MiB执行输出、15ms读取等待，结果不明后的回执查询没有客户端总期限 | 保留既有调用，新增可配置的有界执行客户端；本次不顺带迁移文件插件 |
| `contracts/remote-read-v1.schema.json` | exec已支持超时≤30秒、输出≤4MiB、stdin≤64KiB；read页32～64KiB，有结果查询/取消/关闭 | 复用现有wire契约，插件采用更小预算；不新增Linux命令RPC或statvfs能力 |
| `crates/plugin-host/src/engine.rs`、`bootstrap.js` | Rust Instant已支撑私有`__nativeNow`及定时器；JS没有公开单调时钟；worker内存上限128MiB，待决core调用上限32 | 发布小型公开clock适配；插件不得直接访问`__native*`或用Date.now计算速率 |
| `apps/desktop/src-tauri/src/workbench.rs` / `workbench_services.rs` | 支持dock/tab/window；默认创建入口最终调用open_view/activate，会改变焦点和同容器active视图 | 增加核心内部的默认打开路径，保留已有焦点/右侧选择；显式打开继续激活 |
| `packages/ui/src/charts/index.tsx` | TrendChart含发送/接收固定文案，1200点上限、缺口/键盘/表格替代已存在 | 提取通用SeriesChart/SeriesLegend，TrendChart保留兼容包装；不把CPU命名为发送 |
| `scripts/pack-plugin.mjs`、`scripts/check-ui-boundaries.mjs` | 打包支持plugin参数，但版本注入及UI检查列表只覆盖已有三插件 | 补monitor版本注入与自动发现受检查插件，保持依赖/样式边界检查 |

主要决定：采集、解析、调度、差值、缓存及UI业务均放在`plugins/monitor`；公共层只承担单调时钟、有界exec工具、通用图表与工作台默认打开行为。没有数据库变更、插件互调、服务端Agent、独立后台服务或用户终端命令注入。

## 2. 版本与交付边界

| 项目 | 本次方案 |
| --- | --- |
| 插件 | `sodeck.monitor` / 中文“资源监控” / 首包0.1.0；UI与worker共同独立构建 |
| 核心插件API、SDK | 5.2.0；新增能力`runtime.clock.v1`，保留5.0及T-023的5.1公开方法/DTO。原三插件和T-023新终端包可继续运行，不要求为monitor修改其业务 |
| 公共UI | 1.6.0；增量导出SeriesChart/SeriesLegend，既有TrendChart调用保持；UI随各插件构建，不热替换旧包 |
| 数据 | 核心schema6、观测schema2不变；monitor声明dataSchemaVersion1但本期无持久业务文件；无建库/迁移 |
| 插件承载 | 一个`monitor.resources`服务器级单实例视图，stateVersion1，支持`dock/tab/window`，只声明right停靠；默认320px，最小280×240px |
| 发布与平台 | 新建隔离Windows开发目录包，含现有三插件和monitor；客户端仅Windows验证，真实远端Linux/BusyBox。正式发布、安装器、提交/推送不在授权内 |

monitor清单要求`^5.2.0`和`runtime.clock.v1`，旧5.0/5.1核心明确拒绝新包，不能静默回退墙钟。运行时需同时探测clock存在，否则报告不兼容并停止采样。SDK公共执行函数可在其他消费者注入自己的单调Clock，但本插件只使用此公开clock。旧插件不需要新增能力声明。

manifest候选完整能力集合：`servers.v1`、`connections.v1`、`resources.v1`、`workspace.v4`、`plugin.invoke.v1`、`connections.observe.v1`、`connections.route`、`remote.exec.v1`、`core.state.v1`、`workspace.multiplugin.v1`、`workspace.presentation.v1`、`workspace.contributions.v2`、`plugins.metadata.v1`、`ui.presentation.v1`、`ui.theme.live.v1`、`ui.theme.catalog.v1`、`ui.theme.dopamine.v1`、`runtime.clock.v1`及观测events.v2/context.v1/snapshots.v1/metrics.v1。无需PTY、存储、目录跟随或SFTP能力；采集都在同一exec可见环境解释。

## 3. 模块及文件改动清单

下列新增路径均为计划，不表示已经创建。相近小模块可在不改变职责和测试的前提下合并。

| 路径 | 职责与修改 |
| --- | --- |
| `plugins/monitor/{package.json,manifest.json,tsconfig.json,vite.config.ts,index.html}` | workspace、双入口、版本注入、视图与命令贡献；依赖公开SDK/UI、既有React/Lucide，不引入图表框架或Node运行时依赖 |
| `plugins/monitor/src/{worker.ts,protocol.ts,validation.ts}` | 生命周期与自身命令路由，严格请求/快照校验；worker保有模型，UI只消费快照及发命令 |
| `plugins/monitor/src/{controller.ts,scheduler.ts,connection.ts}` | 每逻辑视图控制器、插件级预算/公平调度、租约/呈现订阅、关闭取消；显式状态机，单一写入点 |
| `plugins/monitor/src/collectors/{linux.ts,scripts.ts,framing.ts,inventory.ts,capacity.ts}` | 固定有限采集脚本、分段协议、能力探测、设备/挂载模型及容量查询；业务逻辑不进入SDK |
| `plugins/monitor/src/metrics/{cpu.ts,memory.ts,counters.ts,history.ts}` | 纯解析与计算、身份和基线判定、紧凑环形历史；以输入样本/clock测试，无SDK依赖 |
| `plugins/monitor/src/{main.tsx,view.tsx,view-client.ts,monitor.css}`及`components/` | 主题初始化、概览/详情/暂停频率/对象选择；CSS仅布局，通用控件和图表来自公开UI |
| `plugins/monitor/src/observation.ts`、`diagnostics/observability.json` | 声明动作/状态事件与插件自身健康指标，不将远端全部采样点写诊断历史 |
| `packages/sdk/src/{runtime.ts,bounded-exec.ts}`及包exports | 公开clock类型/获取方法，有界exec启动/读取/确认/回执/取消封装；不解析CPU/磁盘数据 |
| `crates/plugin-host/src/bootstrap.js`、相关宿主测试、`contracts/capabilities.rs` | 在现有Instant基础上暴露冻结的clock对象，版本/能力发现及旧包兼容；engine只在测试需要处补覆盖，不换计时实现 |
| `apps/desktop/src-tauri/src/{workbench.rs,workbench_services.rs}`及工作台测试 | 自动默认视图创建不抢已有活动工具/焦点，显式打开/激活保持；不按monitor ID特判 |
| `packages/ui/src/charts/`、公共样式/导出/组件测试 | 通用系列图、图例、百分比固定轴、紧凑概览样式；原通信图包装回归 |
| `scripts/{pack-plugin.mjs,check-ui-boundaries.mjs}`、`package-lock.json` | 独立插件版本注入、将新插件纳入扫描、锁文件更新；不为monitor加宽样式白名单 |
| `scripts/package-monitor-dev.mjs`（新增） | 新目录隔离交付，生成四插件清单与hash/来源/证据，禁止覆盖已使用的开发目录；现有历史打包脚本不重构 |
| `tests/ui/monitor.spec.ts`、`tests/desktop/monitor-*.mjs`、插件/SDK/宿主测试 | 各层测试、Linux/BusyBox真实采集、更新/故障/原生多窗；使用现有通用harness，不能直接复用固定旧包的files-harness配置 |

`resources/bundled-plugins.json`当前不在源树；由本任务打包脚本在交付目录生成，显式将`network-proxy`、`monitor`等目录名映射为清单ID，不用字符串猜路径。开发版本号在实施打包时依当前工作区分配并记录，API/数据/插件版本以上表为准。

## 4. 公共接口与通信

### 4.1 worker单调时钟

新增`runtime.clock.v1`的公开JS形状：`sodeck.clock.monotonicMs(): number`。返回从该worker进程创建起的有限非负毫秒数，单调不减，来源为现有Rust Instant；重启重置，不跨worker比较，不当作UTC。SDK `@sodeck/sdk/runtime`公开Clock类型和获取适配，UI使用浏览器performance.now；测试注入可控clock。

clock不经过RPC、不创建资源/外部I/O，因此不为每次本地读时钟写调用日志；所有连接与exec仍逐次经过核心网关。`Date.now`只用于用户可读更新时间。宿主5.2提供clock；缺失、非法或倒退直接使采样停用并报告运行时错误，不偷偷退回Date.now。

### 4.2 有界执行SDK

新增`createBoundedExecClient(bridge, clock, limits)`，公开`run`、`cancelAll`、`dispose`，可注入取消信号或等价token（不能假设QuickJS自带AbortController）。调用者传入不可变`scope + leaseId + connectionGeneration`、唯一operationId、固定command、可选stdin字节、remoteTimeoutMs、总deadlineMs、maxOutputBytes。客户端不拥有租约；租约只由监控connection控制器取得/释放。

返回结构化stdout/stderr字节、exitCode、endedReason、truncated、elapsedMs；非零退出、限额、超时和通道错误分别处理，不吞掉stderr状态。SDK允许业务读取有限失败输出，但不能将truncated结果当成功；采集器据完整分段单独决定局部可用。

内部顺序：`exec.start` → 有界`exec.read`/`exec.ack` → 结束判定 → `exec.close` → 完成后`remote.operations.forget`。read页使用现有允许的32KiB，空结果100ms后重读，最多一个未完成read/ack，不按15ms忙轮询。stdout+stderr共用总字节限额；累计字符串/解码缓存也计预算。

结果不明时保留同一operationId，按100/250/500ms查询回执至总期限；绝不新建ID重发exec.start。注意现有核心`remote.operations.get`取得成功即claim资源，`forget`也会claim：取得迟到execId必须关闭，不能“forget当作取消”。取消先禁止新读、取消未完成operation，已知execId执行cancel/close；始终用最初scope/lease/generation回收，不拿当前新连接替换。

总期限结束但控制通路未恢复时返回`cleanupPending`，保留有上限的清理槽，禁止该采集器继续发新任务；controller尝试释放自己的租约，必要时等待核心工作区/实例撤销。绝不为了回收本插件句柄而断开共享socket，也不把“停止等待”报告为已回收。迟到结果处理器必须继续尽力清理；正常可通信环境回收目标见§8。

现有remote-read-v1 schema无需修改；新SDK严格使用其生成类型/校验。文件插件继续使用旧Remote，本次回归证明没有行为改变；其存量实现改进另行安排。

### 4.3 插件UI与worker

使用既有同插件`plugin.invoke`。所有请求带真实viewId和宿主上下文，拒绝未知字段/命令；worker不能相信UI传入的serverId或任意shell命令。协议版本1，快照revision为十进制字符串，scope和连接代次明确携带。

| 命令 | 请求与结果 | 规则 |
| --- | --- | --- |
| `monitor.snapshot` | viewId、knownRevision、可选明细分页 → unchanged或完整当前摘要+当前选择的趋势 | UI可见时1000ms轮询，单一在途；无变化只回revision；不触发采样或连接 |
| `monitor.setOptions` | refreshSeconds=2/5/10、rangeSeconds=60/300/600、paused；严格可选字段 | worker提交选项并提高revision，重排定时但不复制在途任务；改变频率时重建速率基线 |
| `monitor.select` | metric类别、已公布对象ID、分页/展开选项 | 只选择当前模型中的对象；不存在则明确错误，不接受用户路径作为任意远端命令 |
| `monitor.refresh` | 唯一actionId，目标组可选 | 合并同轮请求；暂停中仅刷新一次；不绕过慢容量30秒/失败退避，返回下一次可采样时间 |
| `monitor.connect` | 唯一actionId | 仅显式操作可普通acquire并触发核心认证；与自动observe分开 |

命令确认立即返回accepted/revision或actionId，不等待整个远端采样完成；状态由snapshot反映，防止长invoke占住宿主业务调用。UI actionId用来合并同一用户操作，核心operationId另行生成。逻辑视图销毁使命令失效。

快照只传当前四组摘要、最多32行明细、最多301个当前选定趋势时间点，限制256KiB；超限返回清楚的分页/部分状态，禁止直接传全部内核原文或所有设备历史。UI本地展开/滚动可暂存；影响采样的选择与暂停只由worker持有。首次挂载拉完整快照，后续根据revision跳过重绘；不新增通用任意topic或跨插件feed。

## 5. 工作台与连接状态机

manifest命令为服务器级`monitor.open`（工具入口，只打开/定位）及`monitor.connect`（面板显式连接）；工具图标先用现有`layout-panel`，不为图标扩展协议。`workspaceEntries`默认创建视图，`workspaceInitializers`为空，不再自动建第二条SSH。右停靠宽320、支持中央/独立窗口，工具按钮使用既有容器管理。

核心新增内部`open_default_view`语义：自动入口重复命中已有单实例只返回，不activate；首次创建保留已有focusedViewId、focus_target及同边活动工具；该边无活动工具时新视图可见但不索取键盘焦点。默认入口按稳定pluginId/viewType顺序处理，随后运行既有首终端初始化；用户显式open/activate仍可切换。核心只处理通用布局，不读取monitor指标或保存其历史。无需增加manifest字段或改workbench DTO；对应F-CORE-004 WB-MON测试。

控制器状态分开维护：生命周期（active/stopping/disposed）、呈现可见性、用户暂停、连接（waiting/acquiring/connected/disconnected）、各指标质量（warming/fresh/stale/unsupported/error）与cleanupPending。不用一个loading布尔值替代全部状态。

- 初始化创建一个controller，订阅`view.presentation.v1`。输入校验scope/视图，支持重复initialize和挂载代次变化；不会因此重复创建采集器。
- 自动路径取得server revision后调用connections.observe(waitMs≤1000)；就绪才原子acquireExisting。等待连接时最多每5秒检查；不在隐藏/暂停状态继续observe。获取成功后再次检查本轮epoch；过期lease即释放。
- 活动租约绑定views.bindResources。每次恢复显示检查connections.route.get中的connectionId/generation；配置编辑不自动把仍有效的旧连接切到新目标。旧连接仍用旧配置时显示已有连接状态，用户明确重连才采用新配置。
- 显式connect去重为一项动作，沿用核心认证/主机指纹；取消认证进入等待用户，不再自动acquire。自动observe仅观察已有连接恢复。
- 同服务器不同终端Tab、终端内二次ssh不改变采集目标；多跳/SOCKS由核心解析，插件只持最终目标租约。
- 移窗保留controller、历史和选项，表面revision只管理UI绑定；真实visible=false立即停止调度、使sampleEpoch失效、取消并释放本插件租约。恢复显示等待释放完成再复用连接，趋势标缺口。
- 暂停与隐藏都停I/O并释放租约；暂停时刷新一次可临时acquireExisting完成快指标新基线/快照后释放，速率需要第二个样本时仍显示“计算中”，不会偷偷持续采两轮。
- close/workspace.closed/stop依次失效epoch、停timer、关闭呈现feed、取消I/O、解除资源绑定/释放租约、清历史；幂等。不同插件的租约不会进入本控制器清理清单。

每份返回数据校验controller实例、workspaceGeneration、connectionId/generation、sampleEpoch和该lane轮次；视图mount变化不直接丢弃仍属于同业务会话的样本。重新连接、机器boot变化、统计对象身份变化则重建差值基线。

## 6. Linux采集与解析

### 6.1 通道与分组

使用`exec.start`的固定command `sh -s`与有界stdin脚本，不分配PTY、不依赖SFTP。只读脚本随插件签名包发布，执行后不落盘，不后台运行，不修改profile。固定`LC_ALL=C`并使用受控基础PATH；无sh/cat等必要能力时说明不支持，不安装软件。`stdinBase64`仅传输编码，远端无需base64程序。

| lane | 内容与频率 | 输出/时间预算 |
| --- | --- | --- |
| fast | /proc/stat、meminfo、loadavg、uptime、net/dev、diskstats；2/5/10秒 | 256KiB，远端4秒，本机总6秒 |
| inventory | boot_id、内核/主机名、逻辑CPU集合、mountinfo、sysfs设备/网卡身份与类型；首次及每60秒 | 512KiB，远端4秒，本机总6秒；与capacity共用slow lane |
| capacity | 根/选中文件系统及展开明细中可见行；同对象至少30秒，低优先级轮转 | 单个64KiB，远端3秒，本机总5秒；每工作区最多1项在途 |

单个脚本以版本、轮次token、固定sectionId和明确结束状态分段，禁止基于“最后一行看起来像数字”猜成功。所有section必须有完整边界，未知/重复section、超长行、异常退出和截断分别处理。proc各段失败能保留其他完整有效段；整轮超时/截断时只发布有完整结束状态且来源可信的段，标partial，不能假装全量快照。启动shell插入的额外输出在封装边界外可记录“额外输出被忽略”的计数，不当成指标或输出全文到日志。

读取循环限制行数/字节；大机器上只取CPU汇总行，CPU数读取/sys/devices/system/cpu/online并解析有界区间，不传每核完整历史。接口/设备/挂载超出预算时模型明确partial及实际可见条数；同轮输出截断不能静默删除原来已知对象。控制器保留上一份完整inventory及时间，更新失败不把列表当空。

### 6.2 指标公式、时钟与身份

- CPU总计为user+nice+system+idle+iowait+irq+softirq+steal；busy差值排除idle/iowait，不再累加guest。每字段需非负且相邻不回退，deltaTotal>0；异常或重启先预热，不将无效比率夹到0～100掩盖问题。负载保留原数值，不是CPU百分比。
- 内存优先MemAvailable；缺字段时估算`MemFree + Buffers + Cached + SReclaimable - Shmem`，缺可选字段按0但标估算，缺MemTotal/MemFree则不可用。估算限定到0～MemTotal并标估算边界；已用为Total-Available。严格解析kB并转1024字节，不能吞下其他单位；SwapTotal=0为未配置。所有其他损坏/负数字段标无效。
- 网络用RX/TX字节；块设备用read/write sectors×512（该计数单位固定512，不能改用硬件扇区大小）。用BigInt作差，最终显示前才转有界浮点。分母优先同脚本采集前后uptime的中点差；无法获得时用本机monotonic请求起止中点差。来源改变、时钟无效、采样跨度过大或间隔超过max(3×周期,10秒)重建基线。远端时钟可用但请求延迟高时保留远端速率并标延迟，不当作即时值。
- boot_id优先作机器启动身份，缺时用uptime回退/连接代次保守判定。网卡身份为本次inventory代次内ifindex+名称；sysfs不可读时退化名称并标识弱身份，消失/重现即换代。块设备用major:minor、sysfs路径/名称，挂载用mountId、major:minor、root、mountpoint、fsType；设备热插拔/对象变化不续接旧趋势。无法观察到的同名短暂重建属于采样分辨率限制，不能宣称绝对识别。
- 严格解析mountinfo的字段和八进制转义，保留原始路径字节及独立显示文本。采集始终来自exec环境，不借文件插件的SFTP挂载模型。容器/命名空间只标为当前SSH可见环境，不推断cgroup资源上限。

### 6.3 容量与重复计量

优先探测GNU/BusyBox实际支持的`stat -f`固定数字输出（块大小、总块、空闲块、普通账号可用块）；只有确实支持预期格式才采用。失败再使用`LC_ALL=C df -Pk`的严格解析，并保留工具口径/保留块说明。`used=blocks-free`、字节乘对应块单位；占用率`used/(used+available)`，总量另列；分母无效则不可用。df仅在可以唯一识别数字列及路径时接受，遇到无法无损定位的输出给不可用，不猜测匹配。

输入只能来自已解析inventory中的绝对路径。脚本stdin按原始字节做POSIX单引号编码（单引号拆接），拒绝NUL，不用有损显示路径回拼命令；有界脚本超过64KiB时拒绝本项。stat/df输出不再被当作shell片段。前后核对挂载身份，不一致丢弃容量并刷新inventory，避免重新挂载后沿用旧容量。

默认排除proc/sysfs等伪文件系统，tmpfs单列；NFS/CIFS/SSHFS等明确网络文件系统默认不轮询，用户选择“读取容量”后才有限查询。不能可靠分类的挂载仍受3秒超时；慢挂载失败退避，不拖住fast。根/选中项优先，可见行最多8个，滚动后替换待查询集合，不预取全部128个挂载容量。

bind挂载标明同文件系统，概览不求整机总容量。网卡默认隐藏lo，虚拟接口可展开；不把bridge/veth/物理网卡累加。块设备显示层级/分区关系，不将dm与底层盘合计。磁盘IO趋势按设备，未确认与根挂载关系时不暗示等同。

## 7. UI、趋势与公共设计系统

继续应用已读取的apple-design：即时按下反馈、数值列稳定、清晰分组、滚动/焦点保持、不阻塞交互、减少动态及键盘替代；不新增装饰动画/玻璃或自动告警。三主题颜色仍来自公共主题。

SeriesChart支持1～2条系列，由调用者传id、label、单位、时间点、nullable值及质量/缺口；无业务请求。CPU/内存采用固定0～100轴；速率采用非负动态轴，不平滑跨缺口。SeriesLegend接受真实标签；旧TrendChart通过包装映射send/receive和原文案，原通信调用无需改参数。

新增紧凑/标准图高度变体与公共系列色别名，几何进入UI tokens来源，颜色别名复用现有主题traffic.send/receive映射，不在插件写色值、不新增主题协议字段；现有通信视觉不变。CPU高值仅数值/曲线，容量等级仍由capacityUsage等公共组件给出。图表表格、方向键、Home/End和文本说明可用；样本刷新不持续aria-live播报。

history按共享单调时间轴和数值列保存最多10分钟、每系列最多301点；最新指标保存无损计数字符串，历史只保存已计算显示值。每条系列独立缺口，不能因网卡缺样让CPU也断线。改变1/5/10分钟范围只读缓存；新对象无历史，未采集时间不补0。切换统计对象可读其已缓存历史，发生身份变化必须新系列。容量保存低频点，不伪造2秒采样密度。

环形历史以2秒为最小存点间隔；同时间桶内手动刷新只替换该桶末值，当前数值仍可更新，不因频繁点击提前挤掉10分钟窗口。低频采样只存真实点，不把正常的5/10秒间隔误标成故障缺口；此图用于资源趋势，不承诺保留桶内全部峰值。

快照携带worker计算的ageMs/相对时间及独立的显示用UTC时间；UI只用自身单调时钟累加“收到快照后”的时间，不直接相减worker的monotonicMs与WebView的performance.now。校时不会改变点间距离、缓存过期或速率分母；显示时间变化可以说明，不回写历史时序。

UI挂载销毁只清呈现轮询，不直接dispose worker业务。序列选择按时间/对象身份保持，新增点不会让用户正在查看的旧点跳到另一点；隐藏/超过窗口范围后说明数据已过期。公共UI处理缩放、图例/表格和色线可辨，插件只组合资源分组与选项。

## 8. 预算、取消与降级

这些是本次已确认的实现上限与验证目标，实测结果单独由T-024记录。

| 对象 | 上限/处理 |
| --- | --- |
| 远端对象 | 每目标64网卡、64块设备、128挂载；确定性保留当前选中/根及排序项，超限显式partial，不能给不存在的总量 |
| 控制器 | 最多64个完整逻辑视图控制器；超限新视图显示容量已满且不取得租约，不使服务器工作区创建失败；关闭释放名额 |
| 活动采样 | 最多8个可见且未暂停目标；超额显示等待采样名额，FIFO接替，隐藏/暂停/关闭立即让位 |
| 执行调度 | 每controller fast与slow各1轮；插件全局最多4个exec（fast最多3、slow最多1），每lane最多1个待执行意图，重复tick合并。先到期轮转，fast/slow各有配额避免互相饥饿 |
| 全部RPC | 本插件最多16个未决调用，其中普通I/O最多12，预留4给cancel/close/release/state关闭；不能用Promise.race放弃请求后把配额立即算作归还 |
| 缓存 | 每controller计入预算的历史/当前快照≤4MiB，全插件≤32MiB；已隐藏的缓存按LRU先驱逐，标“历史缓存已释放”，活动状态不静默丢点。瞬时采集/解码另设全局4MiB，不双份保留大原文 |
| 序列/传输 | 每系列≤301点、UI单快照≤256KiB、明细≤32行；低于现有1MiB宿主帧限；拒绝超过32KiB的单行，输入脚本≤64KiB |
| 取消回收 | 可通信环境停采反馈≤100ms，5秒内回收该次操作/租约；取消期间不接受新轮次。失联不承诺远端派生进程已终止，显示cleanupPending并在控制恢复后核对 |
| 失败退避 | 各指标/任务5→10→30秒，成功恢复所选周期；确定缺能力在连接代次内缓存，显式重试/重连才探测；每对象最多一条待重试意图 |

缓存上限按实际采用的数组/字符串/编码字节与保守对象开销记账；不是声称JS进程RSS等于32MiB。运行时仍有128MiB硬限，需测worker实际峰值与持续增长；超过本预算须修复结构或返回明确降级，不能在测试后擅自提高限制。

性能验证场景：记录Windows机器、worker版本与隔离Linux版本；8个可见目标、每目标最多64接口/64设备/128挂载的受控数据，运行30分钟；其中一目标容量请求超时，另一个10秒断线；终端持续输入输出、文件读取、隧道传输同时进行。必须满足预算、快指标不被慢容量阻塞且有公平进展；100次开关/移窗后回收计数回到基线。远端CPU/网络开销和采样延迟实测报告，不预先声称固定零开销或未经测量的p95目标。

## 9. 观测、留痕与日志

观测描述随插件签名：用户动作open/connect/pause/refresh/select，状态waiting/collecting/partial/disconnected/cleanupPending转换；采样失败按状态变化/恢复记录，不能每2秒写相同错误事件。每次核心RPC仍逐次审计，业务事件去重不是豁免调用记录。

snapshot只报告本插件自身状态、活动采集数、队列/缓存字节、最后错误与目标引用；metrics记录采集耗时、失败数/回收待决数等插件健康。不写挂载路径、网卡清单、原始命令输出或远端全部历史点，不把远端CPU当SoDeck自身CPU。采集流量自然按sodeck.monitor归入T-019，插件不二次上报通信字节。

调试测试入口只能返回计数/版本和采集状态；故障注入放测试transport，不向生产暴露“执行任意命令”的monitor私有入口。

## 10. 实施顺序与验证关口

获确认后连续完成以下步骤，已确认范围内的模块组织/修复无需逐项再问。若真实Linux能力实验表明需要Agent、sudo、新远端OS路径或更改范围，先更新方案。

1. **公共基础与骨架**：clock/API能力、SDK有界exec及测试，monitor清单和双入口、默认工作台打开；验证旧三插件加载及当前单实例模型。
2. **真实采集**：在专用Linux/BusyBox验证有限脚本、超时/输出界限/特殊路径、公式和身份；不先以模拟曲线冒充可用插件。
3. **调度与生命周期**：快慢分组、呈现/租约控制、结果不明/取消、环形历史及预算。先通过确定性故障测试再并行四插件。
4. **界面与公共图表**：SeriesChart兼容包装、概览/明细/范围/对象选择、三主题、键盘/缩放/减少动态；真实数据接入。
5. **工程交付**：完成下列矩阵、必要回归与真实Windows检查，构建两版monitor验证独立更新、坏包与回退，生成隔离开发目录及证据。

| 编号 | 准备/操作 | 预期/通过条件 | 清理、验证及功能映射 |
| --- | --- | --- | --- |
| MCODE-01 clock/版本 | 注入墙钟前后跳；真实worker读取clock；5.0/5.1/5.2加载新旧包，含T-023终端及设置回归 | 单调计时正确，新包被旧核心拒绝，5.2保留原包及设置行为；缺clock停采 | 恢复夹具、停worker；宿主/兼容；MON-04/MON-P-02 |
| MCODE-02 exec封装 | 故意丢启动响应、持续running、迟到execId、丢ack、输出截断、非零退出；取消后再交付成功 | 不重放start；查询/读取有期限，迟到句柄清理；claim后不遗留，未回收明确pending；16/12/4配额保持 | 关闭全部句柄/租约；SDK/契约+真实exec；MON-04/09/12 |
| MCODE-03 默认视图 | 三个测试声明共享right，既有焦点/选中面板；默认重复初始化与显式点击；实际四插件新工作区 | 自动创建不抢占/不复制，空侧栏正常显示；显式点击激活；首终端仍可输入 | 关闭工作区；工作台单元+Windows；MON-01/02/11 |
| MCODE-04 解析及Linux能力 | 已知proc样本、guest/iowait、估算内存、特殊字节路径/引号/换行、保留块；Linux与BusyBox逐项缺工具 | 公式与口径匹配，部分能力准确降级，无注入/乱码回拼；512扇区固定 | 恢复专用权限/挂载；纯函数+真实guest；MON-03/05/06 |
| MCODE-05 身份/时钟 | 重连、boot变化、热插拔、名称复用、慢链路、uptime缺失及本机校时 | 身份/计时来源变化断基线；不跨对象/跨缺口，数据延迟有标记 | 清故障，恢复连接；确定性+真实；MON-02/04 |
| MCODE-06 调度预算 | 使用§8场景，超输出/对象/快照/缓存，65个逻辑视图、9个可见目标、100次移窗/关闭 | 硬上限/partial/排队明确；缺能力不高频重探；慢容量不堵fast；公平进展与资源基线 | 停负载/恢复注入、导出计数；预算/集成；MON-07/12 |
| MCODE-07 呈现与路由 | 真Linux直连、多级SSH、SOCKS；A/B快速切换/独立窗；隐藏/托盘/暂停刷新一次/认证取消 | 只读最终目标；可见性、代次、暂停语义准确，认证不循环，原三插件不断 | 回收专用代理/会话；原生+集成；MON-01/02/08/09/10 |
| MCODE-08 图表兼容 | CPU固定轴、单/双系列、空值、长范围、keyboard与表格；原通信图用原参数 | 标签正确、缺口不连接、CPU轴稳定；旧发送/接收图行为不变 | 清夹具；公共组件+浏览器；MON-07/11 |
| MCODE-09 UI原生 | 三主题、200%文字、高对比/减少动态、窄侧栏/中央/独立窗、连续刷新 | 无裁字/焦点抢占/滚动跳跃；无按周期读屏轰炸；原生表面边界正确 | 恢复系统设置；截图与Windows操作；MON-11 |
| MCODE-10 独立更新/交付 | 0.1.0运行时准备测试0.1.1（UI及解析行为可区分），重启；坏签名/不兼容/启动失败；目录包全新启动 | 当前不热换，重启生效/明确回退；原三插件hash保持，曲线不跨启动；开发包启动有证据 | 恢复隔离包和数据，停进程；真实包+原生；MON-P-01～03 |

已有可运行检查：`npm run check`、`npm run test:contracts`、`npm run test:core`、`npm run build`、`npm run desktop:build`、`node scripts/pack-plugin.mjs --plugin <已有插件>`。新增后计划入口：`npm run test -w @sodeck/monitor`、`node scripts/pack-plugin.mjs --plugin monitor`、`npx playwright test -c playwright.ui.config.ts tests/ui/monitor.spec.ts`、`node tests/desktop/monitor-native.mjs` / `monitor-updates.mjs` / `monitor-load.mjs`、`node scripts/package-monitor-dev.mjs`；当前均不因写入文档而可用。

回归以影响为界：公共SDK/host版本、工作台默认打开、文件容量等级、通信图表、现有终端/文件/网络包共存、托盘/移窗。完成必要检查后不无故扩大范围；Mock、真实SSH、Windows原生、目录包分别记录，不称安装验证。

## 11. 完成标准与当前结果

只有F-MON-001、MON-P及MCODE相关必需检查通过、真实数据界面/独立包可查看、资源清理与限制有证据后，T-024才进入待验收。Linux/BusyBox、路由和Windows原生缺证据时保留未完成；不让用户代做工程验证。用户验收与发布另外记录。

本稿记录获确认的设计基线；当前实现、实际集成版本、各MCODE验证结果与目录包由[T-024实施记录](../tasks/T-024-results.md)维护，不以设计阶段的候选/未运行描述替代当前事实。

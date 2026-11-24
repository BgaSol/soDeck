# SoDeck 当前架构

核心使用 Tauri/Rust，宿主界面使用 TypeScript、原生 DOM 和 Vite；插件以独立进程及独立本地 UI 资源运行。架构围绕连接、资源归属和显示交接管理通用能力，具体操作规则由功能文档定义。

## 分层与代码入口

| 层 | 职责与主要入口 |
| --- | --- |
| 桌面适配 | 原生窗口、系统事件、Tauri 命令及应用收尾：[main.rs](../src-tauri/src/main.rs)、[windows.rs](../src-tauri/src/windows.rs) |
| 宿主界面 | 启动组合、壳导航及应用读取、管理台、工作区和插件 Surface：[main.ts](../src/main.ts)、[shell.ts](../src/shell.ts)、[console.ts](../src/console.ts)、[workspaces.ts](../src/workspaces.ts)、[components.ts](../src/components.ts) |
| 核心状态 | 服务组合、持久配置、受管实例和事件：[core.rs](../src-tauri/src/core.rs) |
| 连接服务 | 资料快照、认证、主机密钥、SSH/SOCKS5 混合链、取消与代次：[connections](../src-tauri/src/connections/mod.rs) |
| 工作台与生命周期 | 工作区/Host/Group/View/Surface、布局、迁移、关闭与资源接纳：[workbench.rs](../src-tauri/src/workbench.rs)、[component_ui.rs](../src-tauri/src/component_ui.rs)、[lifecycle.rs](../src-tauri/src/lifecycle.rs) |
| 通用会话能力 | PTY、短命令、SFTP 文件、流式正文、TCP 通道与计量：[terminal.rs](../src-tauri/src/terminal.rs)、[session_exec.rs](../src-tauri/src/session_exec.rs)、[remote_files.rs](../src-tauri/src/remote_files.rs)、[network](../src-tauri/src/network/mod.rs) |
| 插件与 RPC | 清单、子进程、控制分派、停止和错误：[plugins](../src-tauri/src/plugins/mod.rs) |
| 组件命令 | 单处理者路由、稳定意图、回执与页面关联：[commands.rs](../src-tauri/src/commands.rs)；复用既有 RPC，不增加传输层 |
| 共享状态与设置 | 核心管理作用域、修订、订阅、冲突和持久化，插件解释业务含义：[shared_state.rs](../src-tauri/src/shared_state.rs)、[settings.rs](../src-tauri/src/settings.rs) |
| 更新和系统适配 | 下载、官方验签、缓存、默认应用与本地文件：[updates.rs](../src-tauri/src/updates.rs)、[platform.rs](../src-tauri/src/platform.rs)、[desktop_files.rs](../src-tauri/src/desktop_files.rs) |
| 业务插件 | [terminal](../plugins/terminal/src/main.rs)、[files](../plugins/files/src/main.rs)、[network-proxy](../plugins/network-proxy/src/main.rs)、[system-status](../plugins/system-status/src/main.rs)、[docker](../plugins/docker/src/main.rs)、[mcp](../plugins/mcp/src/main.rs) |

共享 [plugin-protocol](../crates/plugin-protocol/src/lib.rs) 定义 API 版本、Context、请求和错误，核心与插件客户端使用同一契约。[页面 SDK](../packages/plugin-sdk/src/index.js) 随每个插件独立打包，负责 Surface 身份、等待请求与清理。

[plugin-client](../crates/plugin-client/src/lib.rs) 提供 RPC、命令票据、设置、共享状态和文件编辑生命周期客户端，不保存连接凭据，也不封装插件之间的直接调用。文件编辑的标准 UI 在 [packages/file-editor](../packages/file-editor/)，随使用组件独立打包；[接入与职责](features/file-editor.md)由通用编辑功能说明维护。

短命令、命令捕获和终端通过 [remote_execution.rs](../src-tauri/src/remote_execution.rs) 共用通道启动、读写、退出和取消循环；[execution_output.rs](../src-tauri/src/execution_output.rs) 只包含既有内存结果、Capture 和 Replay 三种输出。各服务仍管理自己的接纳、额度与持久结果，不设第四张全局资源表。[session_process.rs](../src-tauri/src/session_process.rs) 替代原命令流资源表，提供原进程的有界顺序输入；正文订阅、固定阅读与 ACK 位于 session_process/output。MCP 管理调查任务、调用端输入身份与本地证据；提示解释和输入内容由客户端决定。

进程正文由[session_process/output.rs](../src-tauri/src/session_process/output.rs)复用核心会话，[capture.rs](../src-tauri/src/streams/capture.rs)管理有界原始字节与固定阅读范围；[content_export.rs](../src-tauri/src/content_export.rs)只负责用户选择的本地字节导出。契约统一见[插件API](plugin-api.md)。Docker命令、原字节解析、源时间、级别、有界记录、历史覆盖、查询、冻结导出与任务代次属于插件进程的日志会话；Docker UI通过有界只读查询呈现投影、测量折行与保存阅读锚点。Surface不是日志任务所有者，重挂载不重开命令。核心不保存容器日志索引，不导入Docker正文样式；颜色和基础控件继续复用packages/ui。

[Agent MCP插件](features/mcp.md)负责回环 HTTP、Token、字面资料发现、工具参数、原请求的目标准备与证据保存。核心负责非敏感资料投影、实际连接接纳、既有会话、命令流及通用复制，契约见[API](plugin-api.md)。桌面与外部 open 使用同一个连接注册表与实际 Attempt；原请求表保存 attached/accepted/failed/cancelled，current 索引只代表当前尝试。accept 在同一连接/工作台锁边界固定原 Context，取消先登记原身份以阻止迟到派发。凭据不进入选择令牌，路线匹配复用原 trust_scope。

MCP 的打开收据表示取得准确可执行 target，核心收据表示 SSH/工作区接纳；两者承担不同阶段。每个未完成打开只有一个有界控制任务，get/事件负责重读，取消按原请求收尾。后台 View 初始化仅登记目标与返回 ACK，不在回调中反向读取核心会话；核心非阻塞观察创建 ACK，MCP 再核对完整 Context，避免 RPC 循环等待或旧代结果误发布 ready。打开终态固定，当前会话事实继续由既有目标接口读取。后台 View 绑定完整 Context，HTTP 会话断开不改变远程任务的工作区归属。插件保留原生结果与本地保存结果，数据使用同一 JSONL 和元数据，不建立恢复数据库、第二套远程执行或业务插件调用路由。MCP 协议使用官方 rmcp SDK，HTTP 使用 SDK 服务与 axum；客户端选择调查步骤和分析结果。

插件内的运行注册表以任务 ID 与请求键索引同一 Task，Task 统一管理自有阶段、取消与有效 revision；原核心 native JSON 保留。有意义的原生字段在 task 中统一选择，版本变化使用未裁剪事实，普通输出在各自出口简化。目标观察统一取得经过 Context/字段校验的类型化 Observation，Task 的 Record 保存确认时的完整 Descriptor；发现、采集和侧栏不通过旧 JSON 传递内部目标。命令与文件使用两条具体采集路径；上传和下载共用文件收集器、接纳序号和原语回执，接纳域占用不跨 RPC 持锁，也不排队启动新 worker。启动解析分离实际执行参数和本次等待，新正等待先取得位置，再登记并同步完成后台派发；等待许可由接纳层持有并交接，查询层消费许可，不保存响应 JSON 或持久等待状态。启动首读与 task_get 使用同一 watch 循环，分别等待稳定完成与既有变化；等待结束释放位置，再按同次捕获读正文。调用取消不绑定 Collector 或 Task 的取消令牌。统一收尾先封闭取消修改，再保存与清理，最后发布稳定终态。Task 的私有 Record 是唯一运行状态；读取、取消与列表交接不可变身份和该次 Record，不将 JSON 投影作为内部状态。ui 模块组装侧栏，快照的本地身份独立于目标观察成败；response 分别构造 MCP 普通/诊断及 UI 输出，evidence 独立定义磁盘元数据 schema 3，保存时消费指定 Record，不复用展示函数。text/full 正文互斥，不建立 DTO 缓存或通用渲染框架。MCP 与 UI 查询使用同一次状态/已保存水位记录和同一证据 reader，UI 读取及取消检查实际 Context 归属；侧栏只接受文本分页和最近页，完整信封和等待属于 MCP 出口。列表与 async 等待消费既有事实。参数及资源额度只在 [MCP API](mcp-api.md)完整定义。

MCP 上传的主机路径只交给核心只读来源适配，普通文件打开后固定句柄并复核身份；不伪造原生 inputId。核心单文件复制把请求策略、来源读取、提交与状态分开维护，原文件插件和 MCP 共同使用。复制快照独立记录提交与进度，终态合并保留这些事实；MCP 在释放原生回执前持久化最终事实，记录失败时保留资源到原生命周期关闭。上传不经过文件插件，不新增传输协议或任务注册表。契约见[文件复制 API](plugin-api.md#文件操作)。

## 宿主状态与 DOM 边界

插件自有图形由插件清单及包内资产提供。核心负责通用路径/版本检查、主题色展示及失败回退，资源读取独立于业务进程生命周期；各显示入口共用同一图标来源。已安装图标取当前包，官方目录预览取同源仓库资源。完整契约见[插件图标](plugin-api.md#插件图标)。

Shell 拥有当前管理页/工作台导航意图及应用快照、网络计量；连接服务只接纳真实会话，显示定位由发起窗口的有效意图明确请求。管理台不是伪工作区，核心页草稿继续属于原页面控制器。Workspaces 管理工作区操作，Components 独占 Surface 挂载、消息身份、有效可见性和输入权；壳隐藏宿主时只取消显示手势与焦点意图，不销毁 View 或远端资源。

更新的启动、一次缓存恢复、自动/手动检查与下载编排由 [UpdateService](../src-tauri/src/desktop_updates/service.rs)随应用管理，与前端就绪和页面导航独立；核心保留原更新状态及与关闭协调共享的接纳边界。批次显式提交本轮结果，守卫只兜底异常收尾。核心 updater 仍独占官方验签与安装，插件检查和切换仍在原更新模块中。

[官方安装编排](../src-tauri/src/desktop_updates/official_install.rs)复用该服务和原保留任务安装执行器，自动整轮与手动单项共用来源检查、下载和登记。AppCore 在有效配置读取后固定启动时是否无插件，Shell 只用其决定一次默认页；安装编排接在本次 `start_enabled` 完成后，新登记不会在本次启动意外加载。整轮守卫覆盖目录读取和项间空隙，与检查、下载、插件登记修改及关闭共享接纳；整体准备状态只保存本进程阶段/固定目录/整体错误，逐项进度仍在原更新表，不持久化另一套工作流。

宿主内部 `update_snapshot` 一次返回更新条目与检查概览，读取失败保留完整上一份结果；应用状态和网络仍分区读取。前端 [update-model.ts](../src/update-model.ts)提供页面、侧栏与顶部入口共同使用的纯显示计算，草稿与勾选不移入 Shell。此接口与核心同包，不修改插件 RPC 或 Release 清单；功能规则见[在线更新](features/updates.md)。

[正文桥](../src/component-data.ts)为每次原生调用创建独立 Tauri Channel，因为各调用的传输序号独立从零开始。Surface 只管理所属订阅的解绑请求；确认、状态、文件请求及其他订阅不会结束正在接收的正文通道。

公共 CSS、主题值、主题 ID 登记及已发行 token 名称统一维护在 [packages/ui](../packages/ui/README.md)。核心直接编译共享源码，插件独立打包自身 CSS 副本并沿用 当前 token 下发；宿主尺寸限定在核心容器内。核心不导入插件业务 UI，不按具体插件 ID 分配入口或业务行为。

页面控制器拥有草稿、基线修订与保存结果，核心快照只提供已保存状态。Shell 持有管理台一级导航和固定的设置子导航槽，管理台组合器分别渲染导航与当前页面；设置、插件和更新各自选择内容，设置分区不承载一级页面状态。组件设置独占自己的挂载容器，父级刷新不遍历或搬回其内部节点。切页/关闭设置只结束显示，草稿随宿主保留；退出规则见[生命周期](features/lifecycle.md)。后台读取分区接收，失败保留上次有效状态并展示对应错误，失效请求不能更新新实体。

[DOM 协调](../src/dom-update.ts)固定使用 morphdom 2.7.8（MIT）。它替代核心与网络隧道的递归 diff；页面声明实体 key，控件 key 限定在所属实体内。稳定表单持续输入不重新挂载，显式保存/放弃仍按模型更新控件。按压队列属于具体容器，click/cancel/失焦后提交最新投影，销毁时结束监听与待绘制任务。iframe、Monaco、xterm 由各自控制器独立挂载，不交给通用协调器。插件通过既有构建入口携带运行资源与许可证，不从核心安装目录加载它。

选型理由是提供成熟的 keyed 节点协调、SVG 与控件属性处理，并删除重复算法；替代方案是控制器逐项更新固定节点，本轮不引入前端框架。上游为 patrick-steele-idem/morphdom，固定版于 2026-01 发布，升级前复验本项目输入、分屏和按压回归；不以依赖替代生命周期设计。

## 连接与资源归属

核心掌握认证资料与真实连接。插件收到包含服务器、工作区、连接和 generation 的固定 Context；创建和提交资源时核对插件进程实例、逻辑 view 与 Context。重连产生新代次，旧请求不能写入新会话。

服务器目录修改由 [change.rs](../src-tauri/src/servers/change.rs)集中编排，桌面完整保存、部分修改及插件 [rpc.rs](../src-tauri/src/servers/rpc.rs)共用同一修订检查、SSH 保存函数、凭据处理和 commit；read 保留非敏感发现投影，插件写入只返回 ID 与修订。入口持有原有修改边界，通知在目录锁释放后发出。

宿主内部 `change_catalog` 的 `saveServer.draft.copyFromId` 是可选凭据来源，仅允许 `id` 为空且 `authentication.secret.action` 为 `keep` 的新增服务器使用；缺省或 null 保持普通新增/编辑行为。核心在接受的目录修订内校验来源 SSH 资料存在、认证方式与私钥路径匹配且有可用密码/口令，从来源 ID 读取后按目标 remember 复制到目标 ID 的持久或运行期存储；主机、端口、账号和路径可修改。来源不被移动或修改，后续目标凭据各自独立；目录/标签与运行期映射一次提交，失败均不提交。字段只用于本次提交，不持久化、不给插件或公开目录返回，也不提供秘密读取接口；宿主已退役立即落盘的 `copyServer`，`copyProxy` 保留。此为核心捆绑宿主命令，不改变插件 RPC 或目录 schema。

Owner/Context、任务结束和 UI 输入权是不同约束：

- [resource_owner.rs](../src-tauri/src/resource_owner.rs) 校验资源调用者及当前接纳状态。
- [resource_tasks.rs](../src-tauri/src/resource_tasks.rs) 跟踪实际在途 worker，取消信号不等于结束。
- 插件进程由核心监督；调用超时后仍可能等待真实回复或进程终止，未知副作用不自动重放。
- 单页资源结束不关闭共享连接；全工作区/应用关闭按范围清理连接。远端命令已有副作用不承诺回滚。

注册表临界区只做短状态操作，跨进程等待、网络和磁盘 IO 不持有状态锁。关闭保留与 view/resource 登记共享协调状态，防止检查与提交之间重新接纳业务。具体终点和失败规则见[生命周期](features/lifecycle.md)。

## 工作台与插件 UI

Workspace 表示服务器会话上下文，Host 表示某个窗口中的显示部分，Group 表示分屏标签组，View 是插件逻辑实例，Surface 是某次页面挂载及输入身份。View 迁移时可以有只读候选 Surface，但只允许有效激活的表面输入。

同窗隐藏/排序/分屏保留 iframe，跨窗按准备、flush、排空、恢复、提交交接；失败取消保留源实例。后台逻辑 view 的运行不依赖 iframe 是否存在。纯快照不创建、销毁或恢复业务资源，初始化由明确工作区和插件状态变化触发。

宿主通过本地资源协议加载插件 HTML/JS/CSS，经 postMessage 和核心数据桥交互。业务 UI 不直接持有服务器认证。统一基线、控制帧、数据分片和版本策略统一维护在[插件 API](plugin-api.md)。

## 组件协作

共享状态表达当前事实，命令表达一次执行意图。两者复用进程 RPC、真实实例和 Context，分别管理修订订阅与命令回执。命令不借用状态订阅的重试，不做广播执行或自动转投。

命令登记表放在 workbench::State，与工作区、布局、关闭保留共用短状态锁。接纳先固定处理实例、来源 Host/Group、指纹和预算，释放锁后加入现有 RPC 队列；commands.submit 不等待另一插件完成。页面登记与命令关联原子提交。回执的逻辑终态和实际 RPC 任务守卫分开，关闭覆盖没有页面的命令，失效调用仍占在途额度直到真实结束。

核心“＋”、默认创建、终端重开和 Docker 进入容器都提交 terminal.open.v1。处理者属于终端进程，不依赖终端 iframe；终端负责创建参数，Docker 负责 socket、Engine、容器身份与 shell。已登记终端归自身 view，来源页的生命周期不收养它。行为声明仅提供展示和命令引用，不保留第二套执行表。完整字段、限额和失败契约见[命令 API](plugin-api.md#命令默认行为与进程主动创建页面)。

## 核心能力与业务边界

- 核心 PTY/流服务传输原始字节、管理恢复水位与额度；终端插件负责 shell 目录集成、xterm、搜索和颜色。
- 核心文件服务负责资源身份、受限本地引用、不可变草稿快照、传输和提交结果；正文内存锁与 SFTP IO 分离。文件插件负责目录 UI、批次编排、冲突交互及编辑器，由核心修订和提交回执投影 dirty/busy，不以通知成功推断保存成功。
- 核心网络服务提供受管 TCP 监听与通道，按实际任务退出确认清理完成；SOCKS5 入口协商和规则管理在网络插件。插件分开保存运行事实、查询错误和停止意图，RPC 等待不持有整个插件状态锁。
- 核心短命令提供启动票据、输出分页、取消和期限；系统状态解释计数和指标，Docker 解释 Engine、容器及命令结果。
- 核心 remote_program 负责当前会话的有界环境探测、POSIX/Windows 程序参数编码和明确启用的启动结果前导，复用原 exec/process/PTY 服务。Docker 自己解析 CLI/Context/端点，固定不可变 Engine 目标并提供身份检查参数；插件不拼平台 shell 或直接依赖传输实现。契约见[程序 API](plugin-api.md#会话目标短命令与终端)。
- 目录/容量共享由核心控制作用域和版本；终端/状态插件发布，文件插件按自身规则消费。核心不为每对插件增加专用调用关系。
- 日志与消息分别接入，来源由核心附加，通知由消息参数明确选择。

分进程不等于沙箱隔离。插件按可信扩展运行，无权限清单或审批；API 版本、参数、资源身份及操作系统自身权限继续校验。

核心会话目标摘要与工作区完整 Context 同时接纳，在断开后保留最近真实连接，仅运行内持有；完整目标事实读取由 session_target 模块完成。字段和失败边界见[插件 API](plugin-api.md#会话目标短命令与终端)，客户端发现与歧义策略归 Agent MCP。

## 数据与平台适配

应用数据目录独立于安装目录，持有实例锁。核心保存插件登记、偏好、消息、服务器资料、主机密钥、组件设置、布局和更新缓存；插件数据放入分配给自身的 `plugin-data/<id>`。当前凭据保存策略见[连接](features/connections.md)。

关键配置使用临时文件、同步写入、替换或备份；损坏及未知版本文件保留，不能当成空配置覆盖。布局只保存容器结构与偏好，不保存活动连接、View ID、正文或命令；终端恢复目录不构成重启后档案。

本地路径使用原生 PathBuf/URL，远端路径按远端文件系统语义。Windows/Linux/macOS 的文件剪贴板、对话框、默认应用、进程组和窗口差异集中在核心适配层。核心启动不依赖开发机 shell 或业务插件。

菜单定位使用已有 Floating UI，Base64/版本/URL/系统打开复用已选标准实现。具体依赖版本以清单和锁文件为准，历史选型记录不作为当前依赖事实。

核心资料的日期换算和 RFC 3339 格式化使用 Rust `time`；宿主界面的日期显示及本地时间输入转换使用 `date-fns`，统一入口为 [datetime.ts](../src/datetime.ts)。超时与耗时计量继续使用标准运行时的时钟和定时器。

## 构建与维护

核心、共享客户端和插件各自保留清单、锁文件与适用工具链。核心包只依赖核心运行资源；业务 UI、字体和后端进入各自插件包。宿主和原生交互夹具位于 tests/scripts，仅测试使用，不发行。

开发与五平台原生验证入口见[开发指南](development.md)，签名与独立分发见[分发文档](distribution.md)。功能变化同步对应正式说明，未落实的结构调整保留临时计划，已完成设计不另建实施流水。

返回[文档索引](README.md)。

Docker 端口联动复用同一命令通道，网络隧道处理普通 TCP ensure／reveal，当前事实通过 sharedState.v1 发布。监听属于网络隧道后台 manager，后台初始化不显示侧栏或启动保存规则。网络插件内部 Service 是手动与协作请求的唯一变更入口，runtime 驱动网络资源，publication 只发布投影；配置磁盘写入与 RPC 等待均不持有业务状态锁。同目标去重由网络插件办理，核心继续只保证原请求去重和真实 Context／资源归属。业务契约见[插件 API](plugin-api.md#网络隧道协作契约)。

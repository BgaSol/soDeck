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
| 业务插件 | [terminal](../plugins/terminal/src/main.rs)、[files](../plugins/files/src/main.rs)、[network-proxy](../plugins/network-proxy/src/main.rs)、[system-status](../plugins/system-status/src/main.rs)、[docker](../plugins/docker/src/main.rs) |

共享 [plugin-client](../crates/plugin-client/src/lib.rs) 提供 RPC、命令票据、设置、共享状态和文件编辑生命周期客户端，不保存连接凭据，也不封装插件之间的直接调用。文件编辑的标准 UI 在 [packages/file-editor](../packages/file-editor/)，随使用组件独立打包；[接入与职责](features/file-editor.md)由通用编辑功能说明维护。

## 宿主状态与 DOM 边界

Shell 拥有当前管理页/工作台导航意图及应用快照、网络计量；连接服务只接纳真实会话，显示定位由发起窗口的有效意图明确请求。管理台不是伪工作区，核心页草稿继续属于原页面控制器。Workspaces 管理工作区操作，Components 独占 Surface 挂载、消息身份、有效可见性和输入权；壳隐藏宿主时只取消显示手势与焦点意图，不销毁 View 或远端资源。

公共 CSS、主题值、主题 ID 登记及已发行 token 名称统一维护在 [packages/ui](../packages/ui/README.md)。核心直接编译共享源码，插件独立打包自身 CSS 副本并沿用 UI v1/v2 的 token 下发；宿主尺寸限定在核心容器内，不改变旧插件几何语义。旧插件选择器兼容仅进入插件包的 legacy.css，核心不导入。核心不导入插件业务 UI，不按具体插件 ID 分配入口或业务行为。

页面控制器拥有草稿、基线修订与保存结果，核心快照只提供已保存状态。设置导航与当前页分别更新；组件设置独占自己的挂载容器，父级刷新不遍历或搬回其内部节点。切页/关闭设置只结束显示，草稿随宿主保留；退出规则见[生命周期](features/lifecycle.md)。后台读取分区接收，失败保留上次有效状态并展示对应错误，失效请求不能更新新实体。

[DOM 协调](../src/dom-update.ts)固定使用 morphdom 2.7.8（MIT）。它替代核心与网络隧道的递归 diff；页面声明实体 key，控件 key 限定在所属实体内。稳定表单持续输入不重新挂载，显式保存/放弃仍按模型更新控件。按压队列属于具体容器，click/cancel/失焦后提交最新投影，销毁时结束监听与待绘制任务。iframe、Monaco、xterm 由各自控制器独立挂载，不交给通用协调器。插件通过既有构建入口携带运行资源与许可证，不从核心安装目录加载它。

选型理由是提供成熟的 keyed 节点协调、SVG 与控件属性处理，并删除重复算法；替代方案是控制器逐项更新固定节点，本轮不引入前端框架。上游为 patrick-steele-idem/morphdom，固定版于 2026-01 发布，升级前复验本项目输入、分屏和按压回归；不以依赖替代生命周期设计。

## 连接与资源归属

核心掌握认证资料与真实连接。插件收到包含服务器、工作区、连接和 generation 的固定 Context；创建和提交资源时核对插件进程实例、逻辑 view 与 Context。重连产生新代次，旧请求不能写入新会话。

Owner/Context、任务结束和 UI 输入权是不同约束：

- [resource_owner.rs](../src-tauri/src/resource_owner.rs) 校验资源调用者及当前接纳状态。
- [resource_tasks.rs](../src-tauri/src/resource_tasks.rs) 跟踪实际在途 worker，取消信号不等于结束。
- 插件进程由核心监督；调用超时后仍可能等待真实回复或进程终止，未知副作用不自动重放。
- 单页资源结束不关闭共享连接；全工作区/应用关闭按范围清理连接。远端命令已有副作用不承诺回滚。

注册表临界区只做短状态操作，跨进程等待、网络和磁盘 IO 不持有状态锁。关闭保留与 view/resource 登记共享协调状态，防止检查与提交之间重新接纳业务。具体终点和失败规则见[生命周期](features/lifecycle.md)。

## 工作台与插件 UI

Workspace 表示服务器会话上下文，Host 表示某个窗口中的显示部分，Group 表示分屏标签组，View 是插件逻辑实例，Surface 是某次页面挂载及输入身份。View 迁移时可以有只读候选 Surface，但只允许有效激活的表面输入。

同窗隐藏/排序/分屏保留 iframe，跨窗按准备、flush、排空、恢复、提交交接；失败取消保留源实例。后台逻辑 view 的运行不依赖 iframe 是否存在。纯快照不创建、销毁或恢复业务资源，初始化由明确工作区和插件状态变化触发。

宿主通过本地资源协议加载插件 HTML/JS/CSS，经 postMessage 和核心数据桥交互。业务 UI 不直接持有服务器认证。UI v1/v2、控制帧、数据分片和兼容策略统一维护在[插件 API](plugin-api.md)。

## 组件协作

共享状态表达当前事实，命令表达一次执行意图。两者复用进程 RPC、真实实例和 Context，分别管理修订订阅与命令回执。命令不借用状态订阅的重试，不做广播执行或自动转投。

命令登记表放在 workbench::State，与工作区、布局、关闭保留共用短状态锁。接纳先固定处理实例、来源 Host/Group、指纹和预算，释放锁后加入现有 RPC 队列；commands.submit 不等待另一插件完成。页面登记与命令关联原子提交。回执的逻辑终态和实际 RPC 任务守卫分开，关闭覆盖没有页面的命令，失效调用仍占在途额度直到真实结束。

核心“＋”、默认创建、终端重开和 Docker 进入容器都提交 terminal.open.v1。处理者属于终端进程，不依赖终端 iframe；终端负责创建参数，Docker 负责 socket、Engine、容器身份与 shell。已登记终端归自身 view，来源页的生命周期不收养它。旧 ui.actions 作为同一服务的兼容适配，不保留独立行为执行表。完整字段、限额和失败契约见[命令 API](plugin-api.md#命令默认行为与进程主动创建页面)。

## 核心能力与业务边界

- 核心 PTY/流服务传输原始字节、管理恢复水位与额度；终端插件负责 shell 目录集成、xterm、搜索和颜色。
- 核心文件服务负责资源身份、受限本地引用、不可变草稿快照、传输和提交结果；正文内存锁与 SFTP IO 分离。文件插件负责目录 UI、批次编排、冲突交互及编辑器，由核心修订和提交回执投影 dirty/busy，不以通知成功推断保存成功。
- 核心网络服务提供受管 TCP 监听与通道，按实际任务退出确认清理完成；SOCKS5 入口协商和规则管理在网络插件。插件分开保存运行事实、查询错误和停止意图，RPC 等待不持有整个插件状态锁。
- 核心短命令提供启动票据、输出分页、取消和期限；系统状态解释计数和指标，Docker 解释 Engine、容器及命令结果。
- 目录/容量共享由核心控制作用域和版本；终端/状态插件发布，文件插件按自身规则消费。核心不为每对插件增加专用调用关系。
- 日志与消息分别接入，来源由核心附加，通知由消息参数明确选择。

分进程不等于沙箱隔离。插件按可信扩展运行，无权限清单或审批；API 版本、参数、资源身份及操作系统自身权限继续校验。

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

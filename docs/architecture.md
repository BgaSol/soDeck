# SoDeck 当前架构

核心使用 Tauri/Rust，宿主界面使用 TypeScript、原生 DOM 和 Vite；插件以独立进程及独立本地 UI 资源运行。架构围绕连接、资源归属和显示交接管理通用能力，具体操作规则由功能文档定义。

## 分层与代码入口

| 层 | 职责与主要入口 |
| --- | --- |
| 桌面适配 | 原生窗口、系统事件、Tauri 命令及应用收尾：[main.rs](../src-tauri/src/main.rs)、[windows.rs](../src-tauri/src/windows.rs) |
| 宿主界面 | 服务器、工作台、设置、更新、消息及插件 Surface：[main.ts](../src/main.ts)、[components.ts](../src/components.ts) |
| 核心状态 | 服务组合、持久配置、受管实例和事件：[core.rs](../src-tauri/src/core.rs) |
| 连接服务 | 资料快照、认证、主机密钥、SSH/SOCKS5 混合链、取消与代次：[connections](../src-tauri/src/connections/mod.rs) |
| 工作台与生命周期 | 工作区/Host/Group/View/Surface、布局、迁移、关闭与资源接纳：[workbench.rs](../src-tauri/src/workbench.rs)、[component_ui.rs](../src-tauri/src/component_ui.rs)、[lifecycle.rs](../src-tauri/src/lifecycle.rs) |
| 通用会话能力 | PTY、短命令、SFTP 文件、流式正文、TCP 通道与计量：[terminal.rs](../src-tauri/src/terminal.rs)、[session_exec.rs](../src-tauri/src/session_exec.rs)、[remote_files.rs](../src-tauri/src/remote_files.rs)、[network](../src-tauri/src/network/mod.rs) |
| 插件与 RPC | 清单、子进程、控制分派、停止和错误：[plugins](../src-tauri/src/plugins/mod.rs) |
| 共享状态与设置 | 核心管理作用域、修订、订阅、冲突和持久化，插件解释业务含义：[shared_state.rs](../src-tauri/src/shared_state.rs)、[settings.rs](../src-tauri/src/settings.rs) |
| 更新和系统适配 | 下载、官方验签、缓存、默认应用与本地文件：[updates.rs](../src-tauri/src/updates.rs)、[platform.rs](../src-tauri/src/platform.rs)、[desktop_files.rs](../src-tauri/src/desktop_files.rs) |
| 业务插件 | [terminal](../plugins/terminal/src/main.rs)、[files](../plugins/files/src/main.rs)、[network-proxy](../plugins/network-proxy/src/main.rs)、[system-status](../plugins/system-status/src/main.rs)、[docker](../plugins/docker/src/main.rs) |

共享 [plugin-client](../crates/plugin-client/src/lib.rs) 提供 RPC、设置和共享状态客户端，不保存连接凭据，也不封装插件之间的直接调用。

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

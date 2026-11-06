# SoDeck 架构草案

官方插件入口由核心的 `official_plugins.rs` 维护小型固定来源列表；安装复用更新模块的 Release 发现、下载、解包与持久登记，不在核心包中加入插件程序。已安装记录始终走原有详情与升级流程，首次安装只在成功登记后产生待重启状态。
后端方案见 [远程工作台设计](plan-remote-workbench.md)：核心统一执行 SSH/SOCKS5 混合路径、管理最终会话、PTY/文件原语、流/任务及网络计量；终端与文件业务分别在独立插件中实现。当前分别报告最外层连接传输和插件有效载荷；尚无可靠估算分摊，明确标为不可归属。

具体代码落点、现有模块的保留/拆分、资源与锁边界、异步 RPC 和分批迁移见 [代码整改方案](plan-code-reorganization.md)。第一批插件控制通信与第二批连接服务已进入源码，真实插件业务继续按批实施。

网络隧道按 [网络隧道设计](plan-network-proxy.md) 实现：核心 `network/` 提供通用 TCP 监听、远端流、原生双向复制、取消和计量，`plugins/network-proxy` 独立进程提供固定转发、SOCKS5 协商、规则持久化和业务界面。`resource_owner.rs` 统一复用现有资源归属校验；通用 dialog 容器通过原有视图挂载和关闭协调。正文不经过 JSON RPC，目标域名交给最终 SSH 服务器解析。

当前实现 Tauri 桌面壳、原生 DOM/TypeScript 前端、外置进程插件、日志、重要消息、主题存储、独立在线更新和本机服务器/代理目录。源码已实现 SSH/SOCKS5 混合连接、PTY、SFTP 文件原语、有界视图数据桥及网络统计；终端和文件分别由独立插件提供。核心升级复用 Tauri 官方 updater；不引入 React 或数据库。当前范围、依赖和验证边界见 [真实插件实施记录](implementation-remote-plugins.md)。

当前代码职责：`main.rs` 组合窗口、命令桥和有界退出；`core.rs` 持有真实状态；`plugins/manifest.rs` 负责清单，`plugins/process.rs` 监督进程，`plugins/rpc.rs` 处理有界控制帧/读写，`plugins/dispatch.rs` 有界分派插件对核心的调用，`plugins/mod.rs` 保留内部入口；`platform.rs` 管理 Windows Job/Unix 进程组；`storage.rs` 保存配置、消息和轮转日志。前端 `main.ts` 管理窗口与浮层，`views.ts` 展示核心数据，`core.ts` 统一调用边界。界面不直接启动插件，不另存运行状态。

控制通信整改已在 v0.5.0 发行：双向请求按 ID 匹配，处理不阻塞接收循环，停止优先，等待和队列有界；进程退出结束在途请求、作废排队工作。回归用真实插件进程复现并修复嵌套请求超时。v0.6.0 在此基础上增加每视图 Tauri Channel 正文桥及真实业务插件，控制通信的边界保持不变。

`servers.rs` 已实现服务器与 SOCKS5 代理目录管理，以及两者混合的有序多层路径，目录模块不自行建连，见 [服务器管理方案](plan-server-management.md)。用户已明确选择密码与私钥口令直接在本机文件保存、不加密，不接系统凭据库；下一段地址固定由当前中转逐站解析。目录使用单实例锁、修订检查、串行提交和上一份有效备份。磁盘认证与界面投影分开；`server-api.ts`、`server-views.ts`、`servers.ts` 分别维护桌面调用、展示和交互草稿。桌面退出与更新重启先询问未保存表单。

## 分层

[服务器 Tab 与可拆合工作区](plan-server-workspaces.md) 已实现到 v0.4.0。`workspaces.rs` 管理内存窗口、工作区、固定 Context 和布局；`windows.rs` 管理原生窗口、先准备后提交的移动和屏幕物理坐标命中；`workspaces.ts` 渲染窗口投影、手势和组件空槽。没有新增生产依赖、持久布局、数据库或 Dock 框架。

只有核心连接服务可交入 `ConnectedSession`；没有前端“创建已连接工作区”命令。正常连接由 connections 服务完成资料快照、逐站身份验证、认证和取消；成功接纳时短锁核对配置身份及尝试 token。连接计数、私钥边界与原生验证见 [连接实施记录](implementation-connections.md)。测试仍可由独立 fixture 或显式 `--smoke-test --test-data-dir` 入口使用隔离档案注入会话。服务器档案修改只更新运行工作区的名称，endpoint 和 connectionId 保留；删除打开档案被拒绝。断线保留工作区，重连递增 generation，旧 Context 被拒绝。

主窗口固定保留服务器管理；独立窗口不复制管理表单。所有全局退出、重启和升级操作集中到主窗口，检查未保存目录表单和组件草稿。组件可声明 flush，同步输入后再移窗或关闭；文件保存完成后才释放资源。移走最后一个 Tab 回收空子窗口，关闭子窗口只关闭其工作区，主窗口退出清理整个应用。Wayland 缺少可靠全局坐标时拒绝猜测落点并提供菜单移动。

一级 Tab 的服务器选择层读取既有目录，连接与认证统一交给 `ConnectionsUi`。连接成功接纳新工作区时使用发起窗口；重连保留已有归属。二级加号集中指定官方终端的稳定 ID，复用组件 open 返回的 viewId 定位新实例，就绪且可见后交接焦点；请求期间切换工作区则不抢回焦点。没有新增偏好字段、生产依赖或插件协议。范围与验证见 [Tab 新建实施记录](implementation-tab-creation.md)。

```text
桌面界面
  ├─ 核心工作台：连接列表、会话、插件管理、日志、消息中心
  └─ 插件视图：终端、文件、Docker、NetBird

核心进程
  ├─ Plugin Manager       插件发现、启动、停止、版本和运行状态
  ├─ Connection Manager   SSH 连接、会话复用、重连和状态
  ├─ Session Services     exec、PTY、SFTP、流式输出
  ├─ Network Services     local forward、remote forward、SOCKS5
  ├─ Server Catalog       本机服务器与代理目录、分组、混合中转路径与认证资料
  ├─ Log Service          插件日志、生命周期日志、检索和轮转
  ├─ Message Service      重要消息发布、历史和界面通知
  └─ RPC Gateway          面向插件的版本化能力接口

插件进程
  ├─ terminal
  ├─ files
  ├─ docker
  └─ netbird
```

## 插件模型

每个插件包含清单和独立进程。当前采用逐行 UTF-8 JSON-RPC 2.0 stdin/stdout，协议见 [插件 API v1](plugin-api.md)。可选 UI v1 中，`component_ui.rs` 定义声明与数据，`component_ui/layout.rs` 负责登记/布局，`host.rs` 负责资源/挂载/RPC，`close.rs` 负责汇总关闭。前端 `components.ts` 保留 iframe，`workspaces.ts` 继续管理服务器级页签与手势。当前业务插件为 `plugins/terminal`、`plugins/files` 和 `plugins/network-proxy`，不在核心根据插件 ID 硬编码视图。旧 Hello 和组件交互示例已退役；宿主布局与草稿行为由仅测试夹具覆盖。详见 [组件与核心 UI 交互设计](plan-component-ui.md)。

插件按可信扩展运行，不实现插件权限清单、授权审批或沙箱。进程分离用于生命周期管理和故障恢复，插件仍可能直接使用操作系统允许的本机和网络能力。核心 API 保留版本、参数有效性和资源状态检查。SSH 认证不属于插件权限系统，仍由连接服务完成。

清单至少包含以下字段。下面是 Windows x86_64 插件目录的示例；同一插件跨系统与架构保持相同的 ID、版本和 API，原生入口按目标设置 `target` 和 `entry`：

```json
{
  "id": "sodeck.files",
  "name": "文件插件",
  "version": "0.1.0",
  "apiVersion": "1",
  "entry": "bin/sodeck-files.exe",
  "target": "x86_64-pc-windows-msvc"
}
```

核心通过稳定接口提供可复用服务，返回会话或资源句柄，避免插件依赖底层实现。句柄用于标识资源和管理生命周期，不作为插件权限凭证。目前实现 logging.write、messages.publish、UI v1、terminal.open/close、files.request 及 metrics.networkSnapshot，详见 [API](plugin-api.md)。以下是原始 MVP 的能力规划名称；实际 PTY/SFTP 使用上述 API，exec/forward/socks 仍待实现：

- `connection.exec`：执行非交互命令并返回流式输出。
- `connection.pty`：创建和管理交互式终端。
- `connection.sftp`：执行远程文件操作。
- `network.forward`：创建和关闭端口转发。
- `network.socks`：创建和关闭 SOCKS5 代理。
- `logging.write`：写入用于排查问题的结构化日志。
- `messages.publish`：发布面向用户的重要消息。

## 插件日志与重要消息

### 日志

插件通过 `logging.write` 上报 `level`、`message` 和可选的结构化字段。级别包括 `debug`、`info`、`warn`、`error`。核心从插件连接上下文附加 `pluginId`、`instanceId` 和 `receivedAt`；可选的 `connectionId` 和 `taskId` 用于关联远程连接或任务。

核心自行记录插件启动、正常退出、异常退出、超时及核心服务的状态变化。插件还未初始化成功就退出时，核心仍需保留已捕获的启动错误和退出码。

如果 RPC 使用标准输入输出，标准输出只承载协议帧，标准错误用于捕获普通诊断日志；SDK 的日志接口发送结构化协议消息。不得向承载 RPC 的标准输出随意打印文本。

日志在核心本地保存，提供按插件、实例、级别和时间筛选的界面。实现时设置单条大小、缓冲区和存储上限，执行日志轮转；发生丢弃时记录数量，避免插件大量输出拖垮核心。密码、私钥和令牌不写入日志。

### 重要消息

插件通过 `messages.publish` 提交 `title`、`body`、`level`（`info`、`success`、`warning`、`error`），以及可选的 `connectionId`、`taskId`、`dedupeKey`。核心自动附加消息来源、时间和消息 ID，写入本地历史后返回消息 ID。

核心提供统一消息中心，负责未读状态、来源展示和打开相关插件的入口。需要即时提醒的消息在应用内显示提示；同一插件相同 `dedupeKey` 的重复消息合并并累计次数。消息历史设置保留数量或期限。

普通日志不会自动生成通知，发布重要消息也不代表必须弹出系统通知。进度高频变化先留在插件界面，完成、失败或需要用户处理的结果再发布消息。第一版不引入独立消息中间件或跨插件工作流系统。

## 网络能力边界

核心网络服务负责连接生命周期、绑定地址、转发流量、停止资源和运行日志。插件优先复用这些服务；核心不实现拦截插件自行联网的权限机制。

默认行为：

- 本地监听默认使用回环地址，可通过参数选择其他绑定地址。
- 每个通道显示目标服务器、监听地址和开始时间。
- 关闭服务器连接时自动关闭依赖它的通道。
- 代理和转发失败必须返回可理解的原因，并释放端口和进程资源。
- 核心记录通道建立、停止和异常，并对意外断开发布重要消息，不增加额外授权步骤。
- 插件创建的 PTY、转发和代理等资源关联到插件实例，插件停止或异常退出后由核心回收；共享 SSH 连接按引用情况释放。

## 数据和稳定性

- 核心保存连接元数据、插件状态、消息历史和日志索引；日志可使用独立的轮转文件。插件数据使用独立命名空间。
- 服务器与代理管理按用户选择，将记住的 SSH/代理密码及私钥口令直接保存在本机 `servers.json`，不加密、不接系统凭据库。私钥只保存用户选定文件的路径。核心区分磁盘数据和界面只读数据，秘密不进入公开快照、事件和日志；未记住的秘密只在当前进程内存有效。具体结构及备份语义见服务器管理方案。
- 连接补填的凭据由核心临时持有，整条路径认证成功并校验原资料及工作台仍有效后，通过目录既有提交方法一次保存勾选记住的各节点凭据。补填默认记住，取消勾选仅本次尝试有效；失败、取消与并发凭据修改不能覆盖原值。此流程不新增插件接口或凭据存储格式。
- 核心捕获插件退出、超时及通信断开，回收其资源并保留日志，支持用户重启。分进程降低故障影响，但不保证插件耗尽系统资源时完全不影响其他功能。
- RPC、插件清单和核心能力都必须带版本号，兼容性检查在插件启动前完成。

## 跨平台兼容与系统适配

第一版的客户端构建目标如下，和被管理服务器的操作系统分别定义：

| 客户端 | Rust target | 插件入口示例 |
| --- | --- | --- |
| Windows x86_64 | `x86_64-pc-windows-msvc` | `bin/sodeck-files.exe` |
| Windows ARM64 | `aarch64-pc-windows-msvc` | `bin/sodeck-files.exe` |
| Linux x86_64 | `x86_64-unknown-linux-gnu` | `bin/sodeck-files` |
| Linux ARM64 | `aarch64-unknown-linux-gnu` | `bin/sodeck-files` |
| Apple Silicon macOS ARM64 | `aarch64-apple-darwin` | `bin/sodeck-files` |

核心的业务接口保持一致，路径、应用数据目录、进程启动和进程树清理由系统适配层处理。使用参数数组启动程序，不让统一流程依赖 PowerShell、cmd 或特定 shell；远程服务器路径和本地路径使用各自的规则。从桌面入口启动时，核心按明确目录找到自己的资源和用户选择的外置插件，不依赖终端初始化脚本里的 PATH。

插件根据自己的实现生成适用系统与架构的文件，独立于核心分发；Linux 和 macOS 的可执行入口需具有执行位。核心只检查清单可读、入口存在、API 主版本匹配，以及原生插件目标与本机匹配，不建立额外授权或签名验证流程。

五个目标分别构建，平台运行结果如实记录。日常验证随改动选择关键路径，不要求每次打包重跑全平台所有插件功能；未验证的运行行为不得描述为已验证。

文件树由文件插件管理排序、展开、选择、路径定位及导航恢复。核心只增加向后兼容的独立目录分页游标，每个资源最多 32 个未完成游标，结束或资源销毁时释放。通过 `files.directoryCursors.v1` 发现能力，避免新插件静默依赖旧核心没有的行为；详见 [文件树设计](plan-file-tree.md) 和 [插件接口](plugin-api.md)。

## 构建与分发边界

核心通用剪贴板适配使用 Tauri 官方 clipboard-manager，在阻塞任务中执行原生文本读写；插件 UI 经有界分块桥复用，文本不进入日志或持久化。终端快捷键、选区和粘贴目标策略仍由终端插件负责。主题色板集中在 `src/themes.css`，共享外观规则在 `src/ui.css`，以 UI v1 增量 tokens 传入独立打包的插件；组件保留布局。实现与兼容说明见 [工作台实施记录](implementation-workbench-design.md)。

核心、插件 SDK 和业务插件可以放在同一仓库，保持依赖边界；插件版本和打包不绑定核心。核心构建与安装不依赖任何业务插件，插件也可以使用独立仓库。

核心安装包只包含桌面壳、核心程序、核心界面和自身运行资源，不带业务插件。插件、配置、消息和日志放在程序安装目录以外；核心覆盖升级不清空这些数据。没有插件时展示工作台，插件管理位于右上角设置内。

插件目录包含清单、入口、依赖和可选界面资源，普通 ZIP/tar.gz 用于下载。启用配置与本次实际进程状态分开，启停在重启后统一生效。`updates.rs` 读取各组件的 GitHub Release、下载并校验插件包；新版解压到唯一目录，保存 pendingUpdate，启动先切换再加载。新版初始化失败时恢复 previous 程序记录；数据独立保存，不做数据迁移回滚。`desktop_updates.rs` 负责 Tauri 核心检查、签名下载与安装；`main.rs` 统一先停止插件再重启/安装。详细流程见 [`distribution.md`](distribution.md)。

## 第一条实现路线

1. 建立可独立安装与覆盖升级的核心、最小 SDK 和一个外置插件，配套标准 GitHub 打包；先验证核心启动、插件加载/卸载、日志、重要消息与资源释放，不先建立复杂发布框架。
2. 定义并验证 SSH 连接服务的 `exec`、PTY、SFTP 和网络能力接口。
3. 接入真实 SSH，实现终端插件和文件插件。
4. 实现本地端口转发和 SOCKS5，并补上资源清理、日志和重要消息的验证。
5. 用同一套插件接口实现 Docker 和 NetBird。

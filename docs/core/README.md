# SoDeck 核心能力

更新日期：2026-10-29 · 修订：r44 · 状态：职责基线；各任务分别维护确认、实现与验收。

[F-CORE-033：GitHub发布与在线升级](online-updates.md)定义自动检查提示、用户下载安装、核心/官方插件独立升级和兼容/退出边界；[代码方案](../technical/online-updates-implementation.md)与[T-029](../tasks/T-029-online-updates.md)为本次待确认来源，不改变T-028已交付事实。

[F-CORE-032：插件完整性、签名状态与启停管理](plugin-management.md)定义设置统一入口、完整重启启停、严格包检查、第三方无签名加载；确认与交付见[T-028](../tasks/T-028-plugin-management.md)，实际验证见[结果](../tasks/T-028-results.md)。

T-027最新代码方案见[代码r1](../technical/mixed-route-implementation.md)/[功能r2](mixed-route.md)：API6/core v7只保留新模型，单一步骤表、通用SSH/SOCKS执行、凭据binding和全链预算；四插件按各自计划修改，MR/MR-CODE及插件测试分别维护。只完成方案，r1兼容建议由用户明确取消，当前未实施。

混合多级连接见[F-CORE-031 r1](mixed-route.md) / [T-027](../tasks/T-027-mixed-route.md)：核心统一组合SSH与SOCKS5有序节点，外壳展示每一步的访问来源，插件继续消费最终目标连接；提出回环/DNS/身份/预算及MR专属测试。当前为功能设计，具体代码方案与实施确认后续记录。

桌面交互统一规则见[F-CORE-030 r1](desktop-interaction.md) / [T-026](../tasks/T-026-desktop-interaction.md)：核心宿主统一适配Tauri与raw Wry的默认菜单、浏览器快捷行为、导航及调试方式，保留业务/文本编辑与系统交互。包含实现设计和DI测试，不新增业务插件、公开API或数据库结构；确认与进度只由任务维护。

连接入口重构见[T-025 r4](../tasks/T-025-connection-home.md) / [F-CORE-029 r2](connection-home.md)及[代码方案r1](../technical/connection-home-implementation.md)。系统页、草稿、代理回执/分页、凭据条件写和两阶段关闭已实现，API5.3、业务库v6保持。核心持有目录/凭据/工作区，外壳持有草稿；[结果](../tasks/T-025-results.md)记录验证与目录包，用户验收待反馈。

T-016本轮[代码方案r1](../technical/observability-implementation.md)固定核心事件/关联、v2历史补读、现场/指标/证据与三插件接入的模块、API4.10、观测schema2、预算和验证关口。[接入需求r2](observability/integration.md)、F-CORE-018/019 r3、021/022 r5维护行为/测试；F-CORE-020问题服务单独后续。仅观测历史四分区备份重建，同版本业务库/凭据保留。等待最终确认，状态见[T-016 r5](../tasks/T-016-observability.md)。

通信监控四项能力的[代码修改方案r1](../technical/communication-monitor-implementation.md)已明确采集、公共观测子集、接口/数据版本与验证关口，确认范围由[T-019 r3](../tasks/T-019-connection-metrics.md)维护；当前尚未实施。

本文件描述核心提供什么、与业务插件的边界和通用资源规则。具体技术实现见[技术方案](../technical/README.md)。能力目录不等于首发功能清单，也不代表所有协议能力都已确定。

P-53新增[统一事件与诊断能力设计](observability.md)，包括事件/操作链、历史/补读、重要问题、现场/指标/证据包、留存/健康。核心提供公共服务，插件报告自己的业务事实，未来AI通过相同公开接口读证据；不开放事件驱动的插件命令互调。本稿为[T-016 r1](../tasks/T-016-observability.md)能力设计，尚未实施。

T-015最新数据规则：按P-52，数据库结构版本不一致时弹窗提示、备份旧库及关联凭据并创建全新库，不做旧库迁移；同版本配置保留。有效行为与START-DB测试见[启动r5](startup-settings.md)，[全新启动r3](clean-start.md)补充保留前提，[服务器管理r3](server-management.md)沿用此规则；最终实施确认由[T-015 r3](../tasks/T-015-workflow-polish.md)记录。

本轮先读[程序核心设计方案](design.md)，了解十二组能力、状态所有者和验证顺序，再按需进入[调用与状态通信](communication.md)、[资源与生命周期](resources.md)和[工作台、插件视图与窗口](workbench.md)。本轮提问、检查和用户反馈集中在 [T-002](../tasks/T-002-core-design.md)。

首批实施从 [T-003：核心与真实终端](../tasks/T-003-core-terminal.md)进入；本批连接功能见 [F-CORE-005](ssh.md)，公共规格只实现任务选取的部分。

本轮 [T-004](../tasks/T-004-server-tabs.md) 根据 P-20、P-21 设计顶部服务器 Tab 与完整工作区拆合，规格和测试见 [F-CORE-006](server-tabs.md)。窗口承载、活动工作区与页签归属由核心管理；实际实现和验证状态以任务为准。

后续 [T-005](../tasks/T-005-startup-settings.md) 针对启动脚本反馈提出直接启动与 App 设置入口，参数归属和功能测试见 [F-CORE-007](startup-settings.md)。沿用既有架构，启动路径由核心持有，组件通过公开接口呈现；本次是待确认设计。

最新 [T-006](../tasks/T-006-workbench-interaction.md) 针对视觉、列表、Tab、输入、分屏及菜单反馈提出 [F-CORE-008](workbench-interaction.md)。本次取消终端列表/分屏，拟收敛为单中央组和独立窗口；公共停靠与窗口职责不变。具体输入属于终端 F-TERM-002，不能因共享菜单将业务移入核心。

按产品 P-14，插件平等调用全部公开且已支持的核心能力，不建立插件能力/服务器授权。每次调用由核心统一留痕，细则及测试见[插件核心调用记录](call-records.md)。身份、参数和资源有效性检查用于归因与运行正确性。

[T-008](../tasks/T-008-server-file-tree.md) 为目录树实现 [F-CORE-009](remote-read.md) 公共 SFTP/有界 exec 及通用多插件承载；目录与挂载解释归[文件插件](../plugins/files/README.md)。当前 dev.5 / API 4.1 的实测和剩余验证见 [T-008 结果](../tasks/T-008-results.md)，不改写旧任务的已验证范围。

[T-010](../tasks/T-010-terminal-directory-follow.md) 已实现 [F-CORE-011](directory-context.md)：核心管理工作区目录来源、状态和代次，终端报告实际 shell 目录、文件消费公开上下文。目录来源与键盘焦点分开，不开放插件互调；实测见[任务结果](../tasks/T-010-results.md)。

T-010 的[通用通信策略](../technical/component-communication.md)与[代码规划](../technical/terminal-directory-follow-implementation.md)已经实施目录/视图呈现共用的状态订阅、公共 PTY 观察与 API 4.2 兼容；测试规格在 F-CORE-001 r5/F-CORE-011 r2，实际结果只在任务记录。

## 1. 能力与边界

[T-014 r7](../tasks/T-014-network-proxy.md)提交[F-CORE-015 r6](dynamic-proxy.md)及[代码方案r1](../technical/network-tunnel-implementation.md)：核心复用T-013路径/身份，新增通用监听/目标TCP、独立网络池、二进制流与splice；[插件](../plugins/proxy/README.md)持有固定转发/HTTP/SOCKS业务。公开契约和实现已建立，预算保持；[T-014结果](../tasks/T-014-results.md)记录NT验证层级及实际范围。

[T-013](../tasks/T-013-ssh-jump.md) / [F-CORE-014](ssh-jump.md)将已声明的跳板职责细化为单层 SSH 连接路径提案：核心统一身份、传输、依赖快照、复用及释放，终端/文件仍通过目标租约访问。本任务已获实施确认，具体实现、实测和交付由任务维护。

[T-012 r3](../tasks/T-012-dopamine.md)为[F-CORE-012 §7/8](appearance.md#7-t-012-第三主题增量方案)提交第三主题、默认迁移、公共组件与DS测试，新增[F-CORE-013](brand.md)品牌资产和专属测试；[静态设计稿r2](../design/dopamine.md)及[代码方案r1](../technical/design-system-implementation.md)供评审，本轮未授权实施。

[T-011](../tasks/T-011-appearance.md) 已获确认并实现 [F-CORE-012](appearance.md) 整体视觉与深浅色切换：核心持有App主题/偏好，外壳和插件通过公开呈现/状态契约消费；App scope及真实窗口订阅身份复用通用通信，验证见[实施结果](../tasks/T-011-results.md)。

本轮 [T-009](../tasks/T-009-clean-start.md) / [F-CORE-010 r2](clean-start.md) 定义全新启动与自动首终端：保留配置/偏好、工作现场仅限本次运行；启动为空，用户创建服务器工作区后自动创建一个终端并连接。核心统一启动及工作区初始化调度/去重/取消，插件管理会话业务；定位/移动不增开。确认与交付状态由任务维护，[API 4 代码方案](../technical/clean-start-implementation.md)及[实测记录](../tasks/T-009-results.md)分别说明契约与验证范围。

### 1.1 核心提供什么

| 核心模块 | 负责的内容 | 明确边界 |
| --- | --- | --- |
| Server Registry | 服务器配置、分组或标签、稳定标识、连接配置修订 | 不存放 Docker 容器、NetBird 节点等业务模型 |
| Identity & Credentials | 密码、私钥引用、Agent、主机指纹与认证提示 | 插件拿到可用的连接句柄，不直接读取凭据 |
| Connection Broker | 直连、跳板和代理路径，连接复用、租约、超时与断线事件 | 工作区连接独立于终端面板；不把单个终端当作所有工具的连接 |
| Remote I/O | 远端命令、标准输入输出、PTY、文件与字节流等通用协议能力 | 不提供 `dockerRestartContainer` 这类业务专用接口 |
| Tunnel Manager | 隧道配置、运行状态、连接路径、转发和恢复策略 | 本地转发、远程转发、动态代理分别声明支持情况，首期不默认全部实现 |
| Port & Endpoint Registry | 本地监听端口、租约、冲突、用途、远端目标和服务入口 | 端口扫描、容器端口解析属于业务插件 |
| Resource & Task Manager | 长任务、进度、取消、资源归属、操作回执与诊断 | 核心记录通用执行状态，业务成功条件由插件解释 |
| Storage & Secrets | 核心数据库、插件命名空间、文件对象和秘密引用 | 插件经公开接口使用；模块私有实现不作为公开 API，命名空间不代表插件权限分配 |
| Workspace & Contributions | 每服务器工作区、侧边停靠、中央 Tab、独立窗口、托盘、焦点、布局及公共界面服务；分屏的最新适用变化见 F-CORE-008 | 插件声明视图及支持位置，提供内容和业务状态；核心统一承载，细则见工作台规格 |
| Plugin Manager | 包验证、版本选择、启停、健康状态和回退 | 插件不能自行替换核心可执行程序 |

SFTP、HTTP 或其他协议能力按实际插件需求引入。把能力放进核心的判断依据是：它是否是可复用的资源或协议原语，以及核心是否必须持有其生命周期；不能仅因旧代码写在 Rust 中就继续留在核心。

### 1.2 对“隧道、端口”的具体解释

这里暂按以下模型理解用户所说的底层能力：

- **连接路径**：如何到达服务器，例如直连或经跳板访问。
- **隧道**：在该路径上建立的转发资源，具有独立的启动、停止和故障状态。
- **监听端口**：由本机或远端实际占用的地址与端口，有明确的持有者和释放规则。
- **服务端点**：某项服务的访问目标，例如远端 `127.0.0.1:8080`；它可以通过隧道映射到本机入口。

例如 Docker 插件发现容器 Web 服务后，向核心注册服务端点，并申请到该端点的本地转发。核心负责分配端口、持有监听 socket、展示映射和释放资源，Docker 插件负责“哪个容器提供了哪个服务”的业务含义。

本机端口分配必须通过实际绑定并持有 socket 完成，避免“先检查空闲、随后绑定”的竞争。远程转发端口以远端绑定结果为准，核心不能承诺占用其他程序或其他客户端管理的端口。首期建议默认绑定回环地址；对外监听的交互和实际系统要求在对应功能中定义，不建立插件权限审批。

## 2. 生命周期与资源归属

核心将以下对象分开管理：

| 对象 | 归属与生命周期 |
| --- | --- |
| 服务器配置 | 核心持久化对象，不依附于任何插件 |
| 工作区 | 核心持有，关联服务器上下文和面板布局 |
| 连接租约 | 由核心根据调用者与用途管理，释放一个租约不直接关闭其他使用者的连接 |
| 插件运行实例 | 使用当前 App 启动时选定的插件版本；业务逻辑与面板显示分别管理，具体进程或运行容器由技术选型决定 |
| 插件视图 | 与渲染表面分离的逻辑实例，可在支持的面板、Tab 和独立窗口之间移动；移动不等于关闭 |
| PTY、流与后台任务 | 核心持有底层资源，记录创建插件、工作区、消费者及取消策略 |
| 用户保存的隧道规则 | 核心持久化；运行中的隧道在本次 App 生命周期内由核心托管 |

资源需要显式区分“随视图关闭”“随工作区结束”“用户管理”的生命周期，不能因为插件注册过资源就允许它永久泄漏。

资源身份与使用关系也要分开：

| 概念 | 用途与失效规则 |
| --- | --- |
| 持久资源标识 | 标识服务器配置、保存的隧道规则等核心对象，可跨 App 重启保留；标识不代表运行资源或租约仍有效 |
| 调用者租约或访问句柄 | 记录某个消费者的资源引用及会话上下文；调用者失效或租约释放后失效，不承担插件能力授权 |
| 连接与会话句柄 | 指向本次运行中的连接、PTY 或流等对象；按对应会话代次失效，不因持久配置仍在而自动恢复有效 |

调用者身份始终明确；服务器、工作区、运行实例和会话代次按资源实际作用域约束。全局设置或插件管理操作不应被迫伪造工作区字段，各接口规格定义自己的必需上下文。

用户保留的核心资源可以比创建它的视图或插件实例存活更久。新实例经核心取得当前有效句柄，无需授权；旧句柄不会因相同插件再次启动而复活。核心重新建立连接也不会复活原 PTY。公开查询、接管和停止接口对插件平等开放，调用及资源何时释放写入相应规格并测试。

- 关闭 Docker 面板不应结束用户明确保留的端口转发。
- 按产品 P-12，关闭终端或插件面板默认结束它创建的会话；用户明确选择保留且完成核心托管时例外。不能由组件卸载偶然决定，也不影响其他消费者仍持有的连接租约。
- 插件崩溃后，核心限制其残留资源和缓冲量，按资源策略取消或保留，并提供全局查看与清理入口。业务恢复只能在结果状态已核对后进行。
- 插件卸载时，明确处理其私有数据和资源引用；用户保存的核心资源不能被静默删除。
- 按[产品 P-11](../product.md)，明确退出 App 结束全部本机持有的连接、监听和 PTY 通道，包括明确保留的运行资源；需要后台工作时保持 App 运行。P-33 的启动规则见 [F-CORE-010](clean-start.md)：保留配置，取消跨启动工作区/视图恢复；同次运行的移动、切换和表面恢复继续保留状态，不等于重建 SSH/PTY。
- 按产品 P-18，主窗口“×”收起到托盘，App 和独立窗口继续运行；移动视图、切换工作区、主窗口隐藏均不触发默认会话关闭。独立窗口与工作区的具体关系见[工作台规格](workbench.md)。

## 3. 单项能力如何细化

每项即将开发的核心功能在本目录建立自己的规格，使用[功能规格模板](../templates/feature.md)，并在下表登记。复杂模块可以再建立子目录，模块总览链接单项功能，不在总览中堆叠所有细节。

| 功能编号 | 功能与范围 | 规格及测试流程 | 任务记录 | 消费者 |
| --- | --- | --- | --- | --- |
| F-CORE-030 | 桌面交互、WebView默认菜单/快捷行为、原生编辑与调试边界 | [规则、实现设计与DI测试](desktop-interaction.md) | [T-026](../tasks/T-026-desktop-interaction.md) | 核心外壳、所有受管窗口及插件表面 |
| F-CORE-029 | 统一启动/＋连接首页、SSH与代理表单、节点选择及草稿 | [功能与CH测试](connection-home.md) | [T-025](../tasks/T-025-connection-home.md) | 核心外壳、目录及工作区；连接消费者保持原边界 |
| F-CORE-028 | 右上角设置窗口、分组/搜索、统一编辑及插件声明接入 | [行为、契约与SET测试r2](settings.md)、[代码方案r1](../technical/settings-implementation.md) | [T-023及实测](../tasks/T-023-results.md) | 外壳、核心设置provider及终端插件 |
| F-CORE-027 | SSH经SOCKS5网络代理、与SSH主机隧道边界分离 | [功能与SK测试](socks-route.md) | [T-020](../tasks/T-020-socks-route.md) | 核心外壳、终端、文件及网络连接消费者 |
| F-CORE-023 | 通信总方案、实时速率/流量、来源与多跳计量 | [功能与CM测试](connection-metrics.md) | [T-019 r3](../tasks/T-019-connection-metrics.md) | 外壳、业务插件及未来诊断消费者；待最终方案确认 |
| F-CORE-024 | 通信历史、比较、导出与留存 | [功能与CH测试](communication-history.md) | [T-019 r3](../tasks/T-019-connection-metrics.md) | 公共观测存储、外壳与证据服务；未实施 |
| F-CORE-025 | 连接诊断、可选SSH响应探测与应用内提醒 | [功能与CQ测试](connection-quality.md) | [T-019 r3](../tasks/T-019-connection-metrics.md) | 连接服务、公共问题与外壳；未实施 |
| F-CORE-026 | 主动上下行测速、取消、方法兼容与结果 | [功能与CSPEED测试](connection-speedtest.md) | [T-019 r3](../tasks/T-019-connection-metrics.md) | 核心诊断任务、历史与外壳；未实施 |
| F-CORE-018 | 统一事件采集、分级日志、用户动作和操作关联 | [模型与EV测试](observability/events.md) | [T-016](../tasks/T-016-observability.md) | 外壳、宿主、核心及全部插件 |
| F-CORE-019 | 历史查询/统计与可补读订阅 | [契约与HQ测试](observability/history.md) | [T-016](../tasks/T-016-observability.md) | 诊断消费者、未来AI |
| F-CORE-020 | 重要问题聚合、确认、恢复与通知意图 | [生命周期与IS测试](observability/issues.md) | [T-016](../tasks/T-016-observability.md) | 外壳和诊断插件 |
| F-CORE-021 | 诊断现场、有限指标和证据包 | [能力与DE测试](observability/evidence.md) | [T-016](../tasks/T-016-observability.md) | 人工排查、未来AI及导出消费者 |
| F-CORE-022 | 观测留存、脱敏、审计兼容和存储健康 | [策略与ST测试](observability/storage.md) | [T-016](../tasks/T-016-observability.md) | 启动、核心存储及诊断服务 |
| F-CORE-016 | 服务器表单与分组管理 | [行为、数据及SM测试](server-management.md) | [T-015设计](../tasks/T-015-workflow-polish.md) | 外壳、服务器目录；插件连接消费者保持兼容 |
| F-CORE-017 | 插件中文名与稳定身份展示 | [元数据及PN测试](plugin-display.md) | [T-015设计](../tasks/T-015-workflow-polish.md) | 插件包、管理器与展示界面 |
| F-CORE-015 | listener/TCP、网络池、资源组、二进制流/splice及多级路径接入（API 4.7） | [契约与NT流程](dynamic-proxy.md) | [实施方案T-014](../tasks/T-014-network-proxy.md) | 网络隧道插件；兼容回归终端/文件 |
| F-CORE-014 | 核心 SSH 跳板、逐节点认证、路径快照与生命周期 | [功能与测试](ssh-jump.md) | [设计方案 T-013](../tasks/T-013-ssh-jump.md) | 核心外壳、终端/文件及其他目标连接消费者 |
| F-CORE-031 | SSH/SOCKS5 混合多级路径、逐段地址上下文与统一编辑器 | [功能与MR测试](mixed-route.md) | [设计方案 T-027](../tasks/T-027-mixed-route.md) | 连接首页、核心连接/身份与全部目标连接消费者 |
| F-CORE-001 | 核心调用、状态事件与有界流的公共契约 | [规格与测试](communication.md) | [设计任务 T-002](../tasks/T-002-core-design.md) | 外壳、插件及核心能力模块 |
| F-CORE-002 | 资源归属、租约、生命周期与清理 | [规格与测试](resources.md) | [设计任务 T-002](../tasks/T-002-core-design.md) | 连接、I/O、隧道、插件运行实例 |
| F-CORE-003 | 无插件权限分配的核心调用记录 | [规格与测试](call-records.md) | [设计任务 T-002](../tasks/T-002-core-design.md) | 全部插件、调用网关及诊断入口 |
| F-CORE-004 | 工作台、插件视图、分屏、独立窗口与托盘 | [规格与测试](workbench.md) | [设计任务 T-002](../tasks/T-002-core-design.md) | 外壳、插件视图、资源协调及原生容器 |
| F-CORE-005 | 服务器、认证、直连 SSH 与 PTY 首条流程 | [规格与测试](ssh.md) | [实施方案 T-003](../tasks/T-003-core-terminal.md) | 核心外壳及终端插件 |
| F-CORE-006 | 无独立标题栏、顶部服务器 Tab 与完整工作区窗口拆合 | [规格与测试](server-tabs.md) | [设计方案 T-004](../tasks/T-004-server-tabs.md) | 原生窗口、核心工作台、视图与会话迁移 |
| F-CORE-007 | 直接启动、数据/资源默认定位与 App 设置入口 | [规格与测试](startup-settings.md) | [设计方案 T-005](../tasks/T-005-startup-settings.md) | 核心启动、外壳设置、插件管理和目录包 |
| F-CORE-008 | 统一视觉、两层导航、Tab/右键关闭、单组布局及兼容迁移 | [规格与测试](workbench-interaction.md) | [设计方案 T-006](../tasks/T-006-workbench-interaction.md) | 核心外壳、公共 UI/原生适配、终端及插件公开契约 |
| F-CORE-009 | 远端目录/属性/有界读取与非 PTY 执行 | [功能语义与测试](remote-read.md) | [设计方案 T-008](../tasks/T-008-server-file-tree.md) | 文件插件；协议资源可供其他插件复用，挂载业务解析留在插件 |
| F-CORE-010 | 全新启动、配置/运行状态分离、旧快照退役及新工作区首终端 | [功能及测试](clean-start.md) | [设计方案 T-009](../tasks/T-009-clean-start.md) | 核心启动、工作台、存储、窗口及插件初始化动作 |
| F-CORE-012 | 整体视觉、浅深主题、全局选择/持久化和实时分发 | [功能及测试](appearance.md) | [方案 T-011](../tasks/T-011-appearance.md) | 工作台、窗口、原生菜单、公开SDK和当前终端/文件插件 |

其余能力的范围与验证准备见[设计方案](design.md)，进入实际任务前再建立各自详细规格；公共设计不等于全部待实现清单。T-003 补充本批选用范围、技术建议与预算，状态及确认只在任务维护。

详细规格必须定义使用场景、输入输出及事件、调用记录和资源归属、状态转换、超时和取消、幂等或结果不明语义、数据与兼容，以及功能自己的测试流程。实际操作系统或远端账号要求单独说明，不增加插件授权。SDK 的通用规则引用[插件协议方案](../technical/plugin-protocol.md)，不重复维护。

能力或接口变化时，先更新本层规格及受影响插件的规格、兼容要求与测试流程，再按[文档更新流程](../documentation-workflow.md)核对用户确认与前置条件后编码。规格就绪与任务进度分别引用来源，索引不重复维护状态。

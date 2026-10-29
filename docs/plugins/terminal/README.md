# 终端插件

更新日期：2026-10-29 · 修订：r20

T-029：[F-TERM-007](features/online-updates.md)定义终端独立发布、手动下载/安装和真实PTY/设置A→B回归；下载/来源/安装服务归核心，当前待整体方案确认。

T-028：[F-TERM-006 r1](features/plugin-management.md)定义终端启停、PTY/设置provider与无签名单包更新回归。启停由核心管理，预期不改终端业务代码；方案确认和结果见[T-028](../../tasks/T-028-plugin-management.md)。

Windows Shell临时修复：[F-TERM-003 r4 / WIN-SHELL-01](features/directory-reporting.md)仅在确认Linux+Bash后启用目录集成；Windows/未知使用普通SSH Shell，不修改全局偏好。终端0.13.1独立包与Windows/Linux原生证据见[T-027结果](../../tasks/T-027-results.md)。

T-027：[插件计划 r4](features/mixed-route-integration.md)保留原混合连接接入及 TERM-MR 结果，新增每终端诊断引用/详情入口、一次组合确认、公共 UI 与 TERM-DIAG-01～03。公共传输只由核心实现，插件独立构建/更新；本轮增量待整体确认，状态见[T-027 r7](../../tasks/T-027-mixed-route.md)。

统一设置接入：[F-TERM-005 r2](features/settings.md)由终端0.11.0声明字号/Shell目录集成，核心统一呈现；[代码方案r1 §10](../../technical/settings-implementation.md#10-终端接入和独立更新验证)规定provider、原子保存、全视图feed/移窗快照。真实SSH与A/B独立更新证据见[T-023结果](../../tasks/T-023-results.md)。

本轮代码接入见[T-016代码方案r1](../../technical/observability-implementation.md)，已固定4.10能力、包目录、上下文、实际完成点与独立更新验证；等待用户最终确认，当前插件代码未改。

本轮诊断接入需求：[F-TERM-004 r2](features/observability.md)细化会话/重连/取消/关闭、目录集成健康、公开快照/指标及TERM-OBS测试；通过[T-016核心公共接口](../../core/observability/integration.md)采集，保留PTY/认证职责。仅设计，未改变当前插件实现。
最新提案：[T-015](../../tasks/T-015-workflow-polish.md)覆盖末行裁切和Tab末尾＋；TERM-VIEW-01～03见[F-TERM-001 r10](features/ssh-terminal.md)，入口/窗口见[F-CORE-008 r6](../../core/workbench-interaction.md)。仅设计，不改写旧任务实现/实测。

插件 ID：`sodeck.terminal` · 当前任务：[T-009](../../tasks/T-009-clean-start.md) · Windows 开发包与验收范围见任务

最新基线：[T-009](../../tasks/T-009-clean-start.md) 的终端包 0.4.0 / API 4，启动和首终端证据见[实测记录](../../tasks/T-009-results.md)。[T-006](../../tasks/T-006-workbench-interaction.md) 的交互收敛按其任务维护。下文首批视图/SDK v1/耦合描述保留 T-003/T-004 设计时基线，按相应后续规格覆盖，不作为当前进度。

## 1. 目标与边界

[T-013](../../tasks/T-013-ssh-jump.md)提出核心跳板连接；终端继续只申请目标租约/PTY，显示当前阶段和失败，通过核心处理认证，不拥有跳板会话。专属消费测试 TERM-JP-01/02 在 [F-TERM-001](features/ssh-terminal.md)。下文首批排除跳板为 T-003 历史范围，本次增量尚待确认。

目录联动已随 [T-010](../../tasks/T-010-terminal-directory-follow.md) 实施为 terminal 0.5.0 / API 4.2：worker 独立观察 PTY 并报告目录，保持 UI 显示 ack。Bash 5.1/5.2 和关闭集成的原登录流程已实测，Zsh 自动跟随后置。结果与开发包见[实测记录](../../tasks/T-010-results.md)，原 0.4.0 包保留，正式发布未发生。

按产品 P-19，作为首个实际业务插件与核心一起交付：用户添加服务器后能打开真实 SSH 终端，操作多个会话，分屏、弹出和移回窗口，并单独更新终端包。Windows 首先验证，远端首批验证 Linux/OpenSSH。

核心拥有服务器档案、认证秘密、主机信任、SSH 连接、PTY、租约、流和窗口布局。插件拥有会话业务模型、终端渲染、键盘/选择/粘贴规则、字号等终端设置。插件通过公开 SDK 工作，不能导入外壳内部组件或直接使用 v1 的全局状态。

首批不迁入文件浏览、SFTP、历史录制/回放、命令库、Docker、NetBird、本地 shell 或跳板链。终端关闭默认结束 PTY；关闭后的显式会话托管另行细化，首批没有“保留会话”操作，收起面板、切换工作区、移窗和托盘运行继续保持会话。此范围是 T-003 的评审建议。

## 2. 功能索引

| 功能 | 规格及自己的测试流程 | 依赖 | 任务 |
| --- | --- | --- | --- |
| F-TERM-005：统一设置接入 | [行为与TERM-SET测试](features/settings.md) | 核心[F-CORE-028](../../core/settings.md)、原插件存储与终端视图 | [T-023](../../tasks/T-023-settings.md) |
| F-TERM-001：真实 SSH 终端与会话交互 | [功能规格](features/ssh-terminal.md)；TERM-01 至 TERM-12 | [核心 SSH](../../core/ssh.md)、工作台、记录及资源契约 | [T-003](../../tasks/T-003-core-terminal.md) |
| F-TERM-002：复制粘贴与本地菜单 | [功能规格](features/terminal-input.md)；IN-01 至 IN-07 | 核心剪贴板/PTY、F-CORE-008 共享 UI | [T-006](../../tasks/T-006-workbench-interaction.md) |
| F-TERM-003：工作目录报告 | [功能设计](features/directory-reporting.md)；DR-01 至 DR-09 | [核心目录上下文](../../core/directory-context.md)、PTY 与 shell 集成；不直接驱动文件树 | [T-010](../../tasks/T-010-terminal-directory-follow.md) |
| F-TERM-004：事件与诊断接入 | [采集规格](features/observability.md)；TERM-OBS-01～06 | 核心事件/context/快照/指标；不录制终端正文 | [T-016](../../tasks/T-016-observability.md) |
| 插件包独立更新与恢复 | [本批更新流程及 UPD 测试](../../technical/first-terminal.md#5-本批独立更新及测试) | 核心插件管理器；插件数据 schema | [T-003](../../tasks/T-003-core-terminal.md) |
| 会话托管、历史、回放等 | 尚未安排；进入相应任务时分别建功能规格 | 按真实需求补齐 | 无 |

## 3. 两种真实业务视图

以下表格保留当前旧基线。T-006 推荐的下一版本声明：退役 `terminal.sessions` 及其默认工作区入口；`terminal.session` 保留每工作区多实例，支持中央 Tab / 独立窗口、默认中央，最小内容区沿用 320×160 CSS px。会话新建在 Tab 行、重命名/重连在右键与异常状态中提供，不能因为删除列表丢失原操作。核心仍管理展示容器，插件仍管理业务；通用退役/位置迁移见 F-CORE-008 第 6 节，实施前补齐版本化契约，不在核心写终端类型特判。

| 视图类型 | 作用域/视图实例数 | 支持位置与默认 | 最小内容区 |
| --- | --- | --- | --- |
| `terminal.sessions`：会话列表 | 每服务器工作区单实例 | 左/右停靠，默认左；列表条目激活对应终端，含新建入口和状态 | 200 × 160 CSS px |
| `terminal.session`：终端会话 | 每服务器工作区多实例；每个会话最多一个逻辑视图 | 中央 Tab、底部停靠、独立窗口，默认中央 Tab | 320 × 160 CSS px |

以上是插件声明，核心按照 [F-CORE-004](../../core/workbench.md)承载，移动保持同一 `viewId` 和 PTY。关闭会话列表只关闭导航视图，不结束列表所引用的终端；它没有创建或持有这些会话的关闭责任。

清单中的 `multiple` 描述某种视图能否重复打开，不决定插件业务实例或进程数量；具体运行粒度见第 4 节。

采用 apple-design 的层次、即时反馈和空间连续性；Windows 窗口习惯保持一致，内容使用等宽字体和清晰对比。具体输入、焦点、缩放和异常状态在 F-TERM-001 中定义。

## 4. 运行、数据与交付

每个启用插件一个业务实例是本批运行粒度。当前 App 实际只加载 `sodeck.terminal`，全部服务器和窗口共享它的一个长期运行 QuickJS 业务进程；尚未实现和验证多个实际业务插件同时运行的完整注册与路由。启动更新校验可能暂时创建并停止候选实例，不属于按终端窗口创建实例。

该业务实例按 `workspaceId` 管理多个终端会话；每个终端会话有自己的逻辑视图与渲染表面。raw Wry 视图中可各自初始化终端 UI / xterm，但不各自创建终端业务进程。操作系统窗口是承载容器，可以放多个视图；WebView 渲染进程数量由运行时安排，不能据此推断插件业务实例数量。连接和 PTY 由核心持有，视图携带有效绑定；窗口移动不重新创建业务实例或会话。

当前业务进程崩溃会触发所属普通连接/PTY 回收，影响该插件在全部服务器上的终端；单个视图表面失败由宿主尝试恢复，不能宣称所有 WebView 进程故障均能严格按单视图隔离。现有实现及 Windows 已执行结果、未验收项见[本批实施基线](../../technical/first-terminal.md)和 T-003。

T-004 建议保持上述业务实例粒度，补齐窗口级上下文、通用视图命令/元数据/生命周期及工作区批量迁移；终端接入公开契约，不自行实现服务器 Tab 或窗口拆合。当前外壳仍含终端专用编排，源码事实、建议改动及测试由 [F-CORE-006 第 6 节](../../core/server-tabs.md#6-核心状态与插件边界)维护；相关重构尚未获得实施确认。

插件包包含 UI、业务 bundle、清单及签名，独立构建，依赖 SDK/API v1 及本批实际能力；不包含插件权限表。核心绑定来源并逐次记录调用，日志不录入终端正文。

持久数据 schema 1 保存字号等插件设置；默认字号 14 CSS px，范围 10–24。PTY、输入及滚屏正文不持久化。P-33/P-34 的 [F-CORE-010 r2](../../core/clean-start.md) / [T-009](../../tasks/T-009-clean-start.md) 取消跨启动占位恢复，保留字号等偏好；App 启动为空，用户创建服务器工作区后自动创建一个首终端并连接。定位/移窗不增开，额外终端手动新建；失败/取消原位重试，关闭最后终端不强制补建。插件通过 API 4 的显式 `workspaceInitializers` 管理初始化业务，核心通用调度，不把 primary 命令直接当初始化钩子；契约见[代码方案](../../technical/clean-start-implementation.md)。同次运行断线重连和表面恢复保持，确认与实际验证见 T-009。

插件独立构建和测试入口已建立，命令见[开发入口](../../development-entry.md)。插件级回归组合为 SSH、TERM、UPD 和任务选取的公共用例；测试数据、故障恢复和清理由对应流程定义。实际结果与验收由各次任务记录，原结果只证明对应版本。

## 5. 修订记录

- r6，2026-10-26：登记 T-010 的实际 cwd 报告和 shell 兼容设计，新增 DR 流程；原终端 0.4.0 不具备该能力，本轮没有改包或编码。

- r3，2026-10-25：登记 T-006 的视图收敛和独立 F-TERM-002 输入规格，区分旧声明与待确认目标；保持插件业务/资源边界。

- r1，2026-10-25：依据 P-19 建立实际终端插件规格，首批直接验证真实终端与独立更新，不创建样例插件。
- r2，2026-10-25：核实并澄清插件业务实例、会话、逻辑视图、渲染表面与窗口的区别；记录实际单插件加载及故障范围，链接 T-004 的通用接口增量建议。

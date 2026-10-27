# SoDeck v2 文档导航

更新日期：2026-10-27 · 修订：r41

## 1. 接续入口

客户端与服务器通信监控进入[T-019 r2完整功能方案](tasks/T-019-connection-metrics.md)：用户已认可整体方向，本轮提交[实时计量与总方案](core/connection-metrics.md)、[历史/比较/导出](core/communication-history.md)、[诊断/探测/提醒](core/connection-quality.md)、[主动测速](core/connection-speedtest.md)及[统一界面](design/communication-monitor.md)，四项各有测试流程。具体方案待评审，仅文档，尚无代码实施确认。

文件列表整行容量与公共等级见[T-018](tasks/T-018-file-capacity.md)，延续原型后的明确实施授权；功能与FC-01～05测试由[F-FILES-001 r16](plugins/files/features/server-file-tree.md)维护。

指纹确认与网络隧道界面重设计见[T-017](tasks/T-017-identity-tunnel-ui.md)，[dev.13结果/截图](tasks/T-017-results.md)和[试用说明](tasks/T-017-acceptance.md)提供本次交付入口；不覆盖T-016的独立设计和T-015历史结果。

网络UI按文件面板返修的最新入口为[T-017 r2实测](tasks/T-017-r2-results.md)与[dev.14试用](tasks/T-017-r2-acceptance.md)，同时修复右停靠面板左边框；旧r1仅保留历史。

核心事件与诊断设计进入[T-016 r1](tasks/T-016-observability.md)：[能力总览](core/observability.md)串联统一采集/操作关联、历史查询/补读、重要问题生命周期、现场/指标/证据包、留存与健康五项规格及各自测试。用户确认总体方向并授权设计；本稿没有应用/AI插件实现，也不改变T-015既有实施确认。

最新13项反馈及数据库补充进入[T-015 r3](tasks/T-015-workflow-polish.md)/[代码修改设计r2](technical/workflow-polish-implementation.md)：服务器表单/分组、终端末行/＋、插件中文名、记录滚动、隧道侧栏、文件加载/容量、窗口控制与GUI启动。代码方案固定API4.8增量、原生模态、插件兼容和六个验证关口；按P-52取消旧库迁移，改为结构版本不一致时提示、备份并创建新库。功能规格与测试已同步，等待用户最终确认，本次只编辑文档。

网络隧道完整实施稿见[T-014 r7](tasks/T-014-network-proxy.md)、[代码方案r1](technical/network-tunnel-implementation.md)与[插件总览](plugins/proxy/README.md)：同时提供[固定端口转发](plugins/proxy/features/port-forward.md)和[HTTP/SOCKS内网代理](plugins/proxy/features/network-proxy.md)。[核心r6](core/dynamic-proxy.md)复用T-013路径，补通用TCP/独立网络池/二进制流/splice、预算及测试；协议与规则在可独立更新插件中。Windows/Linux SSH、v2新界面已按用户确认实施；[结果](tasks/T-014-results.md)和[使用说明](tasks/T-014-acceptance.md)分别记录工程验证与用户验收。

跳板连接设计见 [T-013](tasks/T-013-ssh-jump.md) / [F-CORE-014](core/ssh-jump.md)：目标配置路径，核心统一逐节点认证/传输/租约，终端与文件共用；r2 多级修订已确认并实施，完整依赖、兼容、JP/MH验证及独立开发包见[多级结果](tasks/T-013-multihop-results.md)。旧直连首批范围不再作为跳板永久延期的依据。

本轮[T-012 公共设计系统、多巴胺主题与品牌 r3](tasks/T-012-dopamine.md)提交[代码设计方案 r1](technical/design-system-implementation.md)：公共参数/组件集中定义，插件以功能为主并保留专用布局；DS测试见[F-CORE-012 §8](core/appearance.md#8-t-012-公共设计系统与复用测试)。视觉沿用[三张静态设计稿 r2](design/dopamine.md)，新增三主题应用按钮和展开菜单对比稿；行为与AM测试见[F-CORE-008 §10](core/workbench-interaction.md#10-t-012-应用入口与菜单重设计)。外观增量见[F-CORE-012 §7](core/appearance.md#7-t-012-第三主题增量方案)，品牌见[F-CORE-013](core/brand.md)；当前仅设计，待方案确认。T-011实现与验收保持独立。

整体样式重构和双主题切换进入 [T-011](tasks/T-011-appearance.md)：[F-CORE-012](core/appearance.md)维护用户流程与测试，[主题 r3](standards/theme.md)和[UI r9](standards/ui.md)维护配色/排版/材质。r1已获用户明确实施确认；[实施结果](tasks/T-011-results.md)与[体验验收](tasks/T-011-acceptance.md)分开记录。既有任务的实现及验收事实保持。

终端切换/cd 后目录联动进入 [T-010](tasks/T-010-terminal-directory-follow.md)，实际实现、Linux/Windows 验证及独立开发包见[结果](tasks/T-010-results.md)和[试用步骤](tasks/T-010-acceptance.md)。[通用组件通信策略](technical/component-communication.md)与[代码修改方案](technical/terminal-directory-follow-implementation.md)明确类型化调用、状态订阅、有界流、PTY 观察和职责；[F-FILES-002](plugins/files/features/terminal-directory-follow.md)、[F-TERM-003](plugins/terminal/features/directory-reporting.md)、[F-CORE-011](core/directory-context.md)及[F-CORE-001](core/communication.md)各自维护行为与测试。方案已确认并实施，用户验收仍待进行。

全新启动与自动首终端见 [T-009 r2](tasks/T-009-clean-start.md)：[F-CORE-010 r2](core/clean-start.md) 定义空工作台、配置/临时状态、旧快照退役及新工作区初始化。完整重启不恢复空会话；用户新建服务器 Tab 后自动创建一个终端并连接；已有 Tab 定位/拆合不增开。托盘保持同次运行。确认及交付状态由任务维护，具体证据见[实测记录](tasks/T-009-results.md)。

服务器目录树见 [T-008 r4](tasks/T-008-server-file-tree.md)：[文件插件 r4](plugins/files/README.md)、[F-FILES-001 r4 目录树与挂载详情](plugins/files/features/server-file-tree.md)及 [F-CORE-009 r3 远端读取](core/remote-read.md)分别维护业务、公共能力和各自测试。用户已确认[代码设计与修改规划 r1](technical/server-files-implementation.md)，实现版本、已运行检查和缺口见[实测记录](tasks/T-008-results.md)，开发包操作见[试用步骤](tasks/T-008-acceptance.md)。

T-008 保留 r3 对 T-009 实际代码的复核，r4 固定 API 4.1 兼容扩展、原终端 0.4.0 包兼容、字节路径、SFTP/exec、真实多插件路由/焦点、预算和 Linux 验证计划。实现复用已有启动/初始化机制，工作区现场不跨重启恢复；历史方案提交状态不替代当前任务结果。

服务器预设密码与选择器外观修订进入 [T-007](tasks/T-007-server-editor.md)，用户已明确选择默认记住密码后实施；[实现与验证](tasks/T-007-results.md)、[使用步骤](tasks/T-007-acceptance.md)维护本轮交付。

当前进入 [T-006：工作台视觉与终端交互收敛](tasks/T-006-workbench-interaction.md)：[F-CORE-008](core/workbench-interaction.md) 定义统一视觉、无会话列表/分屏、Tab 与右键关闭；[F-TERM-002](plugins/terminal/features/terminal-input.md) 定义复制粘贴、Ctrl+C、焦点与原生验证。具体实施、开发包和未完成验证见[实施结果](tasks/T-006-results.md)，用户操作见[五步验收](tasks/T-006-acceptance.md)。下列旧任务保留各自确认和实测。

先读 [AI 入口](../AGENTS.md)和[开发规范](../DEVELOPMENT.md)，再从[产品与当前工作](product.md)进入当前任务。接续已有任务时核对确认范围、规格修订和工作区，再按需阅读下表，不要求每次通读所有远期文档。

当前首批方案是 [T-003：核心与真实终端](tasks/T-003-core-terminal.md)，由[核心 SSH](core/ssh.md)、[终端插件](plugins/terminal/README.md)及[实施技术方案](technical/first-terminal.md)细化；不再以前置测试插件作为交付。

本轮界面方向进入 [T-004：顶部服务器 Tab 与工作区窗口设计](tasks/T-004-server-tabs.md)及 [F-CORE-006](core/server-tabs.md)：无独立标题栏、整工作区新窗口与合并。具体模块、公共契约与升级验证见[代码修改方案](technical/server-tabs-implementation.md)，按用户要求确认后再编码。T-003 的实现/测试与确认范围继续由原任务维护。

启动脚本反馈的后续方案进入 [T-005：直接启动与应用设置](tasks/T-005-startup-settings.md)及 [F-CORE-007](core/startup-settings.md)：双击 EXE、默认路径与参数职责、App 设置入口。本轮仅设计，不能将 T-004 的确认当作新设置功能的编码授权。

已确认任务的本地构建、运行与验证命令见 [Windows 开发入口](development-entry.md)，实际覆盖及限制以对应任务结果为准。

## 2. 从粗到细

| 层次 | 文档 | 维护的信息 |
| --- | --- | --- |
| 产品 | [产品总览](product.md) | 用户决定、工程推断、待决问题、当前任务链接 |
| 开发协作 | [开发规范](../DEVELOPMENT.md) | 职责、授权、交付、Git 与发布 |
| 文档维护 | [文档更新与方案确认](documentation-workflow.md) | 先写文档、用户确认、再编码的流程与状态 |
| 总体结构 | [架构](architecture.md) | 核心与插件分层、协作边界、验证路线 |
| 公共规范 | [UI](standards/ui.md)、[编码](standards/coding.md)、[测试](standards/testing.md) | 各模块共同遵循的细则 |
| 主题与颜色 | [独立主题定义](standards/theme.md) | 配色与组件/布局分离、语义与状态色、终端调色、统一消费及替换验证 |
| 核心 | [核心能力](core/README.md) | 公共能力、资源归属及下属功能索引 |
| 核心详细设计 | [设计方案](core/design.md)、[通信契约](core/communication.md)、[资源生命周期](core/resources.md) | 能力清单、状态所有者、基础规格及测试流程 |
| 核心调用记录 | [逐次留痕规格](core/call-records.md) | 无插件权限分配、调用记录字段与异常处理、测试流程 |
| 核心事件与诊断 | [公共能力方案](core/observability.md) | 事件/操作链、历史与实时补读、重要问题、现场/指标/证据包、插件及未来AI消费；五项独立规格和测试 |
| 核心工作台 | [工作台、插件视图与窗口](core/workbench.md) | UI 职责边界、服务器工作区、停靠/Tab/独立窗口、分屏、托盘及自己的测试流程 |
| 服务器 Tab 与工作区窗口 | [功能设计与测试](core/server-tabs.md) | 无独立标题栏、两层 Tab、完整工作区拆合、窗口归属、关闭与恢复 |
| 直接启动与应用设置 | [功能设计与测试](core/startup-settings.md) | 默认资源和数据定位、应用设置、开发覆盖、启动失败及后续目录迁移范围 |
| 全新启动与自动首终端 | [功能设计与测试](core/clean-start.md) | 启动空态、配置/运行状态分离、旧快照退役、新工作区自动连接与失败/取消/去重验证 |
| 工作台视觉与交互收敛 | [功能设计与测试](core/workbench-interaction.md) | 两层导航、Tab 名称、右键菜单关闭、单中央区域、旧布局迁移及原生验证 |
| 终端输入 | [功能设计与测试](plugins/terminal/features/terminal-input.md) | Windows 复制粘贴、中断、菜单、IME 与异步目标保护 |
| 服务器目录树与挂载 | [功能设计与测试](plugins/files/features/server-file-tree.md) | 每工作区文件侧栏、文件大小、挂载点/来源/容量、异步与降级 |
| 远端读取与执行 | [功能语义与测试](core/remote-read.md) | 拟新增 SFTP/非 PTY 执行、句柄、资源隔离和有界读取；契约与预算见本次代码方案 |
| 文件插件代码规划 | [代码设计与修改规划](technical/server-files-implementation.md) | 模块清单、API 4.1 兼容、路径适配、多插件、Linux 采集、预算、实施顺序与 MP 平台测试 |
| 技术 | [选型与工程组织](technical/README.md) | 候选、比较方法、采用依据及验证限制 |
| 插件技术 | [运行容器](technical/plugin-runtime.md)、[协议](technical/plugin-protocol.md)、[更新](technical/plugin-updates.md) | 运行、公开通信、版本与恢复机制 |
| 业务插件 | [插件入口](plugins/README.md) | 插件职责、独立总览和功能规格要求 |
| 单项功能 | [功能模板](templates/feature.md) | 本次行为、接口、验收与自己的测试流程 |
| 插件总览 | [插件模板](templates/plugin.md) | 插件边界、功能索引及共同约束 |
| 本次工作 | [任务模板](templates/task.md) | 具体方案、用户确认、实现、验证及验收 |

## 3. 按任务读取

| 本次工作 | 在当前任务之外重点读取 |
| --- | --- |
| 核心能力变更 | 核心总览、对应功能、公开契约及受影响插件 |
| 插件功能 | 该插件总览、当前功能、所调用的核心接口 |
| UI 设计或调整 | UI 规范、工作台承载契约及对应功能的交互与状态 |
| 技术实验 | 已确认产品约束、相应技术候选、实验方案及判断标准 |
| 缺陷修复 | 正确行为的规格、复现与回归用例、相关实现与日志 |
| 安装、数据迁移或发布 | 对应功能、版本兼容、恢复方案与实际平台验证 |

## 4. 文件约定

- 核心功能放在 `docs/core/<功能>.md`，复杂模块按需建子目录。
- 每个插件拥有 `docs/plugins/<插件>/README.md`，功能放在其 `features/` 下；尚未开发的插件先保留范围与待决项。
- 普通及复杂任务放在 `docs/tasks/T-xxx-<主题>.md`；轻量变更可使用所属功能内的记录。
- 长期技术取舍按需放在 `docs/decisions/`，不为一般内部选择新增文件。
- 单项功能可以合并规格与测试流程；技术实验也可在任务中合并计划和实验测试，避免重复。

具体进度、平台范围和待决项只在产品与任务中维护。总览、模板和候选方案均不代表已批准开始编码。

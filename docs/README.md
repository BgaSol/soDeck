# SoDeck v2 文档导航

更新日期：2026-10-25 · 修订：r13

## 1. 接续入口

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
| 核心工作台 | [工作台、插件视图与窗口](core/workbench.md) | UI 职责边界、服务器工作区、停靠/Tab/独立窗口、分屏、托盘及自己的测试流程 |
| 服务器 Tab 与工作区窗口 | [功能设计与测试](core/server-tabs.md) | 无独立标题栏、两层 Tab、完整工作区拆合、窗口归属、关闭与恢复 |
| 直接启动与应用设置 | [功能设计与测试](core/startup-settings.md) | 默认资源和数据定位、应用设置、开发覆盖、启动失败及后续目录迁移范围 |
| 工作台视觉与交互收敛 | [功能设计与测试](core/workbench-interaction.md) | 两层导航、Tab 名称、右键菜单关闭、单中央区域、旧布局迁移及原生验证 |
| 终端输入 | [功能设计与测试](plugins/terminal/features/terminal-input.md) | Windows 复制粘贴、中断、菜单、IME 与异步目标保护 |
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

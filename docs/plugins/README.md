# 插件文档入口

更新日期：2026-10-29 · 修订：r14 · 状态：各插件文档按各自任务维护，实现与确认见任务。

T-029拟接官方插件独立发布/在线升级，第三方继续手动导入。公共[功能](../core/online-updates.md)/[代码](../technical/online-updates-implementation.md)，各自专项：[终端](terminal/features/online-updates.md)、[文件](files/features/online-updates.md)、[隧道](proxy/features/online-updates.md)、[监控](monitor/features/online-updates.md)；仅设计，确认由[任务](../tasks/T-029-online-updates.md)维护。

T-028插件管理：[公共功能](../core/plugin-management.md) / [代码方案](../technical/plugin-management-implementation.md)统一完整性、签名状态和重启启停。各插件独立回归：[终端](terminal/features/plugin-management.md)、[文件](files/features/plugin-management.md)、[网络隧道](proxy/features/plugin-management.md)、[监控](monitor/features/plugin-management.md)。四个API6.1原签名包在API6.2核心上的验证见[T-028结果](../tasks/T-028-results.md)，确认与验收由[任务](../tasks/T-028-plugin-management.md)维护；第三方打包见[开发说明](../plugin-development.md)。

T-027 四插件各 r4 的计划：[终端](terminal/features/mixed-route-integration.md)、[文件](files/features/mixed-route-integration.md)、[网络隧道](proxy/features/mixed-route-integration.md)、[资源监控](monitor/features/mixed-route-integration.md)。原混合路径实现已交付，结果见任务；本轮[整体修订 r2](../design/connection-editor-refinement.md)合并逐步诊断引用/入口、错误去重、被动状态、统一认证与公共 UI，各插件有具体文件及 DIAG 专项并独立构建/更新。当前修订仅文档，确认与实测由[T-027 r7](../tasks/T-027-mixed-route.md)维护。

## 1. 插件负责的业务

首期来源、开发优先级和平台以[产品总览](../product.md)为准。下面说明业务归属，列出的能力不自动进入首发。

| 插件 | 业务职责 | 使用的核心原语 |
| --- | --- | --- |
| 终端 | 终端渲染、输入交互、选择与粘贴、专属设置；回放等功能另定范围 | PTY、字节流、尺寸同步、受控剪贴板、存储和工作区 |
| Docker | 能力探测、CLI/API 适配、镜像与容器模型、业务操作、日志解释和界面 | 执行、流、任务、文件、隧道与端点 |
| NetBird | 客户端探测、状态解析、节点与网络规则、服务操作及恢复判断 | 执行、网络探测、任务、连接状态和存储 |
| 文件 | 目录树、文件大小和挂载解释；编辑/传输待后续范围 | SFTP/文件原语、有界执行、连接租约与工作台 |
| 资源监控（设计中） | 当前服务器CPU、内存、磁盘、网络的采集解释、短期趋势与界面 | 核心连接观察/租约、有界远端I/O、工作区/呈现与公共UI；调度和指标模型由插件拥有 |
| AI助手（设计中） | 统一对话、范围/证据、模型适配及任务编排 | 公共观测、服务器/连接与远端执行；本机命令已排除 |
| 网络隧道 0.1.0 | 固定端口转发与 HTTP/SOCKS 内网代理的规则、编排、协议业务、指南及视图 | 多层 SSH 路径、TCP 通道、本地监听及资源生命周期；实现与结果见 T-014 |

业务解析和成功条件留在插件，核心只持有通用资源及执行状态。终端渲染器的具体选择仍需技术方案；核心不绑定其内部格式。

## 2. 每个插件的独立文档

使用[插件总览模板](../templates/plugin.md)建立独立目录，并使用[功能规格模板](../templates/feature.md)逐项细化。只把本次要开发的功能写细；后续保留范围、依赖及待决项。

| 插件 | 文档及任务入口 | 下一步需要细化的内容 |
| --- | --- | --- |
| 资源监控 | [独立总览](monitor/README.md)、[T-024](../tasks/T-024-server-monitor.md)、[代码方案r1](../technical/server-monitor-implementation.md) | [功能r2及测试](monitor/features/server-resources.md)与代码稿已固定API5.2、采集/预算/界面和独立更新验证，待明确实施确认 |
| 终端 | [独立总览](terminal/README.md)、[T-003](../tasks/T-003-core-terminal.md) | 真实 SSH、输入输出、会话和窗口、独立更新及各自测试已纳入本批方案 |
| 文件 | [独立总览](files/README.md)、[T-008](../tasks/T-008-server-file-tree.md) | 目录树与挂载详情的规格、公共 I/O / 多插件方案、实际验证和剩余项见任务 |
| Docker | 尚未建立详细规格 | 所选查询或操作、服务版本范围、回执与恢复及测试 |
| AI助手 | [独立总览](ai/README.md)、[T-022](../tasks/T-022-ai-assistant.md) | 统一窗口已确认；[功能草案](ai/features/assistant.md)讨论核心/远端工具和执行策略，无编码授权 |
| NetBird | 尚未建立详细规格 | 所选状态与操作、连接影响、结果核对及测试 |
| 网络隧道 | [独立总览](proxy/README.md)、[T-014](../tasks/T-014-network-proxy.md)、[代码方案](../technical/network-tunnel-implementation.md) | [端口转发](proxy/features/port-forward.md)与[内网代理](proxy/features/network-proxy.md)同时纳入；跨机器路径、Windows/Linux、全新v2界面已实施；[实测](../tasks/T-014-results.md)与用户验收分别记录 |

总览维护插件边界和功能链接，每项功能维护自己的测试流程。实现与验收状态引用关联任务，不在总览再维护一套状态。编码前按[文档流程](../documentation-workflow.md)准备并确认本次方案。

## 3. 依赖与验证

先核对[核心能力](../core/README.md)和[协议](../technical/plugin-protocol.md)。业务变化优先在插件完成，只有缺少可复用原语或必须由核心持有的资源时才提出核心扩展，并说明消费者、兼容影响及契约测试。

内部依赖与通信规则引用[编码规范](../standards/coding.md)，插件发布和恢复证据引用[测试规范](../standards/testing.md)。业务用例不能只验证插件页面：应按实际影响检验其公开调用、资源行为及独立更新结果。

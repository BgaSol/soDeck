# 技术方案与技术选型

更新日期：2026-10-29 · 修订：r27 · 状态：总体候选与取舍背景；已采用组合及验证限制以各任务结果为准。

[在线升级代码方案r2](online-updates-implementation.md)已获用户实施确认：专用GitHub标签/完整签名目录、五组件构建输入与实际版本、来源/候选提交点、Tauri缓存验签及NSIS退出恢复、API6.3且core7/观测2保持。dev.21本地实现、QA安装A→B、四插件与试用包结果见[T-029实施结果](../tasks/T-029-results.md)，操作入口见[发布工具](release-v2.md)。G5真实Actions/公网验证待另行授权，不视为已发布。

[插件管理代码方案r1](plugin-management-implementation.md)对应[T-028](../tasks/T-028-plugin-management.md)：API6.2增量、同一已验证字节快照、独立启用文件与启动/恢复门禁、设置草稿协调、第三方打包及Windows验证；保持core v7。实际实现与验证见[结果](../tasks/T-028-results.md)。

[混合多级连接代码方案r1](mixed-route-implementation.md)对应[T-027 r2](../tasks/T-027-mixed-route.md)/[F-CORE-031 r2](../core/mixed-route.md)：按用户要求不做兼容，采用单一步骤表、API6/core v7备份重建、SSH/SOCKS顺序执行、凭据上下文/预算及四插件各自修改计划。MR-CODE-01～10与插件专项均已定义；当前仅设计，等待整体实施确认。

[连接首页代码方案r1](connection-home-implementation.md)对应[T-025 r2](../tasks/T-025-connection-home.md)/[F-CORE-029 r2](../core/connection-home.md)：外壳独立模块/草稿，核心系统页选中与原生呈现、代理可靠目录、凭据条件写、两阶段关闭及HCODE-01～11；规划API5.3增量，旧契约和v6数据库保持，不默认重建库。与T-023设置退出和T-024呈现正确衔接，本轮未编码。

[资源监控代码方案r1](server-monitor-implementation.md)对应[T-024 r2](../tasks/T-024-server-monitor.md)：独立插件采集/解析/趋势，公共clock、有界exec、系列图表与默认视图增量；API/SDK5.2顺接已授权的T-023/5.1，数据库不变。固定预算、Linux/BusyBox及Windows原生/独立更新验证；仅设计，未获本任务实施确认。

统一设置代码方案见[T-023 r1](settings-implementation.md)：API5.1贡献与provider、成组原子保存/回执、主题v3、窗口退出、状态feed及终端独立更新已按确认范围实施。SCODE-01～11的分层证据、实际版本及目录包由[实施结果](../tasks/T-023-results.md)维护。

[核心事件与诊断代码修改方案r1](observability-implementation.md)对应[T-016 r5](../tasks/T-016-observability.md)：基于当前API4.9/观测schema1，固定4.10/观测2、可信元数据、统一存储与三插件接入、证据及验证关口。仅观测四分区备份重建，同版本core5及凭据保留；问题/通知/AI和新诊断界面后续。等待用户最终确认，本轮未编码。

[通信监控代码修改方案r1](communication-monitor-implementation.md)细化[T-019 r3](../tasks/T-019-connection-metrics.md)：真实计量、SSH响应探测、内存测速、公共观测存储、API4.9、原生工具窗、schema4→5及验证/开发包。当前仅设计，等待用户最终实施确认；三个先行实验尚未运行。

本轮[T-015代码修改设计r2](workflow-polish-implementation.md)覆盖13项体验反馈及P-52数据库策略，固定模块、API4.8/SDK/manifest增量、分组事务、版本不一致时提示/备份/全新建库、通用原生模态、文件高度/容量、启动与六个关口；[任务r3](../tasks/T-015-workflow-polish.md)等待用户最终确认。旧库增量迁移提案已替代，文档中的新增接口/代码/脚本尚未建立。

选型约束来自[产品总览](../product.md)：先在 Windows 用真实终端验证官方插件边界与独立更新，结构保留跨平台能力。[首批实施技术方案](first-terminal.md)已提出本批具体组合、预算及失败策略，随 T-003 确认；下表保留总体比较背景，不要求重新实现所有候选或先交付样例平台。

## 1. 选型总览

| 领域 | 当前候选或建议 | 需要验证的内容 |
| --- | --- | --- |
| 桌面外壳与核心 | 延续 v1 的 Tauri 2、Rust | 目标平台、插件嵌入与调用归因、原生输入和打包 |
| 外壳和插件 UI | React、TypeScript | 独立构建、主题契约、焦点与多视图表现 |
| 持久化 | SQLite，核心与插件独立存储；本次按P-52不兼容旧库结构 | 事务、版本预检、一致备份/重建、隔离和失败恢复；凭据配套保留，独立插件文件按自身契约演进 |
| 插件业务运行方式 | 嵌入 JS 引擎、Web 执行环境等候选，比较后选择；独立进程粒度另行判断 | 依赖兼容、后台生命周期、异步、资源预算、故障影响与终止 |
| 插件视图与窗口 | 承接 v1 已实现并具有原生测试依据的普通多窗口、快照与会话交接；插件嵌入容器仍待选择 | 工作区迁移到通用插件视图的增量、焦点/输入法、调用归因及资源占用；子 WebView/reparent 的 `unstable` 要求不混作既有窗口机制的前提 |
| 工作台布局 | 先核对 v1 工具栏和布局的可复用部分，再比较 FlexLayout、Dockview 能否补足通用承载需求 | 侧边工具区、双区域限制、主题、布局权威状态及插件视图接入；不默认整体替换已有布局 |
| 通信 | 版本化 schema、RPC 和独立有界流通道 | 类型生成、无授权调用及留痕、背压、吞吐和迟到结果 |
| 插件分发 | 签名包、暂存与重启激活 | 完整性、兼容检查、迁移、中断恢复及回退 |

上表保留总体比较背景，不代表所有候选已采用。T-003 实际采用 Tauri/Rust、React/TypeScript、SQLite/Windows DPAPI、ssh2/libssh2 静态 OpenSSL、独立 QuickJS 进程和 raw Wry 视图；固定双区域布局由核心状态与 CSS 承载，没有引入通用停靠库。Windows 协议、原生窗口和独立更新的已执行子场景见结果，IME、多屏和其他平台仍待验证。

## 2. 选型与变更流程

1. 先记录问题、使用场景、约束和候选方案，区分用户已确定要求与技术建议。
2. 写明比较依据、实验范围、步骤、可观察的判断标准、环境与清理方法，提交具体方案并由用户确认后再编写原型。
3. 记录实测数据、失败和限制；满足条件后记录采用理由、替代方案、版本范围及代价，重要取舍建立决策记录。
4. 将结论同步到架构、受影响核心或插件规格及测试流程；确认覆盖本次正式实施方案且满足前置条件后，才实施正式代码。原型确认不自动授权正式采用。
5. 更换已采用方案时，先更新决定、影响和恢复方法，再修改代码，按[文档更新流程](../documentation-workflow.md)执行。

验证顺序与总体目标见[架构实施阶段](../architecture.md#4-实施顺序与架构验收)。尚未明确的指标由对应原型任务提前定义，不以事后结果倒推通过标准。

## 3. 详细方案

- [SOCKS5 SSH 路径代码修改设计 r3](socks-route-implementation.md)，T-020 的主机隧道/网络代理边界、互斥 `accessPath`、显式 SSH 列表、API 5 单一契约、数据库 v6、SOCKS 协议、认证/复用和 SK-01～09 入口；已获确认实施，传输交接、统一替换和验证见[T-020 结果](../tasks/T-020-results.md)

- [通信监控代码修改方案](communication-monitor-implementation.md)，覆盖四项功能的模块/文件、底层适配、接口与插件兼容、数据备份重建、有界资源及E1～E3/MCODE验证；确认与实测由T-019维护

- [网络隧道插件代码实施方案](network-tunnel-implementation.md)，T-014 r7的QuickJS协议/规则、核心公开TCP/网络池/splice、二进制流、API4.7兼容、预算与真实Windows/Linux验证、独立开发包；已确认实施，结果见 T-014

- [公共设计系统与三主题代码设计](design-system-implementation.md)，T-012 r3的样式前置、公共UI组件/插件例外、原生应用菜单、生成/边界检查、独立打包及迁移；本轮仅提交方案，待确认实施

- [组件通信通用策略](component-communication.md)，T-010 已实施状态所有者、类型化 RPC、原子状态订阅、有界流、固定调度与扩展规则；实测见[任务结果](../tasks/T-010-results.md)
- [终端目录联动代码修改设计](terminal-directory-follow-implementation.md)，T-010 已实施核心目录来源、PTY 独立观察、Bash 接入、文件 FollowController 与 API 4.2 兼容；Shell 已确认的额外启动读取见[补充](terminal-shell-startup-review.md)
- [文件插件代码设计与修改规划](server-files-implementation.md)，T-008 的公共 I/O、API 4.1 兼容、多插件路由/焦点、字节路径、文件业务、Linux 环境和验证预算；实施确认、结果与剩余验证见 [T-008](../tasks/T-008-server-file-tree.md)
- [全新启动与自动首终端代码方案](clean-start-implementation.md)，T-009 的 API 4、工作区创建代次与内存状态基线；实际结果由原任务维护
- [顶部服务器 Tab 与公共工作台代码修改方案](server-tabs-implementation.md)，T-004 的模块改动、接口、原生窗口、批次迁移、插件配套升级、预算与验证；等待用户确认后编码
- [核心与真实终端首批实施方案](first-terminal.md)，本批运行路径、契约、预算、记录故障与独立更新测试
- [插件包与运行环境](plugin-runtime.md)，含 v1 迁移源码/历史原生测试、Tauri 与布局库资料及增量验证建议
- [SDK、通信与版本契约](plugin-protocol.md)
- [插件更新、激活与恢复](plugin-updates.md)
- [核心能力与资源模型](../core/README.md)

## 4. 工程组织与 AI 开发边界

建议先使用单仓库，按独立包和依赖边界组织。独立更新不要求一开始拆成多个 Git 仓库。以下展示采用 Tauri/Rust 候选时可能的职责分组；具体目录随选型结论和首个任务建立，不一次创建所有模块。

```text
soDeck-v2/
  AGENTS.md
  DEVELOPMENT.md
  docs/
    architecture.md
  apps/
    desktop/                 # 外壳与 Tauri 应用入口
  crates/
    core/                    # 服务器、连接、资源、端口等 Rust 服务
    plugin-host/             # 仅在采用独立宿主方案时建立
    plugin-manager/          # 包、版本、实例、启动激活及恢复
  packages/
    plugin-protocol/          # schema、RPC 和类型生成
    plugin-sdk/               # 插件 SDK
    ui-kit/                   # 主题协议与可随插件构建的公共组件
  plugins/
    terminal/
    docker/
    netbird/
  tests/
    contracts/
    integration/
    desktop/
```

以上是职责分组示意；T-003 已建立 desktop、core、plugin-host、plugin-manager、SDK 和终端工程，实际目录与命令见[开发入口](../development-entry.md)。空目录和占位接口不算能力完成。跨模块依赖规则以[编码规范](../standards/coding.md)为准，工程方案需保证：

- 插件对 SoDeck 内部模块的依赖通过公开 SDK、协议和公共 UI 包；第三方依赖依据所选运行环境和包构建规则评估。用编译期或自动检查验证核心与业务插件之间的依赖边界。
- 插件可以独立构建、测试和生成带签名的发布包，不要求每次打包完整 App。
- 官方插件使用与未来扩展一致的公开调用路径，避免只有官方面板才能访问隐藏接口。
- AI 开发插件功能时先核对已有能力；只有明确缺少通用原语时才提出核心 API 变更，并补充契约测试和版本要求。

插件交付增加三类证据：声明的核心兼容范围、插件包自身的测试结果、安装后重启启用和失败恢复结果。常规功能交付仍遵循 DEVELOPMENT.md，不要求用户审阅全部代码。

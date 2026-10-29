# 插件 SDK、通信与版本契约

更新日期：2026-10-29 · 修订：r25 · 状态：通用契约基线；T-028增量待确认，实际支持见各任务结果。

T-028新增[插件管理契约方案（第5节）](plugin-management-implementation.md)：拟API/SDK6.2、新plugins.management.v1与plugins.optional-signature.v1，分页元数据、启用偏好/原子回执、导入操作及应用级feed。旧plugins.list/metadata.list/prepareUpdate严格DTO保持，当前^6.1插件无需重编译。停用/未签名不改变公开调用归因或P-14；签名要求统一由[F-CORE-032](../core/plugin-management.md)替代。所有新接口尚未实现，确认见[T-028](../tasks/T-028-plugin-management.md)。

T-027新主版本建议：[混合连接代码方案r1 §6/7](mixed-route-implementation.md#6-契约宿主和界面修改位置)采用API/SDK6.0.0，四官方插件声明^6.0.0及实际v2路径能力，旧包/旧核心互相明确拒绝，不加API5适配。插件职责和独立构建/签名/重启更新保持，各插件文档有具体代码与测试；MR-CODE-10要求API6环境内单包A→B独立更新，不用全量替换冒充。当前未实施，确认见[T-027 r2](../tasks/T-027-mixed-route.md)。

T-023设置接入推荐增量见[F-CORE-028 r2 §4](../core/settings.md#4-核心与插件契约)及[代码方案r1 §4～6](settings-implementation.md#4-插件声明格式和校验)：API5.1新增settings.contributions.v1/storage.preferences.v1/ui.settings.v1，旧settings.v1/get/set键值能力保留；manifest可选contributes.settings引用签名包内settings/settings.json。固定provider回调经现有wire、带代次/修订提交、同文件原子回执、结果核对与settings.page/catalog状态feed。插件拥有配置语义/校验/布局/生效，核心拥有注册/呈现/编排/通用存储，不开放插件互调或权限审批。SCODE-01～03/09/11及SET/TERM-SET验证契约和独立更新；旧无声明插件可运行，更新重启生效，新增接口尚未实现，等待整体方案确认。

T-016 r5[接入需求](../core/observability/integration.md)基于当前API4.9明确分组capability、包内版本化采集描述、UI/worker统一无DOM采集client、经协商的诊断上下文元数据及一个在途采集请求。上下文不塞业务params、不破坏旧strict DTO，完整事件查询另用events.v2.*；新采集包需相应能力，旧包仍audit-only。类型注册受真实包身份约束，不是权限表；插件事件不能作跨插件命令。[代码方案r1](observability-implementation.md)固定API/SDK4.10、可选manifest.observability={descriptorVersion:1,path}、metadata.diagnostics={version:1,contextRef}、worker diagnosticContextVersion协商、invoke第三上下文参数与宿主同实例转交。旧严格DTO不变，diagnostics.get保留，新任务使用diagnostics.capture.get/cancel；v2健康/策略/清理独立。API/SDK4.10及三个独立采集包已实现，版本支持和验证边界见[T-016结果](../tasks/T-016-results.md)。

T-016按P-53提出[统一事件/诊断契约](../core/observability.md)：宿主绑定来源和操作上下文，插件随独立包携带受限、版本化的事件/快照/指标描述，使用核心公开采集与查询服务。描述不是权限清单；旧包可继续运行但业务采集覆盖不足明确可见。历史类型保留、可补读feed、防审计递归和未来AI只读证据路径见F-CORE-018～022。新能力名称/manifest字段/schema与API版本已在本轮代码方案固定，本稿不占用T-015的4.8版本、不修改旧strict DTO、不开放插件互调或外部上传。

T-015增量已在[代码方案r1 §3～5](workflow-polish-implementation.md)固定：API/SDK4.8、servers.catalog.v1/plugins.metadata.v1/workspace.contributions.v2/ui.dialog.v1能力，manifest可选displayName与contributes.presentation，新方法/投影和临时模态endpoint。新增两份schema及生成器，不给旧strict DTO塞字段；新核心兼容旧包，新包最低API4.8。边界来源[F-CORE-017](../core/plugin-display.md)/[F-CORE-008](../core/workbench-interaction.md)/[F-PROXY-003](../plugins/proxy/features/tunnel-panel.md)。本次尚未编码，不将提案描述为已发布接口。

T-014基于当前已实现API/SDK4.6.0提交[网络契约r6](../core/dynamic-proxy.md#8-公开契约与资源归属r6)和[代码方案r1](network-tunnel-implementation.md)，建议4.7.0增加network.tcp.v1/network.stream.v1/network.resources.v1及net.group.v1状态。控制RPC/旧严格DTO保持，TCP正文走已建立持续调用的二进制帧，实例来源、epoch/额度、关闭与审计均有独立验证。旧核心拒绝缺失能力的新包，新核心使用未重编译旧终端/文件包；能力声明仍不是权限。workspace.v4资源枚举不增加新kind，网络组由宿主view索引绑定。旧段落所列版本是当时方案背景，本增量未实施。

T-012建议保留主题v2和app.theme.v1的严格负载，新增`ui.theme.catalog.v1`目录/按ID选择能力与`ui.theme.dopamine.v1`完整样式兼容声明；旧live客户端实时换色但保留旧控件，旧静态客户端自然重挂载更新。能力不构成权限。具体参数、投影与测试见[F-CORE-012 §7](../core/appearance.md#7-t-012-第三主题增量方案)和[T-012](../tasks/T-012-dopamine.md)。此增量尚未实现。

T-012 r3进一步将通用样式与组件集中到公开`@sodeck/ui`，保持UI无SDK/业务依赖、SDK无React/UI依赖；组件与CSS随插件独立构建，核心只分发既有主题快照，不热替换旧插件的代码或几何。新设计参数和补充表面色不扩充严格v2消息，应用专用菜单桥不更改插件`ui.menu.*`协议。目录、公共导出兼容与更新矩阵见[代码方案§8](design-system-implementation.md#8-主题接口更新与旧包兼容)，验证见F-CORE-012 DS-03/06。T-013的4.4增量优先保留，T-012初拟4.5或整合时首个未占用版本。

以下接口名称、schema 和清单为设计示意，不能作为已经实现的 SDK 使用。接口落地前应补齐对应核心能力规格和契约测试流程。

T-011拟采用API/SDK 4.3与`ui.theme.live.v1`、主题数据v2，复用`core.state.v1`增加固定`app.theme.v1`契约和宿主绑定的窗口来源；旧目录/视图scope及响应不改形状。`ui.theme.get`按协商投影，严格旧SDK不接收未知字段/通知；旧插件保持挂载时颜色，当前随包两插件实时跟随。接口、兼容矩阵、独立构建和AP-07验证见[F-CORE-012](../core/appearance.md)及[代码规划](../tasks/T-011-appearance.md#3-代码修改规划)。能力声明仍用于兼容，不增加插件权限。这是待确认增量，不修改T-010的4.2实际结果。

T-009 已实施插件 API 4 / `workspace.v4`，当前生成契约为 `contracts/workbench-v4.schema.json`。`workspaceInitializers: [{id, commandId}]` 显式登记同插件的 server 命令；核心按本次工作区 generation 和动作去重调度。`workspaces.open`/快照返回初始化状态，`workspaces.initialize` 重试未创建页面的失败，已有页面通过原命令/认证重试。定位、移窗、表面挂载不重触发，不执行所有 primary 命令。运行布局结果 `persisted:false` 为正常语义，偏好/旧快照清理通过 `workbench.preferences.retry`。行为与测试见 [F-CORE-010](../core/clean-start.md)，完整兼容/错误/取消落点见[代码方案](clean-start-implementation.md)，实际验证见 [T-009 结果](../tasks/T-009-results.md)。历史 v2/v3 契约保留用于资料及旧测试；API 3 插件不冒充当前兼容包。

T-008 的 `files.*` / 非 PTY `exec.*`、连接观察/仅复用及操作回执，其功能语义与测试统一见 [F-CORE-009 r3](../core/remote-read.md)；[代码方案 r1](server-files-implementation.md)固定 API/SDK 4.1、能力版本、字段、错误、预算与生成 schema。当前 dev.5 / files 0.1.0 已交付实现，实际双插件及剩余验证见 [T-008 结果](../tasks/T-008-results.md)；历史多来源夹具测试不替代真实双插件证据。

T-008 r4 延续 API 4 基线：复用 `workspaceEntries` 默认文件视图和已有工作区 generation，不以文件 initializer 重复发起自动连接；命令/动作、认证/取消和资源回执按真实插件归属隔离。已实现 `connections.observe` 与 `connections.acquireExisting`，后者原子登记现存连接租约且禁止新建回退；普通 acquire 不具备该保证。`workspace.multiplugin.v1` / `workspace.presentation.v1` 承载所有者路由、呈现通知和焦点；未声明能力的旧终端不接收未知生命周期命令。

T-008 终端 0.4.0 原包保持不变：核心按协商能力投影原 v4 响应，不能给严格旧 SDK 增加未知字段，也不能放宽全部校验冒充兼容。files 0.1.0 要求 `^4.1.0` 及声明能力；传输 `RpcRequest.apiVersion:1` 与插件 API 分开。正常 `persisted:false`、原终端兼容、真实双插件与缺能力拒绝由 RI-06 和代码方案 MP-01 验证，实施确认及结果在 T-008 记录。

T-010 计划增量为 API/SDK 4.2（整数 API 4、workspace.v4 和 RPC v1 保持），增加 `core.state.v1`、`workspace.directory.v1`、`remote.pty.observe.v1`、`remote.pty.start.v1`。方法和 schema 规划见[通用通信策略](component-communication.md)及[联动代码方案](terminal-directory-follow-implementation.md)：固定契约的原子快照/订阅、同所有者 PTY 观察、通用 PTY exec 启动；旧包不接收未知字段或通知。能力描述用于版本兼容，不是插件权限表。本增量未实现，确认与测试由 T-010 维护。

基础行为的细化及测试见[调用与状态通信](../core/communication.md)和[资源与生命周期](../core/resources.md)；本文件维护 SDK 能力分组、插件协作的扩展方向与版本规则，不重复维护单项测试结果。

按产品 P-14，插件不分配调用权限，所有公开且已支持的能力一致开放；每次调用由核心留痕，细则见[调用记录](../core/call-records.md)。

## 1. 核心 API、扩展点与协作

建议提供以下 SDK 能力，名称为设计示意，尚不是已实现接口：

| API 分组 | 典型用途 |
| --- | --- |
| `servers` / `context` | 获取服务器资料和当前工作区上下文，普通资料接口不返回凭据正文 |
| `connections` | 按服务器、配置修订和连接路径获取租约，订阅状态，释放租约 |
| `remote.exec` / `remote.pty` | 运行命令、接收输出、输入、调整尺寸、取消和获取执行状态 |
| `remote.files` | 受控文件操作和流式读写，按实现能力协商 |
| `tunnels` / `ports` / `endpoints` | 管理转发、监听资源与服务入口 |
| `tasks` / `streams` | 长任务进度、取消、流订阅、缓冲和恢复标记 |
| `storage` / `secrets` | 插件私有状态、文件对象、凭据引用 |
| `workspace` / `ui` | 注册视图类型与支持位置、打开/移动/关闭逻辑视图、布局与窗口、命令/菜单/通知/设置；细则见工作台规格 |
| `services` | 未来可选扩展，第一阶段不开放；服务发现与插件互调另行设计和确认 |
| `diagnostics` | 记录带插件、服务器和操作标识的诊断信息，屏蔽秘密 |

SDK 协议使用可生成或校验类型的统一契约；若采用 TypeScript 与 Rust，两侧边界类型从同一来源对齐，避免手写两份逐渐偏离。具体 schema 工具在选型中确定。

### 通信职责与契约归属

针对模块通信混乱的问题，每条跨边界交互先在对应功能规格定义发起方、接收方、状态所有者、上下文、结果及失败规则。使用三类清楚的交互：

| 交互 | 职责 | 必须说明的规则 |
| --- | --- | --- |
| 请求与响应 | 请求状态所有者执行操作或获取快照 | 输入输出、错误、超时、取消及重复调用语义 |
| 事件 | 由状态所有者发布已经发生的变化 | 订阅范围、版本、取消订阅与丢失后的恢复；不能靠广播让多个模块争相修改同一状态 |
| 数据流 | 承载终端、日志或文件等连续数据 | 顺序、边界、背压、上限、关闭和截断语义 |

核心资源状态由核心持有，业务解释与业务状态由所属插件持有，视图展示状态留在视图。需要协作时调用公开接口，不跨模块修改私有状态，也不通过任意字符串事件隐式触发业务操作。核心和插件的各语言类型从契约对齐，具体传输实现由选型决定。

首个原型只实现验证这些边界所需的最小契约，不按上面的能力目录提前建立全部接口。

工作台扩展点遵循 [F-CORE-004](../core/workbench.md)：插件可提供多个视图类型，视图实例与展示容器、业务会话分离。声明支持位置不等于插件能力授权；工作区、视图、挂载代次和布局修订用于正确路由。移动及跨窗口重新挂载不触发业务关闭，旧表面身份由宿主撤销。窗口命令携带其所属上下文，不使用主窗口的活动服务器覆盖独立窗口。所有插件工作台调用仍逐次留痕；具体 schema 在实验中收敛。

### 请求和资源的身份

- RPC 请求包含请求编号、超时及协议版本；插件身份、包版本、实例代次和视图身份由可信宿主根据实际通信通道附加，并关联调用记录。
- 资源模型遵循[核心资源归属](../core/README.md#2-生命周期与资源归属)：区分持久标识、访问租约和会话句柄。核心签发的不透明句柄绑定真实调用者及适用作用域；服务器、工作区和会话代次由具体接口定义，不能为不适用的操作强填上下文。
- 持久标识只用于定位对象，不表示会话仍有效。新插件实例取得当前句柄无需授权；重连时按对应会话规则失效旧句柄，不能仅凭相同字符串或相同插件标识恢复使用关系。
- 切换服务器或重连后，旧异步结果不能应用到新上下文。核心检查资源代次，插件也要丢弃过期界面请求。
- 事件包含流或状态版本，可按需获取状态快照；慢消费者使用有界缓冲和背压。若发生截断或丢失，明确报告，不静默当作连续数据。
- SSH 命令本质上可能经过远端 shell。SDK 必须说明执行语义，提供适配目标环境的安全参数编码；不能将 `argv` 外观误当作所有远端系统上的无 shell 执行保证。

### 连接复用与操作结果

多个插件共享经过远端认证的连接上下文和连接池，不保证所有流强制使用同一个物理 socket。复用依据至少包含服务器、路径、账号/凭据修订和必要工作区上下文；终端交互与大流量传输应有独立调度或统一通道容量规则。

核心提供通用状态：尚未发出、已发出、完成、失败、结果不明。远端已经收到命令后发生断线，不能自动认定操作未执行，也不能自动重试写操作。插件负责根据 Docker、NetBird 等业务状态核对结果；RPC 去重不等于远端副作用具备 exactly-once 保证。

### 插件间合作

按[产品 P-13](../product.md)，第一阶段插件各自调用核心公共能力，不开放插件间服务注册、发现、调用或句柄转交。核心不提供可绕过该限制的通用插件消息转发；核心资源仍可按独立租约由多个插件分别使用。

后续确有需求时，插件可通过公开且有版本的服务合作，不导入另一个插件的内部代码或直接读取其数据库。例如 Docker 的“打开容器终端”可在未来通过 `terminal.open` 服务实现；该例仅说明扩展方向，不属于当前阶段。未来跨插件转交仍需核心记录和核对资源使用关系，不能复制句柄造成引用计数混乱。

未来服务协作需单独定义缺失服务时的行为、调用记录、版本和兼容测试；当前不建立占位互调接口。插件硬依赖若以后出现，安装器必须解析版本约束、阻止依赖环，并共同验证兼容更新。

### 插件调用策略

不建立插件权限清单、服务器授权、能力审批或权限扩大确认。宿主识别真实来源以准确留痕；方法存在、参数有效、实例和资源状态满足契约时即可调用。租约和句柄只负责使用关系与生命周期。

`remote.exec` 和 PTY 使用实际远端账号，执行环境自身的认证与权限仍如实返回。不存在的接口不会因此自动实现；P-13 的首阶段不开放插件互调也继续适用。

## 2. 版本与兼容契约

分别管理四类版本：

| 版本 | 作用 |
| --- | --- |
| Core Version | 应用外壳、核心公共能力、所采用的插件运行容器及打包版本 |
| Plugin API Version | 插件调用协议、扩展点和数据类型的兼容契约 |
| Plugin Version | 某个插件独立发布的版本 |
| Plugin Data Version | 某个插件私有持久化数据的 schema |

清单示例仅用于解释结构，以下能力与版本号均为拟定值：

```json
{
  "id": "official.docker",
  "version": "1.2.1",
  "requires": {
    "core": ">=2.0.0 <3.0.0",
    "pluginApi": "^1.0.0",
    "capabilities": ["remote.exec.v1", "tunnels.local.v1"]
  },
  "entry": {
    "ui": "ui/index.html",
    "worker": "worker/main.js"
  },
  "contributes": {
    "views": [{
      "id": "containers",
      "title": "Containers",
      "scope": "server",
      "supportedHosts": ["dock", "tab"],
      "dockEdges": ["left", "right"],
      "defaultLocation": { "host": "dock", "edge": "left" }
    }]
  },
  "dataVersion": 2
}
```

能力要求只表示插件运行所需的宿主能力及版本，不是权限白名单。未声明但核心已公开支持的方法同样可以调用并留痕；清单不包含插件权限字段。

视图清单示例仅说明一种支持停靠及 Tab 的视图；单/多实例、状态版本及尺寸等字段含义由工作台规格定义，完整清单 schema 尚待实验。用户布局优先于插件默认位置；更新改变视图声明时按工作台恢复规则处理。

API 小版本以向后兼容的扩展为主，破坏性变化提升主版本，并规定支持窗口。插件发布需验证声明支持的最低核心版本与当前版本，不能只写宽泛版本范围却只测最新版本。

若插件使用平台特有能力，清单应声明平台/架构约束；远端操作系统和 Docker/NetBird 版本则由插件做能力探测与兼容判断。无法支持时给出明确状态，不能因此阻止整个服务器工作区使用。

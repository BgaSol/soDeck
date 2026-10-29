# T-025：连接首页代码修改设计

文档就绪：已确认实施基线 · 修订：r1 · 更新日期：2026-10-28\
功能基线：[F-CORE-029 r2](../core/connection-home.md) · 确认/结果：[T-025 r2](../tasks/T-025-connection-home.md)\
约束：[架构](../architecture.md)、[编码](../standards/coding.md)、[UI](../standards/ui.md)、[测试](../standards/testing.md)

实施状态与交付见 [T-025 结果](../tasks/T-025-results.md)。本设计基线已获确认并实施；下文“拟/预计/未运行”为评审当时记录。实现将 §5.3 的凭据后续发布待办明确表示为 `CredentialStored.postCommitPending`，后台只重试目录发布；业务数据库仍为 v6。

## 1. 结论、范围与真实基线

采用“核心工作台持有窗口选中对象 + 外壳连接页持有草稿 + 核心配置/凭据服务持有可靠写入”的方案。新建连接是系统页，不伪装成服务器或插件视图。替换旧选择器/表单的全部产品入口，保留已实现的SSH/SOCKS传输、工作区初始化、现有配置和消费者边界。

2026-10-28用户要求“设计一下代码如何修改”，本轮只更新文档，不视为实现授权。推荐方案与验证均在此给出，确认后按整条流程实施，不为每个文件另行确认。

已核实v2工作区，不把其他任务的在途代码当成完成结果：

| 当前代码 | 事实及本次处理 |
| --- | --- |
| `apps/desktop/src/app.tsx` | `picker/serverEditor/proxiesPage/groupsPage`各持模态开关；启动欢迎页再开Dialog；每2秒refresh连带加载服务器。改接连接页控制器与独立目录订阅，其他工作台刷新按原职责保留 |
| `server-picker.tsx`、`server-dialog.tsx` | 选择/管理混合；`steps`嵌套新建跳板、`proxyEditor`隐藏父表单。删除这套呈现和递归创建状态，迁移有效校验/提交语义，不原样搬迁组件 |
| `proxy-dialog.tsx`、`server-groups.tsx` | 业务提交、局部状态与Dialog耦合。拆成受控字段、管理内容和服务，统一右侧呈现；真正的删除/丢稿确认继续使用公共Dialog |
| `server-catalog-client.ts` | 服务器保存已有operationId、回执查询和分组revision；可复用但需区分提交/秘密阶段、不可由组件卸载结束事务 |
| `crates/core/src/proxies.rs` | 代理创建由核心生成ID、无operationId/查询回执；list一次返回全部，修改已有expectedRevision；本次补可靠保存和分页目录，旧方法保留 |
| `crates/core/src/lib.rs` 的 `auth.ssh.set` | 能读取本机私钥并保存密钥内容，文件上限1MiB；无挑战的预设写按“当前”配置取身份，缺提交时的配置条件，需避免草稿旧密码写到并发修改后的新身份 |
| `app.rs::present`、`plugin_presentation.rs` | 原生可见性目前主要依赖壳层上报的visible，显示时可恢复插件焦点；增加系统页呈现约束，拒绝迟到旧布局使终端覆盖表单 |
| `WindowRecord` / `Workbench` | 目前只有active_workspace_id/focused_view_id，无系统页；`snapshot`为workspace.v4，严格DTO不宜直接塞新字段 |
| `settings_window.rs` / `settings_exit.rs` | T-023已在开发，已有设置窗口退出票据/单次许可逻辑；本任务接入统一关闭预检，不能新增第二套互相递归调用app.exit的流程 |
| SQLite / API | 当前业务库结构v6，已有socks_proxies、server_catalog_receipts及目录revision；源码API版本仍可能随T-023推进。T-023规划5.1，T-024预留5.2；本稿按增量API/SDK5.3规划，实装能力按实际合入声明 |

本次不改SSH协议、不创建代理工作区、不修改已有插件业务，不引入新的数据库结构版本或前端运行依赖。凭据回执的请求校验拟在core增加`hmac 0.12`直接依赖，与现有`sha2 0.10`配合，复用`getrandom/zeroize`生成和回收本进程key；实际解析版本写入Cargo.lock，不自行实现密码算法。文件选择复用已有rfd。下列所有“新增”文件/方法/测试均是计划，不是现成可调用入口。

## 2. 模块与文件修改清单

### 2.1 外壳模块

建议目录 `apps/desktop/src/connection-home/`：

| 文件 | 单一职责 |
| --- | --- |
| `index.tsx`、`page.tsx` | 页面入口、左右布局与右侧模式组合；不直接写核心数据 |
| `model.ts`、`drafts.ts` | 纯状态归约、草稿身份/基线/脏状态/路由模式、转换规则和容量；无RPC |
| `controller.ts` | 连接页生命周期、异步操作、返回意图、焦点/滚动恢复；组件只发意图 |
| `catalog.ts` | 一份每窗口目录缓存，订阅服务器/分组/代理revision，分页补读/失效恢复；为列表/选择器提供相同快照 |
| `server-list.tsx`、`draft-switcher.tsx` | 搜索/筛选/行主操作/独立编辑按钮、未完成稿切换；不自建工作区 |
| `ssh-form.tsx`、`credential-fields.tsx` | SSH基础字段与认证字段，受控值和字段错误；不持有提交副本 |
| `access-path-fields.tsx` | 三种互斥模式、已保存SSH节点排序和代理选择；无新建SSH回调 |
| `proxy-fields.tsx`、`proxy-manager.tsx` | 同一代理字段组件在独立创建/内联新增/编辑中复用，管理页负责依赖信息 |
| `group-fields.tsx`、`group-manager.tsx` | 内联建组及独立右侧管理；分组业务使用原目录API |
| `save.ts`、`validation.ts` | 服务器/代理保存协调器与纯校验/名称生成；保存阶段显式建模 |
| `secrets.ts`、`close.ts` | 当前renderer内秘密缓冲及关闭票据响应；不会写入浏览器存储 |
| `layout.css` | 仅本页列宽/响应式/滚动关系，消费公共令牌；不复制公共控件外观 |

`App`只负责挂载控制器、接工作台/窗口导航、传递公开命令，不继续扩展四个模态布尔开关。目录缓存输出也供原服务器Tab标题使用，不让两个列表各读一份有不同新旧程度的数据。

`server-tabs.tsx`增加一个具名系统页项，固定在服务器序列之前；系统页无`data-workspace-id`，不参与服务器reorder/拖动。方向键、Home/End和关闭键覆盖“系统页+服务器页”，ARIA名称改为“工作台页面”。服务器插入位置仍按真实workspaceIds计算；chrome测量明确排除系统页作为workspace拖动目标，视觉偏移不得改变原生命中。

`surface.tsx`保留几何测量职责；`App`按权威选中对象隐藏旧工作区DOM、插件工具和状态。保存草稿Provider放在切换区域之外，表单卸载不丢状态；秘密只保留一份，不同时维护原组件state与草稿副本。

全部入口统一：顶部＋、Ctrl+O、左工具轨服务器入口、Tab右键编辑、`ui.servers.editRequested`、分组内编辑链接均转连接页。单视图独立窗口没有新增服务器Tab；从其编辑通知定位所属工作台，所属窗口失效则回主窗口。原认证/主机指纹Dialog保留真实连接职责，不能当服务器编辑页复用。

### 2.2 原生工作台与核心

| 文件/模块 | 改动 |
| --- | --- |
| `window_registry.rs`、`workbench.rs` | 窗口选中对象、系统页打开状态、返回目标、导航revision及不变量；旧快照只输出已有字段 |
| 新 `connection_home.rs` | 系统页打开/快照、页面会话代次、私钥文件选择和事件路由；调用者/窗口身份来自可信source |
| `workbench_services.rs`、`workspace_move.rs` | 打开/激活/移动/合并/关闭时维护选中对象，可能销毁源窗口时先检查草稿 |
| `app.rs`、`plugin_presentation.rs`、`plugin_dialogs.rs` | 原生呈现过滤、焦点/认证/模态、路由注册及退出接线；保持生命周期清理顺序 |
| 新 `window_close_review.rs`，调整 `settings_exit.rs` | 核心编辑页的关闭预检与最终提交，设置/连接草稿作为参与者，插件原有prepareClose继续适用 |
| 新 `crates/core/src/proxy_catalog.rs` | 带回执的代理save/remove/operation、分页list；复用配置锁和现有代理数据逻辑 |
| `proxies.rs`、`route_store.rs` | 抽出校验/写入/依赖查询供新旧入口共用；旧入口也推进目录失效通知 |
| 新 `configuration_receipts.rs`，调整 `server_catalog_store.rs` | 提取既有回执读写/容量机制，新增代理命名空间，服务器旧回执格式和查询语义保持 |
| 新 `credential_configuration.rs`，调整 `lib.rs`、`proxies.rs` | 无连接挑战的预设凭据增加条件提交与同次运行回执；原连接挑战API继续按原契约执行 |
| `crates/core/src/observability/builtin.rs`、`calls.rs` | 新proxy save/remove与credentials set纳入既有写调用分类及逐次留痕；只记录对象/阶段/结果，不记录秘密、文件内容、请求HMAC或完整路径；回归失败闭合/审计降级规则 |
| `communication_services.rs` | 统一目录revision发布、真实来源路由、feed清理；复用已有`servers.catalog.v1`失效信号 |

## 3. 窗口导航与原生表面

### 3.1 唯一权威模型

在WindowRecord内用一个判别联合表示选择：`ConnectionHome | Workspace { workspaceId } | Empty`。`homeOpen`、`homeGeneration`及`lastWorkspaceId`单独表达系统页是否在Tab中和返回位置。内部改用`selected_workspace()`访问器，避免selection和active_workspace_id两份可写真相；旧读取点逐个替换，历史序列化/测试输入仅做兼容投影。

核心不变量：

1. 一个工作台窗口至多一个连接页；系统页不进入workspaces/views，不注册工作区，不触发默认插件入口/首终端。
2. 选中系统页时旧`workbench.snapshot`返回`activeWorkspaceId=null`、`focusedViewId=null`、空dockSlots；保留workspaces/views的资源信息。当前工作区自己的focus_target、目录来源、布局不清空。
3. 打开服务器时只切换实际承载它的窗口到Workspace；其他窗口的系统页/草稿不被改写。已有工作区继续定位，新工作区继续初始化一次。
4. 移动服务器只移动工作区，系统页及草稿留在原窗口。若原子操作将销毁源子窗口，先完成§7预检；取消则移动/关闭均未开始。不能先移动再发现草稿无处保留。
5. 主窗零工作区时仍有连接页；子窗原有自动回收规则保持，最后工作区移走/关闭前处理该窗草稿，系统页不永久撑住空子窗口。
6. 完整启动只建立主窗系统页；同次renderer重新载入会失去未保存的内存草稿，显示明确恢复提示，不从磁盘恢复密码。页面会话代次重建使旧回执失去UI写入资格，已保存配置仍可读。

新增`ui.connectionHome.snapshot`在同一工作台锁内返回`{workbench: 原SnapshotResult, page: {open, selected, generation}, navigationRevision}`。外壳切换使用此原子视图，旧`workbench.snapshot`返回形状不变，不让旧SDK的严格校验失败。核心导航changed事件驱动刷新，按revision丢弃旧结果。

系统页open/close/导航不引入工作现场持久化；列宽/筛选仅本次运行。`WindowRecord`新的运行字段不写入窗口偏好文件。标题在系统页选中时明确更新为“新建连接 — SoDeck”，不能保留前一台服务器标题。

### 3.2 可见性、焦点与迟到布局

在`present`前统一计算`effectiveVisible = requestedVisible && owner/mount匹配 && windowSelection允许 && 非modal/关闭栅栏`。系统页激活提交后，宿主立即将该窗的非detached插件surface隐藏，发布visible=false，不等待React卸载；失败时返回可恢复错误，不先显示可输入的新页却留下覆盖的原生终端。

切回工作区按最新几何恢复；只有当前选中工作区、当前mount且用户焦点意图仍有效时才focus。迟到visible=true不得越过当前selection。系统页期间不重新装载插件、不触发0尺寸PTY resize、不销毁surface/lease；独立视图窗口属于另一窗口，继续可见。

`views.requestFocus`增加权威selection校验，隐藏surface不能仅凭旧geometry抢焦点。`auth.required`分发以及壳层接收均检查窗口当前选中的workspace；系统页期间保留原终端待处理认证状态，不弹出后台服务器认证。原生菜单/文件选择的focus返回绑定homeGeneration/draftId，页面已切换时不强行跳回。

插件模态打开期间切系统页沿用既有modal屏障，不能把未结束插件模态遗留在新页面上。普通系统页本身不是modal，不用永久`workbench.modal=true`代替页面语义。现有视图presentation协议仍表达可见/焦点，旧插件无需新增分支。

## 4. 草稿与操作状态

`HomeState`持有当前右侧模式、草稿索引、目录快照revision、搜索/分组、布局偏好。右侧模式为welcome/ssh/proxy/proxyManager/groupManager；内联代理只属于一个SSH草稿，用`parentDraftId`关联，绝不读“当前全局选中代理”回填另一个目标。

`SshDraft`保存`draftId, revision, serverId?, baseServerRevision, baseMembershipRevision, fields, selectedDependencyIdentities, fieldErrors, status, lastSavedId, returnIntent`。依赖身份取实际使用的主机/端口/账号/认证字段，忽略跳板自身accessPath、名称和分组，不能用整个节点revision误判无关修改。访问方式用判别联合，其他模式的非秘密输入可放局部stash，serialize只输出当前模式。显示名称采用`auto | custom`：新建空值为auto，已有服务器载入为custom，提交前生成真实name，不增加数据库auto标记。名字超200字符要求用户缩短。

`ProxyDraft`同样区分新建/已有；分组创建有独立操作ID与回执。编辑同一持久对象只有一稿，新建空稿重复点击复用，有修改时建立另一稿并保留原稿。已提交成功但仍可编辑的页面作为clean baseline，不计入“未完成配置”。

秘密缓冲按`draftId + authKind`保存密码、私钥路径、口令、remember与显示状态；不进入可序列化reducer事件、目录cache、state feed、日志、localStorage或URL。UI受控输入读取这一份内存，切页默认重新隐藏明文；切认证清除离开的秘密，明确取消/保存完成/窗口销毁清除引用。JS字符串无法承诺物理擦除，文案不声称安全抹除内存。

每个异步任务捕获`homeGeneration, draftId, draftRevision, operationId, navigationRevision`。响应只推进对应草稿；切到另一个表单不改变响应归属。合法保存仍完成，用户已切走时只显示该草稿状态，不抢焦点、不自动调用新的workspaces.open。保存并连接的前台导航若已失效，改为“已保存，可连接”；已开始的workspaces.open按工作台操作回执处理，绝不为了前台切换创建第二个ID重放。

### 4.1 提交状态机

统一状态：`editing → validating → savingConfiguration → savingCredential → saved → opening`；失败按阶段进入`validationError/configurationError/credentialError/reconciling/conflict`。不以单个busy布尔和try/catch字符串推断实际提交阶段。

- 点击保存冻结当前提交payload/依赖身份及秘密意图；禁止同稿重复提交和修改冻结字段，其他草稿/服务器仍可用。
- 服务器先调用原`servers.catalog.save`。提交成功即保存serverId与baseline，即使后续凭据失败也不再调用create。`postCommitPending`按原回执处理完毕才能进下一阶段。
- 未提供新秘密时不写凭据，保留已有绑定或按连接挑战补充；不能把留空解释为删除。用户提供了秘密才进入条件写入。
- 明确失败可修改字段后生成下一操作ID；TIMEOUT/RESULT_UNKNOWN只核对原操作，不放开“再次创建”。未知结果时保留精确请求，凭据不写进持久配置回执。
- 配置与秘密都确认后，onlySave进入clean编辑态；saveAndConnect通过公共workspaces.open创建/定位工作区。现有编辑始终saveOnly。
- 内联代理有自己的这套提交状态机；完成后只对原parentDraftId选择代理。父稿已明确放弃时代理保留，结果不附加到其他草稿；内联未保存/结果未知时不能提交父稿。

## 5. 核心契约增量与数据兼容

计划API/SDK 5.3增量，能力逐项声明，不改当前三种accessPath。T-023的5.1与T-024拟5.2为并行任务基线；合入时保持版本单调且只声明已实现能力，不以升到5.3声称已实现全部5.2方案。本次连接页不依赖资源监控插件，也不要求重打现有终端/文件/网络包。

### 5.1 系统页与可信来源

新`contracts/connection-home-v1.schema.json`定义`ui.connectionHome.v1`；schema共用已有workbench定义并在生成时组合/去重，产出SDK类型及Rust校验，禁止手写两份DTO。

| 计划方法 | 参数 / 结果与责任 |
| --- | --- |
| `ui.connectionHome.open` | operationId、可选targetWindowId、intent（home/newSsh/newProxy/editServer/manageProxies/manageGroups）；宿主解析合法工作台，返回代次与navigationRevision；editServer只传ID，由目录读当前值 |
| `ui.connectionHome.snapshot` | 可信来源窗口；返回§3原子快照，不接受任意伪造serverId |
| `ui.connectionHome.session.attach` | 客户端启动标识，宿主为当前壳文档发行pageSession；新renderer替代旧session，旧report/respond失效；不自动覆盖仍活跃的另一窗口会话 |
| `ui.connectionHome.session.report` | pageSession、draftRevision、dirtyCount、在途operationId摘要；不传字段/秘密，只用于关闭预检提示 |
| `ui.connectionHome.close.request/respond` | 请求含reason；回应含ticket/pageSession/draftRevision/decision；实际关闭由§7票据协调，不因respond成功就退出App |
| `ui.connectionHome.pickPrivateKey` | pageSession、draftId、requestId；返回selected绝对路径或canceled，不读出文件内容；一次只开一个选择器 |

open回执每窗口最多256条/10分钟，回执过期不可自动恢复已关闭旧页面；snapshot/生命周期幂等不产生资源。原生`rfd`依赖已存在，选择器绑定所属窗口，取消恢复触发焦点；选到不可读/超大文件在提交时由核心校验并给出字段错误。

来源检查只证明正确窗口/代次/资源归属，不新增插件权限审批。插件仍可调用已支持公开能力；系统页session/close回应只能由宿主发行该session的壳文档提交，与原生生命周期消息边界一致。

### 5.2 可靠代理目录

新增`contracts/proxy-catalog-v1.schema.json` / 能力`proxies.catalog.v1`，保持旧`proxies.socks5.*`返回形状。新接口：

| 方法 | 核心规则 |
| --- | --- |
| `proxies.catalog.list` | query/cursor/limit≤100，响应items/catalogRevision/nextCursor；每页≤32KiB，项含代理公开字段与保存配置引用数，不含秘密/运行连接明细 |
| `proxies.catalog.save` | operationId、可选proxyId/expectedRevision、完整configuration；新建缺proxyId，更新必须带expectedRevision；响应proxy及目录revision |
| `proxies.catalog.remove` | operationId、proxyId、expectedRevision；配置/运行依赖检查沿用现有逻辑，成功响应deleted；回执重放不再次删同ID |
| `proxies.catalog.operation` | operationId，返回unknown或committed及对应save/remove结果；不能将别的方法/调用者回执作为当前结果 |

复用现有`server_catalog_receipts`的列和JSON容器，抽出内部通用帮助函数；代理存储键使用明确命名空间，服务器旧原始operationId仍可读。proxy result含辨识类型，旧servers.catalog.operation遇到非服务器回执仍拒绝，不让它误解为服务器。请求hash包含方法/完整非秘密payload，重用ID但内容不同返回OPERATION_CONFLICT。

在configuration锁内，代理配置、目录revision和回执在同一SQLite事务中提交；保存成功丢响应时查询同回执取得同proxyId。端点/账号变化的秘密清理保持原“先清理再发布新身份”安全顺序：清理失败不提交新配置；清理后数据库失败可能导致旧配置需重新输入密码，明确返回该部分影响，不宣称完全回滚秘密。

旧create/update/delete仍走共用校验/依赖/目录失效逻辑；旧create不因此获得未声明的幂等承诺，新UI统一使用新接口。

回执沿用现有总量1024、单条8KiB和已完成24小时清理规则，写前预留容量，未决清理不删除；服务器和代理共享上限。前端记录请求开始时间，超过核对窗口后只读核对目录，不把unknown自动重发为新建。旧代理上限10000保持。

### 5.3 预设凭据的条件提交

新增`contracts/credential-configuration-v1.schema.json` / 能力`credentials.configuration.v1`，专用于保存配置时的凭据，不改变SSH握手挑战的`auth.ssh.set/auth.socks5.set`及主机信任流程。

1. `credentials.configuration.prepare`接收target（ssh serverId或socks5 proxyId）、expectedOwnerRevision和expectedDependencies（显式跳板/代理的ID与实际身份字段）。SSH依赖为host/port/username/authType；代理为host/port/authType/username；核心要求依赖集合与当前路径恰好一致，不递归节点自身路径，也不因名称/分组变化失效。核心在配置锁内核对目标及依赖、取当前凭据revision，返回绑定这些事实的`contextToken`及是否已有有效已记住凭据；不读回秘密。
2. `credentials.configuration.set`接收operationId、contextToken、remember及密码或privateKeyPath/passphrase。核心在同一配置锁内重新核对token、代次和凭据revision，再读文件/保存；并发修改先返回REVISION_CONFLICT，不把旧草稿秘密写到新账号/路径。token不是新的网络身份信任，不绕过首次指纹确认。
3. `credentials.configuration.operation`只返回unknown/running/stored/failed及非秘密结果，供丢响应后核对。凭据写回执仅在本进程内存，按可信Caller+operationId绑定；每次启动新代次，不能跨进程重放秘密。
4. 重试前先查回执，再做条件核对；已stored返回原结果，不因自己上次成功推进凭据revision而误报冲突。请求比较用本进程随机key的HMAC覆盖秘密意图，只保存摘要/结果，不将密码或可离线字典攻击的普通摘要写到磁盘、事件或日志。
5. 旧挑战写成功也推进现有凭据revision，使旧prepare失效。配置与凭据阶段分离的部分成功向用户明示。私钥仍由核心读取且≤1MiB；文件选择不改变已有密钥存储方式。

同次运行凭据回执最多1024条，已完成保留24小时；容量不足在写前拒绝，running不淘汰。prepare票据最多256、5分钟，使用/超时回收；秘密已输入但票据过期时可重新prepare，仍须按冻结依赖核对，不能静默接受变更。存储后目录失效发布失败作为pending副作用重试，不重新轮换秘密或误报“未保存”。

### 5.4 目录同步与数据库

`contracts/server-catalog-v1.schema.json`的CatalogSave增加可选`expectedDependencies`，本次新UI始终提供；旧调用方省略时保留原语义，响应形状不变。`server_catalog.rs`在配置锁内、写事务前按§5.3的身份投影检查，变更时返回依赖冲突，草稿保留；没有预设密码也不能绕过此检查。提交回执hash包含该条件，重试先返回原提交结果，不拿当前依赖否定已经完成的保存。这个增量与生成器/SDK校验一并纳入HCODE-05/07/09，不另升数据库版本。

沿用`servers.catalog.v1`的`catalogRevision`作为整个连接配置目录的失效版本。服务器/组/代理写入、影响hasSecret展示的凭据提交均推进revision；已有shape不变，旧消费者可能多一次刷新但结果不被破坏。宿主将提交完成后版本发布到现有state feed；事件丢失通过observeState快照/恢复补读，回执重放不重复推进revision。

连接页一次载入完整的有界服务器/分组/代理集合时，所有分页须来自同revision；变化后丢弃整批，最多重试3次，仍变化则保留旧列表并提示同步中/可重试，不展示半新半旧依赖。引用数来自同事务中的connection_path_refs；删除依赖由核心重新核对，UI数字不是删除许可。新列表无需每2秒全量重读，不把当前App的工作台轮询一并扩成全局重构。

**业务库保持v6，不增表、不更名、不迁移、不备份重建空库。** 复用回执/目录元数据与现有JSON容器。已有服务器/代理/分组/凭据照常可读；新UI的默认名称只在新建提交时生成，不批量改老名称。UI偏好/草稿不新增持久文件，T-023的外观v3兼容由它自己的已授权任务维护。

API5.0～5.2现有插件仍按原契约运行；只使用新增能力的调用方声明新能力/版本。生成器、SDK测试与宿主注册一起更新；不为了新壳层功能强制把三业务插件manifest全部抬高。旧核心不能承载新外壳，完整桌面包配套升级，不能静默落回已舍弃的旧弹框。

## 6. 公共UI与表单实现

已读取并继续应用apple-design。现有`Field/TextField/SelectField/Checkbox/RadioGroup/InputGroup/FormSection/Stack/Inline/ScrollArea/Surface/Notice/ListRow/TabItem`优先复用；单选访问方式使用有label的radio语义，不能把没有对应tabpanel的选择器伪装为导航Tab。

本次只补真实复用的`PasswordField`（显示开关、label/help/error一体）和可搜索单选`SearchSelect`（SSH节点/代理/组共用，空/加载/错误/键盘完整）；编辑页头/动作条若T-023已提供，直接复用其公共变体。新增组件放`packages/ui/src/components/`，只收数据/回调，没有SDK。内联代理作为section，不在外层form里嵌另一个form；按钮显式type=button，独立校验/提交scope。

尺寸/控件状态参数进入foundation/schema，通过既有token生成器产生TS/CSS；业务layout.css仅排列列宽、响应式和滚动。颜色只消费现有语义令牌，不按themeId分支。三主题几何一致，焦点不改盒尺寸，布局切换不对原生视口祖先做transform。减少动态/透明与高对比按公共规范处理。

列表首期使用同一公共行组件与可见范围渲染，按系统字体测量行高；只保留视口及上下各8行，键盘选中行在挂载后聚焦，完整搜索在目录cache中完成。实际渲染不把10000代理项全放进DOM。选择器仅展示已保存ID，输入搜索词不能产生临时服务器。

布局测量单位统一CSS逻辑像素；左列280、范围240～360，右正文≤720；窄屏折列，真正不足时切换列表/表单，draftId不变。标题/操作条参与布局，字段容器min-height:0并独立滚动。保留系统字体缩放、IME及焦点返回，在普通Enter/组合输入时阻止隐式提交，Ctrl+Enter调用当前提交意图一次。

## 7. 关闭、退出与并行设置任务

新增`window_close_review.rs`只协调现有核心编辑表面（连接页、T-023设置页）及工作台动作，不建设插件草稿平台。参与者内容/秘密仍由自己的renderer持有；宿主只记可信来源、session、dirty revision、在途操作及准备票据。

统一走两阶段：

1. 请求关闭系统页、销毁工作区窗口、关闭/移动其最后工作区、合并源窗口或明确退出时，确定受影响窗口集合并发行ticket。主窗×收托盘无需检查；仅切服务器不检查。
2. 对相关活跃renderer发送prepare，renderer立即冻结该轮编辑/新提交并回报最新draftRevision和在途操作；不能只相信上次异步report，漏掉最后一次输入。
3. clean自动允许；dirty只显示继续编辑/放弃修改。用户选择放弃此时只给许可，暂不清稿/销毁，直到其他参与者和原插件prepareClose也通过。任一取消则全部解除冻结，已给许可的草稿仍然存在。
4. 有保存任务时先核对，10秒仍未决则允许用户选择继续核对或明确放弃等待并关闭，说明已开始保存可能完成；时间到不默认同意。提交已在core持有，renderer消失不会把它重发/回滚，也不会迟到自动打开工作区。
5. 最终核对ticket、session、draftRevision、目标窗口/工作区代次及动作仍有效，设置提交栅栏，清稿并执行原关闭/移动/shutdown一次。等待renderer、用户或worker期间不持workbench/configuration/shutdown锁。

3秒无renderer回应转原生可见提示，默认继续编辑/保留；新输入、其他关闭或页面换代使旧票据失效。全App退出先收齐所有窗口与设置许可，再做任何不可逆关闭；旧settings_exit的“先销毁设置再递归app.exit”在此路径改为参与者提交，不允许两个独立单次许可相互绕过。

`app.exit`在尚待核对时返回明确`CLOSE_REVIEW_REQUIRED`（必要时识别旧SETTINGS_REVIEW_REQUIRED），不得返回成功，因为当前dispatch会在成功后安排进程退出。设置窗口独立关闭仍复用原ui.settings契约；其功能/SCODE-07须作为本次集成回归，不能因本页重构破坏已授权设置任务。记录本稿设计所需适配，不宣称在途设置实现已经通过。

## 8. 容量与异步边界

| 项目 | 本次约束 |
| --- | --- |
| 草稿 | 每窗口最多16个未完成主草稿（内联代理计入父稿预算），非秘密总量≤512KiB，秘密输入总量≤256KiB；超额先拒绝新建并定位已有草稿，不淘汰用户内容 |
| 页面/关闭 | 每窗口一个连接页、一个文件选择器；每目标动作一个关闭ticket，重复请求定位当前确认；票据取消后迟到回应无效 |
| 目录 | 每页≤100/32KiB；cache非秘密总量≤32MiB，超限明确报容量，不截取后冒充完整目录；服务器/代理上限沿核心现有规则 |
| 保存 | 每持久对象/草稿最多一个在途保存；全应用新增配置/预设凭据写最多8个，超出排队或明确BUSY；同ID重送不占新执行槽 |
| 回执 | 配置回执共享既有1024/8KiB/24小时；凭据回执内存1024/24小时，prepare256/5分钟；未决不因容量回收 |
| 导航/查询 | 页面导航operation回执256/10分钟；目录失效补读合并，读取失效最多重试3次；查询不连接任何远端 |

数量和字节在创建/接收前校验；秘密大小错误就近显示，不写日志内容。达到上限不创建第二套隐式持久草稿或清理别的窗口。列表规模与预算分别测试，模拟规模不冒称真实打开相同数量SSH。

## 9. 测试、关口与实施顺序

功能CH-01～13仍是验收来源；以下HCODE测试补代码边界。准备独立`.qa/t025/`、当前v6数据库副本、测试SSH/多跳/SOCKS及真实当前插件包，另用兼容旧包验证接口投影。所有新文件名是计划，不运行并不存在的脚本。

| 用例 | 准备与步骤 | 必须观察到的结果 | 层级/清理 |
| --- | --- | --- | --- |
| HCODE-01 导航不变量 | 主/子窗开系统页，连续＋、开A/B、跨窗定位；模拟打开回执迟到；关闭/移动最后工作区 | 无伪workspace/view；home时activeWorkspace为空；系统页不参与拖动索引；迟到结果不抢当前选择 | Rust模型/契约+UI；清窗口与工作区 |
| HCODE-02 原生可见性 | 真实终端输入中切home；延迟旧visible=true/焦点请求；打开文件选择/认证/插件模态再切换；回A | 终端不覆盖表单、不收新输入；PID/PTY/lease/mount保持，无0尺寸resize；后台认证不弹 | Windows原生+真实SSH；取消挑战、释放资源 |
| HCODE-03 草稿与秘密 | 两SSH稿/代理内联/编辑同对象切换；新建模式/字段错误/名称超限；切认证、保存/放弃/renderer重载 | 一个权威草稿，内联回正确父稿；秘密无普通state/日志/磁盘副本；重载如实丢未存状态 | reducer/UI/日志检查；清测试秘密 |
| HCODE-04 代理可靠写 | 新建提交成功丢响应、原ID重试、不同payload重用ID；update冲突、remove依赖、回执满/过期/重启 | 同proxyId不重复建；过期不盲重放；引用阻断、容量在写前；旧create返回形状保持 | Rust+RPC故障注入；清消费者再删代理 |
| HCODE-05 凭据条件写 | prepare后另一窗修改地址/账号/跳板/代理/秘密；私钥选择取消/文件消失/超1MiB；写成功丢响应；核对正常/失败/重试审计 | 不向变更身份存旧秘密；stored回执可查、重试不重复轮换；秘密/文件错误不冒充配置失败；逐次留痕且不含秘密/路径/HMAC | 核心+原生文件+真实认证；清票据/密钥 |
| HCODE-06 保存编排 | 配置成功后秘密失败，组/代理已保存后取消父稿，保存时切页/关闭，workspaces.open丢回执 | 同配置继续、部分结果真实；仅保存0工作区；凭据未确认不连接；原打开operation核对 | UI+核心/工作台故障；撤销故障并核对数据 |
| HCODE-07 跨窗目录 | 页间写入导致cursor失效；A编辑时B删组/跳板/代理；feed断开/重连；旧API写入 | 原子revision批次、无半新半旧或静默覆盖；旧API也通知；恢复不清草稿/无2秒全量目录轮询 | state契约+UI；清fixture数据 |
| HCODE-08 两阶段关闭 | 同时有设置草稿及两个工作台草稿；一处允许一处取消；最后工作区移窗/合并；保存未决/renderer失联/重复退出/旧票据 | 任一取消无提前销毁/丢稿；最新输入受保护；超时不当同意；最终只shutdown一次；主窗×仅托盘 | Windows原生+SCODE-07回归；核对进程/资源，恢复数据副本 |
| HCODE-09 契约/数据 | 新旧SDK解析snapshot与原代理/auth结果；旧v6数据打开、运行新保存后旧API读；生成器与边界检查 | 原DTO不添未知字段；现有插件不强制升级；DB仍v6、无重建提示，秘密无配置回执 | 契约/Rust/包检查；保留副本作前后对照 |
| HCODE-10 UI/规模 | 三主题/900×600/窄窗/DPI/IME/高对比；大目录、16稿及字节上限、反复切页100次 | 操作条/焦点可见、无nested form、列表DOM有界、超限不丢内容；订阅/窗口/表面回到基线 | 浏览器视觉+Windows原生；恢复显示与测试目录 |
| HCODE-11 连接消费者 | 由新页分别打开直连/显式J1→J2→T/SOCKS目标，终端+文件+网络并用，切home、独立视图、重连/退出 | 原路径/指纹/账号正确、一个首终端；隐藏不关其他资源、不串cwd；退出资源归零，外部代理继续运行 | 真实SSH/SOCKS+原生；停自建服务、清端口/数据 |

预计新增：`tests/ui/connection-home.spec.ts`、connection-home模块的model/save测试、`crates/core/tests/proxy_catalog.rs`、`credential_configuration.rs`、`tests/desktop/connection-home.mjs`及生命周期用例。具体测试文件可按模块合并，不为简单静态字段镜像实现写重复测试。

修改现有`tests/ui/server-tabs.spec.ts/server-groups.spec.ts/workbench.spec.ts`及`tests/desktop/clean-start*.mjs/server-tabs*.mjs/socks-route.mjs`中的旧弹框入口，保留其真实资源/认证/边界断言；其他旧测试如通过公用helper找入口，集中改helper，不机械删除所有旧选择器测试。

实施关口：

1. **契约和核心可靠提交**：生成新schema/SDK/Rust，完成代理回执/分页、凭据条件写、目录revision；HCODE-04/05/07/09通过。
2. **系统页及关闭基础**：权威选中对象、原子快照、表面/焦点过滤、关闭协调与设置适配；HCODE-01/02/08通过。
3. **草稿/服务/表单**：先纯模型与提交协调，再目录、受控字段与管理视图；HCODE-03/06和CH主流程通过。
4. **统一接线与退役旧UI**：所有入口迁移，删除旧Dialog组件/递归steps/无消费者CSS和进口；只保留真实认证/删除确认；完成三主题/规模检查。
5. **原生与消费者交付**：执行HCODE-10/11及CH全流程，更新任务证据，生成独立Windows目录包供验收。包只包含已就绪插件，记录core/插件哈希和配置保留；目录包运行不等于安装验证。

现有检查入口可用：`npm run check`、`npm run test:contracts`、`npm run test:core`、Playwright UI、`npm run build`和`npm run desktop:build`。实现中先跑受影响层，交付前跑约定回归；本轮设计没有运行这些应用检查，也未生成新脚本/原型。

## 10. 交付与接续

代码方案r1与功能r2作为本次整体评审基线。确认覆盖后，内部命名、函数拆分、定位缺陷及不改变行为的校验细节自主完成；新增协议组合、持久草稿、清库/API破坏或改变窗口结果仍需先更新方案。

与并行T-023/T-024的共享文件只作必要接入，实施前核对最新代码/能力号，不覆盖已有变更、不恢复旧版本。代码设计与文档检查的实际结果在T-025记录；工程完成、用户验收、发布分别处理，不自动提交/推送。

# T-015 代码修改设计方案

日期：2026-10-27 · 方案修订：r2 · 文档就绪：可评审

任务：[T-015 r3](../tasks/T-015-workflow-polish.md) · 交互范围：原13项反馈及P-52数据库备份重建

状态：2026-10-27用户已确认实施；本文保留编码前的实现基线，实际完成情况、版本及分层证据见[T-015结果](../tasks/T-015-results.md)，不将设计清单本身视为验证结果。

## 1. 实施边界与核实结论

设计时基线为2026-10-27 `v2`未提交工作区，保留此前T-008～T-014改动。当时核心`2.0.0-dev.11`、API/SDK`4.7.0`、公共UI`1.1.0`，终端`0.7.1`、文件`0.4.1`、网络隧道`0.1.0`。此处记录编码前源码核实，不把旧测试结果当新方案已验证。

| 核实位置 | 当前约束 | 本次处理 |
| --- | --- | --- |
| `crates/core/src/store.rs` / `route_store.rs` / `route_services.rs` | SQLite user_version=3；档案和路径同事务；配置保存会推进服务器revision，认证相关变化会清理凭据 | 当前结构改为v4；版本不一致先提示、备份、全新建库；分组单独存储/修订，复用配置事务内部方法 |
| `crates/plugin-manager/src/package.rs` | manifest/contributions/命令/视图均deny_unknown_fields | 新元数据有显式字段和能力声明；新包不能冒称兼容旧核心 |
| `contracts/workbench-v4.schema.json` | 命令及工作台结果严格校验 | 新增独立查询，不给旧返回结构塞未知字段 |
| `workbench_services.rs` / `plugin_presentation.rs` | 只创建openByDefault入口；当前工具轨从已打开视图生成 | 工具入口从声明生成，首次点击再创建；无规则时也能找到入口 |
| `native.rs` / `surface.tsx` | 插件是独立子WebView；普通React Dialog只在本表面内可见 | 新增公共插件模态承载，表单留在插件，宿主处理原生层级/焦点 |
| `plugins/network-proxy/src/rules.ts` | Rule拒绝未知字段且name必须非空；外层Collection保留schemaVersion=1 | 可选输入在worker归一为非空标题，自动命名标记放外层可选元数据，保持旧规则可读 |
| `plugins/files/src/view.tsx` | 所有行用同一rowHeight计算滚动/定位 | 容量行先明确高度单位，再共同修改虚拟窗口和定位，不能只加CSS |
| `app.rs::bootstrap_paths` / `main.rs` / 打包脚本 | 依赖环境变量、可能回退源码资源；不带环境时未完整发现网络插件；debug EXE会带控制台 | 统一启动配置、三插件包索引、GUI子系统及独立目录交付 |

所有路径下文均相对仓库根。内部文件拆分可微调，公开接口、数据备份重建、交互、资源与验证范围按本文实施。无Git提交、推送或正式发布授权。r2依据P-52取消r1的SQLite增量迁移设计；插件文件格式及公开API的兼容约定保持。

## 2. 模块划分与13项覆盖

| 问题 | 修改/新增位置 | 职责 |
| --- | --- | --- |
| 1、2 | `packages/ui/src/components/controls.tsx`、`styles/components.css`、`tokens/foundation.json`；`apps/desktop/src/server-dialog.tsx`、`styles.css` | 公共内侧焦点及组合输入框；外壳只负责字段布局、认证草稿和保存流程 |
| 3 | 新增`crates/core/src/server_catalog.rs`、`server_catalog_store.rs`；`store.rs`、`route_store.rs`、`route_services.rs`；外壳新增`server-groups.tsx`、`server-catalog-client.ts`，修改`server-picker.tsx` | 核心拥有分组和原子保存；外壳管理筛选、表单与分组页 |
| 4 | `plugins/terminal/src/terminal-view.tsx`、`terminal.css`，新增`terminal-geometry.ts` | 隔离尺寸测量、可见性恢复、fit及PTY resize调度，不改SSH/PTY语义 |
| 5 | `crates/plugin-manager/src/package.rs`、`store.rs`；`plugin_registry.rs`、`management-dialog.tsx`、三份manifest | 经验证的显示名与兼容回退，ID继续作为身份 |
| 6 | 从`management-dialog.tsx`抽出`call-records-panel.tsx`及查询hook；对应组合样式 | 固定筛选/刷新，结果单独滚动，保留游标、去重和代次隔离 |
| 7 | `workbench.rs`、`workbench_services.rs`、`plugin_presentation.rs`；外壳新增`workbench-tools.tsx`；网络插件`view.tsx`、`editor.tsx`、`details.tsx`、`rules.ts`、`manager.ts`、`worker.ts`、`manifest.json` | 宿主通用停靠/入口，插件拥有列表/创建/规则与运行状态 |
| 7弹窗 | 新增宿主`plugin_dialogs.rs`、`surface_endpoint.rs`；`native.rs`、`app.rs`、`window_services.rs`等生命周期接入；SDK新增`dialogs.ts` | 原生临时模态表面，不新增逻辑视图、worker或业务资源 |
| 8 | `apps/desktop/src/app.tsx`及`workbench-tools.tsx`；终端manifest | 通用创建动作映射Tab末尾＋，沿用terminal.new |
| 9～11 | 文件插件`worker.ts`、`state.ts`、`view.tsx`、`files.css`；新增`capacity-cache.ts`、`row-layout.ts`、`mount-capacity.tsx`；退役`details.tsx`的展示部分 | 始终显示隐藏项、加载反馈、按需容量、虚拟布局和链接动作 |
| 12 | 公共新增`WindowControlButton`/`BusyIndicator`；外壳`window-controls.tsx`、`styles.css`、`window_chrome.rs` | 同一按钮盒尺寸/命中，保留原生操作；不让主题改外部几何 |
| 13 | 新增`startup_config.rs`、`startup_diagnostics.rs`；`main.rs`、`app.rs`、`scripts/desktop.mjs`；新增`scripts/package-polish-dev.mjs` | GUI启动、资源/数据定位、错误诊断和独立目录包；复用既有签名/宿主 |
| 数据库补充 | 核心新增`store_bootstrap.rs`，修改`store.rs`、`lib.rs`；外壳`app.rs`与`startup_diagnostics.rs`接启动提示 | 核心负责版本探测、一致备份、全新初始化与恢复；外壳负责原生提示，不让存储层依赖UI |

`@sodeck/ui`不依赖SDK或业务；SDK负责公开契约/呈现通信；插件不导入外壳组件、数据库或其他插件。只抽取本次真正共享的保存事务、表面回执路由和原生窗口层级辅助，不重写整个app.rs或插件运行时。

## 3. 公共契约与版本

### 3.1 建议版本

本轮核心`2.0.0-dev.12`、API/SDK`4.8.0`、UI`1.2.0`；终端`0.8.0`、文件`0.5.0`、网络隧道`0.2.0`。三个新包统一声明API`^4.8.0`及实际所需能力，独立构建、签名和重启启用；旧包在新核心继续使用旧协议。若实施时工作区版本已前进，先记录真实基线并顺延版本，不覆盖现有版本内容。

新增能力：`servers.catalog.v1`、`plugins.metadata.v1`、`workspace.contributions.v2`、`ui.dialog.v1`；不更改网络数据面或现有workbench.v4版本，不新增插件授权机制。

| 新契约来源（拟建） | 内容 |
| --- | --- |
| `contracts/server-catalog-v1.schema.json` | 分组、成员归属、分页、原子保存、错误与目录修订状态 |
| `contracts/workbench-ui-v1.schema.json` | 展示贡献v2、插件展示元数据、徽标、模态请求/结果/通知 |
| `scripts/generate-server-catalog.mjs` / `generate-workbench-ui.mjs` | 同源生成Rust/TS类型与校验数据，提供--check并接入npm run check |
| SDK `server-catalog.ts` / `workbench-ui.ts` / `dialogs.ts`及生成文件 | 严格请求/响应/事件校验、查询及模态辅助；生成文件不手改 |

现有`state-feed-v1.schema.json`/通信生成器只追加catalog查询目标与值类型，Hub方法/信封保持；旧订阅者不会收到未请求的新契约。`communication_services.rs`为app作用域新增独立分支，不把catalog scope误套工作区代次校验；模态feed投递通过endpoint路由。SDK入口接入新校验器与事件校验，既有请求未知字段规则不放宽。

旧`servers.*`、`servers.routes.v2.*`、`plugins.list`、`workbench.snapshot`、`workbench.contributions`返回投影保持原样。新元数据使用新方法，不让旧SDK因additionalProperties=false报错。

### 3.2 manifest增量

顶层可选`displayName`；`contributes`新增可选`presentation`，不直接给CommandDeclaration加展示字段，避免其被旧snapshot序列化出去。提案形状如下：

```json
{
  "displayName": "网络隧道",
  "contributes": {
    "presentation": {
      "tools": [{"viewType":"network.manager","commandId":"network.open","edge":"right","icon":"arrow-left-right","defaultWidth":360}],
      "tabCreate": [],
      "dialogs": [{"id":"network.rule-editor","title":"创建网络隧道","width":560,"height":520}]
    }
  }
}
```

示例仅展示增量，不是一份可用manifest。终端声明`tabCreate:[{commandId:"terminal.new",icon:"plus"}]`；文件声明左侧工具，已有默认展开保持。图标使用SDK约定的有限语义枚举，宿主映射图形，不接受HTML、任意SVG/路径或插件代码。未声明时保留现有旧入口。

校验：引用命令/视图必须属于该插件，创建/工具命令为server作用域且无文本输入；tool边必须被视图支持；同一viewType只声明一个工具入口；单插件本轮最多一个tabCreate。整个App只有一个可用创建动作时直接＋，多个插件声明时＋打开带中文名称的创建菜单，不默默选一个；当前仅终端声明。dialog ID在包内唯一，尺寸有限且实际显示按窗口夹紧。displayName去首尾空白、1～64 Unicode字符、无控制字符。

新`workbench.contributions.v2`按已验证包返回`pluginId`、命令/视图基本信息及presentation；旧方法显式投影旧字段。`plugins.metadata.list`在旧PluginStatus信息之外返回`displayName`、`pendingDisplayName`，当前标题只取已激活包。旧包优先取唯一主视图标题，不唯一则回退ID，不能逐个硬编码官方ID。

## 4. 服务器分组与表单

### 4.1 数据模型、并发与备份重建

本次支持的SQLite结构版本为v4，从当前完整建表定义初始化，包含以下分组数据；不提供v3→v4迁移或旧字段兼容读取：

- `server_groups(id, name, name_key UNIQUE, revision)`；稳定UUID。name_key由核心执行trim→NFC→Unicode小写→NFC；添加轻量`unicode-normalization 0.1`依赖并锁定版本。界面只作辅助校验，不能另定义唯一性。
- `server_group_membership(server_id PRIMARY KEY REFERENCES servers ON DELETE CASCADE, group_id NULL REFERENCES server_groups ON DELETE SET NULL, revision)`。同版本库中经旧API创建的服务器缺行时视为`groupId=null, revision=0`，第一次赋组时建行。保留归属行可检测并发，不用删除行代表普通移出组。
- `server_catalog_revision`存于核心settings专属键，目录变更在同一事务递增，用于分页和通知，不作为SSH路径修订。
- `server_catalog_receipts(operation_id PRIMARY KEY, caller_key, request_hash, result, created_at)`：保存成功写的回执，和配置同事务。每条≤8KiB、最多1024条，保留24小时，按时间清理；过期回执不保证重放去重，UI结果不明时先按已知ID查询，不生成新operationId盲目再建。

依据[P-52](../product.md)和[启动规格r5](../core/startup-settings.md)，数据库启动分为探测、提示、备份、准备和启用，不先调用会修改旧库的Store::open：

1. StartupConfig确定数据根并取得既有app.lock；核心启动预检先处理未完成的重建记录，再以只读SQLite连接读取core/core.sqlite的user_version。仅比较结构版本，不比较应用、API或插件版本；缺库且没有待恢复记录时直接初始化v4。同版本正常打开并保留数据，不备份、不重建；已有库version=0也按不一致处理。无法读取版本/数据库损坏不作为首次启动，报错并保留原文件。
2. 版本低于或高于4均由外壳在打开Core、加载worker之前显示原生提示：“数据库版本与当前程序不一致。将备份旧数据并创建新数据库；原服务器、凭据、信任记录及数据库设置不会自动导入。”附检测版本、目标版本与备份位置。使用单次告知提示，关闭提示后按本流程继续，不再增加迁移选项或二次审批。
3. 提示前确定唯一操作ID与目标路径，开始备份前持久记录本次操作；在数据根backups/core-schema-reset/<时间>-<ID>/写备份。通过SQLite backup API获取包含已提交WAL内容的一致core.sqlite，不仅复制主文件；加密secrets原样备份，不解密。记录版本、时间、文件清单/哈希；关闭备份连接、落盘并验证SQLite完整性及凭据副本后标记backupVerified。备份目录失败、磁盘不足、锁冲突或校验失败均停止启动；失败副本不冒称完整备份，旧库及凭据继续保留。
4. 在同卷独立暂存目录创建完整v4库、空secrets目录，事务建表/索引和user_version；验证新库结构及完整性。只有完整备份和新库都验证通过后才进入替换。启动日志与原子写入的重建记录置于数据库外，记录操作ID、阶段、路径及已完成文件步骤，不记录明文凭据。
5. 启用时先关闭所有相关SQLite连接，将旧core.sqlite及其-wal/-shm和旧secrets移入本次保留区，再发布新库及空secrets；不能让旧WAL或凭据与新库混用。各步骤可按操作ID重入，重建记录在每步前后落盘，旧文件和已验证备份在完成后仍保留，不自动清理旧备份。仅在新库/凭据目录整体就绪并记录committed后，才打开Core和worker。
6. 备份或新库准备阶段失败时原活动数据保持可用，停止本次启动。替换阶段失败或进程被终止时，下次持锁启动先根据记录和文件校验完成同一次启用，或恢复整套旧库/凭据并报错停止；不能把中途缺失的主文件误认成首次启动，不能无记录混搭或再循环备份。恢复过程有可见提示；记录/备份损坏时保持全部文件、报出位置并停止。成功后再次启动同版本不重复提示或重建。

重建边界已核实：

| 数据位置 | 重建结果 |
| --- | --- |
| core.sqlite中的服务器、路径、分组、主机信任、调用记录、settings和操作回执 | 新库从空业务数据开始，写入当前结构与必要默认值；不导入旧记录 |
| core/secrets | 旧加密凭据与数据库配套备份/保留，当前凭据目录为空；新服务器需重新填写凭据及确认主机指纹 |
| plugins下的包、激活记录和storage.get/set私有文件 | 保留，不因核心库重建删除；如旧规则引用已不存在的serverId，不自动挂到同名新服务器或启动，继续按既有服务器归属隔离 |
| appearance-preferences.json、window-preferences.json、日志、既有备份等独立文件 | 保留；不将数据库重建扩大为删除整个data目录 |

当前storage.get/set写插件独立数据文件；公开settings.get/set写核心数据库，因此后者会重置，不能笼统承诺“所有插件配置保留”。备份是留存和人工恢复材料，本次不新增导入/恢复界面，不改user_version伪装兼容。今后采用同一策略的程序不论升级或降级均在结构不匹配时备份建新库；历史旧程序仍保持自身行为，本次不回改旧二进制。需要查看旧数据时只能用支持该结构的程序和独立备份副本，不把恢复旧数据作为运行当前程序的前提。

删除非空组先以catalogRevision确认用户看到的成员集合未变化；事务内将成员置null并递增membershipRevision，再删组、递增catalogRevision。不会删除服务器、改其revision、触发auth清理或断开运行资源。旧服务器更新不写归属表；旧服务器删除依靠外键级联清归属，并推进目录revision。

### 4.2 拟新增公开方法

所有方法经`Core::call/record`留痕，响应不含凭据，schema拒绝未知字段。revision用十进制字符串表示新增目录计数，现有server.revision继续旧类型。

| 方法 | 参数与返回关键字段 |
| --- | --- |
| `serverGroups.list` | `{limit?,cursor?}` → `{groups:[{id,name,revision,serverCount}],catalogRevision,nextCursor}` |
| `serverGroups.create` | `{operationId,name}` → `{group,catalogRevision}` |
| `serverGroups.update` | `{operationId,groupId,expectedRevision,name}` → 更新后的组/目录修订 |
| `serverGroups.delete` | `{operationId,groupId,expectedRevision,expectedCatalogRevision}` → `{deleted:true,movedCount,catalogRevision}` |
| `servers.catalog.list` | `{limit?,cursor?,query?,groupId?}` → `{items:[{server,connectionRoute,groupId,membershipRevision}],catalogRevision,nextCursor}`；省略groupId为全部，null为未分组 |
| `servers.catalog.get` | `{serverId}` → 单条目录项及catalogRevision |
| `servers.catalog.save` | `{operationId,serverId?,expectedServerRevision?,expectedMembershipRevision?,configuration?,groupId?}` → 已保存目录项；新建必须有configuration和明确groupId/null；编辑缺字段表示保持原值 |
| `servers.catalog.operation` | `{operationId}` → `{status:"committed",result}`或`{status:"unknown"}`，绑定同一调用者身份 |

configuration复用既有服务器字段和ConnectionRoute，支持当前多级跳板，不包含密码。编辑只改组时不传configuration；修改配置时检查expectedServerRevision，修改归属时检查expectedMembershipRevision。两者同改必须同事务成功。组消失返回`GROUP_NOT_FOUND`，重名`GROUP_NAME_CONFLICT`，并发`REVISION_CONFLICT`；游标过期`STALE_CURSOR`，原参数哈希不符的operationId复用返回`OPERATION_CONFLICT`。

SQL提交后的凭据清理是独立后处理：回执保存非秘密提交结果及postCommitPending阶段，hasSecret在返回时实时投影，不冻结成旧布尔值；后处理失败返回已保存ID/修订和明确阶段，重试同operationId只继续未完成的幂等清理，不重写配置。待处理回执不被普通过期淘汰；容量满时拒绝新写并提供诊断，不静默丢失回执。auth.set仍由外壳按既有流程单独执行。

分页每批最多100项且≤32KiB，按名称/ID稳定排序，游标绑定目录修订/查询/调用者；期间变化则重新获取，不拼接两代目录。组数不设新的产品级总上限。

实现将`Store::save_server_route`内部“校验/写档案/写路径”抽为接收事务的内部函数；旧入口仍按原规则调用，新catalog保存可在同事务追加归属和回执。不能先调用旧保存再写分组而产生半成功。配置未改变不推进server.revision，分组改变只推进membership/catalog。连接相关变化继续既有凭据失效规则；凭据清理/保存失败保留已保存ID和阶段，界面禁止提前打开连接，不声称SQL可回滚外部凭据文件。

### 4.3 跨窗口与表单接入

通过现有状态Hub新增`servers.catalog.v1`状态，scope为appGeneration，value仅`{catalogRevision}`；不广播整份服务器目录或秘密。宿主在成功写提交后发布，旧创建/更新/删除入口也触发；worker和外壳均走同一审计分发，无第二条私有写通道。

外壳`server-catalog-client.ts`集中分页、缓存与状态订阅；收到修订只刷新可见列表，编辑草稿不被覆盖。关闭窗口释放订阅。状态仅用于提示刷新，提交仍以SQL修订判断正确性。

`server-dialog.tsx`拆出字段组合和保存控制器：标题/底部固定，字段区滚动，认证方式后用户名/密码同行；密码框复用公共组合控件。提交先catalog事务、再现有auth.set；部分成功保留serverId与operationId，重复重试不能重复创建。分组创建是独立保存，取消主表单仍保留组。嵌套新增跳板沿用原steps草稿栈，不因重排丢密码/路径。

`server-groups.tsx`为核心管理页：列表/数量、创建/改名、删除确认、组内服务器编辑跳转；`server-picker.tsx`增加筛选/管理入口，分组只影响列表，不批量启动会话。

## 5. 工作台入口与公共原生模态

### 5.1 工具轨、＋与隐藏视图

外壳按contributions.v2构建两侧工具轨，声明存在即可显示入口；首次执行对应插件命令，返回已有视图则定位，不额外创建。旧插件已打开dock视图补作兼容入口，按pluginId+viewType去重。视图显式移到另一边、中央或独立窗口后，入口动作和位置反映实际归属。

网络manifest增加dock/right支持、默认right，workspaceEntries仍openByDefault=false；首次打开由network.open沿用现有view创建/初始化链。文件继续默认展开。宿主统一保留宽度，默认网络360px，文件沿用原值，最小值来自视图声明。布局不足优先收起另一侧，禁止把终端压成负宽或越出原生边界。

终端＋是tabCreate动作的紧凑按钮，唯一动作时直接调用现有commands.execute；横滚末端保持可达，空Tab组仍可创建。删除原primary重复工具按钮，但保留初始化失败重试。单终端窗不生成工作区创建行。

已有Ctrl+Shift+T使用同一创建动作解析，不再取primary数组第一项；避免网络入口变化后快捷键误开网络页。快捷键与＋共用同一上下文/禁用判断，终端输入焦点规则保持现状。

新增`views.badge.set({viewId,scope,count,attention})`用于插件发布运行/异常摘要；0≤count≤999，无业务正文。校验所属实例/工作区代次，运行态内存存储，不落布局；相同值不重复调用，变更合并不超过每秒一次。网络Manager在启停/恢复状态变化时发布，隐藏UI无需继续轮询；徽标与工作台状态查询通过新契约读取，旧snapshot不增字段。崩溃时用插件失败态覆盖徽标。

### 5.2 原生模态的选定实现

采用**所属主窗之上的临时原生模态表面**，内容仍加载已验证插件的UI入口，宿主注入`surfaceRole:"dialog"`、dialogId、parentViewId、父挂载代次及一次性会话标识。网络`main.tsx`据此渲染Editor入口。它不是新工作区/Tab/逻辑视图，不调用view.initialize/view.ready，不启动第二个worker，不增加PTY或网络资源组。

复用`application_menu.rs`现有owned透明窗口的定位/任务栏隐藏经验，抽取必要平台辅助；不把菜单“失焦即关闭”的策略用于表单。表面覆盖所属窗口客户区以绘制scrim和居中表单，随窗口/DPI更新，不全局置顶、不出现在Alt+Tab/任务栏，不隐藏或重建底层终端。核心只管理容器、归属、焦点和取消，不能解释网络字段或接管network.save。

拟定协议：

| 方法/通知 | 作用 |
| --- | --- |
| `ui.dialog.open({requestId,dialogId,initialState})` | 从有效主插件表面开启，dialogId必须已声明；initialState≤16KiB，返回`{sessionId,status}`；重复requestId不再开窗 |
| `ui.dialog.ready({sessionId})` | 仅当前模态来源在主题/组件就绪后调用；宿主显示并聚焦，防止首帧闪白 |
| `ui.dialog.complete({sessionId,result})` | 仅模态来源提交业务自定义结果≤16KiB，完成一次，通知来源后回收 |
| `ui.dialog.close({sessionId})` | 来源请求取消；常规用户关闭先由插件决定脏草稿是否放弃，宿主销毁/退出可强制结束 |
| `ui.dialog.result({sessionId})` | 来源查询最终结果，状态为opening/open/completed/cancelled/failed，终态回执保留60秒 |
| `ui.dialog.closed` / `ui.dialog.cancelRequested` | 完成通知/系统关闭意图；SDK先订阅再open，回执查询兜底，避免早到结果丢失 |

每所属窗口最多一个插件模态，全App最多8个；初始化10秒未ready则清理并返回`DIALOG_START_FAILED`。重复开启其他dialog返回`UI_BUSY`，不能静默覆盖草稿。用户失焦/最小化不丢草稿；所有者关闭、来源换代、视图移动/销毁、插件退出及App退出强制撤销，不能给新窗口/新挂载投递迟到结果。普通窗口移动/缩放保持会话并更新几何。

核心认证等高优先模态出现时暂时挂起插件模态，保留其草稿，核心模态完成后只在原来源仍有效时恢复；已有核心模态未结束时插件新open返回UI_BUSY。模态屏障使用会话令牌而非共享bool，防止关闭一个窗口误释放另一个屏障。Esc仅消费在当前顶层，脏草稿可用同一表面内确认，不再开第二个原生窗。

旧`workbench.modal({open})`映射到该窗口固定的shell屏障令牌，原有拖动/外壳Dialog行为保持；新插件模态不调用“隐藏全部子WebView”的旧NativeViews.modal，而由自身owned表面和输入屏障覆盖。endpoint分别记录生命周期ownerWindowId和实际surfaceWindowId，资源归原窗口，菜单锚点/IPC回执使用实际表面；不能把临时窗口误加入工作区窗口列表。

新增`surface_endpoint.rs`只封装“来源有效性、回RPC、发通知、释放订阅”：常规binding走NativeViews，模态走临时owned表面；均由宿主绑定原plugin/instance/view/mount身份，不接受JS伪造身份。模态拥有独立feed owner，不复用父表面的subscription key，主题/结果监听随模态销毁释放；审计保留来源plugin及父view，诊断另记dialog session。主视图生命周期方法对模态来源返回上下文错误，不将其误当第二个主挂载；这是对象有效性检查，不是插件权限清单。

临时窗口必须在加载入口前登记其endpoint；Tauri rpc入口按宿主登记表识别为插件来源，不能因为它是WebviewWindow就走`@window`外壳身份分支。未登记/已失效的窗口标签一律拒绝；主题feed和RPC回执也按endpoint路由，而不是广播给该插件所有表面。

不允许任意外部URL/文件作为模态入口；复用签名包资源校验/CSP，页面请求走公开SDK。网络“创建并启动”先保存并关闭模态，再由仍有效的原管理视图发既有启动命令，使身份验证在正确窗口发生。来源已结束时不自动启动，已保存配置仍保留。打开失败不得留下透明输入屏障，后续终端输入必须恢复。

## 6. 网络插件修改与旧规则兼容

`view.tsx`拆为列表/详情两种模式；列表页头固定、内容滚动，保存`selectedId`与列表滚动锚点；详情只在明确点击后打开。多条规则的状态更新不改变路由。Manager快照增加按selectedId定位页的私有查询，创建第21/256条时必须返回包含该规则的页，不出现选中而不可见。

`editor.tsx`继续纯业务表单，改为模态入口；默认auto=true、port为空，名称可空，固定转发目标必填。两模式独立草稿；更多设置收起自动重连。worker的`network.save`接受“输入草稿”与nameMode后规范化，再交Rules保存；不直接让空name穿过旧validateRule。

增加插件私有editor session：打开前worker生成绑定view/scope的editorToken，模态initialState只传此token；`network.editor.get/save/result/close`处理原规则修订、submissionId和完成结果。首次提交生成稳定新ruleId，同submissionId+相同草稿只返回同一结果，参数变化不得复用ID。规则Collection同一次写入还保存有界`saveReceipts`（最多64条，含submissionId/请求哈希/ruleId/提交修订，无正文或凭据）；旧worker同样忽略此外层辅助字段。storage.set结果不明时先读配置核对receipt，未能确认就保留“结果待核实”并暂停后续写，不以新ID盲目重试；回执确认后刷新内存Rules。worker崩溃由核心结束运行资源，重新加载配置后展示已保存规则，不自动接续启动。

存储保持`network-rules`的Collection schemaVersion=1、manifest dataSchemaVersion=1和现有Rule字段。Collection新增可选`naming`映射：`ruleId → {mode:"auto",ruleRevision,generatedName}`，与rules在**同一次storage.set**保存；手填规则无映射。读取只采用ID存在、revision及generatedName与规则匹配的记录，损坏/过期辅助项降为手填，不改实际连接配置。

旧规则全视为手填，名称/端口不变。旧worker忽略外层naming/saveReceipts、仍读非空name；旧版本再次保存会丢辅助信息，之后升级按手填处理，已保存标题和网络行为不丢。新worker对辅助记录也执行类型/长度和整体512KiB预算校验，读错不覆盖原数据。此项是可接受的回退降级，需实测，不引入插件管理器尚未支持的自动dataSchema迁移。

自动标题：代理为“内网代理”，转发为“转发至 主机:端口”；超过现有128字符限制时用“端口转发 端口”简名，完整目标继续在列表/详情显示。自动模式编辑目标时同步标题，手填模式不自动改。新建自动端口写bindMode=auto/port=0；运行入口只取core listener回执，不把临时端口写成固定配置。停止后显示“启动后分配”，不能复制已失效入口。

`run.ts`、协议解析和核心net.*仅回归，不计划更改；启停仍用现有Manager代次/飞行请求去重。编辑运行规则仍是保存下次生效或显式重启，删除只停止本规则。模态保存回执丢失通过operation/session和规则ID核实，不能直接重建；重启与认证取消保持原流程。

实施细节（2026-10-27）：网络保存回执的请求哈希采用`@noble/hashes 2.0.1`的纯JavaScript SHA-256，固定版本并随插件打包，可在QuickJS中运行；只摘要规范化的提交参数，不在回执复制表单正文。此为已确认幂等方案的内部依赖，不改变接口/用户行为。

## 7. 文件树：容量读取和虚拟布局一并调整

### 7.1 隐藏项与动作

worker初始化/恢复始终showHidden=true，移除UI切换与偏好写入；旧快照false忽略。内部旧files.hidden入口可短期返回固定true并不写偏好，防止遗留回调重新隐藏。`.`/`..`协议伪项继续过滤。

删除Details展示，不删除linux-collector/mounts底层解析。select不再为普通文件自动读容量/全部属性；复制路径保留原字节安全处理，符号链接定位改为菜单动作按需readLink/realPath，不因移除详情丢功能。

### 7.2 高度模型

`row-layout.ts`将普通行记1单位，确认挂载点的容量行记2单位；基准单位来自公共密度探针实测。挂载点从开始加载到成功/不可用均占2单位，避免容量回来再改变高度。node与附属容量保持同一个treeitem/选择对象，不新增可导航伪文件。

worker为当前flatten结果维护前缀高度单位数组，二分查找滚动位置；私有files.snapshot改用`startUnit/countUnits/overscanUnits`请求，返回`startUnit/totalUnits/selectedUnit`及每行unitOffset/units。视图scrollTop、spacer、当前行定位、Home/End/PageDown、follow目录定位都用这套单位，移除`index * rowHeight`散落计算。展开/删除/挂载信息变化重建布局修订，保留首个可见nodeId及其行内偏移作为锚点；迟到旧revision页不能拼入新列表。

保持现有节点/响应容量预算和32KiB分段响应，超长路径续取也绑定同一revision。本次不引入第三方虚拟列表库。私有UI/worker同包升级；移动快照升级到schema/stateVersion=2并读v1，能保留selected/展开项时保留，不能可靠转换的旧像素位置按选中节点重定位，不恢复新会话。

### 7.3 容量与动画

`capacity-cache.ts`缓存键为工作区代次、连接代次、mount快照修订、挂载ID/字节路径；只对当前实际可见挂载点发起请求（overscan不预取），最多2个并发、256个LRU条目。复用mountCapacity的挂载一致性和df回退，失败也缓存到刷新/代次变化，不反复请求；正在读取的项目不因LRU被丢成无主操作。

隐藏/折叠/离开可见区撤销不再需要的任务，停止新任务，迟到结果必须校验代次；刷新只使对应快照/容量缓存失效，不做定时全盘扫描。卷用量不是目录大小，不对普通后代重复采集。并发预算纳入现有远端I/O预算，不能再增加独立无限队列。

公共BusyIndicator负责16px旋转/减少动态，TreeRow负责aria-busy；mount-capacity.tsx只绘制真实百分比和已用/总量，不负责远端请求。错误/未知层不造0%。高对比/空间不足仍用文本表达，挂载行2单位的命中边界与绘制一致。

## 8. 终端、调用记录与公共控件

### 8.1 终端末行

先用原生序号输出复现并记录：外壳占位逻辑矩形、宿主实际物理矩形/DPI、插件可用内容矩形、xterm行列及viewport底部。未复现前不认定只有padding问题。

计划将padding留在外部布局层，xterm放入零padding的独立`terminal-grid`；`terminal-geometry.ts`统一收集ResizeObserver、document.fonts.ready、字号变化、提示条变化和表面再次可见事件，用一帧调度合并。复用FitAddon公开fit/proposeDimensions，不读取xterm私有_core、不修改供应商代码。

隐藏/不足最小尺寸时不fit成0；新稳定尺寸只发送一次pty.resize，绑定当前PTY generation并串行合并，迟到失败不重置新代次lastSize。如证据表明宿主逻辑/物理换算裁切，则修native.rs对应换算，使宿主与子WebView使用同一物理矩形；不能仅在终端额外加空行规避。

适配前记录是否位于底部；在底部才恢复到底，否则保持历史锚点/选区。纯换主题不触发fit或PTY resize。误差允许留不足一行的底部空隙，最后一行必须完整。

### 8.2 调用记录

抽出CallRecordsPanel，外层flex列且min-height:0；header/filters不滚动，结果ScrollArea flex:1/overflow:auto/scrollbar-gutter:stable，表头在结果内sticky。IntersectionObserver root指向结果区，游标和在途锁沿用；切筛选/刷新递增请求代次，只重置结果滚动。

追加失败保留行，底部提供重试（重试当前游标，不清首批）；防重复请求/过期结果继续生效。更新旧VIEW-01/05隐藏滚动条断言，保留原分页异步测试的严格性，不用改预期掩盖加载问题。

### 8.3 控件与窗口

公共输入/组合框统一内侧focus-within/focus-visible，36px基准盒尺寸包括描边；只对文字输入、select及组合控件使用，不把checkbox/radio的可访问焦点一起抹掉。服务器布局用现有Field/Grid组合，双列阈值约440px，错误区位于各列，不重排认证语义。

新增WindowControlButton专用公共变体，三者统一46px基准宽和顶栏高度，不再混compact/default；公共视觉层也填满命中区，消除外壳button尺寸与sd-button-face尺寸冲突。外壳只传minimize/maximize/restore/close语义及回调，状态来自核心窗口；window_chrome命中报告与最终盒一致，保留Windows贴靠/系统菜单。

参数在foundation/profile集中生成，颜色沿用主题；更新组件展示页和样式边界检查的必要窄例外，不能跳过检查或给整个插件CSS放行。三主题几何一致，多巴胺仍无阴影，减少动态禁用旋转而保留状态。

## 9. GUI启动与交付包

`main.rs`在Windows无论debug/release均使用GUI subsystem；先保留internal-host子进程入口，再启动App。worker已有CREATE_NO_WINDOW和管道保持，不能让内部宿主失败弹出用户窗口。开发日志通过已连接stdio/日志文件，显式从终端启动不要求自动创建额外控制台。

新增StartupConfig一次解析并由全App使用：

1. 实际EXE旁`portable.json`存在且schemaVersion=1时，使用EXE旁data；无标记则用现有com.sodeck.v2.dev系统数据目录。路径不依赖cwd，不自动迁移或覆盖日常数据。
2. EXE旁resources优先，其次框架resource_dir；要求`bundled-plugins.json`索引列出terminal/files/network-proxy包及公钥文件。路径规范化后必须留在资源根，不回退CARGO_MANIFEST_DIR。缺项显示可恢复错误，不能静默漏加载网络插件。
3. 显式开发覆盖需debug构建且`SODECK_DEV_MODE=1`；scripts/desktop.mjs及现有测试harness显式设置。普通双击忽略遗留SODECK_DATA_DIR、插件路径、调试端口/WebView参数等覆盖；release忽略所有开发覆盖。
4. 出厂包只补缺失或按现有prepare_bundled规则准备，不能覆盖用户已激活的兼容新版；签名验证/重启激活/回退保持。三插件列表来自索引，不在启动器写三套逻辑。

数据根确定并取得锁后，按§4.1处理数据库版本；提示/备份/新库启用全部在Core和插件worker启动前完成。核心导出预检/重建结果，外壳显示提示；非桌面调用者直接打开不匹配库时返回结构化错误，不隐式执行无提示重建。错误不能因GUI子系统而只留在控制台。

StartupDiagnostics在数据可写时写本地有界日志（建议单文件2MiB、3份轮转）；失败早于数据目录可用时用当前用户临时诊断目录并在错误提示显示路径。记录阶段/错误/版本，不dump环境变量、密码或完整请求。主进程启动失败用已有rfd原生提示，资源缺失且外壳可用时用应用内提示；不能只eprintln后消失。内部worker仍回父进程管道。

新增独立打包入口`node scripts/package-polish-dev.mjs`（拟建），输出`.local/dev/SoDeck-v2-T015/`及压缩包，不覆盖T014和其data。包含GUI EXE、三插件签名包、公钥、资源索引、portable.json、README和build.json；不包含实际data、签名私钥、源码目录或测试秘密。

交付构建使用`npm run desktop:build`的release profile；build.json区分`buildProfile:"release"`与`published:false`，记录源码commit+dirty fingerprint、API/插件版本、文件hash和证据索引。开发调试包可另建隔离目录服务自动化，但不能以其结果代替最终release目录包Explorer双击/移位验证。

## 10. 测试关口与实施顺序

各功能完整准备/步骤/预期/清理沿用T-015引用的SM、PN、TN、FT-UX、TERM-VIEW、WB-UX、CR-UX、START-UX；本节补代码设计特有的失败与兼容检查，结果只写任务结果文件。

| 关口 | 拟修改/新增验证入口 | 必须证明 |
| --- | --- | --- |
| G1 契约/数据 | 新`crates/core/tests/server_catalog.rs`、`store_bootstrap.rs`测试，SDK catalog/workbench-ui测试；plugin-manager现有tests | 无库/同版本/高低版本、WAL备份、空库边界、失败保留与中断恢复；归属与连接revision分离；同operationId去重；老API不清分组；新manifest与旧DTO兼容 |
| G2 公共UI/外壳 | 扩展`tests/ui/design-system/`、server-tabs/workbench/call-records用例；新server-groups用例 | 焦点无裁切、两列折行、固定查询、＋/工具入口、窗口按钮三主题几何；不只断言class名 |
| G3 插件 | 文件state/domain/worker、网络rules/manager/worker既有测试，新增row-layout/capacity-cache | 变高行的滚动/跟随锚点；容量并发/迟到回执；规则naming回退、端口默认/旧配置保持、创建定位页 |
| G4 原生模态/终端 | 新`tests/desktop/plugin-dialogs.mjs`、`terminal-geometry.mjs`，复用harness/独立SSH | 模态超过320px侧栏可见；不新建PTY/worker；Esc/IME/核心认证叠层/失焦/移窗/销毁无屏障；末行跨DPI完整 |
| G5 真实联动 | 复用files、directory-follow、network-tunnel、ssh-multihop与updates脚本，补T015场景 | Windows/Linux隔离SSH、两模式、多跳、实际listener地址；隐藏不断流，关闭/退出清零；独立更新及旧包兼容 |
| G6 最终目录包 | 扩展package-smoke、package检查；新增`tests/desktop/startup-gui.ps1` | release包Explorer双击无闪窗；原生版本提示/备份重建和错误可见；进程树/录屏验证；移位/异cwd/中文路径、三插件、重复实例、托盘及退出 |

额外有意义场景：

- **CODE-01**：在隔离副本覆盖无库、同v4、低v3/高v5和已有v0；不匹配库先提示、完整备份后空库启动，同版本数据保留。验证已提交WAL、加密凭据副本和独立插件文件；注入备份/建库失败、各发布步骤终止和恢复记录损坏，旧数据可恢复且不启动半成品，成功再开不重复重建。另覆盖A表单打开时B删除/改组、配置+归属事务失败和凭据保存重试；无孤立归属/重复档案，纯分组不触发连接修订。
- **CODE-02**：新核心装旧终端/文件/网络包，逐项调用旧严格方法；新包送旧核心被明确拒绝；准备/重启激活/失败回退，中文名不抢先变。
- **CODE-03**：模态创建失败/ready超时/早到完成/重复完成、父窗口销毁、旧来源重放、核心认证挂起恢复；每窗一次，回执一次；100次开关后临时表面/订阅/屏障归零，PTY/网络组数量不增加。
- **CODE-04**：新网络规则以空名称提交后回退旧worker，旧worker编辑再升级；具体标题、端口和目标可用，自动命名降级如说明。存储写失败不出现rules/naming/receipt一半成功；同submissionId重送、保存回执丢失和模态来源关闭不重复创建/启动，无法核实结果时保留未决状态。
- **CODE-05**：文件大树中稀疏挂载行的前缀单位、选中定位/键盘/终端cd和滚动锚点；慢容量请求回执跨代次、LRU淘汰、不可见停止，≤2并发且无持续后台轮询。
- **CODE-06**：普通release启动继承过期SODECK环境，移动包后启动；不误用日常data或源码资源、不开放调试端口。真实GUI验证数据库不匹配提示在worker前出现、备份路径可定位，重建后进入空服务器页，失败可见且不自动循环重试。主进程/worker错误分别展示/回管道，不增加伴生控制台。

实施顺序调整为按依赖推进：

1. 补齐schema/生成器/能力版本与当前库初始化、版本预检/备份重建，建立回归夹具；并完成公共焦点、窗口控件、记录滚动和终端复现修复。
2. 接分组事务/外壳管理页、插件元数据、通用工具贡献和＋；验证旧包/旧DTO。
3. 先完成通用原生模态及G4隔离验证，再接网络插件列表/创建/存储兼容；不在核心临时写网络专用表单。
4. 完成文件布局/容量队列及隐藏项策略，回归目录跟随；完成GUI启动/三包索引和诊断。
5. 执行本次必要整合检查与真实场景，生成独立release目录包，交付结果与用户验收步骤。

基础检查沿用`npm run check`、`npm run test:contracts`、`npm run test:core`、`npm run build`；浏览器用`npx playwright test -c playwright.ui.config.ts`运行受影响集合。新增脚本尚不存在，不能提前声称可运行。只有失败、代码变化或原生复现线索要求时才扩大/重复测试；不无理由重跑整个网络吞吐项目。

## 11. 完成条件与授权

完成需13项及P-52全部落地、上述必要关口通过、三主题/DPI真实渲染检查和release目录包无控制台验证有证据；最终界面/操作与规格一致，数据库备份重建和插件/API兼容有结果。浏览器注入、真实SSH、Windows原生及目录包验证分别记录，安装器/正式发布仍不在范围。

用户2026-10-27“可以开始做了”确认本文r2、T-015 r3及其引用功能修订；实施期内部拆文件、调试和范围内缺陷修复自主完成，不逐次询问。授权包含应用、测试、构建修改和独立试用包，不包含Git提交/推送及正式发布。

若选定原生模态方案在目标Windows环境不可行，先保留失败证据并提出替代设计；不得擅自改成挤在窄侧栏的表单、把网络业务放到核心，或用隐藏/重建终端通过测试。只有改变已确认交互/架构/兼容边界时再提交修订；一般实现细节不重复请求确认。

修订记录：r2（2026-10-27）按P-52替代r1的数据库迁移/降级恢复设计，补版本不一致提示、一致备份、全新初始化、凭据/独立文件边界及中断恢复验证。r1的13项模块与公开契约提案保留。

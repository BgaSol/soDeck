# 侧边工具入口与停靠：代码整改方案

日期：2026-11-10。状态：已按本方案完成工作区源码整改，本机验证及平台边界见[实施记录](implementation-tool-docking.md)。功能规则以[侧边工具入口与停靠设计](plan-tool-docking.md)为准；核心与插件仍独立构建，未升级版本或发布。

源码基线为核心 `0.20.1`、HEAD `2c0d371` 及当前工作区。以下现状来自源码检查和用户截图，未启动安装版重演完整缺陷。现有系统状态插件等未提交改动不属于本次方案的实施范围。

## 1. 整改目标与实际原因

交付一个完整行为：侧边只提供工具入口；按钮与实际停靠位置一致；按钮可跨侧移动和同侧排序；拖动展开/收起的工具保持其可见状态；位置、顺序、恢复、取消和移窗均使用核心共同规则。

| 当前代码 | 现状与整改原因 |
| --- | --- |
| [components.ts](../src/components.ts) 的 `paint/action("more")` | 使用 `preferences.side/order` 排按钮，筛选为 `sidebar || locateEntry`；实际面板却使用 Host 的左右位置。终端因 `locateEntry` 进入工具条，移侧后入口可能留在默认侧 |
| [component_ui/layout.rs](../src-tauri/src/component_ui/layout.rs) 的 `open_view` | 新工具首次停靠读取默认偏好；不能落实“先移动未创建工具、以后在新侧打开” |
| [workbench_layout.rs](../src-tauri/src/workbench_layout.rs) 的 `Tools/Host::dock` | 侧栏只有 viewId 列表，不能表示尚未创建的工具入口；`dock` 先移除再追加，不能表达完整按钮顺序，也容易改变原可见状态 |
| [workbench_memory.rs](../src-tauri/src/workbench_memory.rs) 的 `SavedHost::capture/apply_tools` | 保存已创建工具的侧/组与 visible，没有侧边入口顺序；可用但未创建的工具不在 capture 结果中 |
| [component_ui/layout.rs](../src-tauri/src/component_ui/layout.rs) 的 `preference` | 同时改本机推荐偏好、工作区偏好和现有工具位置；普通拖动若复用它，会扩大到新工作台默认值，且可能展开原来收起的工具 |
| [components.ts](../src/components.ts) 的拖动链 | 已有页面/组/面板标题拖动与原生移窗；工具按钮目前被排除。按钮仅同窗移侧，不能直接套用会拆窗的页面拖动 |
| [workspaces.rs](../src-tauri/src/workspaces.rs) 的 `remember_workbench_layout` | 已有独立 IO 锁、备份与原子写入，但接口不返回保存结果；关闭/迁移期间跳过保存与真正保存成功必须区分 |

实施第一步用现有核心/浏览器夹具复现上述行为并增加失败回归，再修改共同路径。本次源码推导不能替代该复现记录。

## 2. 核心与插件的职责边界

| 层 | 持有与执行 | 本次不得接管的职责 |
| --- | --- | --- |
| 核心 Rust | 工具槽位、逻辑视图及 Host 归属、位置投影、布局修订、提交/回执、布局保存、关闭与跨窗交接 | 文件导航、终端启动配置、Docker 命令、状态采集策略、隧道规则启停 |
| 核心 TypeScript/CSS | 按快照绘制工具条/面板/更多清单；捕获宿主拖动、落点和预览；提交用户意图；安排焦点及尺寸 | 自行宣布布局提交成功、改写插件业务状态、复制插件实例、根据插件 ID 决定布局行为 |
| 插件清单 | 声明视图类型、实例策略、标题、图标、初始位置和新建动作推荐 | 用户当前停靠位置、按钮排序、窗口归属及批准移动的权限 |
| 插件进程与页面 | 管理自己的业务实例、草稿、采样和任务；遵守已有挂载、可见性、尺寸、flush 和交接契约 | 实现宿主工具条、拖动宿主按钮、保存左右停靠偏好、访问其他插件 DOM/进程 |
| 核心原生适配 | 窗口命中、坐标/DPI、既有跨窗拖放和系统生命周期 | 向插件暴露平台鼠标接口或要求插件按本机系统处理布局 |

业务插件继续独立进程、独立版本、独立包。核心与插件之间继续使用版本化 RPC 和既有 UI 桥；本次不新增插件权限声明、审批、插件间直接调用或生产依赖。

侧边资格只由已验证声明的 `container == "sidebar"` 决定。`editor/dialog` 不生成侧边入口，旧 `locateEntry` 也不能绕过。一个文件插件提供的目录树和编辑页继续分别属于工具与中央页面。

标签栏「＋」仍使用 `defaultCreateAction` 和插件 `ui.actions`，不在核心恢复“寻找官方终端 ID 并直接创建终端”的逻辑。终端如何创建 PTY、是否运行登录 shell/命令以及目录集成都留在终端插件。

## 3. 数据模型：用槽位表示侧栏位置

### 3.1 调整现有 Tools，不增加另一套可写位置表

在 `workbench_layout.rs` 将左右 `Tools.view_ids` 的存储改为有序工具槽位：

```rust
pub struct ToolSlot {
    pub definition_key: String,
    pub view_id: Option<String>,
}

pub struct Tools {
    pub entries: Vec<ToolSlot>,
    pub active_view_id: Option<String>,
}
```

以上为拟实施类型。左右两组 entries 的所属侧与数组顺序，就是侧栏工具位置及顺序；槽位没有自己的 side/order 字段。空 `view_id` 表示布局入口，不是已创建的业务实例。

必须保持：

- 已实例化工具的侧栏位置只存在于 Host 槽位；进入中央后只存在于中央 Group。不能保留一份侧栏按钮位置，再独立移动 view。
- 同一工作区的同一 sidebar 定义最多一个有效槽位/中央实例归属；同一个 view 只有一个实际 Host/容器。
- 活跃 ID 必须指向该侧槽位中真实存在的 view；一个纯入口槽位不能被标记为展开。
- `Host::view_ids()` 从槽位的非空 viewId 与中央/弹窗位置计算。槽位不参与 Surface 挂载、业务创建、flush、保存草稿或资源释放。
- 缺失插件的已保存槽位可以保留稳定 key，但不能据此制造声明或启动实例；展示不可用原因，恢复登记后再绑定。

Rust 调用者从 `.left/right.view_ids` 迁到只读方法。核心快照可继续输出**计算得到的** `viewIds`，减少中央/关闭测试夹具的不相关变化；它不再是可写存储。新增 entries 的用途限于侧边工具布局，不泛化为全组件注册表。

### 3.2 初始化、创建和容器切换

| 时点 | 核心行为 |
| --- | --- |
| 新建首份工作台 | 从服务器布局槽恢复工具入口；无记录时按本机偏好、插件推荐及稳定 key 建立入口位置/顺序，不因建立槽位创建业务 |
| 打开尚未创建工具 | `open_view` 找到该 key 的实际槽位，沿用既有单例创建路径，绑定新 viewId 后展开；不重新读取 defaultSide 决定当前位置 |
| 后台视图先创建、尚未请求界面 | 将已有 view 绑定原槽位，保留 `ui_requested` 和原后台策略；建立入口不触发 UI/业务初始化 |
| 工具放入中央 | 移除其侧栏槽位，将原 view 放入中央组；普通侧边按钮消失，更多清单可定位原工具 |
| 中央工具显式停回侧栏 | 原 view 从组移到指定槽位并展开，保持实例；只有 sidebar 类型允许此操作 |
| 工具拆到其他窗口 | 准备阶段不改变已提交槽位；共同交接提交时移除来源位置并在目标建立位置 |
| 收起、同侧切换 | 只调整 activeViewId，不移除槽位、不 dispose view |
| 页面/工作台真正关闭 | 沿既有关闭协调器收尾；纯入口槽位不增加等待对象，不把关闭前记忆覆盖成空布局 |

`open_view` 先解析该 key 的实际 View/槽位所在 Host，再对真正的目标 Host 做 layout_admission 和单例检查；不能只检查发起窗口后在另一窗口创建。即使入口尚无 viewId，更多清单请求也打开其当前槽位，而非在发起窗口复制。默认或 background 声明触发的业务初始化仍由原流程管理，槽位恢复本身既不触发创建，也不抵销插件已声明的后台行为。

新建额外 Host 不复制已有单例工具。已存在于其他 Host 的工具仅在更多清单中定位；未实例化入口也只有一个当前归属。尚未显式恢复的历史额外窗口布局只保留在档案中，不自动生成窗口或第二份入口。若工具首次被请求而保存的 Host 尚未恢复，落在发起工作台并明确更新其当前归属，之后恢复旧档案不能夺回或复制工具。

## 4. 统一投影：工具条与更多清单共用核心结果

在 `component_ui.rs`/`component_ui/layout.rs` 的工作区快照增加 `tools`。由核心结合完整 WorkspaceUi、Host 槽位、已登记定义和记忆生成，不只扫描当前窗口的 `views`。

拟新增的只读投影：

```ts
type ToolLocation =
  | { kind: "dock"; windowId: string; hostId: string;
      side: "left" | "right"; index: number }
  | { kind: "group"; windowId: string; hostId: string; groupId: string };

interface ToolProjection {
  key: string;
  viewId?: string;
  location: ToolLocation;
  expanded: boolean;
  pinned: boolean;
  available: boolean;
  error?: string;
}
```

标题、图标和来源优先复用 definitions；缺失定义以保存 key 和不可用提示显示。`expanded` 由实际 activeViewId 推导，`pinned` 读取 Runtime 中已有本机固定偏好，available/error 来自登记和视图状态。工作区 preference.side/order 的旧副本只作为兼容初始化推荐，不再决定当前入口的位置和顺序。前端的界面准备状态仍来自当前 Surface，不另存一份插件运行状态。

`components.ts` 的工具条只取 `kind=dock`、windowId/hostId 为当前工作台、side 为该侧的投影，并按 index 排列。常驻项取 pinned，当前展开项即使未固定也临时加入；容量不足优先保留展开项和更多入口。实际空间不足造成临时隐藏时，由现有有效几何计算最终 `aria-expanded`，不能把未显示内容标成已展开。

两侧更多菜单共用完整 tools，按实际位置显示“左侧/右侧/主区域/其他窗口”。点击名称沿现有 open/locate 定位；显式停靠才执行移动。移到中央/其他窗口的工具不在来源工具条留下普通或选中按钮。

迁移候选投影继续只读，候选入口禁用并显示交接状态；来源按钮在共同提交前保持原位置。候选预览不能成为第二个正常可操作入口。`ui_snapshot` 只读，不修补槽位、不创建 view，也不触发保存。

## 5. 共同操作、并发和保存结果

### 5.1 同窗移动与排序

在现有 `component_ui.action` 增加宿主内部 `moveTool`，不是插件 RPC：

```json
{
  "op": "moveTool",
  "requestId": "本次用户操作编号",
  "key": "插件ID:稳定视图ID",
  "side": "right",
  "beforeKey": null,
  "revision": 12
}
```

Context 继续由外围 action 提供；窗口身份由真实 WebviewWindow 确定。`beforeKey` 是目标侧的稳定工具 key，null 表示末尾；先从原列表去掉本工具再计算插入，原位为无操作。使用稳定 key，避免溢出列表只显示部分按钮时把可见下标当完整顺序。

一个 Runtime 共同操作处理：校验 key 是 sidebar、实际槽位属于当前 Host、目标侧有效、beforeKey 仍在目标侧、修订一致，随后在 Host 副本上移动并验证，最后共同提交。当前已有 view 时再校验完整 Context、实例和冻结/交接状态。未创建时不申请 view、Surface 或插件 RPC。

普通 moveTool 只接受当前 Host 的侧栏槽位；若工具已去了中央/其他窗口，返回位置已改变，让前端刷新。中央工具的显式 `dock`、原生侧栏落点、返回来源和关闭回收，复用同一 Runtime 布局操作及 Host 槽位方法，但分别使用“展开”或“隐藏”的既有生命周期意图。

共同方法必须先读取原侧是否实际活跃，再移除/插入：跨侧移动活跃工具将目标设为活跃，源侧收起；移动非活跃工具不改变目标活跃项；同侧排序保留两侧活跃项与真实页面焦点。原 `Host::dock` 的移除后无条件追加/激活路径不再供用户移侧绕用。

`preference` 继续维护固定状态和新工作台默认值，普通拖动不调用它；移除其因 side 变化直接 host.dock 的分支。清理旧面板菜单“入口置顶”等使用 preference.order 调整当前工具条的路径，改成同一 moveTool。`recommend` 恢复初始推荐，不直接搬当前工具，菜单明确命名为“恢复新工作台推荐”，避免暗示当前布局已恢复。固定状态变更通过核心投影反映到各工作台，side/order/startup 推荐仅影响以后缺少实际布局的位置与初始化。

### 5.2 请求回执与状态锁

复用 `workbench::State` 的短时锁与 `layout_admission`，在同一临界区检查关闭/迁移保留并提交槽位、activeViewId、revision 和请求结果。插件 RPC、原生窗口调用和磁盘 IO 不持该锁。

同窗工具移动需要可查询回执，不能使用当前 open/toggle 的 `ui.requests` 字符串缓存，也不能把空槽位伪装为 View。增加小型本地工具布局回执记录，内容限定为真实窗口、Context、稳定 key、输入指纹、提交修订、布局结果和保存结果；`moveStatus` 先查询本地记录，再查询既有跨窗 MoveReceipt。

- 本地与既有移动编号不得冲突；同编号同输入查询原结果，同编号不同输入拒绝。查询回执先于重新校验当前 revision，避免已成功操作重试被当成过期布局。
- 回执沿用既有移动的五分钟终态保留策略，本地/跨窗总数共同受 4096 项上限约束；额度满拒绝新请求，未结束记录不清空。
- Context/工作区释放后，原回执保持“已结束/已失效”的结果到到期，不能借重试创建槽位或新实例；应用重启清理运行期回执。
- 拖动开始保存基准 revision，松手提交时必须核对；同窗其他用户操作导致冲突时取消当前拖动并刷新，不以新 revision 自动覆盖。
- 前端每个工具只保留一个待确认操作。传输错误只查原 requestId，完成后读取快照；不生成新编号重做移动。

本地结果区分 `status` 和 `persistence`：布局为 pending/completed/failed/expired，保存为 pending/saved/failed。布局已经提交而写盘失败，结果仍为 completed，并附保存错误；不能向用户报告成“移动失败，已回到原位”。

### 5.3 保存与重试

让现有 `remember_workbench_layout` 的底层返回明确的 saved/deferred/failed 结果；保留旧调用者的日志包装，统一使用原 `layout_io`、短锁快照、备份及原子替换。

- moveTool 提交后发工作台变化通知，再在锁外保存；返回/回执携带保存状态。前端只绘制已提交快照，不先搬 DOM 等核心同意。
- 迁移、恢复或关闭保留期间现有全局保存限制仍有效，记为 deferred，不能记作 saved。沿这些既有流程结束时的布局保存入口保存最新快照，并更新仍有效的工具回执/保存提示；不新建定时保存 worker。
- 坏文件保护或写盘异常返回 failed。当前布局保持有效，界面提示“布局已调整，但未能保存，重启后可能恢复原位置”。
- 新增宿主内部“重试布局保存”入口仅调用同一个保存方法，取最新已提交布局；不重复移动、不重建业务，也不覆盖被保护的未知版本文件。
- 保存结果按实际快照修订关联，较早的保存错误不能覆盖之后成功保存的新状态。日志记录宿主操作编号和布局错误，不记录业务正文。

## 6. 拖动接入：复用手势生命周期，区分落点范围

`components.ts` 扩展现有手势状态为页面/组拖动和工具入口拖动，沿用 cancelGestures、lostpointercapture、Esc、blur 和 dispose 清理；不另建第三方拖放框架。

| 来源 | 落点与执行 |
| --- | --- |
| 侧边工具按钮 | 只接受当前工作台左右工具条与排序插入线；纯前端预览，松手调用 moveTool。中央、窗口外、其他工作台/窗口落点取消 |
| 实际面板标题 | 当前窗口侧栏落点走共同工具移动，带 beforeKey；中央/窗口外/其他窗口继续使用既有页面移窗入口 |
| 中央工具标签 | 保留页面/组拖动；停回侧栏时核心校验 sidebar 类型并调用共同槽位方法 |
| 面板/按钮菜单 | 移侧和上移/下移使用 moveTool；中央或其他窗口显式停靠使用既有 dock/workbench_move 与共同提交 |

按钮绑定稳定 key，而不是强求 viewId；只有实际工具视图才进入原生 `workspace_drag_start` 的 View scope。不得为了复用原生拖动而提前调用 open_view。

`windows.rs` 的 DockRect/DropTarget 及 `workspaces.ts` 的 DropTarget 增量携带工具条排序信息或 beforeToolKey，保持现有坐标适配；`Destination` 增加可选 beforeToolKey，仅 side 停靠解释它。同窗实际侧栏工具的标题落点转入共同移动并保留展开状态，跨窗仍执行准备/flush/交接，中央工具停靠则显式展开。落点只能表达意图，最终资格、实例和修订由核心检查。

手势实现要求：

- 达到 6 CSS px 阈值后才开始拖动和捕获指针，捕获在稳定宿主对象上；普通按下不能破坏按钮点击和焦点。
- 只识别明确的工具按钮、面板标题或页面标签手柄。关闭、菜单、收起、更多、新建和业务内容不能因共有 data 属性被当成拖动对象。
- 工具条全高空白可接收移动；更多按钮始终固定。排序以完整投影为准，溢出项目不能被丢掉。
- 拖动时阻止页面文字选区扩展，指针经过 iframe 继续接收移动/松手；结束恢复默认输入，不要求插件拦截鼠标。
- 预览期间不改实际槽位、不保存、不重挂 iframe。开始、结束、取消明确幂等；Esc 结束后迟到 pointerup/lostpointercapture 不再提交。
- 成功移动可见工具后焦点跟随原有效表面；移动收起工具或仅排序时不将业务焦点交给隐藏面板。取消恢复触发焦点；任何结果都不抢另一服务器/窗口的新焦点。
- 新快照使原 Host、槽位或实例身份失效时取消手势；尚未结束的句柄不被普通 chrome 重绘意外移除。

## 7. 记忆格式与旧数据迁移

继续用 `workbench-layout.json` 的服务器/稳定布局槽、分屏树和 `SavedHost.tools`，不新增停靠配置文件。将文档 schemaVersion 从 1 升为 2；Tool 的 side 停靠记录增加 order，group 记录保持原结构。保存从全部槽位和中央工具生成，包括 viewId 为空的入口。

拟保存的侧栏项：

```json
{
  "tools": {
    "sodeck.files:browser": {"group":null,"side":"right","visible":false,"order":0},
    "sodeck.docker:browser": {"group":null,"side":"right","visible":true,"order":1}
  }
}
```

这是既有 SavedHost 内部结构的示例，不是插件清单字段。保存 stable key、位置、顺序和展开意图，不保存 viewId、Context、命令、正文或“已经启动任务”的标识。

迁移规则：

1. 新核心同时读取 schema 1/2。旧记录的实际 side/group 优先；缺失入口按既有用户偏好补齐，顺序按旧 preference.order 和 key 稳定生成。当前运行期已有实例的位置优先于历史档案。
2. 一个工作区的重复历史工具位置以当前实际归属为准；尚无实例时先选已恢复的主布局槽，再按保存槽编号稳定择一。被排除的历史位置不能在以后恢复额外窗口时重复创建。
3. schema 1 在内存迁移，第一次成功保存时写 schema 2，沿用 .bak 备份。读取失败/未知版本保留原文件，不借迁移删除或静默覆盖。
4. 恢复 side 项先建立有序槽位，再沿既有显式初始化/打开策略绑定实例；`visible:false` 项不会因恢复入口而创建 UI/业务。纯槽位本身永远不调用 ui.createView。
5. 插件缺失时保留其布局记录；恢复原 stable key 后沿用位置，不改变其他工具。未知 key 只有已保存元数据，不能生成可用插件能力。
6. 下调到旧核心时，schema 2 按旧核心已有未知版本保护保留原文件并使用可用默认布局；新版停靠定制暂不生效。该降级限制在版本说明中列出。

`ui-layout.json` 保持 schema 1，继续保存固定入口、默认动作和初始布局推荐。旧 editor 的 pinned/locateEntry 推荐保留兼容读取但不参与工具投影，不删除实际终端页、动作设置或插件数据。

保留现有保存额度：每 Host 最多 128 个工具、每服务器 32 个保存 Host、1024 个服务器和 1 MiB 文档；新增 order 检查有效范围与同侧顺序，非法记录走既有保护。运行期/存储一致性验证只由核心执行。

## 8. 插件修改与协议兼容

| 插件 | 必需源码/声明修改 | 验证责任 |
| --- | --- | --- |
| 终端 | [release.json](../plugins/terminal/release.json) 删除 views 中 `locateEntry:true` 和侧边 `defaultPinned:true`；保留 container=editor、multiple、ui.actions、createAction、createOne、startupOrder、locationProvider 及业务实现 | 旧/新核心既有新建动作正常；多终端、首次初始化、最后页关闭、移窗、真实 PTY 与输入保持 |
| 文件 | 本功能预计无需业务或声明修改；目录树仍 sidebar，编辑器仍 editor | 文件导航与草稿移动/收起后连续，打开文件仍进中央，SFTP/传输不中断 |
| Docker | 本功能预计无需业务或声明修改；browser 为 sidebar，container/logs 为 editor | 总览筛选/选择及刷新策略保持，详情/日志入口正常 |
| 系统状态 | 本功能预计无需业务或声明修改；已有 sidebar/background 声明继续有效 | 隐藏/移侧不改变采集、暂停与现有独立窗口状态 |
| 网络隧道 | 本功能预计无需业务或声明修改；管理页 sidebar、编辑页 dialog 保持 | 实际 TCP/SOCKS 隧道不中断，移侧不启停、不重新建立端口 |

新核心继续接受旧 `locateEntry` 字段及原 Declaration 校验，宿主投影忽略其侧边含义；不能因为旧终端带此字段就拒绝加载。新终端清单删除可选字段不会提高核心 API/UI 主版本要求，旧核心仍能按既有字段创建中央终端。

不新增插件“停靠能力”RPC、dockChanged 事件、side 设置、拖动批准或 UI v3。插件无需收到左右位置：同窗仍使用原有效 Surface 和尺寸/可见性通知；跨窗仍使用 UI v2。UI v1 同窗布局继续可用，跨窗保留原 UI v2 限制。

若验收发现某插件在 visibility/resize/remount 时错误地重建任务，应在该插件修复相应业务生命周期并独立交付；不能在核心按插件名单跳过通知或保存业务快照。

包清单由 [package-plugin.mjs](../scripts/package-plugin.mjs) 从对应 release.json 生成，不能只改某个已生成 plugin.json。本次默认不修改打包脚本、共享 plugin-client/bridge 或无关插件；只有实际接口影响得到验证后才扩大。

## 9. 代码落点、调用者与实施顺序

| 批次 | 文件与必须迁移的调用者 | 完成条件 |
| --- | --- | --- |
| A：核心模型与迁移 | workbench_layout.rs、workbench_memory.rs、component_ui.rs/layout.rs；Host 的 view_ids/contains/remove/place/select/dock/preserve_focus/merge_host/insert_portion、工具初始化及快照 | 槽位表示已创建/未创建工具，只有一份可写位置；旧档案可读，新档案有序且不含业务身份 |
| B：共同布局行为 | component_ui/layout.rs、workbench_move.rs、windows.rs、workspaces.rs；包括 Transfer 的 target_projection/提交检查、返回书签与批量返回 | 本地移动、中央停靠、跨窗返回使用共同位置方法，候选不抢来源；回执与保存反馈完整 |
| C：生命周期调用者 | lifecycle/close.rs、lifecycle/host.rs、component_ui/host.rs 与恢复额外窗口入口 | 工具回收/异常 Host 恢复同步槽位；纯入口不增加关闭等待，不占资源、不会恢复已关闭会话 |
| D：宿主交互 | components.ts、components.css、workspaces.ts、workspaces.css；paint、more、panel-menu、工具/页面菜单、perform、drag、dockZones、focus/geometry | 同侧按钮与面板、拖动排序、取消与溢出可用；中央及 Surface 存活行为保持 |
| E：插件声明与夹具 | 终端 release.json；tests/fixtures/plugin.rs、scripts/fixtures/workspace-ui/manifest.json 与相关 UI/生产断言 | 新核心配旧终端也没有侧边入口；新终端的动作与旧字段兼容。保留至少一份带旧 locateEntry 的 fixture 专测兼容 |
| F：验收与文档 | 既有核心/UI/业务测试、usage.md、plugin-api.md、本方案对应 implementation、终端 CHANGELOG | 用户规则与代码、协议实际行为和测试证据一致；未验证平台/降级限制明确 |

每批先跑相应回归，不能将“只删除终端按钮”“只增加拖动”作为完整功能交付。分批提交各自一个主题，发行前完成全部共同入口；不把半迁移状态当作支持旧/新布局共存的正式版本。

实施前重新搜索所有 Tools/view_ids、Host::dock、locateEntry、preference.side/order、SavedHost.tools 的调用者和夹具，检查新增/删除文件对构建输入的影响。现有目录无需整体拆分；本地回执/共同工具操作若使 layout.rs 难以维护，可在现有 component_ui 模块内单独放置 tools.rs，不能形成跨插件 SDK 或通用 Dock 框架。

## 10. 回归、构建与交付验收

| 层 | 必须验证的用户/契约行为 |
| --- | --- |
| 核心布局 | 未创建槽位跨侧/排序不增加 View；可见/隐藏移动保留 active；目标已有工具不 dispose；原位无操作；非 sidebar 拒绝；跨 Host 单例唯一 |
| 核心请求 | 同编号复用、参数冲突、过期 revision、关闭/迁移竞态、Context 结束后的迟到请求、额度与保存结果；拒绝时布局不变 |
| 布局记忆 | schema 1 迁移、实际位置覆盖偏好、缺失插件、纯入口与排序恢复、重复旧位置、损坏/未知版本、.bak、写盘失败/deferred/只保存重试；无运行身份 |
| 真实浏览器宿主 | 工具条和面板始终同侧；按钮/标题/菜单一致；展开未固定项临时可见；20 工具/溢出/短窗/三主题；更多定位中央/其他窗口；iframe 原对象保持 |
| 手势 | 微小移动点击、同侧排序、空白落点、经过 iframe、Esc 后 pointerup、失焦、失去捕获、切换服务器、快照失效、菜单按钮不误拖、无文字选区扩展 |
| 默认动作与兼容 | 保留带 locateEntry 的旧终端声明但不出按钮；「＋」通过通用默认动作；没有插件、没有终端、其他默认动作、UI v1/v2、最后页关闭与移窗均按原契约工作 |
| 真实业务组合 | 文件导航/未保存编辑、持续输出 PTY、Docker 选择、系统采集和运行中的 TCP/SOCKS 在移侧/收起/移窗前后连续；取消关闭保留，真正关闭释放 |
| 原生目标 | 五平台原生构建、打包、实际包启动/退出；三个桌面系统验证真实按钮/标题拖动、菜单键盘、iframe 输入、失焦及 DPI。跨窗原有限制单列 |

优先在 [component_ui.rs 核心测试](../src-tauri/tests/component_ui.rs)、[test-workbench-layout.mjs](../scripts/test-workbench-layout.mjs) 和 [test-workspace-ui.mjs](../scripts/test-workspace-ui.mjs) 扩展现有夹具。已有“工具移到右侧后仍点击左侧按钮”的断言必须改为右侧入口并验证来源入口消失；“定位终端”用例改走中央标签/默认动作，另保留旧声明兼容断言。不要只改测试选择器而省略状态、实例和资源连续性检查。

代码完成后执行：

- `npm run check`、`npm run scripts:test`、`npm run test`、`npm run web:build`。
- `npm run layout:test`、`npm run interactions:test`、`npm run ui:test`；终端清单变化补 `npm run terminal:input:test`，并在实际组合中覆盖相关 terminal/files/network/system-status/docker 与 communication 入口。
- 核心通过 `npm run build` 独立打包；终端通过 `npm run plugin:check` 及 `npm run plugin:build -- --plugin terminal` 独立验证包清单与产物。业务组合可使用独立已发行其他插件包，不要求重新发行无源码变化的插件。

CI 复用 [检查选择](../scripts/select-checks.mjs) 和[开发指南](development.md#版本与持续集成)：核心 Rust/生命周期改动按其实际路径选原生与组合回归，前端选现有 UI，终端元数据选终端检查；检查新测试是否进入已有脚本入口，不增加调用不存在命令的工作流，也不为了缩小检查篡改既有范围。五目标原生实际包验收是正式交付要求，不能由浏览器测试或交叉编译替代。

本轮文档验证与实施测试分开。实际编码后新建对应 implementation 记录本次提交、验证结果、未完成项和风险；发行时读 distribution.md，核心与终端各自使用原独立工作流/版本/Release，不建立同步发布门禁。核心包不包含也不依赖业务插件构建，核心应用版本仍只改 tauri.conf.json，插件版本仍来自自己的 Cargo 清单。

完成标准：槽位与投影统一、全部位置入口迁移、终端新旧声明兼容、状态及真实资源连续、失败与恢复明确、相关检查和五目标交付证据齐全。重点风险是 Tools 调用者遗漏、历史额外窗口抢回单例、保存 deferred 被误报成功和手势取消后的迟到提交；上表必须逐项证明。

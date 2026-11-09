# 默认新建行为与通用终端：代码整改方案

2026-11-09。状态：用户已确认，代码已实施；实际结果和剩余平台范围见[实施记录](implementation-default-behavior-terminal.md)。基于[功能方案](plan-default-behavior-terminal.md)，本文保留确认时的接口、模块、迁移顺序、并发处理与验证范围。整改按两个完整主题实施；后续用户另行授权发布，见[核心 0.16.0](releases/v0.16.0.md)。

## 1. 整改边界

二级“＋”发送默认行为事件。提供该行为的插件处理事件，主动申请自己的页面；页面挂载后由插件启动业务。核心只承担通用声明登记、通信投递、页面身份和连接资源管理。

新工作区的 openOnWorkspace 保留原有通用宿主规则，终端继续在 ui.mount 中启动 PTY。它与手动默认行为相互独立。指定命令启动、目录策略和再次打开配置均由终端解释，不进入核心的动作分派。

代码范围只包含这两处职责修复。Docker 插件及其业务事件、容器文件浏览、默认类型选择界面不在本次实施中。已有 JSON-RPC、正文通道、共享状态和关闭流程继续使用。

## 2. 核心数据与声明

修改 [component_ui.rs](../src-tauri/src/component_ui.rs)：

| 位置 | 具体改动 |
| --- | --- |
| Declaration | 增加 `actions: Vec<ActionDefinition>`，serde default 为空；旧清单不变。 |
| ActionDefinition | 字段 id/title/defaultCreate/dataVersion；不包含 viewType 或启动配置。每插件最多 32 项，id 使用现有视图 ID 字符规则、最长 80 字节且不重复，title 非空且最长 256 字节；每插件最多一个 defaultCreate=true。 |
| RegisteredAction | 保存声明、稳定 key=pluginId:actionId、实际 pluginInstanceId、插件名称、available 和不可用原因。 |
| Preferences | 增加可选 defaultCreateAction，默认 None；继续使用 ui-layout.json 的 schemaVersion=1，其余字段原样保留。 |
| Runtime | 增加 actions 和本次新增入口的请求记录；复用当前 workspaces、surfaces、lifecycle，不新增 AppCore 服务对象。 |

在 [plugins/mod.rs](../src-tauri/src/plugins/mod.rs) 的 Instance 增加 ui_actions_api 原子布尔值。按已有 settings_api 的方式，在 [process.rs](../src-tauri/src/plugins/process.rs) 接收 initialize 响应时记录 uiActionsApiVersion 是否为 "1"，先写支持标记，再发布 Running。它属于实际进程实例，停用、更新待重启期间不采用新包的能力。

核心 initialize capabilities 增加 ui.actions.v1 和 ui.openView.v1。旧插件忽略新能力；声明了 actions 但进程未确认支持时，只登记为不可用，不发送未知回调。

修改 [layout.rs](../src-tauri/src/component_ui/layout.rs) 的 reconcile_ui：登记动作、刷新可用性，等待插件初始化结束后初始化默认行为；多个有效推荐按 pluginId/actionId 稳定排序并一次保存。已经保存的目标始终保留，不被插件安装顺序、当前页面或 openOnWorkspace 改写。原配置损坏时继续保留原文件并报告错误，不覆盖。

ui_snapshot 增加 actions/defaultCreateAction，以及本窗口工作区的 pendingAction 摘要。查询只读取状态；推荐初始化与清理由现有生命周期入口执行。

## 3. 通用事件和插件主动创建页面

### 3.1 明确接口

| 调用方向 | 接口及参数 | 结果与职责 |
| --- | --- | --- |
| 工作台 → 核心 | component_ui 的 invokeAction：`{context,requestId,actionKey,data?}` | 校验真实窗口、当前 Context、声明和目标实例，登记请求后沿现有 RPC 送达事件。工作台从快照取得默认 actionKey，不提交 viewType。 |
| 核心 → 插件 | ui.action：`{context,requestId,actionId,data}` | 插件解释动作；返回 `{viewId?}` 或错误。无 viewId 时核心不补建页面。 |
| 工作台 → 核心 | component_ui 的 actionStatus：`{context,requestId}` | 查询原回执，返回 pending/completed/failed/invalidated 及结果；不再次投递事件。关闭冻结和断线时允许查询仍存在的原身份。 |
| 插件进程 → 核心 | ui.openView：`{expectedContext,requestId,viewType,resourceKey?,params?}` | 只创建调用插件自己的视图，返回 `{viewId}`；使用真实进程身份，入参没有目标 pluginId。 |

请求 ID 为非空字符串，最长 128 字节。动作 data、创建 params 和缓存结果各最多 16 KiB；检查包含 JSON 信封的现有 24 KiB UI 限制与 64 KiB RPC 限制。核心只检查结构和大小，不解释终端参数。

新分派放在一个 [component_ui](../src-tauri/src/component_ui.rs) 子模块 actions.rs，集中处理动作登记数据、invoke/status 和进程 openView 的请求状态。component_ui::dispatch 增加两个操作；[plugins/dispatch.rs](../src-tauri/src/plugins/dispatch.rs) 只增加 ui.openView 方法。固定使用 ui.action 回调，不提供任意插件方法代理。

### 3.2 复用真实回执，不重发创建事件

复用 [Instance::request_observed 和 Receipt](../src-tauri/src/plugins/mod.rs)：

1. 在短临界区登记 requestId、完整 Context、实际提供者实例、动作标识和输入指纹，创建原回执；释放 workspaces/component_ui 锁。
2. 调用 request_observed，继续使用现有 5 秒等待期限及 supervisor。默认事件不使用 state.changed 的 expire_at_deadline 模式，实际回应或进程退出前保留回执。
3. 读取回执得到实际结果；等待超时且回执未完成时返回 pending。不得把这个状态当成“未执行”。
4. 同编号、同输入的 invokeAction 读取原结果；同编号但动作或输入改变返回参数冲突。只有尚未登记的新编号才发送一次事件。
5. actionStatus 只折算已有回执，不创建线程、任务队列或再次执行事件。插件实际失败后，用户新的操作才使用新编号。

本次请求记录使用完整 Context+requestId 定位，并保存原 pluginInstanceId；查询不重新寻找当前插件实例。比较输入可复用核心已经依赖的 sha2 对规范序列化 JSON 计算指纹，不新增依赖。

每工作区只允许一个未结束的默认行为请求，避免多个窗口重复点击。记录保留到其 Context 结束；每 Context 最多 4096 项、全局最多 16384 项，累计缓存数据最多 16 MiB。到达额度明确拒绝新请求，运行中或已结束的去重记录不被静默清空。极长期工作区达到额度时需关闭并重开工作区；这是本期有界实现的明确限制。

### 3.3 ui.openView 的共同创建路径

进程入口先确认 Instance 仍为 Running 且身份相同，再按 workspaces → component_ui 锁序确认完整 Context 在线、当前真实窗口和工作区未冻结。插件被视为可信扩展，这些是资源状态检查，不新增授权流程。

目标 key 由真实 pluginId 和 viewType 组合；继续校验入口声明、instancePolicy、resourceKey 和参数大小。调用既有 open_view，在同一短临界区提交页面与创建请求结果；释放锁后执行 workspace_changed 通知，不在持锁期间等待 ui.mount。

当 requestId 对应正在处理的 ui.action 时，页面申请记录关联原动作；否则为插件自身的页面申请创建同样有界的请求记录。已关联动作被 invalidated/failed 后拒绝迟到的新页面申请。相同请求及输入返回原 viewId；输入变化明确拒绝；原页面已释放返回 viewClosed，不能用旧编号再创建。

旧 UI open/toggle/locate 接口保持语义。本次不把所有旧插件迁移到新动作接口；新的默认行为与终端再次打开走上述受控路径，不再依赖现有 requests 满 256 后 clear 的字符串缓存。共同的页面登记仍只有 open_view 一处，不复制布局逻辑。

动作返回的 viewId 如存在，必须是相同插件实例、相同 Context 的真实页面；错误引用返回协议错误，不能借此聚焦其他插件或工作区。页面创建完成与 PTY 就绪分开，核心不等待 terminal.open 才完成动作。

插件客户端的等待期限目前为 4 秒。若 ui.openView 已提交但插件未及时收到回应，ui.action 可能报告错误；核心状态必须同时保留已知 viewId，说明“页面已创建，行为报告失败”，不声称没有发生创建，也不自动补建或重试。

## 4. 生命周期接入点

在 Runtime 增加一个按已有身份失效请求的方法，由以下实际入口调用；快照和渲染不驱动失效：

| 入口 | 处理 |
| --- | --- |
| [lifecycle.rs](../src-tauri/src/lifecycle.rs) 的关闭准备/保留范围 | 与冻结同一临界区将范围内未结束动作标为 invalidated，禁止迟到的页面创建。取消关闭不重发或复活动作。 |
| [workspaces.rs](../src-tauri/src/workspaces.rs) 的断线、重绑定、工作区移除 | 断线使旧动作失效；generation 改变/范围释放时回收旧 Context 记录。移窗保留相同 Context 的已受理动作，通知当前真实窗口。 |
| [process.rs](../src-tauri/src/plugins/process.rs) 的异常退出/停止 | 失效该实例的动作；原 Receipt 继续由监督器实际回复或退出清理收尾，新实例不继承它。 |
| [layout.rs](../src-tauri/src/component_ui/layout.rs) 的 remove_views | 将新入口的页面申请结果标为 viewClosed，保留去重记录至 Context 释放。 |
| [lifecycle/host.rs](../src-tauri/src/lifecycle/host.rs) 的宿主丢失 | 沿原关闭范围禁止新增；已创建页面与原生资源继续走既有收尾。 |

动作自身没有独立原生资源，失效不代替资源清理。已申请的页面、PTY、exec 和网络资源仍由既有 Owner/Tasks 及关闭 worker 等待实际结束，不改各服务的释放语义，不用 RPC 回执表示 PTY 已结束。

## 5. 工作台前端整改

修改 [components.ts](../src/components.ts)：

- 删除 defaultEditor/defaultView；补 ActionDefinition、动作投影和 pendingAction 类型。
- “＋”按快照中默认动作的 title、available 和运行状态绘制。未设置、旧提供者、停止或断线显示通用原因，移除“终端不可用”“新建终端”等产品默认判断。
- 点击生成一次 requestId，调用 invokeAction。完成后刷新快照，使用返回的真实 viewId 和原 focusEpoch 规则定位；无 viewId 只结束按钮状态。
- pending 时保留原 requestId，显示“结果尚未确认”，通过 actionStatus 检查。可以按一秒间隔查询，隐藏/移窗/unmount 停止旧页面计时器；重新显示从 pendingAction 快照接续查询。禁止重新投递事件。
- 当前工作区等待中禁用“＋”；后台完成不抢当前窗口焦点，错误恢复后允许用户发起新操作。

保留现有标签、菜单、样式、主题和创建后的焦点安排，不新增默认类型选择 UI。一级服务器选择入口保持。

## 6. 终端插件整改

### 6.1 声明和事件处理

[release.json](../plugins/terminal/release.json) 增加 ui.actions 的 new-terminal/defaultCreate/dataVersion=1。保留现有 views、openOnWorkspace、locateEntry 及旧 createAction 字段，保证旧核心仍能加载。

[main.rs](../plugins/terminal/src/main.rs) 在读取 viewId 之前处理 initialize、settings.changed 和 ui.action。initialize 返回 uiActionsApiVersion=1，并分别记录：基本 PTY/正文能力、ui.openView.v1、terminal.launch.v1，以及自动集成所需 session.exec.v1。

ui.action 只接受声明的 new-terminal。调用本插件的创建参数函数，使用原 requestId 和事件绑定的 Context 调用 ui.openView，返回真实 viewId；这个过程不读取 terminal.open 输出、不持有终端状态锁，不承担 Docker 命令生成。

继续直接使用现有 Client::call，不为两个调用新增 SDK 包装层或生产依赖。

### 6.2 创建配置与能力检查

新增插件内 creation.rs，保存 CreateParams/Launch/DirectoryIntegration 及一个 parse/resolve 函数。它是终端自己的规则，不能移入核心或跨插件 SDK：

```rust
enum Launch {
    LoginShell,
    Command(String),
}

enum DirectoryIntegration {
    Auto,
    Manual,
    Off,
}
```

允许 data/params 为 null 或空对象，使用现有登录终端默认值；显式字段严格检查类型。未知可选字段忽略，未知版本/kind 拒绝。完整 JSON 最大 16 KiB，title 最多 128 个字符且不含控制字符，command 非空白且不含 NUL。

LoginShell 缺省目录策略在首次创建读取 settings.get，保留 auto/manual/off；Command 缺省 Off，显式 Auto/Manual 拒绝。Command 只要求 terminal.launch.v1，直接传入 command；不调用 integration::prepare。LoginShell+Auto 继续调用 prepare，旧核心/不支持 shell 时保留原普通主机终端降级。

动作入口在申请页面前校验；初始 ui.mount 的 null 参数在首次挂载时解析。同 viewId 后续挂载使用保存配置，不重新解释参数或读取启动设置。

### 6.3 保留单次启动与释放

当前 main.rs 在全局 terminals 锁内执行 settings/探测/open RPC。改成 `HashMap<viewId, Arc<OnceLock<Result<TerminalView, String>>>>`：短锁取得实例初始化单元，释放全局锁后单次构造。使用标准库保证两个同时 mount 不重复探测、注入或申请 PTY；不同 view 不互相阻塞。

TerminalView 增加 resolvedCreateParams；integration 的 token、实际 streamId 和 openedContext 不放入再次打开参数。初始化失败在同 view 内保留错误，不自动再执行启动；用户显式新建使用新 view。

dispose 移除初始化单元。已经完成时调用原 terminal.close；正在构造的单元不能重新入表。构造结束时检查原单元是否仍登记，已释放则调用 terminal.close 兜底并返回已关闭；核心原 Owner/关闭检查继续阻止迟到资源。

### 6.4 页面再次打开与目录关闭

[view.js](../plugins/terminal/ui-src/view.js) 保存 mount.data.createParams。主机终端保留“新建终端”；Command 显示“重新打开终端”，使用原 launch/title/目录策略创建新页面。它从当前挂载的真实 Context 发起，不复用旧 streamId 或输入序号。

新核心下，页面通过既有 ui.event 发送 `{action:"createTerminal",requestId,params}`；终端后台复用创建函数并调用 ui.openView。旧核心缺少进程入口时，插件自己的页面继续用原 openView 桥携带 params；由新版终端解析，不能把 command 降级成登录 shell。

[location.js](../plugins/terminal/ui-src/location.js) 的 Off 模式保留必要的 unknown/disabled 状态，不采纳 OSC 7/633 为 known，不启动集成握手或输入 busy 判断。退出、断线和输出恢复仍使旧位置失效；文件插件已有 known 检查可以继续使用，不新增容器路径映射。

只修改 ui-src；构建产物仍由 build.mjs 生成。默认 shell 注入脚本本次预计无需修改。

## 7. 测试、夹具与构建输入

| 位置 | 必须补的验证 |
| --- | --- |
| [component_ui.rs 测试](../src-tauri/tests/component_ui.rs) | 动作声明、旧字段缺省、支持响应、默认推荐保存、多个推荐稳定选择、没有 view 的动作、插件主动选择其他 view、归属与参数冲突。 |
| [plugin.rs 夹具](../src-tauri/tests/fixtures/plugin.rs) | 在 viewId 读取前处理 ui.action；原 stdin reader 持续接收嵌套 ui.openView 响应，用可控回执覆盖超时、迟到、停止与 Context 失效。 |
| [workspace_ui.rs 夹具](../src-tauri/tests/fixtures/workspace_ui.rs) | 在现有 frame pump 增加 pending action/openView 请求映射，发送后返回主读循环；响应到达才回复原 ui.action，期间仍处理 ui.mount。无需给核心增加 SDK 生产依赖。 |
| [test-workspace-ui.mjs](../scripts/test-workspace-ui.mjs) 与[清单](../scripts/fixtures/workspace-ui/manifest.json) | 使用中性插件 ID 和真正动作声明；移除冒用官方 ID 的安排；暂停点改在 invokeAction/原回执，验证重复点击、无页面动作、焦点、失败及移窗。 |
| 终端 creation.rs 单元测试 | 缺省、类型、大小、版本、Command+Off、显式不支持策略、启动配置保留和分开的核心能力。 |
| [terminal_flow.rs](../src-tauri/tests/fixtures/terminal_flow.rs)、[test-terminal-ui.mjs](../scripts/test-terminal-ui.mjs) | 实际包指定交互命令、明确退出、再次打开原命令、无主机探测、同时挂载只一个 PTY、关闭构造中的终端。 |
| [通信组合](../scripts/test-component-communication.mjs) | 指定命令输出目录控制序列不会导航；默认主机自动/兼容跟随、移窗、断线和重连保持。 |
| [select-checks.mjs](../scripts/select-checks.mjs) | 确认新核心模块、终端源码与夹具触发相应现有检查。已有目录规则覆盖则不改工作流；确有遗漏才补路径映射及其测试。 |

交互命令由隔离 SSH 测试服务按远端系统生成，验证 Linux/macOS POSIX 和 Windows PowerShell 的已有会话路径；命令生成只在测试内。源码测试不依赖开发机 Docker，也不假定远端一定是 Linux。

每阶段完成后运行相关 check/scripts:test/test 与实际工作台回归。阶段二追加 plugin:check、终端 Rust 测试、terminal:input:test、terminal:test 和 communication:test。正式构建使用现有 web:build、build 及 plugin:build -- --plugin terminal；构建输入仍由现有脚本复制，核心打包不依赖插件产物。若实际改动触及 shell 脚本，再运行现有真实 Bash/Zsh PTY 入口。

本机结果进入新 implementation 文档；五平台原生核心/终端包、实际包启动/退出及新旧组件组合属于正式交付前门槛，留在既有 Actions 流程执行，不以本机测试替代。Docker 实际 exec/TTY 清理属于后续 Docker 插件任务。

## 8. 实施顺序与确认范围

### 阶段一：默认行为闭环

先补可选声明、进程支持标记及核心通用 invoke/status/openView；用中性夹具完成真实子进程闭环，再迁移终端缺省动作和工作台“＋”。完成声明、回执、关闭、旧组件兼容、工作台回归及核心构建后，以“默认新建行为由插件处理”为一个提交主题。

### 阶段二：终端创建参数闭环

加入 creation.rs、单次挂载初始化、Command 启动和再次打开配置，完成无主机探测/无目录误跟随、真实 PTY 与现有主机回归，再以“终端按创建参数启动并保留再次打开配置”为一个提交主题。

两个阶段完成才作为本任务的完整整改结果。实施中同步 API、usage 和受影响方案，新增一份实施记录，不改历史版本验收结果。实际 Git 提交按用户后续指示执行；本方案的“提交主题”是代码组织约定。

确认本方案即确认以下取舍：新核心不替旧动作提供者恢复官方终端直开分支；新终端保持旧核心上的原入口；指定命令首版关闭目录集成；结果未确认时查询原回执，不再次发送；去重记录在 Context 内保留且有明确容量上限。本次不包含 Docker 实现或发行操作。

本轮只写代码整改方案并检查文档。用户确认前不开始上述产品代码修改。

返回[文档索引](README.md)。

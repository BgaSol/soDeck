# 默认新建行为与终端创建边界修复方案

2026-11-09。状态：代码已实施，本机结果和剩余平台范围见[实施记录](implementation-default-behavior-terminal.md)。本方案修复默认新建入口绑定官方终端、终端创建不读取参数这两处边界。它更新[两级 Tab 方案](plan-tab-creation.md)中二级加号的职责，不改变已有版本的实施记录；当前可调用接口仍以[插件 API](plugin-api.md)为准。

具体接口、文件改动、并发处理、迁移顺序与测试范围见已确认的[代码整改方案](plan-default-behavior-terminal-coding.md)。

## 1. 目标与范围

二级“＋”触发的是默认新建行为。插件声明并处理这个行为，终端是当前提供者；工作台不把默认行为解释为某个插件 ID、某种页面或某条启动命令。

完成后的行为：

- 官方终端声明默认新建行为；点击“＋”触发该行为，终端插件自己请求创建页面，并在挂载时启动终端。
- 另一个插件提供同样的默认行为时，工作台可以使用它，源码不增加插件 ID 分支。
- 终端创建接受调用端对启动方式、标题和目录集成的期望；缺省参数保持现有主机终端体验。
- 指定命令启动时，终端跳过主机 shell 探测和注入；失败明确显示，退出后可按原配置重新打开。

本期保留“点击＋立即触发默认行为”的体验，不增加类型选择箭头、默认类型设置页或记忆当前页面。Docker 插件和 Docker → 终端事件的业务契约另行实施；本方案只准备通用动作接入及终端参数边界，不新增 Docker 专用核心接口、传输通道或生产依赖。

## 2. 已核实的现状

| 现状 | 证据与含义 |
| --- | --- |
| 工作台写死 `sodeck.terminal:terminal`，二级加号直接提交组件 open | [components.ts](../src/components.ts) 的 defaultEditor/defaultView/create。这是旧 Tab 方案的有意实现；现在需从“核心指定视图”改成“触发插件声明的默认行为”。 |
| 新工作区按清单和保存偏好创建初始页面 | [layout.rs](../src-tauri/src/component_ui/layout.rs) 的 reconcile_ui/open_view；终端声明 openOnWorkspace=createOne。这是通用宿主行为，与手动默认新建行为分开。 |
| 页面挂载后，插件决定是否启动业务 | [host.rs](../src-tauri/src/component_ui/host.rs) 调用 ui.mount；[终端 main.rs](../plugins/terminal/src/main.rs) 检查 viewId，准备 shell 集成并调用 terminal.open。同 viewId 重挂载复用原终端。 |
| 核心已经保存并传递 params，终端没有读取 | view.params 随 ui.mount 交给插件；终端新页面一律走主机默认配置。 |
| 已有通信通道，但动作入口仍依赖页面 | 双向 JSON-RPC、ui.event 和 sharedState.v1 已实现；ui.event 依赖已有 surface/view，进程侧只有 ui.updateView，没有主动打开本插件页面的 RPC。不能把尚未实现的无页面动作入口写成现有能力。 |
| 核心已有指定命令的 PTY | [terminal.rs](../src-tauri/src/terminal.rs) 支持 terminal.launch.v1；业务命令由插件提供，核心负责会话、通道及资源状态。 |

## 3. 修复一：默认行为由插件声明并执行

### 3.1 声明的是动作

为现有可选 UI v1 增加可选 actions。名称是本方案的拟定接口，实施时同步 API 文档：

```json
{
  "ui": {
    "apiVersion": "1",
    "entry": "ui/index.html",
    "actions": [
      {
        "id": "new-terminal",
        "title": "新建终端",
        "defaultCreate": true,
        "dataVersion": "1"
      }
    ]
  }
}
```

示例省略已有 views。动作具有插件内稳定 id、用户可读 title、默认新建推荐标记，以及可选的输入数据契约版本。动作不声明目标 view；打开什么、如何打开，由插件处理程序决定。dataVersion 供调用方识别契约，核心不解释业务数据。

initialize 响应增加可选 uiActionsApiVersion="1"，表示进程支持下述 ui.action 回调。核心只有同时拿到有效声明和支持响应才登记可触发动作；不能根据已有 createAction 文案推断旧插件会处理新事件。

在既有 ui-layout.json 增加可选 defaultCreateAction，保存稳定的 pluginId:actionId。未设置且初始化完成时，从 defaultCreate=true 的动作选择推荐值；多个推荐按 pluginId、actionId 稳定排序，一次确定并保存。无推荐时保留未设置，不猜页面、名称或插件枚举顺序。

保存的目标优先，新装插件不能覆盖它。目标停用、异常退出或卸载时保留目标并显示不可用，不自动执行另一个插件的行为。本期不新增用户选择默认类型的界面；旧的布局、固定入口、启动偏好与顺序保持。推荐初始化的读取和写入都复用现有布局持久化与初始化入口，快照查询不写配置。

### 3.2 触发事件，插件随后申请页面

工作台使用动作 title 显示按钮提示、执行中状态和不可用原因，移除 defaultEditor、终端专属按钮文案及通过 defaultView 找目标页面的逻辑。

核心沿现有双向 RPC 送达以下通用动作事件，不先创建页面：

```json
{
  "method": "ui.action",
  "params": {
    "actionId": "new-terminal",
    "requestId": "一次用户操作的稳定编号",
    "context": {
      "workspaceId": "workspace",
      "serverId": "server",
      "connectionId": "connection",
      "generation": 1
    },
    "data": null
  }
}
```

这是已有通信协议上的增量事件入口，不另建消息总线。工作台负责触发；核心通信层按声明校验、绑定真实 Context 和目标实例，再投递。核心不根据 actionId 打开终端，也不解析 data 中的启动选项。现有 ui.event 保持原签名，用于已挂载页面的事件。

终端处理 new-terminal，确定自己的创建参数后，通过新增的通用进程 RPC ui.openView 请求打开本插件视图：`{requestId,expectedContext,viewType,resourceKey?,params?}`。核心从真实调用进程固定 pluginId，只允许操作其自身声明，复用已有 open_view、实例策略、归属、关闭冻结和请求去重。接口不接受目标插件 ID，也不为 Docker 特设分支。

随后沿原链路执行：逻辑页面 → 插件 HTML → ui.mount → 终端插件启动 PTY。动作可返回自己实际打开的 viewId，供工作台按原焦点规则定位；成功返回仅证明页面创建，不等于 PTY 启动成功。无 viewId 的动作也不由核心补建页面。终端启动失败通过原输出通道和页面状态表达。

动作 requestId 贯穿投递、插件处理和 ui.openView。重复送达复用同一创建结果；不同真实点击使用不同编号。沿用已有并发 RPC reader，插件等待 ui.openView 响应时仍能接收 ui.mount，不在跨进程等待期间持有终端状态锁。

### 3.3 初始打开与旧插件兼容

- openOnWorkspace 仍表达新工作区的初始页面布局，保留现有通用创建和 ui.mount 链路。它不决定“＋”触发哪个动作，关闭自动打开也不影响手动默认行为。
- 新终端兼容旧核心：旧核心忽略新增声明/响应字段，原页面挂载和主机终端继续工作。
- 新核心搭配旧终端：已有页面与初始打开继续工作；旧终端不支持 ui.action 时，“＋”明确提示默认行为不可用，需要更新提供者。不恢复核心直开官方终端的绕行分支。
- 独立发行时优先提供兼容旧核心的新终端，再交付新核心。安装包继续各自构建；源码和夹具不能通过冒用官方终端 ID 掩盖默认行为选择问题。

## 4. 修复二：终端创建读取调用端参数

### 4.1 第一版参数契约

动作 data 与终端视图 params 使用同一份终端创建契约。插件接收、校验和归一化；核心把业务部分作为普通 JSON 数据转交。

```json
{
  "version": 1,
  "title": "api · 容器终端",
  "launch": {
    "kind": "command",
    "command": "docker exec -it <container-id> /bin/sh"
  },
  "directoryIntegration": "off"
}
```

命令为示意，容器标识、用户、shell 选择及正确转义由调用方负责。本期不在终端或核心加入 Docker 命令生成。

| 字段 | 规则 |
| --- | --- |
| version | 缺省为 1；不支持的版本明确拒绝。调用方须检查动作声明的 dataVersion，不能由核心的 PTY 能力推断旧终端支持创建参数。 |
| title | 可选；非空、最多 128 个字符，不含控制字符。缺省使用现有编号标题；不要求 shell 输出推断标题。 |
| launch | 缺省为 loginShell；支持 `{kind:"loginShell"}` 或 `{kind:"command",command:string}`。未知 kind、缺失命令、纯空白命令和 NUL 明确拒绝。 |
| directoryIntegration | loginShell 缺省读取当前终端设置，允许 auto/manual/off；command 缺省 off，首版只接受 off。显式要求其他模式返回不支持，不能默默改变调用方期望。 |

整个创建参数序列化后最多 16 KiB，继续满足现有控制帧和 UI 信封限制。未知可选字段忽略；改变已知字段含义或增加必填字段时提高契约版本。校验发生在 shell 探测、页面申请和 terminal.open 等副作用之前。

主机登录模式保留现有 auto/manual/off 及旧核心降级。指定命令模式只检查 terminal.launch.v1，不要求 session.exec.v1，不运行主机 shell 探测，也不写主机目录集成文件。缺少指定命令能力时拒绝启动，不回退登录 shell。自动主机集成则继续检查所需的两个核心能力。

### 4.2 保存与再次打开

终端插件抽出一处参数校验与归一化，默认动作、带参数动作、初始 ui.mount 和页面再次打开共用规则。只在首次创建决定参数和读取启动设置；重复挂载使用 TerminalView 保存的配置，不因重新读取设置改变已创建终端。

TerminalView 保存确定后的启动方式、标题及目录策略，挂载 data 返回再次打开所需的参数。外观设置仍按已有实时规则应用。主机兼容模式的主机名配置继续沿用当前设置行为；首版不增加容器目录映射或每页主机名设置。

指定命令终端退出后保留输出，提供“重新打开终端”；此操作在当前有效 Context 中创建新 view、新 PTY 和新 shell 集成标识，继承原启动配置，不恢复原进程。普通主机终端保留“新建终端”的原体验。切换页面、隐藏、移窗、重连和恢复输出都不自动执行再次打开。

command + off 不解析或发布主机目录的 known 报告；初始状态明确为目录集成关闭。文件侧栏因此不会把容器路径当成主机路径。命令成功启动但容器中没有所选 shell等情况，保留实际退出码/错误，调用方后续自行决定如何选择其他 shell。

## 5. 归属、并发与失败处理

| 情况 | 处理要求 |
| --- | --- |
| 默认行为未设置或提供者不可用 | “＋”不执行，展示通用原因；一级服务器入口和已有页面保持可用。 |
| 参数非法或不支持 | 插件在创建前拒绝，不探测 shell、不申请 PTY；用户看到具体错误。初始挂载的非法 params 保留宿主错误页供关闭。 |
| 动作重复送达或页面重复挂载 | requestId 复用页面结果，viewId 复用终端；不增加页面、PTY、探测任务或注入文件。 |
| 投递等待超时 | 标明结果未确认，保留原 requestId；重试同一次操作不得改编号。不自动再次执行。受理结果和待处理项必须有界，运行中结果不能为了腾位置被提前丢弃。 |
| 页面已关闭后重试旧操作 | 返回原操作已结束，不新建或复活页面；新的用户操作用新 requestId。实施时检查并修正既有请求映射的回收边界。 |
| 等待期间切换工作区或操作其他页面 | 原工作区可以完成创建；沿用焦点 epoch 检查，迟到结果不抢焦点。 |
| 关闭冻结、移窗、断线或 generation 改变 | 申请页面和资源时都检查真实实例及 Context；冻结/旧 Context 不提交新业务资源，取消关闭不重放已受理动作。 |
| 插件异常退出、停用或工作区关闭 | 通信等待失效，页面和 PTY 沿原生命周期清理；新实例不继承旧动作执行身份。 |
| PTY 申请或启动命令失败 | 页面保留错误和已有输出；不切换为主机 shell，不自动选择另一个命令。 |

核心保存页面、动作受理事实、连接和原生资源；终端保存创建配置和业务状态。其他插件只是触发方，其页面关闭不自动销毁终端已创建的页面。主机认证资料与完整 Context 均不放入业务启动配置供调用方覆盖。

共享状态继续发布运行事实。当前 sharedState.v1 保存快照并允许变化合并，不能仅靠 state.put 成功宣布默认行为执行成功，也不能把读取旧快照视作一次新的点击。本方案的工作台默认行为沿现有 RPC 投递事件；Docker 事件的送达、去重和回执需在其接入方案中明确，不凭已有 changed 回调声称已完成。

## 6. 代码落点与实施顺序

| 改动 | 代码落点 |
| --- | --- |
| UI 动作声明、支持响应、默认行为持久化及通用事件投递 | [component_ui.rs](../src-tauri/src/component_ui.rs)、[layout.rs](../src-tauri/src/component_ui/layout.rs)、[host.rs](../src-tauri/src/component_ui/host.rs)、[插件清单](../src-tauri/src/plugins/manifest.rs)、[进程](../src-tauri/src/plugins/process.rs)与[RPC 分派](../src-tauri/src/plugins/dispatch.rs) |
| 进程主动打开本插件页面、请求与生命周期检查 | 复用 component_ui 的 open_view、ui_action 和 workspaces Context；[plugin-client](../crates/plugin-client/src/lib.rs)仅按现有 SDK 模式补最小调用辅助 |
| “＋”触发动作、通用文案、执行中及焦点反馈 | [components.ts](../src/components.ts)，删除默认官方终端 ID；不重做工作台布局 |
| 终端动作处理、创建配置、能力分开检查与重挂载 | [终端 main.rs](../plugins/terminal/src/main.rs)、[integration.rs](../plugins/terminal/src/integration.rs)、[release.json](../plugins/terminal/release.json) |
| 输出页面再次打开及关闭目录集成 | [view.js](../plugins/terminal/ui-src/view.js)、[location.js](../plugins/terminal/ui-src/location.js)；ui 构建产物仍由原脚本生成 |

按两个完整主题分别实施与提交：

1. **默认行为闭环。** 先用中性测试插件验证声明 → 事件 → 插件主动申请本插件页面 → 挂载；迁移官方终端的缺省新建行为，移除工作台默认终端硬编码，保留主机使用体验。
2. **终端创建参数闭环。** 实现与验证参数规则、指定命令启动、目录关闭和再次打开；旧入口使用缺省参数，完成所有终端调用点迁移。

实施时同步修改受影响的 plugin-api、usage、组件 UI/Tab 方案段落，新增一份 implementation 记录实际证据并更新索引。方案阶段不把新字段和方法写成已实现，不改应用/插件版本、README 首页、发布工作流或历史发行记录。

## 7. 验收与交付条件

| 验收场景 | 必须观察到的结果 |
| --- | --- |
| 中性插件提供默认行为，处理程序选择自己的另一个视图 | “＋”只送达动作，由插件决定打开什么；宿主不会按动作名称或某个 view ID 先创建页面。 |
| 默认行为只做处理并返回无 viewId | 不凭声明补建页面，准确结束按钮执行状态。 |
| 存在其他插件和多个页面、当前编辑文件 | 点击“＋”仍只触发保存的默认行为，不依赖当前页面；文件草稿保留。 |
| 更新顺序、旧清单/旧响应、默认提供者停用或崩溃 | 原页面和初始主机终端保持；不支持的新入口明确不可用，不调用未知回调或静默回退。 |
| 未传 params、设置 auto/manual/off、旧核心 | 原主机 shell、设置降级和目录联动回归保持。 |
| 带参数启动可辨识的交互命令 | 标题与启动配置正确；没有主机探测或自动注入；输入、复制粘贴、Ctrl-C、尺寸和退出状态正确。 |
| 非法参数、未知版本、缺少能力、命令失败 | 无主机回退；副作用发生前拒绝非法配置，实际启动失败留在原页显示。 |
| 指定命令输出 OSC 7/633 | 目录关闭模式不发布主机 known 路径，文件侧栏不导航。 |
| 重复事件、暂停回包后重试、挂载与移窗 | 一个操作最多产生一个逻辑页面和 PTY；再次挂载不重启；结束操作不被旧事件复活。 |
| 退出后再次打开、断线后重连 | 明确点击才执行原启动配置；新 Context、新页面、新 PTY；不重放旧输入。 |
| 启动与关闭/插件退出交错 | 迟到资源不能登记到已关闭对象，实际清理完成，原连接不被错误关闭。 |

默认行为使用核心/宿主集成夹具，移除 [test-workspace-ui.mjs](../scripts/test-workspace-ui.mjs) 为适配硬编码而冒用官方终端 ID 的安排。终端参数至少增加 Rust 参数回归、实际终端包 UI/PTY 及目录关闭组合回归；测试命令使用隔离会话，不依赖开发机 Docker。

实施阶段运行相关格式、类型、构建和测试：npm run check、npm run plugin:check、npm run scripts:test、npm run test、node scripts/test-workspace-ui.mjs、npm run terminal:input:test、npm run terminal:test、npm run communication:test，以及核心/终端正式构建。若触及现有 shell 注入脚本，追加真实 Bash/Zsh PTY 回归。

正式交付仍要求五个客户端系统/架构的原生构建、打包和实际包启动/退出；这些不能由本机测试或交叉编译替代。真实 Docker 的 shell 选择、双层 TTY 尺寸传递和远端 exec 清理属于 Docker 插件接入验收，本期通用参数测试不冒充这部分通过。

本次产物是修复方案；仅执行文档链接、示例 JSON 和差异检查，产品行为尚未变化。

返回[文档索引](README.md)。

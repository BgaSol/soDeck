# 服务器工作区与终端二级标签：代码修改设计方案

状态：待用户确认，2026-10-22。本轮仅分析代码、编写方案，未实施业务代码修改。

功能依据：[服务器标签与终端标签分层设计](server-terminal-tabs-design.md)。

## 1. 结论与实施边界

这是一轮跨前端、Rust 运行时和跨窗口协议的结构调整，不能仅将现有顶端标签按服务器分组显示。

推荐采用：**服务器工作区唯一登记 + 多个独立终端 + 独立的服务器工具上下文 + 工作区整体迁移**。

- 一个保存的 `server.id` 在当前应用实例中只有一个工作区，覆盖主窗口和独立 SSH 窗口。
- 每个终端继续使用自己的 SSH 连接、PTY、输出通道、重连状态和历史记录。首期不改为单一 SSH transport 上复用多个 shell，避免把连接底层重构叠加进来。
- 文件、监控、Docker、NetBird 使用工作区拥有的已验证连接上下文，不绑定活动终端，也不绑定“终端 1”。
- 关闭最后一个终端保留工作区及工具；迁移以整个工作区为单位。
- 容器终端仍是现有独立窗口类型，不加入服务器的二级 SSH 终端列表；它的打开入口改用工具上下文。
- 不增加分屏、广播输入、单终端拆窗或跨重启自动恢复连接。

不采用“每个二级标签再嵌套一套 `SessionWorkspace`”的方式：这会重复文件树、监控和业务提供者；也不把共享面板临时接到第一个存活终端上，因为关闭、重连和目录身份仍会发生跳变。

## 2. 已核实的现状与隐含依赖

| 位置 | 当前事实 | 修改影响 |
| --- | --- | --- |
| `src/workspaces.ts` | `WorkspaceTab` 是服务器快照 + 单个会话；`forceNew` 允许同服务器多个顶端标签 | 必须拆分服务器工作区和终端实体，移除创建重复服务器页的路径 |
| `src/App.tsx` | `open`、`createConnection`、`pending`、`receiveStatus` 和 `discardPending` 都以一个顶端标签代表一次连接；取消待连接流程会移除标签 | 首页导航状态与终端连接状态需要分开，失败和取消不能误删整个服务器工作区 |
| `src/SessionWorkspace.tsx` | 只保存一个 `sessionId`、`useConnection`、终端控制器和恢复快照；共享工具都收到这个 `sessionId` | 拆成服务器容器与单终端容器 |
| `src/TerminalPane.tsx` | 卸载及连接 effect 清理会调用 `close_connection`；`prepareMove` 会 freeze 单个终端 | 切换标签必须保留组件实例；移动需聚合所有终端并明确移交清理 |
| `src/useConnection.ts` | 每实例有重连调度器、attempt、认证提示和连接 ID；恢复快照在迁移提交后才恢复调度 | 每个终端独立复用，同时增加连接排队及认证协调 |
| `src-tauri/src/lib.rs::State::spec` | 从 `State.terminals` 查找会话，校验窗口和存活状态，取得 `SharedContext` | 工具访问不能继续依赖此终端入口；但终端输入权限不能因此放宽 |
| `connection.rs::ConnectionContext` | 保存经过验证的跳板链、凭据、指纹及 route identity；本身不是一个长期存活的 SSH socket | 可独立保留上下文；各业务原有 `connect_verified` 仍负责建立其需要的连接 |
| `connection.rs::finish` | 终端结束会调用 `monitor.close_session` 与 `netbird.close_session` | 工具生命周期必须迁出该清理路径 |
| `monitor/mod.rs` | `Core.members` 以终端 ID 为成员，存活检查直接遍历 `State.terminals` | 仅改命令入参不够，监控内部成员和授权判断也必须改 |
| `DockerManagementProvider`、`NetBirdProvider`、`MonitorProvider` | 根据 `sessionId` 创建/切换 store 和订阅 | store 的稳定键改为工具上下文，不能是活动终端 ID |
| `useRemoteFileTree.ts` | 按 `sessionId` 重新创建文件树模型 | 改用稳定的工作区工具上下文，保留浏览状态 |
| `remoteFileTree.ts::acceptTerminalDirectory` | 校验目录事件的 sessionId，并自动 `revealPath`；sequence 按单会话比较 | 需分离“更新目录标记”和“定位浏览位置”，目录序号必须带终端及连接身份 |
| `src/useWorkspaceMoves.ts`、`src-tauri/src/moves.rs` | 快照只有 `terminal`，Transfer 只有一个 `session_id`；一个 onReady 就能提交迁移 | 改为集合式接收屏障、原子提交和整体回滚 |
| `terminal_history/*` | 持久化 `workspace_id` 实际来自旧的单终端工作区 ID；真实连接记录用 session ID | 新的服务器 workspace ID 不能直接替换进去，否则多个终端的历史组会混在一起 |
| `TerminalHistoryView.tsx` | 内嵌视图当前也查询全部 sessions，默认选中 binding 的 session；可主动按来源搜索 | 要显式区分本终端记录、服务器来源范围与全局搜索 |
| `desktop_drag/*` | 文件视图、跨服务器复制授权、远程剪贴板都保存/校验终端会话身份 | 切换到工作区工具身份，同时保留 owner、generation 和 route identity 校验 |
| `transfers/mod.rs`、`State::connected_context` | 一些恢复流程从存活终端中寻找相同 route identity 的上下文 | 无终端但工具可用的工作区也必须能恢复这些任务 |
| `lib.rs`、`updates/install.rs` | 删除配置、数据库重置、退出、页面刷新、窗口销毁和更新审查依赖终端注册表 | 增加工作区及工具上下文的检查与释放，不把工具上下文统计成终端 |

### 对前一份功能稿的事实更正

前稿“沿用显式定位”的现状表述不准确：当前文件树会自动跟随终端目录。本方案明确将默认行为改为：**接收目录事件只更新标记，用户点击‘定位到当前终端目录’才改变文件浏览位置**。这是随本方案一并确认的行为调整，需要新增明确入口及回归测试。

前稿“工具已连接”也需细化：`SharedContext` 代表已验证、可供业务建立连接的上下文，不等于所有工具始终有一个存活 socket。界面使用“工具可用 / 需要验证 / 连接异常”，具体监控是否在线由真实采集状态决定。

## 3. 核心不变量

以下约束同时由状态模型、后端校验和测试保证：

1. 一个 `serverId` 只有一个非关闭工作区；零终端、连接失败和迁移中的工作区仍占用唯一登记。
2. 每个终端只属于一个工作区，每次连接尝试只属于一个终端；同终端最多一个有效连接尝试/存活会话。
3. 输入、尺寸调整、清屏、认证和剪贴板发送必须校验具体终端会话，不能只凭 serverId 或工具 contextId。
4. 切换终端不更换工具上下文，不重新创建共享 store，不卸载后台终端。
5. 关闭终端不会撤销服务器工具上下文，也不会停止其他终端。
6. 移交提交前目标工作区不可交互；全部终端接收完成后才一次性提交，源窗口随后卸载。
7. 关闭/重连/迁移后晚到的异步响应不能更新新对象、夺取焦点或复活旧连接。
8. 已验证凭据只在 Rust 内存和原有受控存储中存在，不进入 React 状态、窗口迁移 JSON、全局事件或诊断日志。

## 4. 标识、状态及所有权模型

### 4.1 区分六类标识

| 标识 | 含义与生命周期 |
| --- | --- |
| `serverId` | 持久化服务器配置 ID；只用于配置归属与唯一工作区索引 |
| `workspaceId` | 本次打开的服务器工作区 ID；关闭后重开生成新的 ID |
| `terminalId` | 二级标签 ID；重命名、重连和迁移期间保持不变 |
| `connectionId` / `sessionId` | 一次实际 SSH 连接/PTY ID；重连后变化，仍沿用现有终端协议语义 |
| `ToolContextRef { id, epoch }` | 工具使用的上下文句柄及版本；epoch 变化使旧请求、草稿和授权失效 |
| `WindowOwner { label, generation }` + `ownerEpoch` | 窗口页面代次，以及该工作区自己的归属代次；两者分别校验 |

`requestId` 用于幂等动作，`attempt` 用于终端连接代次，`transferId` 用于迁移事务；三者不互相替代。TS 侧使用明确类型及字段名，Rust 侧使用独立结构，避免一个裸字符串在不同域间混用。

移动一个工作区不能增加整扇窗口的 `generation`，否则会让同窗口其他服务器的请求失效。工作区迁移/回滚使用独立 `ownerEpoch`；页面刷新/销毁才使用窗口 generation。工具 context epoch 则只表达工具上下文换代。

### 4.2 建议对外视图

以下为接口设计，字段命名可在实施时按仓库惯例整理，语义不得混合：

```ts
interface ServerWorkspaceView {
  id: string;
  server: Server;                     // 最新配置的公开展示信息
  owner: { label: string; generation: number; ownerEpoch: number };
  revision: number;
  lifecycle: 'opening' | 'active' | 'moving' | 'closing';
  terminals: TerminalTabView[];
  activeTerminalId: string | null;
  nextTerminalNumber: number;
  tools: WorkspaceToolContextView;
}

interface TerminalTabView {
  id: string;
  number: number;
  title: string | null;                // null 时显示默认编号
  attempt: number;
  connectionId: string | null;
  state: TerminalConnectionState;      // 类型化状态，由展示层转为中文
}

interface WorkspaceToolContextView {
  ref: ToolContextRef | null;
  state: 'unavailable' | 'verifying' | 'available' | 'error';
  connection: ConnectionView | null;   // 公开的实际连接信息，不含凭据
  configurationChanged: boolean;
}
```

### 4.3 权威数据放置

- Rust 新增工作区注册表，持有 `serverId → workspaceId`、窗口所有者、终端成员、连接关联、生命周期、版本号及工具上下文。新增/关闭/迁移的合法性以它为准。
- 标签名、编号与顺序等小型元数据通过工作区命令更新并随工作区视图传递；后端分配编号，前端不并发推算 `max + 1`。
- React 保留窗口内的导航、MRU、布局、共享面板展示状态及 xterm 实例；使用后端视图更新本地投影。活跃终端可立即本地切换并同步元数据；这不会创建或关闭资源。
- 连接恢复调度继续由各终端 `useConnection` 负责；后端保存真实连接阶段，等待重试等前端展示状态通过带 attempt/owner 的状态报告同步，用于其他窗口的首页摘要。过期报告丢弃。
- 全局工作区事件仅广播必要元数据与 revision；终端字节输出继续走该终端自己的 Channel，不广播到所有窗口。
- 窗口先建立事件监听，再读取当前工作区列表；应用事件按 revision 去重，发现缺口重新读视图。不能把单次事件送达当成跨窗口登记成功的唯一凭证。
- 关闭、销毁和登记失败的资源释放有明确路径，不依赖 React 必然执行 cleanup。

## 5. Rust 工作区登记与打开协议

建议新增 `src-tauri/src/workspaces/` 模块，分为 `mod.rs`、`model.rs`、`commands.rs`、`contexts.rs`、`lifecycle.rs`、`tests.rs`，避免继续扩大 `lib.rs`。

### 5.1 核心命令

| 命令或接口 | 责任 |
| --- | --- |
| `open_server_workspace(serverId, intent, requestId)` | 原子查找/登记工作区；intent 为进入或新建终端；返回工作区及 owner |
| `list_server_workspaces` / `get_server_workspace` | 提供跨窗口摘要与权威快照 |
| `create_workspace_terminal(workspaceId, requestId)` | 校验 owner 和生命周期，分配唯一 terminalId/递增编号 |
| `update_workspace_terminal` / 排序命令 | 校验成员、名称、排序集合与 revision，不接收任意资源句柄 |
| 扩展 `open_connection` | 接收 workspaceId、terminalId、attempt、requestId，绑定后端登记；serverId 从登记取得或交叉校验 |
| `prepare_workspace_close` / `close_server_workspace` | 返回真实关闭影响；提交时复核版本和操作租约，执行统一关闭 |
| `close_workspace_terminal` | 取消排队或连接中请求、结束真实连接、移除成员；支持幂等重试 |
| `refresh_workspace_context` | 仅重新验证工具上下文，不创建终端，不运行 shell |
| 工作区事件/定向导航事件 | 更新视图、定位已有工作区、通知某窗口执行已登记的动作 |

具体 Tauri 命令可适当合并，但不能去掉后端唯一登记、成员校验、幂等和窗口身份校验。

### 5.2 普通打开与跨窗口路由

1. 原子查询 serverId；没有时先预留 `opening` 工作区，并登记首个终端。
2. 同时到达的第二个打开请求取得同一工作区。无论连接状态如何，都不能再创建顶端标签。
3. 已有工作区时，“进入”仅激活；“新建终端”在现有工作区登记一个终端。
4. 工作区属于另一窗口时，向 owner 发送定位事件并 show/unminimize/focus 该窗口。`request_workspace_connection` 不再无条件转发给主窗口新建。
5. 新建动作必须先在 Rust 成为持久于本次运行的元数据，再通知 owner；事件丢失后可由视图重读恢复。相同 requestId 返回同一结果。
6. 等待迁移时返回明确的迁移状态并暂缓新建；不在另一个窗口新开副本。普通进入可在迁移完成后定位最终 owner。
7. 预留后前端未挂载、窗口刷新/销毁时释放预留；有界的超时回收只作用于尚未初始化的 opening 记录，不能回收正在用户认证的合法工作区。

首页 `pending` 改为导航意图 `{workspaceId, terminalId, attempt, navigationSerial}`，不再充当工作区的唯一连接管理器。连接失败或取消只更新对应终端；用户已切走时，成功仅更新状态。

## 6. 工具上下文：本次最重要的解耦

### 6.1 上下文不需要隐藏终端

当前 `SharedContext` 已可被文件编辑器、日志及后台任务独立持有；`connect_verified` 会依据它建立实际连接。因此不需要新建一个隐藏 PTY 来维持工具。

推荐流程：

1. 首个终端完成 SSH 认证后，在同一有效 workspace/attempt 下向工作区发布已验证 `SharedContext`。
2. 工具上下文登记在认证完成后、PTY 启动前完成。即使 shell 启动失败，工具仍可能可用；界面分别显示这两个结果。
3. 工作区取得独立 `Arc<ConnectionContext>`，不以终端句柄是否存在作为授权条件。
4. 后续终端认证成功不得无条件覆盖工具上下文，防止不同连接快照或晚到响应改变文件操作目标。
5. 最后一个终端结束只释放终端资源。工具上下文继续有效，业务仍按原路径创建/复用自己的连接。
6. 没有有效工具上下文时，用户可在工具区“连接工具 / 重新验证”，通过不创建 PTY 的交互连接流程取得新上下文；临时验证连接完成后释放。

`connect_interactive` 和 `ConnectEventSink` 已有不依赖 PTY 的基础接口，可以复用。不能直接把验证任务塞进 `State.terminals`：否则退出统计、输入权限和终端历史都会被污染。验证任务建立独立注册、取消及认证回复路由。

### 6.2 授权接口拆分

- 保留/明确 `terminal_spec(sessionId, owner)`，供终端输入、resize、清屏、attach 等操作使用。
- 新增 `workspace_context(ToolContextRef, owner)`，校验工作区 owner/generation/ownerEpoch、context epoch、生命周期和具体 route identity。
- 工作区级工具命令统一接收 `context`，不再把工具 ID 伪装成 `sessionId`。前后端同时升级，不提供任意跨类型 ID 的兜底解析。
- 独立编辑器、日志、容器终端和已登记后台任务在创建时取得自己的上下文租约/Arc；之后使用它们各自现有的窗口或任务授权。
- `connected_context(identity)` 统一通过明确的有效上下文入口查找；保留按 route identity 精确匹配的约束。文件任务恢复代码不得继续私自遍历 `State.terminals`。如需兼容独立容器会话作为既有来源，必须明确登记该来源，不能放宽到同 serverId 任意匹配。

### 6.3 凭据、配置变化与连接目标

首次连接继续使用现有逐跳认证和指纹流程。新终端可先尝试使用相同连接计划下已验证的凭据；必须实际核对指纹，并在认证失效时回到该终端的交互流程，不能绕过验证。

注意：当前 `InteractiveProvider.pinned` 主要用于指纹比较，并不会自动复用所有凭据。实现必须显式设计“计划匹配 → 使用验证凭据 → 失败后交互”的路径，不能仅传入 pinned 就假定已完成该能力。

配置编辑处理如下：

- 名称、标签等展示字段直接刷新，不重连。
- 新终端及重连读取最新配置；已运行终端保留真实连接快照。
- 现有工具上下文保持原目标，标明“工具连接配置已更新，当前仍使用原连接信息”，提供“更新工具连接”。不能因某个新终端连接成功便静默切换文件操作目标。
- 显式更新工具上下文先取得新验证结果，检查需要旧上下文的操作，再一次性换代。新验证失败保留旧上下文；成功后旧请求的结果不再更新新面板。
- 同 route identity 的凭据刷新可保留目录和展示状态，将缓存标记为待刷新；route identity 改变则清空目标相关缓存、选择和预览，防止把旧主机文件展示为新主机内容。
- 当前终端的 route identity 与工具上下文不同，禁止“定位到终端目录”，说明目标不一致；尤其不能只因两边 serverId 相同就执行跨目标路径操作。

### 6.4 工具与独立对象的所有权

| 资源 | 工作区关闭时 | 工作区迁移时 |
| --- | --- | --- |
| 文件浏览、监控/Docker/NetBird 的前端订阅 | 撤销工作区订阅 | 源暂停，目标提交后重新订阅同一工具上下文 |
| 普通只读目录/查询请求 | 取消或丢弃迟到结果 | 不序列化 Promise；目标按状态补查 |
| 尚未提交的删除、Docker、NetBird、传输预览 | 撤销对应 owner 的预览和 token；有用户草稿时在 UI 处理取消/保留选择 | 首期有业务模态草稿时阻止迁移，避免静默丢失用户输入；普通列表选择可迁移 |
| 已进入远端提交阶段、仍依赖工作区验证的操作 | 纳入关闭阻塞项；不因清理 UI 而取消或重跑 | 同样阻止迁移，完成或在对应操作中处理后再移动 |
| 已交给应用任务管理器的上传/下载/服务器复制/删除等 | 保持现有独立任务所有权及取消流程；关闭说明列出仍在运行的数量 | 任务继续，目标重新订阅任务列表；不重复提交 |
| 已提交 Docker/NetBird 操作 | 根据实际执行阶段和独立租约判断；不得仅用一个全局 busy 标志推断安全 | 只有完全脱离工作区 owner 校验的阶段可继续迁移，否则阻止；结果未知保留核对入口 |
| 独立文件编辑器、日志及容器终端 | 保留其上下文及窗口，按原有流程工作 | 不随服务器页迁移，也不关闭 |
| 独立监控窗口 | 保留窗口及已显示数据；工作区关闭后没有有效成员时转为断开 | 继续观察相同 identity，迁移本身不导致服务器离线 |
| 端口转发 | 沿用独立 manager 生命周期 | 继续运行 |

不将 `Arc` 尚有引用直接解释为“任务可安全脱离”：有些操作会在稍后再次调用 owner/session 校验，必须检查整条调用链。独立租约需保存真实 route identity、操作阶段和取消句柄。

## 7. 前端组件与状态拆分

建议结构：

```text
App / DetachedApp
├─ WorkspaceTitlebar → WorkspaceTabs（服务器层）
├─ WorkspaceConnectionCoordinator（连接队列、认证展示协调）
└─ ServerWorkspace（由当前 SessionWorkspace 演进）
   ├─ WorkspaceLayout / 布局 reducer / 焦点管理
   ├─ WorkspaceServicesProvider（稳定的 ToolContextRef）
   │  ├─ FilePanel + RemoteFileTree
   │  └─ MonitorProvider / DockerManagementProvider / NetBirdProvider
   ├─ TerminalTabs（新增，二级导航）
   └─ TerminalSession[]（新增，每个稳定 terminalId 一个实例）
      ├─ useConnection
      ├─ 当前终端工具、连接状态及详情
      └─ TerminalWorkspace → LiveTerminalPane / TerminalHistoryView
```

`TerminalWorkspace.tsx` 已负责单终端的实时/历史切换，继续复用该职责，不把二级标签再塞进这里。

### 7.1 生命周期与焦点

- 所有已打开的终端按稳定 `terminalId` 挂载，只有关闭或成功移交后卸载。不能 `key={activeTerminalId}` 重建唯一终端。
- 生效条件统一为：工作区激活、二级终端激活、中央区域可见、非迁移、非相关模态占用。
- `useWorkspaceFocus` 从单一 controllerRef 改为获取当前终端 controller；原先记住的 DOM 若来自隐藏终端，不再恢复到它。
- 后台终端继续接收字节与记录历史；隐藏终端不发送零尺寸，不接收输入。显示时复用现有串行 resize 队列，发送最后有效尺寸。
- 异步粘贴确认绑定 terminalId + sessionId + input epoch；确认期间切终端、重连或迁移均使旧目标失效，不能把文本发到新的活动终端。
- MRU 在服务器层和每个服务器内部各自维护；关闭后台标签不改变激活项。

### 7.2 两级标签栏

- 顶端保留 `WorkspaceTabs`，移除终端编号标题逻辑；菜单和 aria 名称使用“服务器工作区”。
- 新增 `TerminalTabs.tsx` 和局部样式，支持单行溢出、全部终端菜单、重命名及局部排序；默认编号由后端成员元数据提供。
- `useTabStrip` 已按自身 track 内的 `data-tab-id` 查找元素，可供两层独立实例复用；依赖 `tab-${id}` 的是 `WorkspaceTabs` 中的聚焦逻辑，应分别使用各层 ID 前缀，避免把滚动 hook 做不必要的重构。
- 两层 DOM ID 分别使用 `server-tab-*`、`terminal-tab-*`，对应不同 tabpanel。
- 顶端可复用现有原生跨窗口拖动；终端标签只做同工作区内排序，不调用 workspace 的系统拖出流程。
- 端口转发入口和服务器工具放在服务器级位置；终端断开/重连、历史、复制、粘贴、清屏绑定具体终端。
- 首页 `ServerHome`、`ServerActions`、`ServerDetails` 改读全局工作区摘要；底部改为“X 个服务器 · Y 个终端 · Z 个已连接”。容器终端仍在应用退出统计中单独按真实终端计算。

### 7.3 文件目录与缓存

`RemoteFileTreeSnapshot.sessionId` 与工具 API 改为 context 引用；目录标记另外保存 `{terminalId, sessionId, routeIdentity, sequence, path, home}`。

- 新增 `setActiveTerminalDirectory(cwd | null)`，只更新标记，不自动 reveal。
- sequence 仅在同 terminalId、同 sessionId 内比较；切换到另一个终端即使序号较小，也必须接受其目录。
- 后台终端目录更新记录在该终端状态中，不改变当前文件标记或浏览路径。
- 当前终端退出、断开或尚未上报目录时清除标记，禁用显式定位。
- 新增“定位到当前终端目录”操作，调用既有 `revealPath`；保留 SFTP 对符号链接和 `..` 的现有路径解析约束。
- 工具 context epoch 改变时区分同目标刷新与目标切换；异步目录结果校验 context epoch + 请求代次，不能覆盖新目标。

## 8. 连接、认证和重连协调

### 8.1 保留 per-terminal 状态机

继续复用 `ConnectionRecovery` 的退避、停止及迁移暂停语义。每个终端有独立 request/attempt/prompt/view/error，工具验证任务有独立状态。共享的是协调器，不是各终端的连接状态。

### 8.2 排队与模态

- 首期同一服务器工作区同时只允许一个交互建连流程，包括工具重新验证；后续终端可以立即生成标签，显示“等待连接”。其它已连接终端照常工作。
- 不同服务器可独立建立网络连接；同一窗口只展示一个认证对话框，工作区和终端名称必须明确显示。
- 未轮到展示的认证 challenge 不能套用当前 180 秒“已提示用户”的计时。协调器需要明确 queued/presented 状态：排队时间单独有界，用户交互超时在实际展示后计时；取消、窗口销毁或后台拒绝均可释放队列。
- 认证令牌绑定 owner generation + workspaceId + terminalId/工具验证任务 ID + attempt + challengeId。回复前后都核对，保留“下一个 challenge 在前一个 IPC 返回前到达”的现有正确行为。
- 自动重连需要认证时先显示待处理状态，不在后台强行激活窗口；用户进入该终端或点击提示后再展示。
- 关闭等待中的终端立即撤销排队；关闭认证中的终端取消当前任务并推进队列。其它终端的认证不会被整体清空。
- 网络整体中断时，各终端保留输出；调度器继续退避，并通过工作区队列限制同时重连。恢复不重跑用户命令。

连接请求成功的 ID 可能晚于 Ready 事件返回；关闭也可能发生在这个间隙。保留现有前端取消保护，并增加后端 membership/attempt 检查，使迟到 ID 没有机会重新挂入已关闭终端。

## 9. 关闭、退出与后台资源释放

### 9.1 关闭二级标签

1. 根据真实连接/排队状态生成确认内容，连接中可以取消；已结束的终端直接关闭。
2. 确认后先停止该终端调度、使输入/粘贴及 attempt 失效，再调用后端 `close_workspace_terminal`。
3. 后端原子标记终端关闭，取消队列/验证、发送连接结束信号，并拒绝之后到达的连接创建。
4. 确认资源进入终止流程后移除视图；cleanup 允许幂等收尾，但不能依赖它作为唯一关闭行为。
5. 保留历史归档；零终端时显示空态，共享工具实例继续存在。

### 9.2 关闭服务器工作区

采用“读取影响 → 用户确认 → 提交时复核”的协议：

- 影响信息包括终端数量、连接/认证中数量、阻塞操作及独立继续运行的任务。
- 提交携带 revision/确认令牌；确认期间新增终端、开始关键操作或迁移会使确认过期，必须更新影响，不能用旧数字关闭新资源。
- 所有新建、关键工具操作及迁移须通过同一个工作区生命周期闸门取得许可。提交关闭时先进入 `closing`，再复核已取得的操作租约，防止检查后又开始远端修改。
- 阻塞项存在则保持工作区可用，提供具体任务及处理入口。成功后结束所有终端、释放工作区订阅与上下文登记、撤销草稿/文件视图/跨窗口 grant。
- 独立对象持有的上下文不撤销，其资源在自己的生命周期结束后释放。
- 若已开始结束终端后发生清理错误，不能声称“取消后恢复全部连接”。保留 closing/错误状态、给出实际结果并允许幂等重试；已结束会话不会伪造恢复。

### 9.3 所有关闭入口统一

- 主窗口顶端关闭、二级关闭、菜单关闭、键盘 Delete 统一进入同一关闭服务。
- 独立 SSH 窗口的标题栏关闭与系统 Alt+F4/关闭请求必须走工作区级确认，不能继续直接 `window.close()` 后依靠 Destroyed 断开。
- 主窗口退出仍是应用级退出，统一收尾所有工作区、验证队列及现有独立任务；不逐工作区重复弹确认。
- 页面刷新/窗口异常销毁通过 `WindowOwner.generation` 使旧请求失效，回滚进行中的迁移并清理对应登记；新增模块必须接入 `on_page_load`、`Destroyed` 和 `shutdown_work`。
- 删除配置及重置数据库检查包括零终端工作区和工具验证任务，以及引用该配置的跳板链；不得通过“没有终端”绕过现有占用检查。
- 在线更新影响审查计入工具验证/迁移/关键操作，保持 `terminals` 数量只代表真实终端；更新确认摘要发生变化时沿用现有重新审查机制。

## 10. 多终端跨窗口迁移协议

这是另一项必须独立验证的核心改动。

### 10.1 快照 V2

```ts
interface ServerWorkspaceSnapshotV2 {
  version: 2;
  workspace: ServerWorkspaceView;
  terminals: Array<{
    terminalId: string;
    terminal: TerminalSnapshot;
    recovery: RecoverySnapshot;
    connectionPreferences: ConnectionPreferences;
  }>;
  files?: FilePanelSnapshot;
  docker?: DockerPanelSnapshot;
  netbird?: NetBirdPanelSnapshot;
  layout: WorkspaceLayoutSize;
  tools: WorkspaceToolState;
  resources: ResourcePreferences;
}
```

服务上下文只传句柄与公开信息，真实凭据留在 Rust。前端快照不能授权创建后端不存在的终端；成员集合、session 归属、context epoch、源窗口及 revision 均由后端核对。

### 10.2 准备与接收

1. 后端创建工作区迁移事务，锁定成员集合及目标 owner，冻结新建/关闭/重新验证。正在连接、认证或处理不具备独立租约的修改操作时拒绝迁移。
2. 源端暂停每个终端的重连计时和共享订阅；分别 freeze 所有存活 session，并等候各自 xterm 输出屏障后序列化。断开的终端只保存视图，不建立新连接。
3. 任一 freeze/序列化失败，恢复所有已经暂停的终端及服务，不留下半冻结状态；不要使用只等待首个 rejection 的并发写法而遗漏后来成功 freeze 的终端。
4. 继承现有 **24 MiB 的总快照限制**，按完整 V2 payload 检查；超限整体恢复并提示。不能逐个终端各允许 24 MiB、也不能静默截断输出。
5. 目标窗口按 terminalId 恢复全部组件，逐个 attach；活动与后台终端都要完成接收。零终端工作区也能完成迁移。
6. `useWorkspaceMoves.ready(workspaceId)` 改为成员集合屏障：全部终端恢复完成，文件/布局状态已安装，目标的工具句柄验证完成后，才允许请求最终提交。工具网络采样不必已成功，但目标输入和操作仍禁用。
7. 不能把第一个 `Ready` 事件或 Channel.send 成功等同于目标 xterm 已消费快照/重放。为每个存活终端发送带 transferId/token 的接收屏障，目标在前序 xterm 写入回调完成后确认；断开的终端确认本地快照恢复完成。后端记录全部确认，屏障之后的新输出继续按原流顺序投递。

### 10.3 原子提交与回滚

- Rust `Transfer` 从 `session_id` 改为验证过的 session 集合、workspaceId、源/目标 owner、contextRef 和成员版本。
- 目标 owner 在 take 阶段可以取得受限的恢复/attach 权限，不能执行输入或业务修改。源在迁移中同样不可交互。
- 提交先对**所有** stream 执行 `ready_to_commit`，再统一切换；检查与提交期间按稳定 sessionId 顺序持有所需锁，不能提交第一个之后才发现第二个不可提交。
- 同一事务中转移 workspace/context/terminal 所有权并增加工作区 ownerEpoch；窗口 generation 保持其页面语义不变。随后通知源卸载。源 cleanup 识别 transfer 状态，不再关闭已经移交的会话。
- 提交后目标统一恢复重连调度与共享订阅，重建文件视图和跨窗口授权；旧 owner 的预览与 grant 失效。
- 取消、目标崩溃、接收失败或超时，恢复全部流到源并重放 freeze 后输出；保留已有 `TerminalStream` 的回滚重放和 4 MiB 背压机制。回滚也更新 ownerEpoch，源重新绑定订阅/文件视图，旧异步回调不会因 owner label 恢复相同而重新获得权限。
- 源已销毁时不能回滚为可见的源工作区：按明确策略结束未完成移交的资源并移除登记；不留幽灵 owner。成功提交后源窗口关闭不影响目标。
- 接收与提交应可查询最终事务状态；重复 ready、晚到的失败事件或通知丢失不反转已经提交的事务。

### 10.4 锁与系统调用

现有迁移锁顺序为 `pending → terminals → stream`。新增工作区生命周期后建议明确为 `configuration（需要时）→ workspace registry → moves.pending → terminals → streams（按 ID）`。统一在模块注释与测试中规定。

监控/任务等 manager 不得持有自己的 core 锁后反向获取已被迁移路径持有的 registry 锁；采用先取可验证快照、释放 manager 锁、再校验并按版本应用的方式。网络调用、用户交互等待、窗口创建/聚焦/销毁以及事件通知均放在 registry 锁外。

## 11. 历史兼容与持久化

首期无需修改服务器数据库或重新写入旧历史。

- 保留历史数据库 `sessions.workspace_id` 的既有“终端连续使用分组”语义。新记录写入稳定 `terminalId`，不是新的服务器 workspaceId。
- 新 `open_connection` 同时携带真正 workspaceId 和 terminalId；调用 recorder 时显式使用 terminalId，不能继续把同名字段直接透传。
- 前端历史适配层将旧 IPC/存储中的 `workspaceId` 标注为历史终端分组字段；业务组件使用清晰的 `terminalGroupId`，避免与服务器工作区混淆。
- 每次重连仍有新的 session ID，保留启动/结束及会话边界；同 terminalId 的多次连接可以归为本终端历史。
- 内嵌历史默认展示当前终端组的记录；用户主动选择服务器来源或全局搜索时才扩大范围。每个终端保存自己的历史位置和实时/历史模式。
- 全局历史使用服务器来源、开始时间和必要的短 session ID 区分并发终端；当前运行时可补充已知标签名，重启后不能假装恢复未持久化名称。
- 容器终端保持独立历史来源与旧数据读取规则；clear、copy、replay、export 仍以实际 session ID 定位。
- V2 工作区快照是运行期协议，不是跨版本磁盘格式。应用升级需正常重启，不承诺在线合并旧版存活会话。旧单终端快照测试夹具全部升级，运行时对未知版本明确拒绝，不猜测缺省字段后接收半个工作区。

## 12. 修改文件与调用链清单

以下是计划范围，实施前通过编译器和 `rg` 校核所有调用点；“复查”项只有触及对应契约时修改，不进行无关重构。

| 范围 | 主要文件 | 修改要点 |
| --- | --- | --- |
| 前端工作区模型/IPC（新增及改造） | `workspaces.ts`、新增 `workspaceApi.ts`、`useServerWorkspaces.ts` | 类型化实体、后端事件投影、幂等打开、跨窗口摘要、选择与 MRU |
| 应用入口 | `App.tsx`、`DetachedApp.tsx` | 移除按终端创建顶端标签的路径，改造 pending、菜单、关闭、统计与窗口定位 |
| 服务器与终端容器 | `SessionWorkspace.tsx` 演进为 `ServerWorkspace.tsx`；新增 `TerminalSession.tsx`、`TerminalTabs.tsx`、`WorkspaceCloseDialog.tsx` | 共享提供者提升；单终端状态下沉；关闭协议 |
| 导航及样式 | `WorkspaceTabs.tsx`、`WorkspaceTitlebar.tsx`、`WorkspaceTabMenu.tsx`、`useTabStrip.ts`、`useWorkspaceTabDrag.ts`、`workspace-windows.css`、`workspace-layout.css`、新增终端标签样式 | 两级 DOM/aria/溢出、局部与跨窗口拖动分离 |
| 终端与认证 | `TerminalPane.tsx`、`TerminalWorkspace.tsx`、`useConnection.ts`、`connectionRecovery.ts`、`ConnectionDialogs.tsx`、`types.ts`；新增协调器 | 身份参数、取消保护、工具验证提示、连接队列、隐藏保活 |
| 焦点与剪贴板 | `useWorkspaceFocus.ts`、`WorkspaceInteractionContext.tsx`、`terminalClipboard/*` | 获取当前终端 controller、异步粘贴目标失效、模态归属 |
| 首页 | `ServerHome.tsx`、`ServerActions.tsx`、`ServerDetails.tsx`、`ServerDeleteDialog.tsx` | 进入/新建终端/独立窗口语义，零终端工作区占用及统计 |
| 文件与挂载 | `FilePanel.tsx`、`useRemoteFileTree.ts`、`remoteFileTree.ts`、`useRemoteMounts.ts`、`remoteMounts.ts`、`api.ts`、`fileApi.ts` | context 参数、缓存代次、目录标记/显式定位、旧请求保护 |
| 文件任务与跨服务器操作 | `transferApi.ts`、`nativeDrag.ts`、`CopyTargetDialog.tsx`、`FileOperationDialog.tsx`、`DeleteFilesDialog.tsx`、文件任务相关视图 | context 授权和任务身份展示，恢复与重试入口；禁止把旧 sessionId 当新工具 ID |
| 监控 | `MonitorProvider.tsx`、`resourceMonitoring.ts`、`monitorWindowApi.ts`、`ResourcePanel.tsx`、`SessionConnectionDetails.tsx` | store 稳定键、实际工具连接信息、独立观察窗口语义 |
| Docker | `DockerManagementProvider.tsx`、`dockerManagement.ts`、`dockerApi.ts`、`DockerPanel.tsx`、`docker/*` 相关 API 调用 | 工具上下文、预览/提交 epoch、独立任务租约 |
| NetBird | `netbird/NetBirdProvider.tsx`、`netbirdStore.ts`、`netbirdApi.ts`、`NetBirdPanel.tsx` 及操作视图 | 与 terminal finish 解耦；订阅、认证、草稿失效 |
| 前端历史 | `terminalHistoryApi.ts`、`TerminalHistoryView.tsx`、`TerminalWorkspace.tsx` | 分组语义适配、独立位置、同时间并发记录识别 |
| 前端迁移 | `useWorkspaceMoves.ts`、工作区/终端控制器及快照类型 | 多终端屏障、分阶段回滚、恢复所有调度器 |
| Rust 运行时（新增） | `workspaces/*` | 全局唯一登记、工具上下文、命令、关闭影响、操作闸门 |
| Rust SSH/PTY | `connection.rs`、`terminal.rs`、`lib.rs` | 终端成员与 attempt 绑定、认证后发布工具上下文、验证任务注册、历史分组、清理分离 |
| Rust 文件链路 | `remote_fs.rs`、`remote_mounts.rs`、`files.rs`、`remote_delete.rs`、`transfers/mod.rs`、`transfers/download.rs` | context 入口、异步后置校验、恢复来源、租约和 close blockers |
| Rust 拖放/剪贴板 | `desktop_drag.rs`、`desktop_drag/targets.rs`、`desktop_drag/clipboard.rs` | file view、grant、source/target 从终端身份换成工具身份；迁移撤销旧授权 |
| Rust 监控 | `monitor/mod.rs`、`commands.rs`、`mounts.rs`、`docker.rs`、`windows.rs` | 成员从终端变为工具上下文、存活/授权、迁移暂停、独立窗口状态 |
| Rust Docker / NetBird | `docker/commands.rs`、`docker/tasks.rs` 及有关 draft；`netbird/commands.rs`、`manager.rs`、`operations.rs` | session 查找替换、预览身份、操作租约、订阅清理 |
| Rust 迁移 | `moves.rs`、`terminal.rs` | Transfer 集合、V2 校验、全部 preflight、事务提交/回滚 |
| 独立窗口入口 | `lib.rs::open_container`、`logs.rs::open_container_logs`、`files.rs::open_editor`、监控窗口入口 | 开窗前授权改用工具上下文，开窗后保留独立所有权 |
| 配置/退出/更新/诊断 | `lib.rs`、`updates/install.rs`、`diagnostics/commands_allowlist.rs`、`diagnostics.ts`、`QuitDialog.tsx`、更新展示类型 | 注册新命令、生命周期清理、计数/占用审查、标识脱敏 |
| 复查权限与基础能力 | `capabilities/*.json`、`port_forwarding/*`、`traffic.rs`、全部 `State` 构造夹具 | 不扩展窗口权限白名单；转发与流量保持独立；新 State 字段补齐测试构造 |

需要保留现有未提交的系统设置改动；本方案不把它们并入本功能。具体测试文件同样按当前工作区内容增量修改，不能覆盖其它任务正在修改的测试。

## 13. 分阶段实施与每阶段通过条件

这些是确认后的一轮完整实施步骤，不是只交付前几步的缩减范围。中间阶段保持可编译、可回归，所有强制项完成后才交付。

| 阶段 | 内容 | 通过条件 |
| --- | --- | --- |
| 1. 契约与后端注册表 | 身份类型、工作区索引、终端成员、owner、request 幂等、生命周期闸门；补运行时构造 | 并发打开不重复，旧 owner 不可写，取消/关闭晚到请求不复活 |
| 2. 工具上下文解耦 | context resolver、认证后发布、无 PTY 验证、各业务 API 和 manager 改造、任务恢复 | 单终端 UI 下先验证：结束该终端后工具可用，敏感操作权限不变 |
| 3. 双层前端与连接协调 | 拆组件、二级标签、首页/摘要/焦点/目录行为、排队认证 | 多服务器多终端切换保活、输入不串、工具 store 不重建 |
| 4. 关闭与各类资源审查 | 终端关闭、工作区关闭、独立窗口关闭、后台租约、退出/更新/删除 | 最后终端、取消确认、阻塞操作及独立任务的行为符合方案 |
| 5. 整体迁移 | V2 快照、多终端冻结/接收/提交、所有权迁移及失败注入 | 无半迁移、无输出重复/丢失、失败全体恢复、零终端可移动 |
| 6. 历史、兼容与完整验收 | 历史分组、旧库读取、夹具升级、全链路与真实桌面验证 | 下述必测场景通过，明确记录真实验证范围和剩余限制 |

不在过渡期把“只有当前活动终端被迁移”的实现暴露为可用功能。必要时在集成分支暂时禁用迁移入口，最终交付前必须恢复并通过整体迁移验收。

## 14. 测试方案与失败注入

### 14.1 单元与 Rust 集成测试

- 工作区注册：同 serverId 并发 open、新建的 requestId 去重、零终端占用、取消首次连接、名称相同 ID 不同、关闭后重开。
- 身份权限：错误 terminalId/workspaceId/context epoch/window generation、伪造跨服务器成员、旧窗口在 take/commit/rollback 后的读写权限。
- 连接：独立 attempt、迟到 ID/Ready/Closed、同一终端重复开始、队列撤销、多跳认证、上一个回答返回前下一个提示到达、工具验证没有 PTY/历史/终端统计。
- 工具：关闭首个/当前/最后终端，监控和 NetBird 不被 terminal finish 清理；Docker/file store 不因切终端重建；工具重新验证失败保留旧上下文。
- 目录：同终端递增 sequence、不同终端小序号、同终端重连归零、后台目录变化、不同 route identity、断开清除标记、显式定位及旧目录响应。
- 任务：无终端时恢复传输/核对删除，跨窗口复制 grant 换代，关键操作与关闭/迁移同时发生，已独立任务在工作区关闭后继续。
- 迁移：第 N 个 freeze/attach 失败、某终端接收后其它终端失败、迁移中远端退出、已断开+重连等待+存活终端混合、零终端、24 MiB 超限、目标超时/刷新、源销毁、重复提交、提交通知丢失、4 MiB 背压及全部流回滚重放。
- 生命周期：断开/关闭幂等，删除跳板配置检查，零终端工作区阻止删除，窗口刷新 generation，退出验证任务与队列收尾，更新影响快照变化。
- 历史：旧 catalog 读取，新 terminalId 分组，重连记录独立，迁移不新建历史 session，关闭后历史仍可搜；容器历史回归。

新增/更新 `workspaces.test.ts`、终端状态及协调器测试、`remoteFileTree.test.ts`、`connectionRecovery.test.ts`、`terminalClipboard/*.test.ts`；Rust 新增工作区及集合迁移测试，并更新 `connection_tests.rs`、`terminal.rs`、monitor/Docker/NetBird/transfer 相关测试。

### 14.2 浏览器端 IPC 仿真

更新 `tests/connection-ui.spec.ts` 中单顶端标签即单终端的假设，重点是同服务器重复新建、迁移、独立窗口请求、自动恢复和焦点测试。`terminal-fixture.ts` 及各文件/监控/Docker/NetBird fixture 同时支持工具 context 与终端 session，不能只让返回字段“凑够编译”。

增加二级标签专项用例，覆盖：

- 同服务器 5 个终端只出现 1 个顶端标签；A/B 两服务器互不影响。
- 标签新建、重命名、排序、MRU、关闭取消、最后终端空态、双层溢出和键盘导航。
- 后台持续输出、实时/历史切换、粘贴确认期间切页/重连、模态阻断、窄窗口侧栏焦点。
- 文件位置保持、明确定位到当前终端、关闭最后终端后文件/监控/Docker/NetBird API 仍使用同工具身份。
- 无终端工作区的首页进入、独立窗口定位、统计及配置删除占用。

执行受影响的现有脚本：`test:connection`、`test:workspace-tools`、`test:tree`、`test:resources`、`test:mounts`、`test:docker`、`test:netbird`、`test:transfers`、`test:terminal-history`、`test:terminal-clipboard`、`test:database`、`test:updates`；并回归独立编辑器/日志/转发入口对应测试配置。

### 14.3 真实 SSH 与 Windows 桌面

IPC 仿真不能证明 SSH 生命周期或原生迁移正确，交付需另外记录：

1. 两台测试服务器、同服务器至少 3 个终端：普通 shell、持续输出、全屏程序；切换、断开、重连与关闭互不串扰。
2. 所有终端关闭后继续列目录、刷新监控、读取 Docker/NetBird，独立编辑器保存及已启动传输正常。
3. 主窗口与独立窗口往返移动，确认真实 session ID 不变、远端进程持续、无重复 shell、历史无重复分段；再验证失败回滚。
4. 网络中断、跳板认证、代理重连、指纹/密码变化，以及后台终端待验证时的焦点行为。
5. 系统 Alt+F4、目标窗口异常结束、应用退出；Windows 125%/150% 缩放、最小主窗口/独立窗口、深浅主题及原生拖拽。
6. 多终端持续输出的资源观察：以 3 个服务器、每台 5 个终端为基准，确认共享监控/业务订阅不会随终端数成倍增加；xterm 内存与输出接收按终端增长但无泄漏。记录实测，不预设不存在依据的性能保证。

前端执行 `npm test`、`npm run build`，Rust 执行相关单测与 `cargo check`，最后构建 Tauri 应用。Windows Rust 测试沿用仓库已有 Common Controls v6 清单方案。当前文档编写阶段未运行这些产品测试，不能把此前交互示意的验证当作本功能已通过验证。

## 15. 风险、约束与交付门槛

| 风险 | 防护与验收门槛 |
| --- | --- |
| 只替换 API 参数，遗漏 manager 对真实终端的存活检查 | 对 `.spec(`、`connected_context`、`state.terminals`、`sessionId` 调用链逐项归类；无终端工具验收为强制项 |
| 多窗口同时打开或迁移时产生重复服务器页 | 后端唯一索引、requestId 幂等、事务内 owner 更新及全局事件版本 |
| 标签切换卸载终端或把异步输入送错终端 | 稳定组件 key、独立 controller、session/attempt/input epoch 校验 |
| 工具连接被新终端静默换到另一个主机 | 工具上下文固定归属、显式刷新、route identity 校验及换代清缓存 |
| 多终端迁移部分提交，源/目标同时拥有输入权 | 完整集合校验、全部 preflight、事务提交、目标提交前 inert、源移交清理 |
| 关闭页误停任务，或任务仍在使用已失效 owner | 操作阶段与独立租约核查、关闭影响复核、明确阻塞项、独立任务持续运行用例 |
| 历史组混合、旧记录不兼容 | 保留存储语义，terminalId 显式映射，旧库读取和并发同时间记录测试 |
| 加锁顺序扩大后死锁、原生窗口回调重入 | 统一锁顺序，manager 锁外校验，原生调用与网络 I/O 不在 registry 锁内 |

最终交付至少包含：代码及增量测试、接口/状态说明、真实验证记录和已知限制。未完成共享工具解耦或整体迁移，不视为该功能完成。

## 16. 本次待确认的具体方案

确认本方案即确认以下实现约定：

1. 唯一性覆盖同一应用实例的所有 SSH 工作区窗口，按保存配置 ID 判断。
2. 终端各自连接；工具上下文独立保留，关闭最后终端不关闭服务器页。
3. 文件目录默认从自动跟随调整为仅标记 + 显式定位。
4. 独立窗口移动整台服务器及全部终端，首期不支持单终端拆窗。
5. 涉及关键远端提交或业务模态草稿时，关闭/迁移按上述规则提示并暂缓；已经独立的任务保持自己的生命周期。
6. 连接配置变化后工具上下文显式更新；不会随着活动终端变化而切换工具目标。
7. 按六个阶段完整实施，并以工具生命周期、输入隔离、历史兼容和迁移回滚作为必须通过的验收项。

用户确认前，只保留本设计文档，不开始上述业务代码及测试改造。

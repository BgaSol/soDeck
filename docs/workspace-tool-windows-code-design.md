# 工作区工具窗口代码设计方案

日期：2026-10-21。状态：用户已最终认可并授权实施，前端实现已完成。

承接 [界面优化方案](./workspace-tool-windows-design.md)。以下保留获批代码方案；实际交付与验证范围见 [实施记录](./workspace-tool-windows-implementation.md)。

## 1. 实施目标与决策

工作区采用固定左右工具栏：左侧“远程文件”，右侧依次为 Docker、服务器状态、NetBird。工具入口可打开、切换和收起对应面板，终端获得剩余空间。主窗口与独立 SSH 窗口共用同一实现。

本轮确定以下实现选择：

1. 新会话默认请求展开文件、收起右侧；极窄窗口首次优先显示终端。
2. 右侧工具选择继续使用 `ResourcePreferences.tab`，内部保留 `monitor` 标识，对用户显示“服务器状态”。
3. 布局意图和实际可见性分开。自动让位不改用户开关，也不覆盖偏好宽度。
4. 面板保留挂载，收起不销毁业务组件或 Provider。展示层与订阅兴趣分别更新。
5. 当前运行期间按会话保存状态，随现有移动快照跨窗口传递。重启恢复布局不纳入本轮。
6. 首版采用即时布局调整与短淡入，不增加拖拽开关面板手势或动画依赖；分隔条仍实时跟随指针。
7. 保留现有连接、终端历史、文件任务、端口转发和独立业务窗口流程。

## 2. 当前代码中的关键约束

| 已核实的实现                                                                                              | 对本次设计的影响                                                             |
| --------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `workspaceVisibility()` 在 three 模式固定返回三栏可见                                                     | 需要替换可见性推导，不能只给按钮加 CSS                                       |
| `WorkspaceLayout` 内测量宽度，再通过 `onVisibilityChange` 回传父组件                                      | 改为父级布局 Hook 统一提供解析结果，避免内容已隐藏但业务仍认为可见的中间状态 |
| `ServicePanel` 拥有横向 tablist                                                                           | 移除这层跨工具导航，工具栏按钮与内容区域统一关联                             |
| `MonitorProvider.active` 控制会话采集可见性；Docker 和 NetBird Provider 独立持有任务状态                  | 工具关闭不能直接停止这些 Provider                                            |
| `TerminalPane.tsx` 导出的 `TerminalPane` 实际由 `TerminalWorkspace` 包装                                  | 焦点接口和尺寸优化需兼顾实时终端与历史模式                                   |
| 实时终端在 active/interactive 变化和连接 ready 时自动 focus；`App.tsx` 切会话后也查询 textarea 强制 focus | 需要集中管理工作区焦点，避免窄窗口恢复终端时抢走其他有效焦点                 |
| ResourcePanel 与 DockerPanel 在详情恢复 active 时也会 focus 返回按钮                                      | 仅显式详情导航自动聚焦；被动显隐不能无条件聚焦                               |
| NetBirdPanel 的 active 变化会清除 preview/pending，peer 与 diagnostics 又受 active 条件挂载               | 需拆开可见性与连接身份失效，保留收放期间的输入及结果                         |
| DockerDialogFrame 已 portal 到 body，文件与 NetBird 的若干 dialog 仍位于面板 DOM 下                       | 布局隐藏不能让仍打开的模态对话框落入 hidden/inert 祖先                       |
| 移动 IPC 使用 JSON Value 传递前端快照                                                                     | 可以扩展前端布局字段，预计不需要修改 Rust 协议或数据库                       |

## 3. 状态模型

### 3.1 用户意图

新增 `src/workspaceTools.ts`，负责类型、归一化与纯 reducer。以下为拟定接口，不是已落地代码：

```ts
type ServiceTool = ResourcePreferences["tab"];
type ToolSide = "files" | "services";
type CompactPane = "terminal" | ToolSide;

interface WorkspaceToolState {
  version: 2;
  filesOpen: boolean;
  servicesOpen: boolean;
  lastSide: ToolSide;
  compactPane: CompactPane;
}

interface WorkspacePresentationState {
  tools: WorkspaceToolState;
  layout: WorkspaceLayoutSize;
  resources: ResourcePreferences;
}

const defaultTools: WorkspaceToolState = {
  version: 2,
  filesOpen: true,
  servicesOpen: false,
  lastSide: "files",
  compactPane: "terminal",
};
```

`SessionWorkspace` 用一个 reducer 持有 tools、layout、resources，替换目前各自独立的 view/layout/resources 状态。这样“选择监控 + 展开右侧 + 设置指标详情”可以作为一次更新完成。资源工具选择只存在于 `resources.tab`，不再复制到 tools。

`lastSide` 只表示空间不足时哪个侧栏优先，不能当作 DOM 焦点。`compactPane` 仅决定极窄模式当前显示哪一块，不能改变终端历史/实时模式。

连接、文件模型、Docker/NetBird 快照和任务仍由现有所有者管理，不放进这个 reducer。

### 3.2 事件与转换

| 事件                                 | 转换规则                                                                                                               |
| ------------------------------------ | ---------------------------------------------------------------------------------------------------------------------- |
| `toggle-files`                       | 当前实际可见则设 `filesOpen=false`；已请求打开但被自动隐藏时，应显露文件而非关闭它；打开时更新 lastSide 和 compactPane |
| `toggle-service(tool)`               | 当前该工具实际可见则收起右侧；否则设置 resources.tab、servicesOpen=true，并将 services 设为优先侧和窄屏当前区          |
| `close-side(side)`                   | 明确关闭该侧意图；窄屏正在显示这一侧时回到 terminal                                                                    |
| `return-to-terminal`                 | 仅把 compactPane 设为 terminal，保留两侧意图，便于放宽窗口后恢复                                                       |
| `open-metric(metric)`                | 设 monitor、selectedMetric、detailOpen，打开 services；重复点击是定位操作，不收起                                      |
| `reveal-file(path)`                  | 打开 files，并产生新的定位请求序号；终端 CWD 通知不走该事件                                                            |
| `set-size(side, width)`              | 仅修改该侧用户偏好尺寸；先校验有限值，再按范围限制                                                                     |
| `replace-resource-preferences(next)` | 维持 ResourcePanel 等现有 onChange 语义；不因时间范围、网卡变化改变侧栏开关                                            |
| 容器宽度/字号变化                    | 只重新解析布局，不写回用户意图或偏好尺寸                                                                               |

开关事件携带点击时该侧是否实际可见，来源只能是当前解析结果。reducer 用最新状态判断右侧选择，避免连续操作依赖旧闭包。显式定位使用幂等打开事件，不能复用 toggle。

只容得下一个停靠面板时，关闭当前侧后，另一侧若仍有打开意图，允许恢复显示；两侧都关闭才成为纯终端布局。极窄模式的“返回终端”优先于这一规则。

### 3.3 派生布局

在 `workspaceSizing.ts` 中用一个纯函数替换旧 fit + visibility 组合：

```ts
type PanelPlacement = "hidden" | "docked" | "overlay" | "full";

interface ResolvedWorkspaceLayout {
  mode: "docked" | "overlay" | "single";
  files: { placement: PanelPlacement; width: number };
  services: { placement: PanelPlacement; width: number };
  terminalVisible: boolean;
  visible: WorkspaceVisibility;
  resizeBounds: {
    fileWidth: { min: number; max: number } | null;
    serviceWidth: { min: number; max: number } | null;
  };
}

resolveWorkspaceLayout({ width, metrics, tools, preferred }):
  ResolvedWorkspaceLayout;
```

所有 visible、CSS placement、按钮展开状态和业务 active 从这一结果读取。不要维护另一份可见性 state。

## 4. 布局算法与尺寸

基准字号 14px 下，初始值为：工具栏每侧 44px，分隔条命中区 10px，终端舒适宽度 560px；文件默认 280px、范围 240–520px；服务默认 360px、范围 320–480px；极窄阈值 760px。

尺寸常量使用一套定义，并生成 CSS 变量。新增 `useWorkspaceLayout.ts`，通过 ResizeObserver 测量外层工作区；用可测量的 rem 标尺同步字号变化，将像素化指标交给纯函数。零尺寸的后台工作区保留上一次有效测量，初次尚未测量不臆造 1440px 的展开布局。

解析步骤：

1. 从工作区宽度扣除两侧工具栏；当前显示的分隔条才占宽度。
2. 低于缩放后的 760px，进入 single。按 compactPane 显示终端或一个已请求打开的工具，其他内容隐藏；无效目标回退终端。
3. 其他宽度先检查所有已请求侧栏的偏好尺寸之和。若加上分隔条与 560px 终端仍能容纳，则全部停靠。
4. 放不下两侧时，以 lastSide 为优先，若该侧没请求打开则选择另一侧。只展示该侧，另一侧暂时隐藏。
5. 如果优先侧的最小宽度能与舒适终端共存，临时把该侧渲染宽度限制到剩余预算内并停靠。不要覆盖偏好值。
6. 连一个侧栏最小宽度也放不下时，进入 overlay。该侧覆盖在终端上，靠近自己的工具栏，不额外占据终端列宽，不使用模态遮罩。
7. overlay 宽度限制在中间区域内，并留出边缘间距；内部滚动。single 工具内容使用工具栏之间的完整宽度。

默认两侧均请求打开、服务为优先侧时，预期示例：

| 工作区宽度 | 解析结果                                   |
| ---------- | ------------------------------------------ |
| 1440px     | 文件 280 + 服务 360 同时停靠，终端约 692px |
| 1280px     | 服务停靠，文件临时隐藏                     |
| 1024px     | 服务 360px 停靠，终端约 566px              |
| 860px      | 服务浮层覆盖，终端保持基础宽度             |
| 650px      | single，显示 compactPane 指定内容          |

该表基于两侧打开的测试条件；新会话右侧关闭时，会优先保留文件停靠。阈值取决于偏好宽度、字体与实际内容区域，不直接使用浏览器 viewport 替代。

分隔条只在 docked 显示。保留 Pointer Capture、方向键调整、双击恢复默认值；拖动起点从当前渲染宽度开始，最大宽度保证当前显示组合和终端下限，避免拖动中另一侧突然被挤走。pointercancel、失去 capture 和会话切换都结束拖动。临时缩窄不写回，用户明确拖动才更新偏好。

## 5. 组件结构与接口

```text
SessionWorkspace
  ├─ presentation reducer
  ├─ useWorkspaceLayout / useWorkspaceFocus
  └─ 现有 Monitor / Docker / NetBird Providers
       └─ 工作区交互 Context
            └─ WorkspaceLayout
                 ├─ WorkspaceToolRail（左）
                 ├─ 文件 slot → FilePanel
                 ├─ 终端 slot → TerminalWorkspace → LiveTerminalPane / 历史
                 ├─ 服务 slot → ServicePanel → 现有三个业务面板
                 └─ WorkspaceToolRail（右）
```

### WorkspaceLayout

作为受控展示组件，接收 `resolved`、工具选择、开关/收起/尺寸回调以及现有内容 slots。宽度观察器由 SessionWorkspace 的 Hook 持有，根元素 ref 传入 WorkspaceLayout。

三个内容 slot 和其 React 父子关系固定。通过 data-placement、grid 列宽、position、hidden/inert 切换形态；不要把同一业务组件在停靠树和浮层树之间搬家，也不要把终端按模式渲染为多个实例。

工具栏各自为独立 grid 列；文件与服务隐藏时相关列及分隔条归零。浮层在工作区内定位，位于工具栏内侧。极窄模式增加“返回终端”标题动作。嵌套滚动区域统一 `min-width: 0`、`min-height: 0`。

### WorkspaceToolRail（新增）

纯展示组件，接收工具描述、实际展开状态、按钮 ref 和激活回调。用现有 lucide-react 的 Folder、Boxes、Activity、Network，固定顺序。

按钮稳定名称分别为“远程文件”“Docker”“服务器状态”“NetBird”。用 `aria-expanded`、`aria-controls` 指向当前会话的内容 region；ID 带工作区唯一前缀。同一个右侧共享容器可以被三个按钮控制，容器标题随工具变化。

按钮的高亮表示实际展开，自动隐藏的工具不显示为已展开。tooltip 鼠标悬停与键盘聚焦均可见；不依赖原生 title 作为唯一提示。隐藏工具不影响入口可操作性。

### ServicePanel

删除原横向 tablist 和 tabpanel 语义，改为一个有标题的工具窗口框架，顶部包含当前工具名与收起动作。内部保留三个稳定内容容器，以 hidden/inert 切换，保留现有子面板自己的区域名称和内部业务导航。

不保留两套跨工具切换入口。内部 Docker 分段、NetBird 返回按钮和资源详情返回按钮继续负责本工具内导航。

### FilePanel 与各业务面板

- FilePanel 增加标题尾部收起动作；single 模式提供返回终端入口，保留原路径和文件操作区。
- 服务通用框架负责跨工具标题；业务面板保留服务器、刷新及本页标题，避免重复添加导航条。
- `openMetric()` 与 `revealMount()` 改为明确打开事件，并保留原业务定位逻辑。
- 文件树隐藏时继续接受终端目录更新，由现有 RemoteFileTree 在再次 activate 时完成必要定位，不能自动展开工作区文件面板。

## 6. 焦点与终端尺寸

### 6.1 工作区焦点

新增 `useWorkspaceFocus.ts`。它按会话维护 DOM refs 和一次性导航请求，区别以下原因：首次进入、用户工具导航、返回终端、响应式被动隐藏、模态关闭。焦点目标不写入快照，不保存 DOM 引用到业务状态。

| 情形                             | 焦点规则                                                                       |
| -------------------------------- | ------------------------------------------------------------------------------ |
| 鼠标点工具栏打开面板             | 焦点留在触发按钮，面板不抢焦点                                                 |
| 键盘打开工具                     | 按钮保持焦点，Tab 可进入面板；不自动跳过面板头部操作                           |
| 明确查看指标详情 / 在文件中显示  | 在目标准备好后定位详情或文件行；仅消费本次请求                                 |
| 用标题按钮收起                   | 焦点回该工具栏按钮                                                             |
| 调整窗口导致当前焦点所在面板隐藏 | 回到对应工具按钮；原焦点位于仍可见区域则不改变                                 |
| single 的“返回终端”              | 终端可见后聚焦当前终端模式，不强行切回实时模式                                 |
| 切换会话                         | 恢复该会话仍有效的焦点；无有效历史时，终端可见则聚焦终端，否则聚焦当前工具入口 |
| 有模态弹窗打开                   | 工作区不执行焦点重定位，由弹窗管理焦点                                         |

工具浮层 Esc 仅在焦点属于该浮层、没有内层菜单/弹窗且事件未被处理时关闭；终端内 Esc 正常发送。点击浮层外部不自动关闭，避免点击终端时发生隐含操作；用户可继续操作可见的终端区域。

将 `App.tsx` 中切会话后直接查询 `.xterm-helper-textarea` 的逻辑交由上述工作区规则处理。ResourcePanel 与 DockerPanel 的详情自动 focus 改为显式导航或内容页切换触发，被动 active 变化只恢复内容。

### 6.2 终端接口

为 TerminalController 增加 `focus(): void`，由 TerminalWorkspace 按当前模式路由：实时模式调用 LiveTerminalPane，历史模式恢复历史阅读区域/工具栏焦点。调用必须检查 active、interactive、正在移动和模态状态。

历史阅读区通过容器 ref 或现有历史组件的显式 focus 入口接入，不能查找隐藏的实时 textarea 代替历史焦点。实时终端的 `onData` 发送路径也增加 active/interactive/移动中检查，与剪贴板目标校验一致，避免隐藏期间的迟到输入继续发送。

TerminalPaneProps 增加 `focusPolicy?: "automatic" | "managed"`，默认 automatic，保持其他独立终端调用者的现有行为；SessionWorkspace 使用 managed。managed 仅禁止因可见性/ready 自动聚焦，仍需在失活时 blur 和失效剪贴板读取目标。显式 focus 仍通过上述保护检查。

收起工具不改变 request、initial、sessionId 或终端 key。single 隐藏终端时只暂停交互，输出与历史记录继续接收；再次显示后再 fit。

### 6.3 尺寸同步

LiveTerminalPane 现有 ResizeObserver 每次立即 fit 并发送 `terminal_resize`。将其与激活时的尺寸请求汇入一个 requestAnimationFrame 调度器：每帧至多 fit 一次，只有 cols/rows 实际变化才发送 IPC；首次连接按既有协议发送初始尺寸。

隐藏或零尺寸不提交 0 行列，不覆盖最后有效值；会话变化、移动与卸载清理待执行帧及尺寸去重记录。单次收放不逐帧动画 grid 宽度，避免终端字符连续换行；分隔条拖动按帧跟随并在结束时完成最终校准。

尺寸发送保存会话代次；同一会话有发送尚未完成时，只保留最新待发尺寸，结束后补发最终值。旧会话回包不能推进新会话队列。这样既合并每帧测量，也避免多次异步 IPC 完成顺序让远端停在旧尺寸。

## 7. 弹窗、状态保留与后台采集

### 7.1 弹窗不能附着在可隐藏面板下面

将文件下载、新建/上传等 FileOperationDialog、CopyTargetDialog、DeleteFilesDialog，以及 NetBirdOperationDialog 的实际 dialog 节点 portal 到 document.body，保持 React 所有者与业务状态原位。DockerDialogFrame 继续使用已有 portal。

新增轻量 `WorkspaceInteractionContext.tsx`，提供当前会话有效性与 `restoreFocus(preferred, side)`，不建立新的全局弹窗系统。恢复时依次选择仍可交互的原目标、所属工具按钮、当前工作区可见的后备目标；有其他模态弹窗时让出控制。独立窗口消费者没有该 context 时沿用自身回退策略。

文件原有下载焦点恢复需增加可见性检查；DockerDialogFrame 当前回退到全局首个选中 tab 的方式改为工作区所属工具入口。菜单与挂载详情等非模态临时浮层在所属面板隐藏时关闭，避免留下脱离来源的悬浮菜单。

布局变窄时已打开的业务 dialog 继续显示并保留草稿。面板按钮不能绕过原生模态层。晚到的预检响应如果工具已经隐藏，只保存待查看结果，在用户返回工具后展示；不能因旧请求突然弹出新确认框。

### 7.2 NetBird 的针对性调整

- 拆分目前依赖 `[active, sessionId, token]` 的生命周期：sessionId/token 变化继续使旧预检失效；仅面板 visible 变化不清空已呈现对话框和输入。
- `active` 仍限制新操作与前台兴趣；已提交任务、回执核对和请求去重由原 store 管理。
- peer/diagnostics 页面不再因 active=false 卸载。保持页选择和 token 等业务身份控制的挂载，隐藏时禁用新交互，保留 TCP 端口、CLI 路径输入与已返回结果。
- 预检请求维持请求代次校验；隐藏期间结果不自动呈现弹窗。过期结果由原 expiresAt 校验，不能因重新打开面板自动提交或重新生成操作。
- 工作区失活与布局隐藏单独表达：可传入 `workspaceActive` 区分会话前台归属，防止隐藏会话持有的 portal 抢到其他会话前方。

已呈现的确认框由稳定挂载的 dialog 组件保存本次 requestId 和输入；窗口 resize 造成面板不可见时不关闭。若工作区因会话导航失活，调用原生 close 暂停呈现，但不重新创建 React 组件或请求；返回所属会话时仅恢复此前已呈现的确认框，继续检查原有效期。尚未展示的晚到预检仍等待用户返回工具，不主动抢焦点。

只承诺普通收放保留这些内存状态；跨窗口继续迁移已有快照定义的视图数据，不把未提交确认、敏感草稿或原 DOM 焦点追加序列化。

### 7.3 可见性矩阵

| 所有者 / 消费者              | 控制依据                                                                  |
| ---------------------------- | ------------------------------------------------------------------------- |
| MonitorProvider              | 会话 active 与 document visibility，保持底部指标所需采集                  |
| DockerManagementProvider     | session 生命周期，继续任务事件与结果核对                                  |
| NetBirdProvider              | session 与原有跨窗口 suspended；面板收起不设 suspended                    |
| FilePanel / 挂载细节         | 会话 active × 实际 files 可见，隐藏保留模型缓存                           |
| ResourcePanel                | 会话 active × services 可见 × tab=monitor                                 |
| DockerPanel                  | 会话 active × services 可见 × tab=docker                                  |
| NetBirdPanel                 | 会话 active × services 可见 × tab=netbird，业务身份失效另行处理           |
| TerminalWorkspace            | 会话 active × terminalVisible 控制输入与展示，连接由 request/session 控制 |
| 文件任务、独立日志及容器终端 | 沿用现有任务/窗口生命周期                                                 |

不增加为工具栏图标专门轮询的状态请求。首版入口只展示工具选择与展开状态，详细业务异常由现有面板和摘要表达。

## 8. 快照兼容与跨窗口

WorkspaceSnapshot 新增 `tools?: WorkspaceToolState`；保留 `layout` 和 `resources` 字段。旧 `view`、`resourceWidth`、`fileRatio` 仅作为读取兼容字段，新增快照不再写旧 view。

迁移由单一 `normalizeWorkspacePresentation(initial)` 完成：

| 输入                          | 处理                                                                  |
| ----------------------------- | --------------------------------------------------------------------- |
| 合法 version=2 tools          | 归一化布尔、枚举与尺寸后采用                                          |
| 旧 view.pane=files            | 文件打开、服务关闭、lastSide=files、compactPane=files                 |
| 旧 view.pane=services         | 文件关闭、服务打开、lastSide=services、compactPane=services           |
| 旧 view.pane=terminal         | 两侧请求打开以接近旧宽屏体验，lastSide=services、compactPane=terminal |
| 缺少或不支持的 tools/view     | 使用新默认值，其他独立合法字段仍可恢复                                |
| 旧 resourceWidth              | 沿用当前兼容含义映射到 fileWidth，serviceWidth=320，再按范围校验      |
| NaN、非有限值、负值或无效枚举 | 回退对应默认值；不使用 `value                                         |     | default` 混淆非法值和有效值 |

旧快照没有源窗口宽度，不能准确还原旧 responsive 模式。采用上表确定性规则，不推测源窗口尺寸。新快照传的是意图，目标窗口用自身尺寸重新解析；不会把源窗口因空间不足的临时隐藏固化。

prepareMove 捕获最新 reducer 状态，继续使用现有冻结、接收、ready 和失败恢复协议。只有实际移动才运行暂停/恢复逻辑；普通开关工具不调用 prepareMove。纯界面数据不改变 Rust 所有权校验或传输超时。

## 9. 样式与 Apple Design 实现约束

- 延续现有主题 token 与系统字体，新增 `--workspace-rail-width`、`--workspace-separator-width` 等布局变量。图标 18–20px，按钮约 36–40px，缩放与布局指标同源。
- `:active` 立即反馈，`:focus-visible` 清晰可见，展开态使用柔和选中背景；不能依赖颜色或 tooltip 才知道状态。
- 停靠区域用背景层次与细边界；overlay 使用现有 popover 阴影与单层材质，正文维持可读背景。
- 首版仅在新内容进入时做约 100–140ms opacity 过渡；不对 grid、width、height 使用 `transition: all`，不开关期间锁按钮。
- 关闭立即停止交互；短淡入可取消并从当前值继续。保持同一 DOM，不用退出动画库卸载业务树。
- 本轮没有手势驱动的抽屉，不需要弹簧库。若后续加入侧向拖拽，再采用能承接当前速度的弹簧；不能用固定 keyframes 模拟可抓取手势。
- reduced-motion 使用直接切换或更短淡入；reduced-transparency 使用实色；高对比度加强边界与焦点。动效是可选呈现，所有功能不依赖动画完成回调。

## 10. 文件改动清单

| 文件                                                        | 计划职责                                               |
| ----------------------------------------------------------- | ------------------------------------------------------ |
| 新增 `workspaceTools.ts`                                    | 状态类型、归一化、reducer、旧快照兼容                  |
| 修改 `workspaceSizing.ts`                                   | 纯布局解析、尺寸约束与可见性                           |
| 新增 `useWorkspaceLayout.ts`                                | 宽度/字号测量、派生布局，清理 observer/帧回调          |
| 新增 `WorkspaceToolRail.tsx`                                | 两侧图标入口、tooltip、辅助功能状态                    |
| 新增 `useWorkspaceFocus.ts`                                 | 会话内焦点归属、显式导航与被动隐藏回退                 |
| 新增 `WorkspaceInteractionContext.tsx`                      | 弹窗归属与焦点恢复的轻量桥接                           |
| 修改 `WorkspaceLayout.tsx`                                  | 固定 slots、工具栏、停靠/浮层/single 与分隔条          |
| 修改 `SessionWorkspace.tsx`                                 | reducer 接线、快照、定位事件与统一 active 派生         |
| 修改 `ServicePanel.tsx`                                     | 移除旧 tablist，增加工具标题及收起入口                 |
| 修改 `FilePanel.tsx`、文件相关 dialog                       | 标题动作、portal、非模态菜单清理与焦点回退             |
| 修改 `ResourcePanel.tsx`、`DockerPanel.tsx`                 | 聚焦只随显式导航；配合工具框架与状态保留               |
| 修改 NetBird 面板、详情及操作 dialog                        | 解耦可见性与业务身份，保留输入、portal 与请求代次      |
| 修改 `TerminalPane.tsx`、`TerminalWorkspace.tsx`、`App.tsx` | managed 焦点接口、按帧 fit 与尺寸去重、移除重复 focus  |
| 修改 `DockerDialogFrame.tsx`                                | 焦点回退到所属工具，避免选择错误会话标签               |
| 修改工作区、资源、文件及主题 CSS                            | 移除旧导航样式，新增工具栏与布局形态，保留业务内部样式 |
| 更新相关 Vitest / Playwright                                | 新交互入口、布局行为、状态与连接保留验收               |

无需为了本次改造移动全部源码目录或引入全局状态库。`useWorkspaceMoves.ts` 和 `DetachedApp.tsx` 主要核对类型和恢复流程，若 JSON 传递已覆盖新增字段就不添加冗余逻辑。

## 11. 验证方案

### 纯逻辑

扩展 workspaceSizing 测试并新增 workspaceTools 测试，覆盖打开/切换/收起、隐藏但有打开意图时的激活、显式定位幂等、回到终端、无效快照、边界宽度、字号缩放、宽度偏好不被临时压缩覆盖。

检验布局性质：停靠模式总宽度不超过容器；终端达到约定预算；浮层尺寸不越界；single 只有一个内容区可见；视图随宽度变化可恢复，用户明确关闭不会重新打开。

### 浏览器交互

新增 `tests/workspace-tools-ui.spec.ts`，复用现有受控连接/资源 fixture；增加专用 Playwright 配置和脚本，覆盖 Chromium 与 WebKit。测试用户行为与后端调用结果，不只断言 CSS 类。

核心场景：

1. 左右入口同开、切换、关闭，连续点击最终状态正确；无空白列。
2. 会话切换和模拟移动后布局保留，sessionId 不变，不额外调用 open_connection/close_connection。
3. 目录/选中项/滚动、Docker 搜索、NetBird TCP 端口及 CLI 路径输入在收放后保留。
4. 隐藏时仍收到终端输出和任务完成事件；再次显示恢复正确结果；服务 Provider 未被重建。
5. 从状态栏打开指标，从 Docker/挂载打开文件，进入正确目标且不会 toggle 关闭。
6. 1440/1280/1024/860/650px、边界宽度与放大字体；缩窄后恢复、两侧都关闭、仅单侧打开分别验证。
7. 下载、Docker 创建、NetBird 操作 dialog 打开后缩窄窗口，输入和操作区仍可用；关闭后焦点回到正确工作区。
8. 隐藏工具不在 Tab 顺序；Esc 不被全局劫持；窗口 resize 不抢有效焦点；历史模式返回终端不自动切到实时模式。
9. 终端尺寸非零且变化去重；拖动分隔条无循环 ResizeObserver 错误，最终尺寸与容器一致。
10. 深浅色、减少动态效果、高对比度与减少透明度核对，截图用于视觉复核。

### 既有用例迁移

当前 resources、docker、netbird、connection、desktop 等用例有旧 `getByRole("tab", { name: "监控/Docker/NetBird" })` 和默认右侧展开假设，需一起更新。

新增共享测试帮助函数 `openWorkspaceTool(workspace, tool)`：作用域限定当前会话，仅在目标未展开时点击，并等待目标 region 可见。不能简单全文替换 tab 为 button，否则重复点击会收起工具，测试结果失真。会话标签本身仍然是 tab，不应被替换。

涉及的回归集包括 resources、mounts、docker、netbird、connection、文件树/传输、terminal-history、terminal-clipboard、window-chrome。根据实际受影响范围运行对应配置，最后执行 `npm test` 与 `npm run build`。仅改前端时不把全量 Rust 构建作为本轮完成的必要条件。

### 原生验证

Windows WebView2 重点检查分隔条、终端焦点、原生文件选择器返回、跨窗口移动与真实 SSH resize；macOS 另验窗口控制、字号和键盘行为。使用现有隔离测试环境，不连接用户服务器做试验。

若某平台实机不可用，在实施记录中列明未验证范围；浏览器 WebKit 结果不能当作 macOS 原生验收。

## 12. 实施顺序与交付

1. **状态与算法**：实现 reducer、迁移和布局解析，先验证可见性/尺寸不变量。
2. **工具栏与外壳**：接入固定 slots、右侧统一标题、显式定位与新默认状态，主窗口/独立窗口同步可用。
3. **生命周期与交互**：完成 managed 焦点、terminal fit、portal、NetBird 状态保留及移动恢复。
4. **视觉与响应式**：调整间距、材质、选中态、窄窗口和辅助功能，核对真实内容密度。
5. **完整验收**：迁移旧用例、运行相关回归与构建，补原生验证，记录完成项和实测范围。

实施交付包含代码、针对性测试、主窗口与窄窗口的深浅主题截图，以及 `docs/workspace-tool-windows-implementation.md` 实施记录。最终完成以核心开关、状态保留、导航、焦点、连接与任务连续性均可验证为准。

本方案已按用户确认实施；原生平台验证边界见实施记录。

# UI 优化代码实施方案

日期：2026-10-20。
状态：用户已确认并完成代码实施；验证结果与环境边界见 `ui-refinement-implementation.md`。本文保留实施前的设计契约。
功能依据：docs/ui-refinement-functional-plan.md。设计依据：本会话已经读取的 apple-design/SKILL.md。

## 1. 实施结论与约束

对监控状态管理、Docker 编辑到确认流程进行局部重构；文件菜单交接和外观偏好做局部优化。覆盖上一轮的 8 项问题，按 A → B → C → D 推进。

- 继续使用 React、TypeScript、原生 dialog、现有 CSS 变量与图标，不引入状态机、表单或动画依赖。
- 继续使用现有 MonitorStore、DockerManager 和后台任务系统，不新建全局状态系统。
- 数据库格式与现有已保存配置保持兼容；主题沿用 sodeck.theme 存储键。
- 主要新增后端能力为预览重新检查、草稿释放。确认有效期和草稿保留期分别管理。
- 每批独立验收，不进行无关的整仓格式化。当前工作区已有日志功能改动，实施时针对共享文件做最小增量合并。
- 文中的类型和接口是拟定契约；字段命名允许在实施时小幅调整，状态语义、数据归属、验收条件和批准范围保持一致。

## 2. 文件与职责总览

### 2.1 拟新增模块

| 文件 | 职责 |
| --- | --- |
| src/docker/createContainerDraft.ts | 创建草稿、动态行标识、字段校验、提交规格转换、dirty 比较 |
| src/docker/RestartPolicyFields.tsx | 创建与编辑共用的重启策略字段 |
| src/docker/DockerDialogFrame.tsx | Docker 弹窗的原生 dialog 外壳、固定头尾、滚动正文和焦点交接 |
| src/docker/DockerConfirmationView.tsx | 可嵌入同一 dialog 的确认内容，不自行创建第二个模态层 |
| src/docker/dockerActionFlow.ts | 操作流程状态、纯状态转换、提交错误分类与请求快照规则 |
| src-tauri/src/docker/drafts.rs | 有界草稿集合、预览版本替换、归属校验所需元数据和生命周期规则 |

为纯状态和规则模块增加对应单元测试文件。优先扩展现有界面测试套件，不为本轮另建测试框架。

### 2.2 主要修改模块

| 模块 | 修改职责 |
| --- | --- |
| resourceMonitoring.ts、resourcePresentation.ts | 订阅状态、连接事实、缓存代次与统一呈现 |
| ResourcePanel.tsx、ResourceStatus.tsx | 标题、指标、摘要和重试入口 |
| MonitorProvider.tsx、MonitorWindowApp.tsx | 保持主窗口与独立窗口的启动、停止契约 |
| DockerPanel.tsx、ClientTrafficCard.tsx、SessionConnectionDetails.tsx、MountOverview.tsx、useRemoteMounts.ts | 修正共享监控生命周期下的旧数据与不可用状态；DockerPanel 持有创建流程入口 |
| FilePanel.tsx | 菜单进入弹窗、局部键盘事件与下载焦点恢复 |
| CreateContainerDialog.tsx、ContainerSettingsDialog.tsx | 草稿、校验、统一策略字段和稳定弹窗结构 |
| useDockerActions.ts、ContainerManagement.tsx、DockerConfirmDialog.tsx、DockerTransferDialog.tsx | 统一操作流程、过期恢复、不可变提交请求 |
| ImageManagement.tsx、ComposeManagement.tsx、ResourceManagement.tsx | 接入统一确认流程，调整创建流程归属 |
| dockerApi.ts | 预览类型和两个新增 IPC 接口 |
| src-tauri/src/docker/model.rs、tasks.rs、commands.rs、mod.rs、create.rs | 草稿续查、生命周期、预览策略说明及提交验证 |
| src-tauri/src/lib.rs | 注册两个 IPC 命令，保留其他正在进行的功能注册 |
| theme.ts、ThemeSwitcher.tsx、SystemSettingsDialog.tsx | 外观偏好、实际颜色和两个入口 |
| docker/docker.css、resources.css、system-settings.css、相关现有顶栏/菜单样式 | 只补本批布局和状态所需样式 |

具体波及面以类型检查和引用搜索为准；ResourceMetricDetails 等转发组件只有在 props 改动需要时调整。

## 3. A1：监控状态局部重构

### 3.1 当前实现约束

MonitorStore.start() 在订阅前写 connected=true；订阅失败只写 error，默认指标仍是 collecting。主窗口订阅首先收到 snapshot，独立监控窗口还可能收到显式 connection 事件，而且事件可能早于 subscribe Promise 返回。

同一监控订阅承载指标、挂载、Docker 摘要和流量。Docker 管理操作另有 DockerManagementStore，不把两个订阅合并。

### 3.2 拟定状态类型

```ts
type MonitorConnection = "unknown" | "connected" | "disconnected";

type MonitorSubscription =
  | { phase: "idle" }
  | { phase: "subscribing" }
  | { phase: "active" }
  | { phase: "failed"; error: AppError };

interface MonitorState {
  connection: MonitorConnection;
  subscription: MonitorSubscription;
  // 保留 metrics、mounts、docker、traffic、历史等数据字段。
  // controlError 用于刷新/interest 更新等控制请求错误。
  controlError: string;
}
```

用 connection 替换该 store 当前含义混杂的 connected 布尔字段，并同步迁移它的直接消费者；不同时保存两个可独立修改的连接状态。后端 MonitorEvent 格式不变，单项 Metric.status 继续沿用。

connection 是监控所知的连接事实；终端实际连接状态继续由原连接模块管理。订阅失败不修改实际 SSH 状态，也不把 unknown 呈现为已断开。

### 3.3 生命周期与缓存

| 触发 | 状态处理 |
| --- | --- |
| start | 切到 subscribing；保留最后已知连接事实、数据和缓存所属代次 |
| 收到主窗口首次有效 snapshot | 没有显式连接事件时，可确立本次数据源可用；建立本次 stream generation |
| 收到显式 connection | 以其连接事实为准；断开后的旧数据不能让状态重新变为已连接 |
| subscribe 返回有效 ID | 记录订阅并切到 active，补发最新 interest，再加载相应历史 |
| subscribe 失败 | 切到 failed；保留最后已知连接事实；旧值仅作为上次采样 |
| 单项指标失败 | 只更新对应 Metric；不使整个订阅失败 |
| 主动 refresh / interest 更新失败 | 记录控制请求错误；明确订阅失效时进入可重建状态，不把订阅失效解释为 SSH 断开 |
| stop / 卸载 | 递增请求代次，清理订阅和在途控制请求；后续迟到结果无效 |

start 使用在途 Promise 去重，连续点击共享同一次订阅建立。失败订阅即使曾收到早期事件也不能继续声称采集正常。

延用 epoch、generation、revision：epoch 隔离请求和订阅，generation 隔离数据源，revision 排序同一数据族。重新订阅时保留缓存对应的 generation；同代次保留和合并历史，不同代次清空相关历史后加载新数据，避免序号重用导致混合曲线。

subscribe 结果迟到但该次 epoch 已结束时，立即释放返回的订阅 ID；不能只忽略返回值。成功重试只清理本次生命周期错误，不覆盖其他历史查询或单项指标错误。

### 3.4 展示函数

在 resourcePresentation.ts 增加统一选择器，代替页面各自拼接判断：

```ts
getMonitorPresentation(state: MonitorState): {
  label: string;
  detail?: string;
  canRetry: boolean;
  stale: boolean;
};

describeMetric(metric: Metric<unknown>, state: MonitorState): {
  label: string;
  stale: boolean;
};
```

呈现优先级：明确的连接断开 → 订阅失败 → 建立订阅 → 单项指标状态。首次等待数据显示“等待首次采样”，失效旧值显示“上次采样”。订阅 active 仅说明通道已注册，不能推导所有指标都有数据。

重试按钮在 failed 且存在可用上下文时可用；未知连接事实允许重试订阅；独立窗口已有有效观察订阅但原 SSH 断开时提示重新连接服务器，避免循环新建观察订阅。

挂载缓存仍通过 getMounts/subscribeMounts 提供稳定快照；在订阅失效时更新其 stale/error/loading 并通知挂载消费者。不要在 getMounts 每次调用中创建新对象。流量和 Docker 摘要使用相同的可用性规则，不中止已接受的独立后台任务。

### 3.5 验收与测试

扩展 resourceMonitoring.test.ts、resourcePresentation.test.ts：

- 订阅拒绝或返回空 ID，所有相关展示不再显示正在采集。
- 早期 snapshot 在 Promise 之前到达，以及随后失败的情况。
- 重复 start/retry、重试中 stop、旧订阅迟到释放。
- 同 generation 重订阅保留历史；不同 generation 不混合。
- 单项失败、控制请求失败与 SSH 断开各自独立。
- 挂载消费者收到失效通知，流量与 Docker 摘要不将旧数据视为最新。

扩展 resources-ui.spec.ts，覆盖主面板、摘要、独立监控窗口的失败与恢复；保留现有 mounts 用例验证共享缓存没有回归。

## 4. A2：文件菜单与下载弹窗

### 4.1 修改方式

在 FilePanel 内增加统一的 beginDownload(file, returnPath) 与 closeDownload()，所有下载入口复用。

beginDownload 先保存文件行路径和会话身份，再关闭菜单，最后设置下载对象；菜单不会跨越弹窗生命周期。downloadDialog 增加 aria-labelledby，对应标题 ID，初始焦点落在同名文件处理选项。

菜单方向键、Home/End 和 Esc 改为菜单元素自己的 onKeyDown，不继续使用 window 级键盘监听。外部点击、窗口失焦及布局变化仍可通过现有监听关闭菜单。处理事件前检查 defaultPrevented、isComposing 及事件是否来自菜单本身；其他前台模态层存在时不介入。

closeDownload 在 dialog 确实关闭后恢复焦点：原文件行 → 当前可见选中行 → 文件树可聚焦入口。会话变化、另一个前台模态窗口存在或开始原生保存位置选择时，不抢回焦点。

使用同一个关闭路径处理取消按钮、关闭按钮和原生 cancel 事件。进入原生保存位置选择时，等选择器返回后再决定是否需要恢复焦点。

### 4.2 测试

在 transfer-ui.spec.ts 的现有 IPC fixture 中加入键盘场景：右键与 Shift+F10、Enter 进入下载、上下键修改选项、Esc 退出、焦点返回与原行消失后的回退。断言事件行为和选项变化，不只断言菜单 DOM 消失。

现有 file-tree.spec.ts 是真实 Tauri/SSH/SFTP 套件，保留一条实际 WebView 场景作为补充，不把它误当作自启动的纯浏览器测试。

## 5. B：Docker 创建与编辑表单

### 5.1 草稿模型与校验

createContainerDraft.ts 持有编辑模型，和提交用的 CreateContainerSpec 分离。数值输入在编辑阶段用字符串保存，允许临时空值，不即时强制转换成 0。

```ts
interface CreateContainerDraft {
  name: string;
  start: boolean;
  ports: Array<{
    rowId: string;
    hostIp: string;
    hostPort: string;
    containerPort: string;
    protocol: "tcp" | "udp";
  }>;
  mounts: Array<{
    rowId: string;
    kind: "bind" | "volume";
    source: string;
    destination: string;
    readOnly: boolean;
  }>;
  environmentText: string;
  commandText: string;
  network: string;
  restartPolicy: string;
  maximumRetryCount: string;
  workingDirectory: string;
}

type FieldIssue = {
  field: string; // 稳定字段路径，动态项使用 rowId
  message: string;
  line?: number;
};

type DraftValidation =
  | { ok: true; specification: CreateContainerSpec }
  | { ok: false; issues: FieldIssue[] };
```

提供 createInitialDraft、validateCreateDraft、draftEquals 和共用的 validateRestartPolicy。field path 使用 ports.<rowId>.hostPort 等稳定标识；删除中间行后，错误与焦点不能移到另一行。dirty 比较忽略 rowId，比较所有有实际编辑意义的值；不粗暴 trim 环境变量值和命令参数。

前端规则对应 Rust create::Spec::validate 和 Action::RestartPolicy::validate，覆盖名称、端口范围和重复绑定、环境变量、挂载目标重复、重启次数、网络/路径和现有数量限制。按后端采用的字符或字节语义校验长度，避免中文值与 Rust 字节上限不一致。服务端保持最终验证责任。

环境变量按原始行号解析；忽略空白行但保留报错行号，按首个等号切分，保留值中后续等号及有意义空白。校验格式和重复键。命令文本沿用每行一个参数的既有语义，不增加 shell 拆词。

组件维护 touched 和 submitted 状态：失焦后展示字段错误，已报错字段随修改更新；首次提交检查全部字段。定位错误时先展开分组，再在布局完成后 scrollIntoView 和 focus；用 aria-describedby 关联错误，不用全屏 alert 逐条提示。

### 5.2 稳定的流程归属

当前 ImageManagement 以引擎/镜像选择为 key，且 scope 改变会关闭 creating。将创建入口改成 onCreate(imageId, imageName)，由 DockerPanel 较稳定的层持有创建流程描述和对话框，避免切换详情或引擎刷新时直接卸载草稿。

创建时记录 sessionId、engineToken、镜像 ID 等来源信息。引擎或来源变化后使预览无效，保留本次流程可保留的编辑输入，并明确提示重新检查；不能静默把原操作提交到另一个引擎。整个 SSH 工作区被关闭后的跨会话草稿恢复不在本轮范围内。

草稿仅驻留内存。环境变量和命令不写 localStorage，不增加草稿数据库表。

### 5.3 弹窗结构

DockerDialogFrame 仅处理通用外壳：原生 showModal、标题、关闭入口、正文和 footer 插槽、初始焦点、退出后的焦点恢复。业务决定是否允许退出，外壳不判断 dirty 或提交结果。

创建表单和预览使用同一个原生 dialog，在内部切换 Create 内容和 DockerConfirmationView；DockerConfirmDialog 作为独立确认的薄包装继续供其他页面使用。避免表单卸载后重建第二个 dialog 引发焦点来回跳动。

CSS 使用三行 grid/flex：header / minmax(0,1fr) body / footer；dialog 自身限制高度并隐藏外层溢出，正文 min-height:0、overflow:auto，头尾不参与滚动。给字段设置合理 scroll-margin，避免焦点被头尾遮挡。

退出规则：

- 编辑未修改：直接关闭。
- 编辑已修改：进入同一 dialog 内的放弃提示视图，默认聚焦“继续编辑”。
- 放弃提示中 Esc：返回编辑并恢复原焦点。
- 预览中“返回编辑”：保留全部输入，释放旧预览。
- 预览中取消整个流程：按 dirty 规则处理。
- 预检/刷新中退出：可以放弃当前 UI 请求，递增代次，迟到的预览必须释放。
- 提交中避免再次提交；结果不明时提供核对入口，不能伪装为已取消操作。

### 5.4 重启策略共用字段

RestartPolicyFields 在创建和 ContainerSettingsDialog 中复用策略选项、计数输入和说明。仅 on-failure 时展示重试次数；校验为 0–100000 整数，0 为不限次数。其他策略提交 maximumRetryCount=0，切回 on-failure 时保留本次已输入值。

Rust create.rs 的预览 consequences 增加最终重启策略和次数说明；不修改 Docker 实际命令执行逻辑。

### 5.5 测试

新增 createContainerDraft.test.ts，覆盖边界值、原始行号、重复项、空数值、稳定行标识和精确参数转换。扩展 docker-management-ui.spec.ts，覆盖 dirty/Esc、返回编辑、错误分组展开、窄窗口与短窗口、字体放大、策略切换以及引擎更新后的草稿保留与预览失效。

## 6. B/C 共用：统一 Docker 操作生命周期

### 6.1 状态由谁负责

扩展现有 useDockerActions，统一常规操作、创建、传输和单容器操作的预检/确认/提交状态；ContainerManagement 当前重复的 request/inFlight/pending 逻辑迁入该 hook。表单草稿和字段交互仍由表单拥有。

dockerActionFlow.ts 导出纯状态转换和辅助规则，便于验证。拟定阶段：

```ts
type DockerFlowState =
  | { phase: "idle"; error?: AppError }
  | { phase: "preparing" }
  | { phase: "review"; preview: DockerPreview; invalidReason?: AppError }
  | { phase: "refreshing"; preview: DockerPreview }
  | { phase: "submitting"; request: DockerOperationRequest; preview?: DockerPreview }
  | { phase: "awaitingResult"; request: DockerOperationRequest; error: AppError }
  | { phase: "accepted"; requestId: string; operationId: string };
```

过期由当前 preview.expiresAt 和服务端拒绝共同决定。定时器绑定当前预览 ID，刷新后清理旧定时器；确认按钮激活时再次检查时间，不能只依赖之前的渲染结果。动画不会锁住输入。

hook 提供 prepareOperation、prepareCreate、prepareTransfer、renewPreview、submitPrepared、submitDirect、retrySubmission 和 dismissPreview；沿用现有 actions.act 等调用时可提供过渡包装，所有调用方迁移后去掉重复分支。

pending 锁由 phase、后台任务和现有 operationBlocks 共同派生，不再由多个互相独立的 busy 布尔值决定。每个在途调用有 scope 和 request epoch，包含会话、引擎和目标；切换作用域后迟到结果不得覆盖新操作。

### 6.2 提交请求规则

- 预览和重新检查不会提交操作。
- 第一次确认提交时创建 requestId，并冻结 engineToken、targetId、action、previewId 的完整请求。
- IPC 回包丢失或无法确定是否接收时进入 awaitingResult，保持原请求对象。
- 从 DockerManagementStore 的操作记录按 requestId 匹配；找到记录即按已接受处理，不能因为刷新或详情重绘再创建请求。
- 用户重试发送时使用原 requestId 和完全相同的请求内容，依靠后端既有去重返回同一 operationId。
- 明确的提交前拒绝允许回到编辑或重新检查。提交已接受后的远端校验失败属于后台任务结果，不能当作提交前拒绝自动再发一次操作。
- awaitingResult 状态禁止重新生成预览并换 requestId 提交；核对结果后才能产生下一次用户意图。
- 操作进入 accepted 后释放表单流程；后续执行进度和失败由既有任务 UI 显示。

同时核对 hook 使用方的 onAccepted 收尾：Promise 回包和订阅事件都到达时，同一 requestId 只触发一次关闭/成功处理。

## 7. C：预览过期恢复的后端契约

### 7.1 为什么需要调整草稿生命周期

当前 DockerManager 将 Draft 按 preview.id 存储，后台循环在 expires_at 到期后移除；提交接受后也移除。传输预检会调用本机文件选择器，所以过期后仅调用原 prepareTransfer 会要求重新选择文件。

本次把“供用户确认的预览版本”和“重新检查所需的原始输入”区分开。暂定确认有效期保持现有 60 秒，草稿在内存中最多保留 15 分钟；15 分钟是本方案建议值，不是 Apple 规范要求，也不扩大确认有效期。

### 7.2 数据结构与兼容性

DockerPreview 新增稳定的 draftToken。每次成功重新检查产生新的 preview.id；draftToken 在该次逻辑流程内不变。

```ts
interface DockerPreview {
  id: string;           // 当前确认版本
  draftToken: string;   // 该次流程的内存草稿句柄
  engine: DockerEngineBinding;
  targetId: string;
  targetName: string;
  action: DockerActionPayload;
  consequences: string[];
  expiresAt: number;
}
```

内部 DraftRegistry 按 draftToken 保存一个当前预览及来源：

```text
Draft
  original session / identity / window owner generation
  source = Operation(target, action) | Create(spec) | Transfer(spec)
  current preview + fingerprint
  retainUntil
```

提取到 drafts.rs，避免把生命周期规则继续散布到 tasks.rs。现有最多 100 份草稿的限制按逻辑草稿计算，刷新替换当前版本，不占新名额。

OperationRequest 仍使用 previewId，提交时只接受与当前版本相符的预览；可通过有界集合查找或内部索引定位对应草稿。Create/Transfer action 内现有 draftId 的生成继续和相应预览保持一致；新 draftToken 是续查句柄，不混用这两个概念。

这次只扩展瞬时 IPC 类型，不修改已保存的服务器配置或任务数据库格式；所有浏览器 fixture 同步补充 draftToken。

### 7.3 新增两个命令

```ts
dockerApi.renewPreview(
  sessionId: string,
  draftToken: string,
  expectedPreviewId: string,
): Promise<DockerPreview>;

dockerApi.releasePreview(
  sessionId: string,
  draftToken: string,
): Promise<void>;
```

对应 Tauri 命令：renew_docker_preview、release_docker_preview。

renew 处理顺序：

1. 验证当前窗口、owner generation、原会话/服务器身份、引擎归属与保留期限。
2. 若 expectedPreviewId 已被同一次刷新替换，返回目前版本，用于刷新回包丢失后的幂等重试；不重复创建草稿。
3. 在锁内复制来源和版本快照，释放锁后执行只读预检；创建使用原 Spec，传输使用原本机选择结果，常规操作使用原 target/action。
4. 重新取得配置/草稿锁并按原锁序再次校验。草稿已释放、提交或归属变化时不复活；版本已被另一刷新更新时返回该版本。
5. 原子替换最新 preview 和 fingerprint，生成新的 60 秒确认期限，保持原 retainUntil 上限。

耗时 SSH 和文件检查不在全局草稿锁内执行；首次预检与重新检查复用相同的预检构造函数，避免调用“插入新草稿”的入口而绕过数量约束。

release 对已不存在的句柄幂等；存在时必须匹配归属，不能删除其他窗口或会话草稿。提交已接受后清理草稿，但不撤销任务。

### 7.4 回收与过期语义

- 后台清理改为按 retainUntil 清理输入；submit 仍严格验证当前 preview.expiresAt。
- 新建前、续查前也清理已到保留期限的草稿，不仅依赖后台轮询。
- 用户取消、返回编辑、原窗口关闭时释放相应草稿；迟到的初次预检结果也释放。
- 同一次成功刷新只保留一份输入及当前预览，旧 preview.id 不能被提交。
- 超过 15 分钟：创建表单用仍在界面的输入重新预检；传输提示“原文件选择已失效，请重新选择”，不能猜测文件或路径。
- 刷新前后 consequences、目标或引擎展示信息有变化时提示“操作信息已更新，请重新确认”；无需重置表单。

将明确的预览到期与草稿不可恢复作为结构化错误，例如 previewExpired、previewUnavailable。引擎或归属变化保持冲突/权限错误，由前端展示相应原因；不把所有 conflict 都归类为普通到期。新增错误需同步提交前拒绝分类。

### 7.5 后端验证重点

- 到期预览不能提交，保留期内可以续查；保留期后不能续查。
- 刷新生成新 preview.id，旧版本不能提交；连续刷新不增加草稿数量。
- 相同 expectedPreviewId 的并发或回包丢失重试只返回同一个最新版本。
- release、submit 与 renew 竞争时不复活草稿、不重复执行。
- 原 owner、窗口代次、会话或引擎变化，续查被拒绝。
- 传输续查复用原选择，不再次调用文件选择器；原文件变化/丢失进入明确错误。
- accepted 请求重发仍返回原 operationId，即使草稿已经被移除。

优先将时钟、TTL 与版本替换规则写成可注入时间参数的纯逻辑测试；真实 SSH 预检部分延用 Docker fixture。

## 8. D：外观偏好

### 8.1 类型与通知

```ts
type Theme = "light" | "dark";
type ThemePreference = "system" | Theme;

getTheme(): Theme;
getThemePreference(): ThemePreference;
setThemePreference(next: ThemePreference): void;
subscribeTheme(listener: () => void): () => void;
```

沿用 sodeck.theme。历史 light/dark 直接保留；缺失或无效值解释为 system；选择 system 时移除该键，兼容原来“未设置即跟随系统”的行为。

applyTheme 分别比较 preference 和 resolved theme：任一改变都通知订阅者，只有实际颜色变化才更新 data-theme。getThemePreference 和 getTheme 都返回稳定原始值，避免 useSyncExternalStore 快照重复创建。

系统颜色变化仅影响 system 偏好的实际颜色；storage 事件读取最新值，保持现有跨窗口同步和 HMR 清理。存储写入失败时当前窗口仍可切换，不能声称已完成其他窗口的持久同步。

### 8.2 两个入口

ThemeSwitcher 保留组件名，内部改为紧凑外观菜单，三个项使用 menuitemradio/aria-checked，初始焦点在当前项，支持方向键、Home/End、Enter/Space 和 Esc，位置锚定触发按钮。

SystemSettingsDialog 新增外观区，使用有 label 的原生 select。两处共用选项常量和同一 preference store；避免设置页弹出第二个自定义菜单。

复用现有菜单样式与定位规则，不改造全部菜单组件。菜单退出恢复到按钮；选择主题不重建窗口或丢失其他表单内容。首屏仍在 React 渲染前应用实际颜色。

### 8.3 测试

新增 theme.test.ts，模拟 matchMedia、localStorage 和 storage 事件，验证：旧数据、默认 system、同色不同偏好也通知、系统变化、写入失败及监听清理。

更新 window-chrome-ui.spec.ts、system-settings-ui.spec.ts 中原双按钮的定位，覆盖主窗口/编辑窗口/后续新窗口同步，保持现有“切主题不丢未保存编辑”的断言。

## 9. apple-design 的代码验收标准

以下是本项目执行规则，不将苹果触摸设备的尺寸机械套用到桌面工具。

- 保留现有按下即时反馈；异步调用开始时立即呈现相应状态，不加人为等待。
- 原生 dialog 管理模态焦点，键盘事件只影响当前层；初始焦点、返回焦点与 Esc 行为明确。
- 普通字段错误在上下文中显示；只有确实可能丢失输入时使用放弃提示。
- 关闭、返回编辑和后台任务之间的语义清楚；可取消的预检允许退出，已经接受的操作不伪装成已撤销。
- 使用现有字号、颜色、圆角和间距变量；头尾固定服务于长表单可达性，不增加无关装饰。
- 状态同时有文字和语义属性，不能只依赖颜色。错误播报按一次有意义的变化触发，避免每次按键重复读出。
- 本轮没有新增拖拽或大幅空间移动需求；若添加小范围过渡，保持可打断，支持减少动态效果、减少透明度与增强对比度。
- 验证短窗口、窄窗口、字体放大、浅深色、键盘和目标 WebView；已有可用布局不因本轮重构被重做。

## 10. 实施、验证与提交顺序

| 批次 | 代码内容 | 通过条件 |
| --- | --- | --- |
| A | 监控状态及消费者；文件菜单交接 | 状态转换与焦点回归通过 |
| B | 创建草稿、字段规则、稳定弹窗、策略字段；统一操作流程基础 | 创建/返回/退出/校验完整流程通过，既有提交行为不回归 |
| C | DraftRegistry、两个 IPC、所有确认调用方接入续查 | TTL、归属、并发和去重测试通过；原地恢复可用 |
| D | 偏好类型、外观菜单、设置入口、多窗口同步 | 同色切偏好、系统响应和多窗口用例通过 |

B/C 的接口先按本文对齐，B 可以保留暂未接通的续查入口不展示，C 一次性完成全部相关调用方，避免出现假可用按钮。

实施后的常规验证：

```text
npm test
npm run build
npm run test:resources
npm run test:transfers
npm run test:docker
npm run test:connection
npm run test:settings
cargo test --manifest-path src-tauri/Cargo.toml --lib docker::
```

按批次先跑相关测试；全部代码完成后执行一次总体单元测试和构建。若无新修改或新风险，不重复扩大测试。test:settings 的全局准备会调用 Rust browser-policy-fixture；相关工具链需可用。

真实桌面验证：使用隔离的 QA 数据与已有 SSH/Docker fixture，覆盖文件菜单方向键、原生文件选择后的焦点、传输续查和多窗口主题。test:tree 需要已启动的真实 Tauri 与 SSH fixture；实施时按现有验证文档准备，不连接用户实际服务器做写入测试。当前主机可验证 Windows WebView；macOS 实机如不可用需明确报告，浏览器 WebKit 结果不能等同于 macOS 实机结果。

每批记录修改文件、实际执行的验证、结果及未覆盖环境。只有计划中的 8 项行为全部完成并有对应验证证据后，才报告本轮优化完成。

## 11. 用户最终确认的内容

建议批准本文整体范围后，按 A → B → C → D 开始实现。确认范围包括：

1. 监控连接字段改为三态并迁移直接消费者。
2. Docker 创建与确认流程的小范围组件拆分和状态统一。
3. 两个新增 IPC；确认 60 秒、内存草稿上限 15 分钟、最多 100 份逻辑草稿。
4. 顶栏外观菜单与设置页外观入口。
5. 对应前后端测试与目标桌面验证。

用户已确认实施。实际改动与验证结果见 `ui-refinement-implementation.md`。

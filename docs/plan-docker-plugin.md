# Docker 插件第一版与后续设计

整理日期：2026-11-10。第一版采用下列精简范围，实际实现与验收见[实施记录](implementation-docker-plugin.md)。第 1–18 节保留后续完整功能设计；其中流式执行、文本导出和长任务接口仍是提案，不能视为当前可用 API。

核对基线为核心 0.20.0、提交 `601b307`。第一版不依赖流式执行或文本导出，不增加核心运行能力，**暂不涉及容器终端**。核心与插件职责遵循 [开发约定](../AGENTS.md)、[架构文档](architecture.md)和[功能设计方法](plan-feature-design.md)，已有能力以[插件 API](plugin-api.md)及当前源码为准。

## 已接受的第一版范围（0.1.0）

目标是先完成日常容器查看与单项维护，验证独立 Docker 插件能自然接入现有工作台。客户端沿用五个原生目标；远端先支持 Linux Docker Engine，通过远端用户的 Docker CLI 和显式 Unix socket 访问。

| 功能 | 第一版可验收行为 |
| --- | --- |
| 容器浏览器 | 按需打开侧栏，显示 Engine 身份、版本和容器列表；支持搜索、状态/健康筛选与 Compose 项目排序 |
| 容器详情 | 主区域按资源打开，显示配置、状态与健康、环境变量、端口、挂载、网络、标签；环境变量值默认隐藏 |
| 最近日志 | 主区域显示有界快照，选择最近 50/200/500 行，手动刷新、搜索整个缓存、分页与复制当前页 |
| 单容器操作 | 明确确认后启动、停止、重启、暂停、恢复；只删除已停止容器，不传 force 或删卷参数 |
| 资源快照 | 手动读取 CPU、内存、网络、块 I/O 和进程数，不自动轮询趋势 |
| 生命周期 | 复用 UI v2 视图迁移、关闭和 busy；重复意图只执行一次，中断保留结果未确认，不自动重发 |

只复用 `session.exec.v1`、UI v2、`ui.openView.v1`、设置、统一日志和重要消息。Docker 不直接依赖 SSH 库、终端插件或文件插件。每条命令沿用核心 30 秒上限，停止/重启使用 10 秒宽限。

暂缓容器终端、实时日志跟随、日志时间范围、文本文件导出、镜像/网络/卷管理、镜像拉取、批量操作、长任务中心和跨插件跳转。它们不是 0.1.0 的验收项。后续出现第二个真实用例时，再决定是否扩展核心通用能力。

## 后续完整设计（尚未实施）

阅读顺序：第 1–5 节是范围、界面和代码结构，第 6–14 节逐项定义功能与代码方案，第 15–16 节是核心接口和完整生命周期，第 17–18 节是验收与实施顺序。每项功能都列出用户行为、命令或数据字段、代码入口、失败处理和验收条件。

## 1. 需求理解与范围

用户希望 Docker 作为业务插件接入 SoDeck：复用服务器连接，在工作台查看和管理 Docker 对象，并能把详情、日志、任务放入不同区域。插件增长不应使核心逐渐包含 Docker 规则，也不应让业务任务依赖某个窗口或前端页面的挂载次数。

本方案建议的后续完整版本完整范围如下。客户端平台、远端 Engine 范围、配额和默认值是设计选择，仍可在评审时调整。

| 功能 | 完成后的用户行为 |
| --- | --- |
| Engine 连接与概览 | 知道访问的是哪个 Engine，看到版本、资源数量和磁盘使用估计；访问失败有明确原因 |
| 容器列表 | 查看全部容器，按状态、健康状态、名称、镜像和标签筛选，按 Compose 项目分组 |
| 容器详情 | 查看配置、端口、挂载、网络、健康检查、退出与重启信息，并定位关联资源 |
| 容器操作 | 启动、停止、重启、暂停、恢复和删除；单项与批量操作都有逐对象结果 |
| 容器日志 | 查看有界历史、指定时间范围、持续跟随、搜索、复制和导出已缓存日志 |
| 容器指标 | 查看 CPU、内存、网络和块 I/O，以及短时间趋势 |
| 镜像 | 查看本地镜像、标签、大小和容器引用；拉取镜像，删除符合条件的镜像 |
| 网络 | 查看网络配置和连接容器，删除没有使用的自定义网络 |
| 卷 | 查看卷及所有容器引用，明确选择后删除未使用的卷 |
| Compose 关系 | 识别已有项目与服务，汇总状态并定位关联容器；不修改部署文件 |
| 任务与恢复 | 长操作有进度、取消和实际结果，断线后保留“结果未知”，不自动重放 |
| 工作台与设置 | 一个插件声明多个视图，支持分屏、拖拽和移窗，复用核心设置、日志与重要消息 |

后续完整版本不包含：容器终端、创建容器表单、编辑容器配置、镜像构建与推送、Registry 账号管理、镜像标签管理、Compose 文件编辑及 up/down 部署、Swarm、Kubernetes、自动部署、定时清理、`system prune`。不自动安装 Docker、启动 daemon、修改用户组或加入 `sudo`。

## 2. 现状与职责边界

### 2.1 已有能力可以直接复用

| 当前落点 | 已有能力 | Docker 的接法 |
| --- | --- | --- |
| `src-tauri/src/component_ui/host.rs`、工作台模块 | UI v2 逻辑视图、Surface 输入权、分屏与移窗 | 声明多个 view type；迁移不销毁逻辑视图 |
| `src-tauri/src/session_exec.rs` | 认证会话上的非 PTY 短命令、去重、轮询、取消和释放 | 执行探测、列表、详情和单次指标查询 |
| `src-tauri/src/connections/route.rs` | 连接与通道适配 | Docker 不接触 SSH 库、认证资料和传输对象 |
| `src-tauri/src/resource_owner.rs`、`resource_tasks.rs` | 插件实例、view、完整 Context 的资源归属和创建准入 | 任务在发送远程命令前登记；关闭后拒绝迟到创建 |
| `src-tauri/src/lifecycle/close.rs` | 统一关闭、busy/dirty 汇总和收尾 | 长操作页报告 busy；日志页关闭取消跟随 |
| `plugins/system-status/` | 独立 Rust 插件、后台调度、分块快照、设置和 UI v2 实例 | 复用工程结构与接入约定，不复用系统状态业务采集器 |
| `scripts/package-plugin.mjs`、`.github/workflows/plugin.yml` | 独立插件的五平台原生打包与 Release | Docker 添加自身发行配置，不让核心包依赖 Docker 构建 |

当前短命令有明确上限：1–30 秒、输出默认合计 256 KiB/最大 1 MiB；poll 每路最多 8 KiB；全局 8 个在途任务、每插件进程 4 个。普通插件 RPC 信封最多 64 KiB，UI 数据通常最多 24 KiB，SDK 等待控制调用约 4 秒。不能把拉取过程或大日志一次塞进 `ui.event`。

### 2.2 两个已经出现真实用例的核心缺口

1. 日志跟随与镜像拉取需要超过 30 秒的通用命令任务，且累计输出不能一直保存在内存中。新增有界流式执行契约，保留短命令 v1。
2. 日志快照需要本机“另存为”。当前 `files.exportDraft` 验证并读取文件插件的 FileResource，不能直接导出 Docker 缓存。提取小范围的文本导出服务，保留文件源校验。

不增加 Docker 专用核心模块，不提前加入 Docker HTTP API、远程代理或容器资源管理器。现有终端流 `src-tauri/src/streams.rs` 带终端重放与显示检查点语义，不直接作为 Docker 日志协议。

### 2.3 代码归属

| 核心负责 | Docker 插件负责 | 插件前端负责 |
| --- | --- | --- |
| 连接、凭据、通道、版本化执行契约、配额、进程与资源清理 | Engine 探测、命令参数、JSON 解析、状态判断、资源关系、操作与恢复语义 | 表格、过滤输入、确认内容、日志绘制、选择与滚动 |
| 工作台位置、逻辑 view 与 Surface、关闭和迁移准入 | view 的业务状态、任务队列、缓存与展示修订 | flush 已接纳输入，等待 surfaceReady 后启用操作 |
| 系统保存对话框、本地文件发布、通用日志和消息归因 | 导出源快照、诊断字段与重要消息内容 | 发起导出、显示结果，调用已有剪贴板与文本菜单 |

当前跨插件通信机制足够承载后续协作。本版没有跨插件依赖，也不改变默认新建行为。将来加入终端时另行设计业务事件与终端创建参数，不在本方案中预先增加核心路由。

## 3. 连接方式、兼容范围与身份

### 3.1 推荐后续完整版本通过远端 Docker CLI 访问 Engine

SoDeck 客户端不需要安装 Docker。插件通过核心当前认证会话执行远端已安装的 `docker` CLI，不建立第二条 SSH 连接、不保存服务器凭据、不引入 Docker SDK 或 SSH 生产依赖。

后续完整版本远端范围建议为 Linux Docker Engine、Linux POSIX shell，包含 rootful 和明确配置 socket 的 rootless Engine。拟支持远端 CLI/Engine 24.0 及以上；最低版本与当前稳定版本必须用真实 fixture 验证，不能仅依靠版本号宣布兼容。Docker Desktop、Windows Engine、Podman 和远端 TCP/TLS Docker endpoint 暂不在验收承诺中。

这与五个客户端正式支持目标是两回事：Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 客户端都应能管理上述 Linux Engine。

### 3.2 Endpoint 必须明确

- 默认为 `unix:///var/run/docker.sock`，在界面展示实际地址。
- Rootless 用户在连接设置中明确填写远端绝对 Unix socket 路径，例如 `unix:///run/user/1000/docker.sock`；不凭开发机器环境猜测 UID。
- 不静默采用远端 `DOCKER_HOST` 或 `DOCKER_CONTEXT`，避免访问另一台机器的 Engine。
- 连接设置按 serverId 保存，来自核心提供的插件 dataDirectory；标准设置项走 settings API。保存只使用标准序列化和文件接口，不硬编码本机目录。
- 更换 endpoint 时若仍有日志或操作，先让用户结束相关任务；更换成功后清空关联缓存并重新探测，不把旧操作迁到新 Engine。

命令统一通过 `commands.rs` 生成，示意：

~~~sh
sh -c 'unset DOCKER_HOST DOCKER_CONTEXT DOCKER_TLS_VERIFY DOCKER_CERT_PATH DOCKER_API_VERSION; exec docker --host "$1" version --format "{{json .}}"' sodeck unix:///var/run/docker.sock
~~~

实际代码把 argv 中每个字符串按 POSIX 单引号规则转义，再生成外层 shell 命令；不插入未转义的字符串。远端 registry 拉取可复用该远端用户已有 CLI 配置与凭据助手，但后续完整版本不提供 login、不保存或显示认证内容。明确 endpoint 仍覆盖配置中的默认 context。

### 3.3 Engine 与资源身份

~~~rust
struct EngineKey {
    context: Context,       // workspaceId/serverId/connectionId/generation
    endpoint: String,
    engine_id: String,
}

enum DockerObject {
    Container { id: String },     // 完整 64 位十六进制 ID
    Image { id: String },         // 完整 sha256:...
    Network { id: String },       // 完整 ID
    Volume { name: String, fingerprint: VolumeFingerprint },
}
~~~

资源 key 为有版本的 JSON 字符串，例如 `{"v":1,"engineId":"...","kind":"container","id":"..."}`；核心按不透明字符串处理，插件按严格类型解释。Context 由核心绑定，不放入页面可自行改写的业务参数。

每次变更操作在派发前重新确认 Engine ID 与对象。完整 ID 用于容器、镜像和网络操作，显示名称只用于阅读。卷没有不可变 ID，使用 name 加 CreatedAt/driver/labels 等指纹检查；不能承诺原子阻止“检查后被其他工具删除并同名重建”的竞态。此限制在卷删除确认中说明，失败后不自动重试。

## 4. 视图与界面组织

### 4.1 一个插件声明多个逻辑视图

| viewType | 位置与实例策略 | 内容和生命周期 |
| --- | --- | --- |
| `browser` | sidebar / perWorkspace，默认右侧鲸鱼图标固定入口，openOnWorkspace=none | Engine 概览、容器/镜像/网络/卷分类及筛选；点击入口才创建并探测 |
| `container` | editor / perResource | 单容器详情和指标；相同容器再次打开定位原页 |
| `logs` | editor / perResource | 单容器日志；与详情是不同 view type，可并排或移窗 |
| `resource` | editor / perResource | 镜像、网络、卷或 Compose 项目详情，kind 在 resourceKey 中 |
| `operations` | editor / perWorkspace | 当前工作区全部变更操作与拉取任务；一个稳定的逻辑任务归属 |

声明示例，完整清单还需补齐其它视图与设置：

~~~json
{
  "id": "sodeck.docker",
  "name": "Docker",
  "apiVersion": "1",
  "binary": "sodeck-docker",
  "tagPrefix": "plugin-docker-v",
  "ui": {
    "apiVersion": "2",
    "entry": "ui/index.html",
    "views": [
      {"id":"browser","title":"Docker","icon":"docker","container":"sidebar","instancePolicy":"perWorkspace","defaultSide":"right","defaultPinned":true,"openOnWorkspace":"none"},
      {"id":"container","title":"容器","container":"editor","instancePolicy":"perResource"},
      {"id":"logs","title":"日志","container":"editor","instancePolicy":"perResource"},
      {"id":"resource","title":"Docker 资源","container":"editor","instancePolicy":"perResource"},
      {"id":"operations","title":"Docker 任务","container":"editor","instancePolicy":"perWorkspace"}
    ]
  }
}
~~~

不需要修改核心布局。工具可拖入中央，详情与日志可分屏，页面、二级标签组和工作台可按已有 UI v2 行为移窗。各页面始终属于同一服务器 Context。

### 4.2 页面安排

- browser 顶部显示 Engine 名称、endpoint 和状态；分类下使用紧凑列表，默认容器分类，不增加第二套工作台标签系统。
- 窄侧栏使用两行列表，第一行是名称和状态，第二行是镜像及健康/Compose 摘要；选中后展示详细字段。browser 拖入中央或扩宽后使用表格，建议列为名称、状态、健康、镜像、端口、Compose 服务、运行时间。CPU/内存留在详情，不默认采样每个容器。
- 详情上方是对象名、完整 ID 的复制入口和状态；下方分配置、端口、挂载、网络、健康/退出信息与指标区域。
- 日志页顶部提供时间范围、tail 数量、跟随、暂停采集、自动滚动、搜索、复制和导出。暂停采集与暂停滚动分别表达。
- 任务页显示对象、动作、阶段、开始/耗时、进度和结果；批量任务展开为逐对象记录。
- 空列表、过滤无结果、首次加载、旧快照、不可访问、对象已删除分别渲染，不用同一空白表格表达。

界面沿用现有官方插件的操作工具风格。共享颜色、尺寸和滚动条来自打包器注入的 `src/ui.css`、宿主主题 tokens；文字菜单和剪贴板沿用已有打包资产及 `plugins/shared/clipboard.js`。不为 Docker 单独新增配色或导航体系。

| 场景 | 交互与代码约束 |
| --- | --- |
| 找容器 | 默认筛选“运行中”，提供“全部”“异常”；单击选中，双击或 Enter 打开详情，行内入口打开独立日志页 |
| 键盘操作 | `browser.js` 维护稳定对象 key 和 roving tabindex；上下键移动、Space 切换选择，显式按钮提交批量动作；Esc 关闭弹层并恢复触发按钮焦点 |
| 窄区域 | `style.css` 根据插件自身容器宽度切换列表/表格，工具栏换行或收起次要动作；400 px 日志页仍能搜索、暂停与跟随，不依赖整窗宽度 |
| 大字段 | 名称可折行，ID/镜像/路径提供复制；表格截断处可展开完整值，不能用截断值生成资源 key 或操作参数 |
| 刷新与选中 | 新快照按对象 key 保持焦点和选择；对象消失时移除选择并提示，筛选隐藏的已选对象显示数量，批量确认列出全部确切对象 |
| 危险操作 | 明确确认对象和数据影响，提交中禁止再次确认；不提供无提示 Delete 快捷键，失败保留对象和可阅读诊断 |
| 日志与指标 | 自动滚动只跟随底部；向上阅读保留位置并显示新日志数量。趋势旁保留文本数值、单位与时间，颜色同时有状态文字 |
| 辅助技术 | 使用原生按钮、输入框与表格语义，图标按钮有名称，焦点可见；低频状态变化使用 polite live region，实时日志不逐行播报 |

后续完整版本使用分页/分块追加与有界日志行窗口，不为此新增表格或图表依赖。浅色、深色、多巴胺主题及 reduced-motion 都纳入实施时的视觉验收。

### 4.3 UI v2 接入规则

- `initialize` 检查能力并返回 `apiVersion:"1"`、`uiApiVersion:"2"`、`settingsApiVersion:"1"`；完整版本同时确认第 15.2 节的文本源协议。
- 完整发行包要求 `ui.workbench.v2`、`ui.openView.v1`、`settings.v1`、`session.exec.v1` 及本文新增的 stream/exportText 能力；缺少时明确说明需升级核心。阶段 1 的只读开发版本可以仅要求其实际使用的既有能力，不能当作完整功能包发行。
- `ui.createView` 一次建立逻辑状态；`ui.getView` 只读状态，不创建执行任务。
- `ui.event` 校验 action 与参数，把需要执行的工作放入后台调度，迅速返回 operationId/revision。
- 页面通过 `sodeck-ui-v2`，报告 `prepared` 且 `flush:true`；只读候选不能提交操作，等待 `surfaceReady` 后启用输入。
- flush 同步筛选、表单、选择和滚动定位，不重新启动日志或命令。`freeze:true` 保持冻结至卸载或 transferCancelled；普通 flush 不永久冻结。
- 跨窗只恢复展示状态和逻辑缓存修订；日志 offset、核心 taskId、操作序号由插件进程持有，不由 iframe 重建。
- 候选 Surface 只绘制 `ui.getView` 提供的有界初始快照，不发 `ui.event` 分页/轮询；提交交接并取得输入权后再恢复读取。这沿用当前宿主对候选事件的限制，不新增候选只读 RPC。
- 获取快照采用 `knownRevision`，列表页每次最多 50 行，序列化结果硬限 24 KiB；单行过大则减少页大小，字段过长明确截断展示。原始详情按分区/分页读取。

## 5. 代码结构、状态与调度

### 5.1 插件目录

~~~text
plugins/docker/
  Cargo.toml, Cargo.lock, release.json, CHANGELOG.md, README.md
  src/
    main.rs          # initialize、UI/设置方法分派
    model.rs         # Engine、资源、快照、业务错误与操作状态
    runtime.rs       # 视图注册、Context 缓存、任务队列、结果提交
    commands.rs      # typed args、验证、POSIX 转义、固定模板
    engine.rs        # version/info/df、endpoint 与兼容探测
    containers.rs    # 列表、详情、Compose 标签和容器操作规则
    logs.rs          # 输出解码、行缓存、跟随、查询与导出快照
    stats.rs         # 单次 stats 解析、采样和短趋势
    images.rs        # 镜像列表、引用、拉取进度和删除规则
    resources.rs     # 网络/卷/Compose 聚合与引用检查
    operations.rs    # 变更任务、去重、持久化意图、取消与未知结果
  ui/
    index.html, bridge.js, view.js, style.css
    browser.js, details.js, logs.js, operations.js
~~~

这是职责清单；小模块可以合并，不按方法数量拆文件，不创建通用插件业务框架。依赖先沿用 `sodeck-plugin-client`、已锁定 `serde`/`serde_json` 和 Rust 标准库；本方案不增加生产依赖。

### 5.2 状态管理

~~~rust
struct App {
    views: HashMap<ViewId, DockerView>,
    engines: HashMap<EngineKey, EngineCache>,
    operations: HashMap<OperationId, Operation>,
    queue: VecDeque<Job>,
}

struct EngineCache {
    inventory_revision: u64,
    observed_at: Option<SystemTime>,
    coverage: Coverage,       // complete / partial / failed
    containers: Vec<ContainerSummary>,
    images: Vec<ImageSummary>,
    networks: Vec<NetworkSummary>,
    volumes: Vec<VolumeSummary>,
}
~~~

同 Context/endpoint/Engine 的页面共享查询结果，不各自进行全量扫描。每个实际核心任务仍归一个真实 view；共享缓存不是无归属任务的借口。

运行规则：

- 控制方法只修改短临界区内的状态，远程等待不持有 App 锁。后台工作使用 SDK `call_detailed` 保留错误码。
- 查询完成时核对 owner/view/Context/endpoint/Engine、查询修订和 closing 状态，旧查询不能覆盖新快照。
- 按 view 分配递增的十进制 sequence；真实新命令取新序号，重试 start 复用原 sequence/operationId/完整输入。
- 一个工作区最多执行一个变更子操作，同时最多一个普通查询；全局仍受核心额度控制。流读由一个调度循环处理，避免每个 iframe 创建 worker。
- 同类读请求合并；变更操作优先于定时刷新。resourceBusy 时读请求有界退避，变更排队并显示位置。单实例最多 100 个待执行对象，各 view 同类查询只保留一份待执行项；额度不足明确拒绝，不能在 VecDeque 中无限堆积。
- browser 逻辑页存活时当前分类默认 5 秒刷新；隐藏不会销毁逻辑页或取消已接受任务。用户可以暂停自动刷新。没有 browser/详情采集订阅时不扫描全量清单。
- 容器详情开启指标后默认 2 秒采样，隐藏继续保留短趋势，关闭/显式暂停停止订阅。最多 8 个容器指标订阅，同一容器合并。
- Engine 基础信息 60 秒刷新；磁盘统计手动刷新，避免每 5 秒运行较贵的 df。
- 清单与详情缓存每 Context 建议最多 4 MiB/10,000 对象，单实例合计最多 32 MiB；优先回收没有订阅的缓存。仍达上限则 coverage=partial 并提示，不用部分缓存宣布“没有引用、可以删除”。单对象解析也有大小/字段预算，不能先完整载入任意大 inspect 再检查总量。

### 5.3 归属表

| 对象 | 唯一管理者 | 停止与释放 |
| --- | --- | --- |
| 连接与认证资料 | 核心连接服务 | 按既有连接生命周期，Docker 任务取消不关闭共享连接 |
| inventory/详情查询任务 | 发起查询的逻辑 view，经核心登记 | view 关闭或 Context 失效取消；缓存消费者下次自己发起查询 |
| 指标采样任务 | 实际采样订阅所属 view | 订阅关闭时取消；剩余订阅只在下一轮选择有效 owner |
| 日志跟随与行缓存 | logs view + 插件 runtime | 暂停/关闭停止本次命令，保留有界日志；关闭释放缓存 |
| 变更命令与拉取 | operations view | 明确取消或工作区/进程退出时收尾；详情关闭不影响它 |
| 导出源快照 | logs view + 插件 logs 模块 | 不可变、最多 2 MiB，有效期和 export ticket 引用结束后释放 |
| 本机导出 ticket/临时文件 | 核心，归 view/实例/Context | 完成、取消、关闭或插件退出清理临时文件 |
| Engine 的容器、镜像、网络和卷 | 远端 Docker daemon | SoDeck 退出不会停止或删除它们 |

## 6. Engine 探测与概览

### 6.1 用户行为与代码路径

首次打开 browser：

1. `engine.rs::probe` 通过 `session.state` 确认当前连接；验证 endpoint。
2. 短命令检查 POSIX 执行环境与 `command -v docker`；不自动安装。
3. 执行 `docker --host ENDPOINT version --format '{{json .}}'`，提取 Client/Server 版本和 Server.Os/Arch。
4. 执行 `docker --host ENDPOINT info --format '{{json .}}'`，提取 ID、Name、NCPU、MemTotal、DockerRootDir、存储驱动及计数。
5. Engine ID 确认后建立 EngineKey，再查询当前分类。手动“刷新磁盘使用”调用 `system df --format '{{json .}}'`。

`main.rs` 的事件入口为 `refreshEngine`、`refreshDiskUsage`、`setEndpoint`；都返回排队信息，后台更新 revision。`engine.rs` 只保留白名单字段，不把整个 info 输出写入日志。

### 6.2 展示与失败

- CPU/内存总量是 Engine 所在系统信息，不能误标为当前容器限制。
- df 的 Size/Reclaimable 是 CLI 的汇总估计，保留来源与时间，不把各镜像 Size 相加当磁盘真实占用，也不承诺删除某镜像可释放全部 Size。
- daemon 不可达、socket 拒绝、CLI 缺失、远端系统不支持、格式不兼容、SSH 断线分别显示。CLI stderr 无法可靠结构化时保留经裁剪的诊断，并以“Engine 不可访问”作为确定结论，不能仅凭英文字符串断言一定是权限错误。
- 失败保留旧快照，标明采集时间与过期状态；Engine ID 改变立即使旧对象失效，要求重新加载。

验收：正常 Engine、rootless socket、CLI 缺失、socket 拒绝、daemon 停止、非 Linux Engine、ID 改变和重连均有不同可解释结果；不会访问远端默认 context 指向的其它 endpoint。

## 7. 容器列表、筛选与 Compose 分组

### 7.1 数据与命令

`containers.rs::list` 先取得所有完整 ID：

~~~sh
docker --host ENDPOINT container ls --all --quiet --no-trunc
~~~

对 ID 分批 inspect，建议每批 32 个，整个命令仍不超过核心 16 KiB：

~~~sh
docker --host ENDPOINT container inspect --format '{{json .}}' ID...
~~~

默认使用短命令。inspect 单批超过输出限额时，只对读请求缩小批次；单对象仍超限则使用第 15 节流式有限任务并限制对象解码大小，明确报告 partial。实现可采用固定字段 summary 模板减少输出，但必须测试转义、可选字段和健康检查缺失。

提取 `ContainerSummary`：Id、Name、Config.Image、State.Status/Running/Paused/Restarting/ExitCode/StartedAt/FinishedAt、可选 Health.Status、Created、Config.Labels、NetworkSettings.Ports。Compose 取 `com.docker.compose.project`/`service`/`container-number`/`oneoff`，不能解析人类表格或逗号拼接的 Labels 文本。

### 7.2 UI 和操作

`browser.js` 保存 category、query、statusFilter、healthFilter、groupMode、sort、selection；`ui.event` 使用 `list`、`setFilters`、`openContainer`、`openLogs`。搜索在有界规范化缓存中进行，支持名称、完整/短 ID 的显示搜索、镜像、label key/value；搜索命中短 ID不代表允许短 ID执行操作。

状态规则：

- exited/created 是生命周期状态，不自动视为故障。
- 非零退出、restarting、unhealthy 作为不同信息表达；无健康检查显示“未配置”，不能显示 healthy。
- running 且 paused 展示暂停；健康状态未知显示未知。
- 失效列表只供阅读，不允许直接据此删除。

刷新采用新的快照修订，整轮成功后提交；对象在 ls/inspect 之间消失属于可预期竞态，移除该对象并记录 scan coverage，不能把其它有效数据清空。

验收：含逗号/引号/Unicode 的 label、停止容器、unhealthy、oneoff、分页选择、刷新期间消失、大清单 partial 和两个窗口定位同容器都正确。

## 8. 容器详情与关联资源

`containers.rs::inspect` 只按完整容器 ID查询；`details.js` 按分区请求 `detail`，不一次向 UI 返回完整 inspect。

| 区域 | 读取字段/代码处理 | 可用动作 |
| --- | --- | --- |
| 基础与启动 | Name/Id/Created、Config.Image、Path/Args、Entrypoint/Cmd、WorkingDir/User、restart policy | 复制字段、定位镜像 |
| 状态与健康 | State、RestartCount、可选 Health 及有界最近检查记录 | 刷新、打开日志，显示退出码/OOMKilled/Error |
| 环境变量 | Config.Env，拆分第一个 `=` | 默认隐藏值，用户“显示值”或复制选中项；不写诊断日志 |
| 端口 | NetworkSettings.Ports 与 HostConfig.PortBindings | 区分容器端口、协议、HostIp/HostPort 和未发布端口 |
| 挂载 | Mounts.Type/Name/Source/Destination/RW/Propagation | 按卷名定位卷；bind 路径仅显示为远端路径 |
| 网络 | NetworkSettings.Networks 的网络 ID/IP/别名 | 打开网络详情 |
| 标签 | Config.Labels 分页 | 搜索与复制，定位 Compose 项目 |

端口 `0.0.0.0`/`::` 不能直接作为可点击网页地址；本版不自动建立隧道。容器内路径不作为主机文件路径传给文件插件。详情中删除/启停操作都进入 operations view。

对象已删除时页仍存在，显示“容器已不存在”和最后采集时间；禁用操作，用户可以复制原记录或关闭。相同名称的新容器不替换原页。

验收：非默认 entrypoint、含 `=` 的环境变量、IPv6、未发布端口、bind/volume/tmpfs、无健康检查、容器删除同名重建和大详情分块读取。

## 9. 启停、暂停、恢复、删除与批量操作

### 9.1 共用操作入口

`operations.rs::submit` 接收类型化意图，不接受任意 shell：

~~~rust
enum ContainerAction {
    Start,
    Stop { timeout_seconds: u16 },
    Restart { timeout_seconds: u16 },
    Pause,
    Unpause,
    Remove { force: bool },    // 不包含隐式删除卷
}
~~~

停止/重启默认 grace timeout=10 秒，允许 1–120 秒。操作采用流式有限任务，command timeout 默认 grace timeout+30 秒，最长 5 分钟。Docker daemon 接受请求后可能继续执行，任务取消不是远端操作回滚。

流程：

1. 页面提交唯一 requestId 与完整资源 key，插件校验当前 Context、Engine 与操作参数。
2. 通过 `ui.openView({expectedContext,requestId,viewType:"operations"})` 取得稳定 viewId；未确认任务页建立前不执行远程变更。
3. 请求意图去重，按完整对象记录批量选择；将操作意图持久化成功后才登记可派发状态。
4. 后台重新 inspect 和核对 Engine；根据真实状态拒绝不合法操作或报告“已处于目标状态”。
5. 接入核心执行任务并保存 taskId；轮询输出与结果，不用 stdout 的成功提示代替退出状态。
6. 退出码为 0 后再次 inspect，报告“命令成功/当前观测状态”；若验证失败，分别保留命令结果与验证状态。
7. 使受影响缓存过期，并发布任务页修订与 busy 状态。

变更意图在插件中单独做有界去重：同源逻辑 view 保存递增 intentSequence 和最近 64 个 requestId 回执；getView 返回下一序号，flush 同步已提交序号。相同编号/完整参数返回原操作，参数变化拒绝；回执回收后旧序号返回 operationExpired，不分配新核心序号重新执行。核心命令 sequence 仍按实际任务 owner 分配，两种序号分别解决用户意图重复与远程 start 重试。

### 9.2 命令与条件

| 动作 | 固定命令 | 条件及结果 |
| --- | --- | --- |
| 启动 | `container start ID` | running 报告无需启动；其它状态由 daemon 最终判断 |
| 停止 | `container stop --time N ID` | 停止状态报告已停止，过程保留 grace timeout |
| 重启 | `container restart --time N ID` | 停止容器也可能被启动，确认文案明确说明 |
| 暂停 | `container pause ID` | 必须运行且未暂停；rootless/平台不支持显示 daemon 原因 |
| 恢复 | `container unpause ID` | 必须 paused；已恢复报告无需重复 |
| 删除停止容器 | `container rm ID` | 默认不使用 `--volumes`/`--force` |
| 强制删除运行容器 | `container rm --force ID` | 确认界面明确显示将停止并删除运行容器；仍不加 `--volumes` |

删除确认展示 Engine、完整 ID、当前状态、名称、挂载卷和不可撤销影响；卷保持，bind 数据不由本操作删除。不会把“删除容器”与“清理卷”合并成默认动作。

批量操作最多 100 个对象，顺序执行并保留每项 success/failed/skipped/unknown/cancelledBeforeStart；取消批次只取消当前本地等待并停止派发后续项，已完成项不回滚。批量删除无原子承诺。相同对象在本工作区内的冲突操作串行；其它工具或其它连接的并发修改仍由 daemon 判断。

### 9.3 结果未知

`execSent=false` 且已确认未发送时可以报告“未执行”；`execSent=true` 后断线、超时、缺少退出状态或取消，只能说明本地任务结束/远端结果未知。禁止自动再次 restart、rm 或 pull。

恢复按钮为“刷新对象状态”，不是重放命令。启动/停止/删除可以观测目前是否达到目标，但不能据此宣称原请求一定造成了变化；重启即使 StartedAt 改变也可能来自其它客户端。需要再次操作时用户发起新的明确意图。

验收：同 requestId 重复点击只执行一次；SDK start 回包超时可查询原任务；超时、断线、取消不重放；批量部分成功不会显示全部成功；详情关闭不取消操作，任务页关闭采用第 16 节规则。

## 10. 容器日志

### 10.1 查询与跟随

`logs.rs` 定义 `LogQuery { tail, since, until, follow }`。默认 tail=500，允许 1–5,000；时间转成明确 RFC3339/UTC，校验 since≤until。不把任意“相对时间表达式”直接拼入 shell。

~~~sh
docker --host ENDPOINT container logs --timestamps --tail N \
  [--since RFC3339] [--until RFC3339] [--follow] CONTAINER_ID
~~~

历史查询是有限流式任务，默认 30 秒；follow 是有续租的 continuous 任务。历史加跟随使用同一条命令，避免“先查询历史再另开跟随”产生接缝。启动前 inspect 确认对象、Config.Tty 和日志驱动。

日志驱动不提供读取能力时，报告实际原因；不偷偷改日志驱动或改读远端文件。停止容器仍允许读历史；跟随结束时显示“日志命令已结束”，不循环重新创建命令。

### 10.2 数据管线

~~~text
核心流式任务
  → runtime 按 offset 读取并提交到插件缓存
  → logs.rs 增量 UTF-8 解码、时间戳与行切分
  → 有界 LogBuffer（sequence、stream、timestamp、text）
  → ui.event 的分块 readLogs
  → logs.js 行窗口与自动滚动
~~~

单日志页缓存默认最多 2 MiB 或 10,000 行，取先到的限制；单条未结束行最多 64 KiB。插件日志缓存总预算建议 16 MiB。超出后移除最旧记录，保留“此前内容已移出缓存”的标记和最早 sequence，不能显示成完整历史。

核心输出接收与 UI 展示分开：plugin 在写入有界缓存后 ack，隐藏的 iframe 不承担续租。读取 UI 使用 `afterSequence`/`maxBytes`，单包正文不超过 UI 上限；客户端重复读相同 sequence 不重复追加，低于保留范围返回明确的 gap。

实现要处理：

- UTF-8 字符、时间戳与换行跨核心块；坏字节明确替换，不能在每块直接 lossy 解码造成汉字损坏。
- ANSI/OSC 和控制字符按普通日志规则裁剪/转义，使用 textContent 绘制，不运行终端控制序列。
- 无换行长记录有界裁剪，保留被裁剪字节计数；CR 不触发任意终端光标操作。
- TTY 容器通常已合并 stderr；非 TTY 的 CLI stdout/stderr 也可能包含诊断。不能把 stderr 每一行视为失败，也不能把两路读取顺序声称为容器产生消息的严格全局顺序。
- 保留原时间戳精度；展示顺序是插件接收顺序，必要时提示两路输出限制。是否失败由任务退出状态与 error 判定。

### 10.3 页面动作与恢复

| 动作 | 行为 | 代码入口 |
| --- | --- | --- |
| 查询历史 | 新建有限任务，保留查询边界和覆盖情况 | `logs.rs::start_query` |
| 跟随 | 新建 continuous 任务，一页最多一个跟随 | `logs.rs::start_follow` |
| 暂停采集 | 取消当前命令，保留已缓存行 | `logs.rs::pause` |
| 自动滚动开/关 | 仅改变滚动，不停止采集 | `logs.js` 展示状态 |
| 搜索 | 搜索当前缓存，显示命中数量与缓存范围 | `logs.rs::search`，分页命中定位 |
| 复制 | 复制选中行或有界当前查询结果 | 既有 clipboard/text-menu bridge |
| 导出 | 冻结不可变缓存快照，再走第 15.2 节本机导出 | `logs.rs::make_export` |

断线保留日志并插入中断标记。后续完整版本不自动续开跟随；用户重连后选择“重新查询/继续跟随”，建立新任务。重新查询可能与旧缓存重叠，明确分段显示，不承诺暂停或断线期间无遗漏，也不以文本去重误删相同日志。

取消采集只终止 `docker logs` 客户端命令，不停止容器。关闭日志页自动取消跟随且无需业务删除确认；正在导出的关闭规则见第 16 节。内存容量达到阈值不自动导出远端日志。

验收：history/follow、两路输出、TTY、中文跨块、无换行、CR/ANSI、时间边界、日志驱动不支持、暂停滚动仍采集、缓存 gap、移窗不断跟随、断线分段和慢 UI 不导致无界内存。

## 11. 容器资源指标与短趋势

`stats.rs` 使用短命令批量查询开启采样的运行容器：

~~~sh
docker --host ENDPOINT container stats --no-stream --no-trunc \
  --format '{{json .}}' ID...
~~~

默认 2 秒一轮，允许设置 2/5/10 秒；最多 8 个容器、每个容器保留 60 个点。收到新的 running 状态前不采样停止容器；停止期间显示“未运行”，保留已有曲线并画断点。

`StatsSample` 保留 observedAt、CPU%、内存用量/限制、NetIO、BlockIO、PIDs 与字段支持情况：

- CPU 可超过 100%，不能按单核百分比强行截断。
- CLI 的单位字符串由集中解析器处理 B/kB/MB/GB 与 KiB/MiB/GiB，输入有界且拒绝 NaN/无限值/异常单位。保留显示精度与“CLI 舍入值”来源。
- NetIO/BlockIO 显示累计读写量；速度从相邻样本差值与实际采样间隔计算，首点、计数下降、重启或采样缺失产生空值，不生成负速率。
- rootless/cgroup/平台缺少字段时显示不支持或未知，不能拿 0 表示采集失败。
- 详情中 HostConfig.Memory 与 Engine MemTotal 分别说明限制来源；不把宿主总内存当容器显式配置限制。

`runtime.rs` 按 Engine 合并同一容器订阅，采样任务选择有效订阅 view 作为 owner。关闭 owner 时取消本轮，下一轮才在剩余订阅中重新选择；不能改写已启动核心任务的归属。

`details.js` 用小型 SVG 绘制趋势，复用现有主题，禁用动效时保持静态。缓存、查询任务失败、状态变化都由插件发布修订。

验收：多核 CPU、单位与舍入、计数复位、首点空速率、rootless 缺失、容器停止/重启、两个详情订阅合并、view 关闭和延迟采样不重叠。

## 12. 镜像管理与拉取

### 12.1 本地镜像与引用

`images.rs::list`：

~~~sh
docker --host ENDPOINT image ls --all --no-trunc --digests --format '{{json .}}'
docker --host ENDPOINT image inspect --format '{{json .}}' IMAGE_ID...
~~~

按完整 Image ID 合并重复行，提取 RepoTags/RepoDigests/Created/Size/Os/Architecture/Labels；UI 一行表示一个镜像对象，并展示全部标签。原始字节大小来自 inspect，不从表格列解析。无标签镜像正常显示。

容器引用取完整容器 inspect 的 `.Image`，包括停止容器；一个正在运行/停止的容器引用都算“使用中”。清单不完整时引用状态为 unknown，禁止声称未使用。

`resource` 页显示标签、digest、平台、创建时间、大小和关联容器。镜像大小包含共享层，不能等同于删除可释放空间。

### 12.2 拉取

`images.rs::validate_reference` 提供明确的引用语法子集：repository、可选 registry/port、tag 或 sha256 digest；默认 tag=latest，在确认中展示解析后的完整引用。限制长度、拒绝空白/控制字符/option 风格输入，使用独立的组件解析与测试，不新增依赖或放行任意 Docker 参数。

后续完整版本按远端 Engine 默认平台拉取，不按 SoDeck 客户端架构选择镜像。动作进入 operations 页：

~~~sh
docker --host ENDPOINT image pull REFERENCE
~~~

使用有限流式任务，默认最大 30 分钟。`images.rs` 增量处理 CR/行片段，尽力提取每层的等待/下载/解压/完成及 bytes；无法识别的版本输出保留为有界进度文本，不虚构总体百分比。

结束后保留 CLI 退出码，再 inspect reference 确认当前镜像 ID/digest。tag 可能在拉取中更新，展示实际观测结果。取消或断线的操作为结果未知；daemon 可能继续工作，刷新本地镜像可以确认当前状态，不能自动再次 pull。

### 12.3 删除

后续完整版本删除的是镜像对象，不是隐藏的“解绑一个标签”：

~~~sh
docker --host ENDPOINT image rm --no-prune IMAGE_ID
~~~

使用完整 Image ID、不加 force，保留 Docker 对多标签或正在使用镜像的拒绝。确认列出所有标签、digest 和引用；只对新鲜且完整引用扫描中无容器引用的对象允许提交。多标签对象可能被 daemon 拒绝，后续完整版本如实报告并说明标签管理尚未提供，不自动切换成按 tag 删除或强制删除。

`--no-prune` 避免顺带选择未列出的父镜像；底层已无引用的数据层由 daemon 管理。批量删除进入统一任务队列并逐项记录，不调用 image prune。

验收：同 ID 多标签合并、无标签、停止容器引用、registry:port/tag/digest、非法引用、拉取进度跨块、认证助手失败、tag 更新、取消后刷新、multi-tag 拒绝和外部新建引用竞态。

## 13. 网络、卷与已有 Compose 项目

### 13.1 网络

`resources.rs::list_networks/inspect_network` 使用：

~~~sh
docker --host ENDPOINT network ls --no-trunc --format '{{json .}}'
docker --host ENDPOINT network inspect NETWORK_ID...
~~~

模型包含 ID/name/driver/scope/internal/attachable/ingress/IPAM/options/labels 和连接容器。详情分页展示 subnet/gateway、IPv4/IPv6、容器端点和别名；缺少 IPAM 不视为解析错误。

删除候选必须为 local scope 的自定义网络；保护 bridge/host/none、默认 bridge 与 ingress；不做网络 detach。除 network inspect 端点外，还扫描全部容器的网络引用，包括停止容器。扫描不完整则不允许删除。

`network rm NETWORK_ID` 不 force，派发前再次 inspect 与引用检查；daemon 最终拒绝有活动端点的网络。删除成功刷新网络及容器缓存，不移除其它网络。

验收：IPv6、多 IPAM、internal、停止容器引用、内置/ingress 保护、删除前新端点加入、Engine 的非 local scope 和 scan partial。

### 13.2 卷

`resources.rs::list_volumes/inspect_volume` 使用：

~~~sh
docker --host ENDPOINT volume ls --format '{{json .}}'
docker --host ENDPOINT volume inspect VOLUME_NAME...
~~~

模型包含 name/driver/scope/CreatedAt/Mountpoint/options/labels。关联容器来自全部容器 `Mounts` 中 type=volume 的 Name；包括停止容器。不通过名字形状推断“匿名卷”，不把 bind mount 计入卷。

UI 显示远端 Mountpoint，但不把它当本机路径打开。CLI 不提供可靠卷用量时显示未知，不通过 `du` 扫描卷内容。

删除使用 `volume rm VOLUME_NAME`，不 force。确认明确写出数据会不可恢复地删除，列出卷名、Engine、指纹和当前引用；container rm 默认保存卷，本动作是独立的明确选择。

`resources.rs::check_volume_remove` 在派发前确认完整引用扫描与卷指纹；检查与删除不能跨 daemon 原子锁定，同名重建竞态仍是 CLI 方案限制。失败/超时不自动重放，也不运行 volume prune。

验收：停止容器引用、driver/options、缺少 CreatedAt、bind 与 volume 区分、数据保留于容器删除、卷显式删除、同名重建指纹变化及数据用量未知。

### 13.3 Compose

不要求远端安装 Compose CLI。`resources.rs::group_compose` 只依据容器准确 label 识别 project/service/container-number/oneoff 和可选 working_dir/config_files：

- project 在同 Engine 中聚合，resourceKey 使用 Engine ID + project 名；若工作目录/配置文件 label 不一致，显示多个来源，不能任意选一个。
- 汇总已观测到的服务、容器、状态、健康与引用资源；没有部署文件就不能推断期望副本数或缺失服务。
- oneoff 单列标记；无 Compose label 的容器留在普通容器分组。
- 点击服务打开对应容器列表；项目详情可显式选择当前已观测容器后提交启动/停止/重启批次，先展示确切对象列表。
- 批量操作冻结对象选择，不在执行途中把新出现的服务容器加入批次；不运行 compose up/down，不把卷/网络自动纳入项目删除。

验收：多服务、多副本、oneoff、同 project 不同来源、无标签、部分扫描、服务运行中新增容器，以及 Compose 不存在时仍能查看关系。

## 14. 任务记录、设置、诊断与消息

### 14.1 业务任务状态

~~~text
queued → checking → starting → running → verifying → succeeded
   └───────────────→ failed / cancelledBeforeStart
starting/running/verifying → outcomeUnknown
~~~

核心执行状态与业务状态分别保存。cancelled/timedOut 不自动映射为“Docker 操作失败”；命令成功但验证未完成也分别记录。

`Operation` 建议字段：requestId/operationId、ownerViewId、Context、endpoint/Engine ID、action、完整对象、submittedAt、stage、coreTaskId、execSent、remoteExitConfirmed、exitCode、验证结果和裁剪诊断。字节数、核心序号和长整数在 UI 使用十进制字符串。

`operations.rs` 在插件 dataDirectory 保存版本化操作意图，最多 100 个最近变更记录、文件最多 1 MiB；回收旧终态记录，运行中记录不能因容量被丢弃，仍无空间则拒绝新变更。不保存认证、环境变量、日志正文或可自动执行的重放队列。使用同目录临时文件写入并替换，写入失败时拒绝派发变更；坏文件保留供诊断，不恢复执行。重启读到旧 starting/running/verifying 统一显示结果未知，只支持人工刷新观测；旧 Context/taskId 不用于恢复执行。

删除任务记录只删除本机历史；运行中的操作不能用“清空历史”取消。

### 14.2 设置

通过既有 settings schema 声明 live 设置：

| 设置 | 建议默认与范围 | 生效行为 |
| --- | --- | --- |
| 列表刷新间隔 | 5 秒，可选 5/10/30 秒 | 调整下一轮，不启动重叠扫描 |
| 指标间隔 | 2 秒，可选 2/5/10 秒 | 当前采样完成后切换 |
| 默认历史行数 | 500，范围 1–5,000 | 下一次日志查询生效 |
| 停止 grace timeout | 10 秒，范围 1–120 秒 | 下一次 stop/restart 使用 |
| 日志自动滚动 | 开 | 页面初始偏好，可单页临时关闭 |

endpoint 是服务器连接配置，采用插件按 serverId 保存的专用小表单；不把 socket 路径放进全局设置。缓存容量、执行配额和危险行为不是后续完整版本设置项。插件启用/关闭/升级仍重启生效。

`settings.changed` 按核心 revision 接受新值，重复或迟到修订不覆盖当前设置。所有派发前已冻结的操作继续使用其保存的参数，live 设置不会改变运行中 stop 的 grace timeout。

### 14.3 日志与重要消息

`logging.write` 记录 action、operationId、对象 ID、阶段、错误码、耗时和取消原因；核心添加插件/实例来源与接收时间。不会记录完整 inspect、环境变量、认证配置或业务日志。

读取失败在所在页面显示，不每轮弹通知；重复连接错误合并。显式变更失败、结果未知、后台拉取结束可调用 `messages.publish`，每个 operation 终态最多一次，文字说明实际结论并关联任务信息。日志级别不自动决定弹通知。

建议业务错误码：cliMissing、engineUnavailable、endpointRejected、unsupportedEngine、unsupportedFormat、objectGone、objectChanged、scanIncomplete、logUnavailable、outputLimit、operationRejected、outcomeUnknown。无法确定原因时保留通用码和有限诊断，不猜测成功或权限原因。

## 15. 核心新增接口与实施方案

下面两项是拟新增通用能力。Docker 的对应功能必须等契约实现并验证后才能使用；不能直接把这些参数发给当前 session.exec.v1。

### 15.1 有界流式非交互命令

建议能力 `session.exec.stream.v1`，新方法 `session.exec.stream.start`/`session.exec.stream.read`，复用 `session.exec.cancel`/`release`。旧 start 参数使用 deny_unknown_fields，保留原语义和限制，不能给它追加可选“无限执行”参数。

start 示例：

~~~json
{
  "viewId": "logical-view",
  "expectedContext": {"workspaceId":"w","serverId":"s","connectionId":"c","generation":1},
  "operationId": "op-unique",
  "sequence": "12",
  "command": "validated non-PTY command",
  "mode": "finite",
  "timeoutMs": 1800000
}
~~~

continuous 采用 `mode:"continuous"`，不携带 timeoutMs；finite timeoutMs 为 1,000–1,800,000。command、operationId、Context 与短命令保持原有校验。start 返回 taskId/state/leaseMs，lease 固定建议 15 秒，不作为用户可调整设置。

read 示例：

~~~json
{
  "taskId": "task",
  "stdoutAck": "8192",
  "stderrAck": "0",
  "maxWaitMs": 1000
}
~~~

返回每路 `{offset,dataBase64,nextOffset}`，以及 state、outputComplete、exitCode/exitSignal、execSent、remoteExitConfirmed、error/elapsedMs/leaseMs。每路单包仍≤8 KiB，maxWaitMs 为 0–1,000，控制调用不会等待命令结束。

确认与缓存规则：

- 每路固定 256 KiB 环形缓冲，绝对 byte offset。初始 ack=0；只有把返回字节提交插件缓存后，下一次才发送 nextOffset 作为 ack。
- 相同 ack 可重读，允许最后读到的数据在回包丢失后重传；ack 不能超过已交付范围或倒退到已确认释放范围。单任务只有插件的一个消费者，iframe 不直接消费它。
- ack 释放已提交的字节；未 ack 的数据不被覆盖。缓冲满时暂停从远端通道消费，以反压等待，不能默默丢日志。
- read 在同一短临界区先验证两路 ack，再一起提交确认与续租；一侧参数错误不能先释放另一侧数据。
- worker 等待缓冲空间时仍能响应取消、deadline、lease 和 session loss，不持 registry/output 锁等待，也不能妨碍其它通道。
- 成功的 stream.read 续租；插件进程负责后台续租，页面隐藏和正常移窗不影响租约。旧 session.exec.poll 只处理短命令，不读取或续租流式任务。超时或失联取消本任务，不能无限保留远端命令。
- worker 结束后不立刻丢弃剩余输出；插件继续 read，提交缓存后确认两路 offset，读完再 release。流式终态保留到连续 30 秒没有合法 read 或远端退出后 5 分钟，取先到者；到期返回 taskExpired，插件明确标记未读完。短命令仍保持原来的结束后 30 秒回收。
- worker 退出释放在途名额，尚未读完的缓冲与终态记录继续占用输出/记录预算；只有 release 或到期回收才归零。有待取输出时使用 maxWaitMs=0，各任务轮流读取，不能每路都空等 1 秒而拖延其它任务的续租。
- 取消/释放可以在 closing/frozen/stale 状态清理自己的任务；创建新任务必须经过既有 resource admission。

配额建议直接接入已有总预算与记录上限：

| 限制 | 设计 |
| --- | --- |
| 全部 exec 任务 | 仍为全局 8 / 每实例 4 |
| 流式长任务 | 全局最多 4 / 每实例最多 2，包含 follow 与长变更 |
| 连续跟随 | 单实例默认最多 1 个 follow，给变更保留 1 个长任务名额；第二个日志页仍可查询历史 |
| 输出预留 | 每流式任务 512 KiB，计入原全局 16 MiB/实例 8 MiB |
| 记录 | 沿用每实例 64 条及序号水位 |

这保证 Docker 长任务不会占满所有全局短命令名额；其它插件之间仍需要公平排队。资源已满时明确返回 resourceBusy，Docker 显示名额与排队原因。批量操作逐项派发，历史日志查询也需要流式名额，不能绕过配额。

拟改动：

| 文件 | 代码方案 |
| --- | --- |
| `src-tauri/src/session_exec.rs` | 共用 registry/Owner/序号去重，增加 typed StreamStart、输出模式与 read；Start/StreamStart 用枚举区分，跨模式复用 operationId 仍检查完整输入冲突；旧 Start 保持 |
| `src-tauri/src/session_exec/stream.rs`（新增） | 环形缓冲、ack、lease/deadline、反压与有限等待读取；只提取两个真实输出模式需要的公共部分 |
| `src-tauri/src/connections/route.rs` | 复用通道接口；仅在不能取消反压等待时补足通用通道行为，不传 Docker 语义 |
| `src-tauri/src/plugins/process.rs` | initialize capabilities 加 stream 能力 |
| `src-tauri/src/plugins/dispatch.rs` | 现有 session.* 路由接新方法，结构化错误保留 |
| `src-tauri/src/resource_owner.rs`、`resource_tasks.rs`、`lifecycle/close.rs` | 新任务纳入原 admission/pending/cancel，worker 退出前不提前释放预留预算 |
| `crates/plugin-client/` | 优先沿用 call_detailed；仅补必要错误/协议 fixture，不加 Docker 专用 SDK |
| `src-tauri/tests/fixtures/system_status_flow.rs` | 旧短命令去重、输出限额、取消与连接复用回归保留 |
| `src-tauri/tests/fixtures/session_exec_stream_flow.rs`（新增） | 慢消费、重读/ack、超期、终态读取、取消反压、quota、关闭/断线竞态 |

API 主版本保持 1，以可选能力扩展兼容。Docker 初始化要求 stream 能力，不在旧核心静默降级成每 30 秒重开日志；旧系统状态继续使用 session.exec.v1，终端保持原 terminal/terminal.launch 契约。

### 15.2 本机文本快照导出

建议能力 `ui.exportText.v1`。核心只负责将一个已登记、有界、不变的插件文本源保存到用户选择的本机文件，源内容由插件持有；不把 Docker 数据伪装成 FileResource，也不要求文件插件安装。

流程：

1. logs view 的 `makeExport` 冻结 UTF-8 文本快照，最多 2 MiB，返回 snapshotId/byteLength/建议 filename。快照是缓存范围导出，不是全量远端日志归档。
2. 活跃 Surface 发起 `sodeck.request("data", {op:"exportText",action:"start",requestId,snapshotId,filename})`。
3. 核心在输入准入的短临界区登记归 view/实例所有的 preparing ticket，返回 exportId；后台向同一插件调用专用 `ui.exportText` describe，以 exportId 绑定并保留快照，确认长度与源身份。跨进程调用不持有核心锁；回包后重新核对 ticket/Owner，失效则 release，不打开对话框。
4. 核心将状态改为 choosing，在实际窗口打开已有 Tauri 原生保存对话框；不让控制 RPC 一直等待用户选择。
5. 选定路径后再次确认 ticket/源/输入阶段有效，按≤16 KiB 的块通过专用 ui.exportText read 取源，写同目录临时文件、sync、校验目标变化、原子发布。
6. 页面使用 data 通道 status 查询；成功、失败、取消均通知插件 release 快照并清理临时文件。

拟契约：

| 入口 | 参数/结果 |
| --- | --- |
| data `exportText.start` | requestId/snapshotId/filename → exportId/state；同 view 相同输入去重 |
| data `exportText.status` | exportId → preparing/choosing/writing/succeeded/failed/cancelled、writtenBytes、error |
| data `exportText.cancel` | exportId → 当前状态；禁止迟到 picker 结果继续写入 |
| 核心→插件 `ui.exportText` describe | 核心固定 viewId/Context + exportId/snapshotId → byteLength/contentType；首次调用原子绑定快照，同票据重试返回原描述，其它票据拒绝 |
| 核心→插件 `ui.exportText` read | exportId/snapshotId/offset/maxBytes≤16 KiB → dataBase64/nextOffset；只读该票据绑定源，相同偏移可重读 |
| 核心→插件 `ui.exportText` release | exportId/snapshotId → released；幂等，迟到 describe 不能重新保留已释放快照 |

描述和内容专用方法受 RPC 64 KiB 信封限制，但每块≤16 KiB；不用普通 ui.event 承载全部正文。后续完整版本固定为 text/plain UTF-8，filename 是建议 basename，不能由页面提供本地目录。总量/块大小/UTF-8/offset 均由核心检查。

支持源协议的插件在 initialize 返回 `uiExportTextApiVersion:"1"`；`plugins/mod.rs::Instance` 保存该协商结果，`plugins/process.rs` 在初始化时验证并登记，同时将 `ui.exportText.v1` 加入核心能力。`component_ui/host.rs` 只向声明支持的同一真实实例调用专用方法，并保留 view 调用计数与关闭检查。快照 ID 单次登记，release 后不能复活；原 ticket 回收后的重复 start 返回 expired/sourceGone，不用同一快照再次发起写入。

资源规则：

- 单 view 最多一个 export，应用同时最多两个活跃 ticket 和一个原生选择框；不建立无限选择队列，达到限额明确拒绝。终态回执保留 60 秒、全局最多 64 条，相同 requestId 不重复写文件。
- 快照未 pin 时 60 秒过期；ticket pin 后保留至终态，写入阶段总期限 60 秒。选择路径不占远程执行名额。
- 插件每个 logs view 最多一个未释放快照，单实例快照合计最多 4 MiB；重复 makeExport 返回同一缓存修订的源或明确要求先结束旧导出，不积累未提交快照。
- 路径选择阶段绑定原输入 Surface；若源 Surface 因移窗失效，取消该阶段，迟到选择无效，用户在新位置重新发起。选择完成后的写入归逻辑 view，正常移窗不重新导出。
- 断线不影响仍存在 view 的本地缓存导出。view/插件关闭取消 ticket，禁止迟到回包写入；不删除已有目标文件。
- 本机导出仅检查 view/实例身份和生命周期，不调用 connection_session，不要求 SSH 在线；Context 换代后仍存在的旧日志段可以冻结为新快照，但不得借导出启动远程查询。
- 原生对话框不是任意平台都能被程序强行关闭。优先采用官方异步回调；取消使回调失效，OS 对话框句柄关闭后才认为其原生资源释放。插件快照可在取消后释放，picker 名额仍保留至原生回调结束。五平台必须验证窗口关闭/应用退出时不会留下阻塞退出的 picker，不能靠 JS timeout 宣布资源已释放。
- 选择已有文件时复用系统确认与现有目标变化检查；失败或取消只删自己的临时文件。外部同时修改目标的检测与发布使用现有核心文件语义，不自建一套平台替换实现。

现有 files_export 的目标变化检查是长度/mtime 比较后 rename，不能保证检查到替换之间没有外部写入，也不能识别所有同长度/mtime 变化。文本导出继承这个限制；五平台验证覆盖已有文件的实际替换行为，文案不承诺跨进程事务。快照内容包含日志正文，不进入普通诊断日志。

拟落点：`src-tauri/src/local_export.rs` 管理 ticket、文本源、分块写入与取消；`main.rs::component_data` 路由 exportText 并调用原生 picker；`component_ui/host.rs` 验证 Surface/Owner；`core.rs` 挂载注册表并接清理。`remote_files.rs::files_export` 只提取重复的临时文件发布辅助函数，保留 FileResource、草稿与目标检查。旧文件接口不要求迁移到新源协议。

验收必须包含：离线导出、2 MiB 边界、重复 start、块偏移/源大小错误、picker 取消、移窗、选定后写入关闭、磁盘写入失败、目标改变、插件退出、临时文件清理及旧文件草稿导出回归。

## 16. 完整生命周期、取消与并发规则

### 16.1 视图与任务状态表

| 事件 | browser/详情/指标 | 日志页 | operations 页 |
| --- | --- | --- | --- |
| createView | 注册状态与必要查询订阅 | 注册查询参数，按明确创建动作启动首个查询 | 注册稳定任务归属，加载历史，不重放 |
| getView/普通刷新 | 读取修订，不因挂载重启远程任务 | 读取缓存，不新开 follow | 读取任务，不再次派发 |
| 隐藏/切换标签 | 按逻辑订阅继续；可显式暂停 | follow 继续并由插件续租 | 已接受任务继续 |
| 同窗移组/分屏 | 原逻辑状态保持 | 同一 task/cache | 同一队列和 owner |
| 跨窗候选 | 绘制初始快照，不发送事件/设置写入 | 绘制缓存快照，不接管核心消费 | 绘制初始进度，不新建操作 |
| flush/交接 | 同步本地输入；冻结新用户意图 | 同步滚动/查询表单，不取消 follow | 同步选择与表单，不取消已派发命令 |
| transferCancelled | 恢复原输入权 | 原任务继续 | 原任务继续 |
| 断线 | 标记旧数据，停止本轮采样 | 缓存保留，follow 终止并标中断 | 已发送项 unknown，未派发项停止 |
| 重连 | 新 Context 重新探测，旧结果不提交 | 用户明确重新查询，分段显示 | 刷新观测；不自动重放历史 |
| 真正关闭 | 取消自有查询、撤销订阅、释放状态 | 取消 follow/导出，释放日志缓存 | 通过 busy 关闭决定后取消等待，保留已发送未知结果 |
| 插件/应用退出 | 核心清理归属资源 | 核心清理通道与 ticket | 停止派发、记录未知，不能承诺 daemon 回滚 |

后台操作只改变已存在任务，不能在关闭冻结后排入新的远程命令。checking、verifying 和批量下一项都属于新命令创建，必须重新通过 admission；关闭时停止这些后续阶段，已有命令结果可以用于收尾记录。

### 16.2 关闭行为

- 普通读页面 dirty=false/busy=false；关闭无需额外确认，核心取消自有查询。
- 有 active follow 的日志页也不把“读日志”当不可逆业务。关闭直接取消；只有本机 export 尚在进行时报告 busy。
- operations 有 queued/checking/starting/running/verifying 时 dirty=false/busy=true，`ui.updateView` 与 `ui.beforeClose` 同步反映最终状态。
- 用户取消关闭后，队列恢复准入，但不自动重试被冻结期间已失效的创建请求。
- 既有关闭界面的“保存”不能被解释成等待 30 分钟拉取。`ui.save` 若任务未结束返回清晰错误，要求先等待或取消任务；不在 save RPC 里长期阻塞。
- 用户明确放弃并关闭后，`ui.disposeView` 停止派发、cancel/release 自有命令、记录已发送项 unknown，并幂等释放。未发送项为 cancelledBeforeStart。
- 核心仍负责进程异常退出和插件停用的兜底清理；不能依赖前端 beforeunload。

### 16.3 始终成立的不变量

1. 每个远程命令在写入前有 Owner 和预算；关闭后的迟到请求不能新建任务。
2. 操作名称不是对象身份；相同名字的新容器不会接管原 view 或原变更任务。
3. 读状态/getView/移窗不创建第二份日志跟随、拉取或变更命令。
4. 已发送变更结果未知时不自动重放；coreTaskId 失效也不因此取新 operationId 重发。
5. 取消自己的 Docker CLI 不关闭共享 SSH，不停止容器，不保证撤销 daemon 请求。
6. 不以 partial/过期缓存宣布资源未使用；删除前重新确认，daemon 保留最终拒绝权。
7. 所有正文、缓存、记录、任务和 picker 有预算；前端慢或不可见不能产生无界内存。
8. 输入权与业务任务归属分别管理；正常移窗不把远程任务绑到新窗口。
9. 取消信号、远端退出状态、worker 结束和资源释放是不同事实，不能提前归零预算。

## 17. 测试、构建、工作流与交付

### 17.1 按功能选择验收

| 层级 | 拟落点 | 必须证明的行为 |
| --- | --- | --- |
| Rust 单元测试 | Docker 各业务模块 `#[cfg(test)]` | shell 转义、引用解析、JSON/单位/时间戳、状态与标签、对象 ID、容量限制 |
| 核心契约测试 | `src-tauri/tests/fixtures/session_exec_stream_flow.rs`、本机导出 fixture | 去重、反压、续租、取消、关闭/断线、Surface、实际资源归零，旧 exec/files 回归 |
| 真实插件进程+伪核心 | `scripts/test-docker-process.mjs`（新增） | initialize、UI v2、任务归属、迟到结果、历史 unknown、大小限制与 shutdown |
| UI 行为 | `scripts/test-docker-ui.mjs`（新增） | 筛选/选择、确认真实对象、日志窗口/搜索/复制、候选不发事件、flush、分屏与移窗；三主题、窄侧栏/400 px 日志、键盘及焦点恢复 |
| OpenSSH+隔离 Engine | `scripts/test-docker.mjs`、`src-tauri/tests/fixtures/docker_flow.rs`（新增） | 真实命令、容器/镜像/网络/卷操作、日志和指标、断线后不重放 |
| 实际插件归档 | `scripts/test-docker-package.mjs`（新增） | 解包后的本机原生程序/UI，初始化、create/get/dispose/shutdown、正常退出和缺能力报错 |
| 实际包+真实核心/Engine | 原生验收入口增加 Docker 选择 | 五个客户端目标的发行包通过真实连接执行代表流程，证明不是源码或跨编译代替 |

修复可稳定复现的问题增加回归测试。解析器不只测试当前代码生成的输入，要包含真实 Engine 输出、未知字段、缺字段、无效 JSON、超长字段、重复标签/记录和非英文诊断。

真实集成环境要求：

- 用明确的隔离 Linux Docker daemon/socket 和 OpenSSH fixture，不对开发机已有生产资源执行清理。
- 测试创建的资源带唯一 run label/name；清理按记录的完整 ID/卷名执行，只清理本次资源，不运行全局 prune。
- 测试镜像、Compose label、网络和有数据的卷准备成固定 fixture。拉取用隔离 Registry 与固定 digest，加入延迟/失败场景，避免依赖 Docker Hub 的临时限流或网络状态。
- 可控同步点覆盖“登记后关闭、发送后断线、取消时输出满、对象检查后外部变化”，不靠 sleep 猜测竞态。
- 至少验证 rootful/rootless、最低支持 Engine 与当前稳定 Engine。Linux ARM64 Engine 用原生镜像 fixture，不拿 x86_64 镜像运行成功作为 ARM64 证据。
- 移窗期间跑日志和镜像拉取，打开详情后关闭原窗口，验证实例、操作次数、offset 和缓存没有重建。

### 17.2 接入现有检查与打包

| 文件/入口 | 拟修改 |
| --- | --- |
| `scripts/select-checks.mjs` 及其测试 | 将 docker 加入 pluginNames、插件路径匹配、native/integration 输出和报告；Docker 单改动只触发相关检查，文档仍只检查文档 |
| `package.json` | 实现脚本后才添加 `docker:test`、`docker:ui:test`/相关入口，沿用现有命令风格 |
| `.github/workflows/check.yml` | Docker 原生 Rust 检查和定向 Linux Engine 集成；只有选中 Docker 才准备隔离 daemon/registry |
| `scripts/package-plugin.mjs` | 首先直接使用通用 --plugin docker；只增加确有缺失的资产检查，不复制打包器 |
| `.github/workflows/plugin.yml` | Docker 选择复用五个 native runner；新增实际归档 smoke 条件，启动本机原生插件 |
| `.github/workflows/remote-integration.yml` 或现有实际包验收入口 | 在实施时选择最小兼容接法：加入可选 Docker 包来源/场景，不要求其它插件同时发布；检查包来源提交和原始 run |
| 插件 `Cargo.toml/Cargo.lock/release.json` | 独立版本、API v1/UI v2、五目标与标签前缀，官方发现无需核心维护固定名单 |

本次文档阶段不增加调用尚不存在脚本的工作流。实施后的入口至少包括：

~~~sh
cargo fmt --manifest-path plugins/docker/Cargo.toml -- --check
cargo test --manifest-path plugins/docker/Cargo.toml --locked
npm run docker:test
npm run plugin:build -- --plugin docker
~~~

核心新增执行/导出接口时，运行相关格式、类型检查、核心构建与测试、脚本测试和调用者回归；Docker 各业务阶段运行插件格式、测试、构建及相关 UI/真实流程。通过相关检查后不无故重复所有检查。

发布按[分发流程](distribution.md)与[开发检查范围](development.md#版本与持续集成)：核心扩展独立发布，Docker 随后用自己的版本和 Release 发行。五平台原生包启动/退出和实际组合验收保留；核心打包不依赖 Docker。发行记录保存版本、包来源提交、原始打包 run、Release 与实际验证/跳过范围，不覆盖已正式发行的同版本附件。

### 17.3 文档分工

- 本文维护目标、接口提案、代码计划与验收条件。
- 实现时新增 `docs/implementation-docker-plugin.md` 写实际代码/证据/未完成项，更新索引。
- `docs/plugin-api.md` 只在通用 stream/export 契约实现时更新，明确能力与旧版兼容。
- `docs/usage.md` 维护安装、endpoint、功能操作、限制和未知结果说明。
- `docs/development.md`/`distribution.md` 仅维护实际命令、检查选择与发行流程。
- Docker README/CHANGELOG 维护插件定位、安装要求、版本变化与发行来源。普通功能方案不追加到根 README。

## 18. 实施顺序与评审检查点

每阶段都是一个完整可验证的主题，不把半接入接口作为可发行功能。

| 阶段 | 实施范围 | 阶段完成条件 |
| --- | --- | --- |
| 0：技术探测 | 固定 Engine fixture，验证 CLI JSON/日志驱动/stats/pull 输出；验证原生 picker 与 UI v2 关闭关系 | 明确最低版本/格式、失败边界及 picker 释放行为，修订本文中未验证的参数 |
| 1：插件只读骨架 | 工程/五平台构建入口、UI v2、Engine/endpoint、容器详情与基础资源列表 | 不新增核心能力也能真实浏览；包可 initialize/create/get/dispose/shutdown |
| 2：通用流式执行 | 核心 stream 契约、预算、反压、续租、取消、兼容回归 | 真实 SSH 输出超过旧总量/时限仍有界，关闭断线释放，旧调用者通过 |
| 3：容器操作与任务 | operations view、意图持久化、启停/暂停/删除、批次、unknown | 重试一次执行、断线不重放、关闭/移窗与部分结果完整 |
| 4：日志与指标 | 历史/follow/暂停/搜索/复制、缓存、指标订阅与趋势 | 长时间跟随内存有界，慢消费/移窗通过，缺失指标不显示 0 |
| 5：文本导出 | 通用核心文本源/ticket/picker/原子写入，Docker 快照接入 | 离线/取消/关闭/失败/旧文件草稿导出回归和五平台 picker 行为通过 |
| 6：镜像与资源管理 | 拉取、镜像删除、网络/卷删除、Compose 聚合和批量选择 | 引用完整检查、数据影响明确、只操作选择对象，所有不确定结果保留 |
| 7：独立发行验收 | UI 组合、真实 Engine、五客户端原生包与升级/停止检查 | 实际包验收齐全，API/使用/发行文档同步，记录本次验证范围 |

尚需阶段 0 实证的事项：最低 CLI/Engine 的模板字段与格式兼容；不同日志驱动/TTY 的输出与诊断混合；rootless 指标支持；流式反压时 Docker CLI/daemon 的行为；各系统 picker 取消及窗口退出。它们是验证工作，不表示已发现实际故障。

已知方案限制：远端仅承诺经验证的 Linux Docker Engine；CLI 无法提供事务式“检查再删除”；多标签镜像非强制删除可能被拒绝；暂停/断线日志不承诺无缝恢复；容器指标来自有舍入的 CLI；本机导出只是有界缓存快照。

返回 [文档索引](README.md)

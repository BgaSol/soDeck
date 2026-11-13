# Docker

Docker 是独立插件，默认固定在右侧工具栏、按需打开。通过核心已有短命令能力复用当前连接；Docker 命令和业务规则全部留在插件，不直接依赖 SSH 实现或其他插件。

## 当前功能

- 读取 Engine 概况与容器列表，搜索名称、镜像、标签和完整 ID，筛选状态/健康；“按 Compose 项目排列”按项目名排序，相同项目的容器相邻显示，并展示项目/服务标签。
- 在中央页面查看容器配置、状态/健康、环境变量、端口、挂载、网络和标签。环境变量值默认隐藏。
- 查看最近日志快照，手动刷新、搜索整个缓存、分页及复制当前页；可选择 50/200/500 行，默认 200 行。
- 单项确认后启动、停止、重启、暂停或恢复容器。只允许删除已停止容器，不使用 force 或删除挂载卷参数。
- 运行中的容器详情提供“打开终端”，容器 Shell 默认 /bin/sh，可填写容器内的绝对可执行路径。通过核心 terminal.open.v1 命令请求终端插件，无需直接调用插件。
- 按需读取 CPU、内存、网络、块 IO 和进程数；累计字节不当作速率。列表按设置刷新，详情、日志和指标手动读取。

Compose 分组只是读取标签和排序，没有独立分组标题或折叠操作，也不提供创建、部署或管理 Compose 项目的能力。当前也不提供实时日志跟随、导出文件、批量操作或镜像/网络/卷管理。

## Endpoint 与操作语义

当前远端范围为 Linux Docker Engine，默认明确指定 `unix:///var/run/docker.sock`，可保存其他绝对 Unix socket。更改 endpoint 前需要关闭该服务器的详情/日志并等待请求完成；配置保存在插件数据目录的 `endpoints.json`。

命令清除 Docker host/context/TLS/API 环境覆盖并显式指定 socket，不自动安装 Docker、不修改用户组、不添加 sudo。每次采集核对 Context、Engine ID 和完整容器 ID；Engine 更换后旧详情页提示重新打开。

变更请求按操作 ID、序号和完整输入去重。发送前失败明确拒绝，发送后中断显示结果未确认；刷新不重发操作，取消本地等待不承诺撤销远端变更。停止/重启的宽限为 10 秒，命令沿用核心短任务期限和输出预算。

## 容器终端

本轮容器终端尚未发行，见[未发行记录](../../plugins/docker/CHANGELOG.md)。

需启用支持 terminal.open.v1 的终端插件，Docker 插件初始化要求核心 commands.v1。请求固定当前 Context、socket、Engine ID、完整容器 ID 和 shell；后台在提交前重读 Engine/容器状态，终端启动命令在 exec 前再次核对 Engine。命令清除 Docker 环境覆盖并显式指定 socket，不自动换 socket、容器、shell 或提权。容器已经停止、暂停、更换 Engine 或 shell 不存在时明确失败；提交后的容器状态仍可能变化，实际结果以终端输出为准。

创建期间显示等待，重复点击不提交新意图；UI 重挂载从 Docker 后台继续查询原票据。回执完成只表示终端页面已登记，不保证容器 shell 已成功启动。通信超时保留原编号继续查询；已发送但结果未知时不自动补建。

终端使用指定命令模式并关闭主机目录集成，容器 OSC 目录不会导航主机文件页。已创建终端独立于 Docker 详情页，关闭来源不结束终端；关闭终端才释放对应 PTY。命令及重开语义见[终端](terminal.md)，不是 Docker 对终端进程的直接调用。

## 有界数据与生命周期

| 内容 | 当前上限 |
| --- | --- |
| 逻辑视图与排队请求 | 64 个视图、16 个请求，worker 顺序执行 |
| 容器列表 | 1000 个、2 MiB 缓存，超限标明不完整 |
| 单容器 inspect | 输出 1 MiB，分区前 100 项，单值 4096 字节；截断明确标注 |
| 日志快照 | 合计 256 KiB，单行 4096 字节 |
| 全部视图数据 | 16 MiB |
| UI 分页 | 每页最多 20 KiB，列表 50 项、日志最多 100 行 |

断线保留已读快照并标为待刷新。UI v2 候选只读，不轮询或提交操作；交接等待在途交互，取消后恢复输入。关闭视图、退出插件或 Context 改变清理任务，旧结果不能覆盖新状态。

普通日志只记录动作、对象与结果，重要失败消息默认不弹出；不记录环境变量、inspect 或日志正文。参数、状态检查和远端操作系统权限继续生效，不引入插件授权步骤。

## 实现与回归入口

- 插件：[main.rs](../../plugins/docker/src/main.rs)、[runtime.rs](../../plugins/docker/src/runtime.rs)、[commands.rs](../../plugins/docker/src/commands.rs)、[view.js](../../plugins/docker/ui/view.js)。
- 核心契约：[插件 API](../plugin-api.md)的短命令、UI v2、设置与日志能力。
- 回归：[命令与进程](../../scripts/test-docker-process.mjs)、[UI](../../scripts/test-docker-ui.mjs)、[实际包](../../scripts/test-plugin-package.mjs)、[真实 Docker](../../scripts/test-docker.mjs)。
- 五平台客户端包验证与远端 Docker Engine 兼容是不同范围；rootless、代理路径、生产集群或全部 Engine 版本不因普通夹具通过而视为已验证。独立来源和结果见[CHANGELOG](../../plugins/docker/CHANGELOG.md)。

返回[文档索引](../README.md)。

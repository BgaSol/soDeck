# Docker 插件第一版实施与验收

记录日期：2026-11-10。组件 `sodeck.docker` 0.1.0。范围来自[第一版方案](plan-docker-plugin.md)，发行来源见[插件 CHANGELOG](../plugins/docker/CHANGELOG.md)。

## 实际实现

Docker 作为独立 Rust 进程和独立发行目录接入，未增加核心运行时接口或 Docker 业务代码。核心源码侧仅在 `tests/connections.rs` 引入真实联调夹具；根脚本和工作流增加插件检查、构建及验收入口。没有新增生产依赖，使用已有 serde、serde_json 和 plugin-client，提交独立锁文件。

| 入口 | 行为 |
| --- | --- |
| `browser` | 当前源码默认固定在右侧，以鲸鱼图标按需打开，读取 Engine 概览与容器列表，搜索名称/镜像/标签/完整 ID，筛选状态/健康并按 Compose 项目排列 |
| `container` | 在主区域按 Engine 与容器 ID 打开详情，显示配置、状态/健康、环境变量、端口、挂载、网络和标签；变量值默认隐藏 |
| `logs` | 主区域显示最近日志快照，手动刷新、搜索整个缓存、分页和复制当前页 |
| 容器操作 | 单项确认后启动、停止、重启、暂停、恢复；只删除已停止容器，保留挂载卷，不使用 force 或删卷参数 |
| 资源快照 | 按需读取 CLI 的 CPU、内存、网络、块 I/O、进程数；操作后提示重新采集，累计量不当作速率 |

三个逻辑视图使用 UI v2、`ui.openView.v1`、Surface 输入权、导航快照和统一 busy/关闭机制。候选 Surface 不轮询、不提交操作；迁移 flush 等待已有交互完成，取消后恢复输入。浏览器持续按设置刷新，详情、日志和资源指标按需读取。

连接与命令使用现有 `session.exec.v1`。插件只接收完整 Context，通过核心复用认证会话，不直接依赖 SSH 实现、终端插件或文件插件。远端只支持 Linux Docker Engine，默认明确指定 `unix:///var/run/docker.sock`，可以保存其他绝对 Unix socket。按服务器保存 endpoint；更改前要求关闭其详情/日志并等待在途请求。

## 身份、失败与资源边界

- 每次采集检查会话代次、Engine ID 和完整 64 位十六进制容器 ID。列表可以接受新 Engine，已有资源页仍绑定原 Engine 并提示重新打开。
- 统一生成 POSIX 引号，清除 Docker endpoint/context/TLS/API 环境覆盖，显式指定 socket；不自动安装 Docker、修改用户组或添加 sudo。
- 每条命令沿用核心 30 秒和输出限制，停止/重启宽限 10 秒。SDK 启动回执丢失时只以完全相同的操作 ID、序号和参数查询原启动，最多三次，不产生新变更意图。
- 请求 ID 与意图序号去重。操作前拒绝明确显示失败；发送后的中断显示结果未确认。刷新不重发操作，取消本地等待不承诺回滚远端结果。
- 每个视图保留操作结果和资源快照；断线保留已读数据并标为待刷新。视图关闭、插件退出或代次改变时清理受管命令，旧任务结果不覆盖新 Context。
- 容器操作通过核心统一日志写入动作、对象 ID 和结果；失败/结果未确认发布不弹出的重要消息，不记录环境变量、inspect 正文或日志正文。

| 配额 | 当前限制 |
| --- | --- |
| 逻辑视图 / 排队请求 | 64 / 16，单 worker 顺序执行 |
| 容器列表 | 最多 1000 个、缓存 2 MiB，每批 16 个 inspect；超限显示不完整 |
| 单容器 inspect | 命令输出 1 MiB；详情分区保留前 100 项，超过显示截断记录，单值 4096 字节 |
| 日志 | 默认 200 行，可选 50/200/500；命令合计 256 KiB，单行 4096 字节，过大显示截断 |
| 所有视图数据 | 16 MiB |
| UI 数据分页 | 每页最多 20 KiB；列表 50 项、日志最多 100 行，按实际返回偏移翻页 |
| 服务器 endpoint 配置 | `plugin-data/sodeck.docker/endpoints.json`，64 KiB |

## 右侧入口与图标修正（Docker 0.1.1 / 核心 0.20.1）

2026-11-10 的后续源码修正将推荐入口改为右侧，宿主增加标准 Docker 鲸鱼与容器图标，日志使用既有日志图标。已有布局偏好继续保留；真实核心与 Windows 插件包联调验证默认收起、左侧偏好保留，以及通过面板菜单“恢复推荐布局”将入口和已打开面板一起返回右侧。

类型/格式、正式前端构建、95 项脚本测试、211 项核心测试（13 项外部环境专项仍 ignored）、Windows x86_64 独立打包、归档内进程与退出、Docker UI 和三主题两宽度的工作台截图通过。核心首次并行测试有一项插件夹具初始化超过 5 秒，串行全量复跑全部通过。回归覆盖清单默认位置、所有 Docker 图标的实际渲染和归档内声明。

后续发布验收已在同一源码通过五平台核心/插件原生源码、独立包和实际程序启动/退出，Windows 完整 UI，以及 Linux x86_64 的隔离 Docker Engine/OpenSSH 业务。新增图标随核心 0.20.1 交付，右侧默认随 Docker 0.1.1 交付；准确的包来源、原始 run、Release 与验证范围统一记录在[核心版本说明](releases/v0.20.1.md)和[插件 CHANGELOG](../plugins/docker/CHANGELOG.md)。下节及旧发行章节保留原 0.1.0 包的历史验收。

## 本次验证

在 Windows x86_64 上使用仓库固定 Node/Rust 工具链；真实 Engine 位于本机 Docker Desktop 的隔离 Linux amd64 Docker-in-Docker 中。没有挂载或修改宿主 Docker socket。测试构建临时 OpenSSH 镜像，Alpine 主 CDN 首次访问失败，本机复测使用 `SODECK_TEST_ALPINE_MIRROR=https://mirrors.aliyun.com/alpine`；产品没有软件源依赖。

| 检查 | 结果与覆盖 |
| --- | --- |
| Docker Rust 格式与单元测试 | 通过，3 项；参数/引号、状态检查、UTF-8 与详情归一化 |
| `npm run check`、`npm run plugin:check` | 通过；核心类型/格式、五插件格式 |
| `npm run scripts:test` | 93 项通过；含 Docker 单插件检查范围与原生矩阵选择 |
| `npm run test` | 核心常规测试通过；需要外部环境的 ignored 专项不计作已通过 |
| `plugin:build -- --plugin docker` | Windows x86_64 原生程序、架构检查和独立 zip 构建通过；插件自身使用静态 MSVC CRT |
| `docker:process:test` | 实际进程通过；查询不重复执行、Compose/筛选、丢失回执、变更只执行一次、前置拒绝/中断、跨页日志搜索、endpoint/Engine 更换、冻结与任务释放 |
| `docker:ui:test` | 通过；筛选/翻页键盘入口、确认与 Esc、隐藏变量、跨页日志搜索、资源、候选/冻结恢复及迁移后滚动/导航恢复；三种主题 × 三视图 × 四宽度共 36 张截图 |
| UI 收尾复核 | 筛选后键盘入口和日志搜索两项问题修正后通过；源码 detector 无发现，共享 CSS 在打包时复制，源码扫描未包含该样式 |
| `docker:package:test` | 本机真实归档解包、资源完整性、包内进程协议与退出通过 |
| `docker:test` | 真实核心 + Windows 原生插件 + OpenSSH + Linux Docker Engine 24.0.9 通过；列表、无 HEALTHCHECK 容器、详情、日志、指标、暂停/恢复/启动/停止/重启、删除及保留卷、断线保留日志、统一关闭与退出 |

真实联调发现 `.State.Health` 对未配置 HEALTHCHECK 的容器会触发 Go template 的缺键错误；改为可选 map 读取后恢复列表，真实夹具保留此回归。界面复核发现筛选后失去行 Tab 入口、日志仅搜索当前页，均已修正并增加回归。

测试截图位于 `artifacts/docker-ui/`，真实联调报告位于 `artifacts/docker-integration/result.json`。它们是本机验收产物，不提交进发行包。

## 未完成与已知限制

五个目标的原生构建、归档进程验收已经在本轮 GitHub 打包通过，见下节。五目标原生 WebView 交互尚未专项运行；本机 Rust 核心联调和 Chromium UI 不能替代原生桌面移窗、多屏/DPI 和安装升级验收。

第一版不含容器终端、实时日志跟随、时间范围日志、文件导出、批量操作、镜像拉取与镜像/网络/卷管理、跨插件跳转或持久任务中心。日志 stdout/stderr 分路展示，不保证严格全局顺序；资源快照来自 Docker CLI 文本单位，无趋势。大规模容器 inventory 按批顺序读取，超限或容器中途消失会提示不完整。

## GitHub 原生验收与发行

包来源为 `d32b60a798f593df5a3b89d4a5807cc12d1bc546`，独立版本 0.1.0。原始 [Package plugin #42741187586](https://github.com/BgaSol/soDeck/actions/runs/42741187586) 在 Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 原生构建、校验架构、解开实际归档并运行包内程序的初始化、操作去重、查询、关闭与退出，五个目标全部通过。没有交叉编译结果替代原生执行，也没有重新打包核心。

[Check #42741108087](https://github.com/BgaSol/soDeck/actions/runs/42741108087) 的五平台核心与 Docker 等插件源码、公共检查、Linux 两架构常规业务及 x86_64 真实 Docker/OpenSSH、UI/实际包回归通过。首轮 desktop 在官方目录断言中仍预期四个插件，实际正确返回包括 Docker 的五个；`369000d` 只修正测试预期并增加 Docker 说明断言。[Check desktop #42742706703](https://github.com/BgaSol/soDeck/actions/runs/42742706703) 两次在本地添加后的焦点断言超时。

本机控制 `network_snapshot` 回执时序后稳定复现：`refresh()` 仍在读取旧列表时，添加流程调用的刷新提前返回，详情尚未出现便尝试恢复焦点；随后详情出现，焦点留在 body。`1c71367` 在新详情实际渲染且焦点空缺时恢复焦点，仅增加三行核心前端代码，并保留该时序回归。本机正式前端构建、类型/格式、93 项脚本、9 项交互及 13 组完整 production UI 通过；[Check #42745621259](https://github.com/BgaSol/soDeck/actions/runs/42745621259) 的 Windows 完整 UI、公共检查和汇总全部通过。此提交与包来源的差异只有 `src/main.ts` 和 `scripts/test-production-ui.mjs`，Docker 程序、UI、核心 API 和 Rust 运行代码均未改变，继续复用原包。焦点修正仅在 main，等待下次核心发行；当前 0.20.0 本地快速添加可能需要再次 Tab 或点击控件，官方安装和 Docker 业务不依赖该修正。

本机另从草稿下载全部五份包及更新清单，核对大小、GitHub SHA-256、五平台 URL/摘要、各包内清单，并运行下载的 Windows x86_64 进程验收通过。发布说明在收尾时补齐，更新清单只同步说明文本，五份归档保持原始字节。Release 为 [plugin-docker-v0.1.0](https://github.com/BgaSol/soDeck/releases/tag/plugin-docker-v0.1.0)，本次按用户要求标记 GitHub Latest。发行交付记录统一维护于插件 CHANGELOG；本机快照与日志保存在已忽略的 `artifacts/docker-release/`。

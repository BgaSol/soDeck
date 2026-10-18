# Docker 管理实施与验证记录

更新：2026-10-18。基线：`2fe3c62`。用户已确认实施四批 P0–P2；本轮代码位于当前工作区，未创建提交。

四批功能已接入正式 React／Tauri App。单元测试、SSH 模拟协议测试、浏览器回归和本机桌面构建已通过；真实 Docker Engine 的发布验收仍待执行，不能把模拟测试视为真实引擎验证。

## 已接入功能

| 范围 | 实现与入口 |
| --- | --- |
| 容器日常管理 | 详情和行菜单中的启动、停止、重启、删除；暂停／恢复、强制停止、重命名、重启策略；多选批量启停、重启、删除。删除不附加强制或删除卷参数，AutoRemove 单独提示并确认。 |
| 详情与排障 | 运行状态与健康状态分别显示，退出码、OOM、重启次数、时间、端口、网络、挂载；可见详情按需采样 CPU／内存／累计 I/O／进程数，另有进程列表和显式读取配置。敏感配置默认隐藏，离开后清空。 |
| 镜像 | 拉取、Tag、移除引用／删除实体、平台与大小、包含停止容器的引用关系、从镜像创建容器、导入／导出。操作按内容 ID 和具体引用核对。 |
| 工作区联动 | 发布端口预填现有转发编辑器；已有草稿关闭后再接收新请求，打开请求不保存或启动转发。核对为同一宿主的 Bind Mount 可定位文件面板。 |
| Compose | 通过容器标签发现项目；关联远端目录、有序文件、env 文件、profiles；启动已有服务、停止、重启、拉取镜像、应用配置、Down；服务详情／日志／配置编辑入口。关联单独保存在 SQLite，Down 后仍保留。 |
| 创建容器 | 从本地镜像 ID 创建，名称、端口、环境变量、Bind／已有卷、网络、重启策略、命令参数和工作目录；先记录创建 ID，再启动，启动失败保留容器。 |
| 空间与资源 | 引擎更多菜单 → 空间与资源：用量、卷／网络列表、详情、引用、创建／删除、网络连接／断开。分类清理仅处理选中对象，数据卷默认排除；构建缓存单独确认。 |
| 文件与离线迁移 | 镜像 save／load、容器单文件上传、文件／目录 tar 导出；原生本机文件选择，独立二进制 SSH 通道，显示于现有文件任务面板，支持中止和结果核对。 |
| 后台任务 | App 级执行、同请求去重、对象／引擎互斥、独立已验证 SSH 连接、跨工作区订阅、退出收尾；结果不确定时只读核对，明确解除锁定后才允许新的管理操作。当前连接的更早记录可分页读取。 |

## 界面与架构

继续使用右侧“监控 / Docker”和“容器 / 镜像 / 项目”，卷、网络与清理放入“空间与资源”。沿用主题变量、系统字体、Lucide、菜单和对话框；新增样式位于 `src/docker/docker.css`。原型的演示工具没有进入产品。

检查了 Chromium 与 WebKit 下的 320／360／480px 侧栏、明暗主题、减少动画偏好和创建表单。截图保存在忽略目录 `.qa/docker-{container,image,compose,resources}-<width>-<theme>-<browser>.png`。修复 WebKit 虚拟列表使用旧视口高度时 End 键焦点丢失的问题，并验证列表返回、搜索、排序与滚动恢复。

原 `src-tauri/src/docker.rs` 拆为 `src-tauri/src/docker/`，保留查询／日志／终端调用的兼容导出。管理命令均为类型化 IPC；前端不能提交任意宿主 shell。监控继续共享列表与引擎绑定，管理任务使用独立连接，完成后定向刷新。

任务绑定已验证连接、Endpoint、可得 daemon ID、完整对象 ID；执行前复核目标和预览指纹。同一 App 最多同时执行四个管理任务；单容器动作按容器互斥，其余管理动作首版采用同引擎独占。不会自动 sudo，也不会自动重放结果未知的写命令。

创建环境变量与 Compose 解析配置使用远端独占创建的 0600 临时文件，命令结束时清理。配置值不进入共享任务、迁移快照或 SQLite 关联；异常清理会提示临时路径。二进制传输分别读取 stdout／stderr，要求完成回执；下载只有完整成功才发布目标文件，来源或目标变化会拒绝确认。

## 首版边界与方案调整

- Docker 管理及传输记录仅存在于本次 App 进程内，不支持跨重启恢复或自动续传；Compose 关联持久化。实时帧包含最近 200 条及所有阻塞任务，历史每页 100 条，本次运行最多保留 2000 条。跨服务器别名不合并锁。
- Compose 应用要求支持结构化 config 与固定镜像 digest，拒绝带 build 的部署，镜像须预先拉取；应用时不隐式拉取或构建。重启不会应用配置，拉取与应用分别确认。预览展示服务镜像、现有容器、项目网络及保留范围；结果阶段与当前服务状态分别展示，不推断每个服务的因果成功。首版没有多服务混合日志或逐层部署百分比。
- Compose 发现使用容器标签加本地关联，不扫描服务器目录。配置预检仍受现有有界查询超时约束；远端仓库 digest 查询较慢时会在写入前报错，需要稍后重试。关联文件丢失时，执行须先修复关联。
- 配置／创建草稿不跨窗口迁移；离开后重新填写。不会把包含敏感值的草稿写入窗口快照。
- 创建入口设于镜像详情，先明确选定本地镜像。卷／网络创建首版固定为 local／bridge，未增加高级驱动配置。健康检查输出在完整 inspect 的显式读取中查看，不向共享详情广播。
- Bind 文件与端口联动要求 Unix Endpoint 且引擎名称与 SSH 宿主名匹配；无法确认时只提供说明和手动复制。继承现有 POSIX／Docker CLI 边界，context 专用 TLS 和 SSH transport 不承诺完整支持。
- 容器上传支持一个普通本机文件、文件名 UTF-8 且不超过 100 字节、文件小于 8 GiB，目标为容器已有目录。目录下载导出为 tar，本机不自动解包；当前没有目录递归上传或容器文件浏览器。上传同名文件可能覆盖，确认框明确说明。
- 清理逐项反馈，不提供事务或回滚。共享镜像层不累加承诺释放空间；删除后刷新引擎用量。构建缓存由引擎执行时选择候选，单独说明，不能精确预列全部缓存对象。
- build／push、Registry 账号管理、任意容器克隆重建、事件告警、多 context 管理、Swarm／Kubernetes 保持在本轮范围之外。

## 已执行验证

| 验证 | 结果 |
| --- | --- |
| `npm test` | 13 个文件，102 项通过。 |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib` | 119 项通过，36 项需要专用环境的测试默认忽略。 |
| `cargo check --manifest-path src-tauri/Cargo.toml --tests` | 通过，含隔离 Engine 测试的编译检查。 |
| `jump_docker_management_tasks_and_uncertain_results --ignored` | 真实 SSH／SFTP 和跳板连接、Docker 命令模拟；通过。覆盖长停止、窗口同步、去重／锁、Tag 移动、2 MiB 二进制双流、缺失回执、环境文件权限／清理、历史分页、只读核对和关闭工作区后继续执行。 |
| `npm run test:docker` | Chromium／WebKit 共 22 项通过，包含功能交互和侧栏／主题布局。 |
| `npm run test:resources -- --grep 'Docker\|container operations'` | 12 项通过；另对 WebKit 键盘及滚动用例重复三轮，6 项通过。 |
| 端口转发定向回归 | 服务器请求与 Docker 预填草稿，共 4 项通过。 |
| `npm run tauri -- build --no-bundle` | 前端生产构建与本机 macOS Release 构建通过，产物 `src-tauri/target/release/sodeck`。 |
| `git diff --check` 与新增 Docker Rust 模块格式检查 | 通过。 |

全仓 `cargo fmt --check` 仍有既有文件格式差异，例如基线 `connection_tests.rs` 的长行，以及其他既有模块；未为本任务重排整个仓库。新增 Docker 模块已独立格式化，修改的 Rust 集成片段已整理。前端构建仍有大于 500 kB 的 chunk 提示，不影响构建成功。

## 待真实环境验收

本机没有可用 Docker Engine，未连接用户业务服务器执行变更。没有把本地模拟结果标记成真实 Engine 或完整桌面端到端验收。

已增加 `tests/docker-engine/` 与 `.github/workflows/check.yml` 的独立 Linux 作业。测试启动带 OpenSSH 的专用嵌套 Engine，不挂载日常宿主 Docker socket；测试对象位于该 Engine，清理仅删除记录的测试容器。集成测试覆盖真实镜像、创建与环境值、慢停止、AutoRemove、Tag 引用、卷／网络、Compose 应用／Down 保留卷、二进制传输和启动失败保留对象。该作业已配置但本轮未运行，Docker 28 系列及实际 Compose 版本的兼容性仍需其执行结果确认。

在有 Docker 的独立测试机器运行：

```sh
docker build -t sodeck-docker-engine-fixture tests/docker-engine
python3 tests/docker-engine/environment.py
cargo test --manifest-path src-tauri/Cargo.toml --lib docker_engine_management_contract -- --ignored --nocapture --test-threads=1
python3 tests/docker-engine/environment.py stop
```

还需在原生窗口检查文件选择器、工作区分离／迁移、退出时远端写入结果不确定的提示；Windows／Linux 构建及真实引擎矩阵以 CI／专用测试环境为准。

## 2026-10-18 详情查询兼容性修复

用户报告镜像详情出现 `map has no entry for key "Variant"`，容器详情也报错。镜像 `Variant`、容器 `State.Health` 和 `Config.StopTimeout` 可以被 Engine 省略；原模板直接访问这些字段，Docker CLI 回退到原始 JSON 并启用 `missingkey=error` 后失败，`if .State.Health` 也无法保护缺失的键。现改用 `index` 读取可选字段，缺失时输出 `null`；容器健康信息仍只投影状态与失败次数，完整配置和健康日志不会进入共享详情。

新增 `tests/docker_inspect_cli.py` 和 `docker_inspect_templates_with_real_cli` 回归测试：由本机 HTTP fixture 提供 Engine 返回，真实 Docker CLI 执行生产模板，再交给 Rust 解析。已用 Docker CLI 28.5.1 复现旧模板的 `Variant`／`Health` 错误；修复后通过 4 个镜像用例、5 个容器用例和 2 个必需字段缺失用例，覆盖缺失／null／空 Variant、ARM variant、三种健康状态、默认／零／负值／指定停止超时及敏感字段排除。此测试已接入 Docker CI 作业；它验证真实 CLI 模板行为，不等同于真实 Engine 验收。

本轮 Docker 定向 Rust 测试 13 项通过；CLI 回归和真实 SSH＋模拟 Docker 的管理流程回归分别通过。可在有 Docker CLI（无需运行 Engine）和 Python 的环境执行：

```sh
cargo test --manifest-path src-tauri/Cargo.toml --lib docker_inspect_templates_with_real_cli -- --ignored --nocapture
```

`DOCKER_TEST_CLI` 可指定 Docker CLI 路径，`SSH_TEST_PYTHON` 可指定 Python。测试固定使用临时配置和本机随机端口，不连接当前 Docker context。

本轮 `npm run tauri -- build --debug --no-bundle` 已通过；通过 Tauri bundler 在 `.qa/docker-inspect-fix/debug/bundle/macos/SoDeck.app` 生成独立修复版，完成本机 ad-hoc 签名与校验，并检查包内确实包含修复后的两个查询模板。当前运行的旧 App 保持原样，退出后打开此修复版才能使用新后端。

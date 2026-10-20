# 容器日志改造交付记录

日期：2026-10-20。依据已确认的功能规划、代码方案和 apple-design 技能实施。代码已完成，本地可执行检查已通过；真实 Docker Engine 验收仍需 CI 环境运行。

## 用户可见行为

- 顶部显示采集状态，底部提供唯一的暂停／继续跟随主操作。暂停阅读不停止后台接收和本地保存。
- 同一容器停止后持续等待，启动后重新接续；连接异常自动退避重试，可立即重试。恢复连接保持原有阅读意图：跟随模式继续显示最新输出，暂停模式保留阅读位置。
- 向上翻阅进入浏览暂停；回到底部只自动恢复浏览暂停。手动暂停和选区暂停需要明确继续。普通点击文本不会意外暂停。
- 查找默认建立稳定快照；“实时筛选”显式开启。折叠搜索框后保留条件标签。可从命中记录进入上下文并返回原查询位置。
- 查询建立失败或取消时保留上次可见结果，明确标示当前条件未应用，阻止按新条件复制／导出。可重试查询或继续跟随。
- 导出区分全部已保存、当前筛选结果和选中记录，提供进度与取消；按底层原始字节导出，不以界面高亮或当前已加载页代替全部结果。
- 显示选项包含时间戳、换行、高亮、字号，偏好跨窗口重开保留。支持按序号选择、复制与导出。
- 运行事件独立呈现，不计入应用日志序号或复制／导出正文。启动周期只使用已观测信息，结束边界未知的旧周期不作为完整范围提供。
- 已删除容器保留本地记录；可显式查找同一 daemon、Compose project/service/container-number 的替代对象，验证后打开其独立日志来源，不混入旧存档。
- 关闭主 SSH 会话不关闭独立日志采集；关闭日志窗口停止该窗口采集，已保存内容仍可离线读取。

## apple-design 落地

使用现有主题、窗口标题栏、图标和原生滚动，控制面板按需展开。采集状态与阅读状态各有固定位置，主操作文字直接解释后果。交互反馈即时，不添加持续闪烁或强制平滑滚动。保留键盘焦点、Escape 返回触发按钮、减少动态效果、减少透明度与高对比度样式，并验证 650×400 窗口及 200% 文字下的主操作可达性。

## 代码分工

| 路径 | 职责 |
| --- | --- |
| `src/ContainerLogsApp.tsx` | 日志窗口组合、操作及状态反馈 |
| `src/logs/model.ts`、`useLogSession.ts`、`useLogQuery.ts` | 阅读状态、采集订阅、分页／快照／上下文与迟到响应隔离 |
| `src/logs/LogToolbar.tsx`、`LogViewer.tsx`、`LogReadingBar.tsx` | 工具栏、虚拟日志视图和底部主操作 |
| `src/logs/preferences.ts`、`src/logs.css`、`src/logApi.ts` | 显示偏好、样式与 IPC 数据契约 |
| `src-tauri/src/logs.rs` | 窗口归属、任务／快照管理与命令注册 |
| `src-tauri/src/logs/collector.rs`、`protocol.rs`、`diagnostics.rs` | 采集恢复、状态／错误分类及 CLI 诊断分离 |
| `src-tauri/src/logs/lifecycle.rs`、`replacements.rs` | 辅助生命周期库、启动边界和替代容器校验 |
| `src-tauri/src/logs/snapshot.rs`、`export.rs` | 冻结可变消息、缓存租约、原文字节导出及取消 |
| `src-tauri/src/log_ingest.rs`、`log_store.rs` | 提交协调、旧记录更新通知、重放去重与分片查询 |

保留原始存档格式版本 3、原始序号和旧版本兼容读取，生命周期使用独立辅助数据库。未添加依赖或修改锁文件。

## 实际验证结果

| 检查 | 结果 |
| --- | --- |
| `npm test` | 23 个文件、151 项通过 |
| `npm run test:logs` | 15 项通过，包含实时窗口、存档窗口、查询失败保护、紧凑布局及大字号／高对比度 |
| `npm run build` | TypeScript 与生产构建通过；仍有现有主 bundle 超过 500 kB 的提示 |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib` | 151 项通过、40 项显式 ignored |
| `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` | 通过 |
| `npm run test:desktop -- --grep "container logs"` | 最终重新编译的原生 Tauri 应用，2 项通过 |

原生验证使用本任务专用应用 ID、数据目录、WebView 调试端口和 SSH fixture。连接与 SSH 传输是真实的，Docker CLI 输出由夹具模拟。覆盖全历史同步、重开去重、网络恢复、停止到启动、暂停位置保持、驱动失败诊断分离、主会话关闭后继续采集与离线存档。

Rust 新增回归覆盖可变尾部快照、原文字节与取消导出、缓存活跃租约保护和孤儿清理、缺失周期不推断、诊断分块处理，以及在重放时间戳中途取消不生成假记录。界面截图位于本地 `.qa/container-logs-redesign-compact.png` 和 `.qa/container-logs-redesign-accessible.png`。

## 真实 Engine 验收与边界

新增 ignored 测试 `logs::engine_tests::docker_engine_log_lifecycle_contract`，接入现有隔离 nested Engine CI job。包含 TTY／非 TTY、restart、stop/start、进程非零退出触发自动重启、删除后替代对象发现、目标缺失和不支持读取的日志驱动。自动重启通过容器内部退出触发，避免把管理命令停止误作崩溃。

本机无可用 Docker Engine，因此上述真实 Engine 测试只完成编译，尚未执行；CI 配置已修改但本任务未提交或触发远程 CI。专用 daemon 断电／恢复和真实 Engine 与原生 UI 同时运行的组合验证仍未执行，不能据此声明完整真实生命周期验收通过。

其他明确边界：

- 低频 inspect 只能记录实际观测的周期，不保证捕获快速连续崩溃的每一次启动。不补造未知周期；已选择周期后来失去结束边界时改为明确的起始时间筛选并提示。
- 快照只冻结可变消息，冻结数据预算为 128 MiB；旧格式必要时复制的部分也受预算限制。超过预算明确失败，保留上次可见结果而不暗中改用实时查询。界面缓存限制为 5000 行／16 MiB。
- CLI stderr 分离是保守分类：可确认诊断与应用记录分开，无法确认的字节继续保留。未迁移为 Docker Engine API 的完整结构化错误通道。
- 关闭窗口后没有后台采集服务；不会自动跨容器 ID 合并来源；本轮未实施后台分批补全海量远端历史的 P2 调度。
- 未建立新的负载基准，百万行专项 ignored 测试本次没有运行。

这些记录补充原规划的实施前分析；原规划中的“当前实现”和“计划命令”应按其编写时点理解。

# 文件与网络结果确认：剩余验收

状态：代码已落地，尚未正式发行；以下验收未全部完成，不能将整个任务标为完成。核对基线为 `8115fdcd5bed4108021a729da605fbde77afc2af`。工作区既有文档整理继续保留。

已实现的行为和接口不再在计划中重复维护：

- 用户行为与降级：[文件](../features/files.md)、[网络隧道](../features/network-proxy.md)。
- 关闭与交接：[生命周期](../features/lifecycle.md)。
- 请求字段、状态、能力与兼容：[插件 API](../plugin-api.md)。
- 核心与插件职责：[架构](../architecture.md)。
- 待发行变化：[文件 CHANGELOG](../../plugins/files/CHANGELOG.md)、[网络 CHANGELOG](../../plugins/network-proxy/CHANGELOG.md)。

本轮仍坚持的边界：当前后端不能保证无覆盖移动，因此移动、剪切和改名关闭；复制不接续删源。保存和明确覆盖只面向没有其他写入者的普通文件操作，不提供原子版本比较。未知结果保留原身份并收窄后续动作，不自动重放或回滚。不新增生产依赖、通用事务框架、插件授权或发布状态机。

## 剩余验收

| 范围 | 仍需补充的证据 | 通过条件 |
| --- | --- | --- |
| 文件接纳 | 核心 128 槽位/单资源 16 项满额、Tasks 封闭与并发重复请求的受控交错 | 无幽灵任务，不提前推进序号；相同请求最多一个 worker，活跃回执不被回收 |
| 文件结果记录 | 在真实插件进程中注入 journal 部分写入、sync 失败与恢复 | 落盘前不释放回执；恢复只记一次，不重发变更；取消不虚报已持久化 |
| 保存、复制与关闭 | 完整核心/插件链路中，在提交已发送处设屏障，期间取消、断线、关闭或切换 Context；复制提交回包丢失 | 等待原提交结果；unknown 阻止保存并关闭；不清理未决提交的临时文件，旧结果不清除新输入或更新新连接基线 |
| 草稿与交接 | 系统 IME、必要位置通知失败、取消交接后迟到 flush，以及旧 Surface 分片迟到的组合 | 失败恢复源输入；候选只读；旧身份不能完成新交接或覆盖新正文 |
| 网络核心 | 真实 create 先取消后到达、活跃超过旧 60 秒期限、并发重复请求和旧 Context 迟到响应 | 只有一个监听；旧意图不能重建；清理证据归原 Run，不漏 socket/子任务 |
| 原生导出 | 实际系统保存对话框超过 45 秒、重复请求、取消与关闭组合 | 不重复弹框；迟到路径不写；对话框未退出时不宣布清理完成 |
| 其余四个平台 | Windows ARM64、Linux x86_64/ARM64、macOS ARM64 的对应新源码原生包；Windows x86_64 本地包已验收 | 各组件独立编译、打包、实际包启动/退出；ARM64 使用原生产物，历史版本结果不替代本轮 |

未来只有后端确实提供无覆盖原生 rename，才另行恢复移动。当前 unsupported 分支不以增加复制/删除测试为理由恢复危险功能，也不把未来原生移动用例算作本轮已提供能力。

## 本轮验证位置

已有自动回归保留在代码中，后续沿用这些入口补齐，不新增另一套实施记录：

- [正文同步](../../scripts/file-draft-sync.test.mjs)和[文件 UI](../../scripts/test-files-ui.mjs)：分片失败重试、提交回包丢失、元数据故障、保存/导出并行观察、关闭保存、跨窗正文、BOM/CRLF、离线导出。
- [文件真实进程](../../scripts/test-files-process.mjs)：丢失变更/释放回执、原身份确认、禁止移动回退。
- [文件 OpenSSH](../../src-tauri/tests/fixtures/files_flow.rs)：保存 R 时输入 R+1、重复保存、导出固定 R、取消后迟到路径、旧写入口拒绝、外部变化拒绝。
- [受控 SFTP 提交](../../src-tauri/src/remote_files/save_tests.rs)：真实 RawSftpSession 收到成功 ACK 后 stat 失败、提交回包丢失、明确拒绝；核对协议后续请求，不依赖随机 sleep。它只覆盖提交边界，不替代上表完整关闭/切换链路。
- [网络真实进程](../../scripts/test-network-process.mjs)、[网络夹具](../../src-tauri/tests/fixtures/network_flow.rs)和[网络 UI](../../scripts/test-network-ui.mjs)：创建/查询/释放回包失败、RPC 不占 App 锁、延迟清理、端口重绑、删除写盘失败、TCP/SOCKS5。
- 核心 editor/network 模块状态测试、[关闭](../../src-tauri/tests/close_lifecycle.rs)和[组件交接](../../src-tauri/tests/component_ui.rs)继续保留，不能以其一般状态测试代替上表缺失场景。

本地常规检查：`npm run check`、`npm run plugin:check`、`npm run scripts:test`、`npm run test`，以及两个受影响插件的 Cargo 测试。业务与产物使用既有 `npm run files:test`、`npm run files:editor:test`、`npm run network:test`、`npm run build` 和独立 `plugin:build`。新进程测试已接入业务入口，新 `.test.mjs` 自动纳入脚本测试；未映射路径由现有选择器保守全选。

Windows 实际包补充 [原生编辑器](../../scripts/test-file-editor-native.mjs)与[原生关闭](../../scripts/test-close-native.mjs)，使用隔离 SSH/应用数据。五平台及发布流程仍由[分发文档](../distribution.md)定义；本地包沿用当前版本号用于测试，不是新的正式发行，不能覆盖已发布同版本附件。

2026-11-11 本地 Windows x86_64 已完成核心与两个受影响插件独立打包，并从 MSI 解出实际程序验证启动、持久化重开、SFTP 编辑器保存、跨窗交接、取消关闭保留草稿/转发，以及明确放弃后四个插件退出与端口释放。终端 0.7.1、系统状态 0.4.0 仅作为此次组合验收的既有辅助包；不是这两个组件重新发行的证据。该结果不替代上表故障矩阵、其他架构或实际安装器升级验收。

## 收束条件

补齐上表证据；发现失败则修对应模块并运行相关回归。正式发行时分别确定核心与插件新版本，核心先提供新能力、插件再声明依赖，缺能力明确拒绝。发行证据写对应版本说明或 CHANGELOG。所有约定验收和正式文档同步完成后，删除本计划；不另建完成归档。

# 在线升级实现记录

已实现应用内更新代码与签名发布链路，0.1.2 作为首个带 updater 的基线版本，发布结果以 GitHub Release 为准。

## 用户体验

- 关于窗口提供更新入口，品牌旁提示有可用版本；系统设置最前方是“软件更新”。
- 后端在正式构建启动 30 秒后按 24 小时周期检查稳定版，失败按 15 分钟 / 1 小时 / 6 小时退避。开发构建仅允许手动检查。
- 可关闭自动检查、跳过当前版本、查看完整版本说明和打开该版本发布页。
- 手动下载，所有窗口共享同一任务；关闭设置面板不停止下载。签名验证后才允许安装。
- 安装前先显示影响，再经用户确认；后端在锁住新工作后复核。所有远程编辑窗口需保存关闭，进行中的文件任务、结果待核对的写操作、Docker / NetBird 管理操作会阻止安装。
- 会话、日志和端口转发中断会被明确列出；普通退出与升级安装共用资源清理。
- 下次启动核对实际运行版本，只报告一次更新结果。不承诺自动恢复远端进程、文件传输和端口转发。

## 与原计划的具体取舍

1. 只引入 Rust updater 依赖，使用 Rust AppHandle.restart；不新增前端 updater/process 插件或其 IPC 权限。前端仅调用受限自定义命令，安装只允许主窗口发起。
2. 更新偏好和一次性结果保存在 app_config_dir/update-state.json，独立于已有 settings.json，避免自动检查时间写入与其他设置的并发保存冲突。不改变服务器数据库。
3. Tauri 当前原生升级格式直接复用 Windows EXE 与 Linux AppImage，不额外做旧版兼容 ZIP；macOS 使用 app.tar.gz。
4. 编辑器要求全部保存并关闭，避免异步 dirty 标记与确认操作之间的竞态。文件操作正在收尾时也先阻止安装。
5. 第一版不提供暂停、断点续传或跨进程下载缓存；离开设置仍下载，退出应用则需重新下载。
6. MSI、DEB、开发构建或未知安装方式仅提供手动下载安装，不能通过前端命令绕过限制。

## 主要代码

- src-tauri/src/updates.rs：检查调度、单任务状态、原子保存、下载校验、安装检查、重启和结果确认。
- src-tauri/src/lib.rs：初始化管理器、注册命令、抽取 shutdown_work，共用退出清理并防止退出与升级并发。
- src-tauri/src/files.rs、file_tasks/mod.rs、netbird/manager.rs、netbird/store.rs：提供安装阻止条件。
- src/updateApi.ts、src/useAppUpdate.ts：类型接口与按 revision 同步的全局订阅。
- src/SoftwareUpdateSection.tsx、src/AppBrand.tsx、src/SystemSettingsDialog.tsx：版本、下载、设置与安装确认。
- scripts/update-assets.mjs：收集升级产物、验证 Minisign 签名与版本绑定、生成静态更新清单。
- scripts/release-assets.mjs、scripts/publish-release.mjs：合并现有安装包与升级产物，严格核验后公开草稿；稳定版全局串行且只前进。

## 本地验证

- 前端 TypeScript/Vite 构建通过。
- 既有前端单元测试：26 个测试文件、171 项通过。
- 发布脚本测试：10 项通过，覆盖上传失败保持草稿、签名错误、缺少签名、版本绑定、固定 tag URL 与旧版不能替换 latest。
- 更新界面 Playwright：3 项通过，覆盖关闭设置后继续下载、编辑器阻止安装、二次确认会话和端口转发中断、网络失败、跳过版本和手动安装入口。
- Rust 测试：159 项通过，40 项依赖外部隔离环境的测试按原配置跳过。覆盖语义版本、可信 URL、设置持久化失败、编辑/文件操作阻止安装以及停止新任务；既有后端测试也运行。
- Windows 已实际构建 NSIS EXE、MSI 及对应签名，并使用正式收集脚本验证 EXE 的签名及签名版本。

截图位于 .qa/updates-confirm.png。最初的本地安装包验证使用 0.1.1，发布前已将应用版本统一为 0.1.2；本地验证包不得覆盖 GitHub 已发布版本。

## 尚待正式上线验证

- 发布者已确认配置 GitHub Actions Secret `TAURI_SIGNING_PRIVATE_KEY`，实际签名及公钥匹配由发布构建校验。私钥保存在仓库外，需另做安全备份；GitHub 不支持从设置页取回 Secret 原文。
- macOS 两种架构及 Linux AppImage 的构建与实际安装升级需要对应系统验证；本机为 Windows。
- 尚未执行已安装旧版 → GitHub 新版 → 安装 → 重启的完整外部升级；未自动安装覆盖本机现有应用。
- 0.1.1 用户需手动安装 0.1.2 基线版本并验证用户数据保留，再用后续版本验证升级闭环。
- 保留系统签名/公证现状；updater 签名不能消除 Windows/macOS 的平台发布者提示。

发布步骤见 [版本发布说明](releases.md)。

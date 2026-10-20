# SoDeck 在线升级实现计划

日期：2026-10-20
状态：已确认并进入实现；实际实现与验证结果见 [在线升级实现记录](online-update-implementation.md)
关联规划：[在线升级功能规划](online-update-feature-plan.md)

## 1. 实现目标

把现有“GitHub Release 手动下载”的发布方式接入 App 内升级。第一期完成以下闭环：

1. GitHub Actions 为正式版本生成经过签名的 Tauri updater 产物和 `latest.json`。
2. SoDeck 启动后和用户手动操作时检查稳定版更新。
3. 用户查看版本说明并在后台下载更新。
4. 安装前检查未保存文件、任务、会话和端口转发，给出明确处理结果。
5. 用户确认后关闭活动工作、安装更新并重启。
6. 下次启动确认新版本已实际运行，并保留已有用户数据。

第一期采用一个统一升级通道：只接受正式 Release、只安装高于当前版本的版本、只接受固定 GitHub Release 更新清单和通过签名验证的包。

## 2. 先确定的范围

为了避免一次把所有安装方式都混在一起，按以下顺序交付：

- **完整自动升级首批目标**：Windows NSIS EXE、macOS 应用升级包、Linux AppImage。
- **检查更新和手动下载安装**：Windows MSI、Linux DEB，以及无法识别安装来源的情况。
- **不做**：强制升级、预发布频道、差分包、断点续传、自动降级、升级后自动恢复 SSH/传输/端口转发。

现有发布包仍继续上传，自动升级产物与人工下载产物并行存在。这样即使某个平台的安装器升级验证尚未完成，也不会影响 GitHub Release 的正常下载。

## 3. 代码结构与职责

### 3.1 Rust 后端：更新管理和安装准备

新增 `src-tauri/src/updates.rs`，负责：

- 当前版本、平台、架构和安装来源识别。
- 使用 Tauri updater 检查更新、解析版本和更新说明。
- 管理单个全局更新任务，防止多个窗口重复检查、下载或安装。
- 将状态通过 Tauri 事件广播给所有窗口。
- 维护下载进度、错误类别、目标版本和更新时间。
- 记录“准备升级 / 已交给安装器 / 启动后确认”的状态。
- 对下载包执行 updater 的签名校验，校验失败不得安装。
- 只允许主窗口执行安装准备和最后的安装动作。

在 `src-tauri/src/lib.rs` 中：

- 注册 updater 插件。
- 注册 process 插件，用于更新完成后的重启。
- 初始化 `UpdateManager`，加入现有 `State` 或独立 managed state。
- 注册更新相关 command，例如：
  - `update_snapshot`：读取当前更新状态。
  - `update_check`：手动检查。
  - `update_download`：开始或继续下载。
  - `update_cancel_download`：取消当前下载。
  - `update_prepare_install`：执行安装前检查和关闭准备。
  - `update_install`：安装已验证的包并重启。
  - `update_mark_result`：启动后确认升级结果。

不让前端直接接收任意 URL、任意文件路径或自行执行安装器。前端只提交动作，Rust 根据编译配置和更新清单决定目标。

### 3.2 退出和工作保护

当前 `src/App.tsx` 的关闭处理已经会检查终端、日志、未保存文件、Docker 操作、文件任务和端口转发；`src-tauri/src/lib.rs` 的 `quit_app` 负责真正停止后台资源和退出进程。

实现时抽取一个共享的后端关闭准备层，建议形成以下结构：

- `collect_shutdown_status()`：返回终端、日志、任务、Docker、端口转发、未保存文件等状态。
- `prepare_shutdown(mode)`：按普通退出或升级安装模式执行停止、等待、落盘和窗口关闭。
- `quit_app()`：调用普通退出模式，最终 `app.exit(0)`。
- `update_prepare_install()`：调用升级模式，完成准备后保留安装阶段所需的控制权。

前端仍负责编辑器等界面状态的最后确认，因为部分未保存状态在 React 工作区中维护。安装准备流程如下：

1. 前端请求最新工作状态。
2. 有未保存文件时定位到编辑器并停止流程。
3. 有需要用户处理的任务或结果时停止流程并提供入口。
4. 用户确认会中断活动会话、日志、端口转发后，设置 `installPreparing` 全局锁。
5. 锁定期间禁止新建连接、启动任务和新建编辑。
6. Rust 再次核验状态，停止后台工作、等待可等待任务、落盘数据库和设置。
7. 状态清理完成后才调用 updater 安装。
8. 任何一步失败都解除锁定，不执行安装。

不能把现有 `quit_app` 直接调用后再安装，因为它会立即退出当前进程；需要把“清理”和“最终退出”拆开。

### 3.3 前端：统一更新状态

新增：

- `src/updateApi.ts`：Tauri command 和事件的类型化封装。
- `src/useAppUpdate.ts`：订阅状态、启动检查、手动检查、下载、安装和错误处理。
- `src/SoftwareUpdateSection.tsx`：系统设置中的软件更新区域。
- `src/software-update.css`：更新状态、进度和阻塞提示样式。

修改：

- `src/AppBrand.tsx`：关于窗口展示当前版本、更新标记和“检查更新 / 查看更新”。
- `src/SystemSettingsDialog.tsx`：增加“软件更新”区，支持自动检查、手动检查、下载、稍后、跳过版本和安装重启。
- `src/App.tsx`：接入主窗口级安装协调、全局安装锁和主窗口聚焦；其他窗口只显示状态，不重复弹安装确认。
- 需要创建新连接或启动后台任务的入口：读取更新锁，安装准备中时拒绝新工作并显示原因。
- `src/brand.ts`：继续作为 UI 版本展示的来源，但发布检查必须以 Tauri 运行时版本为准，避免前端版本显示与打包版本漂移。

更新状态使用明确的状态机：

```text
idle
  -> checking
  -> upToDate
  -> available
  -> downloading
  -> ready
  -> preparingInstall
  -> installing
  -> awaitingRestartConfirmation
  -> completed / failed / blocked
```

状态机不允许并发进入两个检查、两个下载或两个安装。`ready` 状态可以在设置窗口关闭后保持，重新打开仍能继续安装。

### 3.4 设置与持久化

修改 `src-tauri/src/system_settings.rs` 的设置结构，增加向后兼容字段：

```json
{
  "update": {
    "autoCheck": true,
    "skippedVersion": null,
    "lastCheckAt": null
  }
}
```

只持久化用户偏好和检查时间，不把下载进度高频写进 `settings.json`。升级过程状态单独保存到应用数据目录，例如 `update-state.json`，用于启动后判断：

- 上次是否准备过升级。
- 目标版本是什么。
- 上次是否交给安装器。
- 当前启动版本是否已经达到目标版本。
- 是否应该展示“升级完成”或“上次升级未完成”。

设置文件旧版本缺少 `update` 字段时使用默认值；不能因为新增字段触发数据库重建或清空配置。

## 4. Tauri 配置修改

### 4.1 依赖

修改 `package.json` 和 `package-lock.json`：

- `@tauri-apps/plugin-updater`
- `@tauri-apps/plugin-process`

修改 `src-tauri/Cargo.toml` 和 `src-tauri/Cargo.lock`：

- `tauri-plugin-updater`
- `tauri-plugin-process`

版本跟随当前 Tauri 2 依赖线，安装后运行 Rust 和前端现有测试。

### 4.2 `tauri.conf.json`

在 `src-tauri/tauri.conf.json` 增加：

- `bundle.createUpdaterArtifacts: true`。
- `plugins.updater.pubkey`：只放公钥，不放私钥。
- `plugins.updater.endpoints`：指向固定仓库的稳定 `latest.json`。
- Windows updater 安装模式先使用 `passive`，实际安装权限和用户级/机器级安装要在真实 Windows 包上验证。

公钥可以提交到仓库；私钥只放 GitHub Actions Secret。生产 endpoint 继续使用 HTTPS，开发环境可关闭自动更新或使用本地 mock，不打开不安全传输配置。

### 4.3 Capabilities

修改 `src-tauri/capabilities/desktop.json`：

- 增加 updater 的最小需要权限。
- 增加 process relaunch 所需权限。
- 保持现有窗口范围和已有关闭权限不变。

权限配置完成后，用开发构建验证：没有多余窗口权限，普通前端无法绕过 Rust 的安装准备流程。

## 5. 发布链路修改

当前发布链路由 `.github/workflows/release.yml`、`.github/workflows/check.yml`、`scripts/release-assets.mjs` 和 `scripts/publish-release.mjs` 共同控制，且会严格检查附件列表。因此不能只打开 Tauri 配置，还要同步修改附件收集和发布校验。

### 5.1 产物

每个支持自动升级的平台需要生成 Tauri updater 所需的升级归档和 `.sig`：

- Windows NSIS updater 归档及签名。
- macOS `.app.tar.gz` 及签名。
- Linux AppImage updater 产物及签名。

原有 EXE、MSI、DMG、DEB、AppImage 和 `SHA256SUMS.txt` 继续保留。升级包和签名文件要有明确命名规则，脚本不允许把普通安装器误当成 updater 产物。

### 5.2 `latest.json`

新增发布脚本逻辑，生成包含以下信息的静态清单：

- `version`。
- `notes`。
- `pub_date`。
- 每个支持目标的 `url`。
- 对应的签名内容。

清单 URL 指向具体 Release tag 的附件，避免用户下载过程中“latest”指向另一个版本。对于当前暂不支持自动安装的 MSI 和 DEB，不写入自动更新目标，App 仍可以把 GitHub Release 页面作为手动下载入口。

现有矩阵构建是并行的，`latest.json` 必须在所有平台构建附件汇总完成后由单一 publish job 生成和上传，避免多个 job 同时覆盖同一个 Release 附件。官方 Tauri Action 也提供 `uploadUpdaterJson` 等能力，但当前仓库已有自定义草稿、附件清单和校验规则，因此第一期保留自定义发布流程，减少迁移范围。（参考：Tauri GitHub Action 官方文档）

### 5.3 签名密钥

增加 GitHub Actions Secrets：

- `TAURI_SIGNING_PRIVATE_KEY`。
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`，如果私钥设置密码。

只有发布 job 能读取私钥，普通 PR 和检查 job 不读取。私钥离线保存恢复副本；一旦丢失，已有安装版本无法继续验证后续签名更新，因此上线前必须完成备份。

## 6. 测试和验证

### 6.1 单元测试

新增或修改：

- `src-tauri/src/updates.rs` 测试版本比较、平台目标选择、状态转换、失败回滚和重复动作拒绝。
- `src-tauri/src/system_settings.rs` 测试旧设置文件读取、新字段默认值和跳过版本持久化。
- `src/updateApi.test.ts` 或 `src/useAppUpdate.test.ts` 测试事件转换和按钮状态。
- `scripts/release.test.mjs` 测试 updater 产物命名、签名文件完整性、`latest.json` 目标映射和草稿拒绝发布条件。

### 6.2 前端集成测试

新增 Playwright 覆盖：

- 无更新、发现更新、检查失败、下载失败。
- 下载中关闭设置窗口后重新打开仍显示进度。
- 多窗口只出现一份更新任务。
- 有未保存文件时安装被阻止。
- 有活动传输、端口转发、Docker 操作时提示具体阻塞原因。
- 取消安装后可以继续使用 App。
- 更新完成提示只在实际运行目标版本后显示。

浏览器测试使用 mock updater，不直接下载 GitHub 包；真实安装测试单独在打包产物上进行。

### 6.3 真实安装验收

至少做相邻版本升级测试，例如：

1. 安装旧版。
2. 创建服务器配置、设置主题和终端偏好。
3. 创建未保存远程文件、传输任务和活动 SSH 会话，确认安装被阻止。
4. 清理工作后下载并安装新版。
5. 重启后确认版本、配置、设置和历史仍在。
6. 测试取消安装、无权限、网络中断、签名错误和升级包缺失。

平台验收顺序：Windows NSIS → macOS arm64/x64 → Linux AppImage → Windows MSI / Linux DEB 手动兜底。

## 7. 分阶段执行顺序

### 阶段 A：发布基础和最小 updater

修改依赖、Tauri 配置、capability 和 Release 构建，加入签名产物与 `latest.json`，先不接复杂安装前保护。目标是能用测试 Release 从旧版完成一次 Windows NSIS 升级。

验收：Release 草稿附件完整、签名可验证、旧版能发现新版本、下载并启动安装器。

### 阶段 B：后端更新状态和前端入口

实现 `updates.rs`、更新 command、事件广播、设置字段、关于窗口入口、系统设置软件更新区和下载进度。

验收：开发环境可以用 mock endpoint 覆盖所有状态；多窗口不会重复任务；错误信息可重试。

### 阶段 C：安装前工作保护

抽取现有退出准备流程，加入安装锁、活动任务确认、未保存文件阻止、主窗口安装协调和启动后结果确认。

验收：活动工作不会被静默打断；安装准备失败可以恢复；确认安装后才会关闭工作。

### 阶段 D：平台扩展和发布验收

完成 macOS、Linux AppImage 的真实升级；为 MSI、DEB 增加手动下载兜底；完善版本文案、日志和回归测试。

验收：跨平台目标选择正确，平台不支持自动安装时不会误报已完成升级。

### 阶段 E：正式启用

发布第一个包含 updater 的基线版本，更新发布文档，明确现有用户需要手动安装一次。之后才把自动检查默认打开并通过正式 Release 推送新版本。

## 8. 预计修改文件清单

**新增：**

- `src-tauri/src/updates.rs`
- `src/updateApi.ts`
- `src/useAppUpdate.ts`
- `src/SoftwareUpdateSection.tsx`
- `src/software-update.css`
- 需要时增加更新状态和发布脚本 fixture

**修改：**

- `package.json`
- `package-lock.json`
- `src-tauri/Cargo.toml`
- `src-tauri/Cargo.lock`
- `src-tauri/tauri.conf.json`
- `src-tauri/capabilities/desktop.json`
- `src-tauri/src/lib.rs`
- `src-tauri/src/system_settings.rs`
- `src/App.tsx`
- `src/AppBrand.tsx`
- `src/SystemSettingsDialog.tsx`
- `scripts/release-assets.mjs`
- `scripts/publish-release.mjs`
- `.github/workflows/check.yml`
- `.github/workflows/release.yml`
- `docs/releases.md`

不会修改服务器数据表结构；第一期只扩展设置文件和更新状态文件。

## 9. 实现前的两个技术验证点

1. 当前 Tauri 2 版本、updater 插件版本和 Rust toolchain 是否能稳定生成三类 updater 产物；先用 Windows NSIS 完成闭环，再扩展平台。
2. 当前 Windows 安装包到底主要使用 NSIS 还是 MSI，以及已安装用户的安装目录和权限模式；这决定自动升级按钮是否对 MSI 用户开放。

如果这两个验证点发现某种安装格式不能安全自动升级，界面会保留检查更新和手动下载，不会把它伪装成自动升级支持。

## 10. 你确认后的执行方式

你确认后，我会按阶段 A → B → C → D 实施，不直接一次性改完所有平台：

- 先修改发布基础和依赖配置。
- 编译、Rust 测试、前端测试和发布脚本测试通过后，再接 App 内状态和界面。
- 每个阶段检查工作区现有未提交改动，避免覆盖当前开发内容。
- 先完成可审查的代码和测试，再进行真实打包升级验证。

本计划不改变版本号，也不立即创建 Release 或推送 tag。首次正式发布 updater 基线版本时，再按现有 `npm run release:prepare` 和 tag 流程处理。

## 参考

- Tauri 2 Updater：官方要求配置 updater 产物、公钥、endpoint 和 capability 权限；默认更新比较只接受高于当前版本的版本。（参考：Tauri Updater 官方文档）
- Tauri 2 Process：提供 `relaunch()` 重新启动当前 App，需配合 process 插件和权限。（参考：Tauri Process 官方文档）
- Tauri GitHub Action：支持将 updater JSON 和签名产物上传到 GitHub Release；当前仓库保留自定义发布流程以兼容现有草稿和附件校验。（参考：Tauri GitHub Action 官方文档）

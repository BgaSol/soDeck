# Windows 本地开发与验证入口

适用范围：T-003 的 Windows 开发版本。先读 [文档入口](README.md)、[T-003](tasks/T-003-core-terminal.md) 和[联调契约](technical/implementation-contract.md)。功能修改先更新本次方案与测试流程，确认后再编码；测试结果写回对应任务附录。

## 环境与首次启动

当前验证环境为 Windows 11、Node.js 24、Rust 1.99 MSVC 工具链、Visual Studio C++ 构建工具和 WebView2。SSH 的 ssh2/libssh2 使用 OpenSSL 后端；Windows 构建需 Perl，`scripts/cargo.mjs` 与 `scripts/desktop.mjs` 会优先使用 `.local/build-tools/strawberry/perl/bin/perl.exe`，也可以通过 `OPENSSL_SRC_PERL` 指定。Perl 仅用于构建，不进入插件或产品运行时。

在仓库根目录依次运行：

```powershell
npm ci
npm run plugin:pack -- --plugin terminal --version 0.1.0
npm run desktop
```

第二步独立构建终端 UI 与 worker，生成 `resources/terminal.sodeck-plugin` 出厂包及开发公钥。首次本地签名密钥生成在忽略目录 `.local/signing`，开发公钥与插件包也不进入 Git。不要将开发密钥当正式发布密钥。

`npm run desktop` 启动 Vite 和 Tauri 桌面；单独执行前端 Vite 只能浏览界面，实际 SSH、插件独立进程及原生迁移需要桌面宿主。程序默认使用 Tauri 的应用数据目录；可设置 `SODECK_DATA_DIR` 指向独立测试目录，避免混用开发数据。同一目录只允许一个插件管理器持有写锁。

## 构建与独立更新

```powershell
npm run desktop:build
npm run plugin:pack -- --plugin terminal --version 0.1.1
```

桌面构建输出为 `target/release/sodeck-desktop.exe`，当前关闭安装包生成。0.1.1 命令只生成独立更新包 `resources/sodeck.terminal-0.1.1.sodeck-plugin`，不覆盖出厂 0.1.0 包。通过插件管理页选择包后，当前运行版本继续工作，正常退出并重新启动才激活新版本；初始化失败保留旧版与旧数据。

本地执行文件仍从仓库的 `resources` 读取出厂包和开发公钥；如需移动运行，应同时配置 `SODECK_PLUGIN_PACKAGE` 与 `SODECK_PLUGIN_KEYS` 为实际路径。本阶段没有完成可分发安装包或正式签名发布流程。

本机已构建的开发目录包位于 `.local/dev/SoDeck-v2/`，双击 `Start-SoDeck.cmd` 运行，数据独立存放在该目录 `data/`。该目录含执行文件、出厂终端包、公钥及文件哈希 `build.json`，已生成的 0.1.1 更新也附在 `resources/`，可在插件页选择后正常退出/重启验证。在构建带静态 shell 的 debug 执行文件和出厂包后，运行 `npm run desktop:package-dev` 可重新复制产物；该命令不编译、不替换 data，也不生成安装器。已执行及未执行范围见[桌面交付记录](tasks/T-003-desktop-results.md)。

## 验证

```powershell
npm run check
npm run test:contracts
npm run test:core
```

`check` 包含 TypeScript 和模块边界检查；契约测试覆盖 SDK、工作台与实际终端 worker；Rust 工作区测试覆盖核心、包管理、独立进程与原生工作台状态。依赖缓存齐备时，Rust 可使用 `node scripts/cargo.mjs test --workspace --offline`。不要把界面契约测试当作原生功能通过。

原生回归脚本需要先构建包含静态 shell 的 debug 桌面执行文件；脚本读取 `target/debug/sodeck-desktop.exe`，不自动构建它。开发时可执行 `npm run desktop:build -- --debug`。SSH 回送服务使用 `tests/core/ssh_fixture.py`，需 Python 与 Paramiko；通过 `SODECK_TEST_PYTHON` 指定 Python，脚本将仓库 `.local/python` 加入 `PYTHONPATH`，其他机器先安装或配置对应依赖。

```powershell
npm run test:terminal:native
npm run test:updates:native
```

脚本使用隔离数据目录并留下 `.qa` 证据。更新回归使用端口 9353/9354，会独立构建 0.1.1 并多次正常重启 App；执行时不要同时重打出厂包或修改开发签名密钥。SSH fixture 验证真实协议和 PTY 字节回送，不提供 Linux shell。

补充回归入口：

| 入口 | 端口与证据 | 覆盖及操作方式 |
| --- | --- | --- |
| `node tests/desktop/cancellation.mjs` | 9373/9374；`.qa/cancellation-native` | 实际原生宿主、worker 与 SSH 取消；按钮和终端输入由 CDP 操作 WebView，不覆盖操作系统键盘路线 |
| `$env:SODECK_LIFECYCLE_EXTERNAL_CLOSE='1'; node tests/desktop/lifecycle.mjs` | 9395/9396；`.qa/lifecycle` | 列表关闭、保存失败、worker 重命名与窗口标题、窗口关闭、删除、退出、重启；原生关闭由外部 Computer Use 完成 |
| `node tests/desktop/move-close.mjs` | 9397/9398；`.qa/move-close` | 10 次弹出/返回、每次挂载真实回显及资源身份、迁移期间原生关闭的串行执行；需配合外部 Computer Use 信号文件 |
| `node tests/desktop/move-faults.mjs` | 9403/9404；`.qa/move-faults` | 实际快照不兼容、目标就绪超时、源/外壳回执丢失；CDP 页面桥接注入，保留产品 10 秒迁移和 SDK 35 秒期限，复制并哈希产物后从副本运行 |
| `node tests/desktop/recovery.mjs` | 9405/9406；`.qa/recovery-native` | 视图状态版本不兼容/保存失败与显式恢复，更新提交前及激活后真实进程终止；仅测试目录签名候选，使用产物副本 |
| `node tests/desktop/recover-storage.mjs` | 9415/9416；`.qa/recover-storage` | 真实渲染进程崩溃后注入布局保存失败，验证恢复已提交/警告、原 PTY 继续、存储恢复后保存及退出；复制产物、CDP 操作 |
| `node tests/desktop/input-observe.mjs` | 9413/9414；`.qa/input-sky` | 双 PTY 原生控制键与焦点、Enter/Esc 重命名；脚本只设置隔离 SSH 回送器并观察，所有 Windows 点击/按键由外部 Computer Use 执行 |

生命周期外部关闭模式等待 `close-request.json`，实际观察标题并关闭后才写 `close-confirmation.json`。并发关闭脚本打印 `ready` 后等待 `start.json`；收到 `move-held.json` 后实际 Alt+F4，再写 `release.json`，应在约 5 秒内完成，产品原有 10 秒迁移期限不变。完整格式、流程及成功记录见[生命周期验证](tasks/T-003-lifecycle-results.md)：本轮生命周期 7 组/2 次正常退出、移动与并发关闭 3 组、迁移故障恢复 4 组均通过；取消连接结果见[桌面交付记录](tasks/T-003-desktop-results.md)。

原生输入观察入口在 `ready.json` 中给出 PID、窗口对应视图、输出目录及 `command.json` 路径；通过唯一 `id` 配合 `mark`、`verify`、`finish` 指令建立字节基线、精确核对两个 PTY 并正常退出，默认 20 分钟超时。`tests/desktop/input-fixture.py` 仅作隔离 SSH 字节回送，主机私钥不落盘。重新获取并激活目标窗口后用逐键输入；本机 `sky.type_text` 会引入 Ctrl+V，不适合此终端逐字节验证。协议、四组通过记录和首次前台不确定的失败证据见[原生输入记录](tasks/T-003-native-input-results.md)。剪贴板功能保留未执行，不能将此入口的只读观察范围当作输入工具内部未访问剪贴板的证明。

现有 `native.mjs` 与生命周期默认 CLI 模式使用 `windows.ps1`；`ime.mjs` 还使用 `ime-windows.ps1` 的前台窗口与 SendInput 自动化。这些是独立 CLI 路线。Computer Use 会话采用上述外部关闭模式，当前中文 IME 成功记录来自 Computer Use 原生按键和实际窗口观察，流程与证据见[IME 记录](tasks/T-003-ime-results.md)，没有新增通用 `ime-sky.mjs` 入口。控制桌面焦点的原生测试应串行执行，避免窗口创建影响另一个测试的前台输入，也不要在同一次 Computer Use 操作期间并行运行旧 PowerShell 全局输入脚本。

Windows 单显示 Microsoft Pinyin、候选位置与迁移时组合输入已单独验证；Linux/OpenSSH、多屏/DPI、辅助技术及正式安装发布仍按任务单独验收。完整已执行范围见[核心记录](tasks/T-003-core-results.md)、[插件更新记录](tasks/T-003-plugin-results.md)和[桌面交付记录](tasks/T-003-desktop-results.md)。

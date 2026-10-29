# Windows 本地开发与验证入口

T-029当前工作区：核心`2.0.0-dev.21`、API/SDK`6.3.0`，core v7和四观测分区v2保持。实现/测试结果见[T-029](tasks/T-029-results.md)，发布配置与命令见[v2发布工具](technical/release-v2.md)。生产渠道公钥尚未配置，development构建明确禁用官方源检查；第三方手动导入保持。下面T-028及更早入口记录各任务当时的交付。

T-029试用入口为`.local/dev/SoDeck-v2-T029-r2/Start-SoDeck.cmd`，独立data且未内置测试服务器。重建执行`node scripts/desktop.mjs build --debug --no-bundle`与`node scripts/cargo.mjs build -p sodeck-plugin-host --bin sodeck-plugin-host`；`node scripts/package-online-updates-dev.mjs`只向全新目录打包，禁止覆盖已有包。原生验证`tests/desktop/online-updates.mjs`、`online-update-installer.mjs`、`online-updates-package.mjs`串行执行，安装脚本只可使用T-029 QA夹具与专属Linux服务。完整实测和清理由结果维护。

T-028当前入口：核心 `2.0.0-dev.20`、API/SDK `6.2.0`，core数据库v7。先读[任务](tasks/T-028-plugin-management.md)与[实测](tasks/T-028-results.md)，第三方资源包见[插件开发](plugin-development.md)。构建前端后执行 `node scripts/cargo.mjs build -p sodeck-desktop -p sodeck-plugin-host --features sodeck-desktop/custom-protocol --offline`；`node scripts/package-plugin-management-dev.mjs`只向全新`.local/dev/SoDeck-v2-T028-r1`打包，存在时拒绝覆盖。

原生测试 `plugin-management.mjs`、`plugin-management-import.mjs`、`plugin-management-updates.mjs`、`plugin-management-package.mjs` 位于 `tests/desktop`，需串行；updates需先准备测试A/B包和隔离Linux SSH服务，package需设置 `SODECK_TEST_PYTHON` 指向Python3供只读数据库备份。具体环境与通过范围见结果。浏览器入口为 `npx playwright test -c playwright.ui.config.ts plugin-management.spec.ts settings.spec.ts connection-home.spec.ts`；管理器检查为 `node scripts/cargo.mjs test -p sodeck-plugin-manager --offline`。

下方保留各历史任务当时的版本和入口，不把旧命令当作当前功能的验证结论。

当前 T-008：核心 `2.0.0-dev.5`、SDK/API `4.1.0`（`workspace.v4` 保持）、文件 `0.1.0`、原签名终端 `0.4.0`。授权、实际通过项和剩余验证见 [任务](tasks/T-008-server-file-tree.md)、[实测](tasks/T-008-results.md)，试用见 [验收步骤](tasks/T-008-acceptance.md)。

T-008 构建顺序：`node scripts/pack-plugin.mjs --plugin files --out resources/files.sodeck-plugin`、`npm run build -w @sodeck/desktop`、`node scripts/cargo.mjs build -p sodeck-desktop --features custom-protocol`、`npm run desktop:package-dev -- T008`。打包使用 `.qa/t008/baseline/terminal-0.4.0.sodeck-plugin` 的原签名副本，不重新构建终端。输出 `.local/dev/SoDeck-v2-T008/`；`SODECK_TEST_PACKAGE_DIR` 指向该目录后运行 `node tests/desktop/package-smoke.mjs`，会复制到另一目录后验证。

文件原生入口 `tests/desktop/files.mjs`、`files-platform.mjs`、`files-visual.mjs`、`files-load.mjs`、`files-updates.mjs`、`files-degraded.mjs` 必须串行，引用独立 Linux 配置；环境见 [隔离 Linux 说明](../tests/linux/t008/README.md)。核心外部测试显式运行 `node scripts/cargo.mjs test -p sodeck-core --test remote_read -- --ignored --nocapture` 并设置 `SODECK_TEST_REMOTE_CONFIG`。源码中列有入口不等于所有矩阵通过，以实测和限制为准。

历史 T-009 基线：核心 `2.0.0-dev.4`、终端 `0.4.0`、SDK/API 4 / `workspace.v4`。规格、授权与结果见 [任务](tasks/T-009-clean-start.md)、[实测](tasks/T-009-results.md)。构建顺序为 `npm run plugin:pack`、`npm run desktop:build -- --debug`、`npm run desktop:package-dev -- T009`，独立输出 `.local/dev/SoDeck-v2-T009/`，不覆盖历史包或日常数据。原生回归入口为 `npm run test:clean-start:native` 和 `npm run test:clean-start:plugins`，必须串行运行；覆盖具体范围以实测为准。以下旧任务及其脚本保留历史依据，不直接用于验证已替代的布局恢复行为。

T-007 服务器编辑修订的当前方案与实际结果见 [任务](tasks/T-007-server-editor.md)、[结果](tasks/T-007-results.md)。构建后执行 `npm run desktop:package-dev -- T007` 生成独立 `.local/dev/SoDeck-v2-T007/`。新增原生入口 `node tests/desktop/server-editor.mjs` 使用隔离 Tauri/WebView2、DPAPI 和 loopback SSH，由 CDP 操作表单，不访问日常服务器或使用全局键盘。目录包冒烟可设置 `SODECK_TEST_PACKAGE_DIR` 指向交付目录。

历史 T-006 基线： 核心 `2.0.0-dev.3`、终端 `0.3.0`、插件 API 3 / `workspace.v3`。先读 [T-006](tasks/T-006-workbench-interaction.md)、[实施方案](technical/workbench-interaction-implementation.md) 和[实测记录](tasks/T-006-results.md)。功能修改先更新方案与测试流程，按已确认范围实施。

## T-006 历史入口

依次执行 `npm run plugin:pack`、`npm run desktop:build -- --debug`、`npm run desktop:package-dev -- T006`。打包命令只复制已构建产物至 `.local/dev/SoDeck-v2-T006/`，保留该目录 data 和旧 T-004 包，不自动编译。双击 Start-SoDeck.cmd；版本/哈希、验收步骤、截图和证据随包提供。

| 入口 | 用途 |
| --- | --- |
| `npm run check`、`npm run test:contracts` | 生成契约、类型、依赖边界、SDK/worker/终端输入 |
| `node scripts/cargo.mjs test --workspace --offline` | Rust 工作区，包括主题/菜单/布局迁移及恢复保护 |
| `npx playwright test -c playwright.ui.config.ts` | 浏览器交互、通用输入框、剪贴板延迟/去重与协议模式 |
| `node tests/desktop/t006.mjs` | Windows 原生菜单、剪贴板、焦点及十次视图拆合 |
| `node tests/desktop/themes.mjs` | 同一二进制两主题、多表面、无效主题回退 |
| `node tests/desktop/ime.mjs` | 实际 Microsoft Pinyin 和移窗输入归属 |
| `node tests/desktop/package-smoke.mjs` | T-006 目录包全文件哈希、配套插件启动和离线拆合 |

全局焦点/键盘的原生测试必须串行；Computer Use 会话不要混用 PowerShell 输入路线。脚本实际覆盖及未运行矩阵以 T-006 实测记录为准，不因列有入口就视为全部已通过。主题测试入口 SODECK_THEME_FILE 仅用于隔离验证，不代表已实现主题设置。历史 API 2 独立更新/升级脚本不能直接作为 API 3 的通过依据。

## T-004 历史入口

```powershell
npm ci
npm run plugin:pack
npm run desktop:build -- --debug
npm run desktop:package-dev
```

以下记录对应 T-004 当时的版本：最后一步曾复制到 `.local/dev/SoDeck-v2-T004/`。当前打包目标以 T-006 为准；保留历史包和该目录 data。终端/SDK 改动后必须重新执行 `plugin:pack`。构建工具、开发签名和真实 SSH 回送夹具的环境要求见下文。

| 本次入口 | 覆盖 |
| --- | --- |
| `npm run check`、`npm run test:contracts` | 生成契约新鲜度、类型/依赖边界、SDK/worker/表面队列 |
| `node scripts/cargo.mjs test --workspace --offline` | 核心、插件宿主/管理器、工作台模型和共享契约；ignored 外部服务用例另跑 |
| `npx playwright test -c playwright.ui.config.ts` | 19 项浏览器交互，不替代 Windows 原生 |
| `node tests/desktop/server-tabs.mjs` | 实际工作区拆合、PTY 身份、元数据、重连绑定、关闭范围 |
| `node tests/desktop/server-tabs-chrome.mjs` | Windows 命中、Snap 截图、物理排序/跨窗/拖出/取消 |
| `node tests/desktop/server-tabs-faults.mjs` | 快照超限、30 秒超时、目标关闭、取消/排队关闭、退出、存储失败 |
| `node tests/desktop/workbench-contract.mjs` | 签名非业务夹具、候选崩溃、32 MiB 批次上限、通用命令、关闭及全局视图 |
| `node tests/desktop/server-tabs-load.mjs` | 五窗口、一 worker、四 PTY 负载、十次拆合、二十次创建关闭与内部计数 |
| `node tests/desktop/exit-stress.mjs` | 三次独立启动，每次十次拆合与并发 IPC、10050 次请求达到容量上限，关闭后正常退出；不依赖鼠标或远端服务 |
| `node tests/desktop/ime.mjs` | Microsoft Pinyin 真实按键、组合输入取消迁移、移窗后两窗中文唯一回显 |
| `npm run test:terminal:native`、`node tests/desktop/cancellation.mjs` | 受影响终端基线与真实 SSH 取消/回收 |
| `npm run test:updates:native`、`node tests/desktop/server-tabs-upgrade.mjs` | 同核心下实际 `0.2.0 → 0.2.1` 独立更新、旧核心/包/布局升级 |
| `node tests/desktop/package-smoke.mjs` | 独立目录包哈希、配套插件启动、离线拆合及正常退出，不写交付包的 data |

原生测试串行运行，先构建 debug 核心并打包终端。独立更新运行期间不得重建核心；测试包在各自 `.qa` 目录，不覆盖交付包。所有测试使用独立数据与本机协议服务；Linux/OpenSSH、不同 DPI 双屏、完整辅助技术的缺口见任务记录。验收步骤见 [T-004 开发包说明](tasks/T-004-acceptance.md)。

构建使用同版本 tauri-runtime-wry 2.12.3 的局部生命周期修复，来源、许可证、差异与撤除条件见 [补丁说明](../third-party/tauri-runtime-wry/SODECK-PATCH.md)。Cargo 不依赖 `.local` 诊断源码，开发目录包同时保留此依赖的许可证。

## T-003 历史入口说明

以下保留 T-003 的历史环境、版本及证据说明。`0.1.x` 包和旧工作台 API 不适用于当前分支；补充脚本中未列入上表的入口尚未适配 API 2，不应直接用于当前回归。T-003 原始产物及已通过记录仍有效，其事实不被 T-004 改写。

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

## T-013 跳板验证入口

实施和环境见 [T-013 结果](tasks/T-013-results.md)。`tests/linux/t013/setup.py` 只针对专属 QEMU overlay（管理22313、跳板22314），`tests/desktop/ssh-jump.mjs` 使用9811–9813，UI脚本使用9821–9823，迁移9831/9832，负载9841/9842；独立开发包命令为 `node scripts/package-dev.mjs T013`。专属核心测试需隔离 Python/Paramiko，并用 `--include-ignored` 明确启用，不能把默认忽略记为通过。清理脚本只关闭本任务 guest，宿主临时文件保留的策略限制见结果。

## T-012 三主题与设计系统入口

实际代码与验证见[T-012结果](tasks/T-012-results.md)。`npm run check`包括设计参数生成漂移、PostCSS/TS AST插件样式边界及类型检查；`npm run test:design-system`运行生成器和违规夹具。`apps/desktop/design-system.html`为Vite开发组件目录，不进入生产导航。样式唯一参数来源是`packages/ui/src/tokens`，颜色是`themes`；插件通过`@sodeck/ui`公开入口使用。

原生测试串行：`node tests/desktop/dopamine.mjs`（9841–9843，三窗/8表面/200菜单）、`dopamine-visual.mjs`（9981–9983，DPI/IME/高对比）、`dopamine-compatibility.mjs`（9923起递增）、`dopamine-lifecycle.mjs`（9901–9903）。全部结果在`.qa/t012`；真实SSH只用专属QEMU loopback22281。`node scripts/package-dev.mjs T012`生成独立目录包。不要与T-013原生负载测试同时运行，两个测试共用9841调试端口。保留和恢复本次实际捕获的系统设置。

## T-013 r2 多级验证入口

新环境为 `.qa/t013-multihop` / QEMU22323、22324，使用 `tests/linux/t013/multihop-setup.py`；不复用已停的r1网络。`ssh-multihop.mjs`（9861–9863，`MH_OLD=1`/`MH_FILES_ONLY=1`）、`ssh-multihop-ui.mjs`（9871–9873）、`ssh-multihop-long-ui.mjs`（9911/9912）、`ssh-multihop-migration.mjs`（动态空闲端口）串行执行，真实边界见[多级结果](tasks/T-013-multihop-results.md)。`ssh_multihop.rs`含需Python/Paramiko的8项，用 `--include-ignored`显式运行；1001节点配置与16节点网络分开记录。独立目录包命令：`node scripts/package-dev.mjs T013-Multihop`；用 `SODECK_TEST_PACKAGE_DIR`指定新包后执行`tests/desktop/package-smoke.mjs`。

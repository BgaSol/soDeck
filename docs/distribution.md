# SoDeck 安装、升级与插件分发

组件设置随核心 0.9.0、终端 0.2.0、文件 0.4.0 和网络隧道 0.2.0 独立发行，五目标构建与实际包组合验收均通过，结果见 [v0.9.0](releases/v0.9.0.md)。清单中的 `settings` 与插件一起分发；终端和文件插件分别携带三款字体的 Regular/Bold、来源校验元数据和许可证，安装后无需联网下载字体。普通构建只校验并复制已提交的资产，维护时才显式运行 `node scripts/prepare-font-assets.mjs`。核心包仍不依赖业务插件构建或包含字体文件。Check 增加通用设置界面回归，原生实际包组合烟测验证终端设置、字体加载、保存重开和 PTY 保留。

v0.6.0 增加 terminal/files 两个独立目录，沿用 Package plugin 的 `--plugin` 和 `plugin-terminal-v*` / `plugin-files-v*` 标签。核心构建不依赖它们。新增 Verify native remote plugins 工作流，接收同一提交的三个独立打包 run ID，在五个原生 runner 安装/解包实际产物，启动隔离 OpenSSH，验证 PTY 经原生 Channel 到 xterm 再回传显示快照，以及文件读取、原子保存和下载。工作流定义不等于已运行；实际结果见 [实施记录](implementation-remote-plugins.md)。

## 原则

核心独立安装和升级，业务插件全部外置。源码和发行包位于公开的 `BgaSol/soDeck`，使用 GitHub Actions 与普通 Releases，不另建更新服务器或插件市场。

## 核心

| 目标 | Rust target | 发行包与在线升级 |
| --- | --- | --- |
| Windows x86_64 | `x86_64-pc-windows-msvc` | NSIS EXE；应用内下载、安装并重启 |
| Windows ARM64 | `aarch64-pc-windows-msvc` | 原生 ARM64 NSIS EXE；同上 |
| Linux x86_64 | `x86_64-unknown-linux-gnu` | DEB 走系统包管理；AppImage 支持应用内升级 |
| Linux ARM64 | `aarch64-unknown-linux-gnu` | 原生 ARM64 DEB 与 AppImage；同上 |
| macOS ARM64 | `aarch64-apple-darwin` | DMG 首次安装；app.tar.gz 用于应用内更新 |

核心版本唯一来源为 `src-tauri/tauri.conf.json`。设置 → 更新集中检查核心与已安装插件，默认每日自动检查，支持选择与批量下载、逐项重试，准备完成后统一重启并应用。下载期间不会主动停止插件；安装前正常退出全部插件，数据保持在系统应用数据目录。核心包落盘后重开可通过官方 updater 离线验签恢复。当前实现与平台验证边界见 [更新体验实施记录](implementation-update-experience.md)。v0.1.1 没有更新模块，需要手动安装一次 v0.2.0。

更新使用固定版本的 Tauri 官方 updater，它要求更新包签名。公钥提交到 Tauri 配置；私钥保存在维护者用户目录 `.sodeck-signing/updater.key` 并配置为 Actions 的 `TAURI_SIGNING_PRIVATE_KEY` Secret，绝不提交私钥。密码可用 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` Secret；本项目初始私钥无密码，Secret 留空即可。此签名与 Windows/macOS 商业代码签名、插件权限无关。必须备份原私钥，随意换公钥会使旧客户端无法升级。

核心 0.18.0 将客户端的 GitHub 请求统一接入备用传输地址，操作见[使用指南](usage.md)。发行仍采用原始 GitHub 仓库和 Release URL；清单、缓存与固定的版本/平台/哈希/签名不保存代理地址。核心由官方 updater 从已读取清单恢复 Update，下载后验签，安装与离线缓存恢复继续使用官方实现。此功能不改变打包、签名或发布工作流，也不把代理首页可用或包首段测试当作正式包验收。实际实现和验证范围见[实施记录](implementation-update-acceleration.md)。

本地普通构建不需要签名密钥：

```sh
npm ci
npm run check
npm run scripts:test
npm run test
npm run build -- --bundles nsis -- --locked
```

正式更新包添加 `--config src-tauri/tauri.release.conf.json` 并通过环境提供签名私钥。Linux 使用 `--bundles deb,appimage`，macOS 使用 `--bundles dmg,app`，显式生成安装盘和签名更新归档。Node 24.22.0、Rust 1.99.1 与锁文件固定；Linux 运行基线仍为 Ubuntu 22.04/24.04 LTS。

### Windows 安装范围与图标

Windows x86_64 与 ARM64 的 NSIS 新安装统一使用 `perMachine`，默认目录为 `Program Files\SoDeck`，安装登记写入 HKLM，供所有用户启动。安装、卸载和核心在线更新需要管理员确认；应用自身仍以普通用户身份运行，服务器资料、凭据、设置和在线插件保存在各用户的应用数据目录，不改为全机共享。插件更新不写核心安装目录。

已有用户安装切换为系统安装时，先退出 SoDeck，卸载旧用户安装，**不要勾选删除应用数据**，再运行新安装包。同一 Windows 用户重新启动后继续使用原资料和插件。新系统安装器只读取系统安装登记，不自动清理旧用户安装；仅执行在线更新可能保留旧程序和旧安装登记。切换安装范围按上述手动重装步骤完成。

应用、快捷方式、安装程序和卸载程序共用 `src-tauri/icons/icon.ico`。安装向导的页眉与欢迎/完成页侧图复用现有 `icon.png` 的 SoDeck 标识，分别提交为 `installer-header.bmp`（150 × 57）与 `installer-sidebar.bmp`（164 × 314），均为 24 位 BMP。后续更换品牌时同步这些图片及已有各平台图标；正常构建直接使用仓库资产，无需额外图片工具。NSIS 配置位于 `src-tauri/tauri.conf.json`；官方 updater 的 `passive` 设置控制更新界面的交互方式，与安装范围不同。

## 插件生命周期与更新

- 设置 → 插件管理从仓库 main 分支的 `plugins/<名称>/release.json` 自动发现官方来源。点击“安装”从正式插件 Release 取得本机原生归档，复用更新器的 API/平台、SHA-256、解包和清单检查后注册为启用；安装期间不启动程序，重启生效。首次安装失败不登记半成品，可重试；同 ID 已安装时转到详情管理，不覆盖本地记录或更改其来源。读取名单不会自动下载插件程序。
- 插件管理提供“已安装 / 官方插件”切换和“从本地添加”入口。官方名单缓存一小时，可手动刷新；失败保留完整旧名单并提示，真实空名单与网络错误分别展示。选择带 `plugin.json` 的外部目录后保存为启用但等待重启。旧记录缺少 enabled 字段时按关闭迁移。
- 启用/关闭只改期望状态，当前进程继续运行；重启统一生效，启动自动加载启用项。可以连续修改多个插件再重启一次。
- 下载先完成校验和解压，再保存新目录记录；已启用或本次启动已加载的插件在下次启动切换。完全关闭的插件立即更新记录，仍不启动。
- 在线程序位于 `<应用数据>/plugins/<插件ID>/<版本-唯一标识>/`。原外部目录保留；数据始终在 `plugin-data/<插件ID>/`。旧程序保留用于新版初始化失败后的恢复；不承诺数据迁移回退，不建立多级版本事务框架。
- 没有来源的本地插件支持手动更新。详情可显式配置发行仓库与标签前缀，例如终端使用 `BgaSol/soDeck` 与 `plugin-terminal-v`。不根据 ID 猜测来源。
- 关闭并重启后才能移除记录，不删除程序、数据、消息和日志。当前不提供运行中热加载/卸载的用户入口。

## 仓库维护

每个 Rust 插件在 `plugins/<名称>/` 包含：Cargo.toml（唯一版本来源）、Cargo.lock、release.json（ID、名称、API、二进制名、标签前缀）、CHANGELOG.md 和 src/。现有业务目录为 terminal、files、network-proxy、system-status 和 docker。新增其他语言插件时再扩展构建适配，不提前建立通用构建框架。

`release.json` 同时作为仓库中的官方发现元数据，不另建集中名单。`id`、`name`、`tagPrefix` 必填；可选 `description` 为用途说明、`icon` 为核心图标名称（缺省使用通用图标）、`order` 为显示顺序（缺省 0，再按 ID 排序）。这些展示字段不改变实际插件包、版本来源或 API。ID 与标签前缀在官方配置中各自唯一；无效配置或不完整目录会使本次刷新失败并保留上次完整缓存，不能用部分结果覆盖名单。

新增官方插件时，将自身目录和发行配置合入 main，再用现有 `Package plugin` 工作流发布自己的正式 Release。名单更新无需修改或重新发布核心，也不需要生成聚合索引、修改旧 Release 附件。配置出现后可被发现，只有正式 Release 有兼容 API、本机平台的包时才可安装；移除该发行配置后下次成功刷新不再列出，但已有用户的插件记录与更新来源保留。旧版核心仍使用自身的固定名单，需要先升级到支持仓库发现的核心版本一次。

系统状态沿用独立 `Package plugin` 和 `plugin-system-status-v` 标签。五个原生打包任务解开本次归档，验证包内程序的 initialize、重复 mount、pause、dispose 和 shutdown 及 UI 资源；远端采集另外通过 `npm run system-status:test` 验证，归档烟测不能代替各远端系统验收。0.1.0 已独立发行，核心 0.10.0 登记官方下载来源；包来源、原始 run、Release 和实际验证范围见[插件 CHANGELOG](../plugins/system-status/CHANGELOG.md)，实现边界见[实施记录](implementation-system-status.md)。

Docker 使用同一独立 `Package plugin` 工作流和 `plugin-docker-v` 标签。五个原生任务解开真实归档，验证包内进程的初始化、查询、操作去重、日志搜索、关闭与正常退出；Check 在 Linux x86_64 上运行真实核心、隔离 Engine 和 OpenSSH 的业务联调及 UI 回归。归档进程验收不能代替真实 Engine 或原生 WebView 交互验收。0.1.0 的包来源、原始 run、Release 与实际验证范围见[插件 CHANGELOG](../plugins/docker/CHANGELOG.md)及[实施记录](implementation-docker-plugin.md)；本次按用户要求标记 Latest，常规插件发布仍默认非 Latest。

插件包与更新清单使用同一 Cargo 元数据读取函数，按插件 `Cargo.toml` 路径选择包版本，不依赖 TOML 表的顺序或空格写法。`Package plugin` 的选择、打包和 Release 汇总任务均显式安装固定 Rust 工具链；发布标签仍须匹配插件自己的正式版本。

UI 插件打包前在仓库根目录执行 `npm ci`；打包工作流已声明此步骤。`scripts/plugin-menu-assets.mjs` 使用锁定的 Vite 将共享文本菜单与 Floating UI 打成独立 `ui/text-menu.js`，附带依赖许可证，不使用 CDN 或运行时 npm 导入。终端自身 UI 构建也调用同一入口。每个插件各自携带资源，核心打包仍不依赖插件构建。

Hello 和组件交互示例已退役，删除了独立工程、官方来源和示例包验收工作流。升级启动前移除这两个 ID 的旧登记，按既有移除规则保留原程序目录和数据。历史 Release 和版本说明保留。宿主交互回归使用 `scripts/fixtures/workspace-ui/` 与仅在 `test-fixtures` 下构建的 `workspace-ui-fixture`，不作为产品插件打包、安装或发行。实际包继续由 `Verify native remote plugins` 验证。

```sh
# 本地插件打包，必须明确插件目录
npm run plugin:build -- --plugin terminal
npm run plugin:build -- --plugin files
npm run plugin:build -- --plugin system-status
npm run plugin:build -- --plugin docker
npm run plugin:build -- --plugin network-proxy --out-dir artifacts/another-build
# 发布核心
# 修改 tauri.conf.json 版本及 docs/releases/v<版本>.md 后提交
 git tag v0.2.0
 git push origin v0.2.0
# 独立发布插件
# 修改 plugins/terminal/Cargo.toml 版本、锁文件和 CHANGELOG 后提交
 git tag plugin-terminal-v0.1.2
 git push origin plugin-terminal-v0.1.2
```

普通提交只运行相关检查。`Package SoDeck` 匹配 `v*`，`Package plugin` 匹配 `plugin-*-v*`；两者也支持手动生成 Artifact。核心与插件手动入口的 publish 默认关闭，开启后准备对应组件草稿；插件还需选择 plugin。标签必须匹配对应唯一版本来源。

核心在轻量前置任务检查版本、发布说明、草稿状态和标签提交；原生平台任务仅构建、签名和验证实际包，成功后才上传含提交/版本/平台/摘要的 Artifact。汇总读取同一运行的已验证包，生成清单并集中准备草稿；同版本写入串行，重试先核对已有附件摘要，正式版本拒绝覆盖。分支默认只输出 Artifact 与清单，不创建 Release。当前实测及边界见 [打包实施记录](implementation-packaging-workflows.md)。

为复用正式编译缓存，优先在 main 手动运行 `Package SoDeck`：需要同时准备新版本草稿时开启 publish，只生成候选时保持关闭。成功的 main 运行按平台保存 `core-release-v2` 依赖缓存；后续适用构建恢复相同引用或默认分支缓存。标签本身不写供其他标签共享的缓存，既有正式版本也不能以开启 publish 覆盖。已有候选包时使用 `core_run_id` 复用，不再为发布重复编译。缓存可能因容量或长时间未访问被清理，未命中时按固定输入正常编译；本轮缓存验收和容量记录见[提速实施记录](implementation-release-performance.md)。

### 一次打包完成核心发布

| 当前情况 | Actions 操作 | 后续操作 |
| --- | --- | --- |
| 新版本尚未打包 | 在 main 运行 `Package SoDeck`，开启 publish，留空 `core_run_id` | 五平台构建、实际包验证后生成草稿；确认同一源码的相关检查，直接发布该草稿 |
| 已有候选包，需要准备草稿 | 运行同一工作流，开启 publish，填写**原始五平台打包运行**的 `core_run_id` | 所有 package 任务 skipped；仅下载、核对原包并准备草稿 |
| 只想验证已有包的汇总 | 填写原始 `core_run_id`，关闭 publish | package 全部 skipped，只核对并生成更新清单，不创建 Release |
| 草稿已齐全 | 打开该草稿，核对版本、源码和五平台附件 | 直接正式发布；不再推相同版本标签启动另一轮构建 |

工作流标题区分 `Package core` 与 `Reuse core`。metadata 汇总链接到**包的原始 run 和提交**；复用操作本身的提交用于执行脚本，不能替代原包的源码检查结果。所选分支/标签的核心版本和发布说明必须与候选版本一致。`core_run_id` 不填写只有清单的复用运行，也不自动选最近一次候选；包内记录的提交、目标、版本、文件大小和 SHA-256 均须匹配。

首次开启 publish 时，脚本创建指向原包提交的普通版本标签和 Release 草稿；Actions token 创建的标签不会再触发一轮标签打包。已有正式版本拒绝覆盖；需要修正发行内容时升新版本重新打包。插件可以在首次独立构建时开启 publish，直接发布同一份草稿归档，核心复用入口不扩展为插件联动发布。

### 失败时只重试需要的部分

发布收尾失败且 Artifact 未过期时，在 Actions 重跑失败的 metadata job，复用同一 run 的已验证包；不重新运行已成功的平台。若发布脚本需要修复后再恢复，手动运行 `Package SoDeck`，填写原始 `core_run_id` 并开启 publish：前置任务确认该运行属于本仓库核心工作流、已结束且 Artifact 尚在，原生构建全部跳过，汇总按原始提交核对产物并恢复草稿。版本须与这些产物一致。某平台失败时重跑失败目标和汇总。已有草稿的同名包必须与当前输入一致，不以覆盖旧附件解决提交或摘要冲突；过期/缺失产物需要重新构建固定版本与源码。

新上传的核心包、独立插件包和核心更新清单 Artifact 保留 30 天，用于验收和发布收尾重试；已有 Artifact 沿用创建时的期限。超过窗口需要重新构建固定源码并重新验证，不能用另一运行或另一版本的包代替。正式安装与更新下载仍使用 Release 附件，不依赖临时 Artifact 的保留期。插件沿用现有独立打包/发布入口，没有核心 `core_run_id` 恢复选项。

各目标使用原生 Runner 构建和上传，某目标失败不阻塞其他成功产物下载，更新清单只列实际成功产物。五目标都是正式支持目标，未完成项必须在交付记录明确列出。草稿附件允许重试构建后补齐；发布后的同版本附件不覆盖，修正升补丁版本。检查附件和结果后将草稿正式发布，应用才能发现它。

每个核心 Release 含 `core-update.json`，每个插件 Release 含 `plugin-update.json`；脚本从构建产物自动生成，包含组件 ID、版本、按 Rust target 区分的下载地址，以及核心签名或插件 SHA-256。插件还有核心 API 版本。版本和清单不手工维护两份。

应用分页读取 Releases，筛选组件自己的标签前缀、正式版本和本机平台；插件还检查 API。不会用全仓库 latest 混淆核心和不同插件。插件 Release 发布时标为非 latest，核心保持仓库默认发行入口。检查和下载失败显示原因与重试，不改旧程序。

## 验证

本轮另用手动 `Verify published release upgrade` 在五个原生目标验证正式旧包到新包的安装切换：测试夹具调用生产插件更新服务和官方 Tauri updater，核对公开包摘要、实际安装字节、插件旧进程保留/正常停止、设置及服务器资料，随后启动更新后的实际程序验证远程业务；Linux 同时验证 DEB 系统安装升级。该夹具不随核心分发，完整更新页面操作仍需桌面验收。固定版本对和运行入口见 [开发指南](development.md)，执行结果统一维护在 [打包实施记录](implementation-packaging-workflows.md)。

工作台样式与原生剪贴板的实施和平台边界见 [实施记录](implementation-workbench-design.md)。`npm run terminal:input:test` 安装终端锁定的前端依赖后验证实际 xterm 与剪贴板桥，无需 OpenSSH；Check 工作流执行该回归。真实远端仍由 `terminal:test` / `files:test` 及原生实际包工作流验证。原生剪贴板为显式 `SODECK_TEST_CLIPBOARD=1` 烟测，普通包烟测不改测试桌面的剪贴板。

真实连接的新增依赖与本机验证范围见 [连接实施记录](implementation-connections.md)。`npm run ssh:test` 使用临时回环 OpenSSH 服务验证实际客户端，Check 在连接/生命周期相关改动或手动 `desktop`/`all` 时执行；`npm run ssh:test -- --packaged <安装后的程序>` 验证包内连接与原生窗口移动，继续由 Windows x86_64 核心打包执行。其他目标保留原生打包与实际包启动，平台相关源码测试按改动选择或手动 `native-core` 补测；不将本机结果外推为五平台已通过。

本机执行类型、格式、脚本测试、Rust 集成测试、前端构建和本机安装包烟测。Rust 测试使用本机 HTTP 服务器验证下载、哈希、平台/API 不兼容、重启切换和启动失败恢复；正式更新来源只允许 HTTPS。

核心工作流在五个原生目标正式构建、签名，再安装/解包 EXE、DEB、DMG 后启动包内程序；AppImage 另检查启动和退出。打包不再执行源码 Rust 测试、完整 UI 或宿主脚本回归，也不构建 `test-fixtures`；相关检查沿用 Check 常规 Linux 测试及手动原生/桌面套件。网络插件 package 的源码测试同样移到 Check 常规 Linux 检查及 `network-plugin` 原生套件。必要检查可与打包并行，维护者在正式发布前确认同一提交的相关结果；不增加跨工作流等待。`Verify native remote plugins` 下载实际核心与业务插件产物验证原生界面、实际远程业务、移窗与重开退出；插件生命周期、日志、消息和重启持久化另由通用集成夹具覆盖。CI 包内启动不等价于真实设备的交互式安装升级，实际完成范围记录在对应版本说明。

真实业务插件使用独立入口 `npm run plugin:build -- --plugin terminal` 和 `--plugin files`，版本分别来自自己的 Cargo.toml，标签为 `plugin-terminal-v<版本>` / `plugin-files-v<版本>`。终端打包在插件目录内执行锁定的 npm 安装和静态资源构建；核心打包不调用这些步骤。普通构建不创建发行版本，官方列表登记来源后仍需发布对应的独立 Release 才能下载。

`Verify native remote plugins` 常用入口填写 `core_version`、`terminal_version`、`files_version`，可选 `network_version`，版本使用 `major.minor.patch`。各组件分别定位来源，允许来自不同提交；已有正式版本读取精确标签的正式 Release，未发布版本在标签源码约束下选择唯一完整的打包 Artifact。多个候选会列出 run ID 并失败，不取全仓库 latest 或最近一次构建代替选择。已有正式 Release 缺包、清单错误或不可访问时同样失败，不改取实验包。

测试明确候选仍可填写 `core_run_id`、`terminal_run_id`、`files_run_id`、`network_run_id`；保留原来的只填 run ID 方式，版本同时填写必须与该运行匹配。每个候选须来自本仓库对应工作流、已结束、五个平台 package job 成功且 Artifact 有效；只因发布汇总失败的运行仍可使用已验证包。网络版本和 run ID 都为空时不加载网络，结果明确记为未参与。

工作流先解析并保存 `native-inputs.json`，再在五个原生 runner 下载固定 Artifact/附件 ID，核对来源、清单、大小/摘要和架构后安装或解包。它加载独立插件、连接隔离 OpenSSH，验证实际 WebView/Channel/xterm 的 PTY 往返、文件读取/保存/下载、网络正文、窗口移动以及重开退出，实际核心报告的版本/平台也须匹配。它独立于核心打包，不阻塞无插件核心的构建与启动。源码侧 `npm run terminal:test` / `npm run files:test` 另验证多 Tab、草稿、保存关闭、冲突、大小边界、取消和重试。

失败平台直接重跑使用已固定的输入；重跑整个同一 run 也复用输入快照。新建验收运行要重放原来的包，可填写 `verification_run_id`，其余版本和 run 输入留空；引用的验收 run 必须已结束并保有唯一有效输入快照，业务失败的 run 也可重放其固定包。输入快照、平台结果及汇总保留 30 天，结果含各组件版本、源码、来源、包摘要与平台失败阶段。源包过期、删除或摘要变化时明确失败，不自动换包；原始构建 Artifact 和 Release 附件不会被验收修改。

网络插件另用 `npm run plugin:build -- --plugin network-proxy` 打包，标签 `plugin-network-proxy-v<版本>`；没有发布独立 Release 前，官方来源无法下载安装。原生远程验收可额外填写 `network_version` 或 `network_run_id`，验证实际包的侧栏、编辑弹窗、TCP/SOCKS5 正文、移窗和端口释放。`npm run network:test` 提供源码侧真实插件与混合跳板、OpenSSH 和页面验证。当前完成范围与尚未运行的平台见 [网络实施记录](implementation-network.md)。

SOCKS5 报文库接入后的边界、独立互操作与实际包证据见[库复用实施记录](implementation-standard-library-adoption.md#第六阶段socks5-报文库接入)。Check 按相关改动执行 Linux 普通网络/curl 互操作，私有 namespace 的 `network:netem` 仅在手动选择 `netem` 时执行，普通 `standard`/`all` 不包含弱网矩阵。工具前提与详细范围见[开发指南](development.md)；这些测试工具不进入发行包，核心发行构建仍独立于业务插件打包。工作流定义不能替代本次提交的实际运行结果。

Windows ARM64 runner 的 OpenSSH 系统可选组件安装曾持续等待十余分钟；该测试在缺少 sshd 时使用微软 Win32-OpenSSH 的固定版本 ARM64 便携包并校验 SHA-256，仅启动隔离回环服务，不安装系统服务，也不随产品分发。其他平台使用 runner 系统包提供的 OpenSSH。

原生远程验收的 Linux 运行依赖安装前使用 `configure-ubuntu-mirror.mjs` 切换到官方 `archive.ubuntu.com`，兼容传统 `.list`、Deb822 `.sources` 和 hosted runner 的 `mirror+file` 入口；ARM64 ports 和第三方源保持原地址。Check 复用该入口，依赖列表、有限下载重试和任务超时继续生效。

服务器管理验收：`npx playwright install chromium` 后运行 `npm run ui:test`，以无头浏览器调用真实目录核心，测试数据保存在临时目录，不连接服务器。Check 在相关核心/UI 改动和手动 `standard`/`all` 时于 Linux x86_64 执行完整回归；手动 `desktop` 保留 Windows 对照，自动 Windows 检查保留系统相关 SSH。CI 使用 `ci:test` 保存结果、耗时、日志及核心长 UI 的失败 trace；核心打包不安装 Chromium 或终端 UI 测试依赖。所有五目标的 `scripts/smoke.mjs` 都会在实际包中保存服务器、分组、代理及混合路径，再重开验证持久资料与临时秘密生命周期。

`npm run ui:test` 还会运行 `npm run ui:production:test`：先生成正式前端产物，再直接加载 dist 中的 HTML/JS/CSS，配合隔离的真实目录核心检查保存主题、服务器/代理表单、滚轮、底部字段、焦点和固定页脚；覆盖浅色、深色、多巴胺及 1100×720 / 720×480。此检查用于发现开发模块样式加载顺序与正式打包顺序不同造成的回归，不以开发服务器的截图代替正式产物。

v0.4.0 起同一入口还运行工作区界面测试；所有原生包烟测增加真实多 WebView 的移窗、合并、独立窗口关闭和空窗口回收，并用实际窗口几何验证 Tab 条和无效区域。烟测不操作指针，因此不能替代跨窗手势与混合 DPI 的人工验收。Wayland 全局坐标不足时使用菜单移窗。测试会话仅来自独立 fixture 或显式烟测入口的隔离资料；普通发行界面不注入示例连接。

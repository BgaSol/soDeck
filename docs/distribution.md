# SoDeck 安装、升级与插件分发

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

核心版本唯一来源为 `src-tauri/tauri.conf.json`。设置 → 应用更新提供手动检查、版本说明、下载进度、安装并重启。下载安装前不会主动停止插件；安装前正常退出全部插件，数据保持在系统应用数据目录。v0.1.1 没有更新模块，需要手动安装一次 v0.2.0。

更新使用固定版本的 Tauri 官方 updater，它要求更新包签名。公钥提交到 Tauri 配置；私钥保存在维护者用户目录 `.sodeck-signing/updater.key` 并配置为 Actions 的 `TAURI_SIGNING_PRIVATE_KEY` Secret，绝不提交私钥。密码可用 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` Secret；本项目初始私钥无密码，Secret 留空即可。此签名与 Windows/macOS 商业代码签名、插件权限无关。必须备份原私钥，随意换公钥会使旧客户端无法升级。

本地普通构建不需要签名密钥：

```sh
npm ci
npm run check
npm run scripts:test
npm run test
npm run build -- --bundles nsis -- --locked
```

正式更新包添加 `--config src-tauri/tauri.release.conf.json` 并通过环境提供签名私钥。Linux 使用 `--bundles deb,appimage`，macOS 使用 `--bundles dmg,app`，显式生成安装盘和签名更新归档。Node 24.22.0、Rust 1.99.1 与锁文件固定；Linux 运行基线仍为 Ubuntu 22.04/24.04 LTS。

## 插件生命周期与更新

- v0.5.2 起，设置 → 插件管理列出核心声明的官方来源。点击“下载并启用”从正式插件 Release 取得本机原生归档，复用更新器的 API/平台、SHA-256、解包和清单检查后注册为启用；安装期间不启动程序，重启生效。首次安装失败不登记半成品，可重试；同 ID 已安装时转到详情管理，不覆盖本地记录或更改其来源。列表不在后台自动下载，不建立插件市场。
- 设置 → 插件管理 → 添加插件目录，选择带 `plugin.json` 的外部目录，保存为启用但等待重启。旧记录缺少 enabled 字段时按关闭迁移。
- 启用/关闭只改期望状态，当前进程继续运行；重启统一生效，启动自动加载启用项。可以连续修改多个插件再重启一次。
- 下载先完成校验和解压，再保存新目录记录；已启用或本次启动已加载的插件在下次启动切换。完全关闭的插件立即更新记录，仍不启动。
- 在线程序位于 `<应用数据>/plugins/<插件ID>/<版本-唯一标识>/`。原外部目录保留；数据始终在 `plugin-data/<插件ID>/`。旧程序保留用于新版初始化失败后的恢复；不承诺数据迁移回退，不建立多级版本事务框架。
- 没有来源的本地插件支持手动更新。详情可显式配置发行仓库与标签前缀；旧 Hello 使用 `BgaSol/soDeck` 与 `plugin-hello-v`。不根据 ID 猜测来源。
- 关闭并重启后才能移除记录，不删除程序、数据、消息和日志。当前不提供运行中热加载/卸载的用户入口。

## 仓库维护

每个 Rust 插件在 `plugins/<名称>/` 包含：Cargo.toml（唯一版本来源）、Cargo.lock、release.json（ID、名称、API、二进制名、标签前缀）、CHANGELOG.md 和 src/。现有模板为 Hello。新增其他语言插件时再扩展构建适配，不提前建立通用构建框架。

组件宿主示例 `ui-example` 使用同一独立打包入口：`npm run plugin:build -- --plugin ui-example`。它另包含可选 `release.json.ui` 声明和 `ui/` 静态资源，打包自动复制，不增加前端构建工具或生产依赖。Windows 与 Hello 一样静态链接 CRT。`Verify packaged core and plugin` 现在在五个原生 runner 上独立构建该示例，并以 `SODECK_TEST_UI_PLUGIN` 验证已安装核心的界面加载与移窗；核心打包本身仍不依赖示例插件。

```sh
# 本地插件打包，默认 hello
npm run plugin:build
npm run plugin:build -- --plugin hello --out-dir artifacts/another-build
# 发布核心
# 修改 tauri.conf.json 版本及 docs/releases/v<版本>.md 后提交
 git tag v0.2.0
 git push origin v0.2.0
# 独立发布插件
# 修改 plugins/hello/Cargo.toml 版本、锁文件和 CHANGELOG 后提交
 git tag plugin-hello-v0.1.1
 git push origin plugin-hello-v0.1.1
```

普通提交只运行相关检查。`Package SoDeck` 匹配 `v*`，`Package plugin` 匹配 `plugin-*-v*`；两者也支持手动生成 Artifact。插件手动入口可选 plugin 和 publish（生成独立草稿）。标签必须匹配对应唯一版本来源。

各目标使用原生 Runner 构建和上传，某目标失败不阻塞其他成功产物下载，更新清单只列实际成功产物。五目标都是正式支持目标，未完成项必须在交付记录明确列出。草稿附件允许重试构建后补齐；发布后的同版本附件不覆盖，修正升补丁版本。检查附件和结果后将草稿正式发布，应用才能发现它。

每个核心 Release 含 `core-update.json`，每个插件 Release 含 `plugin-update.json`；脚本从构建产物自动生成，包含组件 ID、版本、按 Rust target 区分的下载地址，以及核心签名或插件 SHA-256。插件还有核心 API 版本。版本和清单不手工维护两份。

应用分页读取 Releases，筛选组件自己的标签前缀、正式版本和本机平台；插件还检查 API。不会用全仓库 latest 混淆核心和不同插件。插件 Release 发布时标为非 latest，核心保持仓库默认发行入口。检查和下载失败显示原因与重试，不改旧程序。

## 验证

真实连接的新增依赖与本机验证范围见 [连接实施记录](implementation-connections.md)。`npm run ssh:test` 使用临时回环 OpenSSH 服务验证实际客户端，`npm run ssh:test -- --packaged <安装后的程序>` 进一步验证包内连接与原生窗口移动。Windows x86_64 工作流执行这两层；其他目标继续原生构建/隔离 Rust 连接测试及包内启动，不将本机结果外推为五平台已通过。

本机执行类型、格式、脚本测试、Rust 集成测试、前端构建和本机安装包烟测。Rust 测试使用本机 HTTP 服务器验证下载、哈希、平台/API 不兼容、重启切换和启动失败恢复；正式更新来源只允许 HTTPS。

核心工作流在五个原生目标运行 Rust 测试，再安装/解包 EXE、DEB、DMG 后启动包内程序；AppImage 另检查启动和退出。`Verify packaged core and plugin` 下载实际核心与插件产物验证日志、消息、目录、状态和重启保留。CI 包内启动不等价于真实设备的交互式安装升级，实际完成范围记录在对应版本说明。

真实业务插件使用独立入口 `npm run plugin:build -- --plugin terminal` 和 `--plugin files`，版本分别来自自己的 Cargo.toml，标签为 `plugin-terminal-v<版本>` / `plugin-files-v<版本>`。终端打包在插件目录内执行锁定的 npm 安装和静态资源构建；核心打包不调用这些步骤。普通构建不创建发行版本，官方列表登记来源后仍需发布对应的独立 Release 才能下载。

`Verify native remote plugins` 手动输入核心、终端、文件三个成功打包运行的 ID，在五个原生 runner 下载实际包。它安装/解包核心、加载两个独立插件、连接隔离 OpenSSH，验证实际 WebView/Channel/xterm 的 PTY 往返、文件读取/保存/下载、窗口移动以及重开退出。它独立于核心打包，不阻塞无插件核心的构建与启动。源码侧 `npm run terminal:test` / `npm run files:test` 另验证多 Tab、草稿、保存关闭、冲突、大小边界、取消和重试。

Windows ARM64 runner 的 OpenSSH 系统可选组件安装曾持续等待十余分钟；该测试在缺少 sshd 时使用微软 Win32-OpenSSH 的固定版本 ARM64 便携包并校验 SHA-256，仅启动隔离回环服务，不安装系统服务，也不随产品分发。其他平台使用 runner 系统包提供的 OpenSSH。

服务器管理验收：`npx playwright install chromium` 后运行 `npm run ui:test`，以无头浏览器调用真实目录核心，测试数据保存在临时目录，不连接服务器。核心打包工作流在 Windows x86_64 执行此检查；所有五目标的 `scripts/smoke.mjs` 都会在实际包中保存服务器、分组、代理及混合路径，再重开验证持久资料与临时秘密生命周期。

`npm run ui:test` 还会运行 `npm run ui:production:test`：先生成正式前端产物，再直接加载 dist 中的 HTML/JS/CSS，配合隔离的真实目录核心检查保存主题、服务器/代理表单、滚轮、底部字段、焦点和固定页脚；覆盖浅色、深色、多巴胺及 1100×720 / 720×480。此检查用于发现开发模块样式加载顺序与正式打包顺序不同造成的回归，不以开发服务器的截图代替正式产物。

v0.4.0 起同一入口还运行工作区界面测试；所有原生包烟测增加真实多 WebView 的移窗、合并、独立窗口关闭和空窗口回收，并用实际窗口几何验证 Tab 条和无效区域。烟测不操作指针，因此不能替代跨窗手势与混合 DPI 的人工验收。Wayland 全局坐标不足时使用菜单移窗。测试会话仅来自独立 fixture 或显式烟测入口的隔离资料；普通发行界面不注入示例连接。

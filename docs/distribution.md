# SoDeck 安装、升级与插件分发

## 原则

核心独立安装和升级，业务插件全部外置。源码和发行包位于公开的 BgaSol/soDeck，使用 GitHub Actions 与普通 Releases。核心只依赖自身运行资源；插件分别版本化、构建和发行，不建立联动发布框架。

用户安装和首次使用见[使用指南](usage.md)，启停和移除见[插件管理](features/plugins.md)，更新操作及失败恢复见[在线更新](features/updates.md)。本文维护产物、发布流程和交付证据规则。

## 核心

| 目标 | Rust target | 发行包与在线升级 |
| --- | --- | --- |
| Windows x86_64 | `x86_64-pc-windows-msvc` | NSIS EXE；应用内下载、安装并重启 |
| Windows ARM64 | `aarch64-pc-windows-msvc` | 原生 ARM64 NSIS EXE；同上 |
| Linux x86_64 | `x86_64-unknown-linux-gnu` | DEB 走系统包管理；AppImage 支持应用内升级 |
| Linux ARM64 | `aarch64-unknown-linux-gnu` | 原生 ARM64 DEB 与 AppImage；同上 |
| macOS ARM64 | `aarch64-apple-darwin` | DMG 首次安装；app.tar.gz 用于应用内更新 |

版本唯一来源为 [tauri.conf.json](../src-tauri/tauri.conf.json)。构建使用仓库工具链与锁文件，命令统一见[开发指南](development.md#打包)。Linux 运行基线为 Ubuntu 22.04/24.04 LTS。用户运行发行程序无需开发工具链。

正式核心更新包使用 [tauri.release.conf.json](../src-tauri/tauri.release.conf.json) 和固定的 Tauri 官方 updater 签名。公钥提交到 Tauri 配置，私钥保存在维护者用户目录 .sodeck-signing/updater.key，并配置为 Actions 的 TAURI_SIGNING_PRIVATE_KEY Secret，绝不提交私钥；密码使用 TAURI_SIGNING_PRIVATE_KEY_PASSWORD，初始无密码密钥留空。必须备份原私钥，更换公钥会影响旧客户端升级。此签名与商业代码签名或插件授权无关，本地普通构建无需私钥。

发行清单始终保存原始 GitHub Release URL、版本、平台和哈希/签名；客户端备用传输地址不改变发行来源。核心下载、安装和已准备包的离线验签恢复继续使用官方 updater。插件使用归档 SHA-256 和基本清单检查，不增加插件签名信任链。

### Windows 安装范围与图标

Windows x86_64 与 ARM64 的 NSIS 新安装统一使用 `perMachine`，默认目录为 `Program Files\SoDeck`，安装登记写入 HKLM，供所有用户启动。安装、卸载和核心在线更新需要管理员确认；应用自身仍以普通用户身份运行，服务器资料、凭据、设置和在线插件保存在各用户的应用数据目录，不改为全机共享。插件更新不写核心安装目录。

已有用户安装切换为系统安装时，先退出 SoDeck，卸载旧用户安装，**不要勾选删除应用数据**，再运行新安装包。同一 Windows 用户重新启动后继续使用原资料和插件。新系统安装器只读取系统安装登记，不自动清理旧用户安装；仅执行在线更新可能保留旧程序和旧安装登记。切换安装范围按上述手动重装步骤完成。

应用、快捷方式、安装程序和卸载程序共用 `src-tauri/icons/icon.ico`。安装向导的页眉与欢迎/完成页侧图复用现有 `icon.png` 的 SoDeck 标识，分别提交为 `installer-header.bmp`（150 × 57）与 `installer-sidebar.bmp`（164 × 314），均为 24 位 BMP。后续更换品牌时同步这些图片及已有各平台图标；正常构建直接使用仓库资产，无需额外图片工具。NSIS 配置位于 `src-tauri/tauri.conf.json`；官方 updater 的 `passive` 设置控制更新界面的交互方式，与安装范围不同。

## 插件仓库与资源

每个 Rust 插件在 plugins/<名称>/ 包含 Cargo.toml（唯一版本来源）、Cargo.lock、release.json、CHANGELOG.md 和 src/，UI 插件同时携带已构建的 UI 资源。当前目录为 terminal、files、network-proxy、system-status 和 docker。新增语言时再扩展构建适配。

release.json 同时是官方发现元数据和独立发行配置，不另建集中名单。ID、名称、标签前缀必填，API 和二进制配置用于打包；description、icon、order 用于展示。默认图标为通用图标、顺序为 0 再按 ID 排序，ID 和标签前缀在官方配置中各自唯一。完整字段和校验见[发行脚本](../scripts/package-plugin.mjs)及[官方目录实现](../src-tauri/src/official_plugins.rs)。

新增官方插件先将目录和发行配置合入 main，再用 Package plugin 发布自己的正式 Release。目录刷新无需重新发行核心；有兼容 API 和本机原生包的正式 Release 才能安装。移除发行配置后，已有用户的插件记录和更新来源继续保留。

插件版本由 cargo metadata 按清单路径解析，支持合法 TOML 与 workspace 版本继承，不从文本猜测；选择、打包和汇总任务均安装固定 Rust 工具链。插件标签为 plugin-<名称>-v<版本>，必须匹配其唯一版本来源。

插件打包使用根目录锁定的 Vite/Floating UI 生成自带文本菜单；终端 xterm 和文件 Monaco 使用各自 npm 锁文件构建。无 CDN 或用户运行时 npm 依赖。终端和文件各自携带字体、来源校验元数据与许可证；普通构建只校验和复制资产，维护时才显式运行 prepare-font-assets.mjs，核心包不携带这些插件字体。

Windows 插件在自身工程目录运行 Cargo，以应用静态运行库配置；归档前检查原生架构及额外 Visual C++ 运行库依赖。用户不需另外安装 Visual C++ 运行库。宿主测试夹具不发行，已退役的 Hello/组件交互示例不重新加入官方目录。

## 发布操作

普通提交只运行按改动选择的检查。核心 Package SoDeck 与独立 Package plugin 支持手动打包；publish 默认关闭，开启后准备草稿，插件还需指定 plugin。无现成候选时也可使用匹配版本的 v* 或 plugin-*-v* 标签触发打包；已有相同源码候选时，优先复用，避免重复编译。

核心轻量 preflight 检查版本、发布说明、草稿状态和标签提交。package 只构建、签名和归档，随即上传含提交、版本、平台、大小与摘要的 Artifact；独立 verify 下载原包，验证实际程序。metadata 核对五平台成功报告与同源包字节、生成清单并准备草稿；同版本写入串行，已有同名附件须摘要一致，正式版本拒绝覆盖。分支且 publish 关闭时只输出 Artifact 与清单。

优先在 main 手动打包。成功的 main 运行保存各平台 core-release-v2 依赖缓存，适用分支或标签恢复同引用/默认分支缓存；该键不随版本、提交或 run ID 改名。缓存仍按工具链、系统和架构隔离，只缓存依赖，不缓存 workspace crate 或 incremental；恢复后照常安装锁定依赖、构建本次前端与核心。失效或容量清理导致未命中时按固定输入正常编译。日常检查缓存规则见[开发指南](development.md#版本与持续集成)。

### 一次打包完成核心发布

| 当前情况 | Actions 操作 | 后续操作 |
| --- | --- | --- |
| 新版本尚未打包 | 在 main 运行 `Package SoDeck`，开启 publish，留空 `core_run_id` | 五平台构建、实际包验证后生成草稿；确认同一源码的相关检查，直接发布该草稿 |
| 已有候选包，需要准备草稿 | 运行同一工作流，开启 publish，填写**原始五平台打包运行**的 `core_run_id` | 所有 package 任务 skipped；下载、复验原包并准备草稿 |
| 只想验证已有包的汇总 | 填写原始 `core_run_id`，关闭 publish | package 全部 skipped，复验原包并生成更新清单，不创建 Release |
| 草稿已齐全 | 打开该草稿，核对版本、源码和五平台附件 | 直接正式发布；不再推相同版本标签启动另一轮构建 |

工作流标题区分 `Package core` 与 `Reuse core`。metadata 汇总链接到**包的原始 run 和提交**；复用操作本身的提交用于执行脚本，不能替代原包的源码检查结果。所选分支/标签的核心版本和发布说明必须与候选版本一致。`core_run_id` 不填写只有清单的复用运行，也不自动选最近一次候选；包内记录的提交、目标、版本、文件大小和 SHA-256 均须匹配。

独立核心基础验收使用 0.23.1 起的 `packageNative` 报告。更早候选包须使用其来源提交中的兼容工作流；新入口会明确失败，不把旧包缺少的新断言视为通过。已发布旧版本仍可作为远程组合或升级专项的输入。

首次开启 publish 时，脚本创建指向原包提交的普通版本标签和 Release 草稿；Actions token 创建的标签不会再触发一轮标签打包。已有正式版本拒绝覆盖；需要修正发行内容时升新版本重新打包。插件可以在首次独立构建时开启 publish，直接发布同一份草稿归档，核心复用入口不扩展为插件联动发布。

### 失败时只重试需要的部分

发布收尾失败且 Artifact 未过期时，在 Actions 重跑失败的 metadata job，复用同一 run 的已验证包；不重新运行已成功的平台。若发布脚本需要修复后再恢复，手动运行 `Package SoDeck`，填写原始 `core_run_id` 并开启 publish：前置任务确认该运行属于本仓库核心工作流、已结束且 Artifact 尚在，原生构建全部跳过，verify 复验原包，汇总按原始提交核对产物并恢复草稿。版本须与这些产物一致。某平台构建失败时只重跑失败目标；verify 失败时重跑失败的 verify 和后续汇总，不重编译。已有草稿的同名包必须与当前输入一致，不以覆盖旧附件解决提交或摘要冲突；过期/缺失产物需要重新构建固定版本与源码。

新上传的核心包、独立插件包和核心更新清单 Artifact 保留 30 天，用于验收和发布收尾重试；已有 Artifact 沿用创建时的期限。超过窗口需要重新构建固定源码并重新验证，不能用另一运行或另一版本的包代替。正式安装与更新下载仍使用 Release 附件，不依赖临时 Artifact 的保留期。插件沿用现有独立打包/发布入口，没有核心 `core_run_id` 恢复选项。

各目标使用原生 Runner 构建和上传，某目标失败不阻塞其他成功产物下载。五目标都通过独立包验收后才生成本次清单和草稿；局部通过不能视为正式交付。草稿附件允许上传失败后按相同摘要补齐；发布后的同版本附件不覆盖，修正升补丁版本。检查附件和结果后将草稿正式发布，应用才能发现它。

每个核心 Release 含 `core-update.json`，每个插件 Release 含 `plugin-update.json`；脚本从构建产物自动生成，包含组件 ID、版本、按 Rust target 区分的下载地址，以及核心签名或插件 SHA-256。插件还有核心 API 版本。版本和清单不手工维护两份。

官方更新通道只接受不含预发布标识或构建信息的 SemVer `major.minor.patch`；当前已安装版本和候选版本都须满足此限制。

应用按组件仓库和标签前缀读取轻量标签引用，再按版本降序确认单个 Release，只读取首个公开正式发行的更新清单；缺席顺延、兼容检查及失败边界见[在线更新](features/updates.md#最新版本发现)。不依赖全仓库 latest 区分核心和不同插件。插件 Release 发布时标为非 latest，核心保持仓库默认发行入口。

## 验证

源码检查和打包分别执行。Check 的选择与手动套件见[开发指南](development.md#版本与持续集成)，发布前确认**包来源提交**的相关检查结果；打包不等待 Check，不用复用脚本或文档提交替代包源码。netem 仅手动专项，工作流存在、任务 skipped 或历史通过均不代表本次验收通过。

| 层次 | 实际范围与限制 |
| --- | --- |
| 源码与浏览器 | 格式、类型、脚本、Rust 及相关真实业务；正式前端产物回归不能替代原生 WebView、系统指针/IME/DPI |
| 核心实际包 | 五原生目标构建、签名，安装/解包 EXE、DEB、DMG 后启动包内程序，AppImage 和 macOS updater tar 另验启动退出；保存资料、重开和退出；不运行 SSH/工作台/默认应用业务 |
| 独立插件包 | 每个插件五平台原生产物及其包内程序启动、生命周期、资源和退出；系统状态/Docker 的进程验收不等于所有真实远端系统或 Engine 验收 |
| 远程实际包组合 | Verify native remote plugins 核验明确来源的核心、终端、文件及可选网络包，在原生 WebView/Channel 下验证 PTY、读写下载、网络、移窗与重开退出 |
| 升级 | Verify published release upgrade 使用明确旧/新正式版本，验证生产更新服务、官方 updater、实际安装字节和资料保留；Linux 另验 DEB 系统升级，不代替完整桌面操作 |
| 系统交互专项 | 原生文件输入、默认应用、关闭手势、编辑器 Worker、真实 IME/DPI/读屏等按所选专项记录，不从普通包启动推定通过 |

Verify native remote plugins 的 core_version、terminal_version、files_version 和可选 network_version 使用精确版本；各组件允许来自不同提交。已发布版本读取精确标签的正式 Release，缺包或错误不回退到实验包。候选可填写各自原始 *_run_id；版本同时填写必须匹配。候选须属于本仓库对应工作流，运行已结束、五个平台 package job 成功、新流程还要求五个 verify job 成功且 Artifact 有效（历史流程的验收位于 package 内）；发布汇总失败不使已验证的包失效。多个候选不自动取最近一次。

工作流固定 native-inputs.json，保存附件/Artifact ID、源码、版本、平台和摘要，再安装或解包。网络输入为空时明确记为未参与。现有组合入口不包含系统状态和 Docker，不能因此声称它们也完成同轮组合验收。

platform 默认 all；补测可选单个平台，重跑复用固定输入。新建运行重放时填写 verification_run_id，其余版本/run 输入留空；原运行须已结束并保有唯一输入快照，业务失败的运行也可重放原包。单平台通过只代表该平台，交付记录同时保留其他平台的成功报告。源包缺失、过期或摘要变化明确失败，不自动换包。输入快照和验收结果保留 30 天，不修改原始包或 Release。

平台包和更新清单元数据按结构及字段值复验；对象键顺序不影响结果，数组顺序、字段类型和字段值必须一致。

原生文件剪贴板 desktop_files、默认应用 desktop_open 与 Windows 深度编辑器 file_editor 默认关闭；明确开启后失败仍使工作流失败。编辑器自动化需要 WebView2 调试端口。系统默认应用验收区分实际目标收据与关联受保护的 skipped，不能把启动命令成功等同目标应用收到请求。隔离前提、工具和运行命令见[开发指南](development.md#检查与测试)。

Windows ARM64 的隔离 OpenSSH 在缺失时从固定 Microsoft Release 下载原生便携包，核对大小与 SHA-256，不安装系统服务或随产品分发。Linux 依赖安装使用官方软件源修正和有限重试；这些测试准备不改变产品依赖。

## 交付记录

核心记录写入 docs/releases/v<版本>.md，插件写入自身 CHANGELOG。每次保留：组件版本、包来源提交、原始打包 run 链接、Release 链接，以及实际验证、失败重试和跳过范围。待发布草稿也保留这些字段，后续会话才能核对同一份包。

核心与插件各自记录版本和验收，不要求同时升版。源码完成、候选打包、正式发布分别陈述；历史记录链接固定提交中的材料，当前行为链接正式功能文档。CI Artifact 会过期，必要结论与验证范围必须直接留在版本记录中。

返回[文档索引](README.md)。

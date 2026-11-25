# 登录后台启动整改与剩余验收

状态：2026-11-26，核心 2.5.0 已正式发布为 Latest，本轮源码、原生程序与五目标实际包验证通过。行为归入[桌面设置](../features/settings.md#登录后自动启动)，职责归入[架构](../architecture.md)，安装与清理归入[分发](../distribution.md)，当前结果归入[2.5.0](../releases/v2.5.0.md)。原设计与代码整改方案通过 Git 历史查询，不另建完成归档。

## macOS 登录项图标整改方案

本节正在实施，当前代码与验收进度见[2.5.1](../releases/v2.5.1.md)。用户已确认：系统设置的“登录项”中，SoDeck 的图标显示不正确，希望使用与正式应用一致的图标并保持登录后台运行；系统界面与真实注销登录仍须分别记录证据。

### 调查结论与问题边界

现有调用链：应用设置开关 → `main.rs` 的 `set_login_startup` → `login_startup::Service` 串行修改 → macOS 适配器生成 `~/Library/LaunchAgents/com.bgasol.sodeck.plist` → `ProgramArguments` 直接执行 `.app/Contents/MacOS/sodeck --login-start` → 桌面启动准入 → 主窗口后台呈现。

已从 2.5.0 的正式 updater 归档核对：

- `CFBundleIdentifier=com.bgasol.sodeck`，`CFBundleName` 与 `CFBundleDisplayName` 均为 `SoDeck`。
- `CFBundleIconFile=icon.icns`，`Contents/Resources/icon.icns` 存在，SHA-256 与仓库原图标一致；不能将问题直接归因为图标未打包。
- 当前 LaunchAgent 已有 `AssociatedBundleIdentifiers`；再添加同一个字段不能解决问题。代码未显式核对完整 app bundle 的身份，也未建立可验证的 Launch Services 关联。
- 当前 macOS 使用 ad hoc 签名。Apple 对旧 LaunchAgent 的应用归属还要求 Launch Services 能找到目标应用，且可执行文件与关联应用的 Team Identifier 符合要求。具体是哪一环导致用户机器上的显示错误，仍需原生复现确认，不能断言 ad hoc 一定是根因。
- 状态读取只查询 plist 和 `launchctl print-disabled`，未使用 macOS 13+ 的 Service Management 状态；当前原生包烟测也只验证隔离 plist 的内容，未验证系统登录项中的应用身份和可见图标。

整改目标是让系统登记、状态查询和展示归属围绕同一个 SoDeck.app 工作。验收包括正确图标和名称、登录后台、手动打开、用户禁用、旧登记迁移和原地更新。系统可能保留历史记录；不能要求清空所有历史条目或保证系统缓存立即刷新。

### 推荐实现

macOS 13+ 使用 Apple 的 `SMAppService.mainApp` 登记主应用，调用原生 `register`、`unregister` 和 `status`。它直接代表主应用作为登录项，注册只影响后续登录，取消登记不会结束当前主应用，适合现有单进程产品边界。

当前 ARM64 产品并未约定 macOS 13 起步，包中的 `LSMinimumSystemVersion` 仍为 Tauri 默认的 10.13。不能为图标问题静默提高最低版本：通过运行时 API 可用性选择，macOS 11/12 继续走原有 LaunchAgent 兼容路径；macOS 13+ 只使用原生主应用登录项，旧 LaunchAgent 只用于识别和清理。低版本路径也采用完整 bundle 校验，并在明确登记操作时确认 Launch Services 归属。

复用已安装的 `objc2`、Foundation 和 AppKit，通过一个私有 macOS 原生桥接调用系统 ServiceManagement/CoreServices；必要时只启用现有 crate 的功能。`unsafe`、framework 链接和运行时可用性检查集中在平台模块。不增加生产依赖包、Swift 子进程、独立 helper、守护进程或自动重启策略。

先在原生 Mac 上用当前 ad hoc 签名与真实安装包验证上述 API、图标归属和登录来源时机，再实施完整切换。若需要 Developer ID 才能满足验收，明确记录实际结果和发行前提，不顺带加入商业签名/公证系统，也不以清理系统缓存冒充修复。

### 代码职责与调整范围

| 位置 | 调整职责 |
| --- | --- |
| `src-tauri/src/login_startup/macos.rs` | 作为 macOS 入口，负责 bundle 身份、平台选择、系统状态与有限旧登记迁移；停止把“plist 存在”当作现代系统的开启事实 |
| `src-tauri/src/login_startup/macos/native.rs`（新增） | 封装 Service Management、Launch Services 和启动 Apple Event 的小型原生桥接；隔离 `unsafe` 与版本检查 |
| `src-tauri/src/login_startup/macos/legacy.rs`（按需从现有代码移入） | 原有 plist IO、归属验证与旧系统支持；保留超时、错误处理和系统禁用识别，不形成可扩展后端框架 |
| `src-tauri/src/login_startup.rs` | 保留公共状态契约、修改串行、隔离限制与退出收束；只有原生线程要求确有需要才调整调度，不让 Windows/Linux 接入新框架 |
| `src-tauri/src/desktop_startup.rs`、`src-tauri/src/main.rs` | 在原生启动阶段合并一次性登录来源，继续复用现有独占、第二实例、重启及呈现逻辑 |
| `src/application-settings.ts` | 复用现有开关、状态与反馈；仅在旧项需要迁移时提供一个“更新登录项”操作，调用现有设置命令 |
| `src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json` | 必要的现有依赖功能与系统链接；图标、应用标识、最低系统版本和签名策略不凭推测修改 |
| `scripts/test-core-package.mjs`、设置回归与原生夹具 | 补包身份检查、状态/迁移回归和真实原生专项，明确区分静态检查、模拟事件和实际登录 |

不引入通用自启动管理器。只有 macOS 原生事实和兼容逻辑需要整理；核心服务、插件业务、连接协议与其他平台启动机制保持原职责。

### 应用身份校验

从运行中的主 bundle 取得 URL、Bundle ID 和主可执行文件，校对它们与当前进程、Tauri 配置一致。确认图标声明指向包内现有资源；登记的是当前应用副本，不按显示名称搜索另一份 SoDeck。

安装位置检查替换现有 `.app/Contents/MacOS/` 字符串包含判断，结合 bundle 结构、实际路径和系统 Translocation 状态拒绝 DMG、临时目录及 App Translocation。允许稳定本地目录，不硬编码只能安装到 `/Applications`；开发和隔离模式继续禁止修改真实登录项。

明确登记时使用系统支持的 bundle 登记 API 保证 Launch Services 知道当前应用，并传播错误。不得调用私有 `lsregister` 工具、扫描磁盘、重建全局数据库、清空图标缓存或修改其他应用的关联。

### 保持登录后台与普通打开行为

`SMAppService.mainApp` 由系统打开应用，不能继续假定会带 `--login-start`。在 macOS 的应用启动回调阶段读取启动 Apple Event，识别 `kAEOpenApplication` 与登录来源属性，立即复制为本次启动的布尔事实。当前锁定的 Tao/Tauri 链路从 `applicationDidFinishLaunching` 同步进入初始化/setup，具体读取位置必须由原生专项验证；不得放在 Rust `main()` 入口或稍后异步任务中猜测。

保留一次性来源，最终呈现意图按以下顺序确定：

1. 明确的替换重启按现有重启交接处理，不继承上次登录隐藏状态。
2. 已收到用户显式唤出请求时显示窗口。
3. 新原生登录来源或旧版 `--login-start` 来源进入现有后台呈现。
4. 其余普通打开显示窗口。

原生来源到达前仍允许进行不产生窗口的独占准入；来源抵达后决定首次呈现，不重新打开核心、不再取一次数据锁。macOS 现有事件循环之前的 Accessory 设置需合并到首次呈现决定中，避免形成两处互相覆盖的显示策略。测试早期激活/Dock 闪现与启动事件时序，不能只验证窗口最终隐藏。

`Reopen` 仍表示用户希望返回应用；不要把系统初始打开与之后的 Dock/Launchpad/Finder 打开混在一起。普通最小化、Cmd+H、必要认证、更新退出与托盘失败继续复用现有 `DesktopShell`。不替换 Tauri 的 application delegate，不按父进程名、进程启动时间或系统登录时间推测来源。

### 系统状态与旧版本迁移

现代系统的主要事实来自 `SMAppService.status`，不新增持久化开启偏好。复用公共状态：

| 系统事实 | 应用显示 |
| --- | --- |
| `enabled` | 已开启 |
| `notRegistered`，无有效旧项 | 已关闭 |
| `requiresApproval` | 已被系统限制或需要用户允许，显示系统设置位置；不声称开启成功 |
| `notFound`、调用失败或身份不符 | 无法确认或不可用，保留具体原因 |
| 仅存在可确认已开启的旧 SoDeck LaunchAgent | `stale`，保留原开启事实并提示“更新登录项” |

macOS 13+ 核对旧项时使用 `statusForLegacyPlist` 取得系统授权状态，结合旧文件配置判断，不能只把 `launchctl print-disabled` 没有匹配当成允许。迁移尚需系统确认且旧项仍有效时，整体保持 `stale` 与旧项实际开启事实，原因明确说明新项尚未允许；不能因新项未允许就谎称旧项已关闭，也不能声称迁移完成。

状态读取始终只读。升级启动、页面刷新和更新安装不自动重新注册；特别是被系统禁用或状态不明的旧项，不能借“迁移”重新开启。旧用户在现有设置中点击一次“更新登录项”完成迁移，无需增加第二个长期开关。

显式迁移顺序：

1. 校验旧文件的 Label、参数、可归属的 bundle 身份与固定位置。非本应用所有或无法确认时保留文件并报错，不能仅凭文件名删除。
2. 登记主应用并读回状态。只有确认 `enabled` 才继续清理旧文件；失败不删除旧文件，需要用户允许时说明真实状态，保留未完成迁移的事实。
3. 移除旧 LaunchAgent 的磁盘登记，使下次登录只剩新路径。不对正在托管当前 SoDeck 进程的旧 job 调用 `launchctl bootout`，避免迁移时杀死当前应用。
4. 如果旧文件清理失败，撤销本次新建登记，保留旧项并返回迁移未完成；撤销也失败时报告两份登记的实际结果，不显示成功、不无限重试。

这是一段限定旧版格式的迁移流程，无磁盘扫描、全局清理、迁移偏好或长期双写。迁移中只有新项已允许但旧项清理未完成的短暂阶段；完成后不保留两套可启动登记。失败时保留实际状态和重试入口。

用户明确关闭时，取消当前主应用登记并清理可确认归属的旧文件；重复关闭应幂等，部分失败后读回实际状态。关闭只影响后续登录，当前窗口、会话和进程继续运行。对同标识的另一安装副本采用归属检查，禁止随意删除其路径登记；多副本冲突需要明确原因，不能按名字猜测。

原地升级仍使用同一 Bundle ID，核验登记和图标身份保留；不在 updater 路径中重新开启系统禁用项。程序移动后按系统实际状态反馈，不扫描并悄悄重定向。取消或删除应用后的系统历史记录由系统管理，不以强制重置 BTM 数据库作为卸载步骤。

### 实施顺序与验收

1. **原生复现与最小验证。** 将当前正式包安装在稳定目录，记录 macOS 版本、登录项所在分区、名称/图标、实际 bundle URL、Launch Services 归属和签名信息。以同样签名方式验证主应用登记和启动事件时机，确认推荐路径可满足视觉与行为目标。
2. **整理 macOS 适配。** 完成 bundle 校验、系统 API、状态映射和低版本兼容。复用已有 Service 串行与退出收束，确认原生调用线程要求，不在持有锁时同步等待必须取得该锁的主线程任务。
3. **接通启动与迁移。** 合并首次呈现意图，补一次“更新登录项”的上下文操作，完成原子化意图的迁移及失败回退，校验取消登记不会影响当前工作。
4. **自动化回归。** 覆盖身份不符/特殊路径、系统四态、重复操作、迁移成功/拒绝/清理失败/回退失败、陌生旧文件保留、关闭与退出竞争、登录后重启和用户显式唤出优先。隔离模式不得登记真实用户登录项。
5. **实际包与系统界面验收。** 核对 DMG 和 updater app 的名称、标识、图标字节与包签名；在原生 ARM64 Mac 正式安装后检查系统设置图标，执行真实注销登录、外部关闭、重新允许、当前进程内关闭、2.5.0 迁移及原地更新。旧系统兼容路径单独记录结果。系统 UI 证据与 API/测试结果分别保存，不能从 plist 解析或模拟启动事件推定图标和实际登录通过。

实现时运行相关格式、类型、脚本、核心和设置 UI 测试；涉及共享启动代码的回归覆盖第二实例、数据锁与重启。发行时继续按既有五平台原生打包、签名和包内启动/退出要求执行；其他平台不增加业务功能。未执行的真实系统验证继续留在计划，未满足图标与真实登录验收不能宣布本项整改完成。

实施后同步 `docs/features/settings.md`、`docs/architecture.md` 中受影响的职责、`docs/distribution.md`、`docs/development.md` 的验收范围和新版本说明；根 README 无需修改。历史 2.5.0 的收据保留当时事实。完成后迁出必要证据，删除本节，全部剩余验收结束后删除本计划。

参考：[Apple SMAppService.mainApp](https://developer.apple.com/documentation/servicemanagement/smappservice/mainapp)、[旧机制迁移与应用归属](https://developer.apple.com/documentation/servicemanagement/updating-helper-executables-from-earlier-versions-of-macos)、[注册](https://developer.apple.com/documentation/servicemanagement/smappservice/register())、[取消登记](https://developer.apple.com/documentation/servicemanagement/smappservice/unregister())、[Launch Services 登记](https://developer.apple.com/documentation/coreservices/1446350-lsregisterurl)。

## 仍需核对

- 真实 Windows 两架构注销/登录及托盘返回；当前用户与所有用户安装的普通用户设置、系统启动项外部禁用、卸载归属和原地更新保留。隔离注册表 IO 与传入登录参数不能代替真实登录。
- 真实 macOS ARM64 Aqua 登录、菜单栏/Dock/Launchpad Reopen，登录项系统禁用与安装位置变化；RunAtLoad 不设置 KeepAlive，退出后不能在本次登录被再次拉起。
- Linux 两架构的真实有/无托盘桌面登录，DEB 与固定 AppImage 的实际自启动，系统禁用、桌面策略和长/特殊字符路径；Xvfb 参数启动不能代替桌面会话自启动。
- 从登录来源进入正式 Windows updater 直接结束进程的升级，与 Linux/macOS 安装返回后重启的实际交接；窗口、数据锁、系统登记和已有资料正确保留。普通替换重启不能替代安装器升级。
- 原生错误注入、未完成系统登记与退出交错、多窗口设置、读屏/IME/DPI；已有[托盘专项](system-tray.md)继续只维护原托盘范围。

每项实际验证与跳过范围写入版本记录，并从上面移除已完成条目。设备和实际登录专项未执行前继续保留本计划，不宣称整个验收已完成。全部约定验收完成后迁出必要证据、修复引用并删除本文件。

返回[文档索引](../README.md)。

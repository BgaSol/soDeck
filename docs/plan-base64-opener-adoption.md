# Base64 与系统打开库接入方案

2026-11-10。方案基于本次工作树，HEAD 为 `30d7861452e2c2ee01b2b3febba4003789f0288a`，包含已有未提交改动。本文是待实施方案；本轮只核对源码、上游发行接口并执行隔离的 Base64 对照，没有修改生产代码或新增生产依赖。

## 1. 结论与当前基线

建议分两个独立改动：先接入 Base64 库并修复 Docker 的宽松解码，再处理系统打开适配。每个改动分别提交、验收，核心与插件继续独立构建和发行。

本轮基线与 11 月 9 日早先审计有两处差异：

- Base64 有三处手写实现：核心 `session_exec.rs` 的编码、共享 `plugin-client/encoding.rs` 的编解码、Docker `commands.rs` 的解码。后两处都消费核心的命令输出，应使用相同规则。
- 系统打开已由[系统绑定整改](implementation-native-system-bindings.md)合并到 `desktop_open.rs`，使用 Windows Shell、AppKit、GTK 类型绑定。原来的 rundll32/open/xdg-open 三分支已删除。接入 opener 的收益是减少现有跨系统适配代码，不再以删除这些旧命令为目标，也不承诺减少安装包体积。

不把终端历史搜索、系统状态调度、流通道改造或 CI 提速的现有未提交改动混入本次提交。

## 2. 依赖选择及证据

| 用途 | 拟采用版本 | 选择依据与替代方案 |
| --- | --- | --- |
| Base64 | `base64 =0.23.1` | 当前核心锁文件已有该版本；发行于 2026-08-04，MIT OR Apache-2.0，最低 Rust 1.71。标准字母表、规范填充和非法尾位检查适合现有协议。[上游](https://github.com/marshallpierce/rust-base64)未归档，最近推送 2026-08-18。替代为现有 `base64ct`，但不通过 SSH 库的重导出让通用客户端依赖 SSH；本次统一手写实现使用独立编码库。 |
| 系统打开 | `tauri-plugin-opener =2.7.2` | 发行于 2026-10-14，MIT OR Apache-2.0；发行包 Cargo.toml 与精确版本 API 声明最低 Rust 1.90，兼容项目 1.99.1 和 Tauri 2.12.5。[官方插件仓库](https://github.com/tauri-apps/plugins-workspace)未归档，最近推送 2026-11-09。采用 Rust 自由函数，覆盖现有 Windows/Linux/macOS 桌面目标。 |
| opener 的底层打开实现 | 锁定解析到 `open 5.4.5` | 本轮核对了该发行源码；MIT，最低 Rust 1.62，[上游](https://github.com/Byron/open-rs)未归档，最近推送 2026-10-30。2.7.2 对 open 使用 `5` 范围，实施时必须核对实际锁定版本，不能只凭插件版本推断底层错误行为。 |

维护信息为本轮查询快照，不等于长期维护保证。实施时保存依赖清单、许可和锁文件差异，按实际解析结果复核最低 Rust 与 feature。正式构建使用仓库固定工具链和 `--locked`。

直接使用 `open` 可少引入一层插件依赖；保留当前系统绑定则无需新增打开库。本文给出官方 opener 的接入设计。实施前仍需量化新增传递依赖和包体变化：其清单包含 `open`、Linux `zbus`、Windows `windows` 等，不会因只调用自由函数就自动移除这些依赖。若实际包或平台行为出现回归，保留当前适配，并记录具体失败，不额外叠加一套自动回退后端。

## 3. Base64：协议与兼容行为

统一采用 `base64::engine::general_purpose::STANDARD` 和 `Engine`：

- 字母表为 `A–Z a–z 0–9 + /`，需要时使用结尾 `=` 填充；不切换 URL-safe、不插入换行、不接受非规范尾位。
- 空字符串表示空字节；解码返回 `Vec<u8>`，不在编码层转换成 UTF-8。stdout/stderr 中的 NUL、非文本字节和 UTF-8 跨页内容均原样保留。
- 共享客户端继续在调用库前检查编码输入长度：必须为 4 的倍数，且不超过 2 MiB。这是编码文本长度，最大解码结果为 1.5 MiB；不改成另一个输出限额。
- `session.exec.poll` 继续按原始字节偏移读取 stdout/stderr，每路每次最多 8192 字节；字段名 `dataBase64`、`nextOffset`、`outputComplete` 和 64 KiB 控制帧规则不变。
- 所有合法旧数据的编码字节完全一致，旧插件可读新核心，新插件可读旧核心；不升级核心 API 主版本。
- Docker 改为严格拒绝畸形 Base64。这属于缺陷修复：原实现遇到第一个 `=` 就停止，不核对剩余字符、填充或尾位，可能静默返回部分命令输出。

共享函数仍导出 `base64(&[u8]) -> String` 和 `unbase64(&str) -> Result<Vec<u8>, String>`，终端和系统状态调用方无需迁移。长度错误保留“Base64 长度无效”；其他非法编码统一返回“Base64 编码无效”。本轮检索未发现按旧错误文字分支的调用方，实施时复核；不把库错误或原始命令输出拼进诊断。Docker 在消费处继续将解码失败映射为“核心输出编码无效”，保留“缺少 stdout/stderr”等字段错误。

## 4. Base64：具体改动

### 4.1 依赖与实现

核心和共享客户端分别声明直接依赖：

```toml
base64 = { version = "=0.23.1", default-features = false, features = ["std"] }
```

共享客户端只需标准分配接口，不主动启用可选 SIMD。核心其他依赖可能通过 Cargo feature 合并启用同版本的其他 feature，不能把上述声明当作全依赖树关闭 SIMD 的证明；使用 `cargo tree -e features` 核对实际情况。

共享 `encoding.rs` 保留小型边界包装，删除字母表、位移、字符查找和手写填充逻辑：

```rust
use ::base64::{engine::general_purpose::STANDARD, Engine};

pub fn base64(bytes: &[u8]) -> String {
    STANDARD.encode(bytes)
}

pub fn unbase64(input: &str) -> Result<Vec<u8>, String> {
    if !input.len().is_multiple_of(4) || input.len() > 2 * 1024 * 1024 {
        return Err("Base64 长度无效".into());
    }
    STANDARD.decode(input).map_err(|_| "Base64 编码无效".into())
}
```

核心 `session_exec.rs` 直接用 `STANDARD.encode` 构造两路输出，删除私有 `fn base64`。核心不增加对插件客户端 crate 的依赖；两个进程组件分别使用同一第三方库。

Docker `runtime.rs` 的 stdout/stderr 两处改用 `sodeck_plugin_client::encoding::unbase64`，删除 `commands.rs::base64`。继续执行已有本地总输出预算检查与任务释放，不因解码失败重发 Docker 命令。

### 4.2 文件、调用方及锁文件

| 文件/范围 | 修改或核验 |
| --- | --- |
| `src-tauri/Cargo.toml`、`src-tauri/Cargo.lock` | 增加 Base64 直接依赖关系；不整体升级已有依赖。保留仍被其他库使用的 base64 0.21.7、base64ct 等包。 |
| `src-tauri/src/session_exec.rs` | 替换编码、调整原有私有函数测试；使用 RPC 层测试验证分页与字节结果。 |
| `crates/plugin-client/Cargo.toml`、`Cargo.lock`、`src/encoding.rs` | 增加固定依赖，保留函数签名、前置长度限制和中文错误，补充边界回归。 |
| `plugins/docker/src/commands.rs`、`runtime.rs` 及相关测试 | 删除宽松解码，改用共享函数；复现畸形输入被接受的问题，并验证错误后释放原任务。 |
| 五个插件的 `Cargo.lock` | 终端、文件、网络隧道、系统状态、Docker 均通过路径依赖使用 plugin-client；逐个更新根包解析，使各自 `--locked` 构建成立。不只更新真正调用编解码函数的两个插件。 |
| 终端 `integration.rs`、系统状态 `collectors.rs/scheduler.rs` | 函数签名不改；验证命令输出读取、UTF-16LE PowerShell EncodedCommand 和采样结果。 |
| `src-tauri/src/updates.rs::decode_repository_blob` | 已用库解码 GitHub envelope，并有专用空白归一化；本次不改。它不能使用控制通道包装器的 2 MiB/无空白规则替代自身格式规则。 |
| 前端、RPC、设置和版本配置 | 无需变更；标准浏览器编解码接口不是手写实现，不为统一名称增加包装。 |

修改后检索三处原字母表/位移函数，确认全部删除。最终统计生产代码与测试代码，报告实际净变化，不把新增回归行数算成仍在手写协议。

## 5. Base64：回归与验收

### 5.1 已运行的选型对照

本轮在 Windows x86_64、Rust 1.99.1 的临时工程中使用下载的 base64 0.23.1 发行源码；没有修改项目锁文件。对照结果：

- 0 至 8193 共 8194 种字节长度，包含全部余数类别及确定性的二进制内容；新库与核心、共享客户端编码一致，解码还原一致。
- 共享客户端和库对选定非法输入的接受/拒绝一致；2 MiB 编码输入解码一致，共享包装拒绝超限数据。
- Docker 原解码会接受 `Zg=a`、`Zh==`、`Zg`、`Zg==!`、`Zg===garbage`、`====` 等无效输入，而标准严格解码拒绝。此问题已通过原函数的隔离调用复现，不声称已造成真实 Docker 操作事故。

对照程序位于临时目录，不属于持久回归。实施时将下面的少量代表性用例纳入仓库；不把 8194 次对照循环机械搬进每次 CI。

### 5.2 持久回归

| 层级 | 必须验证的行为 |
| --- | --- |
| 共享编码单测 | RFC 4648 文本向量、全部 256 字节、空输入；1/2/3 字节余数；8191/8192/8193 字节；2 MiB 编码输入边界和超限；非法字符、空白、URL-safe 字符、内部/多余/缺失填充、非零尾位。 |
| Docker 回归 | 修改前 `Zg==!`、`Zg=a` 的拒绝断言失败，修改后通过；正常中文/JSON/二进制输出仍能读取，错误路径不重复操作并释放已有任务。 |
| 核心 RPC 集成 | 在现有 `tests/fixtures/system_status_flow.rs` 的会话任务测试补确定性二进制 stdout/stderr；分页拼接还原、重复 poll 一致、偏移按原始字节增长、完整结果标记正确、实际序列化控制帧未超限。 |
| 真实使用 | 终端目录自动识别、系统状态采集、Docker 查询/操作回执通过现有真实会话流程；Windows 远端采集命令仍符合 EncodedCommand 与 cmd.exe 长度约束。 |

执行共享客户端和五插件的 `cargo fmt`、`cargo test --locked`，并执行 `npm run check`、`npm run plugin:check`、`npm run scripts:test`、`npm run test`、`npm run web:build`。业务入口使用现有 `terminal:test`、`system-status:test`、`docker:test`；构建/实际包要求见第 8 节。

现有 `select-checks.mjs` 对共享客户端及 `session_exec` 改动会扩大检查范围。本批核对结果并补必要选择器回归，不借本次接入改写 CI 分层，不因本机某项通过取消既有任务。弱网 netem 与本次纯编码替换没有新增关联，不另设专项。

## 6. opener：目标行为与接口选择

### 6.1 两类入口

| 用户动作 | 当前调用链 | 接入后的打开接口 |
| --- | --- | --- |
| 更新说明网页链接、Release 页面、使用帮助 | `main.rs` 三个命令 → `updates.rs` 的 HTTP/HTTPS 验证 → `desktop_open::web_page` | `tauri_plugin_opener::open_url(url.as_str(), None::<&str>)` |
| 文件下载后的打开目标目录 | `component_input` 的 `input.reveal` → `files_local_directory` 检查页面/资源/输入归属 → `desktop_files::reveal_directory` | `tauri_plugin_opener::open_path(&path, None::<&str>)` |

目录动作仍表示进入目标目录，不改成 `reveal_item_in_dir` 的“打开父目录并选中条目”。`PathBuf` 始终传给接受 `AsRef<Path>` 的自由函数，不使用 `OpenerExt::open_path` 的字符串方法，也不调用 `to_string_lossy()`。

### 6.2 选择自由函数

只在核心 Cargo 清单增加 `tauri-plugin-opener = "=2.7.2"`，调用发行包公开的 `open_url/open_path` 自由函数。这些函数不需要 `.plugin(init())` 注册，也不读取插件 scope；当前业务校验继续在核心完成。

无需安装 `@tauri-apps/plugin-opener`，无需增加 capabilities、插件权限声明或审批流程。前端仍调用现有 Tauri 命令；不把指定外部程序的 `with` 参数暴露给用户或插件。

如果后续改成 OpenerExt，必须同时处理注册与默认 JS 链接脚本：默认 `init()` 会注入 `_blank`/修饰键链接处理，不能与现有事件委托重复打开。本方案使用自由函数，不引入这项行为。

### 6.3 保留的校验与错误

- 网页保留 `web_page_url` 的绝对 HTTP/HTTPS、主机检查，拒绝 javascript/data/file、相对路径及命令选项形状的输入；先校验，再调用系统打开接口。
- 目录保留 `files_local_directory` 的视图、资源和已认领下载目标检查；在打开适配中确认目录为绝对路径、没有 NUL，且 metadata 显示仍为目录。已删除路径、传入普通文件、不可读 metadata 返回明确错误，不误开父级或当前工作目录。
- Windows 保留原生 UTF-16 路径，Unix 保留非 UTF-8 原生路径字节；中文、空格、`#`、`%`、引号、反斜线和 UNC/扩展路径按适用系统验证。
- 错误继续通过现有 action/插件输入结果展示；包装为“无法打开网页：…”或“无法打开本地目标目录：…”。不把“调用返回 Ok”显示为“目标窗口已经成功展示”。
- 核心返回的命令名称、前端参数、异步 Promise 和 `Result<(), String>` 保持兼容。版本与数据迁移均不属于本次实现。

### 6.4 平台语义与线程

本轮核对的 2.7.2 自由函数委托 `open::that_detached`；其行为取决于锁定的 open 版本。使用 5.4.5 时：

| 平台 | 实际路径 | 返回成功的含义及限制 |
| --- | --- | --- |
| Windows | 插件启用 open 的 `shellexecute-on-windows`；调用 Shell API，目录可能先走 Shell 文件夹接口 | 系统接受请求，错误同步返回；不表示浏览器已完成加载或目标窗口可见。 |
| macOS | 等待 `/usr/bin/open` 启动器退出 | 启动器报告成功；等待的是启动器，不是用户关闭目标应用。需要验证退出状态和原生路径。 |
| Linux | xdg-open、gio 等桌面启动器与失败回退 | 每个启动器观察早期失败约 250ms；仍运行则视为已交付，之后失败无法报告。时间窗不是整个调用的硬墙钟上限。 |

统一在 `tauri::async_runtime::spawn_blocking` 中执行同步自由函数，保持 UI 和 Tokio 工作线程响应；不把阻塞调用放在事件循环。删除旧 GTK/AppKit 主线程派发、oneshot 和三个 `native` 分支。

不新增自行 fork 的打开进程管理器或线程池，不将目标应用纳入插件进程 Job/会话资源。Linux 启动器可能在核心退出后继续运行，由库负责其已建立的等待/回收；不得因 SoDeck 关闭而关闭用户浏览器。若等待异常，记录失败或未确认，不自动重试同一点击，避免重复窗口。

AppImage 尤其需要实包核验：库启动的系统工具会继承包运行环境，不能假设 GTK 内部调用与子进程方式等价。保留从实际 AppImage 验证默认浏览器/文件管理器的任务，不用 DEB 或源码程序的成功替代。

## 7. opener：具体改动与回归

### 7.1 代码与构建

- `src-tauri/Cargo.toml/Cargo.lock`：增加固定官方依赖，确认 open 5.4.5；检查新增包/feature/最低 Rust，不执行整树更新。不把传递 open 再作为未直接使用的生产依赖重复声明。
- `desktop_open.rs`：保留现有 `directory(handle, path)`、`web_page(handle, url)` 签名，AppHandle 参数改为 `_handle`；两类 Target 在同一 blocking worker 中分别调用自由函数。保留目录边界检查和错误映射，移除平台系统打开代码。
- `updates.rs`、`desktop_files.rs`、`main.rs`、`src/main.ts`：现有调用协议保留；只在测试需要时调整可见性或用例，不更改用户流程。
- 平台 Cargo feature：只移除确因打开适配删除而失去全部使用处的 feature。剪贴板/窗口/标题栏仍使用 objc2、GTK、windows-sys，不删这些依赖，也不移除原生测试需要的 feature。
- `select-checks.mjs`：保留 desktop_open → 五平台核心/UI/文件集成的现有映射；新增的测试入口按核心原生打开专项映射并回归，避免落入未映射路径全量检查。
- 核心独立构建；opener 不加入任何插件清单，插件包不携带打开库。

### 7.2 验证层次

1. **无系统副作用的单测**：保留 URL 接受/拒绝；增加目录类型、绝对路径、NUL、删除目标检查。验证错误输入在调用库前失败。不要为此引入后端 trait 或通用模拟框架。
2. **真实前端协议**：扩展已有 `scripts/test-production-ui.mjs` 链接回归，确认更新、Release、帮助入口参数正确；点击/修饰键不会执行两次，工作台不会导航或丢失插件 iframe。夹具记录 invoke 只能证明命令派发。
3. **源码业务**：`npm run files:test` 验证下载目标选择与 `input.reveal` 的有效/失效输入；过期、另一页面资源或已清理下载目标不能打开目录。
4. **隔离原生打开专项**：新增 `scripts/test-desktop-open-native.mjs`，复用已有明确测试模式和隔离资料目录。真实核心打开回环 HTTP 唯一标记 URL，由受控默认浏览器/测试 handler 接收；真实目录由受控默认文件管理器/handler 记录原生目标。记录 API 结果、收到的目标、时间及重复次数，而非只断言 Rust 返回 Ok。
5. **实际包**：从核心安装包解出/安装并使用该程序执行专项，覆盖首次打开、重复点击、调用后关闭核心、无可用关联/启动器的可控失败。普通包启动/退出和资料重开继续由原 smoke 验证。

原生专项使用专门 runner/测试账户；Linux 使用隔离的 XDG 配置和桌面会话，不在用户日常桌面改默认应用关联。测试默认应用输出唯一标记并有限退出，由脚本清理自己创建的进程/窗口；不枚举关闭用户原有浏览器。

现有 `validation/desktop_files.rs` 只验证剪贴板和来源变化，没有调用目录或网页打开。本次不得把历史文件剪贴板通过当作 opener 通过。为了不让核心包验收依赖业务插件，可扩展现有显式原生 probe 模式为独立打开 probe，并要求明确测试模式及隔离数据目录；其入口委托生产 desktop_open，不能复制另一套打开逻辑。辅助 handler/夹具二进制仅用于测试，不进入发行附件。文件插件的真实下载→打开目录流程留在独立远程验证工作流。

通过测试脚本后才增加相应 workflow 调用。具体安装测试浏览器/文件管理器的系统工具声明在开发文档和 runner 步骤，不依赖开发机 PATH 或手工默认关联；无桌面/无 handler 时报告缺失条件，不能写成打开成功。

执行 `npm run check`、`npm run scripts:test`、`npm run test`、`npm run web:build`、`npm run ui:production:test` 和 `npm run files:test`。涉及新脚本时执行 `node --check` 和相关脚本回归；不在无人值守单元测试中打开实际用户浏览器。

## 8. 五平台、包与交付

| 正式目标 | 核心实际包 | Base64 的插件与业务 | opener 的原生场景 |
| --- | --- | --- | --- |
| Windows x86_64 | 原生 x64 NSIS 内程序 | 五插件各自原生包启动/退出；终端、系统状态、Docker 使用真实核心输出 | 默认浏览器、Explorer/测试 handler、中文/特殊字符/UNC、无控制台闪烁、错误返回 |
| Windows ARM64 | 原生 ARM64 NSIS 内程序 | 同上，使用 ARM64 程序，不用 x64 模拟结果代替 | 同上，ARM64 原生调用和退出 |
| Linux x86_64 | 原生 DEB 与 AppImage | 同上；真实 SSH 远端，Docker 流程使用其支持的守护进程环境 | X11/原生 Wayland 适用会话、桌面关联、启动器失败、非 UTF-8 路径；DEB/AppImage 分别验证 |
| Linux ARM64 | 原生 ARM64 DEB 与 AppImage | 同上，使用 ARM64 程序 | 同上，不用 x86 容器替代 |
| macOS ARM64 | Apple Silicon DMG 内 app | 同上 | 默认浏览器/Finder、原生路径、启动器失败与核心正常退出 |

Base64 共享依赖变化影响五个插件的打包解析，五者都需重新构建、打包、验证包内程序 initialize/shutdown 与退出；终端、系统状态、Docker 另验证实际消费路径。文件/网络无需为了纯编码接入新增业务需求，执行现有适用烟测。

核心 `Package SoDeck` 保持自身构建/签名/包验证，不依赖插件构建。插件沿用各自 `Package plugin`，组合远程流程复用 `Verify native remote plugins` 或组件已有实际包入口；不添加全组件同步发布门禁。缺少 macOS、ARM64、Wayland 或实际 handler 条件时记录具体待验收项，不用交叉编译替代。

实现完成后在[基础逻辑复用实施记录](implementation-standard-library-adoption.md)分别记录两批实际变化、缺陷基线、测试/构建/实际包结果、代码规模和新增依赖。开发命令或测试工具变化只更新 development；发行输入变化只更新 distribution；不在根 README 追加过程。

本轮不改版本。正式交付修复时分别按核心与受影响插件的现行独立版本规则准备版本说明/CHANGELOG，保存包来源提交、原始打包 run、Release/草稿链接和实际验证/跳过范围，遵循[发布流程](distribution.md)。

## 9. 实施顺序与完成条件

### 第一批：Base64

1. 保存三处实现和当前调用方基线，先增加 Docker 畸形输入拒绝回归并确认旧实现失败。
2. 声明核心/共享客户端依赖，替换三处实现，按每个独立 Cargo 项目更新锁文件。
3. 完成共享边界与 RPC 二进制分页回归，执行相关格式、类型、脚本、Rust、真实业务和构建。
4. 五平台原生包与适用业务验收后，记录依赖、代码变化和未完成项，单独提交。

完成条件：三处手写实现全部删除；合法数据及协议兼容，畸形 Docker 输出明确失败，长度/控制帧预算保留；核心与五插件均能按各自锁文件构建，并获得上述平台证据。

### 第二批：opener

1. 核对当前 desktop_open/核心依赖树，解析固定候选并记录新增依赖与包体基线。
2. 使用 Rust 自由函数替换系统打开分支，保留业务校验、异步接口、原生路径和错误映射。
3. 完成 URL/目录单测、前端派发、文件来源和受控默认 handler 专项；在脚本存在后更新 CI 入口。
4. 在五平台实际核心包复验默认应用、失败返回和退出，独立记录与提交；若库路径存在无法接受的回归，保留现有实现并记录具体结果。

完成条件：平台打开分支已由库接管；无额外前端链接处理、能力授权流程或插件依赖；合法目标在系统默认应用准确打开一次，非法/失效目标拒绝，错误不假报成功；五平台及两种 Linux 包有实际证据。

## 10. 本轮证据边界

已完成：当前调用链、三处实现、既有测试/CI、固定候选发行接口/许可/维护状态核对；Windows x86_64 临时 Base64 编解码对照及 Docker 原函数缺陷复现。

尚未执行：生产接入、opener 在当前完整核心依赖树中的编译、默认浏览器/目录原生专项、五平台构建/打包和正式发行。本轮方案完整性不等于这些验收已经通过。

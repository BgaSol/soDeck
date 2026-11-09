# 本机系统能力改用库绑定：功能与代码设计

整理日期：2026-11-09。本文维护功能与代码设计；源码实施和本次验收见[实施记录](implementation-native-system-bindings.md)，不以方案代替运行结果。

本方案覆盖当前运行代码与原生测试中的手写系统绑定，并补充本机网页打开入口。文件剪贴板与本地目录的详细业务规则复用[基础逻辑复用计划第四阶段](plan-standard-library-adoption.md#6-第四阶段三系统本地文件适配整改)，实际完成情况继续由后续实施记录维护。产品与交付边界以 [开发约定](../AGENTS.md)、[架构文档](architecture.md)和[分发文档](distribution.md)为准。

## 1. 对需求的理解与推测

用户明确希望把目前按各个平台手工调用系统的实现改为使用库绑定。结合源码，推测目标是：**系统函数签名、平台类型、ABI、常量以及库能够管理的底层对象交给成熟绑定；SoDeck 保留功能编排、参数检查、线程调度和资源生命周期。**

预期收益包括：

- macOS 不再自行选择 Objective-C 消息返回签名、转换函数指针或区分结构体返回 ABI。
- Linux 不再自行声明 GTK/GDK/GLib 函数及其对象类型、连接信号和释放 GLib 错误。
- Windows 已有标准绑定覆盖的函数直接导入；测试也复用绑定，减少维护另一份 P/Invoke 类型定义的工作。
- 网页打开等已有库支持的本机能力，不再在业务模块中维护各系统启动命令。
- 五个正式目标使用同一套明确的平台适配边界，类型错误尽量在编译时暴露。

“使用库绑定”仍会调用操作系统。`windows-sys`、`libc` 和少量原生指针借用可能仍需要 `unsafe`；本方案要求消除项目自定义的系统 ABI 声明，并说明保留的边界条件。它也不保证 OS 请求可取消，或自动解决剪贴板来源变化和页面失效。

本次按本机桌面能力理解需求。远程系统状态插件通过 SSH 执行 PowerShell、shell 和 Docker 命令属于远程采集协议；改成远端库调用需要部署远端程序，是另一个功能。SSH/SFTP、凭据存储、路径和进程管理中已使用的标准库或绑定继续沿用。

## 2. 当前源码与完整范围

核对基线为本次工作区。已有未提交的工作台设计改动不属于本方案。

| 落点 | 当前做法 | 处理决定 |
| --- | --- | --- |
| `src-tauri/src/desktop_files/macos.rs` | 自定义 `Obj`；声明 objc runtime；多种 `objc_msgSend` 函数指针转换；手工 pool | 改为 objc2 的 Foundation/AppKit 类型化接口 |
| `src-tauri/src/validation/macos_titlebar.rs` | 手写 `Rect` 和 Objective-C 调用；自行区分 ARM64/x86_64 结构体返回 | 改为 `NSWindow`、`NSView`、`NSRect` 和按钮常量；覆盖实际包验收入口 |
| `src-tauri/src/desktop_files/linux.rs` | 四组 GTK/GDK/GLib/GObject 声明、通用裸指针、手写信号注册和错误释放 | 改为现有 GTK3 绑定；保留并补齐有界异步请求的生命周期 |
| `src-tauri/src/windows.rs` | 两处 user32 声明：`WindowFromPoint/GetAncestor`、`ScreenToClient` | 直接导入已有 `windows-sys` 对应接口和常量 |
| `scripts/native-pointer.ps1` | `Add-Type` 中手写结构体和 18 个 DllImport 声明 | 迁移为使用 `windows-sys` 的 Rust 测试夹具；由原 Node 测试驱动 |
| `src-tauri/src/updates.rs` | Windows `rundll32`、macOS `open`、Linux `xdg-open` 分支及子进程回收 | 用已经选定的系统绑定打开 URL，与本地目录打开复用一个小模块 |
| `src-tauri/src/desktop_files/windows.rs` | 已使用 `windows-sys` 的剪贴板和 Shell 接口 | 保留；用绑定常量替换格式/显示方式数字，核验借用句柄与错误处理 |
| `src-tauri/src/platform.rs` | Windows Job 使用 `windows-sys`；Unix 进程组使用标准库和 `libc::kill` | 已符合方向，保留现有接口和退出行为 |
| 文本剪贴板、拖放、窗口基础行为 | 已使用 Tauri、clipboard-manager 等库 | 保留现有能力，作为相关业务回归的调用方 |

可以从源码确认 `isFileURL` 当前经过返回 `usize` 的手写函数签名，而目标绑定返回 `bool`；标题栏结构体返回和 Linux 通用指针也由项目自行维护。本次没有在 macOS/Linux 原生环境复现崩溃，不能把这些源码风险写成已经发生的故障。

## 3. 完成后的功能行为

这是内部实现整改。现有用户入口继续完成同样的操作，错误和失效路径必须可验收。

| 场景 | 验收行为 |
| --- | --- |
| 从系统文件管理器复制文件/目录，在文件插件粘贴 | 真实路径和上传内容正确；空或纯文本剪贴板没有文件来源；保留当前容量与编码规则 |
| 读取时换来源、关闭页面或断开连接 | 旧结果不能登记有效 input、创建任务或操作新目标；结果明确失败或失效 |
| Linux 来源不响应、回退时取消、退出时仍有请求 | 5 秒结束业务等待；未返回的 OS 请求合计最多 4 个；迟到回调不重新交付；退出断开订阅 |
| 下载后打开本地目标目录 | 按原生路径打开对应目录；中文、空格、特殊字符与支持的非 UTF-8 Unix 路径不损坏 |
| 打开 Release、更新说明链接或使用帮助 | 通过系统默认浏览器打开经过验证的 HTTP/HTTPS URL；明确返回系统拒绝或处理失败 |
| Windows 跨窗口拖拽 | 仍以指针所在位置的实际最上层原生窗口为准；被其他窗口遮挡时不得命中后方 SoDeck 窗口 |
| Windows 原生坐标与鼠标验收 | 使用系统坐标/DPI和真实鼠标输入；成功、失败均释放按键并恢复指针及 DPI 上下文 |
| macOS 实际包标题栏验收 | 主窗口、拆出窗口的三个原生按钮均存在；原有顶部中心 22pt、左侧 86pt 范围判据继续适用 |
| 核心退出、重启、更新安装 | 系统适配在现有应用关闭收尾中清理；单个页面或普通窗口关闭不关闭全局剪贴板后端 |

网页打开成功的含义是系统接受了打开请求。应用不能据此报告浏览器已经加载页面，也不增加自动重复打开。现有 Wayland 跨窗口拖动限制继续成立；换用绑定不能提供桌面协议没有的全局坐标。

## 4. 绑定与依赖选择

优先复用当前 Tauri 依赖体系。2026-11-09 核对了锁文件、对应版本的本地源码、crates.io 元数据和上游仓库状态。

| 平台 | 选择与版本 | 用途、维护情况及替代方案 |
| --- | --- | --- |
| Windows | 已有直接依赖 `windows-sys =0.61.2` | 微软 [windows-rs](https://github.com/microsoft/windows-rs) 生成绑定，仓库未归档且有维护活动。替代是高层 `windows` crate，但当前所需 C API 已覆盖，迁移无必要 |
| macOS | `objc2 =0.6.5`、`objc2-foundation =0.3.2`、`objc2-app-kit =0.3.2` | 三包已在核心锁文件中，需增加 macOS 直接声明。[objc2](https://github.com/madsmtm/objc2) 未归档，最近推送 2026-10-17；objc2 0.6.5 发布于 2026-10-05，包声明 MIT。Foundation/AppKit 负责系统类、返回类型和对象所有权；不再另选旧 Cocoa 包或自定义消息签名 |
| Linux | `gtk =0.18.2`、`gdk =0.18.2`、`glib =0.18.5` | 与当前锁定 Tauri 的 GTK3/GLib 一致，需增加实际引用的 Linux 直接声明。[gtk3-rs](https://github.com/gtk-rs/gtk3-rs) 未归档，最近推送 2026-09-22；最新 gtk/gdk 为 0.19.0。[gtk-rs-core](https://github.com/gtk-rs/gtk-rs-core) 有维护活动。当前兼容版不等于长期维护承诺；本次不顺带升级 GUI 体系。高层缺口使用同版本生成的 `ffi` 接口 |
| Unix 进程 | 已有 `libc =0.2.191` 和标准库 | 当前 `kill` 就是库绑定，不因它需要 `unsafe` 而再加 nix/rustix 或进程框架 |
| 网页与目录打开 | 上述 Windows Shell、AppKit、GTK 接口 | 三个平台都有真实复用入口，不新增 opener 依赖。[Tauri opener](https://crates.io/crates/tauri-plugin-opener) 是可用替代，但再包一层不会减少本次剪贴板/窗口所需绑定，且部分系统实现仍会启动外部工具 |

实施时固定直接依赖版本并提交 `src-tauri/Cargo.lock`。传递依赖出现在锁文件中不等于允许直接引用；每个平台仍显式声明。只启用用到的 feature，不能通过全量默认 AppKit/Foundation feature 掩盖缺失声明。

macOS feature 以 `std`、`NSArray`、`NSString`、`NSURL`、`NSGeometry` 和实际 AppKit 类为起点；包括 `NSPasteboard/NSPasteboardItem`、`NSWorkspace`、`NSWindow/NSView/NSButton`，并按生成源码补足方法的条件依赖。此表是可实施的选择，最终最小 feature 集需在 macOS 编译确认。

Windows 生产代码增加 `Win32_UI_WindowsAndMessaging`、`Win32_Graphics_Gdi`、`Win32_System_Ole`；鼠标测试额外需要 `Win32_UI_HiDpi`、`Win32_UI_Input_KeyboardAndMouse`，仅随 `test-fixtures` 启用，复用原有线程相关 feature。

## 5. 代码设计

### 5.1 保持当前模块边界

文件输入留在 `desktop_files/`，窗口行为留在 `windows.rs`，插件进程留在 `platform.rs`，标题栏验收留在 `validation/`。业务模块消费 `PathBuf`、现有 `Clipboard` 和业务错误；不向插件 RPC 暴露 `HWND`、GTK 对象或 Objective-C 指针。

新增的生产模块只有 `src-tauri/src/desktop_open.rs`：网页和目录已经构成两个真实调用场景，在这里复用系统打开和线程调度。模块内用 `cfg` 选择实现，不增加平台后端 trait、运行时注册、统一系统调用服务或新权限流程。

### 5.2 Windows：导入绑定并保留原生判据

`windows.rs` 直接导入：

```rust
use windows_sys::Win32::{
    Foundation::{HWND, POINT},
    Graphics::Gdi::ScreenToClient,
    UI::WindowsAndMessaging::{GetAncestor, WindowFromPoint, GA_ROOT},
};
```

以上模块位置已按 0.61.2 源码核对：`ScreenToClient` 位于 `Graphics::Gdi`。删除两处 `#[link]`/`extern` 声明，`GetAncestor` 的数字 `2` 改为 `GA_ROOT`。继续由 Tauri 查询窗口可见性、客户区和 scale factor；保留跨窗查询时不持有注册状态锁的规则。

`desktop_files/windows.rs` 的 `15` 改为绑定的 `System::Ole::CF_HDROP`，在需要 `u32` 的接口边界转换；显示方式用 `SW_SHOWNORMAL`。保留 `OwnedHandle`、CloseClipboard Guard、5×20ms 重试、UTF-16 路径与序列号核对。`GetClipboardData` 返回借用句柄，读取方不调用 `GlobalFree` 或 `DragFinish`。

### 5.3 macOS：使用系统类和正确的所有权

文件后端删除 `Obj`、runtime 声明、`msg/arg/number`、函数指针 `transmute` 和手工 pool。使用以下对应接口：

| 功能 | 类型化接口 |
| --- | --- |
| 读取来源与变化 | `NSPasteboard::generalPasteboard()`、`types()`、`changeCount()` |
| 文件格式和条目 | `NSPasteboardTypeFileURL`、`pasteboardItems()`、`NSPasteboardItem::stringForType()` |
| 文件 URL | `NSURL::URLWithString()`、`isFileURL()`、`fileSystemRepresentation()` |
| 对象存活 | `Retained<T>`、`objc2::rc::autoreleasepool` |
| 打开 URL | `NSWorkspace::sharedWorkspace().openURL(&url)` |

`changeCount` 使用绑定的 `NSInteger`；数组和 URL 的可空返回值显式处理。文件系统表示在 NSURL 存活期间立即复制为 `OsString/PathBuf`，不能有损转为 UTF-8 或把借用指针返回给调用方。原生路径构造 NSURL 的指针接口仍可保留小范围 `unsafe`，注明 C 字符串无 NUL、存活期与返回对象管理。

标题栏验收在 Tauri `with_webview` 回调中借用 `ns_window()` 为 `&NSWindow`；检查非空，使用 `MainThreadMarker` 确认 UI 线程。窗口归 Tauri 管理，不从借用指针构造接管所有权的对象，也不把该引用带出回调。

通过 `contentView().bounds()`、`standardWindowButton(NSWindowButton::CloseButton/MiniaturizeButton/ZoomButton)` 和 `convertRect_toView(..., None)` 计算既有判据。直接使用 Foundation `NSRect`，删除项目自己的 `Rect`、`objc_msgSend_stret` 与架构分支。只支持现有正式 macOS ARM64 目标，不扩展发布平台。

### 5.4 Linux：GTK 类型化接口与有限生成接口

沿用 Tauri 初始化的 GTK3 主线程与主循环。GTK 对象保存在 Linux 后端的主线程状态中，不放入跨线程 `AppCore`，不初始化第二个 GTK 运行时。

按 0.18.2 的实际 API 实现：

1. `gtk::Clipboard` 获取 CLIPBOARD，`gdk::Atom` 表示目标格式。
2. `request_contents(TARGETS, closure)` 配合 `SelectionData::targets()` 完成轻量格式查询；此版本没有高层 `request_targets`，不能写一个不存在的方法。
3. `request_contents(text/uri-list, closure)` 读取正文，未提供时回退 GNOME copied-files。使用原有 `uri_paths`，保留 copy/cut 标记的复制上传语义。
4. 先检查 `SelectionData::length()`，再读取正文：负值表示格式未提供，零值为空，超过 4MiB 拒绝。`data()` 会复制内存，不能先复制再限流。非零长度/空指针等异常检查必要时调用 `gtk::ffi::gtk_selection_data_get_data`，使用生成类型并限定借用范围。
5. `connect_local("owner-change", ...)` 保存 `SignalHandlerId`；该版本生成的 `connect_owner_change` 被注释，不能假设它可用。在应用收尾时断开信号。

默认使用库闭包适配，由库提供 C 回调 trampoline。OS 未回调时闭包仍可能被持有，闭包只捕获请求 ID，资源上界由下一节控制。若 TARGETS 在目标桌面不能正确查询，先复现，再使用 `gtk::ffi` 的生成接口补该落点；不重新声明系统函数。项目如需实现业务回调函数，签名使用生成的回调类型，它不承担系统 API 声明。

### 5.5 网页与目录打开：小型共同模块

`desktop_open.rs` 提供以下内部接口形状；片段是方案，不是已提交实现：

```rust
pub(crate) async fn directory(handle: tauri::AppHandle, path: PathBuf)
    -> Result<(), String>;
pub(crate) async fn web_page(handle: tauri::AppHandle, url: reqwest::Url)
    -> Result<(), String>;
```

平台落点：

- Windows：两种目标复用 `ShellExecuteW`，目录保持原生 UTF-16 路径，网页传入已验证 URL；检查返回值 `<=32` 的失败规则，在 `spawn_blocking` 中执行。保留 Explorer 支持的 UNC 本地文件系统路径。
- macOS：目录从原生文件系统字节构造 NSURL，网页从 URL 字符串构造 NSURL；主线程调用类型化 `NSWorkspace::openURL`，检查返回值。
- Linux：目录使用已有 `reqwest::Url::from_file_path`，网页使用已验证 URL；主线程调用 `gtk::show_uri_on_window`，由 `glib::Error` 管理错误，删除逐字节 URI 拼接和手工 `g_error_free`。

`desktop_files::reveal_directory` 保留现有签名并委托该模块，平台文件删除原 `reveal` 的重复实现。`updates.rs` 保留 HTTP/HTTPS 验证规则，把 `open_release_page/open_web_page` 改成接受 AppHandle 的异步函数；删除三平台启动命令与等待子进程线程。

`main.rs` 的 `open_update_release`、`open_update_link`、`open_usage_help` 同步迁移为接收 AppHandle 的异步 Tauri 命令，命令名、前端参数和结果保持兼容。三个入口都委托同一校验/打开路径。切勿把未经验证的任意 scheme 直接送到系统。

### 5.6 原生鼠标测试：使用 Rust 夹具

新增 `src-tauri/tests/fixtures/native_pointer.rs`，在现有 Cargo 清单声明 `native-pointer-fixture`，要求 `test-fixtures` 且 `test=false`。Windows 使用已有绑定；其他系统构建一个明确提示不支持的入口，使五平台 test-fixtures 构建不因无条件导入 Win32 失败。夹具不进入核心安装包或插件发行包。

`scripts/test-close-native.mjs::nativePointer` 使用 Node `spawn` 的参数数组启动夹具，传入目标进程 ID、`arrange/drag` 和 JSON 计划，保留当前 stdout JSON/stderr/退出码协议。夹具路径由测试显式参数或 `SODECK_TEST_NATIVE_POINTER` 指定；使用目标架构产物，不从 PATH 或开发者绝对路径猜测。迁移调用方后删除 `scripts/native-pointer.ps1`。

夹具保留窗口所属 PID、前台激活、监视器工作区、DPI、客户区坐标及拖拽步骤。用绑定提供的结构、函数和常量，不改成 DOM 事件。为原有 `mouse_event` 优先做直接绑定迁移；若需要改用 `SendInput`，另以输入行为验证支持该变更。

采用 RAII 保存/恢复原指针和线程 DPI 上下文，并在已按下之后的所有退出路径释放鼠标按键。`AttachThreadInput` 的建立与解除也成对执行。参数无效、目标窗口失效、前台激活失败均返回明确错误，不把输入发给其他窗口。

## 6. 异步状态、资源与退出

### 6.1 资源归属

| 资源 | 唯一管理者 | 结束方式 |
| --- | --- | --- |
| Windows Job / OwnedHandle | 既有 `platform.rs` 与插件进程监督 | 沿用停止、异常退出和应用收尾 |
| Windows 剪贴板访问 | 当前一次读取的 Guard | 每条成功/错误路径 CloseClipboard；不释放借用的数据句柄 |
| macOS 临时对象与字节借用 | 当前 UI 线程调用、Retained/pool | 在返回业务值前复制字节，按绑定释放对象 |
| Tauri 原生窗口 | Tauri | 验收只借用，不 retain 成长期状态或自行 release |
| GTK Clipboard、owner-change 连接 | Linux 主线程后端 | 应用原生收尾前断开信号、清空业务引用 |
| GTK 系统请求 | Linux 后端 pending 表与系统回调 | 回调返回归还槽位；业务超时只释放发送端，系统槽位继续计数 |
| 鼠标、输入线程连接、DPI 上下文 | 原生测试夹具的 RAII guard | helper 成功/错误退出时恢复 |

### 6.2 Linux 请求的最小状态

复用现有请求 ID、revision、sender 和最多 4 个请求的表。状态留在主线程 `thread_local` 后端中；请求启动前完成登记，释放内部借用之后再调用 GTK，允许回调同步或重入。回调不持有核心锁，不让 Rust panic 穿过 FFI 边界。

| 状态/事件 | 操作与终点 |
| --- | --- |
| 排队，业务已取消或超时 | 真正调用 GTK 前检查 sender 是否关闭，直接跳过；不占系统请求槽位 |
| 正常请求 | 登记 ID/起始 revision，进入 TARGETS 或主格式等待；提交结果前再次核对 revision 和有效性 |
| 主格式未提供 | 当前回调结束后，为同一业务请求发起一次 fallback；请求失效则不回退 |
| 成功、无文件或明确失败 | 完成一次 sender，移除登记，归还系统槽位 |
| 5 秒超时或调用 future 被丢弃 | 主线程将该 ID 标为过期，释放 sender；保留轻量 ID 和系统槽位，等待底层回调 |
| 过期回调到达 | 不复制正文、不回退、不再次交付，只移除登记和归还槽位 |
| 四个未返回请求占满 | 新请求明确返回忙；不删除旧登记后继续创建 OS 请求 |
| 应用实际退出/重启 | 后端停止接纳、断开信号、结束 sender 并清空登记；迟到闭包查询不到 ID，安全结束 |

共同 `clipboard()` 的 Linux 分支在调度前分配 ID，使用一个小型取消 Guard；超时和 future drop 都向主线程投递取消该 ID。若事件循环已关闭，应用收尾的后端 shutdown 是清理终点。取消 Guard 只服务本功能，不提取通用异步任务框架。

macOS/Linux 已有 5 秒 timeout 包围接收等待；它不能抢占正在 UI 线程执行的同步 OS 调用。主线程任务开始前检查业务是否仍等待；已经发出的打开请求不因等待结束而自动再次发送。同步调用耗时在原生验收中单独观察，不声称绑定提供硬取消。

Linux 后端 cleanup 接入 `main.rs::attach_native_finalizer` 的应用级 Exit/Restart/InstallAndRestart 分支。沿用现有顺序：先完成可能失败的 updater install，再在 `exit_handoff` 和原生 exit/restart 之前调度主线程清理并确认结果。安装失败不提前关闭后端，取消关闭不清理，全局后端不随页面/普通窗口关闭。

cleanup 本身幂等：停止接纳、断开信号、结束业务请求；当前 finalizer 工作线程等待主线程确认时不持有核心锁。调度或确认失败沿用 finalizer 的错误返回，记录已完成步骤并允许重试，不假装安装已撤回。请求 ID 保持单调不复用，重复收尾和旧回调不能命中新请求。这里不新增独立的发布或后端恢复状态机。

## 7. 调用方、兼容与文档影响

| 范围 | 必须同步处理的落点 |
| --- | --- |
| 核心依赖与导出 | `src-tauri/Cargo.toml`、`Cargo.lock`、`src/lib.rs` 的私有 desktop_open 模块 |
| 文件能力 | `desktop_files.rs`、三个平台文件、`main.rs` 文件数据/实际 smoke 入口；保留 `Clipboard` 与插件 API v1 |
| 网页打开 | `updates.rs` URL 单测、`main.rs` 三个命令；前端调用协议不变 |
| 窗口与包内验收 | `windows.rs`、`validation/macos_titlebar.rs`、既有 workspaces/window_config/关闭回归 |
| 测试夹具 | Cargo bin 声明、native_pointer.rs、`test-close-native.mjs` 调用与准备步骤；删除旧 P/Invoke 脚本 |
| CI 范围 | `scripts/select-checks.mjs` 及测试：新增 desktop_open/fixture 路径明确映射到核心、文件或 Windows 原生验收；保留原有失败汇总 |
| 文档 | 本方案和索引；实现时更新受影响的 architecture/development，实际验收写专门实施记录，发布时写核心版本说明 |

不改变插件启停规则、持久数据、核心/插件独立发布或现有 RPC 版本；所有平台句柄在核心内部消化。应用版本继续只在实际发行时修改 `tauri.conf.json`。本次不增加权限声明、审批、平台插件或远端 agent。

## 8. 验证方案与证据边界

先跑旧实现建立行为基线；可稳定复现的缺陷先增加失败回归，再修改。绑定迁移自身需要原生编译与功能验收，静态扫描不代替实际调用。

| 层次 | 检查与判据 |
| --- | --- |
| 静态范围 | 检索源码/测试中的 `#[link]`、系统 extern 声明、`objc_msgSend`、相关函数指针 transmute、DllImport 和三个网页启动分支；确认清单内旧实现已删除。使用生成类型的业务回调允许保留 |
| 格式与源码 | `npm run check`、`npm run scripts:test`、`npm run test`；目标平台 `cargo check --manifest-path src-tauri/Cargo.toml --locked`；运行适用的 web/build 与现有原生套件 |
| 请求生命周期 | 可控时序验证先取消后执行、回调前超时、回退中取消、四个来源不响应、第五个请求返回忙、迟到归还槽位、shutdown 后的旧回调与安装失败保留后端；断言一次交付和资源上界 |
| 文件业务 | 复用[第四阶段验收矩阵](plan-standard-library-adoption.md#67-测试设计与执行顺序)；测试真实文件格式和上传内容。现有 JS stub、纯 URI 单测与文本 clipboardNative 不代表文件剪贴板原生通过 |
| Windows 窗口/输入 | 原生遮挡、客户区/DPI转换、真实拖出/返回、helper 中途失败与鼠标恢复；同架构 fixture 和实际包共同运行，不能只测坐标函数返回成功 |
| macOS 标题栏 | 实际 ARM64 app 包的主窗口和拆出窗口执行原有按钮几何验收；Tauri 配置单测不代替 AppKit 检查 |
| 系统打开 | HTTP/HTTPS 拒绝规则、浏览器默认关联、中文/空格/特殊字符目录、处理程序不存在或拒绝；自动验证系统接纳，实际浏览器/文件管理器显示另记录 |
| 发布产物 | Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 均在原生目标编译、打包，并验证包内程序启动与退出；受影响功能用实际包验证 |

Linux X11 与原生 Wayland 的剪贴板/打开结果分别记录，XWayland 不算原生 Wayland。Windows 混合 DPI 与多显示器、实际默认浏览器显示等需要桌面环境的用例明确记录人工执行或未执行；CI 无交互会话不能记为已通过。

沿用现有 Check、Package SoDeck 和实际包组合验收，不建立新的发布框架或全平台联动门禁。核心打包仍只依赖核心与自身运行资源；文件插件组合回归独立准备插件包。新增 fixture 和测试入口实现之后才调整实际调用它们的工作流。

本次文档交付只执行文档链接、结尾换行、冲突标记与 diff 空白检查；类型、构建、运行测试和原生包验收属于后续代码实施，不能引用历史通过结果替代。

## 9. 实施顺序与完成条件

按可独立验收的主题提交，不把全仓库改写塞进一个提交：

1. **Windows 运行绑定与鼠标夹具**：删除 user32/PInvoke 声明，迁移测试调用与准备路径；同架构实际输入、遮挡、恢复和退出通过。
2. **macOS 类型化绑定**：同时迁移文件后端与标题栏验证；文件、对象存活、按钮几何及实际 ARM64 包通过。
3. **Linux 绑定与请求收尾**：迁移 GTK 读取/信号，补取消与有界状态，接入应用 finalizer；文件、超时/迟到、X11/Wayland及退出通过。
4. **统一系统打开**：两类真实入口迁入 desktop_open，迁移三个网页命令，删除平台启动命令与重复 reveal；网页、目录和五平台实际包验证。
5. **交付记录**：确认全部范围已迁移，保存本次源码结果、五平台包来源和实际功能验证/跳过范围，按现有核心发布流程交付。

每批进入实施前重新核对源码、依赖声明和工作区；前一批确需调整的函数签名同步更新调用方。全部清单落点的旧系统声明删除、既有业务契约保持、异常/取消资源闭合、五平台实际包验证完成后，才可报告“系统绑定整改完成”。

当前待确认项是目标系统下的最小 feature、GTK TARGETS/owner-change 行为、原生输入环境与失败收尾；本方案已经给出验证和替代落点，不把它们记为已实施能力。

返回 [文档索引](README.md)

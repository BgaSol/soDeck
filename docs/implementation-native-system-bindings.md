# 本机系统绑定实施记录

2026-11-09。对应[功能与代码设计](plan-native-system-bindings.md)，同时完成[基础逻辑复用第四阶段](plan-standard-library-adoption.md#6-第四阶段三系统本地文件适配整改)的源码迁移。本记录维护本次实际证据，发布来源写入 [0.20.0 版本说明](releases/v0.20.0.md)。

## 源码与原因

- Windows 窗口命中/坐标转换直接导入 windows-sys，删除 user32 声明。剪贴板使用 CF_HDROP 常量，保留 CloseClipboard Guard、UTF-16 路径、忙重试和序列号检查。
- macOS 文件读取改用 NSPasteboard、NSURL 和 Retained/pool；BOOL、NSInteger、数组和可空值由绑定定义，删除 objc_msgSend/transmute。严格 URL 检查后在 NSURL 存活期间复制原生路径字节。
- macOS 标题栏实际包验收借用 Tauri 的 NSWindow，在 UI 线程使用 NSView/NSRect/NSWindowButton；删除自定义结构和架构 ABI 分支，保留原按钮几何判据。
- Linux 使用 GTK3 的 Clipboard、SelectionData、Atom 和 GLib 信号。只有高层不能提供受检字节借用时使用生成 ffi；不声明系统函数。TARGETS 查询和 GNOME copied-files 回退均走库回调。
- Linux 原先删除已关闭接收端的登记，但 GTK 可能仍持有底层回调；再次读取可以突破四个系统请求的原设计上限。现改为取消/超时释放 sender，系统槽位保留到回调返回；过期回调不复制/回退/交付。请求上界、槽位恢复、shutdown 和旧 ID 的回归在各宿主执行。此原因由源码和可控请求状态确认，不报告为已经复现的原生崩溃。
- `desktop_open.rs` 为目录和 HTTP/HTTPS 网页复用 Windows Shell/AppKit/GTK；删除三平台打开命令及等待子进程线程。Tauri 命令名、参数和插件 API 保持兼容。
- 原生鼠标测试改为 Rust 夹具，使用生成结构/函数和 RAII 清理，不再维护 PowerShell P/Invoke。独立文件剪贴板提供者与包内 probe 验证真实文件格式、来源变化和退出；Linux 覆盖 URI-list 与 GNOME 格式。夹具仅在 test-fixtures 构建，不进入核心或插件包。

新增平台直接依赖均已存在于原锁文件：objc2 0.6.5、Foundation/AppKit 0.3.2、GTK/GDK 0.18.2、GLib 0.18.5；固定版本和维护/替代依据见方案。Windows/libc 沿用已有绑定。锁文件仅增加六项直接依赖关系，没有整体升级。

应用级原生 finalizer 在 updater install 成功后、退出交接之前确认 Linux 后端清理；安装失败不提前关闭后端。普通页面/窗口关闭不停止全局订阅，关闭取消保留后端。明确 smoke 入口同样清理后端。

## 本机验证

本机为 Windows x86_64。源码改造前的核心套件通过；迁移后核心 211 项通过、12 项远端用例 ignored，脚本 93 项通过，类型/格式检查、test-fixtures 全部二进制编译与 NSIS 构建通过。本机安装包来源为 `68b47e6`；解开 0.20.0 安装包后，实际程序完成启动、窗口迁移、资料保存重开与退出烟测。正式发行使用下节的 `3e75370` 包，本机结果不替代正式包验收。

Windows 原生交互测试与烟测并行时，拖出窗口判据失败；顺序复验通过真实鼠标拖出/返回、PTY 接续、分隔线拖动、关闭取消、明确放弃与四插件/SSH/转发资源释放。并行结果不计为通过。日志位于忽略目录 `artifacts/native-bindings-*`，成功交互报告为 `artifacts/close-lifecycle/native-close.json`。

## 原生与实际包验收

首次 GitHub 原生检查发现 Linux 打开模块引用了高层 GDK 未导出的 `CURRENT_TIME`；固定 0.18.2 的常量实际位于 `gdk::ffi::GDK_CURRENT_TIME`。修正引用后重新进行源码检查与打包，失败候选不用于发行。

正式核心包来源为 `3e75370726c6a39ddb8ae742ed1dc9df0e062768`。[完整 Check](https://github.com/BgaSol/soDeck/actions/runs/42618172797) 通过五平台核心/四插件源码、格式、前端、脚本、Windows SSH/UI 与 Linux 两架构业务回归；Windows 正式前端焦点断言首次超时，本机 13 组复验通过，重跑失败 desktop 任务后完整 Check 通过。netem 为 skipped。

[原始打包运行](https://github.com/BgaSol/soDeck/actions/runs/42618160193) 的五平台原生构建、签名及实际包启动/退出全部通过，Linux 同时运行 DEB 内程序和 AppImage，macOS 检查主窗口与拆出窗口的 AppKit 按钮几何。首次 metadata 创建草稿后未立即读回同一记录；核对标签与源码后只重跑 metadata 成功，复用五平台附件。草稿包含 16 个附件，名称、大小和 SHA-256 均已核对。

[最终独立提供者/远程插件验收](https://github.com/BgaSol/soDeck/actions/runs/42621468753) 使用脚本提交 `03604d4a03ae44acce13e93f4f27b1930a08d549`，固定上述原始核心包 run，以及正式终端 0.7.0、文件 0.8.0、网络隧道 0.3.0。汇总与五份原生报告均为 success：

| 原生目标 | 实际包、SSH/PTY/SFTP/网络 | 独立系统文件格式、来源变化与退出 |
| --- | --- | --- |
| Windows x86_64 | 通过 | CF_HDROP 通过 |
| Windows ARM64 | 通过 | CF_HDROP 通过 |
| Linux x86_64 | 通过 | URI-list、GNOME copied-files 均通过 |
| Linux ARM64 | 通过 | URI-list、GNOME copied-files 均通过 |
| macOS ARM64 | 通过 | NSPasteboard FileURL 通过 |

ARM64 均使用原生 ARM64 程序，未以交叉编译代替实际包执行。系统状态 0.4.0 本次不重新发行；四插件源码矩阵与本机原生关闭测试包含它，最终远程包组合的独立输入为表中三款插件。

`desktopFilesNative` 单独证明实际文件格式与变化，不由旧文本 clipboardNative 或 JS stub 推断。该层在隔离 runner 显式开启，失败/缺报告/版本架构不匹配不能通过汇总。

首次独立提供者验收在新建目录误设 `SODECK_VERIFY_PERSISTED=1`，文件 probe 完成后由既有工作区验收报“重开后服务器未保留”。修正测试编排：文件 probe 只启动一次，来源改为文本后不自动重放；普通包烟测继续验证资料重开。脚本回归同时检查不重放、拒绝旧报告和拒绝文本 clipboard 报告。该修正不更换已验证核心包，最终验收记录分别保留包源码与测试脚本提交。

本地最终脚本 93 项、类型/格式、Node 语法和文档检查通过；脚本提交的[补充 Check](https://github.com/BgaSol/soDeck/actions/runs/42621433901) 首次在 Linux x86_64 文件 UI 等待“草稿已导出”提示超时，只重跑失败 integration 任务后全部通过。其他平台源码、Windows 完整 UI 和 Linux ARM64 业务均通过；netem 为 skipped。

## 未执行范围与限制

原生库仍需要少量 unsafe，平台绑定不能提供 OS 没有的硬取消或 Wayland 全局坐标。GTK 回调永不返回时，四个槽位会保留并明确返回忙，直到来源返回或应用退出；不会通过删除登记无界重试。

实际 Finder/Explorer/发行版文件管理器人工操作、原生 Wayland、混合 DPI/多显示器、默认浏览器实际页面显示及无处理程序的桌面专项需独立记录。合成原生提供者和包启动验收不代表这些人工场景均已执行。本次没有更换插件版本，也不执行 netem 或历史版本升级。

返回 [文档索引](README.md)

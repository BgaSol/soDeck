# TRAY-001 托盘交互实施结果

更新日期：2026-10-28 · 状态：工程完成，待用户验收

授权与规格：[F-CORE-004 r12 / TRAY-001 r1](../core/workbench.md#10-tray-001托盘单击与菜单代码方案)。用户“可以开始改了。”确认完整实施、验证及独立 Windows 试用目录；未提交、推送或发布。

## 实现与交付

- 左键单击松开后显示、必要时取消最小化并聚焦已有主窗口，不弹菜单；保留最大化、工作区与会话。
- 右键显示原生菜单“显示主窗口 / 退出 SoDeck”；显示项与左键共用已审计的 `app.show`。主窗“×”仍收起，退出沿用 `app.exit` 的全局清理和记录落盘。
- 显示操作与退出锁串行；缺少主窗、已停止、原生操作失败均返回实际错误，不新建窗口或重连。
- 必要附带修复：观测目录包含大数字边界时，重启会误报“历史类型版本的描述已改变”。比较改用与持久化相同的确定性序列化；实际描述变化仍拒绝。修复前回归失败见 `.qa/tray/catalog-before.log`，修复后通过。未改变 API、数据库格式、插件或产品范围。

试用入口：`.local/dev/SoDeck-v2-Tray/sodeck-desktop.exe`，也可运行同目录 `Start-SoDeck.cmd`。数据在该目录首次启动时新建的 `data/`，测试数据已转存 `.qa/tray/delivery-test-data`。目录包含三个官方插件、主题、图标及许可证。

基于 `v2` 未提交工作区，版本 `2.0.0-dev.16` / API 4.10 / core schema 5 / 观测 schema 2；此包包含工作区既有其他任务实现，不表示这些任务已验收。EXE SHA-256：

```text
a4faec684c0013364eb2a0ecc79ff12bd34414d618ee323719d5d7e29d05f729
```

本次应用差异相对 `.qa/tray/app-before.rs` 保存为试用目录的 `tray-application.diff`。核心修复位于 `crates/core/src/observability/catalog.rs`，回归位于 `crates/core/tests/observability.rs`；测试与打包入口为 `tests/desktop/tray*.mjs`、`tray-windows.ps1`、`scripts/package-tray-dev.mjs`，既有 `clean-start.mjs` 增加相关回归选择。

## 验证结果

环境：Windows 原生 Tauri / WebView2；真实系统鼠标输入及原生菜单，未预先聚焦 App。SSH/PTY 和 TCP 为本机隔离的 Paramiko 协议夹具，不是外部 Linux sshd 或用户业务服务器。所有抢占焦点的脚本串行执行。

| 检查 | 结果与证据 |
| --- | --- |
| TRAY-01 | 10 次隐藏/单击恢复，主窗 HWND、工作区及 PTY 保持，无左键菜单；`.qa/tray/native-1793162078719/results.json` |
| TRAY-02 | 最小化、最大化后隐藏、被子窗遮挡、前台重复点击、双击及连续 10 次点击通过；已有模态仍可操作，恢复前台焦点；同上 |
| TRAY-03 | 隐藏/可见时原生右键菜单、Esc、点击外部及键盘显示通过；截图 `native-1793162078719/menu.png` |
| TRAY-04 | 两个真实 PTY、独立工作区及 TCP 回环转发在隐藏时继续工作；菜单退出码 0，SSH 通道与监听关闭；同上及 `app/shutdown.json` |
| TRAY-05 | 先显示后退出、退出期间重复点击各 5 次，通过；`.qa/tray/exit-1793162121237/results.json`。每次退出均成功落盘且只记录一次；`.qa/tray/audit-verified.json` |
| TRAY-06 | 缺少主窗、停止状态、部分恢复失败及退出锁竞争的 3 项注入单元测试通过；原生错误阶段由源码传播审查覆盖。未声称实际 Windows API 故障注入 |
| 桌面 Rust | `node scripts/cargo.mjs check -p sodeck-desktop` 通过；`node scripts/cargo.mjs test -p sodeck-desktop`：44 通过，日志 `.qa/tray/rust-final-tests.log` |
| 核心目录 | `node scripts/cargo.mjs test -p sodeck-core --test observability`：32 通过、1 个子进程辅助用例 ignored；`.qa/tray/catalog-final.log` |
| 构建 | debug/release 构建通过；最终 release 日志 `.qa/tray/release-final.log`，目标 EXE 与交付哈希一致 |
| 启动相关回归 | `node tests/desktop/clean-start.mjs --tray-regression` 通过：托盘保持、拆合、渲染器恢复、最后终端关闭及普通/强制重启；`.qa/t009/1793161537959/results.json`、`.qa/tray/clean-start-scoped.log` |
| 最终便携包 | `node tests/desktop/tray-package.mjs` 通过：复制到中文目录，连续两次普通启动，共用独立便携数据，三插件目录正常，真实左键/右键/退出；`.qa/tray/package-1793162038071/results.json` |

图标位置覆盖：原生专项最终运行的 40 次目标定位均在可见任务栏；便携包在隐藏图标区验证，先展开系统箭头，再按测试进程的图标矩形点击。Windows 的 `Shell_NotifyIconGetRect` 在图标隐藏时返回箭头矩形，早期助手因此点错位置；`.qa/tray/icon-location.png` 保存复现，修正仅在测试助手，未弱化产品断言。最终包两次各有一次成功 `app.show` 和一次成功 `app.exit` 落盘。

清理核对：专项与包测试均正常退出，最终进程清单中无 `sodeck-desktop.exe` 或插件宿主残留；测试数据与证据保留在 `.qa/tray`。未改系统显示设置。

## 限制与独立失败

- 旧 `clean-start.mjs` 全量脚本的后段终端容量检查得到 7 个 PTY，预期 8 个，仍失败；增加等待未解决且已撤回，不归因为已确认的等待问题。本次相关前段通过，未把全量 T009 标为通过；见 `.qa/tray/clean-start-serial.log`。
- 该脚本旧 SSH 夹具不支持目录集成 exec；测试准备通过现有设置关闭目录集成，产品默认值不变，本次不验证 Bash 集成。
- 初次并行原生测试互抢焦点，以及 release 首次误用仅 debug 生效的调试端口，属于未通过的测试运行；原日志保留，最终均按正确准备串行复验。
- 只验证 Windows 便携目录；未验证安装器、升级、macOS/Linux，也未正式发布。现有构建警告未作为无警告构建宣称。

## 用户验收

1. 从上述独立目录启动，打开一个终端后点主窗“×”。
2. 左键单击托盘，确认主窗立即回到前台，终端和工作区仍在；最小化后重复一次。
3. 右键确认只有“显示主窗口 / 退出 SoDeck”，取消菜单不改变主窗可见性。
4. 选择退出，确认 App 与独立窗口关闭；再次启动正常且工作现场为空。

用户验收尚未记录；下一步等待用户对本次交互体验验收。

# T-010 实测与交付记录

更新日期：2026-10-26。依据 T-010 r2、组件通信 r1、目录联动代码方案 r2 及 Shell 启动补充 r1。验证对象为 `v2` 工作区 HEAD `d550c76` 加未提交的 T-008/T-009/T-010 工作；没有创建 Git 提交或发布。

## 实现和边界

Windows 开发版核心 `2.0.0-dev.6`，SDK/插件 API `4.2.0`，终端 `0.5.0`，文件 `0.2.0`。每服务器工作区仍只有一份文件树；终端切换选择对应来源，Bash shell 完成目录变化后报告实际 cwd，核心以有界状态订阅交付，文件插件独立验证进程路径视图并定位。文件手动浏览、隐藏和输入、取消、一次定位与关闭重开按功能规格处理；插件之间没有直接调用或句柄转交。视图呈现也复用同一个状态订阅机制。

终端默认开启 Shell 目录集成，设置可关闭且仅影响新建/明确重连的会话。集成登录比原 SSH shell 请求多一次非交互 `.bashrc` 读取，这是用户已确认的兼容边界；不改写配置文件、不注入既有终端命令。Zsh 5.9 在本轮保持普通终端功能，目录报告为 unsupported；tmux、嵌套 SSH、容器与 `sudo -i` 等内层会话不自动穿透。

## 实际验证

| 环境与入口 | 结果和证据 |
| --- | --- |
| `npm run check`、`npm run test:contracts`、`node scripts/cargo.mjs test --workspace --offline` | 全部通过；生成契约、TS、边界和 SDK/插件测试通过；Rust 工作区各项非 ignored 测试通过。`.qa/t010/check-final.log`、`contracts-final.log`、`rust-final.log`。 |
| `npx playwright test -c playwright.ui.config.ts` | 32 项 UI 回归通过；`.qa/t010/browser-final.log`。 |
| 原生 Tauri/WebView2 + 独立 Debian/OpenSSH 22281 | 初始目录、成功/失败 cd、两终端切换、手动路径、开关/一次定位、隐藏无文件远端 I/O、取消、独立窗口焦点、第二服务器隔离、整工作区拆合、关闭工作区重开、资源归零通过。`.qa/t010/native/results.json`、`hidden-io.json`、`native-final.log`。 |
| Bash 5.1.0 / 5.2.37、Zsh 5.9 | Bash login、cd/-/pushd/popd、函数、别名、复合命令、退出码与 PIPESTATUS、原始字节路径、软链、嵌套 shell 与用户钩子覆盖通过；Zsh 普通终端通过，无目录报告。`.qa/t010/linux/shell-matrix.json`；原启动关口失败记录保留 `shell-gate.json`。 |
| Windows 系统缩放 100/150/200% × graphite/paper | 六组合实际 DPI、键盘/焦点、主题、布局、减少动态通过；恢复原 200%。`.qa/t010/visual/results.json` 与六张截图。真实 Microsoft Pinyin 组词期间目录变化不覆盖草稿，结束编辑才定位；`.qa/t010/ime/results.json` 和可信输入事件。 |
| 新旧包矩阵 | 原终端/原文件、新旧交叉、双新、签名更新重启、偏好持久化通过；旧 API 4.1 核心准确拒绝新包，终端先因旧元数据解析拒绝，文件因新 API 要求拒绝。`.qa/t010/compatibility/results.json`。 |
| 16 worker/128 feed/12000 更新/60 秒 | 慢消费者暂停 10 秒后恢复；90,000 控制样本 ack p95 为 927 μs，峰值状态队列 35,216 B；`pressure.log`。真实 8 PTY、观察/显示独立、启动回执及重复操作通过 `pty-real.log`。 |
| 真实 SFTP/exec/PTy 回归 | 隔离 Linux 共享连接真实 I/O 通过，`remote-final.log`。 |
| 独立开发包搬迁启动 | 两份签名插件和核心按 `build.json` 哈希核对；整个包搬至含空格路径启动、离线错误可见、整工作区拆合、退出 worker 回收、重启空台通过。`.qa/t010/package-smoke.json`。 |

最终原生样本的文件路径视图映射约 557–743 ms，缓存/定位段 0–256 ms（18 次，后者 p95 256 ms）；远端映射单列。核心到 worker 的压力测试 ack p95 927 μs；这些数值不代表任意网络的端到端总耗时。隐藏区间的 SQLite 审计中，文件插件远端调用为 0。

## 交付与限制

独立 T-010 开发包在 `.local/dev/SoDeck-v2-T010/`；`build.json` 记录包哈希、源提交和未提交源码指纹。只附测试日志、结果、截图及必需许可证，不附 SSH 测试凭据、VM 磁盘或 App 数据。双击 `Start-SoDeck.cmd` 在包内独立 `data` 启动。此包是 Windows debug 试用版，用户验收和正式发布未发生。

T-008 原开发包、签名终端包及 150% 历史验证记录保持原状；T-010 独立完成六种原生缩放测试不追改旧任务。Linux proc/SFTP 身份无法验证、受限 namespace 或未知 shell 时目录降级，终端和手动文件浏览仍可用。远端身份核对与文件加载需要网络 I/O，不承诺固定端到端响应时间。

完成后独立 guest 正常关闭，overlay 保留用于复查，系统 DPI 恢复 200%，原 T-008 应用三个已运行进程及原终端包哈希未变化；见 `.qa/t010/cleanup.json`。本次压力样本量化了 host→worker 的 ack 时延与队列峰值，本地 UI 缓存定位段有独立耗时；未把这些样本外推为所有网络的完整用户可见 p95。正式安装器、macOS/Linux 客户端和用户验收不在此开发包内。

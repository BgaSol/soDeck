# SSH 端口转发实施与验证记录

日期：2026-10-16。分支：`codex/ssh-port-forwarding`。

代码已实现，可在独立工作树运行和评审。三平台原生验收尚未全部完成，不能据此宣称 A01–A16 全部通过。

## 功能与入口

- 主窗口标题栏“端口转发”采用 32px 图标入口，以小圆点提示运行／需处理状态；名称与具体数量位于悬停提示和辅助阅读描述中。
- 服务器选择窗口和会话工具栏的“端口转发”筛选该服务器的已有规则，新建时预选服务器。
- 独立管理窗口提供新增、搜索、编辑、启动、取消、停止、删除、停止并应用、重新认证、目标连接测试、地址/主机/端口复制和 HTTP(S) 打开。
- 支持 IPv4/IPv6 回环、固定/自动端口、直连或单层跳板，以及最终 SSH 服务器解析目标域名。
- 关闭已就绪规则的管理窗口后继续运行；关闭首次启动中的窗口会取消启动。
- 自动重连保持端口和运行配置，关闭旧业务连接；身份变化进入需处理，旧请求不会重放。
- 规则使用独立 `port-forwards.sqlite`，包含版本冲突检查；不改变 `servers.db` 的 schema。退出应用、删除服务器和重建数据库都检查活动规则。
- SSH 通信统计新增端口转发分类，规则详情显示独立的业务字节统计。

## 实现结构

| 模块 | 职责 |
| --- | --- |
| `ssh_transport/forwarding.rs` | 独占 SSH session 的多通道调度、稳定缓冲、半关闭、关闭握手、取消及 readiness 等待 |
| `ssh_transport.rs` | 原跳板适配至同一 ForwardCore，删除旧 pump；保留现有 forward 接口和失败阶段 |
| `ssh_transport/resolver.rs` | 4 个系统 DNS worker、16 项等待队列；数字 IP 直接使用，业务目标不在本机解析 |
| `connection.rs` | 通用认证事件 sink、无终端连接入口、已验证路径重连、运行内固定主机指纹 |
| `port_forwarding/{model,store,mod,worker}.rs` | 规则与运行快照、CAS 存储、资源登记、监听、连接配额、重连和清理 |
| `port_forwarding/{commands,observe}.rs` | 单实例窗口、IPC 权限与代次校验、应用意图、带 ACK 的状态流 |
| `PortForward*.tsx`、`usePortForwards.ts` | 管理界面、独立草稿、认证队列、确认操作和订阅恢复 |

每规则最多 64 条通道、8 项等待打开的请求；全局最多 256 条业务连接、32 条运行规则。每通道两块固定 32 KiB Rust 缓冲，libssh2 自身的接收窗口另计。控制队列容量 16；每规则最近事件最多 200 条，相邻重复事件合并计数。

打开通道超时 12 秒，关闭超时 5 秒，挂起的协议发送上限 30 秒。超时后无法安全继续使用的 SSH session 整体释放，由规则决定重连。退避为约 1、2、4、8、16、30 秒并带少量抖动。

TCP 保活请求采用空闲 30 秒、间隔 10 秒；Linux/macOS 请求最多 3 次探测，Windows 使用系统支持的次数。SSH 保活由单一 owner 调度，约每 20 秒发送。libssh2 0.3.3 所携带实现的 keepalive API 会吞掉部分发送的 EAGAIN；检测到 outbound 未完成时立即淘汰此路径，避免复用悬挂的协议发送。实际断网检测时间受系统及网络行为影响。

## 相对设计稿的落地调整

1. SQLite 保存索引列 id/revision/server_id 与经过验证的 JSON configuration，仍以独立 schema 版本和事务 CAS 管理；密码、私钥口令及运行状态不写入规则库。
2. 认证继续使用既有 ConnectionEvent/ConnectionAnswer 和包含 hop/stage 的 AppError，运行归属、attempt、目标和时间由 Runtime/Prompt/事件包装，不重复引入另一套终端协议。
3. 管理窗口采用单一完整状态流，每 250 ms 至多发送一帧，只有收到精确 ACK 后才发送下一帧；15 秒未 ACK 注销观察者。速率每秒采样。当前认证挑战持续保留在运行状态，直到回答、取消或超时，因此同一状态流可重放挑战且不会因合并丢失。前端按订阅代次和 revision 丢弃旧帧，5 秒无新帧会重新订阅。
4. 安全服务器选项随状态帧更新；表单草稿独立，不受状态刷新覆盖。主窗口摘要采用串行定时查询，不复制认证信息。
5. 配置读取故障时仍显示已登记运行的紧急快照，允许停止，不用空数据库替换原文件。

## 已执行验证

所有 SSH 测试使用生成的临时密钥、临时数据库和隔离服务，不读取用户保存的服务器配置或连接业务服务器。

| 检查 | 结果与范围 |
| --- | --- |
| Windows `cargo check` | 通过，无新增编译警告 |
| `npm run build` | 通过；现有主包大小提示仍存在 |
| `npm test` | 95 项通过 |
| 端口转发 Playwright | Chromium/WebKit 共 8 项通过；真实地址、保存并启动、编辑隔离、显式应用、连续认证、旧帧丢弃 |
| 连接与标题栏 Playwright | 原 50 项通过，更新退出摘要 mock 后另 2 项重跑通过，共 52 项 |
| Linux Rust 默认套件 | 109 项通过、33 项明确忽略，忽略项不计为通过 |
| Linux 转发专项 | 6 项通过；生命周期、身份固定、窗口代次、端口冲突、IPv6、ACK 背压、并发半关闭、慢读端隔离、探测取消 |
| Windows 转发专项 | 5 项通过；含 4×3 MiB 并发逐字节比较、半关闭、慢读端、重连、固定身份和资源释放 |
| 既有真实 SSH 跳板回归 | 8 项通过；终端、SFTP、文件下载/删除、日志、Docker、资源监控、私钥、取消、断线及计量 |
| 隔离 OpenSSH A/B | 通过；MaxSessions=0、AllowTcpForwarding=local、PermitOpen 限制，直连与跳板，远端域名、并发、规则隔离、目标拒绝；验证 session channel 被拒绝时转发仍工作 |

Linux 重连 fixture 曾因 TIME_WAIT 无法重绑测试端口；仅在 Unix fixture listener 设置 SO_REUSEADDR 后重跑通过。产品监听策略未因此放宽。

原跳板筛选 `jump_` 曾额外选中需要专用容器环境的新 OpenSSH 测试，导致缺少环境变量的失败；该测试更名以分离测试入口。专用 OpenSSH 环境已单独验证通过。

## 原生验证限制

- Windows 单元测试 EXE 初始缺少 Common Controls v6 manifest，导致入口加载错误。提供仅处理测试产物的脚本；专项测试随后通过。最后一次 Windows 全量测试启动被本机应用程序控制策略阻止，未关闭或绕过该策略；对应全量套件在 Linux 运行通过。
- macOS 原生构建/运行未执行；WebKit 浏览器测试不等同于 macOS 桌面验收。
- 未执行真实系统休眠/恢复、长时间黑洞网络、平台 SSH Agent 全组合、系统默认浏览器和桌面窗口完整手工验收。
- 未做长期吞吐、CPU/RSS 基准或最大连接数压测。当前上限是实现的资源约束，不是性能承诺。

A01/A02/A04/A07/A10/A11/A12/A14 的核心行为已有协议或状态测试证据；A03/A05/A06/A08/A09/A13/A15 有相应覆盖但仍需平台场景补充。A16 尚未完成。发布前按功能需求的验收表补齐上述原生检查。

## 复现入口

前端：`npm run build`、`npm test`、`npm run test:port-forwarding`、`npm run test:connection`。

Rust 默认检查：`cargo test --manifest-path src-tauri/Cargo.toml --lib`。

安装 tests/requirements.txt 后，指定 SSH_TEST_PYTHON，执行：

```text
cargo test --manifest-path src-tauri/Cargo.toml --lib forward_ -- --include-ignored --skip forward_openssh_ --test-threads=1
cargo test --manifest-path src-tauri/Cargo.toml --lib connection::tests:: -- --ignored --skip jump_agent_authentication --test-threads=1
```

Windows 可用 `tests/port-forwarding/run-windows.ps1` 将上述测试参数传给带 Common Controls manifest 的测试 EXE；仍遵守本机应用控制策略。

OpenSSH 容器：先按 `tests/rsync/README.md` 构建 `ssh-workspace-rsync-fixture` 与 `ssh-workspace-rsync-runner` 两个镜像，再从 Linux/WSL 执行 `python3 tests/port-forwarding/environment.py`。脚本创建专用网络和临时 A/B 容器，不映射宿主端口，结束时清理容器和网络。


## 2026-10-17 界面与操作调整

已落实确认后的界面方案，复用系统字体、主题变量、统一标题栏、居中弹窗标题和现有路径节点样式。侧栏使用淡蓝选中态；详情保留访问地址、路径和流量，诊断默认折叠。编辑／删除及复制主机／端口收入锚定菜单，支持键盘导航和恢复焦点。复制反馈留在按钮原位。

新建采用分组表单与高级折叠，目标地址默认 127.0.0.1，目标端口不预填；固定端口跟随建议直到手动编辑，切换自动分配保留固定草稿。运行中编辑默认下次生效，可选择保存后重新启动；中断确认在写入前完成，保存与启动失败分别反馈。状态流不覆盖草稿，服务器入口不再自动弹出新建。

后端仅扩展窗口导航请求和目标连接结果的状态表达，保留原有转发内核及认证、运行快照、后台生命周期。新增取消探测迟到失败过滤与重连后目标结果失效处理。

本次在 macOS 工作区执行的验证（与上文初版记录分开）：

| 检查 | 结果 |
| --- | --- |
| `npm run build` | TypeScript 与 Vite 构建通过；原有主包大小提示仍在 |
| `npm test` | 98 项通过，包括端口草稿、主机校验与运行／保存配置切换 |
| `npm run test:port-forwarding` | Chromium/WebKit 共 24 项通过；补充复制菜单状态变化和运行／保存路径对照后，相关 4 项再次通过 |
| `npm run test:connection` | Chromium/WebKit 共 52 项通过 |
| Rust 转发专项 | 7 项通过，含真实 SSH 生命周期、重连、取消探测迟到结果、并发半关闭、IPv6、窗口代次与 ACK 背压；独立 OpenSSH 容器项未在本次重跑 |
| 视觉检查 | 检查实际组件截图的 1100×760、800×560、浅色／深色、长名称、IPv6 与高级表单；最小窗口正文可滚动，底部操作可见 |

截图由 `tests/port-forwarding-ui.spec.ts` 输出到 `.qa/forwarding-actual-{chromium,webkit}-*.png`，使用模拟 IPC 数据；不能代替原生窗口手工验收。本次 Rust 代码在 macOS 完成编译及隔离 SSH 测试，未执行原生桌面窗口、系统浏览器或系统剪贴板的手工端到端验收。

真实 SSH 专项复现（使用安装了 tests/requirements.txt 的 Python）：

```sh
SSH_TEST_PYTHON=/path/to/python cargo test --manifest-path src-tauri/Cargo.toml --lib forward_ -- --include-ignored --skip forward_openssh_ --test-threads=1
```

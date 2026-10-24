# T-003 核心实施与验证记录

日期：2026-10-25 · 状态：核心构建、协议、日志故障与 60 秒负载测试通过；完整桌面验证见 T-003 主记录

## 实施结果

`crates/core` 已实现服务器 SQLite 存储与配置修订、Windows DPAPI 凭据、先验主机指纹确认、SSH 连接复用与独立租约、多 PTY、有界输出与 read/ack、输入序号、视图挂载代次、实例/应用退出清理、逐次调用记录、插件设置命名空间及诊断。API 字段见[联调契约](../technical/implementation-contract.md)。插件能力没有按来源建立权限白名单，来源由宿主绑定，用于归因和资源生命周期。

SSH 连接在后台执行，DNS/TCP/握手等待可被实例结束或 App 退出取消。新建 PTY 以非阻塞阶段推进，避免等待新通道时阻塞同连接现有通道的 I/O 或关闭请求。输入按序号去重；同一视图和代次的重复绑定可安全返回，旧挂载不能继续读写。

调用开始先持久化，存储不可写时普通调用返回 `LOG_UNAVAILABLE`，不执行副作用；资源清理和 `diagnostics.get` 仍可执行，并把记录缺口加入最多 1,024 条的内存故障队列。执行后结果写入失败返回 `RESULT_UNKNOWN`；重启把遗留开始记录标为 `interrupted`，不补写成功或重放。查询支持插件、方法、服务器、状态、时间游标和限额，所有查询本身也留痕；参数摘要不保存密码、密钥、输入输出正文。

## 构建依赖

Windows 使用 ssh2/libssh2 的静态 OpenSSL 后端，私钥在内存解密/认证，不创建明文私钥临时文件。原先 WinCNG 在新增 RSA OpenSSH 格式实测时失败；核查 loader 仅支持 PEM 后改为上游 OpenSSL 后端，API、连接 worker 与工作台边界保持既定方案。

静态 OpenSSL 构建成功，首次耗时 5 分 28 秒。OpenSSL 构建需要原生 Windows Perl；Git 自带 Perl 缺 `Locale::Maketext::Simple`。本机在忽略目录 `.local/build-tools/strawberry` 使用官方 Portable Strawberry Perl 5.32.1.1，下载 SHA-256 与官网 releases.json 的 `692646105b0f5e058198a852dc52a48f1cebcaf676d63bbdeae12f4eaee9bf5c` 一致，未安装到系统。仓库构建脚本会检测该本地路径，或可配置 `OPENSSL_SRC_PERL`。Perl 仅是构建依赖，产品运行无需解释器。

OpenSSL 静态库缺附带 PDB 时，MSVC 输出 LNK4099 调试符号警告；链接和运行测试成功。没有使用 WinCNG 的本地 vendor 补丁。核心测试用 `.qa/core-check/Cargo.toml` 直接引用正式源码，以独立 target/Cargo 缓存避免与桌面构建争锁，不存在第二份核心实现。

## 本机实测结果

- 单元测试 **7 项通过**：存储重启/参数及修订冲突；秘密标记不进日志；过期实例；插件设置隔离与无能力 ACL、按插件查询；真实 SQLite 写入失败、清理与诊断继续可用、结果待核对及重启中断；PTY 尺寸与输出确认边界。
- 真实 loopback SSH 协议测试 **2 项通过**：密码及错误后重新提交；加密/未加密 RSA PEM、RSA OpenSSH 和 Ed25519 OpenSSH 私钥；主机信任前不认证、主机密钥改变时在认证前拒绝；DPAPI 重启；共享连接/独立租约、多 PTY 隔离、释放；不含换行的普通输入及时回送；重复输入与过期挂载拒绝；对已接受但不发送 SSH banner 的真实 socket，在 App 退出时 500 ms 内取消等待。夹具的流控制行识别与交互回送分开，避免负载模式使普通输入变成按行缓冲。
- 最新非阻塞 PTY 调度与即时回送夹具版本的 `ssh_load --ignored` **通过，61.91 秒**：4 个 PTY 各 256 KiB/s，持续 60 秒；暂停第一个消费者 10 秒后，输出队列峰值恰为 **2,097,152 字节**。四路各校验 **1,920 个连续 8 KiB 编号块**，无缺口或重复，各接收约 15.72 MiB。输入标记 **P95 21 ms**，资源查询 **P95 1 ms**。另 **20 次连接/PTY 创建关闭**后，核心索引及夹具实际 SSH socket 均恢复基线。
- Windows Rust 测试进程 working set：基线 **6,930,432 字节**、峰值 **24,911,872 字节**、清理后 **16,191,488 字节**。采样对象是测试进程；未测 WebView 或整个桌面 App 内存。

脱敏机器结果 `.qa/core-load-result.json`，命令输出 `.qa/core-unit-run.log`、`.qa/core-protocol-run.log`、`.qa/core-load-run.log`。这些是本机忽略的运行证据，以上数字和范围保留于本记录。

## 复跑入口与范围

### 接续审查：资源关闭与迟到 PTY（2026-10-25）

本次仍属于 T-003 r1 已确认的取消、独立清理及资源回收范围。审查发现：`pty.open` 等待期间租约被释放后，迟到的通道结果可能重新登记；PTY 关闭与输入共用有界队列，队列满时清理可能失败，资源管理停止路径还可能提前移除索引。修复保持原有 API 与共享连接语义，关闭信号由现有 PTY 状态传递，完成创建前复核实例、租约、连接及应用代次。

实施前约定的回归流程：在 loopback SSH 夹具中保留第一条可用 PTY，并暂停第二条 shell 受理；释放第二条所属租约后再放行响应，预期第二条创建失败、没有遗留 PTY，第一条仍能输入回送。另对已满控制队列验证关闭信号仍可受理，并重跑核心单元与协议测试。仅测试夹具允许通过隔离目录的 marker 暂停 shell；不向产品加入故障开关。结果不代替 Linux/OpenSSH 或原生验证。

同次补齐原方案要求的单次连接取消：`connections.acquire` 和 `pty.open` 以同一可选 `operationId` 关联，`operations.cancel` 只取消相同实例的该操作；取消先抵达时保留标记，排队等待、握手及 PTY 等待均响应取消，不关闭其他会话复用的连接。标记保留 10 分钟，最多 10,000 项，活跃项不被驱逐。补验提前取消、实际无 banner socket 单次取消、共享连接中延迟 PTY 取消及原连接继续输入；桌面与 worker 负责取消后的迟到会话回收，由其结果另记。

本轮工作区实测：

- 核心单元 **9 项通过**，包含控制队列已满仍可关闭、重复关闭、关闭后拒绝输入；提前取消的实例隔离，以及 SQLite 记录不可写时取消仍可执行。`workbench.cancelOpen` 与 `operations.cancel` 均列入记录故障时仍允许的清理调用。证据 `.qa/core-operation-unit-run.log`。
- `ssh_protocol -- --include-ignored --nocapture` **4 项通过**：保留全部密码/密钥/主机校验流程，新增真实 socket 中排队及握手单次取消均在 **500 ms 内**返回且互不影响；共享连接中释放租约后的迟到 PTY 被拒绝且远端通道关闭；第二次延迟 shell 的取消在 **500 ms 内**返回，原会话继续回送。证据 `.qa/core-operation-protocol-run.log`。认证阶段的取消和超时保持 `CANCELED/TIMEOUT`，不再被包装成认证失败。
- 修改 SSH 调度后重跑负载与回收，**通过，61.84 秒**：四路各 **1,920** 个连续 8 KiB 编号块，暂停消费者仍到达 **2,097,152 字节**预算；输入 **P95 19 ms**、资源查询 **P95 1 ms**；**20 次创建/关闭**后核心索引及实际 SSH socket 回到基线。测试进程 working set 基线 **7,598,080**、峰值 **25,481,216**、清理后 **17,219,584** 字节。证据 `.qa/core-operation-load-result.json`、`.qa/core-operation-load-run.log`；未测 WebView 内存。

上述命令通过 `node scripts/cargo.mjs` 执行正式 workspace 的 `-p sodeck-core`，并使用已缓存 rsproxy 的 offline 配置。隔离 core-check 的一次冗余构建随后也通过 9 项单元；以正式 workspace 上述结果作为本轮依据。没有改插件管理器/宿主代码，其先前验证范围与未覆盖更新分支保持 [插件记录](T-003-plugin-results.md) 所述。

### 原生取消集成补验计划（2026-10-25，实施前记录）

针对同次取消修复，新增 `node tests/desktop/cancellation.mjs`，等待核心执行文件与实际终端包重建后运行。使用真实 Tauri、QuickJS worker、shell 和 raw Wry 页面，以 CDP/RPC 驱动，不使用原生全局键鼠或前台窗口控制。隔离 loopback socket 接受 TCP 后不发送 SSH banner，待 `pendingOpens` 与实际 socket 均出现，通过 shell 的取消按钮取消：预期 1 秒内结束为 `CANCELED`，没有遗留终端视图、PTY、租约或连接，实际 socket 关闭。

另以同一标识先调用 `workbench.cancelOpen` 再创建终端，预期直接取消且不建立 socket；随后连接独立 Paramiko 协议夹具取得实际 PTY，对已经完成的创建操作再次取消，预期返回 `canceled:false` 和原 viewId，原视图、PTY 及回送继续。最后通过 `app.exit` 正常退出并断言退出码为 0，实际 SSH 通道/socket 回到基线；保存执行文件/终端包哈希、延迟、调用来源记录和截图到 `.qa/cancellation-native/<run>/`，清理 fixture 与临时测试私钥。该计划不改变既定交互与验收目标，结果运行后补充。

本轮已执行 `node tests/desktop/cancellation.mjs`，**5 组检查通过**，证据 `.qa/cancellation-native/1792872576744/`。真实 shell「取消连接」按钮在 **36 ms** 内使无 banner SSH 建连返回 `CANCELED`；资源、视图、worker 会话及实际 socket 无遗留。提前取消未产生新 socket；完成后的取消返回 `canceled:false` 与原 viewId，原连接/租约/PTY 标识不变，原生 xterm 显示 `completed-operation-still-alive` 实际回送。调用记录确认 `operations.cancel` 和被取消的 `connections.acquire` 均归属于实际 `sodeck.terminal` worker。App 正常退出码 **0**；Paramiko 记录 **2 次 connection-open / 2 次 connection-close**、**1 次 shell-open / 1 次 shell-close**，测试 fixture 与临时私钥已清理。

已查看 `pending-connection.png`（显示正在连接及取消按钮）和 `completed-operation-preserved.png`（显示已连接与回送），均来自实际原生 WebView 页面；没有以浏览器注入提供者代替。执行文件 SHA-256 `cc240635acc5f20a1b82893eddcde722bfbd7ea43f8370b2294d68ec51f35d7c`，实际出厂终端包 SHA-256 `591ab37668e4138c93fbf34e821ef0586ceda4f66d5625d76358aa503d8331dd`。退出日志保留 WebView2 窗口类注销提示，正常退出码及通道关闭已分别断言；没有改动执行文件或包来运行测试。

单元入口为 `cargo test -p sodeck-core --lib`。协议入口为 `cargo test -p sodeck-core --test ssh_protocol -- --ignored --nocapture`；负载入口为 `cargo test -p sodeck-core --test ssh_load -- --ignored --nocapture`。测试需要按 `tests/core/requirements.txt` 安装 Paramiko，并通过 `SODECK_TEST_PYTHON` 指定 Python；可用 `SODECK_CORE_LOAD_RESULT` 指定负载 JSON 输出位置。等待 banner 的取消测试不依赖 Python，普通 `cargo test` 会执行。

本机没有 Linux/OpenSSH 服务；按用户决定延期真实 Linux 验证。`tests/core/ssh_fixture.py` 只监听 loopback，执行真实 SSH 协议、PTY 字节回送及有序负载，**不执行 Linux shell**，不能代替 Linux/OpenSSH、原生 IME/多屏或整个 App 内存验收。完整工作区重新构建及原生窗口场景由 T-003 主任务汇总，不能从核心测试推定通过。

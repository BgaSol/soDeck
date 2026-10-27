# T-008 实测与交付记录

更新日期：2026-10-26。适用已确认的 T-008 r4、代码方案 r1、F-FILES-001 r4、F-CORE-009 r3。验证对象为 `v2` 工作区，基于 HEAD `d550c76`，包含保留的 T-009 未提交改动；未创建提交或发布。

后续兼容修复：[FIX-FILES-001 / F-FILES-001 r7](../plugins/files/features/server-file-tree.md#fix-files-001特殊源内根导致挂载表不可用) 记录文件 0.3.1 对 nsfs 特殊源内根的解析修复、真实服务器复现/验证和独立更新。下文保留原 T-008 交付事实。

OpenWrt 后续兼容：[COMPAT-FILES-002 / F-FILES-001 r10](../plugins/files/features/server-file-tree.md#compat-files-002openwrt-挂载兼容) 已交付文件 0.3.2，使用 procfs 根对应和 df 容量回退；真实 OpenWrt/GNU Linux、原生更新及字段降级结果由该轻量条目维护。

## 1. 版本与实现

| 项目 | 本次版本与边界 |
| --- | --- |
| 核心 | `2.0.0-dev.5`，Windows debug 开发目录包 |
| SDK / 插件 API | `4.1.0`；工作台 schema 和整数 API 仍为 4 / `workspace.v4` |
| 文件插件 | 独立 `sodeck.files` `0.1.0`，UI、状态机和 Linux 挂载解释都随插件交付 |
| 终端插件 | 原签名包 `0.4.0`，SHA-256 `cee21412074f6f39c82e092dcb3a6d38919df3bd9b3424fd58a703939e8fe1b2` |

每个服务器工作区默认打开左侧文件树，支持路径定位、按需展开、文件大小和精确字节数、挂载标记及详情、刷新、隐藏文件和复制路径。可切换左右侧、收起、随完整工作区拆合；树与侧栏现场只属于当前工作区创建期间。重启空台、首终端及已保存配置遵循 T-009。

核心新增字节路径 SFTP、有界非 PTY exec、仅复用现存连接、操作回执/取消/预算及通用多插件所有者路由。文件与终端分别持有租约，底层连接可共享；插件之间不互调。原始文件名、大小与 Linux 挂载解释分别在协议、插件层处理。

## 2. 环境和证据边界

- Windows 11 专业工作站版 `10.0.26300`，原生 Tauri/WebView2；Intel Core Ultra 5 250K Plus，18 逻辑 CPU，约 32 GiB RAM。实际原生窗口 DPI 为 192（200%）。
- 独立 QEMU 11.1.0 TCG、Debian 13、Linux `6.12.113+deb13-cloud-amd64`、OpenSSH，2 vCPU / 2 GiB RAM / 12 GiB overlay。SSH 仅绑定宿主 `127.0.0.1:22280`，无宿主目录共享、未安装 Windows 服务。
- 普通账号执行产品操作；管理员仅在隔离 guest 内准备 ext4/bind/只读 bind、嵌套 tmpfs、叠加挂载、Btrfs 子卷、guest 回环 Samba/CIFS、受限 chroot/internal-SFTP 和 10,000 文件夹具。
- 浏览器注入桥、Rust 单元/契约、回环 SSH 协议、原生桌面和真实 Linux 分层记录；故障包是实际文件插件派生的签名测试夹具，不作为第三个产品插件交付。
- 先前提供的 Ubuntu 22.04 端点只做过基础只读 SSH/SFTP 检查；本次完整文件插件验证使用隔离 Debian，未在该端点建立夹具，也未宣称该端点完成验收。

工作区证据位于 `.qa/t008/`；开发包附带精选 `evidence/` 和 `screenshots/`。私有测试配置、SSH 私钥、VM 磁盘及 App data 不进入交付目录或源码版本控制。

## 3. 已运行检查

| 命令 / 证据 | 实际结果 |
| --- | --- |
| `npm run check`；`check.log` | 通过：生成契约新鲜度、TypeScript、依赖边界 |
| `npm run test:contracts`；`contracts.log` | 54 项通过：共享契约、文件规则/状态、SDK、终端 worker/输入 |
| `node --test scripts/dependency-boundaries.test.mjs`；`boundaries.log` | 通过：包含违规依赖负例，文件业务不进入核心/终端 |
| `node scripts/cargo.mjs test --workspace`；`rust-tests.log` | 66 项通过；需外部服务的用例显式 ignored，不计为通过 |
| 配置 `SODECK_TEST_REMOTE_CONFIG` 后，`node scripts/cargo.mjs test -p sodeck-core --test remote_read -- --ignored --nocapture`；`core-remote.log` | 真实 Linux 用例通过：SFTP/PTY/exec 并发、特殊字节、分页重放/冲突、超限/超时、错所有者/代次拒绝、取消与最终实际通道归零 |
| `npx playwright test -c playwright.ui.config.ts`；`browser.log` | 32 项通过，含文件树 3 项及延迟路径定位回归；使用模拟桥，不替代原生 |
| `node tests/desktop/clean-start.mjs`；`clean-start.log` | T-009 单插件受影响启动/首终端回归通过 |
| `node tests/desktop/clean-start-plugins.mjs`；`clean-start-plugins.log` | T-009 插件初始化、缺失/更新回归通过 |
| `node tests/desktop/files.mjs`；`native-files/results.json` | 实际双插件、Linux 文件/挂载、权限、两服务器、拆合及关闭资源通过 |
| `node tests/desktop/files-platform.mjs`；`platform/results.json` | 同名命令/初始化按所有者隔离、目标表面失败全体回滚、文件默认视图失败不阻断首终端通过 |
| `node tests/desktop/files-updates.mjs`；`updates/results.json` | 文件独立更新/坏候选回退、准确 worker PID 故障、终端存活、跨插件冒充拒绝通过 |
| `node tests/desktop/files-degraded.mjs`；`degraded/results.json` | 第二来源准备取消回滚、关闭最后终端后浏览、真实 chroot 降级、取消认证后显式文件连接通过 |
| `node tests/desktop/files-visual.mjs`；`visual/results.json` | 两主题、1280×800 / 900×600 逻辑客户区、150% 文字放大、高对比/减少动态模拟、键盘/剪贴板、左右侧栏、刷新焦点和托盘恢复通过；实际系统缩放为 200% |
| `node tests/desktop/files-load.mjs`；`load/metrics.json` | 受控性能目标及 20 次打开/关闭资源循环通过，见下一节 |
| `node tests/desktop/package-smoke.mjs`，指定 T008 包目录；`package-smoke.json` | 通过：整包白名单/哈希，复制到含空格的新目录并以新目录为 cwd，双插件加载、离线首终端错误保留、拆合、正常退出、插件 worker 退出、重启空台与配置保留 |

各原生脚本串行运行，以免焦点相互影响。最后修复旧响应式 CSS 将侧栏压到 210px 的问题，按已确认最小宽度移除旧限制；重建外壳后重跑受影响的原生视觉与目录包流程。核心 I/O、插件包和路由逻辑未再修改，早先全套结果保留其实际构建依据；原生视觉在两种尺寸均增加侧栏 320 CSS px 断言。

## 4. 用例对应和实测细节

| 范围 | 证据和可观察结果 |
| --- | --- |
| FT-01/02/08 | 两个真实插件、每服务器一份文件树；默认 320 CSS px。0/1023/1024 字节、稀疏 5 GiB、反斜杠/非 UTF-8/换行名称、链接/FIFO 分别处理；直接定位第 9,999 项无需读取全部兄弟节点。超 2^53 大小另由协议夹具验证 |
| FT-03/04 | ext4、bind、只读 bind、嵌套 tmpfs、真实 CIFS 和 Btrfs 来源/容量一致；同目标叠加挂载保持不确定提示，不按记录顺序猜有效层。规则夹具覆盖路径段边界、最长匹配及敏感参数脱敏 |
| FT-05/06/07 | 真实无权限目录保留错误，chroot/forced internal-SFTP 仍能浏览且明确挂载不可核实。状态/桥夹具验证删除节点后退回祖先、刷新竞态、迟到回执、有限缓存；原生和核心验证取消/断线不误断终端 |
| FT-09/10、MP-03 | 原生成功拆合保留原 PTY、选择及归属；第二插件 prepare 取消和目标 UI 故障全体回滚。两主题截图已人工查看，大小列/精确值可读；长名可横向滚动、详情可滚动。原始 UTF-8 路径复制保留真实换行，非 UTF-8 明示“复制转义路径” |
| FT-11、MP-02/04 | `0.1.1` 派生包同时改变文件 UI 版本和挂载解释文字；pending 仅重启生效，随附旧包不覆盖 pending。`0.1.2` 初始化失败回退并撤销候选存储写入；核心与原终端哈希不变。准确终止文件 worker 后终端继续、文件可恢复 |
| FT-12/13/14、RI-06/07、MP-01 | 独立租约共享连接、文件默认视图失败不阻断首终端、取消认证不自动重试、显式文件连接可用；关闭文件保留终端，结束工作区清理所属使用权。两插件同名本地命令/动作不串用，冒充其他来源被拒绝。重启空台及配置保留另由更新/T-009 回归验证 |
| RI-01 至 05 | 字节路径和十进制长度契约；真实 SFTP 及非 PTY exec 分帧/ack、回执重放/冲突、过量输出、超时、取消 PTY 创建后 SFTP 仍可用。单元验证 TTL 和未领取租约清理，集成验证所有者/创建代次和多来源绑定回滚 |

真实 OpenSSH 的 `sleep 20` 可延迟 channel-close 确认：本地句柄撤销后，原生计数如实保持 `cleanupPending`，终端仍可用，远端命令结束后计数归零。该情况不能描述为 5 秒内任意远端进程已终止；正常已完成读取关闭另验 5 秒内归零。

原生视觉脚本原先误把 1280×800 / 900×600 直接传给物理像素 API，已修正为按 DPI 换算并断言 WebView 的实际逻辑客户区；最新 `visual/results.json` 同时保存物理窗口、DPI、逻辑尺寸及 devicePixelRatio。

## 5. 性能

在上述本地隔离环境、10,000 条目、双服务器工作区与终端并存条件下，P95 结果如下；完整样本见 `load/metrics.json`。

| 指标 | 样本 | P95 | 已确认目标 |
| --- | --- | --- | --- |
| 首批目录加载 | 20 | 516.16 ms | ≤2,000 ms |
| 终端空载回显 | 100 | 68.68 ms | 对照 |
| 文件浏览负载下回显 | 100 | 68.21 ms | ≤150 ms，较空载增量 ≤100 ms |
| 本地选择 | 30 | 7.48 ms | ≤100 ms |

曾发现 mount 匹配在每个 snapshot 中重复构建导致首批超标，已改为缓存字节前缀索引后重测通过；没有放宽指标。此结果不是任意网络或远端性能承诺。

## 6. 交付、清理及剩余验证

交付目录 `.local/dev/SoDeck-v2-T008/`，入口 `Start-SoDeck.cmd`；[试用步骤](T-008-acceptance.md)。`build.json` 记录版本、每文件/插件哈希、HEAD、dirty 状态和包含未跟踪源码的工作区指纹。打包只取白名单产物，不复制测试 App data、SSH 凭据或签名私钥。

目录迁移冒烟已通过，证据 `.qa/t008/package-smoke.json`。原生可执行文件与实际插件验证的是同一组哈希，后续打包仅同步文档/证据；最终清单逐文件复核。

隔离环境按层级卸载 9 项 fixture 挂载（含同目标叠加的两层），停止 guest Samba，测试账号无残留进程，再正常关闭 guest；宿主精确核对记录的 QEMU PID 已退出、22280 监听消失、工作区测试 App/插件进程为空。首轮 PID 16372 清理后，视觉修复补测使用同一磁盘重新启动并再次执行相同清理，最终 PID/状态见 `.qa/t008/cleanup.json`。未强杀 VM，未在挂载目录递归删除文件；磁盘和脱敏证据保留。可重复清理入口为 `tests/linux/t008/cleanup.py`。

FT-10 的 **150% 系统缩放尚未实测**：本机为 200%，已完成原生 200% 与 150% 文字放大，两者不能冒充 150% 系统 DPI。尝试通过 Windows 设置补测，但 `SystemSettings.exe` 和显示设置 URI 均未产生可操作设置窗口；尚未改变用户缩放设置。保留此验证缺口，不据其他 DPI 的通过写成全矩阵通过。

首轮范围为 Windows 客户端与 Linux/OpenSSH 只读访问，不提供文件修改/传输/目录递归大小/挂载管理；未知容量和无法证明的挂载关系明确降级。未执行安装器、macOS/Linux 客户端或正式发布验证。用户验收仍待用户反馈。

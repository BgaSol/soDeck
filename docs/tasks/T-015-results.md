# T-015 实现与验证记录

日期：2026-10-27。基线：[任务 r3](T-015-workflow-polish.md)、[代码方案 r2](../technical/workflow-polish-implementation.md)。用户已明确确认实施；在 `v2` 未提交工作区完成，保留此前 T-008～T-014 修改。用户验收、正式发布、Git 提交和推送均未进行。

应用 **2.0.0-dev.12**，插件 API/SDK **4.8.0**，公共 UI **1.2.0**；终端 **0.8.0**、文件 **0.5.0**、网络隧道 **0.2.0**；数据库结构 **v4**。Windows 目录包使用 release profile，不是正式安装器。

## 交付状态

13 项体验修改及 P-52 数据库规则已实现，必要功能检查通过，进入用户验收。

- 最终目录：`.local/dev/SoDeck-v2-T015/`；双击其中的 `sodeck-desktop.exe`。
- 压缩包：`.local/dev/SoDeck-v2-T015.zip`。解压后运行，保持 `resources/` 与程序同目录。
- 数据由 `portable.json` 指定保存在旁边新建的 `data/`；出厂目录不带测试或日常数据。
- 附 `README.md`（5 步试用）、本报告 `RESULTS.md`、版本与文件 SHA-256 清单 `build.json`。明确 `buildProfile: release`、`published: false` 和未提交源码指纹。
- 最终构建已在隔离副本通过原生启动检查：`startup-shell-delivery.log` / `startup-shell-1793056084404/results.json`。交付包完整性与二进制一致性记录为 `package-integrity.json`，测试进程清理记录为 `cleanup-final.json`，均位于 `.local/test-results/t015/`。

## 实现结果

| 用户反馈 | 当前行为 |
| --- | --- |
| 1、2：服务器表单 | 输入框使用内侧焦点环，聚焦不扩大尺寸；密码认证的用户名/密码同排，窄窗折行；操作区固定 |
| 3：服务器分组 | 添加时选择或独立新建分组；选择器按组筛选；管理页新建、改名、删除；删除组仅取消归属，不删除服务器或断开连接 |
| 4：终端末行 | 将 xterm 测量网格与外层 padding 分离，合并几何更新、串行 PTY resize；底部完整，历史滚动位置保留 |
| 5：插件名称 | 插件声明中文显示名，“文件”“网络隧道”“终端”统一展示；准备中的版本不抢占当前名称 |
| 6：调用记录 | 查询条件固定、结果独立滚动且有滚动条；到底自动追加，追加失败保留已有行并重试同一游标 |
| 7：网络隧道 | 右侧入口和服务器专属列表，列表/详情分开；创建与编辑采用所属窗口的原生模态；名称可选、默认自动端口，实际监听就绪后才显示可复制入口 |
| 8：新建终端 | 最后一个终端 Tab 后提供“＋”，空工作区仍有创建入口 |
| 9～11：文件树 | 始终显示隐藏文件；目录加载图标旋转，减少动态效果下保留静态状态；移除节点详情，只在挂载行展示真实容量与进度条 |
| 12：窗口控制 | 公共窗口按钮统一几何、命中区域与反馈；三主题一致，多巴胺保持无投影 |
| 13：GUI 启动 | debug/release 均使用 GUI 子系统；便携资源从 EXE 所在目录定位；release 忽略开发覆盖变量，失败显示原生提示和日志位置 |
| 数据库补充 | 同结构版本继续使用；高低版本不一致时在 worker 启动前提示、备份 SQLite/已提交 WAL/加密凭据，再启用空库；不做迁移，失败与中断保留恢复路径 |

分组/服务器保存有原子事务、版本检查与持久操作回执；新的展示与目录契约独立版本化，旧严格 DTO 不增加字段。核心只负责通用资源、数据、承载与协议，网络/文件/终端业务仍在各插件。

网络保存结果不明时保留原提交并查询回执，避免重复创建。切换用途保留独立草稿；未保存检测覆盖暂未显示的草稿。模态具有独立 endpoint/订阅所有者，关闭、移窗、超时与退出均释放临时表面和输入屏障。文件容量仅为可见挂载点排队，最多 2 并发、256 项缓存，隐藏时停止；变量行高和目录定位使用布局提交后的锚点。

## 验证环境与分层结果

Windows 11 专业工作站版 x64（10.0.26300）、原生 WebView2；隔离 Debian/QEMU OpenSSH、多节点网络命名空间及专用 Win32-OpenSSH。浏览器测试使用注入桥；协议夹具、真实 SSH、原生桌面和 release 启动结果分别列出。以下证据路径均相对源码工作区；目录包附报告，不包含测试数据库、密钥或虚拟机。

| 关口 | 实际检查与结果 | 证据 |
| --- | --- | --- |
| 基础检查 | `npm run check`、`npm run test:contracts`、`npm run build` 通过；包含生成契约、TypeScript、公共 UI/依赖边界及插件测试 | `.local/test-results/t015/{check-delivery,contracts-delivery,build-delivery}.log` |
| G1 数据/契约 | `cargo test --offline --workspace` 通过；数据库 5 项覆盖无库/同版/v0/v3/v5、已提交 WAL、凭据备份、失败保留、发布中断恢复及损坏头部与 WAL 不被改写；服务器目录 6 项覆盖归属、原子失败、revision/游标与去重；旧 DTO/插件元数据兼容通过 | `rust-delivery.log`；原始专项 `database-tests.log`、`catalog-tests.log`、`metadata-tests.log` |
| G2 界面 | 最终受影响集合 **57 项通过**：公共设计系统、分组、服务器 Tab、工作台、终端、调用记录、文件、网络；包含三主题真实几何、焦点、键盘、窄窗/200%文字缩放、减少动态效果、固定查询和重试 | `ui-delivery.log`；`npx playwright test -c playwright.ui.config.ts tests/ui/design-system tests/ui/server-groups.spec.ts tests/ui/server-tabs.spec.ts tests/ui/workbench.spec.ts tests/ui/terminal.spec.ts tests/ui/call-records.spec.ts tests/ui/files.spec.ts tests/ui/network-tunnel.spec.ts` |
| G3 插件规则/文件 | 合约与 worker 测试通过；网络 34 项、文件 61 项；覆盖自动命名、旧规则、两种草稿、存储失败、回执去重/核实、容量队列与锚点；浏览器额外验证挂载行变高后底部锚点和末端定位 | `contracts-delivery.log`、`ui-files-anchor.log`、`ui-network.log` |
| G4 原生模态 | 最终 9 类检查通过：超过侧栏宽度、重复打开去重、来源限制、核心模态挂起/恢复、100 次开关、实际表单取消/焦点恢复、父视图移窗/关闭、独立原生父窗口关闭、创建失败恢复及 ready 超时；没有额外逻辑视图、worker 或 PTY | `dialog-native-delivery.log`；`dialogs-1793056024236/{results,diagnostics}.json` |
| G4 终端 DPI | 原生旧基线复现末行溢出约 **7.992 px**；修复后真实 Windows **100/125/150/200%** 缩放 × 7 个高度，共 **28 项通过**，最大溢出 **-2.016 px**（仍在视口内） | `terminal-dpi.log`；`terminal-verified-1793054117405/measurements.json` |
| G4 终端行为 | 历史滚动在调整高度和隐藏/恢复后保留，PTY 不变；真实原生菜单调整字号 14→15 后末行仍完整；系统缩放恢复到原来的 200% | `terminal-behavior.log`；`terminal-verified-1793055397314/measurements.json` |
| G5 Linux 文件 | 真实 OpenSSH 隐藏文件、根挂载容量、隐藏停止容量队列、10,000 文件目录末端定位实际绘制、缓存/行数边界、文件关闭保留终端及工作区关闭资源清零均通过 | `files-native.log`；`files-native-1793054524411/{results,measurements}.json` |
| G5 目录联动 | 真实 Bash cwd、成功/失败 cd、手动浏览、终端切换、隐藏恢复、异步取消、独立窗口激活、完整工作区拆合、两服务器隔离、重开默认状态及最终资源清零均通过 | `directory-follow-verified.log`；`directory-follow-verified/results.json` |
| G5 网络原生流程 | 实际创建/保存/启动、核心主机认证弹层、显示真实入口、HTTP/SOCKS、固定转发、独立停止与关闭清理通过；本行使用三节点 Paramiko 协议夹具 | `network-native.log`；`network-native-1793053418388/` |
| G5 真实网络 | Debian OpenSSH A→B→C→D，HTTP/HTTPS CONNECT/SOCKS5、远端 DNS、SSE、early 413/101 尾字节、浏览器多目标、两个同地址网络隔离、固定转发、64 MiB 哈希、专用 Windows OpenSSH、Windows→Linux 链及最终清零均通过 | `network-openssh.log`；`.qa/t014/openssh-1793055170489/` |
| G5 独立升级/旧包 | 网络 0.2.0→测试 0.2.1 的 UI 与真实 HTTP 行为仅重启后启用；规则保留且不自动恢复运行；坏候选 0.2.2 回退存储及版本、核心哈希不变。未经重编译的终端 0.6.0/文件 0.3.2 在新核心上经真实三跳执行终端 I/O、SFTP/exec 并清零 | `network-updates.log`、`old-plugins.log`；`.qa/t014/updates-1793055239069/`、`old-plugins/old-results.json` |
| G6 数据库 GUI | release 结构 v3/v5 原生提示先于 worker；备份内容及空 v4 校验通过，同版重启保留数据；备份失败原生报错，旧 v3 保留且不启动 worker | `startup-matrix-fixed.log`；`startup-1793054766118/results.json` |
| G6 GUI 目录包 | ShellExecute 启动链、中文移动路径、异 cwd、过期环境变量、无控制台/调试端口、三插件中文名、关闭托盘、重复实例隔离、退出子进程清理、缺包保留已验证活动版本、损坏库错误与原字节保留通过 | `startup-shell.log`；最终二进制复核见 `startup-shell-delivery.log` |
| 最后修复复核 | 弹窗配置路径故障修复后桌面 Rust **40 项通过**，重新生成 debug 与 release，完整原生模态集合通过 | `rust-dialog-fix.log`、`debug-dialog-fix.log`、`release-dialog-fix.log`、`dialog-native-delivery.log` |

上表中未写前缀的日志和目录均位于 `.local/test-results/t015/`。Rust workspace 内依赖外部夹具的既有测试按原规则标记 ignored；本次实际 SSH 覆盖以独立原生/真实服务脚本为准，不将 ignored 计作通过。

## 修复过程中发现的问题

- 文件列表在提交新占位高度前设置 scrollTop，会被旧范围截断；改为布局提交后应用锚点，并保留等待期间用户的新滚动。
- 网络保存 TIMEOUT 原先未按未知结果处理；现在与 RESULT_UNKNOWN 一样核对原回执。未保存检测补齐非当前用途草稿，组合输入中的 Enter 不提交。
- Tauri setup 返回 Err 会先触发内部 panic，绕过 main 的错误提示；现在在 setup 内清理并显示原生失败提示和日志路径。真实备份失败已复测。
- 缺少一个随附插件时原逻辑丢弃有效公钥、覆盖缺包原因；现在逐包报告缺失，继续用有效公钥验证已有活动版本，不绕过签名。
- 损坏 SQLite 头部伴随 WAL 时，SQLite 打开失败的清理仍可能改写原文件；先普通只读检查文件标识，再禁用只读连接关闭检查点，原库/侧车逐字节保持测试通过。
- 弹窗配置路径被同名文件占用时 WebView2 创建迟迟不返回；创建原生容器前建立配置目录，路径冲突直接进入失败回执与清理。移除隔离故障后重试成功，独立父窗口关闭也释放模态和输入屏障。
- 目录联动的旧断言因 200% DPI 下输出折行而失败；测试改为匹配真实输出的拼接行，并等待会话运行，避开把回显命令误当成功输出。完整真实联动复测通过。

早期失败日志保留；结论以本表列出的最终日志为准，不把早期截图中的注入桥错误当作当前界面结果。

## 界面证据

已查看服务器表单、真实挂载容量和原生模态截图，确认焦点边缘、两列认证、挂载行容量与弹窗层级可读。界面文件可从工作区打开：

- [服务器表单：多巴胺](../../.local/test-results/t015/server-form-dopamine.png)、[浅色](../../.local/test-results/t015/server-form-light.png)、[深色](../../.local/test-results/t015/server-form-dark.png)。
- [原生网络隧道弹窗](../../.local/test-results/t015/dialogs-1793056024236/native-composite.png)。
- [真实挂载容量](../../.local/test-results/t015/files-native-1793054524411/mount-capacity.png)、[10,000 文件末端定位](../../.local/test-results/t015/files-native-1793054524411/located-file.png)。
- [终端 200% DPI](../../.local/test-results/t015/terminal-verified-1793054117405/terminal-200.png)。

## 清理与验证边界

专用 QEMU 进程 39320/17808 已通过隔离 SSH 请求正常关机；专用 Windows sshd 22268 已停止；其监听端口 22281/22323/22324/22327 已释放。日常 sshd 27636 未停止。测试数据与证据保留在 `.qa/`、`.local/`，不复制进交付数据目录。最终进程审计见 `cleanup-final.json`。

输入法测试是在原生 WebView2 内注入 composition 事件并验证不误提交、取消及焦点恢复，没有自动操作系统中文输入法候选窗。缩放在单显示器上实际切换，未验证双显示器不同 DPI 跨屏。没有宣称 macOS/Linux 桌面、正式安装器、系统签名或发布验证。网络耗时是该隔离环境结果，不是普适性能承诺。

工程验证不代替用户体验验收。建议按[试用说明](T-015-acceptance.md)的 5 步确认；未进行 Git 提交、推送或发布。

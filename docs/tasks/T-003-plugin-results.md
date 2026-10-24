# T-003 插件平台与原生更新验证记录

日期：2026-10-25 · 关联 [T-003](T-003-core-terminal.md) · 本附录记录已执行的插件平台检查，不代替整批验收。

## 结论与验证边界

实际终端的独立 UI/worker 包、运行期间固定版本、重启激活、初始化失败回退、缺失 active 包时的核心恢复界面，以及视图 schema 不兼容占位与显式重建，已在 Windows 原生桌面通过检查。接续补验还通过了提交前写入副本后的真实进程终止恢复，以及已激活运行期间终止后的数据保留；UPD-03/04 的全部故障分支仍未全部执行，不能将整个更新矩阵标为完成。

执行入口为 `npm run test:updates:native`，脚本 `tests/desktop/updates.mjs`。本轮证据目录为 `.qa/update-native/1792870279704/`，`result.json` 为 `passed: true`，包含 7 组检查与 `normalStops: 6`。六次停止都断言 `app.exit` 后进程退出码为 0，未用强制结束替代正常重启；各次日志为 `app-run-1.log` 至 `app-run-6.log`。

脚本运行真实 Tauri 执行文件、raw Wry 终端页面和独立 QuickJS 子进程，使用隔离数据目录及调试端口 9353/9354。SSH 由 loopback Paramiko 服务提供，认证、PTY、输入输出均经过真实 SSH 协议；远端内容为字节回送，不是 Linux/OpenSSH shell。故障候选以实际终端包为基础修改并重新签名，仅存在测试目录，没有新增产品测试插件。

## 原生检查结果

| 用例范围 | 已执行操作与实际结果 |
| --- | --- |
| UPD-01：运行版本固定 | 运行 0.1.0，字号设为 19，建立真实 SSH PTY；独立构建并准备 0.1.1 后，active 仍为 0.1.0、pending 为 0.1.1。旧 UI 没有重置按钮，旧 worker 的 reset 返回 METHOD_NOT_FOUND，输入输出继续，PTY/连接 ID 不变。整个流程核心及出厂包哈希不变。 |
| UPD-02：正常重启激活 | 正常退出并使用同一目录启动后，active 为 0.1.1、pending 清空、字号 19 保留，旧 PTY 未自动重建。明确新建终端后，新版原生“恢复默认字号”按钮将设置保存为 14，实际 SSH 输入输出继续。 |
| UPD-03：内容篡改 | 修改实际包的 worker 并重算 ZIP CRC，保留原签名和完整性清单。宿主因内容哈希不匹配拒绝，既有 active 0.1.0、pending 0.1.1 和真实会话继续有效。 |
| UPD-03：已签名但不兼容 | 基于实际终端制作正确签名、要求 core >=99 的候选。宿主明确拒绝兼容性错误，原 active/pending 不变；继续通过原生终端输入回送，并核对原 PTY/连接 ID。 |
| UPD-04：初始化阶段与副本回退 | 已签名的 0.1.2 故障候选在 prepare 中尝试删除实际服务器和调用 resources.stop，二者都返回 INVALID_STATE；随后在私有数据副本写入字号 23，再主动抛错。重启仍使用 0.1.1、pending 清空、失败原因可查询，实际服务器保留，原字号 17 保留，reset 仍可用。 |
| UPD-05：active 包缺失及恢复 | 正常退出后，仅临时移走隔离目录中的 active 0.1.1 包。真实核心外壳仍能读取服务器，打开资源页与调用记录页；保留的视图显示不可用占位，点击重新加载可见缺包原因，PTY 为 0。正常退出、恢复原包后再启动，worker 与字号 14 恢复，未自动连接 SSH。 |
| 重复启动 | 再次正常启动仍使用 0.1.1，字号 14、服务器档案保留、PTY 为 0，没有失败激活循环。 |

初始化拒绝证据保存为 `candidate-phase-denials.json`：两条记录均属于插件 `sodeck.terminal` 版本 0.1.2 的同一 instanceId，方法分别为 `servers.delete` 与 `resources.stop`，状态 failed、错误 INVALID_STATE，均有结束时间。这验证生命周期阶段限制及其留痕，不是插件权限分配；正常业务阶段仍调用统一公开核心能力。

`resources.stop` 的测试目标为故障用例标识，初始化阶段应在资源查找前拒绝；本轮没有在候选初始化时保留另一条正在运行的远端连接，因此不据此声称验证了“初始化期间停止其他活跃连接”的完整场景。

已查看 `terminal-0.1.1-reset.png` 与 `missing-package-shell.png`：前者可见已连接状态、真实回送和字号 14；后者可见服务器工作区、视图不可用占位、重新加载入口及 PLUGIN_UNAVAILABLE 缺文件原因。缺包原因原文另存 `missing-package-reason.txt`。这些截图来自实际桌面表面。

SSH 事件记录为 3 次 connection-open / 3 次 connection-close、2 次 shell-open / 2 次 shell-close；额外一次连接来自首次主机信任挑战。测试后 App 与 SSH fixture 均结束。退出日志含 WebView2 窗口类注销信息，fixture 日志含 socket reset；正常退出码及实际通道关闭已分别核对。

## 本轮构建标识

| 产物 | SHA-256 |
| --- | --- |
| `target/debug/sodeck-desktop.exe` | `cabd8a8f2f2d4c7988ad731cb2ffa874bcc5797460694b9ef4f939c3d05e2e4d` |
| 出厂 0.1.0：`resources/terminal.sodeck-plugin` | `c930042807c4478f1dff34902f3204de8579305c40d994170a769d08b4148d3c` |
| 独立更新 0.1.1：`resources/sodeck.terminal-0.1.1.sodeck-plugin` | `d6817b18679414539dfbbbc13108a0e8e8cf049a76c2c802a45956a083e5d5de` |

核心与出厂包的前后哈希一致；更新构建没有重构建核心或替换出厂包。以后重新构建须记录新的哈希，不能沿用本轮结果。环境为 Windows 11 专业工作站版 10.0.26300、WebView2 155.0.4283.62、Node 24.22.0、Rust 1.99；测试 Paramiko 5.1.0 位于 `.local/python`。本地构建和运行方式见[开发入口](../development-entry.md)。

## 模块检查

当前工作区执行 `cargo test -p sodeck-plugin-host -p sodeck-plugin-manager --offline`，使用本机已缓存的 rsproxy 来源，通过 19 项 Rust 测试：

- 包管理器 10 项：签名信任及版本不可变、安装内容复核、初始化失败保留数据、启动切换及副本、写锁、合成恢复日志，以及已签名的 core/API/能力/平台/入口/视图不兼容、32 MiB 包体/2000 项/128 MiB 解压限制、大小写路径冲突、数据 schema 改变拒绝、候选缺包回退及 active 内容缺失报错。
- 引擎/协议 4 项：帧限制与截断、生命周期与异步核心回调、结构化异常、无限 Promise 任务的执行期限。
- 实际独立进程 5 项：核心回调往返、无限执行隔离、超限核心响应显式 RESULT_UNKNOWN 且后续小请求可用、真实 OS 管道阻塞时过期未发送帧不执行、停止不会等待阻塞的 stdin 写入。压力用例以约 900 KiB 帧填满子进程输入队列，停止使用 6 秒断言容差核对 5 秒预算。

父端采用 32 帧 / 4 MiB 有界独立写队列；排队前开始计算请求期限，未开始发送的过期请求可取消，已开始发送的超时结果不确定。具体接口见[联调契约](../technical/implementation-contract.md)。

实际终端 worker 6 项、终端快照 3 项及 TypeScript 检查通过。worker 的重命名回归覆盖 trim 后 1–80 个 Unicode 码点、控制字符拒绝、不调用核心、不改变会话/PTY/租约标识，以及重连期间 BUSY。这些模块检查与本轮更新脚本分别证明对应行为；更新脚本没有单独验收重命名交互，原生工作台验证见主任务附录。

## 接续验证

### 接续补验计划（2026-10-25，实施前记录）

新增独立 `tests/desktop/recovery.mjs`，使用 9405/9406 端口和独立 `.qa/recovery-native/<run>/` 数据，复用已构建实际终端包；只在测试目录解包、修改清单/初始化行为并以现有开发密钥重新签名，不重建或覆盖正式资源。

- UPD-05：实际创建并保存 schema 1 会话列表视图，准备相同实际终端但声明 stateVersion 2 的候选，正常重启。既定预期是保留明确不兼容占位与原视图标识，核心页仍可查询、PTY 为 0；显式重新加载丢弃不兼容展示状态并按新 schema 重建。当前只读发现工作台未保存/比较该字段，先实际复现，再按原规格修复，不改正确预期。
- UPD-04 提交前：候选 prepare 在数据副本保存测试字号和就绪标记，再延迟后主动失败。确认实际 recovery.json 存在、active 仍选旧代次、候选副本标记落盘后，直接终止真实 Tauri 父进程；重启让候选失败，验证旧版本/旧字号/旧代次可用、pending 清空。记录被终止 PID、退出结果及终止前磁盘事实，不用合成恢复日志冒充中断。
- UPD-04 提交后：正常激活后修改当前数据，再直接终止实际 App，重启验证已提交版本和运行后数据未被旧副本覆盖。此项不声称精确命中 activation.json 提交后到 recovery.json 删除前的极短窗口。

本轮不填满磁盘或挂载系统卷。恢复日志写入前、数据复制中、提交原子替换瞬间的全部中断点及真实空间不足，缺少安全确定的注入条件时保留未验证。结果运行后另记哈希、退出码、数据代次及原生截图。

2026-10-25 原生复现：`node tests/desktop/recovery.mjs --schema-only` **失败**，证据 `.qa/recovery-native/1792873481987/`。实际签名 stateVersion 2 候选已激活，旧会话列表仍为 `ready`，未显示既定的不兼容占位。修复属于 T-003 r1 的 UPD-05 既定要求：保存每个视图的 stateVersion，旧布局缺字段按原版本 1 读取；启动对比当前声明，冲突保留同一视图并提供具体原因；用户重新加载时明确清除旧展示状态，在无运行句柄前提下按当前版本重建，结束的终端不自动新建 SSH。先补工作台单元回归（旧格式/不兼容/重置/再次重启），再执行类型检查并重建后重跑上述真实原生流程，不改变失败预期。

### 2026-10-25 状态版本修复与真实中断补验结果

`node tests/desktop/recovery.mjs` **3 组通过、退出码 0**，证据 `.qa/recovery-native/1792874331367/result.json`。执行前将本轮 exe、出厂包与公钥复制到该目录的 `artifacts/`，校验副本与源文件 SHA-256 一致，再用隔离数据目录和副本启动；没有锁定主构建文件，也没有更换正式资源。修复的工作台单元检查 14 项及前端类型检查通过；整合后的桌面 Rust 检查由主任务记录。

| 范围 | 实际操作与结果 |
| --- | --- |
| UPD-05：视图状态版本 | 保存 stateVersion 1 的实际会话列表，准备正确签名的 0.1.6/stateVersion 2 候选并正常重启。旧 viewId 保留，状态变为 `viewFailed`，界面显示保存版本 1/当前版本 2 及重新加载说明；服务器、资源、调用记录仍可查询，PTY 为 0。点击真实“重新加载视图”按钮后，同一 viewId 变为 stateVersion 2/ready，原因清空，实际独立插件页面显示“还没有终端”，没有自动连接。 |
| UPD-04：提交前真实中断 | 0.1.7 候选在 prepare 的数据副本写入字号 23/就绪标记并延迟。确认实际恢复日志存在、旧 active 仍为 0.1.6/字号 17/原数据代次后，终止真实 Tauri PID 36740；Node 返回 `code: null, signal: SIGTERM`。重启重试候选后按既定故障失败，pending 清空、失败原因可查询，仍选旧版本和原数据代次，字号 17、服务器档案保留。 |
| UPD-04：已提交运行中真实中断 | 回退完成后，将已激活数据中的字号保存为 21，记录 activation generation 3 及选中代次，然后终止真实 Tauri PID 30692，同样返回 SIGTERM。重启仍使用 0.1.6、同一数据代次和字号 21，PTY 为 0。此项覆盖运行中已提交数据保留，不代表命中 activation.json 原子替换后到恢复日志删除前的瞬间。 |

三个正常停止均断言 `app.exit` 后退出码 0；两次强制结束用于上述明确中断用例，未代替正常重启。`interruptions.json` 保存终止 PID、退出结果、实际恢复日志、activation 及副本内容。旧 active 数据代次为 `2891e00b-aa5f-486f-878c-5171bebe705f`，被中断候选副本代次为 `7610bac7-d2b4-4d61-8fa2-f84eefde93f8`。测试结束后查询 Windows 进程，无残留 `sodeck-desktop.exe`。

已目视检查 `schema-before-recovery.png` 与 `schema-restored-plugin.png`：前者可见明确不兼容说明和按钮，后者为恢复后的实际插件空会话页。`schema-after-recovery.png` 是核心 WebView 截图，不包含独立 Wry 子表面，恢复内容以单独插件截图及实际 DOM 为准。

| 本轮副本 | SHA-256（执行前后相同） |
| --- | --- |
| 核心 | `827bc187126e296b2eb5f691c044afb2a5b2ee9c3b49a9db19f4d0f759b8658a` |
| 出厂终端包 | `591ab37668e4138c93fbf34e821ef0586ceda4f66d5625d76358aa503d8331dd` |
| 开发公钥 | `6db114256be2b9114f9a7faca0bebf66bc22548ea12ccd9a6b305a63094f29a3` |

仅测试目录内的签名候选 0.1.6 为 `3b6497ebc4cc2d2acc120c1ac87ee6b3f8a8c0affc68255ce8a1af82cbd42e61`，0.1.7 为 `b0c6723613ef8a0927e3b468db9970870c965a2e3880148743a126a64f8b3734`；候选改变的是视图 stateVersion，插件数据 schema 仍为 1。

### 2026-10-25 恢复落盘失败补查计划（实施前记录）

按[工作台第 9 节](../core/workbench.md#9-实施前待收敛与修订)的 WB-12 补查，在上述 schema 不兼容视图点击重新加载之前，于本轮隔离 App 数据根创建空目录 `workbench.pending`，使真实布局暂存文件无法写入。调用实际 `workbench.recover`，要求返回 `STORAGE_ERROR`，完整内存快照（含版本、状态、revision）及磁盘 `workbench.json` 原始字节不变，PTY 仍为 0。随后仅移除本测试创建的空目录，继续原有真实按钮恢复及两组进程中断检查。该注入是确定的路径冲突，不等同于磁盘满。

补查已通过：`.qa/recovery-native/1792874578804/result.json` 为 **4 组通过、退出码 0**，包含前述全部 3 组。`schema-storage-failure.json` 记录实际错误 `STORAGE_ERROR / 拒绝访问。(os error 5)`；失败前后完整内存快照相等，viewId 不变、stateVersion 1、status `viewFailed`、revision 1、mountGeneration 1、不兼容原因保留；磁盘布局逐字节相同、PTY 为 0。仅移除空目录后，实际按钮重试恢复为 stateVersion 2/ready，插件空会话页已目视复核。

本轮执行副本核心哈希为 `0bbcf543aee0e193dcd8dad7744d9ec08b88e0d5bbd028f97032b2b929536828`，出厂包、公钥、两份签名候选均与上一轮相同，执行前后副本哈希一致。3 次正常停止退出码均为 0；提交前/运行中中断 PID 分别为 19384/16056，均返回 SIGTERM；仍保留旧数据代次 `b51448a1-e9dd-45f3-9802-9bfcaddea35a`，被中断候选代次为 `d3f4f06e-0615-417e-99af-252e1f572d42`。`cleanup-processes.json` 记录本轮副本的剩余进程为空。上一轮结果保留为历史事实，当前代码对应本轮哈希。

### 2026-10-25 接续补查计划

在已有原生更新脚本中增加 UPD-03 的真实错签名：保持终端内容、完整性清单、ZIP CRC 与受信 keyId 不变，仅更改 signature.json 的签名字节；预期返回签名验证错误，原 active/pending、连接与 PTY 保持且可继续输入。该项沿用已确认 UPD-03 的预期。

补查已通过：`.qa/update-native/1792872587935/result.json`，8 组检查、6 次正常退出。包含原有全部7组，以及真实错签名拒绝后同一SSH会话继续输入。该轮核心哈希前后一致为 `cc240635acc5f20a1b82893eddcde722bfbd7ea43f8370b2294d68ec51f35d7c`，出厂包 `591ab37668e4138c93fbf34e821ef0586ceda4f66d5625d76358aa503d8331dd`，独立更新包 `784d04a462b47a804a7617e7394ec956218e156e5fcd11773dba26689fca202f`。上方旧哈希保留为第一次验证事实，不代表最新开发包。

## 尚未覆盖的范围

- UPD-03：越界/超量全部组合、暂存空间不足。原生错签名已由上述补查完成；管理器已覆盖若干边界，但不能替代原生完整矩阵。
- UPD-04：恢复日志写入前、数据复制中、activation.json 原子替换瞬间及提交后到日志删除前的全部中断点，以及数据复制 I/O/真实空间不足失败。提交前写入候选副本后和已提交运行中的真实进程终止已补验，其他精确窗口仍只有部分合成状态单测。实际升级为数据 schema 1 → 1，未实现通用 schema 迁移。
- UPD-05：active 包缺失、视图状态 schema 不兼容占位及显式恢复均已有原生证据；其他插件或未来展示状态结构不由本轮实际终端场景推定。

Linux/OpenSSH、中文 IME、多屏、窗口迁移、负载及完整调用记录验收以[核心验证记录](T-003-core-results.md)和[前端验证记录](T-003-ui-results.md)各自证据为准，不能用本附录的 SSH 回送或模块单测代填。

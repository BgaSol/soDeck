# 终端目录联动：代码修改设计

更新日期：2026-10-26 · 修订：r3 · 状态：已实施，待用户验收
任务：[T-010 r2](../tasks/T-010-terminal-directory-follow.md)
功能：[F-FILES-002 r2](../plugins/files/features/terminal-directory-follow.md)、[F-TERM-003 r2](../plugins/terminal/features/directory-reporting.md)、[F-CORE-011 r2](../core/directory-context.md)
公共规则：[组件通信策略 r1](component-communication.md)、[F-CORE-001 r5](../core/communication.md)

## 1. 基线、目标和改动范围

只读核对 `v2` / HEAD `d550c76` 及现有未提交工作。当前 core dev.5 / API、SDK 4.1 / terminal 0.4.0 / files 0.1.0。终端 worker 没有 cwd；PTY 由 UI 读取并 ack；文件 worker 已有按字节路径 locate 与呈现生命周期；核心没有独立于焦点的目录来源。不能只给两个 UI 加一个相互监听器完成此功能。

目标为每工作区一份文件树，跟随明确选择的终端及其真实目录变化。使用公共通信机制承载目录与视图呈现；终端解析 shell，核心管理来源/资源，文件决定导航。保留目录根 `/`、无反向 cd、无抢焦点、无自动认证和运行现场仅内存的行为。此节的 dev.5 基线是设计时事实；实施结果与新版本见 [T-010 结果](../tasks/T-010-results.md)。

本方案包含契约/SDK/宿主、目录上下文、PTY 通用观察、终端报告、文件跟随、版本兼容与测试；不重做终端渲染/迁移、全部 RPC、文件挂载采集或其他插件。不修改 T-008 原终端签名包，不创建 Git 提交。

## 2. 状态归属与数据路径

| 状态 | 唯一所有者 | 谁可以改变 |
| --- | --- | --- |
| 每终端最近报告、shell 集成/解析状态 | terminal worker 的 Session | 对应会话的报告观察器 |
| 可供协作的来源、目录快照与有效性 | 核心 DirectoryContextService | 通过受校验的 register/report/unregister |
| 每工作区选择的来源 | 核心工作台 | 明确选择终端 Tab/其独立窗口等用户操作；后台报告无权选择 |
| 跟随开关、暂停原因、待定位目标 | files worker 的 WorkspaceFollowState | 文件自身命令与上下文订阅 |
| 树缓存/路径/展开/选择 | 已有 Explorer | 用户浏览或有效自动定位提交 |
| 路径草稿、菜单/拖动状态、焦点 | 文件 UI | UI 操作；同步只能延后，不覆盖草稿 |

目录链路：PTY 原始字节 → 同所有者只读观察 → terminal DirectoryReporter → `directories.report` → 核心所选来源快照 → `state` 订阅 → files FollowController → 既有文件 API / Explorer → UI。文件不接触终端 PTY、nonce、输入或私有命令。

## 3. 目录契约和核心修改

新增 `contracts/directory-context-v1.schema.json`、示例和双端生成类型。声明 `workspace.directory.v1` 能力。方法为本次拟实现名称：

| 方法 | 关键字段与结果 |
| --- | --- |
| `directories.register` | `{scope, viewId, pty:{id,generation}}` → `{sourceId, sourceGeneration}`；服务从真实 Caller、视图和 PTY 查出所有者/连接/配置身份，不接受自填插件身份 |
| `directories.report` | `{sourceId, sourceGeneration, reportSequence, status, logicalPath?, physicalPath?, evidence?}` → `{accepted, sourceRevision}` |
| `directories.unregister` | 来源身份；幂等撤销，只能释放本实例来源 |
| `state.get/open`，contract=`workspace.directory.v1` | target 为工作区 ID+generation；返回公共目录快照，无可操作 PTY/连接句柄 |

每逻辑视图最多一个当前来源，重新连接创建新 sourceGeneration，旧报告立即失效。相同 reportSequence、相同内容幂等返回原结果；相同序号不同内容拒绝，旧序号拒绝。目录最多 4096 原始字节且为绝对路径、无 NUL，沿用 RemotePath 的 bytesBase64；单报告最多 16 KiB。状态只保留当前值，不存历史。

公共快照包括：`scope/revision`、`source {id,generation,viewId,title}` 或 null、`selectionRevision`、`pathRevision`、`status`、逻辑/物理路径、有限的路径视图证据及 `observedAt`。observedAt 只展示新鲜度，排序使用 revision。`unknown/ready/lastConfirmed/unmappable/unsupported/disconnected` 是判别状态；只有有效目录且映射核对通过才允许导航。`lastConfirmed` 明确是上次外层 shell 确认的位置，不代表内层程序当前位置。

新增 desktop `directory_context.rs` 实现领域服务，`workbench_services.rs` 的明确用户激活事务更新所选来源。工作台先原子提交新来源及其快照（未知来源返回 unknown），再交付通知，不能短暂把 A 路径挂在 B 名下。报告不变更 selectionRevision；相同路径报告不变更 pathRevision；A→B→A 即使最终 ID 相同仍有新的 selectionRevision，合并交付后也能判断发生过选择变化。

注册时若该视图是工作台当前有效的目录候选，可补足首次来源；以注册时的工作台选择修订核对，不能用迟到注册覆盖后来选择。点击文件/菜单保留来源；用户显式聚焦终端弹窗是选择，OS 呈现回调/移窗 ready 不是。多窗口按核心提交序排序。

停止插件、释放 PTY、断线/配置换代、关闭视图/工作区均撤销相应有效报告，工作台明确选择替补时再切换。移动视图只改变表面 generation，不撤销后台来源；旧 UI 请求仍被拒绝。工作区状态不写 SQLite 或工作台恢复快照。

## 4. PTY 的无损旁路观察

现有 UI `pty.read/ack` 保持唯一显示消费语义。禁止让 worker 共享这一 ack：它会提前删掉还未渲染的字节，导致丢屏/移窗错误。

新增 `remote.pty.observe.v1` 和 `contracts/pty-observe-v1.schema.json`：

- `pty.observe.open` 绑定本实例拥有的 PTY/generation，返回独立 observerId/cursor；每 PTY 本批最多一个观察器。
- `pty.observe.read` 是立即返回的有界读，最多 64 KiB，返回 bytes、nextCursor、gap、closed；读取只推进自己的缓冲，不修改显示 ack。
- `pty.observe.close` 幂等关闭，PTY/实例结束自动关闭。
- 宿主向对应 worker 发送合并的 `stream.readable` 提示，只说明“可能有新数据”；worker 顺序 drain，提示可合并，最后一次 read 与下一次 ready 注册必须在资源锁内消除漏唤醒。它不携带终端正文。

在 ssh owner loop 接收字节时复制到观察 ring，且先登记观察再启动可报告的 shell，避免漏掉首次 prompt。每 observer 256 KiB、App 8 MiB；超额 open 返回 CAPACITY。观察 ring 溢出只丢最旧观察字节并明确 gap，不能反压或关闭主 PTY；解析器清空半帧、撤销当前可信状态，等下一个完整有效报告恢复。UI 的 2 MiB 显示缓冲和既有背压仍有效；若远端本身因显示背压停止产出，不能宣称观察器可继续获得不存在的新输出。

新增 `pty.openCommand`（能力 `remote.pty.start.v1`）：通用 PTY+exec 启动，参数为已有租约、尺寸、有界 command 和可选原子创建 observer，返回资源回执；核心只执行协议，不认识 Bash/OSC。旧 `pty.open` 的 `.shell()` 行为及返回 shape 保持不变。command 上限 32 KiB，记录摘要及字节数而不记录正文。命令经过终端插件的严格 shell 参数编码，无用户路径字符串拼接；不借此增加文件写入产品功能。

## 5. Shell 集成路线与前段验证关口

**r2 补充覆盖：** 原 r1 启动次数保持关口在 Bash 5.2.37 真实 SSH 中失败。用户已确认[启动补充 r1](terminal-shell-startup-review.md)：集成模式明确允许额外读取一次非交互 `.bashrc`，默认开启“Shell 目录集成”，关闭后新建/明确重连使用原 `.shell()`。偏好持久化，现有会话不热改。以下保留原候选与验证依据，其中“启动次数必须等同普通登录”和“此差异不得默认启用”由本补充替代；其他交互/退出码/历史/无 dotfile 写入要求保持。

首个交付目标为 Linux/OpenSSH + Bash 5.1、5.2 的常规登录配置；Zsh 5.9 是后续兼容候选，本批先验证其普通终端降级，不承诺自动跟随。tmux/screen、嵌套 SSH、容器、身份切换和 chroot 不自动穿透。

推荐先验证“仅本次进程环境注入”的 Bash 启动适配：经 PTY exec 启动已确认的 Bash 路径，设置私有函数和初始 PROMPT_COMMAND/PS0，最后 exec 真正的 Bash login+interactive shell。登录 profile 仍由 Bash 读取，login_shell 语义保留；不写 `.bashrc/.profile`，不安装脚本或远端服务。PS0 表达外层命令开始，prompt hook 报告最终目录；不用 DEBUG trap、输入匹配、prompt 文本正则或另一个 exec 的 pwd 推断。

这条路线有明确兼容条件：用户配置可能覆盖或限制环境函数、PROMPT_COMMAND、PS0；SSH exec 入口也可能使账户 shell 的启动文件额外执行。不能预先宣称透明兼容。阶段一必须记录 profile 实际执行次数/顺序、login 标志、已有 hook、提示符、退出码及管道状态、历史、补全和退出行为。若原配置覆盖集成，保留用户配置并降级，不在用户输入后追加安装命令。若发现初始化副作用重复或破坏交互，该启动候选不得成为默认实现。

**关口与停止条件：** 在上述两版 Bash 的默认配置及自定义兼容夹具下，通过 DR-02/07 后才采用该路线。失败时仍可继续公共通信等无依赖工作；终端默认保持原 `.shell()` 路径，联动功能不得记为工程完成。需要临时 rc 文件、模拟 login、修改用户 hook 或缩减首期目标时，先写出新的启动方案与影响，再按文档流程确认受影响部分。这里授权的是有边界的验证设计，当前未执行，也不承诺一个未经验证的万能注入技巧。

集成成功时发送私有 OSC 帧：固定版本、每会话随机 tag、单调 reportSequence、外层 PID/starttime、ready/busy 标志、逻辑/物理字节路径及路径视图证据；以 ST 结束。全部字段有界，路径使用 base64；物理 pwd 的结尾只去掉一个协议分隔换行，不能用普通命令替换吞掉合法路径末尾换行。解析器支持任意跨帧、OSC 嵌套噪声和取消；最长完整控制帧 16 KiB，超过即丢弃并回到扫描状态。

tag 只作关联，不能当作防同账号恶意代码的安全认证。启动握手绑定原始外层 PID/starttime；首个外层 hook 建立后清除初始化函数和会话变量的 export 属性，子 shell 不继承有效报告身份；后续帧必须核对该原始进程，不能接受新报来的 PID 替换它。任意 OSC 7/hostname 不直接导航。原始 OSC 不由 core 解释或删去，终端 UI 对精确匹配的私有 OSC 注册无可见副作用处理器，其他终端控制序列按原路径处理。

busy/ready 丢失时只能保留“上次确认/新鲜度未知”，不能猜当前提示符状态。首次有效报告在 shell ready 后 5 秒内未到，标记 unsupported/unknown；不因超时杀掉可用终端、不反复启动命令。SSH 明确拒绝 exec 且确认未执行时可回到原 shell 启动；远端可能已执行或回执不明时禁止自动重放，保留准确错误和用户显式重连入口。

## 6. 路径与文件服务的映射证明

同一连接/账号名或 SFTP 与独立 exec 一致，都不足以证明 PTY shell 的路径可导航。terminal 通过自己取得的公开 SFTP 资源读取 `/proc/<外层pid>`，结合 starttime 前后核对、UID、root 对象身份、mount namespace 和已报告的 physical cwd，证明实际交互进程身份和路径视图；报告只带有限身份摘要，不带资源句柄。

files 通过自己的 SFTP 会话核对这份证据与文件浏览 root/namespace，身份相等且目标路径有效才 locate。复用现有 Linux mount 采集中的核对原语，但不能把“exec 与 SFTP 一致”结果当作 PTY 证明。Linux/proc 解析仍在插件；需要复用的纯数据算法可提取到公开、无宿主依赖的 Linux 工具模块，不搬进 core 业务分支。

每次新 ready 报告需确认其外层进程身份仍有效；身份/路径视图证据未知或正在变化则不可映射。SFTP server chroot、hidepid、容器同名路径、PID 重用、连接换代都覆盖降级测试。不能把报来的自述直接标记 verified；不能因核对失败改用更高权限账号。两次观察之间仍存在远端变化窗口，最终读取结果继续按文件 API 的真实结果处理，不宣称跨远端进程的原子事务。

## 7. 文件跟随控制器

PTY 启动回执落点：`pty.openCommand` 必填 operationId，同实例同操作同参数只执行一次，冲突拒绝；`pty.start.get` 查询 running/completed/failed。回执完成后保留 60 秒，过期操作保留 10 分钟墓碑并拒绝重放；尚未绑定视图的孤立 PTY 在回执过期时关闭。每实例最多 64、总计 512 个回执（含墓碑）；容量不足明确拒绝。DR-07/08 与 COM-16 验证超时查询和重复启动不增开 shell。

实施细化（2026-10-26，已确认范围内）：OpenSSH 的 SFTP 进程可能为 non-dumpable，使用同一 SFTP 会话打开 `/` 后读取自身 fdinfo/mountinfo 证明根对象，不要求同账号 sibling exec 读取 SFTP 的 `/proc/PID/root`。exec 仍核对真实 shell 根与双方 PID/starttime，记录采用 NUL 分隔以保留进程名换行。公共订阅按插件实例合计预算、按 UI endpoint 校验身份；已参与工作区订阅的实例收到类型化 `workspace.closed` 通知，用于清理已关闭视图留下的工作区内存状态。通知不含业务数据且只送曾参与的实例，失败使实例明确失效。DF-10、DC-05、COM-14 覆盖这一释放路径。

新增 `plugins/files/src/follow.ts`，每工作区一份 FollowController；保留于 worker 的 workspace map，文件 UI 重开可以重接，工作区结束删除。Explorer 专注文件树。新增 `follow.setEnabled`、`follow.locateOnce`、`follow.interaction` 等同插件命令与快照字段，由宿主绑定实际 view/mount 身份，不能仅相信参数中的 viewId。

控制器保存 enabled、pausedByCancel、latestTarget、lastObservedSelectionRevision/pathRevision、navigationGeneration、manualRevision、UI interaction 状态。收到上下文时先比较来源代次与修订；只有路径/明确来源变化、重新开启、单次动作、隐藏后恢复才触发定位。观察状态变新但路径不变不刷新树/挂载。手动浏览更新 manualRevision；已提交的手动输入消费此前积压目标，不被旧自动请求覆盖。

每次自动定位捕获 `{workspaceGeneration, sourceGeneration, selectionRevision, pathRevision, navigationGeneration, manualRevision}`。并发中只保留最新目标；取消旧读取，结果回来后再次比对全部身份。先准备必要目录页，再原子提交路径/选择/滚动；失败保留原树/选择并展示错误。手动请求和自动请求分组取消，自动取消不关闭其他文件读取或终端租约。

隐藏时停止自动文件读取并关闭目录 feed；呈现订阅保留，恢复后 open 最新上下文并合并一次。路径草稿/菜单/拖动中只记最新目标；Enter 提交手动位置优先，取消编辑再应用积压目标。取消自动读取会暂停跟随，后续 prompt 不能重试；重新开启/单次定位由用户明确触发。回到可见不会绕过取消。

UI 沿用已读取的 apple-design：立即更新来源/加载反馈，异步结果可被新目标取代，保持焦点和输入，不做大幅动画；状态使用文字与现有主题令牌。代码落点为 `view.tsx`/`files.css`；不增加第二套状态总线或全局热键。

## 8. 文件与模块修改清单

| 模块 | 预期文件与职责 |
| --- | --- |
| 通用通信 | 新 `crates/communication`、SDK client/state/worker、state-feed schema；见独立策略，不复制实现 |
| 核心目录 | desktop 新 `directory_context.rs`、`communication_services.rs`；修改 app.rs 装配、workbench_services.rs 选择、plugin_registry.rs 实例撤销、workspace_move.rs 表面换代联动 |
| 核心 PTY | core `ssh.rs`/`lib.rs`，新 `ssh/observer.rs`；通用 exec 启动、独立观察缓冲及资源回执，不解析 shell |
| 宿主 | plugin-host bootstrap/process/transport；单通道通知、调度、关闭和真实身份绑定 |
| 终端 | 新 `directory-reporter.ts`、`shell-integration.ts`、`directory-protocol.ts`；worker.ts 编排注册/释放，terminal-view.tsx 仅处理私有 OSC 显示及保持原消费/迁移 |
| 文件 | 新 follow.ts 与私有命令类型；worker.ts 管理工作区状态，state.ts 复用/补充定位提交边界，view.tsx/files.css 展示控制与状态；linux-collector.ts 提取路径视图核对原语 |
| 生成与依赖 | 新 schema/examples 与生成入口、SDK Rust/TS 校验、scripts/check-boundaries.mjs / dependency-boundaries.mjs 扩展；禁止插件私有互导及核心导入 shell 业务 |
| 测试与交付 | 各所属模块测试、tests/contracts 或现有 SDK 契约入口、tests/linux 与 tests/desktop 的 T-010 流程；版本、manifest、开发包说明统一更新 |

以上新文件目前不存在；最终拆分可在不改变职责/契约的情况下调整。测试实现只在确认后建立，优先验证真实边界和竞态，不为每个转发函数复制浅层测试。

## 9. 版本、预算与兼容

拟发布 core `2.0.0-dev.6`、SDK/API `4.2.0`、terminal `0.5.0`、files `0.2.0`。保持整数 apiVersion=4、workspace.v4 和 RPC 信封 v1；新增可协商能力和独立 schema，不增加旧响应字段。新品包 requires `^4.2.0` 及实际使用的新增能力，旧核心准确拒绝加载新版包，不能运行到中途才报缺方法。

| 组合 | 预期 |
| --- | --- |
| 新核心 + 原 terminal 0.4.0 / files 0.1.0 | 原功能照常，无自动跟随；原包字节与签名不变 |
| 新核心 + 新 files + 原 terminal | 文件手动浏览正常，明确无目录报告 |
| 新核心 + 新 terminal + 原 files | 终端正常报告；旧文件不消费，无联动 |
| 新核心 + 两个新版 | 受支持 shell 下完整联动；不支持配置准确降级 |
| 旧核心 + 任一新版包 | 管理器按最低 API/能力拒绝，已安装可用旧版保持 |

呈现状态新旧路径按能力择一，旧包不接收新增通知。升级按现有重启规则；无新持久数据迁移，回退清空运行状态即可。原 T-008 150% 系统 DPI 缺口仍留原任务，不用 T-010 结果替代。

通信预算见通用策略；目录报告每来源最多 10 次/秒，terminal 合并中间状态，只保留最新完整报告；核心超限返回 RATE_LIMITED，关闭/失效不受频率合并影响。文件自动导航最多一个在途及一个最新目标；不人为延迟显式切换，连续目录变化可合并 50 ms。文件分页/缓存/并发沿用 F-FILES-001/F-CORE-009，不递归扫描。

测量分开记录：核心发布到 worker 的本地通信 p95 ≤ 200 ms；已验证且缓存完整目标，收到状态到 UI 选中 p95 ≤ 300 ms；真实 SSH 下记录报告、映射核对、文件加载各段耗时，不以这些本地指标承诺任意网络下的总耗时。测试场景为同工作区 2 终端、第二工作区隔离，加 8 活跃 PTY 和流量/慢消费者压力；观察缓冲及通用队列不得超限，关闭 1 秒内本地计数回到基线。

## 10. 实施顺序、验证和交付

收尾细化（2026-10-26，仍属已确认范围）：目录报告在工作台锁内再次核对来源视图和工作区代次，关闭竞态不得接受旧报告；补真实文件路径输入框的 Microsoft Pinyin 合成期间目录延迟到达、视图关闭重开与工作区销毁重建、跨工作区及整工作区拆合验证。Shell 矩阵分别覆盖仅 PS0 覆盖与字符串/数组 PROMPT_COMMAND 覆盖，不能用模拟输入法事件替代原生证据。T-010 包独立输出，新旧包组合和旧核心拒绝的实际错误阶段如实记录，保持 T-008 包及证据不变。

隐藏期远端 I/O 使用核心调用记录的源插件身份与开始时间审计，以原生测试记录的区间为准；只统计文件插件的 `files.*`/`remote.*` 方法，不把测试快照 `plugin.invoke` 或终端自身 PTY 读写算作文件远端读取。

同一全局 Shell 偏好在终端重新获得焦点/用户进入设置时刷新，避免已打开视图显示旧值；文件呈现订阅失败必须进入可见错误并清除旧目录，不能形成未处理的异步拒绝。以上是既定全局偏好与故障边界的实现修复。

1. **固定契约并验证 Shell 接入可行性。** 生成 schema/类型；隔离 Linux 验证 §5 候选，保留 profile/交互证据。失败按关口处理，不用 mock 通过替代真实登录。
2. **公共通信底座。** 实现状态订阅和宿主通知，用目录夹具与真实呈现适配验证 COM-11～16；旧呈现/终端兼容回归。
3. **资源与目录来源。** 实现 PTY observer、启动、目录服务和用户选择，验证 DC-01～08、DR-05/08；先证明无显示 ack 干扰、无旧代次回写。
4. **终端与文件接入。** 通过关口的 shell 报告接入，FollowController/UI 接入；真实 Bash 目录变化与 DF-01～11 全链路测试。
5. **整合交付。** 运行类型/依赖/契约/Rust/构建及受影响原生回归；覆盖两主题、100/150/200% 系统 DPI、键盘/IME、完整工作区移动与终端弹窗。新版包及旧包矩阵分别验证，输出独立 T-010 开发包、验收步骤及证据。

功能测试步骤分别在通信/目录/终端/文件规格中维护，结果集中写 T-010。模拟解析和乱序测试、真实 Linux、浏览器、Windows 原生、包升级分别标记；正式安装/其他平台不冒充已验证。测试清理所有观察/订阅、隔离目录与进程，不改用户日常服务器。

本轮仅写设计与测试流程，没有创建以上模块、测试脚本或新版本，没有启动 App/VM。用户确认本方案后才编码；确认覆盖方案内实施、明确列出的验证和本地开发包，Git 提交/推送、用户验收、正式发布分别处理。

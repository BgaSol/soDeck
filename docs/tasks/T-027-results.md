# T-027 实施记录

更新日期：2026-10-29 · 状态：待验收（Windows Shell修复 r9；诊断与编辑修订 r8 历史保留）

## WIN-SHELL-01 验证与交付（2026-10-29）

用户已明确授权默认禁用Windows远端Shell集成并直接修改。实施依据F-TERM-003 r4/WIN-SHELL-01：终端独立探测、普通Shell回退、每会话状态、取消/清理和Windows/Linux真实回归。沿用r8的core/API/v7及其他三插件，仅更新终端0.13.1；原r8目录/数据保留，只读复制配置。实现、原生回归、独立r9开发包普通启动和清理验证已完成，待用户验收。

只有目标独立exec确认Linux+Bash，才执行目录集成脚本；Windows、无法识别或探测失败使用普通SSH Shell。全局偏好保持，底栏显示本会话未启用；既有会话不热改。修复重连表面保留旧PTY关闭错误的问题，新会话实际错误仍显示。终端增加对已有`runtime.clock.v1`的声明，不修改核心/API/数据库。

- `npm run test --workspace @sodeck/terminal`：27项worker、15项Vitest通过，包含Windows/未知/拒绝exec、截断/超时/清理未完成、Linux+Bash、取消后迟到回执回收；日志`.qa/t027/windows-shell-tests.log`。
- `npm run check`：仓库类型、生成产物及依赖边界通过；最终UI重连错误清理补丁另跑终端类型检查通过。日志`windows-shell-repository-check.log`、`windows-shell-check.log`。
- 独立终端构建/签名包0.13.1：`.qa/t027/packages/terminal-windows-fix.sodeck-plugin`；SHA256 `d9c33c52b82894fb2f0ebc253c1440340d9a97b9c8e985dac5ad47631b2b85ac`。核心桌面仍为r8 SHA256 `cf623c71a5e860f0a76e617e867e13d18623872f889fb558d4f2a5416fcfd750`，另外三插件不变。
- `tests/desktop/windows-shell.mjs`：r8配置只读在线备份及引用DPAPI副本，实际连接已授权Windows `.104`。全局true、会话集成false、控件禁用且未选；普通CMD新建/重连均执行echo并保持连接，无`-n`错误/旧PTY关闭提示。没有改远端配置。
- 同一Windows原生应用连接专属T-027 Debian测试VM的管理OpenSSH/Bash：初始`/root`、`cd /tmp`、全局关闭后已有PTY继续报告`/`，明确重连后普通Shell没有集成prompt hook。结果`.qa/t027/windows-shell-native-latest.json`，最终通过目录`windows-shell-native-1793218465781`；截图`windows-normal-shell.png`、`linux-integrated-shell.png`已核对。

中间验证事实保留：初版脚本把核心通知的active误当作非running，且未等待提示符/错误地要求命令回显不换行；断言已按实际协议修正。A节点配置MaxSessions 0不允许终端，改用同一专属VM管理服务，没有放松A/B夹具。一次管理端`cd /tmp`后目录状态等待超时，证据`windows-shell-native-1793218360453/failure.json`；当时未保存目录状态，不能断言根因。补充状态证据后，同一业务实现完整重跑通过；没有以改变目录预期掩盖失败。原生应用均正常退出，exitCode 0，未强制终止用户实例。

交付入口：`.local/dev/SoDeck-v2-T027-r9/Start-SoDeck.cmd`。构建脚本`scripts/package-windows-shell-dev.mjs`逐文件核对并复用r8程序/其他插件，生成新目录；单插件包也可独立交付。复制r8的core v7：5台服务器、2个代理、4条主机信任、6份引用DPAPI；当前激活终端settings.json也复制到新数据代次，保留用户已有`shellIntegration=false`（不会擅自替用户开启全局集成）。后续如开启全局集成，Windows仍会被本会话能力门槛排除。两目录后续不自动同步。

普通exe及Start脚本启动均通过：加载0.13.1及另外三个原版本、API6.1、零启动错误、无自动连接、设置仍为false、正常退出；证据`.qa/t027/windows-shell-delivery-latest.json`。core完整性ok、九张表逐行摘要与复制时一致，无schema重建，证据包内`evidence/config-copy.json`、`terminal-preferences-copy.json`、`data-retention.json`。专属VM27404已关机，22373～22375与本轮调试端口均释放；r9已退出。收尾时没有SoDeck桌面实例在运行；本轮只向测试启动的实例发出退出请求，不断言用户旧实例持续运行。证据`evidence/cleanup.json`。

用户验收：启动r9，用原`.104`新建/明确重连；确认正常提示符、底栏“本会话未启用”，执行日常命令。Linux/Bash按原全局偏好启用；Windows目录自动跟随和Windows资源监控仍不在本次范围。没有提交、推送、安装器或正式发布，用户验收尚未发生。

## Windows 目标终端退出反馈（2026-10-29，只读定位）

用户运行r8连接 `192.168.1.104`，终端输出“此时不应有 -n。”随后显示“远程 shell 已结束”，要求先排查原因。本轮未修改应用/插件、用户设置或远端配置。

- 当前r8持久记录显示 `.104` 使用直接SSH；`connections.acquire`、登录和deliver成功，随后 `pty.openCommand` 成功受理，终端约1秒内由running变为closed。通道请求成功不等于启动脚本执行成功。
- `plugins/terminal/src/worker.ts` 在默认开启Shell目录集成时无远端Shell能力判断，直接把 `shell-integration.ts` 生成的Bash脚本交给SSH exec；第一行是 `if [ -n "${BASH_VERSION-}" ]; then`。Bash语法的条件本身不能在Windows cmd中执行，后面的else无法提供Windows降级。
- 在本机Windows cmd仅执行这条无副作用首行，得到exitCode=1和 `-n was unexpected at this time.`，与截图中文错误对应。结合本次真实调用记录和终端输出，故障定位为目录集成启动的Shell兼容缺陷；没有将本地复现写成重新登录远端验证。
- 已有关闭集成分支使用原生SSH shell请求，可作为临时绕开入口（设置→终端→Shell目录集成关闭，再明确重连）；本轮未替用户更改设置或验证远端关闭后的结果。后续修复应保持不支持集成的Shell仍能正常交互，不用未知退出后自动重放启动的方式掩盖问题。
- 右侧监控当前只有Linux采集器，执行 `sh -s` 并读取 `/proc`；Windows支持与未支持状态提示需单独处理，不能把没有监控数据当作SSH建立失败。

本轮结论依据r8原生记录、用户截图、源码及本地cmd解析复现。应用修复和新的Windows远端回归尚未执行；原r8对Linux/OpenSSH及混合路径的已通过范围保持。

## 诊断修订交付验证（2026-10-29）

用户“你改吧。”已确认整体修订 r2、诊断 r1 与四插件 r4；本轮基线为 `v2` 未提交工作区。诊断、合并身份与登录确认、表单和四插件改动均已实现，真实协议、原生和目录包检查通过，进入待用户验收。不改变下文原 r6 的历史事实。

交付验证细化属于已确认范围：更新 `package-mixed-route-dev.mjs` 的 API6.1、四插件及本轮证据；新建 `.local/dev/SoDeck-v2-T027-r8`，拒绝覆盖 r6。四插件独立更新测试在专属复制目录中重跑，只验证本轮 API6.1 包，跳过已完成且本轮不涉及的 v6 重建测试。普通直接/启动脚本运行验证无 startupError、四插件版本和新能力、v7 无重建；允许用只读 SQLite 在线备份和被引用 DPAPI 文件为新包建立用户配置副本，校验配置/凭据计数及完整性，不复制旧插件缓存和自动连接工作区，不改源程序/数据。验证只读取真实配置，不自动连接真实服务器。测试应用正常退出，专属 QEMU 正常关机，端口回收后记录结果。

普通启动首轮已通过加载检查，但测试将 `app.exit` 的异步 `CLOSE_REVIEW_REQUIRED` 当作退出失败并清理了隔离 r8 实例。该回执只表示正在协调窗口，不证明有未保存输入。修正测试为等待既有关闭协调和真实进程退出，不取消审查、不改变产品预期；用户 r6 实例未受影响。

### 本轮实现与证据

- 核心：真实阶段开始/成功/等待/失败/取消、精确三元引用、不可变路线与候选 round、有界分页缓存、安全字段白名单、准备/交付失败和后续断线。等待操作在取消/所有者失效/过期后结束，继续认证创建关联的新尝试。观测事件修正为真实 outcome；不保留秘密或自由错误全文。
- 认证：每 SSH 节点合并身份与登录确认，主机信任与凭据绑定同事务；过期答复拒绝，重复提交幂等。SOCKS 仍单独确认。未关闭指纹检查或自动接受新身份。
- UI：统一连接详情、成功项折叠/失败项展开/未执行节点、旧记录导航、准确编辑失败节点和复制报告；修复关闭确认正文/动作区、下拉实底/锚点/焦点、内联代理重复边框和标题。复制报告明确含主机/端口，不含账号/密码/私钥。
- 四插件：终端错误保留精确引用；文件命令与状态同故障只提示一次；监控被动解释失败且不拨号，显式连接保留自己的引用；网络池成员、监听和目标服务错误分别归属，不串用 session 引用。各自 UI/worker/manifest/包均更新。

| 验证层 | 本轮结果与证据（`.qa/t027/`） |
| --- | --- |
| 静态/契约/插件 | `refinement-check.log`、`refinement-contracts.log` 通过：桌面13、SDK71、UI2、文件70+1 worker、监控16+4 worker、网络38、终端20+14 worker；类型、生成契约、样式/依赖边界通过。 |
| 核心状态与事务 | `refinement-core-lib.log`：98 passed / 9 ignored；最终维护小修后 `refinement-diagnostics-tests.log` 4 passed；`refinement-auth-tests.log` 2 passed（包含在库测试中，不重复计数）。覆盖 EOF 0/2、1/2、精确引用/快照/分页、秘密字段、预算淘汰、等待取消/继续/过期、合并提交回滚/幂等。 |
| 受影响 Rust 集成 | `refinement-core-integration.log`：connection_configuration 5、mixed_route 3、ssh_jump 3 个非忽略用例通过，9项需专用夹具仍 ignored；`refinement-socks.log` 6 passed / 1 ignored；候选重建 `refinement-candidate.log` 1 passed。 |
| 真实协议及原生诊断 | `refinement-native.log` / `diagnostics-native-latest.json`：Windows WebView2 + 专属 Paramiko SSH/SOCKS 夹具；A/B确认各一次，转发成功后 EOF 0/2 和 1/2 精确区分，目标未执行，查看/复制不拨号，四插件引用/去重、失败后成功交付、连接成功后断线追加均通过。 |
| 真实 OpenSSH | `refinement-openssh-native.log` / `native-latest.json`：专属 Linux OpenSSH A→B→P→T；3次合并 SSH 确认，无额外 preset 弹窗；目标 PTY、SFTP、Linux CPU 采集、HTTP/SOCKS/固定转发及改路径/旧连接保留通过。`refinement-public-openssh.log` 保留独立身份/凭据原语的公开 API 回归，不将其计作合并弹窗测试。 |
| 界面 | `refinement-home-ui.log`：9 Playwright 通过；`refinement-visual.log` / `visual-latest.json`：三主题、900×600、Windows100/125/150/200%DPI、高对比、微软拼音、键盘/焦点/私钥选择通过；减少动态使用 WebView 模拟媒体设置。系统缩放/输入法已恢复。 |
| 独立构建与更新 | `refinement-desktop-build.log` debug 构建、四份 `refinement-pack-*.log` 通过；`refinement-package-latest.json`：四插件逐个准备更新，重启后各自 UI/worker 新标记生效，其他插件和核心/交付包 hash 不变。测试候选版本未安装到交付数据。 |
| 实际交付 | `refinement-delivery-latest.json`：直接/启动脚本普通运行均无 startupError，四插件及API6.1能力正常，无自动连接和认证弹窗，正常退出。`refinement-data-retention.json`：v7、所有原配置行、凭据绑定和表集合保持，未重建 core；独立观测库为新目录初始化。 |
| 清理 | `refinement-cleanup.json`：专属QEMU PID31052由guest正常关机，22373～22375监听释放；隔离应用退出，原r6及其他用户实例未操作。 |

开发目录包：[Start-SoDeck.cmd](../../.local/dev/SoDeck-v2-T027-r8/Start-SoDeck.cmd)。应用 `2.0.0-dev.19`、包修订 r8、API/SDK `6.1.0`、UI `1.8.0`；独立插件 terminal `0.13.0`、files `0.10.0`、network-proxy `0.6.0`、monitor `0.3.0`。桌面 SHA256：`cf623c71a5e860f0a76e617e867e13d18623872f889fb558d4f2a5416fcfd750`。

已从原 r6 通过只读 SQLite 在线备份复制完整 v7 配置和4个被引用 DPAPI 加密文件：4个服务器、2个代理、3条主机信任、4条凭据绑定。来源、计数和完整性见 `refinement-config-copy.json`；未读取/输出秘密明文。原 r6 程序/数据继续保留，r8 是独立副本，后续编辑不与 r6 同步。未复制旧插件安装、布局或自动连接工作区。原生包启动没有连接用户真实服务器。

### 本轮用户验收

1. 双击 r8 的启动脚本，核对原服务器/代理仍在，编辑路径时检查实底下拉、内联代理和关闭时“继续编辑”。
2. 用原目标连接，失败提示点“查看连接详情”：核对 SSH 已完成、SOCKS 具体失败阶段/回复字节、后续目标未执行；可复制报告。
3. 在文件、监控及网络插件检查同一故障没有重复提示、监控查看详情不会自行连接；不同连接尝试各自保留记录。
4. 在可用测试路线首次连接，确认每个未知 SSH 节点只有一张身份与登录确认；取消后详情应结束为已取消。原实际代理恢复后再验收真实目标连通。

限制：本轮没有再次探测或修复 `.104` 的远端代理服务；之前确认的 12021 无监听、GOST1080依赖该上游仍是最后已知真实环境结论。界面只凭 EOF 不断言端口未监听。仅 Windows 开发目录包，非安装器；未运行全 workspace Rust 或 T-024 已停止的长压测，ignored不当作通过。诊断内存详情重启后不恢复；用户验收、Git提交/推送、正式发布均未发生。

以下为 r6 及更早历史记录，包入口、状态和测试数量仅对应当时基线。

## 真实环境反馈（2026-10-29）

用户授权读取其当前数据并调试，必要时操作电脑。当前数据位于 T027-r6；实际路径为本机 → SSH 192.168.1.104 → SOCKS5 127.0.0.1:12021 → SSH 65.11.10.25。失败记录为 `PROXY_DISCONNECTED`，尚未定位根因。原 2026-10-28 隔离环境验证仍只证明当时场景。

诊断计划：只读 SQLite 在线备份和对应 DPAPI blob 到 `.qa/t027/real-debug-*`；不输出秘密明文。独立 SSH 协议客户端核对已保存主机指纹，用相同凭据建立到 .104 的通道，逐段记录 SOCKS 方法协商、CONNECT 状态与目标 SSH banner，仅保存协议非秘密字段。再用 SoDeck 当前构建在副本复现，对比失败阶段。若暴露实现缺陷，先补具体回归流程，再在 T-027 已批准范围内修复；不改真实远端代理/SSH 配置。

本轮定位结果：

- 原程序为 T027-r6，桌面 PID 29916；r3/r4/r5 同时运行但其核心目录为空，未操作这些实例。当前真实目标为 `.25`，路径只有 `.104 → SOCKS 12021`，没有 `.16`。另存的名为 `12021` 的代理档案实际端口是 1080，但不是当前路线引用的对象。
- `.104` 为 OpenSSH for Windows 9.5。独立客户端按已保存 ECDSA 算法核对相同指纹后 SSH 认证成功；向 12021 的通道发送 SOCKS 方法请求，未收到任何回复即 EOF。初次独立客户端默认选择另一种主机密钥算法，指纹检查中止且未发送凭据；限定已保存算法后核对通过，未忽略主机检查。
- 在 `.104` 本机执行 curl SOCKS 检查：`127.0.0.1:12021` 明确连接被拒绝；Windows TCP 监听表无此端口。SoDeck 在一致数据副本复现 `PROXY_DISCONNECTED`，`stage=proxyNegotiate`、`stepIndex=1`，尚未触达目标 SSH。
- 用户要求枚举“五位数、含 2”的监听端口。实测为 10243（System）、21603（xllite/迅雷）、29190（UniVPNPromoteService）、54321（DownloadSDKServer/迅雷）、54322（thunder）；没有 12021。未向这些无关端口发送协议探测。
- 已识别的 SOCKS 服务是 GOST 1080。只读 GOST 配置确认该入口需要账号密码，且链的上游正是 `127.0.0.1:12021`。用配置中的凭据在内存中认证 1080 成功，但 CONNECT 返回 `05 03 00 01`（目标网络不可达），因此仅改为 1080 仍未打通上游。未输出、持久化或修改代理明文凭据。
- 证据位于 `.qa/t027/real-debug-1793205157507/`：`independent-protocol.json`、`sodeck-reproduction.json`、`candidate-ports.json`、`all-listeners.json`、`gost-upstream-test.json`。副本测试应用正常退出；原数据、原程序与远端服务配置均未修改。

当前未恢复真实连接；阻点为 `.104` 的上游 SOCKS 服务未在配置的 12021 端口监听，需要找到并恢复该服务或取得正确的新监听地址。本轮尚无证据表明混合传输执行器需要改动；不将配置/环境诊断写成代码已修复。

授权：[T-027 r3](T-027-mixed-route.md)，用户“你开始做吧。”确认功能r2/代码r1/四插件r1。基线为v2当前未提交工作区，保留所有其他任务的在途改动。

## 实施关口

| 关口 | 状态 | 证据 |
| --- | --- | --- |
| G0 真实混合传输 | 通过 | 2个协议夹具测试（5种组合与取消）及1个真实OpenSSH测试（3种组合）；证据见下 |
| G1 API6/v7存储 | 通过 | `.qa/t027/g1-routes.log`、`contracts.log`；Rust 3 条混合路径模型测试通过，API6/v7 公开契约通过 |
| G2 快照/凭据/资源 | 通过 | `.qa/t027/g2-executor.log`；真实 OpenSSH API 逐站挑战/预设绑定、50 次取消、资源释放通过 |
| G3 界面及四插件 | 通过 | `.qa/t027/native-latest.json`；Windows WebView2 原生整链及四插件消费者通过 |
| G4 验证/Windows目录包 | 通过 | `npm run check`、`npm run test:contracts`、Playwright 9 项、独立目录包普通启动通过 |

主流程和最终专项均已通过：完整备份/空库重建/旧程序回退、四插件单包更新，以及原生键盘/IME/缩放、实际容量和候选重试已补齐证据。用户验收与发布尚未发生。未提交/推送。

本次收尾测试实现仍在已确认范围内：新增 `tests/desktop/mixed-route-package.mjs`，在专属临时数据/复制的程序包中验证四插件逐个更新、启动时 API5/API6 双向拒绝、v6 完整备份及空 v7 重建、旧程序读取备份恢复；记录包/其他插件哈希。首次原生选择器失败、Enter 输入及 DPI 物理/逻辑尺寸误用均为测试工具修正，正确业务预期保持。

最终 workspace 回归暴露 `ssh_jump.rs` 的三条非忽略测试仍依赖旧 DTO/旧表。按已批准 MR-CODE-02/03/09 改为 `steps`、`server_route_steps` 与预设凭据绑定：保留删除依赖、自引用、同版本重开、事务故障无半写、非法持久行不发起连接、备份完整及高版本拒绝的断言，不为旧模型增加兼容层。

MR-CODE-08 收尾按既有焦点规格修正移动节点后的落点：留在该节点的移动按钮，移动到边界时转到相反方向的可用按钮。新增 `tests/desktop/mixed-route-visual.mjs`，复用 T-025 的原生 DPI/高对比/IME 驱动，在含 SSH/SSH/SOCKS 的表单检查排序/删除/Esc、三主题、900×600、100～200% DPI、减少动态及可信输入法事件，结束恢复系统设置。MR-CODE-06 复用已定义的 16×16 活连接容量用例，更新其 API6 挑战和资源字段；另补 512 代理上下文的并发预留与候选 CONNECT 失败后重建前缀测试。均不改变批准行为或扩大产品范围。

目录包脚本的本地收尾：预读并校验输入后再创建输出目录，直接读取本项目生成的 stored-ZIP manifest，移除个人 Python 安装路径；默认输出用独立时间戳目录，显式同名目录仍拒绝覆盖。最终交付固定 r6 目录，仅更新本次焦点修复后的程序及证据/hash，不为每次文档更新重复生成目录。

## G0（2026-10-28）

新增顺序执行器 `ssh/route_executor.rs`，复用现有 SSH 桥与精确 SOCKS 协议读取，桥指标保存实际 `edgeId`；已接入生产 Connection 入口及四个 API6 插件消费者。

- 协议夹具：`node scripts/cargo.mjs test -p sodeck-core --lib ssh::route_executor -- --ignored --nocapture`（加入真实测试前，2通过）。`SODECK_TEST_PYTHON`使用bundled Python，`PYTHONPATH=.local/python`。五种组合：SSH→SSH→SOCKS→SSH、SOCKS→SOCKS→SSH、SOCKS→SSH→SOCKS→SSH、SSH→SOCKS→SOCKS→SSH、SSH→SOCKS→SSH→SOCKS→SSH。只在目标打开session，桥数与SSH数−1一致，逐段记录与释放检查通过；远端SOCKS暂停时取消响应<1秒，连接清理<5秒。
- 真实服务：新建 `.qa/t027/linux/mixed.qcow2`，只读backing为已停止的T-008测试盘；QEMU PID23272，管理端口仅127.0.0.1:22373，测试入口22374/22375。专属命名空间mr-b/mr-t；A与B `MaxSessions=0`，P/Q仅监听B的127.0.0.1:12021/12022，目标域名仅B可解析，未访问用户示例地址。
- `node scripts/cargo.mjs test -p sodeck-core --lib real_openssh_with_remote_loopback -- --ignored --nocapture`，`SODECK_MIXED_OPENSSH_CONFIG=.qa/t027/linux/config.json`：1通过，包含A→B→P→T、A→B→P→Q→T、P→B→Q→T三条真实路径。目标执行结果均为`T027:mr_t`，桥数分别2/2/1，局部释放测量均<1ms；不能据此宣称所有运行负载均为该耗时。
- 证据：`.qa/t027/g0-protocol.log`、`.qa/t027/g0-openssh.log`、`.qa/t027/linux/topology.txt`及专属guest日志。VM在首次验证后停止；最终多消费者复验按同一隔离磁盘重启，最终清理见下。
- 首轮曾因测试Credential构造不符合Drop类型规则而编译失败，修正后通过；首次夹具启动漏设PYTHONPATH失败，使用既有隔离依赖后通过。第三方现有deprecated/PDB警告不影响上述结果。

多候选地址重建、凭据上下文、运行总预算及50次取消由G2验证，不包含在G0通过范围。

## G1～G4（2026-10-28）

- G1：`node scripts/cargo.mjs test -p sodeck-core --test mixed_route --test server_catalog --test ssh_multihop -- --nocapture` 通过（12 个非忽略定向用例）；`npm run check` 通过；`npm run test:contracts` 通过（桌面 13、SDK 69、UI 2、文件 69+1 worker、监控 14+4 worker、网络 37、终端 19+14 worker 个测试）。路径只由 `server_route_steps` 保存，数据库 `PRAGMA user_version=7`，旧路径形状与重复/自引用拒绝。
- G2：公开 API 测试 `public_api_authenticates_each_typed_step_and_binds_presets` 在 A→B→P→T 上通过，逐站请求 3 次主机信任与 3 次预设凭据确认，绑定文件/上下文数量正确（`.qa/t027/g2-public-openssh.log`）；另外三条真实路径组合由执行器测试覆盖，不将其算作该公开 API 用例的覆盖；远端 SOCKS 等待期间连续 50 次取消均释放。
- G3：`node tests/desktop/mixed-route.mjs` 通过。实际 Windows WebView2 流程保存 A→B→B-loopback SOCKS→T；终端输出 `T027_USER=mr_t` 与目标标记；文件插件读取 `/home/mr_t/t027-marker.txt`；监控发布目标 Linux 样本；网络隧道 HTTP 代理、SOCKS5 代理和固定转发均读取目标 HTTP 内容。三主题截图保存在最近一次 `.qa/t027/native-*` 目录。
- G4：`npx playwright test -c playwright.ui.config.ts tests/ui/connection-home.spec.ts` 9 项通过；`npm run desktop:build` 通过；`node scripts/package-mixed-route-dev.mjs .local/dev/SoDeck-v2-T027-r6` 生成 45 个文件的独立包，随后以普通（无开发环境变量）启动，`app.info.startupError=null`，四个插件均加载，`data/core/core.sqlite` 的 schema version 为 7。包入口为 [`Start-SoDeck.cmd`](../../.local/dev/SoDeck-v2-T027-r6/Start-SoDeck.cmd)；包不是安装器或发布物。
- 清理：首次专属 QEMU（PID 23272）已停止。最终复验的 PID 22132 已从 guest 正常关机；确认进程退出、22373～22375 无监听。原生 DPI/高对比/输入法设置已恢复，测试应用均正常退出，未操作 T-024 日常运行程序。

## 最终专项与交付基线

| 实测 | 结果与证据（日志位于 `.qa/t027/`） |
| --- | --- |
| workspace Rust 回归 | `node scripts/cargo.mjs test --workspace --no-fail-fast -- --test-threads=1`：246 passed / 35 ignored；`rust-final2.log`。忽略项需要各自外部夹具，并不表示已运行。后续新增并发预算与候选重试分别单独通过。 |
| 真实容量与并发预算 | 16 条各 16 层 SSH 的协议夹具同时保持 256 sessions / 240 bridges，第 257 会话在建连前拒绝，共享原连接可用，退出归零；`capacity-final2.log`。16 线程同时预留 512 代理上下文，超额拒绝，释放归零：`concurrent-budget-final.log`。 |
| 候选重试 | 首个本机解析候选被 SOCKS 拒绝后，重新建立 A/B/P，第二地址成功，T 只连接一次；`candidates-final.log`。 |
| 多消费者及路线变更 | `native-final3.log` / `native-latest.json`：两终端、文件、监控和网络隧道消费者真实通过；移窗/合并/托盘保持 PTY ID，关闭一终端保留另一；增加第二个 B 回环代理后，新申请使用新路径，旧终端继续使用旧快照。 |
| 原生界面 | `visual-final2.log` / `visual-latest.json`：3 主题×4 DPI（100/125/150/200%）、900×600 逻辑尺寸、键盘移动焦点/Esc/删除、可信微软拼音、高对比、减少动态及私钥选择取消/错误通过。减少动态由 WebView 媒体设置模拟；DPI、高对比及 IME 使用真实 Windows 接口。 |
| 插件独立更新 | `package-final3.log` / `package-latest.json`：四插件逐个 A→B 准备更新，重启前旧版继续、重启后各自 UI/worker 版本标记生效；其他插件活动版本、核心和生产包 hash 不变。候选为独立签名测试包，未替换交付插件包。 |
| 拒绝旧 API、备份及回退 | 同一 package 专项：4 个 API5 包在新核逐个拒绝，旧核拒绝 4 个 API6 包；v6 副本显示原生提示，完整数据库/加密凭据备份 hash 匹配，创建空 v7，独立插件文件保留；原版本程序在另一副本读取恢复后的原 serverId 和 remembered 凭据。 |
| 最终前端与构建 | `check-final2.log`、`ui-final2.log`（9 passed）、`build-debug-final.log`；最终包为经过原生验证的 debug exe，之前 release 编译通过不作为此包的运行证据。 |

最终目录包：`.local/dev/SoDeck-v2-T027-r6`。桌面程序 SHA256：`19329d0fb827cdd5e6a5a7b632807d599f856148b562a94c6fdc76648165dd12`；四插件版本为 terminal 0.12.0、files 0.9.0、network-proxy 0.5.0、monitor 0.2.0。最终普通启动/重启结果写入 `.qa/t027/delivery-final.json`；包内 `build.json` 记录文件 hash，`evidence/validation.md` 为本记录副本。全程验证当前未提交工作区；未创建 Git 提交。

## 结果边界与历史失败

- 初次 workspace 并行回归出现 observability feed cache 公共存储暂不可写；串行复验已通过。随后发现的三条旧路径测试改用新 DTO/新表后通过；未添加兼容层。保留原日志 `rust-workspace.log`、`rust-final.log` 和最终 `rust-final2.log`。
- 原生脚本曾因选择器歧义、回车输入、DPI尺寸、隐藏插件截图、草稿关闭审查、旧包路径和提示框自动化失败；修正测试操作后通过，不改变业务预期。
- 旧数据副本的已安装 API5 插件文件被保留，因此重建后提示插件 API 不兼容；不会把旧插件当作兼容包启用。交付目录使用独立空 v7 数据和完整 API6 包，正常启动无此警告。不提供旧配置/插件兼容或自动导入。
- 300 步测试证明存储/分页无固定跳数限制；实际建立仍受资源与时限预算约束。代理密码认证在协议夹具验证，真实 OpenSSH 命名空间中的 SOCKS 无密码；两类证据分开。
- 仅 Windows 开发目录包；不宣称 macOS/Linux 原生、安装器或发布验证。第三方 OpenSSL PDB/deprecation 警告仍存在。用户验收、Git 提交/推送和发布尚未发生。

## 用户验收入口

1. 启动目录包的 `Start-SoDeck.cmd`，分别保存 .16 与 .104 的 SSH 档案。
2. 新建最终 .24 目标，选择“经由中转”，按 SSH .16 → SSH .104 → SOCKS5 127.0.0.1:12021 排序；核对 SOCKS 下方明确显示由 .104 访问。
3. 保存并连接，逐站处理指纹与凭据确认，在最终终端检查身份；文件、监控和网络隧道均应使用最终目标。
4. 调整节点顺序、取消内联代理、关闭一个终端，确认草稿和其他消费者行为符合预期。

验收需用户自备该实际网络与凭据；工程测试使用专属隔离网络，没有访问用户示例地址。


## 2026-10-29 用户验收反馈

用户指出弹窗布局、连续两次确认及路径表单/下拉/代理边框问题。已完成只读原因分析和[具体修订方案 r1](../design/connection-editor-refinement.md)，含 REF-01～08 和四插件计划；当前待修订确认，未修改应用/插件/测试代码。上文是原版本的实际检查，不证明本次反馈已修复，也不作为用户验收通过。

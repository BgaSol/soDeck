# T-013 实施与验证结果

日期：2026-10-26 · 基线：`v2` 未提交工作区，F-CORE-014 / T-013 r1 已获“可以开始做了”确认。核心 2.0.0-dev.7、API/SDK 4.4.0、终端 0.6.0、文件 0.3.3。用户验收、Git 提交、推送、正式发布均未进行。

## 实际行为

核心建立本机 → 单层 SSH 跳板 → 目标的独立传输，使用 `direct-tcpip`，跳板不需要 shell/PTY。目标名称交给跳板解析；目标凭据保留在本机。路径档案、引用校验、两端信任与认证、租约、取消、阶段记录和释放属于核心；终端/文件继续使用普通目标 PTY、SFTP 和 exec。

服务器编辑器可选已有直连跳板，或在子步骤保存并选用；草稿和焦点返回原表单。认证弹窗区分跳板/目标和目标工作区。底部状态可查看完整路径；运行资源列出路径及持有实例，删除前列出保存和运行依赖。

新连接按完整路径及认证修订复用；改配置不终止已有会话。目录跟随先比对真实 connectionId/generation，不把旧终端 cwd 应用到另一连接；切回匹配来源后恢复。文件插件可独立连接，路径不依赖终端插件。

## 可复查证据

全部证据位于工作区 `.qa/t013/`，开发包仅复制脱敏日志、结果和截图，不包含测试数据库、私钥、密码或虚拟机磁盘。

| 验证层 / 命令 | 结果与范围 |
| --- | --- |
| `npm run check` | 通过：生成文件、TypeScript、核心/插件依赖边界 |
| `npm run test:contracts` | 通过：4 个应用队列测试、26 个 SDK 测试、58 个文件模块测试、14 个终端模块测试及 18 个 worker 测试。新增路径严格契约、不同连接目录拒绝及恢复 |
| `node scripts/cargo.mjs test --workspace` | 通过：核心、状态通信、插件宿主/管理器和桌面既有测试；忽略的真实服务用例不计为通过，跳板用例单独执行 |
| `cargo test -p sodeck-core --test ssh_jump -- --include-ignored`，经项目 cargo wrapper，设置隔离 Python/Paramiko | 11/11 通过。真实 SSH 协议夹具，目标为 echo 服务，不宣称 Linux shell/SFTP。包括保存事务回滚、旧 DTO、迁移、上下文、两端指纹/认证、转发拒绝、握手取消、等待者隔离、配置竞争、不同网络同名目标、断线、64 路径上限、50 次开关、50 次取消、日志失败清理 |
| `npx playwright test --config playwright.ui.config.ts` | 39/39 通过。浏览器模拟回归：服务器编辑/密码部分成功、初始终端、取消、后台认证、工作区和主题。真实跳板 UI 另见原生测试 |
| `node tests/desktop/ssh-jump.mjs` | Windows 原生 + Debian 13/OpenSSH 10.0p2：四组密码/私钥/加密私钥组合，真实终端、SFTP/挂载 exec，旧/新路径目录隔离，引用删除、移窗合并、托盘恢复、调用阶段记录及关闭回收 |
| 同上，`T013_OLD=1` | 未重新构建的原 API 4 插件包能访问跳板目标，终端及文件读取通过 |
| 同上，`T013_FILES_ONLY=1` | 仅加载文件插件，显式连接及读取通过，不依赖终端插件 |
| `node tests/desktop/ssh-jump-ui.mjs` | Windows WebView2 真实添加跳板子步骤、草稿/焦点、逐节点信任、路径详情、删除依赖；真实 Microsoft Pinyin 可信输入事件、100%/200% Windows DPI、900×600 逻辑窗口、深浅色、高对比、减少动态。系统 DPI/高对比/输入法恢复原值 |
| `node tests/desktop/ssh-jump-migration.mjs` | 真正的旧 API 4.3 核心创建直连数据/信任/DPAPI 凭据；新核心升级并连接；升级前备份恢复到另一目录后旧核心连接成功 |
| `node tests/desktop/ssh-jump-load.mjs` | 8 条真实 OpenSSH 路径（16 个 SSH session/8 个转发），第 9 个 PTY 拒绝；持续大输出、消费背压，峰值桥缓冲 32 KiB，低于每方向 256 KiB；关闭 8 条路径及 PTY 本机计数回零，96 ms。64 条路径容量由协议夹具单独验证 |
| `node scripts/desktop.mjs build --debug --no-bundle` | Windows 开发构建通过。保留已有 ssh2 编译/链接 PDB 和前端 bundle 大小警告；不表示安装器或发布构建验证 |

JP-01/02/14 由 UI、存储及既有编辑器回归交叉覆盖；JP-03/04/07/09/11/15 由真实 OpenSSH 原生流程覆盖；JP-05/06/08/10/12 由隔离故障协议夹具与 OpenSSH 组合覆盖；JP-13 由原包、真实旧核心迁移及事务测试覆盖。RI-JP、TERM-JP、FILES-JP 消费者验证均经公开核心调用。

## 测试拓扑与边界

专属 QEMU overlay 位于 `.qa/t013/linux/`，原 T-008 磁盘仅作只读 backing。宿主 `127.0.0.1:22314` 转发到 A 的 2222，A 允许 TCP 转发且 `MaxSessions=0`；B 只监听 guest `127.0.0.2:2200`，无宿主端口转发。`target.t013` 仅写入 guest hosts，Windows DNS 查询失败。22313 仅供隔离夹具管理，未用于产品连接。OpenSSH 配置/监听证据在 `linux/topology.txt`；会话和通道日志在清理时保留。

64 路径 OpenSSH 压测首次在第 64 条的 A 握手超时，不能称为 64 条 OpenSSH 实测成功；独立于 SoDeck 的 Paramiko 客户端也在第 64 条复现相同现象（63 成功），见 `load/qemu-capacity-diagnostic.json`，将该限制定位到此隔离环境而非核心 64 路径准入逻辑。随后按层验证 64 条真实协议路径的核心容量边界和 8 条 OpenSSH 的消费/背压。故障测试中的 DNS/延迟/密钥注入属于可控夹具；不声称已覆盖所有 DNS 服务、VPN 或服务器策略。跨平台、安装器、签名、Agent/MFA、多层链不属于本次验证。

实施中发现并修复：文件面板恢复可见时错误释放旧路径租约而导致移窗失败；服务器长表单正文覆盖固定底部操作；Windows 内部桥在 SSH 窗口背压时应按 libssh2 所需方向等待 socket；容量预留必须一直持有到连接交付。相关回归均通过，未放宽取消或缓冲预算。

## 包与清理

独立入口：`.local/dev/SoDeck-v2-T013/Start-SoDeck.cmd`。用户流程见 [验收说明](T-013-acceptance.md)。包的逐文件 SHA-256、源码指纹、插件版本和非发布标记由 `build.json` 记录。包搬迁启动、服务端清理及剩余本地清理项如下。

包搬迁实测已通过：完整文件清单与 SHA-256 一致，不带测试数据/凭据；在工作区以外的复制目录启动，两个插件版本正确，离线首终端失败可见，拆合正常，退出码 0，插件进程退出；重启保留档案但工作区、PTY、连接和租约为空。证据：`package-smoke.json`，对应核心和插件哈希见该文件。

服务端清理已通过：测试账号进程及 A/B 已建立传输均为空，保存 sshd 日志后关闭专属虚拟机；宿主确认记录 PID 已退出、22313/22314 不再监听。原 T-008 backing 保留，Windows DPI、高对比及输入法均恢复原值。

**仍未完成的清理项**：自动审批拒绝递归删除 `.qa/t013/` 内临时测试目录/密钥/overlay 的命令，返回 `blocked by policy`，未提供更具体原因；未改用其他方式绕过。故本地临时文件仍在忽略目录内，测试服务已经停止，未打入开发包。`cleanup.json` 如实记录 `temporaryFilesRemoved:false`。任务保持“开发中（功能/包验证通过，临时文件清理受限）”，不把剩余项或用户验收写成已完成。

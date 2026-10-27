# T-014 实现与验证记录

更新日期：2026-10-27。对应 T-014 r7、代码方案 r1、核心 r6、端口转发 r4、内网代理 r6。工程实现及本次必要验证完成，用户验收与发布尚未进行。验证对象是 `v2` 未提交工作区；没有创建 Git 提交或推送。

## 1. 实际交付

核心 `2.0.0-dev.11`、API/SDK `4.7.0`，独立网络插件 `sodeck.network-proxy@0.1.0`，同包保留终端 `0.7.1` 与文件 `0.4.1`。

- 端口转发：本机回环端口，经所选服务器的多层 SSH 路径，透明连接固定 TCP 目标。
- 内网代理：同一本地端口接受 HTTP/1.1、HTTPS CONNECT 与 SOCKS5 CONNECT；最终 SSH 节点访问请求目标、解析提交给它的域名。浏览器仍使用远端服务原地址。
- 每服务器手动打开管理视图，配置与运行分开；实际入口检查、路径详情、诊断、复制指南、独立启停、默认手动恢复及可选三次自动恢复。
- 隐藏、托盘和视图移动保留运行；明确关闭、停止或退出释放资源；重新打开 App 只保留配置。

HTTP/SOCKS 规则和协议在 QuickJS 插件中；核心仅持有通用 listener、SSH 目标通道、流控、资源和审计。无需部署远端代理程序，不接管系统代理。UI 使用公共组件和三主题，未照搬 v1。

开发包目录：`.local/dev/SoDeck-v2-T014-NetworkTunnel/`。入口和五步验收见 [使用说明](T-014-acceptance.md)。包内 `build.json` 记录文件 SHA-256、源码指纹和版本；仅包含精选证据，不包含开发配置或测试私钥。

## 2. 代码位置及修复

| 层 | 实现 |
| --- | --- |
| 公开契约/SDK | `contracts/network-v1.schema.json`、`packages/sdk/src/network*.ts`；带资源代次的操作回执与可认领结果 |
| 核心 | `crates/core/src/network/`、`ssh/network.rs`；独立网络池、固定事件循环、有界缓冲、原生 splice、超时成员 draining |
| 私有流 | `crates/communication/src/binary.rs`、`crates/plugin-host/`；TypedArray、累计 ACK/额度、消费游标、正文不进 JSON/调用记录 |
| 插件 | `plugins/network-proxy/src/`；规则、状态、HTTP/SOCKS、连接调度及 v2 视图 |
| 工作台 | `network-resources.tsx`、视图资源组、公开服务器编辑入口；保留旧 DTO/旧插件兼容 |

收尾解决了目标超时误报取消、停后立即重启的迟到回执、探测迟到结果、超长列表分页、上游非法 HTTP 字段的 502，以及大负载下状态 ACK 被普通调用挤占的问题。插件 RPC 分普通/状态/清理三通道，总在途不超过 24；目标建连另限制完整操作并发，避免 pending 回执提前释放调度额度。核心 16/池与 64/全局 pending 上限保持不变。

流缓冲采用固定 64 KiB 环形存储与批量复制，EAGAIN 重试保留相同指针、长度及字节。队列有数据时唤醒固定工作线程，减少周期等待，没有为每个流新建线程或扩大窗口。

## 3. 验证分层

| 场景 | 实际证据与结论 |
| --- | --- |
| 静态/契约/浏览器 | `check.log`、`contracts.log` 通过；浏览器 56 项通过（注入桥，不充当 SSH 证明） |
| Rust | `rust-final.log` 工作区通过；显式外部网络夹具 `core-network-final.log` 6 项通过；需环境的其他历史测试仍在工作区命令中标记 ignored |
| 真实 QuickJS | 工作区测试包含真实子进程 2 MiB 字节往返、握手尾部移交、反向半关闭、实例失效清理；SDK/二进制单元验证乱序、额度、旧代次及队列上限 |
| 原生基础流程 | `native-latest.json`：真实 Windows App UI，经三个 Paramiko 协议节点，创建/认证/HTTP/SOCKS/固定转发/检查/独立停止/编辑入口/关闭 |
| 真实远端平台 | `openssh-latest.json`：Debian OpenSSH A→B→C→D→独立 LAN E/F；真实 Chromium、TLS 证书验证、远端域名、SSE、提前 413、101 尾部、64 MiB 内容校验 |
| Windows SSH | 同一 Windows 上独立临时 loopback Win32-OpenSSH，验证其作为最终节点的 HTTP/SOCKS/固定转发，以及 Windows→Linux 四节点中转；不是独立 Windows VM/LAN 测试 |
| 地址方向 | 本地与 D 均有 `127.0.0.1:18080`，经代理只收到远端标记，本地诱饵服务请求数为 0；浏览器同代理切换 E/F。A 网络与 D 后方 LAN 的 `198.18.14.18:18080` 分别返回不同标记，两模式并行无串路，停止一个入口不影响另一个 |
| 生命周期 | `lifecycle-latest.json`：身份取消、端口占用、共享传输强停影响、移窗草稿/运行保留、托盘、路径修订、1/3/10 秒有限恢复、准确 worker PID 终止后资源及端口释放 |
| 独立更新 | `updates-latest.json`：测试 0.1.1 同时改变 UI 与真实 HTTP 错误行为，重启生效，核心 hash 不变；坏 0.1.2 prepare 写入回滚且退回 0.1.1。测试版本不作为生产插件交付 |
| 兼容 | `compatibility/results.json`：实际 API 4.6 旧核心拒绝新插件；新核心加载未重编译旧终端 0.6.0/文件 0.3.2，真实四节点 SSH、PTY、SFTP、exec 通过 |
| 原生视觉 | `visual-latest.json`：Windows 拼音真实 composition，三主题、100/200% DPI、900×600 逻辑尺寸、高对比、减少动画；截图已查看，系统设置恢复 |
| 故障/边界 | 三节点 Paramiko 夹具验证真实 libssh2 未回复 open、15 秒超时、取消、四 draining 成员、既有流持续可用；缓冲/资源单元触达 512 端点、64 MiB 和 pending 上限；审计存储故障仍能清理，日志不含正文 |

证据根目录 `.qa/t014/`；每个 `*-latest.json` 记录实际带时间戳目录。原始失败证据保留。Paramiko 是故障协议夹具，真实平台结论仅来自对应 OpenSSH 测试。

## 4. 性能关口

同一 Windows、Intel Core Ultra 5 250K Plus（18 逻辑处理器），隔离 Debian QEMU TCG 2 vCPU / 2 GiB，一跳、相同目标、64 MiB，debug 构建三次中位数。正式参考程序 `crates/core/examples/network_reference.rs` 使用同工作区 ssh2/OpenSSL，直接 TCP→direct-tcpip 复制。

| 路径 | 三次耗时 ms | 中位数 ms |
| --- | --- | --- |
| 原生参考 | 1231.72 / 2034.80 / 1112.42 | 1231.72 |
| 核心 splice | 1471.65 / 1209.77 / 1256.37 | 1256.37 |
| 插件 HTTP | 1674.01 / 1719.12 / 1426.17 | 1674.01 |

splice/参考吞吐为 **98.04%**（要求 ≥70%），HTTP/splice 为 **75.05%**（要求 ≥25%）；每次长度及 SHA-256 一致。核心 RSS 约 50.0→50.5 MiB、线程 56→56；worker RSS 19.5→20.8 MiB、线程 6→6。完整 CPU/RSS/私有内存/线程见 `benchmark-latest.json`。这些是此隔离环境的实现关口，不是网速承诺；早期失败保留，未修改门槛。

并发最终记录 `load-latest.json`：16 规则、64 条活跃流保持 **60,088 ms**，16 条停读 10 秒后恢复；状态持续 ready，后续真实请求成功。控制 p95 **188.25 ms**（要求 ≤200 ms），四节点 OpenSSH 命令行对照吞吐 **94.37%**。100 次启停全部通过，每次检查资源归零，最长停止 **38.92 ms**（要求 ≤1 秒）；关闭及重开只恢复规则配置。正式三轮吞吐基准与此四节点单次对照分别记录。

移位目录包检查通过：完整复制到 `.qa/package-smoke/1793042972343/relocated package/` 后，以该目录为工作目录启动；全部声明文件及 SHA-256 一致，三个独立插件版本正确，没有附带服务器、数据目录或私钥。手动打开网络视图、保存规则、移窗/合并、正常退出、worker PID 退出、重启配置保留且无运行资源均通过。证据 `package-smoke.json`；测试数据只写入隔离目录。

专属环境已清理：从 Linux A 与 B/C/D/E/F 网络命名空间核对目标 SSH/TCP 连接为零后正常关闭 QEMU PID 16652；停止精确匹配专属配置的 Windows sshd PID 28124。22323/22324/22327 均不再监听，日常 sshd PID 27636 仍运行。清理了早期 OpenSSH 参考测试遗留子进程，并修正负载脚本按参考进程树退出。DPI/高对比恢复原设置；保留测试证据及隔离磁盘，没有修改日常配置。证据 `cleanup.json`、`linux/final-remote-audit.json`。

## 5. 限制与验收状态

仅交付 Windows 开发目录包，未验证 macOS/Linux 桌面、签名安装或正式升级发布。支持 TCP；不提供 UDP、VPN/TUN、系统代理/PAC、远端监听入口或退出后后台驻留。远端 SSH 必须允许所需 TCP 转发且能到达目标；SOCKS 远端 DNS 需要客户端提交域名（如 `socks5h`）。

用户验收未进行；方案确认、工程验证、用户验收和发布分别记录。测试只使用隔离环境，没有连接需求示例中的用户业务地址，也没有修改用户日常 SSH 服务。

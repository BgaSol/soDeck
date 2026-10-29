# T-020 实施与验证记录

更新：2026-10-28 · 修订：r1 · 状态：工程检查完成，待用户验收。

范围依据：[任务 r6](T-020-socks-route.md)、[F-CORE-027 r4](../core/socks-route.md)和[代码方案 r3](../technical/socks-route-implementation.md)。用户“你开始做吧”已授权完整实现、SK-01～09 与独立 Windows 开发包；没有提交、推送或发布授权。工作区分支 v2，保留其他任务原有未提交内容。

## 实现

- 目标服务器使用 direct / hostTunnel / networkProxy 三种互斥 accessPath。SSH 主机隧道按目标的显式有序列表逐台认证，不继承跳板自身路径，不创建跳板 PTY。
- SOCKS5 代理拥有独立档案、账号和加密秘密 namespace。只进行网络方法协商、可选用户名密码认证及 CONNECT；同一 TCP 流交给最终目标 SSH，不登录代理主机，不创建代理指纹或工作区。
- 代理管理独立于服务器；目标表单支持添加、编辑、删除/依赖阻止、草稿与焦点恢复。状态、认证和路径展示分别标明网络代理与 SSH 主机。
- 默认代理解析目标域名，可按目标选择本机解析；后者多 IP 逐个经 SOCKS 尝试，无直连降级。协议按帧精确读取，保留紧随 CONNECT 的 SSH banner，记录实际网络协议字节。
- 凭据/信任绑定有效访问上下文，连接池绑定不可变配置与秘密修订。名称变化和密码轮换不改变主机身份；端点、账号、DNS 变化不会向新上下文复用旧凭据/信任。旧连接继续使用原快照，旧认证答复拒绝写入。
- API/SDK 5.0.0 单一契约替换旧路由契约。官方终端 0.10.0、文件 0.8.0、网络隧道 0.4.0 同步升级；插件无需处理代理协议或接收代理密码。
- 核心数据库 v6；结构不匹配先提示并完整备份/核验，再创建空库，不导入旧配置。代理秘密子目录包含在备份内。仅支持当前数据集恢复 journal；未知旧恢复格式保留原文件并报错，不执行旧迁移。

## 环境与证据

Windows 原生 Tauri/WebView2；开发构建 2.0.0-dev.17，custom-protocol debug。SOCKS 是隔离 Python TCP 服务（真实 SOCKS5 字节协议，无 SSH 服务）；核心集成使用 Paramiko SSH 协议夹具，桌面使用隔离 QEMU Debian/OpenSSH，22320 端口仅供本任务。代理把仅夹具识别的 target.internal 映射至隔离目标，本机不配置这个名字。没有使用业务服务器或日常代理。

证据根为 `.qa/t020/`。协议夹具只记录方法、目标和结果，不记录密码。取消试验中 Paramiko 的 banner EOF 日志是客户端中止握手的预期结果，以取消延迟、资源断言及 socket open/close 平衡判断通过。

| 检查 | 实际结果 | 证据 |
| --- | --- | --- |
| 生成文件、TS、模块/UI 边界 | 通过 | check-latest.log |
| JS/TS/SDK/插件契约 | 通过；SOCKS 挑战禁止 SSH server/role/hop 字段 | contracts-latest.log |
| Rust 工作区 | 通过；外部夹具的 ignored 项另行运行 | workspace-tests-latest.log |
| SOCKS 协议 + 真实 SSH 交接 | 7 通过 | socks-protocol-final.log |
| 显式路径与 SOCKS 集成 | 4 通过，含并发保存、DNS 独立、配置变化挑战失效、无认证/账号、100 次重连、50 次取消、多 IP 首个拒绝后重试 | access-path-latest.log |
| SSH 主机隧道对照 | 11 + 8 通过，含多级、15 跳、容量、取消、信任及资源回收 | real-routes.log |
| 核心网络消费者 | 6 通过 | network-final.log |
| 数据集备份/中断/恢复 | 11 通过，含嵌套代理秘密、WAL、损坏备份拒绝 | storage-final.log；亦包含在工作区测试 |
| Windows 原生 SOCKS/OpenSSH | 通过；真实 OpenSSH、中文 IME、窗口和全部资源释放 | native-latest.json 指向完整目录 |
| 独立目录包普通启动 | exe / 启动脚本均通过；三插件、空 v6、隔离 data、正常退出 | package-ordinary/results.json |

## SK 验证对应

| 用例 | 入口与已覆盖行为 |
| --- | --- |
| SK-01 | 原生三项访问方式、独立代理管理、父子表单保存/取消与草稿/焦点 |
| SK-02 | 真实 OpenSSH 终端输入、SFTP 列表/stat、exec；1 个目标 SSH / 0 bridge，代理仅 SOCKS 日志 |
| SK-03 | 无认证/账号、错误密码、拒绝降级、过期答复、取消/超时；两类挑战严格分离 |
| SK-04 | DOMAIN/IP/IPv6 编码、畸形/截断/分片、8 种 REP、SSH banner 精确交接；本机多 IP 首个拒绝后成功 |
| SK-05 | SSH 显式列表多级与原有容量/认证/释放回归；修改跳板自身路径不改变目标路径或凭据 |
| SK-06 | 两终端、文件、独立网络池；关闭单个/全部、移窗合并、托盘隐藏恢复；50 次取消及 socket 平衡 |
| SK-07 | 代理秘密隔离/端点变化清除、配置修订/名称变化新池与旧连接快照、依赖删除阻止、挑战过期 |
| SK-08 | fresh v6、旧 API/插件拒绝、无迁移、备份核验及每个重启边界；生成契约和运行时校验 |
| SK-09 | Windows 原生 900×600 逻辑尺寸、100%/200% DPI、三主题、Tab/Esc/焦点、高对比；减少动态为 WebView2 媒体模拟，中文 IME 使用可信 Microsoft Pinyin composition/input 实测通过 |

最终原生证据：native-1793169316236；文件重复连接保持 ready，网络插件路径明确显示“SOCKS5 网络代理 · 1 台主机”，真实转发返回 OpenSSH banner。截图已查看。退出 exitCode=0、forced=false；DPI 175% 与高对比 flags=126 恢复到运行前值。IME 独立证据在 native-1793168710322/ime-events.json，该轮完成可信中文输入但随后暴露文件重复连接缺陷；后续因外部前台焦点变化被测试安全检查拒绝发送键盘输入，最终流程使用 SODECK_TEST_SKIP_IME=1，仅复用同一外壳二进制已通过的 IME 证据，没有伪造新一轮通过。

目录包普通启动证据：package-ordinary/results.json 和 schemas.json；复制到两个含空格的新目录分别启动，忽略陈旧开发变量，全部插件加载、无启动错误、data 位于各自目录、数据库 user_version=6、全部进程正常退出。WebView2 外部检查端口用于断言；应用普通模式，不使用开发插件路径覆盖，不宣称关闭所有检查端口的验证。48 个交付文件已逐文件复核 SHA-256，版本及源码指纹保存在 build.json。

测试服务清理见 cleanup.json：仅停止核实过路径、-snapshot 参数和 22320 转发的专用 QEMU；本任务 SOCKS 夹具均已退出，22320 释放。未关闭用户其他实例。Git 差异格式检查通过；8 份文档的 429 个本地链接无缺失。

## 修复与验证边界

实施中修复了代理秘密目录未创建、预览遗漏路径、路径变更过度清除 SSH 凭据、旧 SDK 挑战默认 SSH 索引、900×600 表单页脚越界、网络代理错误误显示为目标阶段等问题。修复使用相同验收预期，未降低检查要求。

SK-06 最终原生回归发现：文件视图自动连接已 ready 后再次调用 files.connect，会先改成 loading，随后因已有 lease 提前返回，永久保留 loading。修复属于本次多消费者已确认范围：重复连接保留已有视图的加载结果，不多持有 lease。补充 ready 后重复调用的单元回归，并重新运行 SOCKS/OpenSSH 原生文件读取；失败证据保留在 native-1793168710322。

仅验证 Windows 开发目录包，不代表正式安装包、macOS/Linux 客户端或用户验收。SOCKS 服务实现为可控协议夹具，真实 SSH 交接另使用 Debian OpenSSH；IPv6 协议编码/多 IP 已验证，不宣称公网 IPv6 代理环境矩阵。没有实现混合链、SOCKS 链、系统代理或自动回退。本轮没有重新验收其他任务的全部功能。

## 用户验收入口

最终目录：`.local/dev/SoDeck-v2-T020/Start-SoDeck.cmd`。数据位于包内 data，开发测试使用复制包的隔离 data，交付目录不预载测试服务器或秘密。

1. 添加目标服务器，选择“通过 SOCKS5 网络代理”，创建自己的代理并保存。
2. 验证先出现网络代理认证（需要时），随后只确认/登录目标 SSH；连接状态标明经由 SOCKS5。
3. 打开两个终端与文件列表，关闭一个终端确认另一个保持；可打开网络隧道检查独立消费者。
4. 改为 SSH 主机隧道，按顺序选择跳板，确认逐台 SSH 身份/认证；检查代理管理中没有 SSH 主机账号/私钥字段。
5. 关闭工作区、从应用菜单退出，再启动检查配置保留且连接不自动恢复。

用户验收、Git 提交、推送和发布均未执行。

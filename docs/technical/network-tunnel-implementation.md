# 网络隧道插件代码实施方案

更新日期：2026-10-27 · 方案修订：r1 · 文档就绪：已确认实施并完成工程验证，用户验收见 T-014  
任务：[T-014 r7](../tasks/T-014-network-proxy.md)  
功能：[端口转发 r4](../plugins/proxy/features/port-forward.md)、[内网代理 r6](../plugins/proxy/features/network-proxy.md)  
公共契约及预算的有效来源：[F-CORE-015 r6](../core/dynamic-proxy.md)

实施记录见 [T-014 结果](../tasks/T-014-results.md)。下列代码基线表保留 2026-10-26 的核实事实，计划路径已有实际实现，不把方案提交时的未建立状态当作当前状态。收尾在原预算内增加完整目标操作的有界调度、固定缓冲批量复制及队列唤醒；未改变核心/插件职责或协议范围。

## 1. 本次选择与实际基线

交付一个 `sodeck.network-proxy` 插件，含 React/TypeScript 界面和 QuickJS/TypeScript 业务 worker。核心提供通用网络原语，插件包内实现固定转发编排、HTTP/1.1 和 SOCKS5 协议。HTTP/SOCKS 处理在本机，目标 socket 由最终 SSH 主机创建；不部署远端 helper，不依赖终端，不接管系统代理。

2026-10-26 只读核实的基线如下；本稿路径均相对仓库根目录。

| 代码位置 | 当前事实 | 本次增量 |
| --- | --- | --- |
| `crates/core/src/route.rs`、`route_store.rs`、`route_v2.rs` | T-013 已支持任意有限无环链、逐跳身份及分页路径；API/SDK 4.6.0 | 复用，不重写路径模型，不恢复固定跳数限制 |
| `crates/core/src/ssh.rs` | 目标连接共用 PTY/SFTP/exec；每 session 的 channel-open 操作需要串行；已处理 EAGAIN 的原调用续进 | 为网络数据建立独立传输池和目标通道执行器，复用核心建连/认证代码 |
| `ssh/transport.rs` | 内部跳板桥在失败时会关闭其 gateway socket | 不能直接作为业务目标桥；新目标失败不得关闭父链路 |
| `crates/plugin-host/src/engine.rs`、`bootstrap.js` | QuickJS 128 MiB、5秒连续执行预算，异步 core call；没有 Node 网络 API | 保留引擎，增加受控的二进制流会话适配，不给插件直接 socket/SSH |
| `plugin-host/src/transport.rs`、`process.rs` | 长度前缀 JSON，单帧1 MiB，32个待处理调用和有界队列 | 在同一私有管道中增加独立调度的二进制帧，不把正文转 base64/JSON |
| `plugins/files`、`plugins/terminal` | 界面/worker 分离，视图可迁移，独立签名包及重启更新 | 新建第三个真实业务包，沿用加载和更新流程 |
| `Core::release_view_resources`、工作台 v4 Resource | 当前只认识 PTY/lease | 增加按逻辑 view 归属的通用网络资源组；不向旧严格 DTO 塞入新枚举 |

没有新增代码、schema、测试脚本、依赖或开发包。本方案明确提交实现和验证范围，用户最终确认后一起实施，不先做未授权的可执行原型。

## 2. 分层及模块

```mermaid
flowchart LR
  UI[插件 React 界面] --> W[插件 QuickJS worker：规则 / HTTP / SOCKS]
  W --> SDK[公开 SDK：控制与字节流会话]
  SDK --> N[核心：资源组 / listener / stream / splice]
  N --> S[核心 SSH 路径与网络传输池]
  S --> R[最终 SSH 节点]
  R --> T[目标服务]
```

| 新建或修改位置 | 职责与限制 |
| --- | --- |
| `plugins/network-proxy/manifest.json`、构建入口 | 插件身份、能力依赖、单工作区管理视图；初版0.1.0，UI和worker一起打包 |
| `src/worker.ts`、`rules.ts`、`controller.ts`、`runtime.ts` | 命令分派、配置存储、运行状态机、串行启停/编辑、测试与恢复；按 view/workspace generation 建立上下文 |
| `src/protocol/sniff.ts`、`http.ts`、`http-body.ts`、`socks5.ts`、`address.ts` | 有界增量解析、目标选择、HTTP framing/逐跳头、SOCKS协商；不导入原生模块 |
| `src/view.tsx`、`rule-editor.tsx`、`rule-detail.tsx`、`state.ts` | 只消费插件命令/快照和公开UI，流量正文不进入WebView；界面按插件总览 §3 |
| `crates/core/src/network/{mod,group,listener,stream,splice,operation,resources}.rs` | 所有原生socket/句柄、资源/代次、缓冲/背压、幂等与清理、公开契约校验；不识别HTTP/SOCKS规则类型 |
| `crates/core/src/network/{route_pool,tcp}.rs`、`ssh/network.rs` | 复用现有路线解析/逐跳验证，隔离网络传输池、非阻塞direct-tcpip、通道失败与传输故障区分 |
| `packages/sdk/src/network.ts`、`network-stream.ts`及生成类型 | 类型校验、ByteSession、读取窗口/消费游标、写额度、取消/回执；纯JS兼容QuickJS，界面只使用控制子集 |
| `contracts/network-v1.schema.json`、生成脚本 | 所有新方法/事件/结果/错误及帧版本的单一schema来源，生成Rust/TS校验；不手改生成产物 |
| `crates/plugin-host/src/{transport,process,engine,bootstrap}` | 私有管道二进制帧、TypedArray拷贝、额度/公平调度和原生来源绑定，不含代理业务 |
| `apps/desktop/src-tauri/src/network_services.rs`等宿主接入 | 校验view归属、状态feed接入、关闭/工作区销毁/崩溃兜底，核心资源面板展示网络资源；不复制插件状态机 |
| `scripts/{generate-network,pack-plugin,package-dev}.mjs`及边界检查 | 按已有工具风格增加契约生成/独立包/三插件开发包；只扩展实际需要的通用规则 |

不新增插件原生可执行文件、WASM运行时或Node服务。依赖优先沿用 Rust 的 polling/socket2/ssh2 和当前 TS 工具链；代理解析采用受测试约束的增量状态机，不引入浏览器/Node专属网络依赖。不得将HTTP parser编入核心来绕过数据通道验证。

## 3. 三种数据路径

**固定转发：** 核心接收本地连接 → 插件读取规则固定目标 → `net.tcp.open` → `net.streams.splice`。成功后双向字节直接在核心流转；插件只接收状态/计数。

**SOCKS5 / HTTPS CONNECT：** 插件通过 ByteSession 读取最小握手字节 → 校验并提交动态目标 → 等远端目标确实打开 → 发送成功应答 → 将两端交给核心splice。握手报文与后续数据同包到达时，未消费尾部留在核心读取窗口中，不丢失、不重复。

**普通HTTP：** 插件增量解析请求/响应边界、改写代理请求头和决定目标；正文以二进制流会话分块传输并背压，不拼成完整字符串、不缓存在页面。完成一次响应后可处理同客户端的下一个请求，包括不同目标。协议业务随插件更新，核心只理解字节、EOF、额度和句柄。WebSocket收到正确101后转splice，SSE持续流式转发。

HTTP实现明确：绝对URI转origin-form，目标与Host一致；拒绝歧义Content-Length/Transfer-Encoding、NUL/非法换行及不支持的transfer coding；正确处理HEAD/204/304、Content-Length、chunked含trailer、close-delimited响应、100/其他临时响应与Expect。请求体发送和响应读取并行，以免100 Continue或服务器提前拒绝导致死锁。移除Proxy-Authorization/Proxy-Connection及Connection指定的逐跳头，Upgrade按协商专门处理；不记录Authorization、Cookie或URL路径。HTTPS只做CONNECT，不解密TLS。

普通HTTP客户端keep-alive复用，流水请求按顺序处理且只保留有界待解析字节，不同时创建无上限的请求队列。为降低上游复用状态复杂度，首版每个普通HTTP请求建立一个上游目标通道，当前响应结束后释放；下游仍可复用。非101连接不因Upgrade请求就提前转为透明隧道。HTTP/2/3明文代理入口首版不支持；HTTPS隧道内客户端协议透明传输，UDP/QUIC仍不在范围。

SOCKS5仅无认证CONNECT，支持IPv4、IPv6和域名；域名交由最终SSH主机解析，拒绝BIND/UDP ASSOCIATE。SSH协议通常不能可靠区分所有目标DNS/路由/拒绝原因，保留可证实的阶段及原始错误类别，不伪造精确原因；未知目标错误返回SOCKS general failure。

## 4. SSH通道超时与连接池

现有libssh2在EAGAIN后需继续同一次channel-open，超时不能简单丢弃后开始不同目标；内部跳板桥的失败还会关闭gateway。这两点是本任务必测重点。

采用核心管理的网络数据池，键为工作区创建代次、最终serverId、完整routeRevision及用途`network-data`。插件不获得session或凭据。该池与终端/SFTP的物理会话分开，两种网络规则可在同一池内持有独立租约；额外SSH会话全部计入原有64目标连接、256session/桥预算，不能另开无计数连接。

实现时抽出核心已有建链、逐跳验证/挑战及全链预算预留作为内部共享服务；常规connections.acquire仍只查常规连接用途，不能因同serverId/routeRevision而误拿网络池成员。网络池登记自己的运行依赖，服务器删除/改路提示、信任失效、逐跳挑战、route资源查询和退出清理均合并检查两类消费者；对外旧DTO保持，网络详情走新分页接口。内部锁按scope/配置协调→预算/注册→成员调度分阶段获取，不持有全局注册锁等待SSH或插件回复。

- 一个池至多4条物理路径，每条为完整嵌套SSH传输；按需创建，初始只建立1条。不是每个HTTP请求重新认证。每条正常路径可复用多个已打开目标通道，同一时刻只推进一个libssh2 channel-open。
- 超时/取消的open立即对调用者结束，后台仍继续排空原调用；该物理路径标为draining，禁止接新open，已有目标通道继续I/O。新请求选另一健康路径或在预算内创建新路径，不重连整条规则、不误关现有通道。
- 如果draining路径已无任何有效目标通道，可关闭它自己的传输回收pending open；若还有别人的有效通道，保留实际cleanupPending，直到open排空或有效通道自然结束。全部4条占用时明确容量不足；不无限创建，也不杀掉健康长连接腾位置。
- 普通目标拒绝且open已结束，只关闭该目标；单条池传输故障影响该物理传输的通道，其他池成员仍可用。规则只在没有可用传输时进入需恢复状态。新建池成员仍须相同路径修订/有效信任，不换出口。
- 多余空闲成员30秒后回收；存在活跃路由租约时保留一个就绪成员。规则停止释放自己的通道与租约；最后租约关闭池，所有成员同时取消，不能按跳数逐个累计长等待。

已有连接保留路径快照。恢复/补建会话时以expectedRouteRevision复核当前配置，修订已变化进入“需要处理”，等待明确应用并重启，不自动使用新路径或重用已失效凭据。该约束细化此前“恢复不悄悄换出口”的要求；不重新连接已删除或信任已失效的旧配置。

## 5. 二进制流与来源

控制RPC继续JSON；二进制流是通过已审计 `net.streams.exchange` 建立的持续调用，不通过state通知、UI回调或独立的逐块JSON写入方法。宿主把绑定的Caller和ByteSession关联，不接受插件帧中的pluginId作为身份。

私有管道保留4字节大端长度前缀。JSON原帧保持不变；新二进制帧以0x00标记，含version、type、flags、32位attachment槽、32位epoch、64位序号/偏移、32位payload长度，共24字节头。payload≤64KiB；ACK/CREDIT/FIN/RESET均有明确类型与校验。仅新宿主与其同版本子进程在初始化协商后启用；旧插件继续JSON，不要求重打包。

ByteSession提供读取窗口、消费游标和有额度的发送队列。接收采用peek/consume：核心保留未消费字节，插件只能提交连续消费位置；投递不等于消费。发送完成只表示核心接纳字节，不表示服务已执行。所有队列含QuickJS副本纳入预算，SDK不再无界复制。关闭/额度独立控制队列优先投递，数据轮转调度；没有每帧新线程。

数据帧使用单调偏移及累计额度，重复credit不能增加额度、旧epoch不能作用于复用的槽位；超过JavaScript安全整数范围前结束会话并要求新建，不循环复用序号。read/consume是本地ByteSession的窗口操作，发送队列是该持续调用的帧生产端，若另建独立远程write请求则按独立API逐次审计。单pipe下控制优先发生在帧边界；数据帧上限限制不可抢占写入长度，worker不读或失效时核心仍能直接取消资源，不能等待pipe腾空间才关闭socket。

交给splice时冻结两端ByteSession，核实epoch/消费游标，完成已接纳的写队列，再原子转移读写所有权；未消费尾部由核心先转发，旧epoch的迟到帧失效。失败则保持原owner或明确关闭，不能同一字节被JS与splice同时读取。半关闭只结束一个方向，另一方向读到EOF后再释放；固定转发、CONNECT和SOCKS使用同一实现。

逻辑SDK流会话内的组帧/额度操作属于既有持续调用的数据帧，不是旁路RPC。控制API每次逐条记录；流建立、终止、累计字节、错误与异常帧摘要有审计记录。若实现另行暴露独立write API，必须逐次留痕，不能以吞吐为由采样。帧正文、握手正文和秘密均不得落审计记录。

## 6. 配置、状态与操作

持久schemaVersion=1：规则具有id、revision、name、serverId、type（固定后不变）、bindMode/port、autoReconnect；只有port-forward保存targetHost/targetPort。私有storage命名空间保存一份有版本规则集合，worker全局串行写入并比较expectedRevision；保存失败不替换内存正式配置。首版最多256条保存规则、每条名称128字符、目标255字节，序列化≤512KiB，低于既有1MiB插件存储上限。未知schema拒绝写入；不保存SSH秘密、运行句柄或请求历史。服务器已删除时保留不可启动的规则并允许删除，不按同名服务器自动重绑定。

每次启动产生新的runId和runGeneration，绑定不可变规则快照、核心groupId/routeId、workspaceGeneration和viewId。重复点击启动/停止/重启由每规则操作队列合并；创建有operationId和回执，不因超时换ID重试。已保存配置与正在运行快照同时显示；运行中改端口/目标不立即生效。

启动顺序：创建资源组 → 绑定暂停accept的本地端口（先发现冲突）→ 获得核心路由 → 开启accept → ready。认证挑战使用现有核心身份流程；取消或启动失败释放本次listener/路由/组。ready仅证明入口/SSH就绪，不能声称任意目标或网页业务成功。

重连按插件总览1/3/10秒最多3次、默认关闭。规则失去可用路径时保留端口，代理请求返回协议失败、固定转发关闭新连接；不积压等待、不重放已发送请求。恢复只让客户端新建连接。工作区切换、隐藏/移窗/托盘不触发网络重建；UI快照轮询可见时500ms，隐藏停止页面轮询，worker与核心仍运行。

诊断环形缓冲最多200条，含阶段、时间、目标主机/端口及错误，不含URL路径/正文/认证信息。UI分页最多50条，单快照≤32KiB，统计每秒合并；全量核心资源分页查询，不能靠日志保存业务状态。

## 7. 检查、关闭及平台适配

连接检查必须经过实际listener。内网代理使用核心只允许连接本资源组listener的 `net.listeners.connectSelf`，发送真实CONNECT/SOCKS握手并等待目标成功回复。固定转发使用该方法返回的probeId关联实际accept及目标open结果：仅本机TCP connect成功不算远端连接成功。检查不发送业务请求，不宣称TLS/网页/数据库验证成功；测试结果绑定runGeneration和目标，停止/修改后标记过期。

网络资源组建立时宿主验证worker、view与工作区创建代次并登记核心view索引；所有子资源原子归组。因此不需要把新资源kind添加到旧workspace.v4的PTY/lease枚举中。关闭逻辑视图/工作区的提交阶段由宿主调用核心撤销组，即使插件未返回也有兜底；用户取消关闭时不撤销。移窗只换表面身份，不换资源owner。插件崩溃、禁用生效及App退出撤销所属组和数据帧会话。

核心资源面板通过新网络分页接口列出组/监听/池/通道、实际状态及cleanupPending，可停止组或物理传输，影响范围在结果中给出。强停共享物理传输与普通释放自己的使用权分开；插件崩溃后仍能查看清理。正常停止1秒内结束本地监听/撤销句柄，5秒内完成不受外部阻塞的本地清理；未完成的SSH drain真实计数不能清零伪装完成。

远端只依赖SSH TCP转发，不检测shell、不执行curl、不写临时文件；Windows/Linux差异主要由SSH服务的转发配置/网络可达性决定。客户端仍先交付Windows。监听127.0.0.1，目标支持IPv4/IPv6/域名；系统/浏览器代理设置及私网bypass由指南说明。

## 8. 兼容、构建与交付

推荐API/SDK 4.7.0，新增 `network.tcp.v1`、`network.stream.v1`、`network.resources.v1` 能力；不改变现有严格请求/响应。新插件最低要求4.7及这些能力、routes.v2和既有workspace/state/storage/theme能力。旧核心拒绝激活新包并显示缺失能力；新核心跑未重编译terminal/files旧包，验证公开协议兼容。具体开发版本号实施时在当时基线递增，不覆盖其他任务版本。

包沿用manifest/ui/worker签名结构，插件代码没有自带原生网络程序。准备更新不中断当前规则，重启后启用新包且所有规则初始停止。0.1.0→0.1.1验证使用真实HTTP解析/诊断行为变化，不能只改版本文字；坏包/初始化失败按既有机制回退或禁用单插件。迁移只在本地副本，初始化不得建SSH或监听。

交付独立开发目录 `.local/dev/SoDeck-v2-T014-NetworkTunnel/`（计划路径），包含核心、三插件包、独立数据入口、版本/散列清单、测试证据和验收步骤；另提供可独立安装的插件包。验证搬离源码目录后启动。此次不含v1规则自动迁移、正式安装器、签名发布、Git提交或推送。

## 9. 验证与实施顺序

所有以下入口为拟新增，不宣称已可执行。复用已有check/contracts/Rust/原生工具，具体功能步骤见PF/NP/NT；结果仅记T-014。

| 顺序 | 实施及验证 | 通过条件 |
| --- | --- | --- |
| 1 | network schema/SDK、原生资源和传输池；`crates/core/tests/network.rs`、`tests/core/network_fixture.py` | 真实direct-tcpip、多级目标、并发/半关闭；卡住的open不误关其他通道/终端/文件，预算/迟到结果/取消可核对 |
| 2 | plugin-host二进制帧、ByteSession及splice；`crates/plugin-host/tests/network_stream.rs` | QuickJS真实进程可流式读写；握手与正文同包无丢重，信用/乱序/旧epoch/缓冲/公平/崩溃可控 |
| 3 | 插件规则/HTTP/SOCKS、UI和生命周期；插件protocol/worker测试及`tests/ui/network-tunnel.spec.ts` | 不是模拟按钮：真实worker驱动核心；协议边界、保存/启动、视图移动、配置修订正确，UI模拟结果单独记录 |
| 4 | `tests/desktop/network-tunnel.mjs`、`network-tunnel-visual.mjs`、`network-tunnel-updates.mjs` | 实际浏览器/代理客户端、隔离Windows/Linux SSH、三节点链、多目标同IP隔离、真实三主题/键盘/IME/缩放、更新回退 |
| 5 | 预算压力/旧插件回归/开发包与清理 | NT容量指标通过；旧包终端/文件仍可用，全部功能PF/NP/NT完成且结果不混测试层级 |

前两步是正式实现中的技术验证关口，包含本次最终确认范围；未通过先修复，不带着漏字节、无限缓冲或误断会话问题继续宣称功能完成。如必须改变插件独立更新、核心边界或用户功能才可解决，先写回方案并说明影响，不能默认降级。

真实环境计划：复用T-013专用QEMU Debian镜像的独立overlay和命名空间，建立A/B/C与后方HTTP/TLS/TCP服务，只允许相邻SSH可达；独立Windows Sandbox/VM中使用Win32-OpenSSH作为最终节点及至少一次中间节点。若只能建立Windows协议模拟夹具，保留真实Windows验证未完成，不用Paramiko冒充。缺少Windows Sandbox/VM镜像时先使用可获得的隔离Windows测试环境；安装系统组件、持久改宿主网络或创建系统账户需另说明，不能默认修改日常环境。测试范围不依赖用户业务服务器。

所有测试使用独立数据、浏览器配置、凭据和端口；结束后关闭App、监听、sshd/服务/VM，核对OS连接、session/桥与资源预算，恢复临时设置。残留清理如受实际策略阻止如实记录，不换工具绕过。对照T-013旧结果仅用于回归基线，不能代替本插件实测。

# T-003 联调契约

日期：2026-10-25 · 修订 r2 · 状态：已确认方案内的实现细化

本文件补充 [T-003](../tasks/T-003-core-terminal.md) 的工程边界；不改变产品范围。编码与测试按核心 SSH、终端及首批技术规格执行。

本轮协作实现待办：UI 代理请读 `.qa/ui-followup.txt`（通用清单驱动工作台规则，`workbench.rs` 归其修改；`app.rs` 归父任务）；插件平台代理请读 `.qa/plugin-platform-followup.txt` 后半段（原生宿主审查、剩余包异常测试、开发入口）；核心代理请读 `.qa/core-followup.txt`（后端构建、真实负载/回收及记录异常）。这些文件是临时协作通道，正式结论写回任务附录；代理消息正文为空时直接读取文件。

- 核心 Rust crate `sodeck-core` 提供 `Core::open(data_dir)`、`Core::call(caller, method, params)`，返回 JSON 值或结构化错误；服务器、认证、SSH/PTY、日志、资源与设置都经此边界。桌面 crate 负责 RPC envelope、原生来源绑定、窗口与插件视图路由。
- RPC 请求为 `{apiVersion:1, requestId, method, params, timeoutMs?}`，响应 `{requestId,result}` 或 `{requestId,error:{code,message,operationId?,phase?}}`。调用者由原生宿主绑定，不读取参数中的自报 pluginId。所有公开调用经记录边界。
- 核心调用者字段：`source`（shell/plugin）、`pluginId?`、`pluginVersion?`、`instanceId`、`viewId?`、`mountGeneration?`。Rust 命名使用 snake_case、serde camelCase。
- 会话使用稳定 serverId/workspaceId/viewId，核心返回 leaseId/connectionId/ptyId/generation。PTY 输出使用有界、带游标的 `pty.read` 与 `pty.ack`，每块 base64 编码，读调用自身留痕；只在 xterm 消费后确认。旧输入绑定由原生宿主与核心代次共同校验。原生层不得把无界事件广播当输出流。
- UI 的 `core.call(method,params)` 不区分 shell 或插件能力白名单。外壳经 Tauri `rpc` command，插件原生页面经 `window.ipc.postMessage`；原生响应入口 `window.__sodeckReceive(response)`。视图启动提供 `window.__SODECK_CONTEXT__`（已绑定版本、视图、服务器等非秘密信息）。浏览器测试可以注入测试 transport，但生产缺少原生桥时明确不可用。
- Rust 插件管理 crate `sodeck-plugin-manager` 负责签名包、固定 active/pending、本地激活与回退，返回已核验的 UI/worker 路径及清单。`sodeck-plugin-host` 负责 QuickJS 独立子进程，通过私有 stdin/stdout JSON 帧；宿主内部命令模式必须先于 Tauri 初始化执行。
- 插件 worker 暴露初始化/业务命令/停止入口；终端 view 与 worker 的同插件交互由公开 `plugin.invoke` 路由，本阶段不允许指定其他 pluginId。业务命令包括新建/关闭会话和字号处理，远端资源调用仍经核心公开 API并留痕。
- 工作台布局、原生窗口、挂载关系由桌面核心维护；前端负责渲染和反馈。具体方法/类型在相应模块落地前由实现者补充本文件或所属规格，并通知其他实现者，避免两端各自猜字段。

验证入口：各模块先执行本批契约/单元测试，再由桌面集成执行真实 SSH、原生迁移和实际终端更新。验证结果集中记录于 T-003。

依赖落地核查：首次 npm 审计指出初始 Vite 7.2、Vitest 4.0 与 esbuild 0.27 的已知问题，实施内改用已修复的 Vite 7.3.7、Vitest 4.1.12、esbuild 0.28.2 并重新锁定；不改变 React/TypeScript 架构与产品范围。

## 核心方法字段（r1 内部联调）

- `servers.create` 接受 `{name,host,port?,username,authType}`，返回 Server；`update` 另需 `serverId,expectedRevision`，`delete` 另需 `serverId,stopSessions?`。查询返回 `{servers}` 或 Server，Server 含 `id,revision,hasSecret`，不含秘密。
- 核心认证交互 `auth.set` 接受 `{serverId,password?,privateKeyPath?,passphrase?,remember?}`，私钥只由核心读取；`hosts.trust` 接受握手返回的 `{serverId,fingerprint,algorithm}`。插件业务只接受引用与认证状态；秘密输入由外壳交互承载，不传给插件 worker/UI。
- `connections.acquire` 接受 `{serverId,workspaceId,configRevision?,operationId?}`，返回 `{leaseId,connectionId,generation}`。握手发现未信任/改变的主机密钥时返回 `HOST_KEY_REQUIRED/HOST_KEY_CHANGED`，details 包含 host、port、algorithm、fingerprint、previousFingerprint；凭据缺失为 `AUTH_REQUIRED`。用户处理后显式重试，不自动发送凭据。
- `pty.open` 接受 `{leaseId,cols?,rows?,operationId?}`，返回 `{ptyId,streamId,connectionId,generation}`。本批同步核心入口运行在桌面后台执行器；网络阶段不阻塞 UI 线程。`pty.write` 使用 `{ptyId,generation,inputSeq,bytes}`，bytes 为 base64；返回 acceptedBytes、inputSeq。`resize` 使用 cols/rows；`close` 使用 ptyId/generation。
- `operations.cancel({operationId})` 返回 `{canceled:true}`，仅取消同一调用者实例内的操作。acquire 与 pty.open 可贯穿同一个 UUID；取消不停止已复用的连接。核心保留 10 分钟/最多 10000 项提前取消标记、不淘汰活跃项，握手、gate 等待及 PTY 创建检查取消，迟到 PTY 回收；取消错误保持 `CANCELED`，不会变成认证失败。工作台层的 `workbench.openTerminal({serverId,operationId?})`、`workbench.cancelOpen({operationId})` 与 `snapshot.pendingOpens` 对应签名见[前端契约](terminal-ui-contract.md)。取消为清理调用，缓存或记录故障时仍能执行。
- `pty.read` 接受 `{ptyId,generation,after?}`，返回 `{frames:[{sequence,bytes}],state,reason?,lastSequence}`；单次最多 256 KiB，`pty.ack` 使用 `{ptyId,generation,sequence}`。确认只能前进到已产生的帧，超过已缓冲历史返回 STREAM_CURSOR_EXPIRED，调用方不得静默掩盖输出缺口。`pty.bind` 由宿主迁移提交时绑定 viewId/mountGeneration，后续输入须与该绑定匹配。
- 迁移快照通过 `view.snapshotChunk({operationId,index,total,data})` 顺序发送，每块最多 256 KiB、总快照最多 4 MiB；宿主核对操作与原生来源、块序号和总长度，完整解析后才接受。prepare 通知先冻结页面新输入，等待已接受输入与输出确认完成，收到最终快照后冻结源原生绑定；目标 ready 后先提交 PTY 新代次再启用目标。超时/拒绝恢复源，候选挂载不读取或确认源输出。
- `workbench.recover` 只重建失败表面并前进挂载代次，不新建 SSH；屏幕已丢失时明确显示。插件发起新终端遇到主机/认证需求，由核心向所属外壳发送 `auth.required`，秘密输入留在外壳。
- 去重缓存用于可能产生副作用的控制请求；纯查询和按游标读取/确认不保存输出副本，重复读取仍由游标和绑定校验保证安全。所有查询仍逐次留痕。缓存只保存请求摘要，不保存密码、快照或输入正文；最多 10000 个控制结果、10 分钟，以免空闲输出轮询耗尽控制请求容量。
- 跨窗口拖放由原生获取屏幕物理坐标，命中可见主窗口时转换为其 CSS 坐标并选择已测量的中央/底部落点；仍归原服务器工作区。窗口外释放则弹出或移动单视图窗口，首批最多两个；不向已经占用的单视图独立窗口追加第二套工作台。屏幕坐标不由页面自报位置直接决定原生归属。
- 前端补齐独立窗口核心 Tab 条作为拖回入口，释放在源页面之外调用上述原生落点分支。底部页切换以核心 active 为准，Tab 支持方向键/Home/End。2026-10-25 工作台浏览器回归 5 项通过（原 3 项加底部权威激活及独立句柄/Esc/落点请求），desktop 类型检查通过；这些新增仅影响 shell 资产，不影响终端包。原生构建需重新包含 shell 并验证 detached Tab 拖回；浏览器测试不能证明原生窗口命中。Workbench 后续已接清单目录与活动页归一，公开签名及10项Rust验证见 [UI记录](../tasks/T-003-ui-results.md)；会话列表重命名及完整11项浏览器契约回归同见该记录。
- `resources.list` 返回 `{connections,ptys,leases}`；`resources.stop` 使用 `{resourceId}` 并显式影响全部使用者。`settings.get/set` 使用 `{key,value?}`，默认 namespace 来自宿主绑定 pluginId，外壳为 shell；`calls.list` 支持 pluginId/method/serverId/status/before/limit，pluginId 是筛选条件，对各插件一致开放。记录存储写入失败时，资源清理与 `diagnostics.get` 仍可执行；诊断返回有界内存故障记录并明确记录缺口，其他新调用返回 LOG_UNAVAILABLE，不能在没有开始记录时执行副作用。

核心补充测试入口使用 `tests/core/ssh_fixture.py`：仅监听 loopback，执行真实 SSH 握手、密码/密钥认证、PTY 通道和有界字节回送，记录脱敏认证/关闭事件。它验证协议、主机先验校验、多通道隔离、代次及释放；回送器不是 Linux shell，因此不能替代本批 Linux/OpenSSH、原生终端及 IME 流程。Python/Paramiko 是测试依赖，不进入产品运行时。

2026-10-25 密钥兼容实测：v1 WinCNG 后端通过密码与 RSA PEM，但 RSA OpenSSH 格式失败；源码核实 RSA loader 仅支持 PEM。为满足既定 OpenSSH 私钥范围，Windows SSH 适配改用同一 ssh2/libssh2 的 OpenSSL 后端，私钥只在内存解密/认证，持久化仍为 DPAPI。OpenSSL/Perl 属于构建依赖，产品不依赖外装解释器。此调整保持 API、连接 worker 与工作台边界；实际构建及密钥结果另记，不把 WinCNG 原有结果套到新后端。

## 插件平台落地接口（r1 内实现细化）

- crate 路径 `crates/plugin-manager`、`crates/plugin-host`。管理器 `PluginManager::open(root, trusted_keys)`；公钥为 keyId → Ed25519 32 字节公钥。`prepare_package(path)` 只更新 pending；`activate_pending(check)` 在启动时验证后提交 active，check 接收已验证包和私有数据副本目录。`active(plugin_id)` 返回 `InstalledPlugin`（manifest、root、ui_path、worker_path、data_dir）；`status()` 返回运行/准备版本及失败原因。管理器生命周期持有数据锁。
- 包为 ZIP：`manifest.json`、`integrity.json`、`signature.json` 及 UI/worker 内容。integrity 是文件相对路径 → SHA-256 的 JSON 对象，包含 manifest、排除自身及 signature；签名签署 integrity 的原始 UTF-8 字节。signature 是 `{keyId,signature}`，signature 使用 base64。清单名称采用 `pluginId/version/requires/entry/contributes/dataSchemaVersion`。打包脚本 `scripts/plugin-pack.mjs --source <构建目录> --out <包路径> --key <PKCS8 PEM私钥> --key-id <ID>`。
- 宿主 `run_internal_host_if_requested() -> Option<i32>` 必须最先调用。父进程 `PluginProcess::spawn(executable, worker_path, core_callback)` 后 `initialize(context)`（prepare、activate 均限本地无副作用）、`invoke(command, params)`、`stop()`。core_callback 是已绑定真实来源的 `Arc<dyn Fn(&str, Value) -> Result<Value, RpcError> + Send + Sync>`。
- worker 定义 `globalThis.sodeckPlugin = { prepare(context), activate(context), invoke(command, params), stop() }`；`globalThis.sodeck.call(method,params)` 返回 Promise，所有调用回到父进程。没有 Node、本机文件 API 或跨插件通道。生命周期入口不可打开远端资源，父进程需统一检查阶段。业务入口始于初始化成功后，JS 连续执行期限为 5 秒。
- 父进程向 worker 的帧为 `{kind:'initialize',id,code,context}` / `{kind:'invoke',id,command,params}` / `{kind:'stop',id}`；worker 向父进程的核心请求是 `{kind:'core',id,method,params}`，结果帧统一 `{kind:'result',id,result}` 或 `{kind:'result',id,error}`。双向帧使用 4 字节大端长度前缀，限 1 MiB。
- 终端 worker 已在 `plugins/terminal/src/worker.ts` 定义，清单为 `plugins/terminal/manifest.json`。业务命令：`sessions.open`/`sessions.create` 输入 serverId/workspaceId，返回 sessionId/leaseId/connectionId/ptyId/generation/status/title；`sessions.get` 与 `sessions.bind` 输入 sessionId/viewId；`sessions.list` 按 workspaceId 筛选；`sessions.close`/`sessions.reconnect` 输入 sessionId；`settings.get/set/reset` 使用 fontSize。0.1.0 不支持 reset，0.1.1 包加入该操作。外部 `plugin.invoke` 的 args 由桌面转为上述 params。
- 终端字号由 worker 调用 `storage.get/set` 的 `fontSize` 键（get 返回值本身），桌面将其路由到已激活的数据代次目录并留痕；初始化候选读写的是副本。核心通用 `settings.get/set` 仍维护自己命名空间，不与插件激活数据混用。
- `npm run plugin:pack -- --plugin terminal --version 0.1.0` 实际入口为 `scripts/pack-plugin.mjs`，独立运行 Vite 和 esbuild 后签名；`--source` 可直接封装已有产物。开发公钥文件为 `resources/development-public-key.json`，字段 `{keyId,publicKey,developmentOnly:true}`，publicKey 是原始 32 字节公钥 base64；私钥仅在忽略目录 `.local/signing`。
- 子进程父端写入采用最多 32 帧且总计 4 MiB 的独立有界队列；请求期限从进入队列前计算。未开始发送的过期帧取消并返回 notSent；发送中/已发送超时返回结果待核对，不自动重发。停止流程不等待 stdin 锁，5 秒总期限后独立结束子进程；测试包含真实子进程和排队压力。
- `sessions.rename` 接受 `{sessionId,title}`，去除 title 两端空白后要求 1–80 个 Unicode 码点、拒绝 C0/C1 控制字符，返回更新后的同一逻辑会话；只改变展示标题，不重连、不调用远端、不更换 PTY/lease/view 标识。与同一会话的关闭/重连串行，处理中的会话返回 BUSY。工作台标题由宿主在该调用成功后同步。
- 初始化或资源加载失败时，宿主立即结束候选进程并撤销其核心实例；独立窗口重命名保留服务器名称。原生调用拒绝的回复也经过有界主线程队列，不为每条过载请求另建线程；队列已满时由 SDK 显示超时并允许查询实际状态，不自动重发。
- 同一 App 的主窗口和独立窗口使用同一隔离 shell profile，插件表面另用 terminal profile；调试端口仅 debug 显式环境变量启用。未加载插件时使用空视图声明，保留布局并展示不可用状态。开发目录包包含执行文件、独立终端包、公钥和启动脚本，数据放其独立 data 目录，不作为正式安装包。
- 控制请求去重缓存满时继续允许关闭、停止、释放和退出；该分支直接执行幂等清理，不增加缓存。原生回归用无副作用的未知方法填满 10,000 项缓存，确认查询、关闭和正常退出仍可用，不能仅验证在途队列保留名额。

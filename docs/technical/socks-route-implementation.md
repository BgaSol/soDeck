# T-020：SOCKS5 SSH 路径代码修改设计

更新日期：2026-10-28 · 修订：r3 · 状态：已确认实施；结果见 T-020\
所属任务：[T-020](../tasks/T-020-socks-route.md) · 行为规格：[F-CORE-027 r4](../core/socks-route.md)\
设计起点：API 4.10.0 / 数据库 v5；本次落地：当前 `v2` 工作区、API 5.0.0 / 数据库 v6，详见[实施结果](../tasks/T-020-results.md)

本文修正上一版设计。用户明确区分了两类能力：SSH 跳板是**主机隧道代理**，需要逐台 SSH 登录主机；SOCKS5 是**网络代理**，只提供 TCP 网络通道，不登录代理主机。上一版的通用混合节点图、跨类型依赖闭包和 `RouteCarrier` 方案因此不采用。

本轮也不保留旧核心/旧 SDK/旧路由契约兼容。采用一次性替换：所有官方插件、外壳、SDK 和核心一起升级，数据库按 P-52 版本不一致规则备份后重建空库。

## 1. 新的代码模型

目标服务器只有一个明确的 `accessPath`，三种方式互斥：

```text
Direct
HostTunnel { jumpServerIds: Vec<ServerId> }
NetworkProxy { proxyId: ProxyId, resolve: ProxyResolve }
```

### HostTunnel：主机隧道代理

`jumpServerIds` 是完整的有序 SSH 主机 ID 列表，只读取这些主机的端点和 SSH 认证信息，不递归解析其自身 `accessPath`。核心对每台跳板建立 SSH session、核对主机密钥、完成该主机认证，再通过 `direct-tcpip` 打开下一台主机或最终目标。跳板不创建 PTY 或 shell，但它确实是已认证的 SSH 主机连接。每台跳板使用自己的 SSH 用户名、密码/私钥和主机信任。此处保留多级能力，替换当前 `route_store::resolve` 递归跟随单个 `jumpServerId` 的配置语义。

### NetworkProxy：SOCKS5 网络代理

`proxyId` 指向独立的 SOCKS5 服务配置。核心只连接代理端点，完成 SOCKS5 方法协商、可选用户名密码认证和一次 CONNECT，然后把同一 TCP 流交给最终目标的 SSH session。代理不具有 `Server` 类型、SSH 字段、主机指纹、工作区或插件视图。

本次建议不实现 SOCKS 链、SSH 跳板与 SOCKS 混合、系统代理/PAC、HTTP CONNECT、SOCKS4、UDP/BIND 或自动切换线路。互斥是本期范围取舍，不是协议不能组合。未来要组合两种代理时，重新设计访问路径，不在本次 API 中留下可执行但未定义的组合状态。

## 2. 模块和文件修改

### 2.1 核心 Rust

| 文件/模块 | 修改 |
| --- | --- |
| `crates/core/src/route.rs` | 用新的 `AccessPath`、`HostTunnelPath`、`NetworkProxyPath`、`ProxyResolve` 替换旧 `ConnectionRoute`。校验互斥模式、跳板 ID 唯一、跳板数量/字段有效和目标模式一致；不再解析旧 `sshJump`/v1/v2 枚举。 |
| `crates/core/src/route_store.rs` | 重写路径保存、预览、依赖和删除校验。主机隧道只遍历 `jumpServerIds`；网络代理只查一个 `proxyId`。事务内拒绝自身/重复/缺失跳板和缺失代理，不建立服务器与代理的通用有向图。 |
| 新增 `crates/core/src/socks5.rs` | 独立 SOCKS5 状态机：Greeting、方法选择、用户名密码子协商、CONNECT 请求/回复、域名/IP 编码、短读/分片、4 KiB 帧缓冲、REP 错误映射。只接受 CONNECT，不处理 SSH 或 UI。严格按帧剩余长度读，不能预读并丢失后续 SSH banner。 |
| `crates/core/src/ssh.rs` | 从现有 `connect_for` 拆出 `connect_direct`、`connect_host_tunnel`、`connect_network_proxy` 三条路径；提取共用 SSH 握手/信任/认证函数，避免复制目标登录逻辑。主机隧道保留逐跳 session/bridge；网络代理先取得一条 `TcpStream`，再交给最终 SSH session。禁止路径间静默回退。 |
| `crates/core/src/ssh/transport.rs` | 保留 SSH `direct-tcpip` bridge 和 256 KiB 双向背压；增加 `socks5_connect(stream, target, resolve, ...)` 的有界适配，不抽象成跨类型 `RouteCarrier`。SOCKS 直连目标不需要额外 loopback bridge。 |
| `crates/core/src/route_services.rs` | 重写为新的单一 `servers.routes.*`、`connections.route.*` 和 `proxies.socks5.*` 服务；返回路径模式、目标、SSH 跳板摘要或网络代理摘要。旧 v1/v2 方法和投影删除。 |
| `crates/core/src/lib.rs` | 统一注册 API 5 的路径、代理 CRUD、连接状态和挑战方法。认证挑战明确 `ownerKind: sshHost | socks5 | targetSsh`，不允许把 SOCKS 挑战提交到 SSH 认证入口。 |
| `crates/core/src/secrets.rs` | 增加 `SecretOwner::{Server, SocksProxy}`，服务器秘密继续使用 server namespace，代理秘密使用 `secrets/proxies/<uuid>.dpapi`。临时密码和私钥继续 zeroize。 |
| `crates/core/src/store_bootstrap.rs`、`store_bootstrap/*` | 支持新 v6 空库及包含代理秘密子目录的完整备份/核验/恢复，移除旧库迁移分支；结构版本不一致继续执行 P-52 的提示与备份后重建。 |
| `crates/core/src/network/route_pool.rs` | 网络池仍只申请目标 server lease；由核心按目标的 `accessPath` 建立连接。主机隧道和 SOCKS 的资源统计分开，不把 SOCKS 计为 SSH session。 |
| `crates/core/src/traffic/*` | 记录 `accessMode: direct | hostTunnel | networkProxy`。SOCKS 协商/CONNECT 是外层协议开销，目标业务字节只归目标连接；主机隧道 bridge 按现有段指标记录。 |

### 2.2 契约与 SDK：一次性替换

删除 `connection-route-v1.schema.json`、`connection-route-v2.schema.json`、`route_v2.rs` 及旧生成投影，新增单一 `contracts/connection-route.schema.json`。核心 API 升到 **5.0.0**，所有官方插件和外壳同时重建；删除现有 v1/v2 路由兼容分支，不实施上一版提出的 v3 兼容方案，也不保留 `ROUTE_VERSION_REQUIRED` 或旧字段静默降级。旧插件通过 manifest/API 主版本检查明确判定为不支持，不启动后再错误解释字段。

建议契约形状：

```text
AccessPath = { kind: "direct" }
           | { kind: "hostTunnel", jumpServerIds: string[] }
           | { kind: "networkProxy", proxyId: string, resolve: "proxy" | "local" }

SocksProxy = {
  id, name, host, port,
  authType: "none" | "usernamePassword",
  username?: string,
  revision, hasSecret
}
```

`Server` 返回 `accessPath`；`SocksProxy` 单独返回，不含密码。路径节点摘要必须带 `kind: sshHost | targetSsh | socks5`，但 SOCKS 节点只出现在状态/诊断摘要，不伪装为服务器节点。

`resolve` 只存在于目标的 `accessPath`，代理档案不重复保存该字段。代理自身域名由本机解析；目标域名在 proxy 模式以 DOMAIN 发送，本机不解析。local 模式在总超时内依次尝试解析 IP，每次新建 SOCKS 通道；IP 字面量直接编码为 IPv4/IPv6，不把本机解析失败自动切到 proxy 模式。

建议方法：`servers.routes.get/update`、`proxies.socks5.list/get/create/update/delete`、`connections.route.get`、`auth.challenge.get`、`auth.ssh.set`、`auth.socks5.set`。现有目标连接、PTY、SFTP/exec 和网络资源方法统一升级到 API 5 的新请求/响应，但仍只接受目标 serverId 和 lease，不接受插件自组路径。

`scripts/generate-connection-route.mjs` 改为生成单一 schema；`packages/sdk`、外壳和三个官方插件一起更新。生成文件由脚本产生，契约检查必须验证未知 `accessPath.kind`、SOCKS 字段混入服务器和密码字段出现在响应时失败。

### 2.3 SOCKS 协议交接

只提供配置指定的认证方法：无认证为 0x00，用户名密码为 0x02；拒绝服务端选择未提供的方法，验证子协商版本及状态。用户名/密码编码后均为 1～255 字节，域名按 ASCII/IDNA 编码后为 1～255 字节；超长、端口非法和无效地址在发包前拒绝。回复严格验证 VER、RSV、ATYP 和完整 BND.ADDR/BND.PORT；REP 仅映射标准可知原因，不把 host unreachable 推断成 DNS 失败。

CONNECT 回复和目标 SSH banner 可能连续到达。状态机每次只读取当前帧所需字节，读完回复立即归还同一 `TcpStream`，不使用可能持有预读字节的缓冲读器交给 `set_tcp_stream`。整个过程用现有可取消的非阻塞等待，不启动外部代理进程或监听端口。

## 3. 存储、凭据和资源

### 3.1 数据库

数据库结构版本从 5 提升到 **6**。按 P-52，版本不一致时备份 SQLite 主文件/WAL/SHM 与核心秘密，建立空 v6 库，不导入旧服务器或旧路径。

新库使用：

```sql
CREATE TABLE connection_paths(
  server_id TEXT PRIMARY KEY REFERENCES servers(id) ON DELETE CASCADE,
  data TEXT NOT NULL,
  revision INTEGER NOT NULL
);
CREATE TABLE socks_proxies(
  id TEXT PRIMARY KEY,
  data TEXT NOT NULL,
  revision INTEGER NOT NULL
);
CREATE TABLE connection_path_refs(
  server_id TEXT NOT NULL REFERENCES servers(id) ON DELETE CASCADE,
  ordinal INTEGER NOT NULL,
  ref_kind TEXT NOT NULL CHECK(ref_kind IN ('sshServer','socks5')),
  ref_id TEXT NOT NULL,
  PRIMARY KEY(server_id, ordinal)
);
CREATE INDEX connection_path_refs_target ON connection_path_refs(ref_kind, ref_id);
```

`connection_paths.data` 是唯一行为来源，`connection_path_refs` 是事务内重建的依赖索引。`hostTunnel` 只能写 `sshServer` 引用，`networkProxy` 只能写一条 `socks5` 引用；保存、删除和并发修订在同一事务内再次验证。旧 `server_routes`、旧 `jump_server_id` 和旧路由 JSON 不进入新库。

### 3.2 认证和秘密

SSH 主机隧道：每个 `jumpServerId` 以及最终目标都有 SSH 主机指纹挑战和 SSH 凭据挑战。SOCKS 网络代理：只有配置为用户名密码时才有代理凭据挑战，没有主机指纹挑战。目标 SSH 的挑战始终独立出现。

挑战详情至少包含 `ownerKind`、`ownerId`、`targetServerId`、`hopIndex`（仅 SSH 主机隧道）、`operationId`、工作区/窗口代次、`accessPathRevision` 和 5 分钟有效期。迟到、取消或路径变更后的答复没有副作用。

区分三种标识：

- **代理秘密上下文**：代理 ID、端点、认证模式、用户名；不包含目标 ID 或 `resolve`，从而允许多目标复用同一代理账号。端点或账号变化后旧秘密不自动复用。
- **目标信任/SSH 凭据上下文**：目标端点/账号及有效网络路径身份。SOCKS 路径包括代理端点/账号和 `resolve`；SSH 路径包括实际有序主机身份及已核验上游指纹。名称、配置计数与密码轮换不改变主机身份，相关网络上下文变化必须重新确认。
- **运行修订/池复用键**：目标与有效路径、各相关配置和凭据修订，以及现有 purpose/工作区隔离维度；不包含未执行的跳板自身 `accessPath`。不同修订不得复用连接。已建立连接可继续服务原持有者；相关配置变化后待认证尝试和挑战失效，新申请使用最新快照。

### 3.3 资源预算

- 直接：1 个目标 SSH session。
- 主机隧道：H 个跳板需要 H+1 个 SSH session、H 个 bridge，沿用现有 256 session/256 bridge、256 KiB 每方向桥缓冲和多级取消回收；保存/申请时校验预算。
- 网络代理：1 条到 SOCKS 的 TCP 通道承载 1 个目标 SSH session，没有代理 SSH session 或 bridge，也不另外拨号目标。每个实际连接分别计数，独立网络池保持隔离。

SOCKS 协商（含协议认证）和 CONNECT 各 15 秒，目标 SSH 握手/协议认证各 30 秒，整体受连接总截止时间约束；等待人工凭据/指纹答复使用独立 5 分钟挑战有效期，不能被误算成 15/30 秒网络阶段。取消在 1 秒内撤销等待，本地资源 5 秒内回收。代理服务属于外部资源，退出 App 不停止它。

## 4. 外壳、认证 UI 与插件

- `server-dialog.tsx` 将“连接方式”改成三项互斥选择。SSH 主机隧道使用有序主机选择器；SOCKS 使用独立“网络代理”选择器，显示说明“无需登录代理主机”。
- 新增 `proxy-dialog.tsx`/代理管理视图，代理不出现在服务器选择器和工作区列表；父表单子步骤沿用草稿、焦点和部分成功回执。
- `route-path.tsx` 按 `主机隧道`、`网络代理`、`目标 SSH` 展示不同节点。网络代理行没有 SSH 用户名、指纹或工作区入口。
- `auth-dialog.tsx` 拆为 SSH 主机认证和 SOCKS 代理认证两个明确分支；SOCKS 分支只显示网络代理账号字段，目标 SSH 仍回到目标认证。
- `workbench_services.rs` 的认证事件使用 `ownerKind` 联合载荷。终端、文件和网络插件只改 API 5 SDK 调用，不增加代理逻辑。

已有网络隧道插件提供 HTTP/SOCKS 服务供其他客户端使用；本次核心新增的是连接外部 SOCKS5 的客户端能力，两者方向和生命周期不同。代理管理放在核心设置/目标表单，不要求安装或启用该插件。

UI 复用现有公共组件和三主题，使用 `apple-design` 的渐进展开、即时反馈、稳定焦点、减少动态和高对比规则；不增加混合路径编辑器或装饰动效。

## 5. 实施顺序与首个关口

0. **先验证传输交接**：确认方案后，先以隔离夹具验证“TCP → SOCKS5 CONNECT → `ssh2::Session::set_tcp_stream` → 目标 SSH”。验证无认证/用户名密码、分片与紧随回复的 SSH banner、非阻塞握手、取消、超时、失败清理和错误阶段；不先做全量契约替换。通过后将这部分实现纳入正式模块。
1. **替换契约和模型**：删除旧 route schema/生成文件，建立 API 5 单一 `accessPath` 和代理 DTO。
2. **数据库 v6 与存储**：建立新表、代理 CRUD、依赖索引、代理秘密 namespace；先完成事务和空库重建验证。
3. **SOCKS 协议**：实现并单测 SOCKS5 状态机、短读、认证、CONNECT、DNS 模式和取消。
4. **网络连接路径**：将已验证的 SOCKS 通道接入连接管理器、快照/池、认证挑战和计量；不引入外部 ssh/ProxyCommand/远端 helper。
5. **SSH 主机隧道回归**：把现有多级 SSH 接入新 `accessPath.hostTunnel`，确认逐节点认证和 bridge 资源不受网络代理改动影响。
6. **外壳与认证 UI**：代理管理、三项访问方式、状态、挑战和错误路径。
7. **插件/观测/开发包**：三个官方插件统一升级 API 5，补齐计量和 Windows 独立包验证。

首个停止关口是第 0 步：如果现有 ssh2 非阻塞握手无法直接承载 SOCKS CONNECT 后的 `TcpStream`，不能用 loopback 对外开放、外部命令或远端脚本掩盖问题；先回到方案修订。当前 `set_tcp_stream` 接受已建立 socket，因此采用这一方向；实施已通过真实 SOCKS5 与目标 SSH 交接，证据见[实施结果](../tasks/T-020-results.md)。

## 6. 测试入口和风险

| 层级 | 建议入口 | 证明内容 |
| --- | --- | --- |
| SOCKS 单元 | `crates/core/src/socks5.rs` tests | 状态机、方法、用户名密码、域名/IP、REP、边界和取消 |
| 路径/存储 | `crates/core/tests/access_path.rs` | 三种互斥模式、SSH 跳板顺序、代理引用、事务依赖、v6 空库 |
| 契约/SDK | `packages/sdk/src/connection-route.test.ts` | API 5 单一契约、挑战 ownerKind、秘密字段隔离 |
| 真实网络 | `tests/core/socks_route_fixture.py`、`tests/core/ssh_jump_fixture.py` | SOCKS 无 SSH 登录、目标 SSH、SSH 主机隧道对照、DNS 和日志 |
| 桌面/原生 | `tests/desktop/socks-route*.mjs`、T-020 Windows 包 | 三项访问方式、认证/取消、窗口/焦点/主题/缩放 |

不再编写旧 API/schema/数据库导入的兼容成功测试；验证新统一实现、fresh v6、旧插件/API 明确拒绝以及版本不一致时的备份核验/中断恢复。纯 SSH 多级连接、逐台认证及资源回收仍必须通过；旧递归配置用例按显式列表重新定义预期，不能将旧配置语义误当作兼容要求。

## 7. 修订记录

- r3，2026-10-28：核实当前递归 route 与 ssh2 socket 接口，补齐显式跳板语义、DNS 唯一归属、认证/复用键、协议精确读取及备份子目录；传输交接关口前置到全量重构之前。
- r2，2026-10-28：根据用户澄清重写。明确主机隧道与网络代理边界，取消混合路径、旧兼容层和通用 `RouteCarrier`，改为 API 5 单一模型。
- r1，2026-10-28：曾提出 v3 兼容契约与混合路径，现由 r2 替代。

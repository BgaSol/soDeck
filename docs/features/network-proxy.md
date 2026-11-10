# 网络隧道

网络隧道插件在当前服务器工作区管理 TCP 转发和 SOCKS5 代理，默认右侧收起。它复用核心现有认证会话和完整跳板链，与服务器管理中用于建链的 SOCKS5 资料是不同用途。

## 规则与运行

- TCP 转发：本机监听 → 当前服务器 → 固定目标。
- SOCKS5：本机提供无认证的 TCP CONNECT，支持 IPv4、IPv6 和域名；目标域名由最终远端服务器解析，客户端需选择代理解析，例如 `socks5h://`。
- 默认监听 `127.0.0.1`，端口自动分配，也可指定本机 IP 和固定端口。`0.0.0.0` 或 `::` 监听对应地址族全部接口；端口冲突明确报错。
- 规则按服务器保存，支持新建、编辑、删除、启动、停止、复制实际入口和检查连通性。保存只写配置，不修改已经运行的监听。应用新配置需分别停止、确认清理完成、再启动；没有保存并重启的组合动作。
- 启动中、运行中、停止中和状态待确认的规则不能删除；确认清理完成后才可删除，先落盘成功再移除运行记录，写入失败保留规则。收起、切换和移窗保留原端口，断线、工作区关闭、插件或应用退出释放端口；重连和重开后手动启动。

规则文件 `network-rules.json` 只保存配置与服务器 ID，不保存 SSH 凭据或运行状态。当前最多 256 条、512 KiB，原子保存并检查修订；损坏或不支持的 schema 保留原文件。

## 状态、检查与边界

显示实际绑定入口、连接数、约三秒速率和本次运行累计有效载荷。查询失败显示“状态待确认”，保留原监听、最后观测时间和累计量，速率标为过期；禁用启动、删除和检查，允许停止原启动或继续查询。停止回执只说明接受取消，必须等核心确认监听、流、转发和探测任务全部退出后才能重新启动或删除。客户端连接错误与整条规则监听失败分开；普通连接错误写日志，重要中断按服务器去重，不因日志级别自动弹消息。

连通性检查经过实际本地入口及 SOCKS5 协商，只证明 TCP 可达，不保证上层服务、认证或业务健康。刷新保留规则节点、输入、焦点和展开状态，不打断按压中的操作。

不提供 HTTP 代理、UDP、反向转发、自动重连或系统代理接管。插件配置规则，核心提供通用监听、TCP 通道、有界协商、字节传输及归属清理；不把 SOCKS5 业务规则放进核心权限系统。

## 实现与回归入口

- 插件：[manager.rs](../../plugins/network-proxy/src/manager.rs)、[rules.rs](../../plugins/network-proxy/src/rules.rs)、[socks5.rs](../../plugins/network-proxy/src/socks5.rs)、[checks.rs](../../plugins/network-proxy/src/checks.rs)、[view.js](../../plugins/network-proxy/ui/view.js)。
- 核心：[network](../../src-tauri/src/network/mod.rs)、[relay.rs](../../src-tauri/src/network/relay.rs)、[resource_owner.rs](../../src-tauri/src/resource_owner.rs)、[network_metrics.rs](../../src-tauri/src/network_metrics.rs)。
- 回归：[真实网络夹具](../../src-tauri/tests/fixtures/network_flow.rs)、[原生进程与延迟端口回收](../../scripts/test-network-process.mjs)、[网络 UI](../../scripts/test-network-ui.mjs)、[OpenSSH](../../scripts/test-openssh.mjs)、[netem 专项](../../scripts/test-network-netem.mjs)。实际字节、取消、端口回收和重连隔离需集成验证。
- 普通协议分片测试不等于丢包/延迟测试；netem 手动执行，限制和命令见[开发指南](../development.md)。接口见[插件 API](../plugin-api.md)，独立发布证据见[CHANGELOG](../../plugins/network-proxy/CHANGELOG.md)。

返回[文档索引](../README.md)。

# 网络隧道

独立插件 `sodeck.network-proxy`，需要核心能力 `net.tcp.v1` 和 `ui.dialog.v1`。在已连接服务器的工作台右侧打开“网络隧道”；侧栏默认收起，可通过宿主菜单移到左侧。

- TCP 转发：本机端口 → 当前服务器 → 固定目标。
- SOCKS5：无认证的 TCP CONNECT，支持 IPv4、IPv6、域名；域名由最终远端服务器解析。
- 按服务器保存规则，支持编辑、删除、立即启停、复制实际入口、连通性检查和本次运行流量。
- 默认 `127.0.0.1`、自动分配端口；可指定本机绑定 IP 和固定端口。`0.0.0.0` / `::` 会监听对应地址族的所有接口。
- 收起侧栏、切换和移动工作区保留原监听；关闭工作区、服务器断线、插件退出或应用退出后释放端口。应用重开和服务器重连后需要手动启动。

规则位于核心分配的插件数据目录中的 `network-rules.json`，只保存服务器 ID 与规则，不保存 SSH 凭据或运行状态。运行时复用核心会话和完整跳板链。HTTP、UDP、反向转发和系统代理接管不在本版本范围。

```sh
cargo test --manifest-path plugins/network-proxy/Cargo.toml --locked
node scripts/package-plugin.mjs --plugin network-proxy
npm run network:test
```

`network:test` 需要本机 OpenSSH 服务端和 Playwright Chromium。测试启动临时回环 SSH 服务和实际外置插件，不读取用户服务器凭据。插件包在 `artifacts/plugins/`；五平台打包复用 `Package plugin`，标签 `plugin-network-proxy-v0.1.0`。核心安装包不包含此插件。

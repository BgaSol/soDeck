# NetBird 客户端首版实现

2026-10-18。对应已确认的功能、UI 和代码设计；入口为服务器右侧「监控 / Docker / NetBird」。

## 已接入的功能

- 概览：本机 IPv4 / IPv6、CLI / daemon 版本、Profile、Management / Signal / Relay / DNS、最后成功更新时间。
- 可见节点：概览预览、名称 / FQDN / IP 搜索、状态与直连 / 中继筛选、虚拟列表、详情、握手 / 延迟 / 累计收发量。Idle、中继和缺失测量分别展示，不把未知值转成 0。
- 诊断：按需读取原始状态、最近 200 行 systemd 日志、复制不含名称 / IP / 服务地址的摘要、自定义 CLI 路径；离开诊断页清理原文。
- 单次检测：由当前服务器向当前快照中的节点发起 Ping 或一个 TCP 端口检测。结果独立于节点隧道状态，不接受任意主机或命令。
- 操作：连接、断开、启动 / 重启服务。先预检、确认，提交后再次检查，再核对目标状态。重复 request ID 返回同一回执，回执丢失不自动重放。
- 工作区：页面、搜索、筛选、选中节点、滚动位置随会话移动。切换到其他页或隐藏工作区时暂停常规采集；已提交操作独立完成。

## 适用边界

首版通过已验证的 SSH 连接管理 **Linux 原生客户端的默认 daemon**，固定 socket 为 `unix:///var/run/netbird.sock`。不需要 Management API Token。不支持容器客户端、多实例 / Profile 选择、自动安装、自动登录、配置网络或诊断包。

查询清除 NetBird 环境变量并显式指定目标。调用前检查默认 socket；若存在实例目录，写操作保持只读。版本缺少 `daemonStatus` / 客户端身份时保留可读取的数据，但不开放无法核对的操作。

| 功能 | 额外条件 |
| --- | --- |
| 断开 | 结构化身份与状态可确认，当前已连接或连接中；SSH 路径为 independent |
| 连接 | 已断开、默认 Profile、已确认的 root 原生服务；CLI 支持 `--no-browser`，远端有 GNU timeout。最多运行 20 秒；需要登录时回到终端处理 |
| 启动服务 | 本次应用运行中已经确认过服务与客户端的对应关系，当前单元停止，配置未改变；SSH 路径为 independent |
| 重启服务 | root、已确认的 `netbird.service`、MainPID 与默认 socket 的监听进程一致，且存在 InvocationID；核对时必须同时有命令回执和新的 InvocationID |
| Ping / TCP | 远端具备 timeout 和 ping / nc；使用固定单次请求和超时参数 |
| 日志 | 可运行 journalctl，并有 netbird.service 日志读取权限 |

服务对应关系使用 systemd 有效的 ExecStart、环境与单元路径做指纹，不把运行时的 PID / 启停时间混入绑定指纹。服务有自定义 Environment / EnvironmentFiles 或非 root User 时不开放服务操作；不自动 sudo。首次打开时服务已经停止、缺少历史身份的情况只提供诊断，需先在终端启动。

SSH 路径检查使用实际 `SSH_CONNECTION`、本机 NetBird 地址、`ip -j address` 和返回路由。当前首版只在直接连接、配置为 IP 且与实际 SSH 目标对应时判定 independent。跳板、域名无法对应、缺少网络工具或证据冲突均判为 unknown；中断型操作被阻止。检测证明的是当次检查的路径，不能消除检查后外部网络变化的可能性。

## 代码组织

- `src/netbird/`：IPC 契约、外部 Store、Provider、概览 / 节点 / 详情 / 诊断 / 确认弹窗及独立样式。
- `src-tauri/src/netbird/`：独立 Manager、窗口订阅与 ack 背压、CLI 检测与有界执行、JSON 解析、路径评估、持久回执和操作核对。
- `ServicePanel`：第三个一级页签与循环键盘导航。
- `SessionWorkspace` / `moves`：UI 状态快照、会话移动时的订阅清理，以及取消移动后的订阅恢复。
- `State` / `connection` / `traffic`：退出收尾、会话关闭、服务器删除保护和独立流量归类。

快照按 SSH connection identity 共享。不同别名 / 路径指向同一 SSH 主机密钥与 daemon 的操作共用写锁。可见时约 15 秒轮询；后端支持 background 级别，当前 UI 在离开 NetBird 时选择 none。刷新合并，旧 epoch / revision 被丢弃，修改 CLI 路径后的旧查询不能覆盖新状态。

`netbird.sqlite` 与服务器数据库同目录，只存 CLI 路径和操作回执 / 幂等索引，不存状态快照、节点、原始日志、SSH 凭据。恢复时将未完成任务标为 uncertain，等待用户检查，不自动重放。命令有严格的末尾回执；只有 EOF 不代表成功。

## 验证

```sh
npm test
npm run build
npm run test:netbird
cargo test --manifest-path src-tauri/Cargo.toml --lib
cargo test --manifest-path src-tauri/Cargo.toml --lib netbird_client_operations_over_verified_ssh -- --ignored --nocapture
```

SSH 集成测试需要 `tests/requirements.txt` 中的 Python 依赖，默认使用 `.qa/venv/bin/python`，也可设置 `SSH_TEST_PYTHON`。它启动临时 loopback SSH 服务器，验证主机密钥与实际 SSH Channel；NetBird 命令由 `tests/netbird_fixture.py` 模拟，不执行宿主机网络命令。

覆盖：JSON 单位 / 缺字段 / IPv6 / 节点上限、命令引用、严格回执、三态路径判断、preview 窗口归属 / 过期、SQLite 恢复和幂等、订阅背压、四种操作核对、回执丢失、查询权限错误保留旧数据、服务监听进程不匹配。Chromium / WebKit 覆盖页面往返、筛选、虚拟列表滚动、诊断清理、危险路径拦截、提交回复丢失、状态核对、三页签键盘操作、320px 深色 / 高对比度 / 减少动态效果。

本次验证：前端单元测试 111 项通过；Rust 全量测试 134 项通过、38 项按环境要求跳过，新增 NetBird 9 项单元测试再次单独通过；18 项 NetBird 浏览器测试和 6 项既有监控 / Docker 布局回归通过；上述 SSH 模拟集成测试已显式运行并通过。前端生产构建通过。

JSON 样本按官方 `client/status/status.go` 字段和 Go duration 编码构造，数值为纳秒，展示时换算毫秒。测试使用模拟客户端，**尚未连接真实 NetBird 网络**；需要在独立的 Linux 测试节点上验证目标版本及实际 systemd / 路由环境。

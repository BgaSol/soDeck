# SoDeck 架构草案

## 分层

```text
桌面界面
  ├─ 核心工作台：连接列表、会话、插件管理、审计
  └─ 插件视图：终端、文件、Docker、NetBird

核心进程
  ├─ Plugin Manager       插件发现、启动、停止、版本和权限
  ├─ Connection Manager   SSH 连接、会话复用、重连和状态
  ├─ Session Services     exec、PTY、SFTP、流式输出
  ├─ Network Services     local forward、remote forward、SOCKS5
  ├─ Secret Store         操作系统凭据存储和 SSH agent
  ├─ Policy & Audit       授权、审计和可观测性
  └─ RPC Gateway          面向插件的版本化能力接口

插件进程
  ├─ terminal
  ├─ files
  ├─ docker
  └─ netbird
```

## 插件模型

每个插件包含清单、独立进程和可选的界面资源。核心只根据清单启动插件，并通过 JSON-RPC 2.0（或兼容的消息协议）提供能力。

清单至少包含：

```json
{
  "id": "sodeck.files",
  "version": "0.1.0",
  "apiVersion": "1",
  "entry": "sodeck-files",
  "minCoreVersion": "0.1.0",
  "permissions": ["connection.sftp", "secrets.session"],
  "contributes": ["panel.files"]
}
```

核心能力按范围发放，而不是把底层对象直接暴露给插件。第一版能力包括：

- `connection.exec`：执行非交互命令并返回流式输出。
- `connection.pty`：创建和管理交互式终端。
- `connection.sftp`：执行范围明确的文件操作。
- `network.forward`：创建和关闭端口转发。
- `network.socks`：创建和关闭 SOCKS5 代理。
- `audit.record`：记录插件执行的用户可见操作。
- `secrets.session`：使用已授权会话，不返回原始凭据。

## 网络能力边界

核心网络服务负责连接生命周期、绑定地址、转发流量、停止资源和审计。插件只请求能力，不自行创建绕过策略的隧道。

默认策略：

- 本地监听只允许回环地址。
- 每个通道显示目标服务器、监听地址和开始时间。
- 关闭服务器连接时自动关闭依赖它的通道。
- 代理和转发失败必须返回可理解的原因，并释放端口和进程资源。
- 外部监听、反向转发和长期运行的通道需要单独权限和明确的审计记录。

## 数据和稳定性

- 核心数据库只保存连接元数据、插件状态、策略和审计索引；插件数据使用独立命名空间。
- 私钥、密码和令牌进入 Windows Credential Manager 或 SSH agent，不进入普通配置文件和日志。
- 插件崩溃只影响该插件。核心负责超时、退出码、重启次数和用户可见错误。
- RPC、插件清单和核心能力都必须带版本号，兼容性检查在插件启动前完成。

## 第一条实现路线

1. 建立核心、插件 SDK 和测试插件，先验证安装、启用、停用、卸载。
2. 加入一个假的 SSH 连接服务，验证 `exec`、PTY、SFTP 和网络能力的接口，不先绑定真实 UI。
3. 接入真实 SSH，实现终端插件和文件插件。
4. 实现本地端口转发和 SOCKS5，并补上资源清理和审计测试。
5. 用同一套插件接口实现 Docker 和 NetBird。

# SoDeck

SoDeck 是面向运维人员的远程服务器工具。它提供统一的连接、终端、文件和网络能力，并通过插件扩展 Docker、NetBird 等具体运维功能。

当前项目处于产品和架构收敛阶段。第一版目标、插件边界和核心能力见：

- [`docs/product.md`](docs/product.md)
- [`docs/architecture.md`](docs/architecture.md)

## 当前默认方向

- Windows 优先的桌面应用
- Tauri + Rust 核心 + React/TypeScript 界面
- SSH 为第一种远程连接协议
- 插件独立进程运行，通过版本化 RPC 与核心通信
- 先完成终端、文件和本地 SOCKS 代理，再接入 Docker 与 NetBird

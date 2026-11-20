# Docker Windows 远端剩余验收

状态：2026-11-19，核心 1.8.1 / Docker 0.6.0 的源码、真实 Docker Desktop/Linux 流程与独立五平台发行验收完成；发行证据见[核心版本记录](../releases/v1.8.1.md)和[Docker CHANGELOG](../../plugins/docker/CHANGELOG.md)。正式行为、边界和契约已迁入 [Docker 功能](../features/docker.md)、[架构](../architecture.md)和[程序 API](../plugin-api.md#sessionprogramv1核心-181)。本文只保留尚未完成的真实远端要求，不代表这些组合已经通过。

## 尚未完成的真实远端范围

Windows Server Windows Engine 是原设计的完成条件，目前没有可用测试机器。已实现 Engine 系统识别、Windows 容器程序默认值、盘符路径与系统网络保护，但进程模拟不能代替真实 Engine；可以分步发行已经验过的 Desktop/Linux 行为，不能据此宣布整个远端矩阵完成。

取得可用环境后记录宿主系统/架构、CLI/Engine 版本、SSH 默认 shell 和容器镜像版本，并用独立命名/标签测试资源验证：

1. 当前账号自动 Context 与手动 npipe；CLI 缺失、管道拒绝、Engine 未启动、错误路径及不支持端点失败，旧配置保持。
2. 四分类、inspect、Docker Root、Windows 路径与缺失指标；默认/系统网络不可删除，自定义网络和无引用卷可删除，多标签镜像保持原生拒绝。
3. 容器日志历史/跟随、原始字节、搜索与导出；检查失败不进入正文，主程序相同协议前缀保持原文，缺少启动结果保持未知。
4. cmd.exe 及镜像内可用 PowerShell 的真实 PTY；特殊路径/参数、程序不存在、关闭来源后独立终端与实际退出。
5. 启停/重启/删除、公开镜像拉取、取消/断线/回执丢失、Engine 身份/系统切换与旧响应拒绝，确认未知操作不重发。
6. cmd 和 PowerShell 默认 SSH shell 的实际远端完整流程；本机进程编码回归不能代替后一组合。

ARM64 远端只在 Docker 原生发行物可用且真实测试后记为已验；五平台 SoDeck/插件原生客户端包不能推出远端 Docker 支持。rootless、更多 Engine 版本及生产环境按取得的具体环境补测，不从普通 Linux 夹具推出全矩阵通过。

资源树设备复核与日志工作台其他专项仍由[资源树计划](docker-expansion.md)和[日志计划](docker-log-viewer.md)维护，本主题不接管它们。

返回[文档索引](../README.md)。

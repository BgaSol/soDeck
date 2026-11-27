# 通用目录跟随剩余验收

状态：2026-11-27，核心 2.11.0／终端 0.12.0／文件 0.14.0／Docker 0.13.0 代码已实现，尚待本轮 CI、五平台包及原生远程组合验收和正式发布。

已实现行为、边界和协议分别维护于[文件](../features/files.md)、[终端](../features/terminal.md)、[Docker](../features/docker.md#compose-工作目录)、[插件 API](../plugin-api.md#通用目录位置)及[架构](../architecture.md)。设计历史随本主题 Git 提交保存，不另建完成计划档案。

本机已通过类型与格式、233 项脚本、终端输入／阅读／搜索／历史、三插件真实 OpenSSH／PTY／SFTP 通信、Docker 三种模拟远端组合的进程及界面回归。核心完整测试与最终变更复验继续进行；当前 Windows 环境没有 Docker CLI，真实隔离 Engine 的 Compose 标签目录协作交由本次 Linux CI 执行，不计为本机已通过。

剩余范围：

- [ ] 最终来源提交的核心／插件源码及相关业务 Check，包括真实隔离 Engine 的目录发布组合。
- [ ] 核心与三个改动插件分别完成五系统架构原生打包、实际包程序启动／退出及核心签名验证。
- [ ] 固定本次原始候选包，完成五平台原生 WebView 的终端／文件／网络远程组合；原生通用目录检查包含 Linux 自动 Shell。
- [ ] 核对各组件来源、包 run、Release 与验证范围，记录到核心版本说明／插件 CHANGELOG，正式发布原附件；插件不设 Latest。
- [ ] 发布证据和引用收束后删除本计划。

本功能不扩展容器内部目录、Compose 部署、PowerShell 自动 cwd 或跨主机路径映射。Compose 标签无法证明另一台客户端原目录身份。物理 IME／DPI／读屏、netem、正式版实际升级与全部 Docker 远端组合不计为本次验收；通用设备专项沿用[原生验收](native-verification.md)。

返回[文档索引](../README.md)。

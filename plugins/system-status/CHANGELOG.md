# 系统状态插件

## 0.2.0

- 重新组织右侧状态面板：紧凑服务器摘要、清晰的 CPU/内存读数、网络收发和挂载剩余空间，设备与采集详情按需展开。
- 网络同时展示接收和发送趋势；三个图表支持鼠标悬停与方向键读取历史，采样缺口和断线后的空白不补零。
- 容量达到 85% 时以颜色与“空间偏紧”提示；保留各挂载和数据组真实的最后成功时间。
- 适配宿主浅色、深色和多巴胺主题及 160–480px 侧栏，刷新/暂停入口固定可见。
- 修正刷新失败提示被导航操作清除、首读失败不显示原因的问题；大于 JavaScript 精确整数范围的容量保持整数运算并正确显示单位。
- 连接切换后清空旧读数，按新连接各组成功观测恢复；最近五分钟无有效网络样本时不显示零峰值。

需要核心 0.10.0 起的既有能力；本次不修改核心版本、采集协议、动态周期或后台实例生命周期，不新增生产依赖。

包来源提交：[`cfbf7c9fd7faa868c8b0a066a8c75e0e9bce7ca2`](https://github.com/BgaSol/soDeck/commit/cfbf7c9fd7faa868c8b0a066a8c75e0e9bce7ca2)。原始打包：[Package plugin #42472880934](https://github.com/BgaSol/soDeck/actions/runs/42472880934)。发行链接：[system-status 0.2.0](https://github.com/BgaSol/soDeck/releases/tag/plugin-system-status-v0.2.0)。

Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 五个原生归档完成 initialize、重复 mount、pause、dispose、shutdown 和包内 UI 资源验证；五份归档与更新清单的摘要一致。发布前另下载 GitHub Windows x86_64 归档核对大小、SHA-256，重复进程生命周期验证并核对四份生产 UI 源码。相同来源的 [Check #42472864180](https://github.com/BgaSol/soDeck/actions/runs/42472864180) 通过五平台插件源码、78 项脚本回归和 Linux x86_64 的真实隔离 OpenSSH 采集及包内界面。

本机 TypeScript/格式、正式前端构建、14 项插件源码回归、隔离 Windows OpenSSH 采集及实际包 UI 验证通过；界面覆盖宿主三主题、160–480px、双向历史与键盘、容量分页和大整数、错误保留、断线旧值、上下文切换无观测及过期/全缺口历史。独立复核评分两项修复已解决，设计规范已落盘。完整证据见[实施记录](../../docs/implementation-system-status.md)。

本次没有重复 Docker GNU/BusyBox 专项（源码容器专项保持忽略）、远端平台设备验收、核心包或完整宿主桌面套件；Check 的 desktop/netem 未选择。既有采集器平台限制继续保留，五平台客户端包通过不代表所有远端系统已验证。

## 0.1.0

- 新增默认右侧展开的只读系统状态面板，隐藏时后台继续采集。
- 设备规格、动态指标、拓扑及容量分别调度，支持暂停、刷新和五分钟趋势。
- 添加 Linux/OpenWrt、Windows、macOS、BSD 及通用 Unix 的分级采集路径；未知指标明确显示不可用。

需要核心 0.10.0 的 `session.exec.v1` 和 `ui.background.v1`，先升级核心再安装插件。核心安装包不包含插件。

包来源提交：[`fdbd2833631be83a6b45841c7535a356870456ec`](https://github.com/BgaSol/soDeck/commit/fdbd2833631be83a6b45841c7535a356870456ec)。原始打包：[Package plugin #42423046397](https://github.com/BgaSol/soDeck/actions/runs/42423046397)。正式发行：[system-status 0.1.0](https://github.com/BgaSol/soDeck/releases/tag/plugin-system-status-v0.1.0)。

Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 五个原生归档均完成包内程序 initialize、重复 mount、pause、dispose、shutdown 和 UI 资源验证；发布前另外下载 GitHub Windows x86_64 归档核对大小、SHA-256 并重复包内验证。同一来源提交的 [Check #42423017061](https://github.com/BgaSol/soDeck/actions/runs/42423017061) 全部通过。本机隔离 Windows OpenSSH 与 GNU sh/BusyBox 容器采集通过，完整源码和界面证据见[实施记录](../../docs/implementation-system-status.md)。

真实 OpenWrt/Dropbear、Windows Server/不同默认 SSH shell、macOS/BSD/其他 Unix 仍待设备验收；macOS/BSD 总内存百分比与部分磁盘 I/O 暂不可用，Windows 网络未映射物理网卡。五平台客户端包通过不代表所有远端系统已验证。

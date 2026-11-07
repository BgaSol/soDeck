# 系统状态插件

## 0.1.0

- 新增默认右侧展开的只读系统状态面板，隐藏时后台继续采集。
- 设备规格、动态指标、拓扑及容量分别调度，支持暂停、刷新和五分钟趋势。
- 添加 Linux/OpenWrt、Windows、macOS、BSD 及通用 Unix 的分级采集路径；未知指标明确显示不可用。

需要核心 0.10.0 的 `session.exec.v1` 和 `ui.background.v1`，先升级核心再安装插件。核心安装包不包含插件。

包来源提交：[`fdbd2833631be83a6b45841c7535a356870456ec`](https://github.com/BgaSol/soDeck/commit/fdbd2833631be83a6b45841c7535a356870456ec)。原始打包：[Package plugin #42423046397](https://github.com/BgaSol/soDeck/actions/runs/42423046397)。正式发行：[system-status 0.1.0](https://github.com/BgaSol/soDeck/releases/tag/plugin-system-status-v0.1.0)。

Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 五个原生归档均完成包内程序 initialize、重复 mount、pause、dispose、shutdown 和 UI 资源验证；发布前另外下载 GitHub Windows x86_64 归档核对大小、SHA-256 并重复包内验证。同一来源提交的 [Check #42423017061](https://github.com/BgaSol/soDeck/actions/runs/42423017061) 全部通过。本机隔离 Windows OpenSSH 与 GNU sh/BusyBox 容器采集通过，完整源码和界面证据见[实施记录](../../docs/implementation-system-status.md)。

真实 OpenWrt/Dropbear、Windows Server/不同默认 SSH shell、macOS/BSD/其他 Unix 仍待设备验收；macOS/BSD 总内存百分比与部分磁盘 I/O 暂不可用，Windows 网络未映射物理网卡。五平台客户端包通过不代表所有远端系统已验证。

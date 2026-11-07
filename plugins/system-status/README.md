# 系统状态

SoDeck 外置插件。默认在服务器工作区右侧展开，隐藏面板、切换服务器和移窗仍采集；关闭工作区释放采集任务。使用当前核心认证会话，不需要远端 Agent。

动态 CPU、内存和网络/磁盘计数默认每 5 秒读取，可在组件设置选择 3/5/10/30 秒；拓扑 30 秒、容量和运行时间 60 秒，设备信息在连接及刷新时读取。暂停只影响当前工作区，暂停时刷新执行一轮后保持暂停。趋势保留最近 5 分钟，未知值与采样缺口不补零。

```sh
npm run plugin:build -- --plugin system-status
npm run system-status:test
```

将产物目录从设置 → 插件 → 添加插件导入并重启。核心须提供 `session.exec.v1` 和 `ui.background.v1`，旧核心会明确提示升级。

采集器包含 Linux/OpenWrt procfs、Windows CIM、macOS 和 BSD 命令路径；其他 Unix 提供基础回退。Windows 网络速率以性能计数器实例显示，未映射到物理网卡；macOS/BSD 缺少可靠的可用内存口径时不显示总使用率。Linux 容量只查询明确识别的本地文件系统，NFS/CIFS 排除。实际验证范围见[实施记录](../../docs/implementation-system-status.md)，代码路径不代表对应系统已完成实测。

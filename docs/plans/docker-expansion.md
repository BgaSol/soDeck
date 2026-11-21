# Docker 单层资源树交付收尾

状态：2026-11-21，Docker 0.8.0 已实现，尚未发行；核心 1.11.0 与网络隧道 0.5.0 保持原版本。已实现行为与边界维护在 [Docker 功能](../features/docker.md)，版本与实际结果进入 [Docker CHANGELOG](../../plugins/docker/CHANGELOG.md#080)。

## 剩余验证与发布

- 对最终源码完成树、端口、日志与真实 Engine 回归，核对来源 Check。Windows OpenSSH／Docker Desktop 已走通信息、隧道、终端与维护，公开拉取遇到 Docker Hub TLS handshake timeout；直接 docker pull 复现相同外部故障，不能记为整套通过。
- 使用现有 Package plugin 对 Docker 独立构建五平台原生归档，并在对应原生 Runner 验证实际包的资源、初始化、生命周期和退出；成功后发布同一草稿附件。
- 核对匿名更新清单及五份归档的大小、摘要、标签与来源提交。仓库全局 Latest 按用户既有决定保留核心；Docker 为自身通道最新正式版。
- 将来源提交、原始打包 run、Release、实际验证与跳过范围写入 CHANGELOG 和永久交付证据，完成后删除本文。

## 专项归属

真实 IPv6／指定网卡／rootless 继续由[端口隧道验收](docker-port-tunnels.md)维护；Windows Server Windows Engine 沿用[Windows 远端验收](docker-windows.md)，日志长时／设备范围沿用[日志验收](docker-log-viewer.md)。这些范围不从普通浏览器、模拟进程或包启动结果推定通过。

实际应用的默认右侧、用户手动停靠恢复、键盘、DPI、输入法与读屏由[原生设备验收](native-verification.md)维护，不覆盖用户既有布局。短期指标趋势与批量维护仍是未选择扩展，本次不实施。

返回[文档索引](../README.md)。

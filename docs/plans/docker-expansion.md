# Docker 单层资源树交付收尾

状态：2026-11-21，Docker 0.8.0 已实现并正式发行；核心 1.11.0 与网络隧道 0.5.0 保持原版本。已实现行为与边界维护在 [Docker 功能](../features/docker.md)，版本与实际结果进入 [Docker CHANGELOG](../../plugins/docker/CHANGELOG.md#080)。

## 剩余验证与发布

- 对最终源码完成树、端口、日志与真实 Engine 回归，核对来源 Check。Windows OpenSSH／Docker Desktop 已走通信息、隧道、终端与维护，公开拉取遇到 Docker Hub TLS handshake timeout；直接 docker pull 复现相同外部故障，不能记为整套通过。
- 已完成 Package plugin 的五平台原生归档、对应 Runner 包内程序验收及同一草稿发布。
- 已核对匿名更新清单及五份归档的大小、摘要、标签与来源提交。仓库全局 Latest 保留核心；Docker 为自身通道最新正式版。
- 交付记录已写入 CHANGELOG 和永久交付证据；本计划保留专项验收边界，完成这些专项后删除。

## 专项归属

真实 IPv6／指定网卡／rootless 继续由[端口隧道验收](docker-port-tunnels.md)维护；Windows Server Windows Engine 沿用[Windows 远端验收](docker-windows.md)，日志长时／设备范围沿用[日志验收](docker-log-viewer.md)。这些范围不从普通浏览器、模拟进程或包启动结果推定通过。

实际应用的默认右侧、用户手动停靠恢复、键盘、DPI、输入法与读屏由[原生设备验收](native-verification.md)维护，不覆盖用户既有布局。短期指标趋势与批量维护仍是未选择扩展，本次不实施。

返回[文档索引](../README.md)。

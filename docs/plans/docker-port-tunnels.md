# Docker 端口隧道剩余验收

状态：2026-11-21，Docker 0.7.0／网络隧道 0.5.0 已完成本次源码及五平台原生包验收并独立正式发行。正常操作、失败与限制已迁入 [Docker](../features/docker.md#容器端口隧道)、[网络隧道](../features/network-proxy.md#其他插件的快捷入口)及[插件 API](../plugin-api.md#网络隧道协作契约)。实现采用既有 commands、sharedState 和后台逻辑视图，没有新增核心产品代码或生产依赖。

原功能与代码方案保存在 Git 历史提交 `7b86e65`，不保留另一份完成计划。源码与测试中已实现的规则不在本文重复定义。

本次源码、真实 OpenSSH／Linux Engine／Windows Docker Desktop 流程、五平台原生包及公开下载的实际结果、来源提交和 Release 已迁入 [Docker CHANGELOG](../../plugins/docker/CHANGELOG.md#070) 与[网络隧道 CHANGELOG](../../plugins/network-proxy/CHANGELOG.md#050)，本文仅保留尚未完成的专项。

## 尚未完成的专项范围

1. 在真实 IPv6 发布入口、指定非回环网卡发布、rootless Engine 上验证实际字节传输。地址推导和错误拒绝的单元／进程测试不能替代这些环境的真实行为。
2. Windows Server Windows Engine 与其他 Docker 远端组合沿用[Windows 远端剩余验收](docker-windows.md)，不把客户端五平台包验收视为该远端矩阵通过。
3. 在原生 WebView 中复核本功能跨窗查看规则、真实键盘／读屏／混合 DPI 与隐藏来源关闭的交互；通用设备范围沿用[原生验收计划](native-verification.md)。浏览器测试和包内进程启动不替代设备操作。

这些项目是验证范围，不新增未发布端口、UDP、容器联动或自动重连等功能承诺。取得环境后在此缩减未完成范围，把实际结果迁入插件 CHANGELOG；全部完成后删除本文，不另建完成计划归档。

返回[文档索引](../README.md)。

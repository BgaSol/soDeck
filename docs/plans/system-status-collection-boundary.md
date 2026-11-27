# 系统状态采集协议边界整改剩余交付

状态：实现与本机验证完成，待同源 CI、五平台独立包和正式发行（2026-11-28）。插件版本 0.7.1，API 2.2，核心不升版。

## 已实现与原因

POSIX 包装脚本在 END 前添加一个换行；旧分段器把它作为数据下传。Linux 严格解析因此把分隔空行视为缺字段，导致磁盘/接口失败和挂载 partial。修复统一在分段器移除包装脚本自己的一个换行，保留实际内容、空白与真实坏行检查。未改变 Windows JSON、RPC、采样调度、UI 布局、依赖或共享契约。

已补齐协议边界、脱敏动态/拓扑 fixture、逐来源失败、旧值时间与恢复预热、接口身份/手选、partial 到 complete 的共享挂载/容量清理回归。Linux/macOS 原生测试改为逐来源验收，真实 SSH 使用有界等待检查各来源，Linux 三插件通信检查完整扫描抵达文件消费端。

## 当前证据

- 修复前新加的分段/原始 Linux 回归稳定失败；修复后 Windows 本机 42 项插件 Rust 测试通过，2 项 Linux 容器专项按环境忽略。
- 格式/类型、234 项工程脚本、真实 OpenSSH 采集与隐藏/暂停/恢复、三主题七宽度 UI、三插件共享容量通过。
- 本机原生 Windows 发行归档已构建；正式发行使用 Actions 的同源原包。
- 首轮候选 `f249a4c` 的 [Check](https://github.com/BgaSol/soDeck/actions/runs/46040210811) 与[打包](https://github.com/BgaSol/soDeck/actions/runs/46040215441)在发布前主动取消：复核发现旧容器测试辅助每轮重建简化状态，需把接口断言移到拓扑当轮。产品修复未改变；最终提交重新检查和打包，不发布首轮候选。
- 真实 Ubuntu 机器通过现有会话执行生产代码生成的只读命令，再用相同源码的 decode/apply/Publisher 验证；7 个盘/分区、9 个接口、12 个地址、37 个挂载、4 项容量与第二轮磁盘速率通过，发布投影为 complete。[脱敏设备证据](../../plugins/system-status/release-evidence/v0.7.1-device.json)记录范围；未替换用户正在运行的插件，也未操作鼠标。

## 剩余步骤

- [ ] 确认最终源码提交的相关 Check 全部通过，保留首次失败及重试事实。
- [ ] main 手动运行既有 Package plugin，system-status、publish=true；五平台原生 package/verify 通过。
- [ ] 核对同一草稿五份归档与清单，正式发布，显式非 Latest；确认 Latest 仍为核心。
- [ ] CHANGELOG 与永久发行证据保留来源提交、原始 run、Release、附件摘要及实际验证/跳过范围。
- [ ] 完成后删除本文，修复引用；功能已同步到[系统状态](../features/system-status.md)。

Linux 地址来源单独失败被降级为空地址的问题与本故障无关，本次不扩展其字段或降级契约。深度设备范围仍在[原生验收计划](native-verification.md)，不把普通包进程验证记为全部远端系统或原生 UI 验收。交付遵循[分发流程](../distribution.md)，不覆盖正式同版本附件。

# CI 后续优化

状态：部分完成，保留后续工作。核对基线：`8115fdcd5bed4108021a729da605fbde77afc2af`。

## 目标与现状

减少重复准备、等待和缓存浪费，保持原有源码、业务与五平台原生包验证范围。当前任务选择、Linux browser、三行 integration 矩阵和诊断输出已落地，正式规则见[开发指南](../development.md#版本与持续集成)；核心打包复用见[分发流程](../distribution.md)。

## 剩余范围

| 工作 | 验收条件 |
| --- | --- |
| 缓存清理与容量 | 核对活跃键、大小、最近使用及命中率，再清理确认废弃的缓存；约 8 GiB 是操作目标，不当成 GitHub 配额或硬保证 |
| 更细的改动路由 | 先找真实重复执行，补删除、重命名、未知输入与共享输入的选择回归；未知输入仍扩大范围 |
| 插件发行预检 | 已正式发布的相同版本在原生构建前明确拒绝；保留草稿重试和独立发布，不覆盖正式附件 |
| UI 诊断覆盖 | 给尚未接入的 UI 测试补失败 trace，核对成功清理、失败保留与 Artifact 期限 |
| 分阶段耗时 | 区分排队、准备、缓存、编译、测试和汇总，保留相同源码与平台的可比较数字 |
| 条件实验 | 只有测量显示收益时试验 Playwright 容器或有界网络并发；不预先更换整套 CI |

每项分别实施和验证。至少取 3–5 次可比较的成功样本，报告中位数、范围及总 job 时间；一次成功不能给出 P95 或长期节省保证。保留失败样本及原因，不能排除失败后只展示最快一轮。

## 证据与收束

首批实现的成功检查为 [Check 42793876847](https://github.com/BgaSol/soDeck/actions/runs/42793876847)；它只证明该次输入，不能替代下一次改动验证。详细历史见[原实施记录](https://github.com/BgaSol/soDeck/blob/8115fdcd5bed4108021a729da605fbde77afc2af/docs/implementation-ci-optimization.md)。

改动入口：[check.yml](../../.github/workflows/check.yml)、[select-checks.mjs](../../scripts/select-checks.mjs)、[build.yml](../../.github/workflows/build.yml)、[plugin.yml](../../.github/workflows/plugin.yml)。按改动运行脚本回归、actionlint 和同一源码的相关工作流，实际发布仍按独立组件验收。

完成后把有效路由、缓存与诊断规则回写开发指南和分发流程，测量结论留在提交/PR 或相关发行记录，删除本文。部分完成时仅移出已落实部分。

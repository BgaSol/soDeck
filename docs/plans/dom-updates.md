# 表单与工作台局部刷新

状态：尚未实施。原标准库复用计划中的第 5 阶段；其他已完成阶段已归入正式文档。

## 现状与目标

[src/ui.ts](../../src/ui.ts) 的 `replace` 仍替换整段 DOM，并只为 text/search 输入恢复选区。旧试验中，密码框选区 `[1,4]` 在刷新后变成 `[0,0]`；当前类型过滤仍在。这是待修复的交互缺陷，不能通过放宽功能说明抹掉。

异步刷新应保留当前实体的有效草稿、焦点、选区和输入法组合；切换实体时不把旧输入带到新对象。不要为减少 DOM 更新改变保存、取消、关闭或输入所有权。

## 实施与验收

1. 用当前源码重现密码选区问题，并覆盖 textarea、select、checkbox、连续输入和 IME；区分同实体刷新与实体切换。
2. 先修普通表单，按实际问题选择最小接法。曾试验的 morphdom 尚未选定或安装；新增生产依赖仍遵守 [AGENTS.md](../../AGENTS.md) 的要求。
3. 有证据后再处理工作台 `patchChildren` / `patchChrome`：保留 iframe、正在按压的按钮、指针捕获、菜单与输入权，不因状态刷新重建业务页面。
4. 用真实浏览器事件覆盖异步返回、刷新期间按压、保存/放弃后的焦点以及实体切换。相关入口见[开发指南](../development.md#检查与测试)，原生 IME 另按设备验收范围记录。

历史试验见[原标准库复用记录](https://github.com/BgaSol/soDeck/blob/8115fdcd5bed4108021a729da605fbde77afc2af/docs/implementation-standard-library-adoption.md)。旧试验结果不能替代修复后的回归。

完成后回写[设置](../features/settings.md)和[工作台](../features/workbench.md)的交互约束，必要时更新架构；留下回归测试，删除本文。

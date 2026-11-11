# DOM 刷新与输入连续性

状态：仍待真实流程复现、必要修复与原生验收。本文只保留尚未完成的 DOM/IME 要求；已实现的表单选区恢复不代表这些要求已全部满足。

本文限定普通表单与工作台 DOM。文件编辑器的 IME、草稿与交接故障组合由[结果确认计划](operation-outcomes.md)维护，跨功能原生设备矩阵由[原生验收计划](native-verification.md)维护。

前序七项标准库与现有依赖复用改动的交付和验证记录见[核心 0.23.5](../releases/v0.23.5.md)、[文件 0.10.3](../../plugins/files/CHANGELOG.md#0103)、[终端 0.8.4](../../plugins/terminal/CHANGELOG.md#084)、[网络隧道 0.4.2](../../plugins/network-proxy/CHANGELOG.md#042)、[系统状态 0.4.4](../../plugins/system-status/CHANGELOG.md#044)及 [Docker 0.1.4](../../plugins/docker/CHANGELOG.md#014)。

## 剩余要求

- 同一实体的异步刷新保留有效草稿、焦点、选区方向和正在进行的 IME 组合输入；持续输入不能因刷新丢失、重复或被旧异步结果覆盖。
- 切换实体不能把上一对象的输入带入新对象。保存、放弃、取消和关闭继续遵循各自草稿及生命周期规则，明确当前输入归属和失效结果的处理。
- 在真实表单中补齐连续输入、组合输入、异步返回、select/checkbox 和实体切换的验证，依据实际失败选择最小修复。
- 工作台 [patchChrome/patchChildren](../../src/ui.ts) 继续保留 iframe、按压按钮、指针捕获、菜单及输入权，补齐按压期间刷新、保存/放弃焦点和实体切换的回归。隐藏、同窗布局调整与跨窗交接的业务资源语义沿用正式契约。

## 核心与插件边界

核心管理自身表单、工作台 DOM 和页面显示交接；插件管理自身页面、编辑模型及业务输入。布局刷新不能重建仍在使用的插件 iframe 或替插件重放业务，插件不能自行提交跨窗输入归属。具体交接和关闭规则见[架构](../architecture.md)、[工作台](../features/workbench.md)与[生命周期](../features/lifecycle.md)。

先复现再决定接法。整体 DOM morphing 和核心 overlay 改用原生 dialog 尚未选定，不因本计划安装 morphdom 或全局替换工作台渲染。

## 验收

| 范围 | 可验收行为与入口 |
| --- | --- |
| 普通表单 | 在服务器资料与设置的真实异步流程中持续输入和选择，覆盖 IME 组合、select/checkbox、旧结果返回、保存/放弃/关闭及实体切换；当前对象保留有效草稿，新对象不继承旧输入。入口：[服务器 UI](../../scripts/test-server-ui.mjs)、[设置 UI](../../scripts/test-settings-ui.mjs)、[正式前端](../../scripts/test-production-ui.mjs)。 |
| 工作台 | 在 pointerdown 与 pointerup 之间刷新，原按压节点、指针捕获、菜单目标和 iframe 保持有效；布局、隐藏与移窗继续遵循输入权和资源契约。入口：[工作台交互](../../scripts/test-workbench-interactions.mjs)、[布局](../../scripts/test-workbench-layout.mjs)、[工作区](../../scripts/test-workspace-ui.mjs)、[原生关闭与迁移](../../scripts/test-close-native.mjs)。 |
| 原生设备 | 本计划中的表单与工作台问题需补原生 IME、真实窗口缩放与 DPI 证据；五平台设备范围统一见[原生验收计划](native-verification.md)。分别记录环境、结果和跳过范围，浏览器事件模拟及包启动不能替代设备验证。执行方法见[开发指南](../development.md#检查与测试)。 |

Windows CI 的文件编辑器专项目前受调试端口不可用限制，原因尚未确认；本地 Windows WebView2 编辑器专项已通过。该差异需在原生验收中核对，不能据此宣布 IME 或所有 DOM 流程已验收。

每个稳定复现的缺陷形成长期回归，修复后按受影响范围完成格式、类型、构建与测试。新增或修复测试入口时同步实际检查选择，不以 skipped 或历史通过替代本次验收。

## 正式文档归属与收束

| 行为或证据 | 唯一归属 |
| --- | --- |
| 服务器表单草稿、焦点、选区与实体切换 | [连接](../features/connections.md) |
| 设置草稿、冲突与保存反馈 | [设置](../features/settings.md) |
| 工作台 DOM、指针、菜单与页面输入权 | [工作台](../features/workbench.md)，关闭语义链接[生命周期](../features/lifecycle.md) |
| 插件自身编辑与业务输入 | 对应[功能文档](../README.md#插件功能)；文件编辑器归[文件](../features/files.md) |
| 验证方法与发行结果 | 方法归[开发指南](../development.md)；核心结果归对应 `docs/releases/`，插件结果归各自 CHANGELOG |

已实现行为及时写入正式文档，本文只保留尚未完成范围及必要上下文。全部要求完成或经明确取消、替代后，安置仍有效约束、修复引用并删除本文与无用附件；不新建完成计划档案。

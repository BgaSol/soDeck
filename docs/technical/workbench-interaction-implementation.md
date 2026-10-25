# T-006 实施细化

2026-10-25 · r1 · 已确认实施范围内的接口和验证细化

依据 [T-006](../tasks/T-006-workbench-interaction.md)、F-CORE-008 r2 / F-TERM-002 r2 / UI r8 / 主题 r1。用户已明确要求实施，以下为其范围内技术落点，不新增审批。

## 1. 主题

`themes/graphite.json` 是默认主题唯一色值来源；契约在 `contracts/presentation-v1.schema.json`。主题含 schemaVersion=1、id、mode、colors（完整语义映射）、terminal（全部 xterm 色字段与 240 项扩展色）。核心以编译内置文件为回退，并读取包资源中的同名主题；隔离测试可用 SODECK_THEME_FILE 指定另一文件。颜色校验为 #RRGGBB/#RRGGBBAA，缺失或非法主题整份回退并报告。主题快照含 revision，注入各表面初始化上下文并提供 `ui.theme.get`。SDK 只负责映射，不持有默认色。高对比在平台/共享主题层映射系统色。

## 2. 呈现与命令接口

- `ui.menu.open({menuId,anchor:{x,y},items:[{id,label,enabled,shortcut?,separator?}]}) → {menuId}`，坐标为来源表面 CSS 像素；核心绑定 source/window/view/mount，转为屏幕坐标。限制 64 项、文字 256 字符；ID 唯一，禁用项不能执行。
- `ui.menu.close({menuId}) → {closed}`，`ui.menu.result({menuId}) → {menuId,status:'open'|'closed',selectedId:null|string}`；`ui.menu.closed` 发送相同结束结果。查询和关闭只能操作同一来源创建的菜单，旧挂载拒绝。结果缓存有界，支持事件早于 open 回执的竞态与丢通知恢复。
- Windows 使用 TrackPopupMenuEx 与 owner-draw；核心在专用菜单线程创建临时隐藏 owner HWND 处理绘制/度量，结束立即销毁，颜色取主题。调用立即返回，菜单运行在独立线程的原生消息循环，外部/Esc/失焦自动取消；工作台变更、移窗、销毁主动 EndMenu。应用最多一个原生菜单，重复触发取消旧菜单；无 workbench.modal 屏障、无隐藏终端。菜单状态最终关闭在所有退出路径执行。
- 原生菜单选中项的正文和快捷键消费 selectedText，与 selected 背景配对；禁用项消费 disabled。该映射也适用于系统高对比主题，避免保留常规正文色导致选中状态不可读。
- `commands` 可选 `input:{kind:'text',key,label,initial:'viewTitle',minLength,maxLength}`，核心展示通用输入框，插件校验并执行命令；不以 terminal.rename 名称分支。`views.actions` 由插件按状态发布重命名/重连可用性。
- 清单 `retiredViews:[viewType]` 为所属插件退役类型，必须不在当前 views 中；仅在启动且无运行资源时移除相同 pluginId 的历史条目。未知缺失类型仍为故障占位。

## 3. 布局与版本

API/SDK 3.0.0，workspace.v3，ui.presentation.v1；核心 2.0.0-dev.3、终端 0.3.0。保留 v2 schema/用例只作为旧格式夹具。布局 schema 3 在加载旧数据时先备份、合并两个中央组，再按插件支持位置将底部终端追加中央；活动页按 focus_target/原活动页恢复。声明应用后保存候选再创建表面；失败保留文件和恢复提示。所有 group=1 / split 非 none 请求拒绝且不变更状态。应用菜单提供“重试保存布局”，调用 workbench.persistence.retry → {persisted:true}；仅对已成功读入/迁移的候选解除保存故障，损坏源文件继续保护并提示退出后恢复 pre-v3 备份，不允许用空布局覆盖。升级新核心时配套激活新终端包；旧核心拒绝 API 3 包，不写旧数据。

## 4. 输入与测试

终端输入处理按 F-TERM-002，独立模块捕获选区/焦点世代、阻止 DOM 默认重复粘贴，异步写入前核对完整绑定。保留原输入队列及序号。退出后重连从宿主提供的 instanceKey/server/workspace/title 恢复配置，明确创建新 PTY，不保存秘密或输出。

按 UX-01…10 / IN-01…07 执行。增加 schema/主题校验、布局迁移幂等/写失败/退役归属、剪贴板事件/迟到结果、菜单来源/结果缓存测试；浏览器验证交互，Windows 原生验证菜单/焦点/剪贴板/IME，SSH 区分 loopback 与 Linux shell。配色更换使用同一二进制、两份独立测试主题。开发包 `.local/dev/SoDeck-v2-T006/`，旧包不覆盖，实际结果写任务附录。

# T-003 前端与工作台验证记录

日期：2026-10-25 · 关联 [T-003](T-003-core-terminal.md) · 验证对象为当前任务工作区源码及对应构建，用户验收、完整工程完成和发布由主任务分别记录。

## 实现范围

已实现独立 SDK、核心外壳、实际 xterm 终端与会话列表，稳定接口见[前端联调约定](../technical/terminal-ui-contract.md)和[联调契约](../technical/implementation-contract.md)。核心提供服务器工作区、最多两个中央区域、底部停靠、独立窗口、模态与故障外框；终端 UI/worker 独立打包。

工作台支持声明目录驱动的位置、默认标题、scope 和单/多实例校验。目录不持久化；布局恢复清除运行句柄，不支持位置回落声明默认位置，缺包保留占位。活动页由核心快照决定，合并、移动、关闭后每个容器只有一个活动页。中央和底部 Tab 支持方向键/Home/End，独立窗口提供可拖动 Tab 及菜单替代。

终端使用实际字节消费和确认游标，输入有序、背压不跳号、结果不明不重放；跨窗口快照冻结/提交保持 PTY，恢复从权威输出/输入游标继续。恢复丢失屏幕时明确提示，无 PTY 的占位不声称连接仍存活。会话列表提供行内重命名，支持取消、输入法组合保护、失败重试和焦点返回。

## 最终自动化结果

| 检查 | 结果与范围 | 证据 |
| --- | --- | --- |
| TypeScript | SDK、desktop、terminal 检查通过 | 各 workspace 的 npm run check |
| SDK/组件/worker | SDK 4 项、表面队列 3 项、终端快照 3 项、真实 worker 6 项通过 | npm run test:contracts 及对应 workspace test 入口 |
| 浏览器契约与界面 | **13 项全部通过**：工作台 7 项、实际 xterm 组件 4 项、会话列表 2 项 | .qa/final-ui-tests.log、.qa/ui/results.json |
| Rust 工作台模型 | **11 项通过**；最终桌面 crate 共 13 项通过，另 2 项验证原生几何和已加载资产不可变 | .qa/final-rust-tests.log |
| 桌面构建 | 主任务的 debug 原生构建成功；最终运行产物、包哈希与启动方式由主任务统一记录 | .qa/final-native-build.log、[本地入口](../development-entry.md) |

浏览器命令为 `npx playwright test --config playwright.ui.config.ts`。页面注入测试 IPC/PTY 提供者，只证明渲染与边界契约，没有创建 Mock 产品插件，也不能代替真实 SSH、原生窗口或 IME。

工作台模型检查覆盖：目录/句柄不持久化、通用声明的默认位置/标题和位置拒绝、server/global 作用域、单/多实例、无效目录的原子拒绝、声明变化回落和缺失占位、活动页归一、工作区隔离、配置失败不部分修改，以及连续三次保存/加载后结束和故障占位保持。Rust 复跑入口为：

```powershell
node scripts/cargo.mjs --config 'source.crates-io.replace-with="rsproxy"' --config 'source.rsproxy.registry="sparse+https://rsproxy.cn/index/"' test --offline -p sodeck-desktop workbench::tests -- --nocapture
```

## 界面证据与回归重点

已查看 `.qa/ui/welcome.png`、`workbench-contract.png`、`compact-contrast.png`、`terminal-contract.png`、`session-rename-contract.png`。这些是浏览器契约截图；重命名截图采用 220×560 窄侧栏，输入、焦点与保存/取消操作可见。shell 单页截图不能包含 raw Wry 子表面的全部真实原生合成画面。

以下缺陷已有对应回归：

- 底部会话被外部激活后显示旧页：以核心 active 为准，验证可见表面与 aria-selected 同步。
- 拖动释放产生合成 click：独立标记消费该次点击，包括 Esc 取消后的释放；实际鼠标事件验证 DOM 收到 click 而不额外 activate，下一次普通点击仍有效。
- 迁移或恢复游标重置：验证输出不从零读取，输入使用最后受理序号的下一号；快照先恢复原 cols/rows，中文与 emoji 分块不截断代理对。
- 第二次启动把 ended 改为 ready：重复保存/加载保持 ended/viewFailed，始终清除 PTY/session/generation。
- 缺包故障页误称连接存活、resize 清空操作错误：无 PTY 显示未恢复会话，重试保留实际 PLUGIN_UNAVAILABLE 等错误码和原因。

## 原生结果及其边界

主任务已经运行当前桌面原生回归，`.qa/native/1792870279702/result.json` 为 passed: true，范围是 Windows + loopback Paramiko SSH 协议，远端是字节回送器，**不是 Linux/OpenSSH shell**。该结果包含：两个工作区四个真实 PTY、分屏/底部停靠、十次弹出返回、托盘继续运行、200% 缩放下从独立 Tab 物理拖回主窗口且 PTY 不变、源拒绝后的回滚、真实 renderer 崩溃与显式恢复、QuickJS 子进程终止后资源回收，以及去重缓存满时查询/关闭/正常退出仍可用。

完整桌面脚本、截图、构建标识和其余结果以[桌面附录](T-003-desktop-results.md)为准；独立更新与包校验另见[插件记录](T-003-plugin-results.md)，SSH/日志故障与负载另见[核心记录](T-003-core-results.md)。

`tests/desktop/windows.ps1` 只接受当前工作区测试程序的 PID，在该进程内按标题选择窗口。list 提供客户区物理位置、尺寸和 DPI，move 保持大小/Z 序；独立烟测 `.qa/windows-helper/1792869642470/result.json` 验证 192 DPI 的移动前后尺寸与客户区偏移一致，非测试 PID 在窗口操作前被拒绝。

Linux/OpenSSH 按用户决定保留未验证；中文 IME 的真实候选窗口、多显示器不同缩放、完整可访问性、最小分屏尺寸及正式安装发布不能从这些浏览器/单屏原生结果推定通过。当前用户验收状态由 T-003 维护，本附录不代填通过。

## 2026-10-25 接续：单次新建取消

依据已确认 SSH-03/06、TERM-01/08 补齐原缺失入口，先更新核心 SSH r2 的 SSH-06a 与前端契约 r2，再实现；不扩大本批范围。apple-design 落点为直接反馈、明确目标、与操作相邻的取消按钮及键盘焦点返回。

工作台的每次新建都关联独立操作 ID，快照统一列出待连接项，因此主按钮、插件会话列表及 ended 占位入口都由外壳提供取消；工作区切换不隐藏待连接项。取消与视图提交按同一状态锁判断；已提交视图不被当作取消对象，取消先发生则关闭迟到 session 并且不新增 Tab。worker 将操作 ID 传给 acquire/pty.open，取消后的迟到租约/PTY 回收，清理失败仍显示 CLEANUP_REQUIRED。主机确认/凭据对话框取消不会自动开始下一次尝试。

本轮工作区实际验证：

- `npm run check`：三个 TypeScript 包及依赖边界通过。
- `node --test plugins/terminal/tests/worker.test.mjs`：9 项通过；新增单项取消隔离、acquire/PTY 迟到清理、提前取消以及清理失败保留原因。该层使用注入核心提供者。
- `npx playwright test -c playwright.ui.config.ts tests/ui/workbench.spec.ts`：9 项通过；新增键盘取消和再次新建、外部来源两项独立取消、取消失败可重试。补充焦点返回及取消后的迟到认证响应/通知抑制后，两项受影响用例再次通过，TypeScript/Rust 编译同样重新通过。
- `node scripts/cargo.mjs --config 'source.crates-io.replace-with="rsproxy"' --config 'source.rsproxy.registry="sparse+https://rsproxy.cn/index/"' check --offline -p sodeck-desktop`：通过，覆盖 app.rs 与当前核心取消 API 编译。
- 已查看 `.qa/ui/pending-connection-cancel.png`：900×640、高对比、减少动态效果下状态、服务器名称与取消按钮可见，无横向溢出。

上述结果属于浏览器/worker/编译验证；核心真实协议取消及已重新构建的原生取消按钮结果见[核心附录](T-003-core-results.md)，最终包哈希由[桌面记录](T-003-desktop-results.md)维护。

### 调用记录分页补查

同毫秒记录原先仅以时间翻页会漏项。已采用开始时间和记录 ID 的稳定复合游标，保留旧 before 时间筛选，界面增加加载更早记录及来源/服务器/结果筛选、保留起点。改变筛选会重新读取第一页，旧响应不能覆盖新筛选。

真实 SQLite 205 条同时间记录分三页读取（100/100/5），无重复遗漏，中途新记录不影响后续旧记录；无效游标和筛选隔离检查通过，日志 `.qa/record-pagination-rust.log`。浏览器分页/四类筛选/迟到响应用例通过，日志 `.qa/record-pagination-ui.log`；已查看 900×640 实际浏览器截图 `.qa/ui/call-records-pagination.png`，所有操作及保留区间可见。此处浏览器采用注入提供者；存储正确性由独立 SQLite 用例证明。

本轮整合 JS/TS 契约 19 项、浏览器 16 项通过，日志 `.qa/resume-contracts.log`、`.qa/resume-ui.log`。后续更改导致受影响的检查单独重跑，最终原生证据以桌面记录为准。

### 原生输入与视图版本恢复补验

此前标为待验证的单显示中文IME已有[输入法记录](T-003-ime-results.md)；常用控制键、两个PTY焦点隔离及实际重命名输入框的Enter/Esc操作也已补齐，4组通过，见[原生输入记录](T-003-native-input-results.md)。旧段落仅记录当时范围，不代表这些项目目前仍未执行；剪贴板完整路线、多显示器和辅助技术仍有缺口。

UPD-05原生复现发现旧视图未检查状态版本，已保存并比较每个视图的stateVersion，外壳显示明确的不兼容原因；用户重新加载后清除旧展示状态，结束的终端不自动连接。最终工作台相关9项浏览器测试、TypeScript及依赖边界复验通过（`.qa/schema-ui-tests.log`、`.qa/schema-check.log`），18项桌面Rust测试通过。新构建真实占位/恢复与两次更新中断验证见[插件记录](T-003-plugin-results.md)；旧浏览器测试未替代这次原生结果。

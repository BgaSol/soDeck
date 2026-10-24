# T-003 前端联调约定

2026-10-25 · r2 · 已确认方案内的实现细化。由前端实现维护，桌面集成按此约定对齐，不改变 T-003 范围。

本轮实现、公开对接签名及实际验证结果见 [T-003 UI 记录](../tasks/T-003-ui-results.md)；本文件维护稳定的联调契约。

## UI 基线

已读取 apple-design：系统字体；主内容不透明，侧栏分层；14px 基础字号，4/8px 间距，8px 控件圆角；深色终端、清晰的白色/绿色/琥珀色状态；可见键盘焦点；所有拖动提供菜单替代。原生表面由工作台测量 CSS 像素边界交给桌面转换 DPI；模态期间隐藏插件原生表面，关闭后恢复。主题令牌通过公共 CSS 与终端 SDK 主题一致，不向插件共享 React 实例。

## 桥与上下文

SDK `core.call<T>(method, params)` 使用联调契约 RPC 封装；不存在原生桥时显示不可用，生产没有模拟终端。原生通知入口为 `window.__sodeckNotify({type,payload})`，订阅通过 SDK；页面准备完成先调用 `view.ready`。可注入的上下文包括 `viewId/viewType/workspaceId/serverId/serverName/pluginVersion/mountGeneration/ptyId/generation` 以及迁移 `snapshot`。调用者不接受页面自报身份。

PTY 读请求 `{ptyId,generation,after}`，结果 `{frames:[{sequence,bytes}],lastSequence,state,reason?}`；`bytes` 为 base64，消费后 `pty.ack({ptyId,generation,sequence})`。`pty.write` 逐序号且不自动重试。快照 schema 1 保存 xterm 屏幕、滚屏位置、选择、字号、输入序号和已消费游标；迁移收到 `view.prepareMove` 通知时冻结输入、等待在途消费并通过 `view.snapshotChunk({operationId,index,total,data})` 顺序回送；data 为 JSON 字符串片段，每片最多 256 KiB UTF-8，总量最多 4 MiB。目标恢复完成调用 `view.ready`。`view.moveCancelled` 恢复源输入，`view.moveCommitted` 使源失效；最终块结果不明不能自行恢复源输入。核心必须拒绝旧挂载输入。

## 工作台方法

实现边界补充：Rust `Workbench` 在启动加载后通过 `set_declarations(Vec<ViewDeclaration>)` 接收已验证插件清单的视图目录，目录不写入布局。位置映射只认识通用 `tab→center`、`window→detached`、`dock→left/right/bottom`；是否支持由 `supportedHosts/dockEdges` 决定，不按终端类型硬编码允许位置。`open_view(view_type,server_id)` 使用声明默认位置/标题与 `scope/multiple`，同一服务器内单实例复用，全局单实例跨服务器复用。保留当前终端便捷入口，缺少声明返回错误。恢复布局只作结构检查，目录接入后缺失视图保留故障占位、不支持位置回落声明默认位置；不恢复运行句柄。合并分组后每个容器只保留一个活动页，移动或关闭活动页时补选有效相邻页。

上述契约单元验证包含任意非终端声明、默认 dock/tab/window、缺失/非法默认、单/多实例和全局作用域、拒绝未声明位置、目录不持久化、布局恢复回落、分组合并及移动后活动页唯一；不以仅终端特判通过替代清单边界。

布局连续多次保存/启动后，已结束资源视图仍保持 ended 占位，不能在第二次启动被改成 ready；故障占位保留故障状态。恢复运行句柄始终为空，重建 UI 不代表重新建立 SSH。对应 WB-11/12、TERM-08/11 的模型回归连续保存/加载两次并核对状态和句柄。故障页只有在快照确实带有 PTY 时才提示可重建存活连接的显示；缺包或没有 PTY 时说明没有恢复运行会话。重试失败展示实际结构化错误码及原因（如 PLUGIN_UNAVAILABLE），布局测量成功不能清掉该操作错误；浏览器契约补查该提示及错误在 resize 后仍可见。

外壳调用 `workbench.snapshot` 获得 `{views,activeWorkspaceId,split,sessionsSide,sessionsVisible}`；view 含 `viewId/viewType/workspaceId/serverId/title/status/placement/group/mountGeneration`，placement 为 `center/bottom/detached/left/right`；group 为 0 或 1。

- `workbench.activateWorkspace({serverId})`：打开或激活该服务器工作区，不创建 SSH。
- `workbench.openTerminal({serverId,operationId?})`：插件业务创建并挂载真实终端，返回 view；缺省操作 ID 由宿主生成。快照额外提供 `pendingOpens:[{operationId,serverId,cancelling}]`，开始/结束会发送 changed。所有来源在外壳显示明确目标及单项取消入口，切换工作区仍可取消。
- `workbench.cancelOpen({operationId})`：取消尚未提交的单次新建，返回 `{canceled:true}`；若视图已提交则返回 `{canceled:false,viewId}`，不结束运行会话。宿主保留有界提前取消记录，取消与视图提交互斥；收到迟到的 session 时关闭其 PTY/租约，不创建视图。
- `workbench.activate({viewId})`、`workbench.close({viewId})`、`workbench.move({viewId,placement,group?})`。
- `workbench.configure({split?,sessionsSide?,sessionsVisible?})`；split 为 `none/horizontal/vertical`，最多双区域。
- `workbench.surface({viewId,bounds:{x,y,width,height},visible})`：CSS px；隐藏不关闭 PTY；核心原生视图尺寸与可见性入口。
- `workbench.modal({open})`：跨插件表面屏障。
- `app.hide`、`app.exit`、`plugins.list`、`plugins.prepareUpdate({path})`：核心入口。

UI 在 `workbench.changed` 通知后刷新快照；另有低频刷新恢复漏通知。独立窗口外壳从 `window.__SODECK_CONTEXT__.detachedViewId` 获取唯一视图。窗口移动和快照由原生核心协调，UI 不重新建连。

中央及底部容器统一以核心快照的活动视图为准；用户从会话列表或另一个窗口激活视图时，旧的本地 Tab 选择不能覆盖核心结果。Tab 条提供左右方向键及 Home/End 选页，Space/Enter 激活当前页；子按钮的键盘事件不触发外层 Tab 激活。对应 WB-03/13、TERM-04/05 的浏览器契约回归：在底部打开两页，先本地选 A，再由核心通知激活 B，验证可见表面和 aria-selected 同步；通过方向键及 Home/End 验证激活请求和焦点始终指向同一页。原生终端焦点交接由桌面测试验证，执行结果统一写入任务附录。

独立终端内容上方保留核心 Tab 条，作为跨原生窗口拖回主工作台的可抓取入口；系统标题栏仍承担整个窗口移动。拖出源页面后的释放只提交原生落点请求，目标位置由原生当前指针位置决定。浏览器验证该入口、Esc 取消及请求契约；WB-05/13、TERM-06/09 的原生流程另验证实际窗口命中、DPI 和连续输入。

Tab 拖动越过阈值后须消费该次鼠标释放产生的合成 click，包括 Esc 取消后的释放；不能在提交移动后又发送源页 activate，打断迁移焦点或切换原服务器工作区。抑制只针对该次拖动，下一次正常单击仍可选页。WB-05/13、TERM-06 的浏览器回归使用实际鼠标按下/移动/释放：断言只有移动请求、没有迟到激活，再正常单击同一 Tab 断言可以激活；原生物理跨窗口测试与浏览器分层执行，结果统一写入任务附录。

## 终端插件命令

同插件路由 `plugin.invoke({command,args})`：`sessions.list({workspaceId})` 返回数组，`sessions.create({workspaceId,serverId,operationId?})` / `sessions.cancelOpen({operationId})` / `sessions.close({sessionId})` / `sessions.reconnect({sessionId})`，`settings.get` 返回 `{fontSize}`；`settings.set({fontSize})`，0.1.1 新增 `settings.reset`。view 类型 `terminal.sessions` 使用会话列表，其余使用 xterm。业务版本从上下文读取，0.1.0 构建不包含恢复字号入口。

会话列表的重命名按钮打开当前行内输入框，初始全选现名；Enter 保存、Esc 取消，另有保存/取消按钮。名称去除两端空白后须为 1–80 个字符，不能包含控制字符；输入法组合中的 Enter 不保存。请求 `plugin.invoke({command:"sessions.rename",args:{sessionId,title}})`，核心同步视图/Tab 和独立窗口标题，保持 sessionId/viewId/PTY 不变。保存失败保留输入并显示原因，不关闭会话。浏览器契约验证取消不发送、去空白保存、空白禁用、组合中 Enter、失败重试及焦点返回；原生回归另检查 Tab/独立窗口同步和 SSH 连续。

## 验证

SDK 契约验证请求关联、超时不重发、错误、二进制块；布局验证最多两区域、隐藏和模态尺寸；终端依据 TERM-01 至 TERM-12 执行，浏览器仅证明布局/契约，真实 SSH、中文 IME 与移窗必须由原生验证。

单次取消延伸已有 SSH-03/06、TERM-01/08：`sessions.create` 将可选 `operationId` 透传 `connections.acquire` 与 `pty.open`；`sessions.cancelOpen` 调用 `operations.cancel` 并标记本地尝试，成功到达但尚未交付的 PTY 也必须关闭。核心 token 按调用者实例隔离，提前取消保留 10 分钟、最多 10000 项，不淘汰活跃项。取消不停止已复用的连接。取消入口属于清理调用，RPC 缓存/记录故障不能阻止它。浏览器补测等待状态、取消键盘路径、取消失败可重试、外部来源待连接、取消后迟到认证不会重新弹窗；worker 补测两项并发只取消一项及两阶段迟到清理。原生验证仍需真实 Tauri/SSH 证据，不以注入提供者替代。

## 表面、迁移与恢复约束

原生表面布局按 viewId 串行提交，每个视图最多保留一个执行中和一个最新待执行布局，resize 合并为最新值。卸载旧容器的隐藏先于新容器显示；隐藏也提供完整 `bounds`，卸载使用零 bounds，避免迟到隐藏覆盖新位置。

单窗口内移动保留屏幕实例，跨窗口按快照恢复。快照包含原 cols/rows，目标先按原尺寸恢复屏幕，再适配新容器，避免长行提前换行。客户端按 64 Ki UTF-16 单元且不截断代理对拆分；宿主继续验证 UTF-8 长度、块序号、来源、总量和完整 JSON。每块最多 256 KiB、总量最多 4 MiB、最多 64 块，RPC 仍受 1 MiB 帧限额约束。

PTY 只确认实际消费的最后一帧，不能直接用 read 返回的 lastSequence 作为确认游标；结束状态仍消费剩余缓冲。输入序号仅在受理后前进，背压不跳号，结果不明暂停输入且不自动重发。输入法组合未完成时明确拒绝迁移；目标 ready 完成前不读/确认输出、不调整 PTY、不接受输入，避免候选挂载抢占源绑定。

恢复表面由原生 `pty.bind` 返回权威 `acknowledged/lastInputSeq` 并写入恢复上下文，插件从该位置继续，不从零盲读或重用输入序号。重建 UI 不新建 SSH；丢失屏幕明确提示，不能冒称已恢复。当前 `view.ready` 返回 `{ready:true}`，客户端不能依赖未提供的游标字段。

会话列表发起新终端若遇到主机身份或认证需求，核心向所属外壳发送 `auth.required`，payload 为 `{server,error,operationId}`；取消时外壳收到 `workbench.openCanceled({operationId})`，关闭对应认证层并抑制该次尝试迟到的认证响应/通知，不能自动重新打开。秘密输入始终留在外壳，用户处理后显式重试创建。插件缺失时通过空声明目录保留故障占位；没有 PTY 的旧终端在恢复时回到 ended 占位，提供明确新建入口，不挂载没有会话的终端页。

跨窗口落点由原生物理指针位置与可见主窗口客户区决定，转换 DPI 后选择中央/底部区域；页面自报 screenPosition 不决定归属。原生测试与浏览器契约分别记录，实际覆盖范围以 [T-003](../tasks/T-003-core-terminal.md) 及其结果附录为准。本文件不累加历史进度或测试次数。

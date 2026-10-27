# T-009 代码方案

2026-10-26 · r2 · 依据：用户已确认 T-009 / F-CORE-010 r2 并要求开始修改。

## 1. 模块与兼容

- 工作台继续作为核心权威内存模型。生产启动不读取旧布局；新增独立偏好存储模块，从可识别旧布局白名单提取主窗 bounds，保存 `window-preferences.json` schema 1，仅含主窗位置/尺寸/最大化。旧快照按 F-CORE-010 清理，目录/链接拒绝删除；错误可见且可重试。
- 生产布局提交只改内存；`persisted:false` 表示本次运行对象不持久保存，不附 STORAGE_ERROR。主窗偏好写入/旧记录清理以独立 `workbench.preferences.retry` 重试，不伪称保存布局。原操作提交与去重语义保持。
- 生成 `workbench-v4.schema.json`，当前生成器/SDK/宿主消费 v4；保留历史 v2/v3 作为旧协议资料。插件 API 4 / workspace.v4，出厂终端 0.4.0、核心开发包 dev.4；旧 API 包明确拒绝，开发数据中旧终端按已有出厂兼容升级路径处理，设置继续由管理器迁移保留。准备 0.4.1 独立包验证只更新终端，失败回退不清空配置。
- 清单增加显式 `workspaceInitializers`：动作 ID 与同插件 server scope commandId；验证唯一性/作用域。核心不识别终端命令名。工作区增加本次创建 generation；初始化状态按该 generation、动作和尝试记录，快照可查询，重试/取消通过公共操作。

## 2. 工作区创建与关闭

所有 workspaces.open 串行提交创建判断，已有工作区直接定位。新工作区先登记初始化动作再异步执行；返回工作区创建与初始化操作标识，UI 不等待网络响应才显示 Tab。重复创建/响应丢失、渲染重挂载不重复执行。关闭使旧代次/操作失效，禁止晚到 views.open/bind/update 绑定新的同名工作区。

复用 commands.execute、pending operations、统一认证及 operation.get/cancel。增加初始化重试入口处理缺插件/额度不足；首终端已有视图时由终端原位重试。调用记录通过原核心网关，初始化动作的内部调度仍记录 commands.execute。

终端先建立 connecting 会话模型和逻辑视图，再 acquire/PTY；失败、取消或等待认证保留本次页，资源补偿后更新状态。认证继续原请求时通过同一 viewId 重试，旧尝试不得更改新尝试；视图关闭删除模型并取消其连接，已关闭后晚到资源补偿释放。额度覆盖存活模型与正在分配的首终端，原位重试不重复占用。表面失败仍走原 PTY 恢复。

## 3. UI

沿用已读取的 apple-design 与主题规则。空态区分加载、无档案、已有档案及读取失败。新增表单“保存并打开”，编辑仅保存。首终端连接中/认证/失败/取消可见，操作期间不缓存输入；资源/状态变化刷新当前表面，只有仍可见且前台的目标可获焦点。异步完成不重新激活工作区。初始化失败在所属工作区显示重试/插件管理入口。

## 4. 验证与产物

新增 Rust 偏好/旧记录清理测试、初始化去重/代次模型测试；更新真实终端 worker 测试覆盖先建页、认证原位重试、取消、关闭晚到、预算与失败释放。更新生成契约/SDK 测试、浏览器空态/表单测试。旧布局恢复与落盘失败用例依 F-CORE-010 §6 替代，保留历史结果。

执行 npm run check、npm run test:contracts、Rust workspace tests、Playwright UI、生产前端/终端及 Windows debug 构建。新增 `tests/desktop/clean-start.mjs` 与 `tests/desktop/clean-start-plugins.mjs`，复用 harness、受控 loopback SSH 和独立 `.qa/t009` 数据，覆盖 CS-01 至 CS-17 适用原生流程及截图；Linux shell 与 loopback 协议证据分开。复验受影响托盘/移窗/认证/更新；避免使用旧脚本的不兼容恢复预期。交付独立 `.local/dev/SoDeck-v2-T009` 目录包，不覆盖旧包/data，不自动提交。

结果与未完成项记录 T-009；不得把编译、浏览器或协议夹具通过写成所有原生/真实 Linux 场景通过。

## 5. 范围内实施细节（r2）

- `workspaces.open` 回执携带 generation 与初始化动作；同 operationId 的重放返回已提交结果，不因原工作区已关闭而重建。普通“新建”命令遇到正在执行的初始同命令时返回已有操作。`workspaces.initialize` 只重试未产生视图的失败动作；已有视图由插件重试，主动关闭最后终端不补建。
- `workspaceInitializers` 限同插件、唯一的 server 命令，不能带必填用户输入；初始化身份由核心登记并匹配。保留当前单插件宿主范围，没有扩大到 T-008 的多插件系统。
- 认证继续通过 `commands.retry` 消费旧挑战，传入同一 viewId 和工作区代次。后台工作区的认证保留在所属页“继续认证”，不弹窗打断当前工作区。
- loading/取消页使用真实插件表面；原生表面仅在可见、宿主前台且由隐藏转为显示时取得焦点。表面重建保留旧 PTY 与输入游标，不创建新会话。
- 已退役的整个工作台读写函数只参与历史模型测试，不编译入生产程序。实际偏好文件仅含 schemaVersion/main bounds；旧快照路径阻断不影响运行操作。
- 测试夹具新增不同方法名 `sample.seed` 和不应自动执行的 primary 命令，用实际签名包验证通用调度。属于原定 CS-11/17，不代表已交付第二个业务插件。

目录包冒烟复用 `tests/desktop/package-smoke.mjs`：按 build.json 校验版本/哈希；API 4 分支打开隔离拒绝连接地址，确认首终端错误页、回执代次/操作、内存移动和相同数据重启为空。历史 API 3 分支保留其旧包检查。

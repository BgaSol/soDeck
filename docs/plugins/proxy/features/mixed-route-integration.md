# T-027：网络隧道插件混合连接接入计划

文档就绪：可评审 · 修订：r4 · 更新日期：2026-10-29\
所属：[网络隧道插件](../README.md)、[端口转发](port-forward.md)、[内网代理](network-proxy.md)\
依赖：[F-CORE-031 r2](../../../core/mixed-route.md)、[代码方案r1](../../../technical/mixed-route-implementation.md)、[T-027 r2](../../../tasks/T-027-mixed-route.md)

## 1. 两种代理方向和两项业务

核心路径中的P是用于**到达最终SSH主机T**的外部SOCKS服务；插件创建的本机HTTP/SOCKS监听是给其他客户端使用的入口。这两个生命周期不能混淆。T的SSH建成以后，本插件仍经T的direct-tcpip访问规则中的服务地址。

本轮必须分别覆盖固定端口转发和HTTP/SOCKS内网代理；两者消费同一核心混合路径，但规则、监听、客户端代理协议和业务状态仍由插件负责。UI显示“到达SSH主机的路径”和“通过该主机访问的服务”，不能把P或任一跳板误标为最终访问节点。

## 2. 代码修改

| 文件 | 计划 |
| --- | --- |
| `plugins/network-proxy/src/path.tsx` | 当前三模式标签、单独route.proxy和SSH nodes拼接全部改为API6类型化步骤页。按stepId显示SSH/SOCKS顺序、各段访问来源/回环解释；使用stepCount而非“nodeCount台主机”；严格验证reference/offset，迟到页不混入新路径。 |
| `plugins/network-proxy/src/run.ts` | 保存新RouteMeta和expectedRouteRevision；统一认证错误分类含preset确认，调用net.routes.acquire/refresh前核对运行快照。start中先核实目标/计划再开listener，避免不存在目标时抢占端口。认证失败关闭本次listener、保留可继续的合法挑战；取消关闭group/route/operations。 |
| `plugins/network-proxy/src/run.ts`重连分支 | 保留既有autoReconnect选项和有界退避，只为同一已保存snapshot重新申请，配置变化转needs-attention等待用户应用；遇认证/指纹/preset确认停止自动重试。绝不跳过P或降级直连，也不重放已接收的客户端数据。 |
| `plugins/network-proxy/src/view.tsx`、`editor-dialog.tsx`及`manager.ts` | 启动/重连使用核心统一预算；规则失效/路径变化和认证错误给出具体原因，停用/关闭仍可取消；运行详情显示实际快照而非最新编辑值。 |
| `plugins/network-proxy/src/rules.ts` | 不把路径数组复制进规则。规则仍只保存serverId及业务目标；新core空库后若独立文件中保留旧规则，标明目标缺失，允许用户编辑重新选择或删除，不按同名匹配、不自动启动。 |
| `plugins/network-proxy/src/run-observation.test.ts`、`rules.test.ts`及新增路径组件用例 | 新契约夹具、重复代理步骤分页、start/认证/重连/停止竞态，旧serverId缺失，两个业务类型分别断言。 |
| manifest/package及包锁 | SDK6、`pluginApi:^6.0.0`、`connections.route.v2`/`servers.routes.v2`，network.tcp/stream/resources现有业务语义保持；候选版本0.5.0。 |

本地HTTP CONNECT、SOCKS5客户端请求解析仍在插件 `protocol/` 等现有模块；到达T的外部SOCKS握手不得复制进插件。公共UI复用原组件与apple-design稳定列表/即时反馈；不引入第二个连接路径编辑器。

## 3. 插件专属测试

适用r1，对应MR-AC-01～05。准备隔离A→B→P→T，T网络有S1/S2测试服务且本机/A/B/P不能直接替代T访问；本机新插件监听只用隔离端口，准备HTTP/HTTPS测试服务及客户端。各服务分日志验证真实出口。

| 用例 | 步骤 | 预期 |
| --- | --- | --- |
| PF-MR-01 固定转发 | 新库保存完整混合路线，建立本机端口→T可达S1规则，客户端传送有界双向数据；关闭规则 | 数据经过整条核心链再从T到S1，P只负责到T，不需登录P；停止后listener/组/lease回收，终端连接不被误关 |
| NP-MR-01 内网代理 | 使用插件HTTP/SOCKS本地入口分别访问S1/S2及仅T可解析域名，覆盖HTTP CONNECT与SOCKS5；与固定转发同时运行 | 两业务均按请求从T访问服务；路径P不会被当作客户端代理入口；DNS责任准确、共享配置但网络池独立预算 |
| PROXY-MR-01 展示 | A/B/P/T和含重复代理引用的长链分页，运行中编辑目标配置；读取旧页迟到 | 顺序/类型/来源准确，不只显示一个P，旧页不混入；运行路线保持原reference，新配置单独显示 |
| PROXY-MR-02 认证/重连 | P预设确认、错误代理密码、断B/停止P；开关autoReconnect，改路线；在退避/挑战时停止规则 | 有限重连仅同快照，认证停止自动重试且可由宿主继续，配置变化待应用；取消不复活listener、不回退直连/重放客户端请求 |
| PROXY-MR-03 缺失目标 | 保留插件规则文件，按v7空库启动，尝试启动引用旧serverId规则；重新选择新目标再保存 | 开始监听前明确拒绝缺失目标，无同名猜测，规则可修复；不会导入旧服务器配置 |
| PROXY-MR-04 包/预算 | 核心预算内慢链、多成员network-data池；API6中仅插件A→B更新重启 | 外层命令不先超时；至多4个成员各计整链资源；只该包变化，两业务及终端/文件/监控均正常 |

清理所有listener/group/route/stream和测试客户端、测试服务及凭据；确认外部P进程未被SoDeck停止。原生路径截图、真实数据出口和模拟竞态分别记录。

## 4. 状态与结果

已按本计划完成 API6 路径展示、快照/预算、认证/重连和固定转发/内网代理消费者。实现、证据与验收统一见 T-027；T-014 原有两类业务及其验证保留历史范围。

## 2026-10-29 验收修订计划（已实施）

规则编辑/放弃/删除弹窗采用公共 body/actions，去掉 network-pad 补丁式间距并检查三主题/窄窗/焦点。网络池接统一 SSH 答复后只启动一次监听；取消/失效答复无遗留监听。保留插件所有业务和规则存储职责。

来源：[整体修订 r2](../../../design/connection-editor-refinement.md)，对应 REF-01/04～08；与下方诊断增量合并评审。包版本核对在途版本后递增，不能只更新宿主而宣称本插件完成。具体方案已获确认并实施，结果由T-027 r9及实施记录维护，原r2交付记录保留。

## 逐步诊断增量（r4，已实施）

公共事实与接口见[诊断方案 r1](../../../technical/connection-diagnostics.md)。`run.ts` 为启动/重连及池成员保存核心 diagnosticRef，将插件 attemptId 与核心尝试标识分开；manager 的运行快照带来源/成员/引用，`view.tsx`/`path.tsx` 显示失败摘要和统一宿主详情入口。到达 SSH 服务器失败、本机监听失败、SSH 就绪后目标服务不可达分开，不能在后两种情形把 SSH 全链标红；旧 Run 保留自己的不可变路径。既有最多三次退避和配置/认证停止规则保持，不因诊断增加自动重试或客户端数据重放。公共 body/actions 与新 SDK 能力都进入独立包。

准备：隔离混合路径、固定转发规则及 HTTP/SOCKS 内网代理规则、可控目标服务和多个 network-data 成员，沿用 PF/NP-MR 环境；具体执行证据见T-027结果。

| 编号 | 步骤 | 预期 |
| --- | --- | --- |
| PROXY-DIAG-01 | 固定转发与 HTTP/SOCKS 内网代理分别在路径 SOCKS 处失败；同时终端成功，从规则打开详情 | 规则/成员/attempt 定位准确，network-data 与 session 不串；来源→失败代理→未执行目标清楚，无遗留本机监听。 |
| PROXY-DIAG-02 | 依次注入本机端口占用、SSH 就绪后目标服务拒绝、建连后上游断线 | 分别标监听/业务目标/生命周期故障；已有建连成功事实保留，不虚构具体未知断点。 |
| PROXY-DIAG-03 | 自动重连开关、候选切换、认证等待、取消和编辑配置交错；随后单包更新 | 每轮/每成员可追溯，不被最新全局尝试覆盖；次数/总预算不扩大，取消不复活、不重发客户端数据；仅插件包更新，两业务都验证。 |

关闭所有规则/客户端/组/stream/lease 和测试服务，核对监听回收及终端不受影响；真实流量、原生截图、模拟重连分层记录到 T-027-results。

## 本轮实现版本

插件 0.6.0 已独立构建和打包，使用SDK6.1/UI1.8，声明公共诊断与统一确认能力。运行错误及每条历史保存自己的引用；网络池member与session隔离，本机listen/SSH route/target-service分开，监听失败不挂SSH引用。 本插件定向状态/worker测试、真实混合链原生流程和独立单包更新均有本轮证据，统一见 [T-027结果](../../../tasks/T-027-results.md)。测试候选包只用于隔离更新验证，交付仍使用本段版本；用户验收和发布未发生。

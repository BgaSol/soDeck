# 默认行为与通用终端实施记录

2026-11-09。按已确认的[代码整改方案](plan-default-behavior-terminal-coding.md)实施。代码整改完成后，用户另行授权发布 latest，发行版本为核心 0.16.0、终端 0.6.0；发行来源与五平台验收集中记录在[版本说明](releases/v0.16.0.md)。以下保留整改阶段本机验证，不引用历史版本结果。

## 已实现行为

工作台二级“＋”从 ui.actions/defaultCreateAction 取得行为声明，通过已有 RPC 向插件进程发送 ui.action。终端是默认推荐提供者，工作台不包含 sodeck.terminal 的固定目标或页面创建规则。插件主动通过 ui.openView 申请自己的视图，原 openOnWorkspace 自动布局继续使用。

核心登记声明、初始化支持标记、稳定偏好和有界请求记录。启动插件逐个加载期间暂停推荐选择，全部加载结束后按稳定 key 保存一次；旧偏好不覆盖。5 秒等待超时后查询同一 Receipt，不重发事件；同编号输入变化拒绝，无页面动作不补建。页面已创建后插件报告失败保留已知 viewId。关闭、断线、实例停止使未完成行为失效，迟到申请被拒绝；取消关闭不复活请求。移窗保留 Context，重连和工作区结束清理旧记录。

ui.openView 只使用真实调用插件的声明和实例，通过既有 open_view 登记页面。重复输入返回原页面，关闭后返回 viewClosed；原单例页面仍属于已停止实例时要求先关闭，不将旧资源归给新实例。新入口每 Context 合计最多 4096 条、全局最多 16384 条、缓存预算 16 MiB，满额明确拒绝，历史去重记录不静默清空。

终端解析 null/空对象及 version/title/launch/directoryIntegration 配置。登录 shell 保留设置中的 auto/manual/off；command 只需要 terminal.launch.v1，直接传入 PTY，关闭目录集成且不探测/注入主机 shell。首次挂载保存解析结果；标准库 OnceLock 保证同 view 只初始化一次，不同 view 可以并行。失败缓存至页面释放，构造中 dispose 后清理迟到 PTY。

command 退出后“重新打开终端”保留原命令、标题和目录策略，使用当前 Context、新 view 和新 PTY。旧核心沿用页面 openView 桥并携带参数；缺少命令能力明确拒绝。off 模式忽略 OSC7/633 的 known 报告，只保留未知/关闭状态，不导航主机文件侧栏。核心 metadata 标题上限增至 512 字节，以容纳终端契约的 128 个 Unicode 字符。

## 代码与接口

核心新增 [actions.rs](../src-tauri/src/component_ui/actions.rs)，登记与快照在 [layout.rs](../src-tauri/src/component_ui/layout.rs)，进程声明与停止沿已有 supervisor，关闭与连接失效沿已有生命周期。没有新增服务对象、业务路由、SDK 包装或生产依赖。

终端新增 [creation.rs](../plugins/terminal/src/creation.rs)，[main.rs](../plugins/terminal/src/main.rs) 处理事件、页面申请和单次挂载；UI 仅修改 ui-src，包仍由原构建脚本生成。工作台夹具使用中性 test.workbench 标识，异步帧循环在页面申请期间继续接收核心回调。

实际契约见[插件 API](plugin-api.md#默认行为与进程主动创建页面)，用户操作见[使用指南](usage.md)。新核心不会对旧动作提供者恢复固定终端直开分支；新终端保留旧视图字段与旧核心上的页面入口。

## 本机验证

主机为 Windows x86_64，Rust 原生目标 x86_64-pc-windows-msvc。使用隔离数据目录、隔离回环 OpenSSH 服务、实际独立插件包与 Playwright xterm 页面；没有连接用户业务服务器。

| 检查 | 本次结果 |
| --- | --- |
| npm run check / plugin:check | TypeScript 与核心、插件 Rust 格式检查通过 |
| npm run scripts:test | 90 项通过，包含 Off 模式 OSC7/633 未知状态回归 |
| npm run test | 全量核心测试 175 项通过、10 项专项忽略；新增请求预算单测另行通过 |
| 核心 component_ui 回归 | 24 项通过：声明、旧插件、稳定选择、无页面、其他视图、身份/参数冲突、超时原回执、关闭取消、移窗与重连 |
| cargo test --manifest-path plugins/terminal/Cargo.toml --locked | 4 项单测与 4 项实际插件进程集成测试通过；包含无主机探测、独立能力、并发挂载、迟到释放和失败缓存 |
| node scripts/test-workspace-ui.mjs | 21 项通过，无页面错误；真实延迟回执进入 pending，切换后查询完成，没有再次 invokeAction |
| npm run terminal:input:test | 实际 xterm 输入、粘贴、括号粘贴、取消和主题回归通过 |
| npm run terminal:test | 实际终端包、OpenSSH PTY、指定命令交互与结束、原配置再次打开、WebView 重挂载通过 |
| npm run communication:test | 三个实际插件包组合通过；命令 OSC7 不导航主机，原主机跟随、隐藏、移窗、断线/重连通过 |
| npm run web:build | 生产前端构建通过 |
| npm run plugin:build -- --plugin terminal | Windows x86_64 独立插件包生成并验证原生二进制 |
| npm run build | 最终工作树 Windows x64 核心程序与 MSI/NSIS 安装包构建通过 |
| node scripts/smoke.mjs src-tauri/target/release/sodeck.exe | 最终生产程序的原生窗口启动、工作区移窗、关闭、持久化重开和退出通过，版本/目标检查通过 |

整改未新增生产依赖，发行准备仅更新终端包自身的锁文件版本。select-checks 的既有核心子目录、插件目录和夹具回退规则覆盖新文件，现有工作流会执行相应检查，未增加新的构建体系。

## 实际限制与未验证范围

本机 Windows OpenSSH PTY 对 PowerShell `exit 7` 报告退出状态 0；独立 ssh -tt 客户端得到相同结果，核心按原协议保留服务器报告，不猜测退出码。POSIX 测试分支要求真实退出码 7，需在对应系统执行。

整改阶段尚未完成五平台原生核心/插件包及实际包组合验收；后续发行执行结果见上述版本说明，本机结果不能替代其他平台。未安装本次 MSI/NSIS 到用户环境。真实 Bash/Zsh 脚本本次未修改，本机没有执行 POSIX shell 专项。Docker 插件、容器 exec 命令生成和容器 TTY 清理不在本次任务范围。

默认行为选择界面尚未新增；极长期 Context 达到请求记录预算时需关闭并重开工作区。命令终端首版只允许 directoryIntegration=off。以上限制与确认方案一致。

# 组件通信与共享状态实施记录

2026-11-09，按[功能设计](plan-component-communication.md)和[代码方案](plan-component-communication-coding.md)整体实现共享状态、两个业务联动及生命周期接入，没有新增生产依赖、权限系统或数据库。随后随核心 0.13.0 与三个插件独立发行，发布验证见[版本说明](releases/v0.13.0.md)。参数以[插件 API](plugin-api.md)为准，用户配置见[使用说明](usage.md)。

## 实现行为

[StateHub](../src-tauri/src/shared_state.rs) 在核心进程保存运行事实，以完整 workspace/server/connection/generation Context 隔离。核心从实际实例和逻辑 view 推导生产者，页面绑定真实 surface/window；插件共同提供主题，但只能更新自己的贡献。Core 的 `workspace.presentation.v1/current` 来自工作台实际活动中央页，插件不能覆盖它。

条件写入使用 u64 十进制修订号；null、未存在与删除墓碑分别表达。重复相同值不刷新时间。订阅与冻结初始快照原子完成，分页读取和确认后接续变化；变化合并为不可变批次，拆帧后最终 ACK 才推进游标，丢失回执和重复投递可恢复。租约 60 秒、快照空闲 30 秒到期。提前取消保留有界客户端 ID 标记，防止晚到初始化复活。

进程通过版本化 RPC 使用可选 `sharedState.v1`；[Rust SDK](../crates/plugin-client/src/shared_state.rs) 在 RPC reader 接收并原子应用批次，调用方每 30 秒 renew，失效后重建。页面通过定向事件和真实挂载桥使用包内 [JS SDK](../plugins/shared/state.js)，自动分页、拼批、确认、续订和重同步。页面计时器延迟时，读取 valid 也立即检查租约。旧核心没有能力时禁用增强，原有业务继续工作。

投递最多保留 64 个待处理对象，每实例最多 4 个在途请求，单次等待 5 秒、最多三次尝试；写入只唤醒分发，不等待消费者。通知到期回收不改变既有保存/释放调用对真实结果的保留规则。断线与冻结分别记录，既有读取、续订、ACK 和释放可继续；重连丢弃旧 Context。view/实例停止撤销贡献，surface 销毁仅清理页面订阅；移窗提交清理旧窗口挂载，同窗口排序保留原挂载。宿主丢失、关闭取消和最终释放均接入现有生命周期。

额度为记录 16 KiB、进程帧 32 KiB、页面帧 24 KiB、每 Context 数据 4 MiB、全局当前/临时数据各 32 MiB、每 Context 4096 个记录与墓碑、每实例 32 个订阅、每订阅待发与在途合计 1 MiB。最终 RPC/页面 JSON 信封长度有测试；旧快照与在途批次引用的值继续计费。超限保留旧事实或要求重新同步。计费约束保留的数据，不等同于进程 RSS。

## 两个业务联动

终端按原 PTY Context 发布 `terminal.location.v1/current`。xterm 只在真实输出帧解析 OSC 7，验证已配置主机名、URI 编码和绝对路径；写入与追赶完成后提交最新观测。屏幕快照恢复不重新生成报告，输出缺口、退出、断线或配置改变使目录转为 unknown。串行 get/CAS put 合并最新目标，较早 offset 不覆盖新报告；不猜提示符、不执行生产探测命令、不修改远端 shell 配置。

文件侧栏默认开启“跟随终端”，开关通过现有组件设置保存。只消费 Core 指定的活动终端；后台终端、未知目录和隐藏面板不改变位置。同路径报告保留手动导航，重新启用或显示时定位当前候选。resolve、stat、目录加载、选中与滚动均检查导航和 Context 序号。后台采集变化只取消仍在执行的跟随，不取消用户手动定位。

核心 `files.location.v1` 将默认文件系统的 POSIX 或原生 Windows 盘符绝对路径转为远端表示，再由当前文件服务 realpath/stat 验证。UNC、相对路径和未知 filesystemView 返回 unsupportedLocation，不根据客户端系统猜路径。

系统状态在原后台采集调度中发布 `filesystem.mounts.v1` 与 `filesystem.capacity.v1`，不要求打开其页面，不加快采样，不在业务锁内执行 RPC。Linux 独立解析完整 mountinfo，容量沿用原采集范围；Windows 使用固定本地卷 DeviceID 与主路径，macOS/BSD 使用 df 部分信息。扫描覆盖与 publishing/synced/limited 分开；部分或失败扫描不把缺项当卸载。同路径身份变化生成新 mountId 并撤销旧容量，每条容量保留自身采集时间。冲突或未知结果重新核对后提交最新意图。发布缓存预留未来确认副本后限在 960 KiB，超限回退旧观测并公开 limited。

文件树只组合同来源、同 mountId 容量，多个来源须选择；挂载映射按 Context/source/mountId 缓存，最多两路并发解析，容量变化不重复扫描。更新直接修改现有节点文本与详情，保留选择、展开和滚动；未知值不显示为 0，拓扑与容量分别判断过期。窄侧栏工具栏可换行，挂载文字截断后仍有完整悬停详情。

## 验收证据

本机 Windows x86_64，使用固定 Rust 1.99.1 与锁文件。Cargo 已安装但未在终端 PATH，检查命令仅补入本机 Cargo 路径，没有修改工程入口。真实 SSH 测试使用隔离 OpenSSH 与数据目录，不使用日常连接资料。

| 检查 | 实际结果 |
| --- | --- |
| `npm run check`、`npm run plugin:check`、SDK 格式 | TypeScript 与相关 Rust 格式通过 |
| `npm run test` | 最终代码 167 项通过、10 项按原配置 ignored；含真实两个插件子进程发布/订阅、来源停止清理，以及状态、设置、连接、关闭与工作区回归 |
| `npm run scripts:test` | 87 项通过；含共享数字/null/墓碑/分帧、租约、旧能力与手动定位回归 |
| SDK 与插件 Rust 测试 | SDK 3 项、文件 4 项、系统状态 23 项通过；系统状态 1 项 Linux Docker 测试 ignored；终端编译通过，无 Rust 单元用例 |
| `npm run terminal:input:test`、`npm run terminal:test` | 原真实 xterm 输入/剪贴板，以及 SSH/PTY、多页、恢复回归通过 |
| `npm run files:test` 与 `--files-ui-only` | 原文件协议及实际文件 UI 通过，包括读写、冲突、跨窗口草稿、传输、窄侧栏与三主题 |
| `npm run system-status:test` | 真实 Windows 采集与状态 UI 通过，包括失败保留、分页、隐藏采集、断线、30 秒采样间隔 |
| `npm run settings:test` | 4 个停用插件设置页、实际离线字体、冲突/草稿与 12 种响应布局通过 |
| 工作区与连接 UI | 20 项工作区检查及连接页面实际失败/取消/重试通过，无页面错误 |
| `npm run build -- --no-bundle -- --locked` | 正式前端与 Windows x86_64 release EXE 构建通过 |
| `node scripts/smoke.mjs src-tauri/target/release/sodeck.exe` | 两次原生启动、WebView 渲染、移窗、关闭拆出窗口、资料持久化与重开、退出通过；不是安装器解包验证 |
| `npm run communication:test` | 最终三插件独立原生包、实际 RPC/PTY/SFTP 和定向页面事件通过，11 项组合检查通过 |

组合脚本 [test-component-communication.mjs](../scripts/test-component-communication.mjs) 覆盖真实 PowerShell OSC 7 → 核心 → 文件定位，以及后台系统采集 → 核心 → 挂载/容量增强。另验证手动干预、开关持久化、多终端切换、恢复不重报、取消旧 resolver、隐藏/显示、移窗往返、断线与新 Context。本机报告和截图在忽略目录 `artifacts/communication/result.json` 与 `plugins.png`，不是 GitHub 验收记录。文件单插件测试覆盖生产插件缺席，SDK 明确验证无能力时不发新 RPC。

实际发现并修复：Windows 完整 EncodedCommand 超过 cmd.exe 的 8191 字符限制（发送当前采集组，并加各组长度回归）；Windows 根路径被截成相对盘符；新按钮使窄侧栏溢出；新设置挤出字号控件；同窗口排序错误撤销 surface；共享变化误取消手动定位。现有真实 UI 回归和新增确定性测试保留这些检查。

最终 EXE 烟测退出时两次输出 WebView2 `Failed to unregister class Chrome_WidgetWin_0 / 1412` 诊断；程序返回 0，当前烟测协议、资源退出和重开验证通过。未据此声称安装器验收通过。

Check 工作流已登记通信组合；公共存储、SDK 或状态桥变化选择核心、页面与三个插件。终端开发构建及终端/文件独立包复制公共 state.js，核心打包不依赖业务插件构建。三插件 Windows x86_64 独立包已验证原生架构与完整资源。

## 支持边界与未执行范围

Windows 本机 PowerShell 默认文件系统的可靠报告已实际验证。主机名配置是用户确认 shell 与文件浏览同一文件系统的选择；OSC 7 本身不能识别同主机名的容器、chroot 或嵌套 shell，进入这些环境前须关闭跟随。未知视角、不同主机与 UNC 被拒绝，第一版不支持这些映射。没有默认注入 bash/zsh/PowerShell 集成，用户须配置正确百分号编码的报告。

Linux mountinfo 解析与部分扫描/身份变化测试通过，本机未执行远端 Linux/macOS 真实三插件组合、bash/zsh 实际报告或 Docker 专项。Windows 固定本地卷和 macOS/BSD df 均声明 partial。采样之间发生又恢复的挂载变化无法观察；观测都有新鲜度期限，不声称实时事务或完整容量覆盖。

实现阶段的上述本机检查不替代原生包验证。后续正式发布已执行同一源码的 Check、五平台核心/插件打包、签名、实际包启动/退出及远程组合；Linux x86_64 真实 bash 与三插件联动通过。源码、原始运行、正式 Release 和实际执行/跳过范围统一保存在[核心 0.13.0 版本说明](releases/v0.13.0.md)及插件 CHANGELOG，不在此重复维护发行记录。

返回[文档索引](README.md)。

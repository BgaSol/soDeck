# 0.4.3

- 规则列表、详情与表单使用独立打包的 morphdom 资源，删除重复递归 diff，附 MIT 许可证。
- 按压队列随挂载容器结束；轮询结果核对挂载代次，卸载后的快照不再绘制页面。
- 表单切换自动端口时保留其他输入节点；网络监听与业务生命周期保持原规则。

包来源：[2a510727c8fba367a638b9e79fc8468323f68256](https://github.com/BgaSol/soDeck/commit/2a510727c8fba367a638b9e79fc8468323f68256)；原始独立打包与验收：[43373715746](https://github.com/BgaSol/soDeck/actions/runs/43373715746)。Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 五平台原生归档的架构、摘要、资源、初始化、视图创建/释放与退出全部一次通过，包含独立 DOM 运行资源和许可证。

[来源 Check](https://github.com/BgaSol/soDeck/actions/runs/43373679922) 全部相关检查通过，包含五平台原生源码及 Linux x86_64/ARM64 的真实 SSH/SOCKS/curl、进程与网络界面回归。本地真实网络专项同时通过。与本轮核心 0.23.11、terminal 0.8.5、files 0.11.1 的[原包远程组合](https://github.com/BgaSol/soDeck/actions/runs/43374667881)五平台一次通过，包含真实网络通道、移窗与重开退出。

发行：[network-proxy 0.4.3](https://github.com/BgaSol/soDeck/releases/tag/plugin-network-proxy-v0.4.3)，非 Latest。六个附件沿用原始 run；发布前只同步说明与更新清单中的说明，五份程序归档字节保持不变。摘要与本轮来源见[发行附件记录](../../docs/releases/v0.23.11-assets.json)。系统 IME/触控板/DPI、生产服务器、正式版实际升级和 netem 不计为已验证。

# 0.4.2

- 插件客户端使用标准库有界逐行读取，保留超长控制帧立即退出，Interrupted 自动重试；网络通道和清理策略保持原有契约。

未新增或升级生产依赖，核心 API 与 RPC 字段保持兼容。

包来源：[90d49f4c7ef509f89025696a71ddddebfec99d51](https://github.com/BgaSol/soDeck/commit/90d49f4c7ef509f89025696a71ddddebfec99d51)；原始独立打包与验收：[43083461261](https://github.com/BgaSol/soDeck/actions/runs/43083461261)。Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 五平台原生归档的架构、摘要、资源、初始化、视图创建/释放与退出全部一次通过。

[来源 Check](https://github.com/BgaSol/soDeck/actions/runs/43083351828) 的五平台源码及 Linux x86_64/ARM64 真实 SOCKS/curl、进程与网络界面回归通过。[原包组合](https://github.com/BgaSol/soDeck/actions/runs/43088114019) 配合核心 0.23.5、terminal 0.8.4、files 0.10.3 在五平台通过远程流程与重开；组合脚本来自仅增加诊断的 330dc90，程序来源仍为上述提交。

发行：[network-proxy 0.4.2](https://github.com/BgaSol/soDeck/releases/tag/plugin-network-proxy-v0.4.2)，非 Latest。六个附件沿用原始 run，发布前只同步说明与更新清单的验收记录，五份程序归档字节保持不变。未执行本版历史版本升级、netem 或生产服务器专项。

# 0.4.1

- 构建与实际归档验收分任务执行，失败复验保留原包。
- 原生归档统一检查资源、初始化、视图生命周期和退出；发布前绑定来源与摘要，拒绝覆盖不同内容的同版本附件。
- 插件 API 与在线更新协议保持兼容。

包来源：[a52157346409d4457c4b00b5bbd3364df86df36c](https://github.com/BgaSol/soDeck/commit/a52157346409d4457c4b00b5bbd3364df86df36c)；原始打包与验收：[42914263101](https://github.com/BgaSol/soDeck/actions/runs/42914263101)。Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 全部原生构建，最终归档的摘要、资源、初始化、视图释放和退出通过；上传与验收独立执行。

[来源提交 Check](https://github.com/BgaSol/soDeck/actions/runs/42914202961) 全部通过，包含五平台核心/插件/共享客户端源码测试及相关浏览器、SSH/SFTP、终端、网络、Docker、通信回归。首次草稿索引延迟导致收尾失败，重跑 release 成功，未重编译或覆盖包。

发行：[network-proxy 0.4.1](https://github.com/BgaSol/soDeck/releases/tag/plugin-network-proxy-v0.4.1)，非 Latest。[真实正式版升级 0.4.0 → 0.4.1](https://github.com/BgaSol/soDeck/actions/runs/42919809565) 五平台通过，宿主固定为核心 0.23.1，验证下载校验、升级前原进程保留、重启激活及设置/服务器资料保留。没有将源码或包生命周期测试算作生产服务器、真实 IME/DPI/Wayland 或所有设备兼容验收。

[正式包组合](https://github.com/BgaSol/soDeck/actions/runs/42919779115) 与核心 0.23.1、terminal 0.8.2、files 0.10.1、network-proxy 0.4.1 在五平台通过 SSH/PTY/SFTP、网络、工作台与重开；系统桌面测试的具体范围及 macOS 默认关联收据跳过见[核心记录](../../docs/releases/v0.23.1.md)。

# 0.4.0

保存只保存规则，删除需先确认监听清理完成，移除保存并重启和停止后自动删除。查询失败保留原监听并显示待确认；后台 RPC 不占用整个应用状态锁，停止等待实际 socket 和子任务退出。

依赖核心 `net.listenerLifecycle.v1`。 API 主版本保持 1；缺少能力时初始化明确拒绝，不回退到旧流程。本版已独立发行；核心需先升级至 0.23.0 并重启，再升级插件。尚未补齐的故障组合见[剩余验收](../../docs/plans/operation-outcomes.md)。

包来源：[`fcf4f44`](https://github.com/BgaSol/soDeck/commit/fcf4f446d0e588c16da2187473c2c22a63d31f7c)；原始独立打包：[Package plugin #42883702381](https://github.com/BgaSol/soDeck/actions/runs/42883702381)；发行：[network-proxy 0.4.0](https://github.com/BgaSol/soDeck/releases/tag/plugin-network-proxy-v0.4.0)，本插件最新正式版本。五平台均为原生产物，下载原五份归档并核对 SHA-256、更新清单、包内版本、UI 入口及架构，直接发布同一草稿的六个附件。[Check #42885519869](https://github.com/BgaSol/soDeck/actions/runs/42885519869) 与 [Docker 补测 #42887442456](https://github.com/BgaSol/soDeck/actions/runs/42887442456) 对相同产品源码的五平台 Rust 及相关普通业务复验通过，后续仅修正测试，插件程序未改变。

[Verify native remote plugins #42885589898](https://github.com/BgaSol/soDeck/actions/runs/42885589898) 配合核心 0.23.0、终端 0.8.1、文件 0.10.0 和网络隧道 0.4.0，完成五平台实际包的启动、对应远程业务、移窗、重开及退出。下载的 Windows x86_64 进程另验查询失败、停止不被查询阻塞、明确保存/启动/删除与真实端口清理。未重复历史版本在线升级、netem、生产服务器或 IME/DPI 等深度原生专项。

发布标签触发的[重复打包 #42888179780](https://github.com/BgaSol/soDeck/actions/runs/42888179780) 已取消，不是发行来源；原六个附件的 ID、大小与摘要保持不变。

# 0.3.1

共享客户端改用固定 base64 0.23.1；保留标准编码、严格解码、原始字节分页和既有 API。本版同步锁文件并重新独立打包，插件业务和核心 API 主版本不变。

实现见[接入记录](https://github.com/BgaSol/soDeck/blob/8115fdcd5bed4108021a729da605fbde77afc2af/docs/implementation-base64-opener-adoption.md)；核心与插件继续独立发行，核心包不含业务插件。

包来源：[`0abaf3e8ccc2045e437dac723d898eca87759f6c`](https://github.com/BgaSol/soDeck/commit/0abaf3e8ccc2045e437dac723d898eca87759f6c)；原始独立打包：[Package plugin #42779058143](https://github.com/BgaSol/soDeck/actions/runs/42779058143)；正式发行：[network-proxy 0.3.1](https://github.com/BgaSol/soDeck/releases/tag/plugin-network-proxy-v0.3.1)，非 Latest。

五个原生目标独立构建、解包、包内进程启动与退出通过；ARM64 使用原生程序。[Check #42807748154](https://github.com/BgaSol/soDeck/actions/runs/42807748154) 复验相同插件运行源码的五平台测试与普通网络业务。实际包配合核心 0.22.1、终端 0.8.0 和文件 0.9.0，在 [Verify native remote plugins #42814180548，attempt 2](https://github.com/BgaSol/soDeck/actions/runs/42814180548/attempts/2) 完成五平台 TCP/SOCKS5 正文、移窗、端口释放与退出；首轮输入解析失败未启动业务。另下载原五份归档，核对 SHA-256、清单、更新地址和原生二进制架构，直接发布原六个附件。netem、历史版本在线升级与用户生产服务器未重复；不重建已通过包或覆盖正式附件。

正式发布时 GitHub 创建插件标签，触发的[重复打包 #42817942627](https://github.com/BgaSol/soDeck/actions/runs/42817942627) 已取消；不是发行来源。正式六个附件的 ID、大小与摘要、公开更新清单仍与原始草稿一致。

# 0.3.0

接入核心 0.17.0 的工作台与 UI v2。网络隧道工具可停靠中央、左右侧栏或独立窗口；移动和隐藏复用原规则与监听，局部关闭按统一协调器返回仍有效的工作台，全服务器关闭时结束连接并释放端口。

逻辑创建、候选只读、冻结同步和激活分开，避免新旧表面同时输入。请先升级核心，再升级插件并重启；核心 API 主版本保持 1，没有新增生产依赖。

包来源：[`7a3747e`](https://github.com/BgaSol/soDeck/commit/7a3747e009bf0ee51ec5be5d1ba8c72dc678cf2d)；原始独立打包：[Package plugin #42573205282](https://github.com/BgaSol/soDeck/actions/runs/42573205282)；发行：[network-proxy 0.3.0](https://github.com/BgaSol/soDeck/releases/tag/plugin-network-proxy-v0.3.0)，非 Latest。五平台原生包与包内进程生命周期验收通过；源码与普通网络任务、实际包远程组合、Windows 四插件原生交互以及未验证范围统一见[核心 0.17.0](../../docs/releases/v0.17.0.md)。插件业务源码与原包一致，复用同一份归档。

# 0.2.2

加深多巴胺主题的辅助文字，连接入口说明在黄色背景上达到小字对比度要求。

规则列表与详情刷新保留节点、键盘焦点、展开状态和检查输入，按住启停按钮跨刷新只执行一次，移出后取消。复制实际入口给出就地反馈并跨刷新保留，分页有明确的可访问名称。客户端连接失败与整条规则状态分开显示，移除界面中写死的旧版本号。

# 0.2.1

输入框文本菜单在缩窗和对话框内自动避让，保留选区、键盘操作和复制粘贴。菜单资源随插件独立打包，使用时无需联网加载。

SOCKS5 报文改用固定版本协议库。问候未完成或无效时直接关闭，不发送 CONNECT 阶段错误帧；认证方式拒绝只回复一次。入口检查支持三种绑定地址响应，并限制每个逻辑响应的总读取时间。目标连接和正文转发继续复用核心会话。

# 0.2.0

新增网络隧道独立设置页，配置新规则使用的监听地址、端口策略和 SOCKS5 认证默认值；修改偏好保留现有规则和正在运行的监听。页面使用共享三主题滚动条及应用文本菜单。组件设置需要核心 0.9.0，请先升级核心，再升级插件并重启；TCP/SOCKS5 仍复用核心连接并独立管理生命周期。

# 0.1.0

- 新增独立网络隧道插件：固定 TCP 转发和 SOCKS5 CONNECT，复用当前服务器及 SSH/SOCKS5 混合跳板路径。
- 新增按服务器管理的规则、宿主编辑弹窗、立即启停、实际入口复制、连通性检查、连接数和流量统计。
- 收起和移窗保留原运行，断线、关闭和插件退出后清理资源；重开后手动启动。
- 支持浅色、深色和多巴胺主题。核心须提供 `net.tcp.v1` 与 `ui.dialog.v1`。

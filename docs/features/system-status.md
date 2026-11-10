# 系统状态

系统状态插件复用当前认证会话，读取远端设备信息、CPU、内存、文件系统及网络/磁盘指标，不要求远端 Agent。默认在右侧展开；隐藏、切换服务器、最小化和移窗不决定采集开关。

## 采集与显示

| 数据 | 当前采集方式 |
| --- | --- |
| 设备规格 | 连接及手动刷新读取 |
| CPU、内存、网络/磁盘计数 | 默认 5 秒，组件设置可选 3/5/10/30 秒 |
| 拓扑 | 30 秒 |
| 容量、uptime 校准 | 60 秒 |

每工作区串行，全插件最多四个远端任务。暂停只影响当前工作区；暂停时刷新仅采一轮并继续保持暂停。合并重复刷新，错过周期后跳过积压，不无限补采。慢命令可以推迟动态采样，界面显示实际观察时间与成功时间。

趋势最多保留五分钟/100 点，并按缓存预算收缩。速率来自计数差与真实时间窗口；计数回退、重连、设备变化和采样缺口不补零。断线、暂停和过期保留最后读数并明确标注，不把旧数据当实时值。

Linux/OpenWrt 走 procfs 与 POSIX sh，不 source os-release、不依赖远端 Python/SFTP；容量只查询可明确识别的本地文件系统，排除 NFS/CIFS。Windows 使用 PowerShell CIM/性能计数器；macOS/BSD/其他 Unix 使用各自可用的命令路径。缺少可靠来源的字段显示不可用。

Windows 网络计数器实例尚未可靠映射为物理网卡；macOS/BSD 缺少统一口径时不显示总内存使用率或部分磁盘 I/O。编入采集器不代表真实 OpenWrt、Dropbear、各 Windows SSH shell 或 BSD 设备已经验证。

## 当前界面约束

服务器概况和刷新/暂停共用顶部区域，网卡选择显示当前项与全部 IP，长 IPv6 可换行。该组界面自 0.4.2 发行，版本与验证范围见[插件 CHANGELOG](../../plugins/system-status/CHANGELOG.md)。

指标在一个连续滚动面中按细线分区，继承宿主三主题、字体和描边；不另设数据卡片或阴影。窄面板缩减细节并换行，设备详情转为单列。具体色值、字号和间距以插件样式为唯一实现来源。

- 曲线只连接真实连续样本，CPU/内存使用 0–100 百分比，网络按所选接口历史峰值缩放。
- 指针、方向键和 Home/End 查看采样，Esc 隐藏提示；暂停、断线或过期曲线以较浅虚线区分，成功时间持续显示。
- 挂载容量显示已用/总量、可用量与单位，已知使用率才画只读容量条；达到 85% 同时显示注意色和“空间偏紧”文字。
- 原生 select、details、meter 保留可访问名称和焦点；减少动态效果关闭过渡，强制颜色模式采用系统颜色。未知值必须使用文本说明。

## 生命周期与入口

逻辑 view 独立于 iframe，隐藏停止页面轮询但继续后台采集。断线/重连代次隔离旧结果，关闭工作区或插件后释放受管任务；取消单个命令不关闭共享连接。容量共享经核心发布，文件插件只消费已确认的快照。

- 实现：[scheduler.rs](../../plugins/system-status/src/scheduler.rs)、[collectors.rs](../../plugins/system-status/src/collectors.rs)、[metrics.rs](../../plugins/system-status/src/metrics.rs)、[shared_state.rs](../../plugins/system-status/src/shared_state.rs)、[view.js](../../plugins/system-status/ui/view.js)、[charts.js](../../plugins/system-status/ui/charts.js)、[style.css](../../plugins/system-status/ui/style.css)。
- 核心能力：[session_exec.rs](../../src-tauri/src/session_exec.rs)、[后台 view](../../src-tauri/src/component_ui/background.rs)、[共享状态](../../src-tauri/src/shared_state.rs)。契约见[插件 API](../plugin-api.md)。
- 回归：[系统状态夹具](../../src-tauri/tests/fixtures/system_status_flow.rs)、[UI](../../scripts/test-system-status-ui.mjs)、[实际包](../../scripts/test-plugin-package.mjs)、[跨插件联动](../../scripts/test-component-communication.mjs)。源码和设备验证分开记录，运行方法见[开发指南](../development.md)。

返回[文档索引](../README.md)。

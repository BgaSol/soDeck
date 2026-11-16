# 系统状态

系统状态插件复用当前认证会话，读取远端设备信息、CPU、内存、文件系统及网络/磁盘指标，不要求远端 Agent。默认在右侧展开；隐藏、切换服务器、最小化和移窗不决定采集开关。

## 采集与显示

| 数据 | 当前采集方式 |
| --- | --- |
| 设备规格 | 连接及手动刷新读取 |
| CPU、内存、网络/磁盘计数 | 默认 5 秒，组件设置可选 3/5/10/30 秒 |
| 拓扑 | 30 秒 |
| 容量、uptime 校准 | 60 秒 |

每工作区串行，全插件最多四个远端任务。暂停只影响当前工作区；暂停时刷新仅采一轮并继续保持暂停。合并重复刷新，错过周期后跳过积压，不无限补采。慢命令可以推迟动态采样，界面显示实际观察时间与成功时间。Linux 容量每轮最多读取 16 个路径并轮转，挂载多时单项观察间隔可能超过 60 秒。

趋势最多保留五分钟/100 点，并按缓存预算收缩。速率来自计数差与真实时间窗口；计数回退、重连、设备变化和采样缺口不补零。断线、暂停和过期保留最后读数并明确标注，不把旧数据当实时值。

Linux/OpenWrt 走 procfs 与 POSIX sh，不 source os-release、不依赖远端 Python/SFTP；容量只查询可明确识别的本地文件系统，排除 NFS/CIFS。Windows 使用 PowerShell CIM/性能计数器；macOS/BSD/其他 Unix 使用各自可用的命令路径。缺少可靠来源的字段显示不可用。

Windows 网络计数器实例尚未可靠映射为物理网卡；macOS/BSD 缺少统一口径时不显示总内存使用率或部分磁盘 I/O。编入采集器不代表真实 OpenWrt、Dropbear、各 Windows SSH shell 或 BSD 设备已经验证。

## 当前界面约束

服务器概况和刷新/暂停共用顶部区域，网卡选择显示当前项与全部 IP，长 IPv6 可换行。该组界面自 0.4.2 发行，版本与验证范围见[插件 CHANGELOG](../../plugins/system-status/CHANGELOG.md)。

指标在一个连续滚动面中按细线分区，继承宿主三主题、字体和描边；不另设数据卡片或阴影。窄面板缩减细节并换行，设备详情转为单列。具体色值、字号和间距以插件样式为唯一实现来源。

- 曲线只连接真实连续样本，CPU/内存使用 0–100 百分比，网络按所选接口历史峰值缩放。
- 指针、方向键和 Home/End 查看采样，Esc 隐藏提示；暂停、断线或过期曲线以较浅虚线区分，成功时间持续显示。
- 文件系统采用两行概览，显示路径、使用率、已用/总量及短容量条；展开单项查看完整路径、可用量、类型、只读属性和实际容量观察时间。刷新保留详情与焦点。
- 原生 select、details、meter 保留可访问名称和焦点；减少动态效果关闭过渡，强制颜色模式采用系统颜色。未知值必须使用文本说明。

## 文件系统分组

主列表默认保留根卷、所有真实容量告警，再补普通存储至三项；可展开全部主要存储。有效容量使用率达到 85% 时显示注意色和“空间偏紧”，告警超过三项仍全部进入主列表，超过单页预算时保留完整告警数和翻页入口。

“其他挂载”默认折叠，按需读取辅助分区和系统挂载。已确认本地分区上的精确 `/boot`、`/boot/efi` 正常时进入辅助分区；容量偏紧时自动回到主列表，恢复后重新折叠。Linux 已确认 tmpfs/ramfs 的 `/run`、`/run/credentials/…`、`/run/snapd/ns`、`/run/user/<数字 uid>`、`/dev/shm` 进入系统挂载；有效高占用同样提升为告警。

已确认 `squashfs` 且只读的镜像进入系统挂载，显示“只读镜像”和镜像大小；正常 100% 不产生空间告警。运行挂载 0/0 显示“容量不适用”，不画容量条；普通存储缺失或异常容量显示“容量不可读”。根卷保持可见。普通只读数据卷、自定义 tmpfs、可写 overlay、`/snap` 路径下的业务卷不因路径前缀或只读属性自动豁免；无法确认用途时保守留在主列表。

分类在完整有界缓存上执行后再分页，每页至多 32 项并受原有响应预算约束。内部 `list(kind="filesystems")` 可选 `group=overview/primary/auxiliary/system`；不传 group 保留原始完整容量列表。分组投影附加 `mountKey`、`filesystemType`、`readOnly`、`displayGroup`、`capacityState`、`spaceWarning`；快照提供 `filesystemSummary`，分页提供修订与游标。挂载身份、分组或顺序变化时页面从该组第一页重读，不拼接不同修订的页面。导航保存“其他挂载”的展开状态。

容量观察与当时确认的挂载身份绑定。同路径换挂载后清空旧容量等待新观察；只读属性变化重新分类，不能更新旧容量时间。Windows 以卷 ID 关联，类型沿用现有 `kind`、只读未知；macOS/BSD 使用已有部分拓扑，类型或只读来源缺失时不猜测。共享发布继续提供完整挂载与容量，界面隐藏不会删除文件插件可读取的记录。此行为随插件 0.4.5 交付，验证范围见[插件 CHANGELOG](../../plugins/system-status/CHANGELOG.md)。

## 生命周期与入口

逻辑 view 独立于 iframe，隐藏停止页面轮询但继续后台采集。断线/重连代次隔离旧结果，关闭工作区或插件后释放受管任务；取消单个命令不关闭共享连接。容量共享经核心发布，文件插件只消费已确认的快照。

- 实现：[scheduler.rs](../../plugins/system-status/src/scheduler.rs)、[filesystem.rs](../../plugins/system-status/src/filesystem.rs)、[collectors.rs](../../plugins/system-status/src/collectors.rs)、[metrics.rs](../../plugins/system-status/src/metrics.rs)、[shared_state.rs](../../plugins/system-status/src/shared_state.rs)、[view.js](../../plugins/system-status/ui/view.js)、[charts.js](../../plugins/system-status/ui/charts.js)、[style.css](../../plugins/system-status/ui/style.css)。
- 核心能力：[session_exec.rs](../../src-tauri/src/session_exec.rs)、[后台 view](../../src-tauri/src/component_ui/background.rs)、[共享状态](../../src-tauri/src/shared_state.rs)。契约见[插件 API](../plugin-api.md)。
- 回归：[系统状态夹具](../../src-tauri/tests/fixtures/system_status_flow.rs)、[UI](../../scripts/test-system-status-ui.mjs)、[实际包](../../scripts/test-plugin-package.mjs)、[跨插件联动](../../scripts/test-component-communication.mjs)。源码和设备验证分开记录，运行方法见[开发指南](../development.md)。

返回[文档索引](../README.md)。

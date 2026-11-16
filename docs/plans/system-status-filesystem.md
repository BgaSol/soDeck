# 系统状态文件系统整改

状态：待发行验收。2026-11-16 已实现分类、身份关联、分页、两行界面及正式回归；剩余五平台原生包和 Latest 发布。当前行为已回写功能文档，本计划在交付后删除。

## 目标与范围

文件系统区域默认展示主要存储和需要处理的容量风险。Snap 只读镜像的正常满容量不产生“空间偏紧”；系统运行挂载与正常辅助分区收进可展开的“其他挂载”。保留紧凑概览、真实容量、完整路径、单项观察时间和现有失败提示。

改动限于系统状态插件的容量展示、内部数据投影及相关回归。不新增生产依赖、核心能力、用户权限流程、隐藏规则设置或挂载管理功能。完整挂载拓扑与容量仍由现有共享接口发布，文件插件不消费经过界面过滤的列表。采集频率、远端命令配额及网络文件系统查询边界沿用现状。

## 缺陷依据

用户截图中 `/snap/core20/2318`、其他 Snap 版本等显示 100% 并统一提示“空间偏紧”；`/run/credentials/systemd-sysusers.service` 显示 0 B / 0 B；正常 `/boot`、`/boot/efi` 和运行挂载与数据盘等权铺开。相关可复现数据应直接进入回归夹具，不依赖临时截图或原型文件。

源码原因：

- [collectors.rs](../../plugins/system-status/src/collectors.rs) 的 `mount_paths()` 把 `squashfs`、`tmpfs`、`ramfs` 列为可查询的本地文件系统；这是采集边界，不能拿它代替默认展示规则。
- [shared_state.rs](../../plugins/system-status/src/shared_state.rs) 的 `linux_mounts()` 已解析挂载身份、类型、来源与只读属性；[metrics.rs](../../plugins/system-status/src/metrics.rs) 将这些信息保存在 `mountTopology`，容量记录另存于 `filesystems`。
- [scheduler.rs](../../plugins/system-status/src/scheduler.rs) 的 `snapshot()` 移除完整拓扑和容量列表；`list()` 只分页返回原始容量记录，尚未合并挂载属性。
- [view.js](../../plugins/system-status/ui/view.js) 的 `lists()` 只按使用率达到 85% 决定告警，对当前页全部挂载逐项展开。先分页再在浏览器筛选会把后续页的主要存储与告警遗漏。

Linux 容量每轮最多查询 16 个路径并轮转；挂载多时，同一项的实际观察间隔可能超过容量组的 60 秒周期。截图里的旧容量时间必须继续如实显示，本次不借展示整改改写采样时间或承诺每项每 60 秒更新。

## 可验收行为

| 场景 | 默认位置 | 容量与告警行为 |
| --- | --- | --- |
| `/`、`/mnt/data` 等普通本地存储 | 主列表 | 保留路径、已用/总量、使用率、容量条；已确认容量使用率达到 85% 时提示 |
| 已确认 `squashfs` 且只读的 Snap 镜像 | 其他挂载 → 系统挂载 | 显示“只读镜像”和镜像大小；100% 不计入空间偏紧；展开可看原始占用、类型、只读属性 |
| 已确认的系统运行挂载，正常容量 | 其他挂载 → 系统挂载 | 保留读数；不把它与普通数据盘等权展开 |
| 已确认的运行挂载，容量为 0/0 | 其他挂载 → 系统挂载 | 显示“容量不适用”；不画使用率条，不当作空间耗尽或读取失败 |
| 正常 `/boot`、`/boot/efi` 本地分区 | 其他挂载 → 辅助分区 | 保留容量，默认折叠 |
| 辅助分区或可写运行挂载达到 85% | 主列表 | 自动露出并计入真实告警，不受正常项的三项预览限制 |
| 普通只读数据卷，或 `/snap/business-data` 实际为可写 ext4 | 主列表 | 不因为只读属性或目录前缀单独豁免；有效高使用率仍提示 |
| 挂载类型/只读属性不足以确认用途 | 主列表 | 保守保留，详情显示“未确认”；不猜测为 Snap 或系统挂载 |
| 普通存储容量缺失、总量为零或数值无效 | 主列表 | 显示“容量不可读”，不画假进度、不产生满容量告警 |
| 暂停、断线、过期、容量读取失败 | 原有分组保留最后确认结果 | 标记旧数据与各项观察时间；不把新拓扑时间当作容量更新，不新增弹出通知 |

规则顺序：先确认挂载属性与容量适用性，再判定真实容量风险，最后决定默认分组。不得根据路径前缀、所有只读卷、所有 tmpfs、所有 overlay 或“小分区”一刀切隐藏/豁免。

初次实现的确定规则保持有限：固定镜像识别使用已覆盖的 `squashfs + readOnly=true`；运行挂载仅识别 Linux 上类型已确认的 tmpfs/ramfs 与明确系统位置，例如 `/run`、`/run/credentials/…`、`/run/snapd/ns`、`/run/user/<uid>`、`/dev/shm`，按完整路径段匹配；辅助分区识别已确认本地分区上的精确 `/boot`、`/boot/efi`。任意 `/tmp`、自定义业务路径、容器 overlay 与 bind mount 保守展示，不在本次新增合并或去重规则。

Linux/OpenWrt 使用已有 mountinfo；Windows 复用现有卷 ID 与 `kind`，只读未知保持未知；macOS/BSD/其他 Unix 沿用现有部分覆盖，缺少类型/只读来源时保守展示。运行程序的五个平台与远端系统采集能力分别验收，不能互相替代。

## 数据与代码设计

### 插件内统一规则

新增插件私有 `src/filesystem.rs`，只放本功能实际需要的挂载属性关联、分类、告警与列表投影函数，供快照摘要和列表入口复用。由 `src/main.rs` 登记模块；浏览器不再维护另一套类型名单或 85% 告警判定。

保持原有容量字段与十进制字节字符串。内部投影补充 `filesystemType`、`readOnly`、短 `mountKey`、`displayGroup`、`capacityState`、`spaceWarning`；枚举分别为 `primary/auxiliary/system` 与 `known/unknown/notApplicable/fixedImage`。未确认字段为 null，不把未知只读状态转换为可写。

容量有效性、已用量推导和百分比处理复用现有字节解析口径；Windows 缺少 `used` 时由合法 total/available 推导。覆盖大于 2^53 的字节数、零总量、available 大于 total、非有限/越界百分比，不能通过浮点字节转换制造有效容量。

### 元数据关联与身份

在容量观察进入 `metrics.rs` 时，用当前已确认拓扑给该观察关联挂载身份；Linux 校验路径、来源/设备及已有 `identity`，Windows 使用现有 `capacityIdentity`/卷 ID。新字段只补充来源事实，UI 分类结果在投影时计算。

拓扑更新或只读属性变化时重新计算分组。相同路径换成不同挂载身份时，旧容量不得贴到新挂载；重置该项可用容量，等新一轮观察。关联失败保持未知并保守展示。短 `mountKey` 用于页面节点与展开状态，不把完整重复身份路径塞进每次响应，也不依赖可选共享发布是否启用。

容量的 `observedAt` 仅来自实际容量成功观察。保留 [Publisher](../../plugins/system-status/src/shared_state.rs) 现有同源身份、新鲜度、完整/部分扫描、撤销和配额处理；不得把隐藏项从 `mountTopology`、采集候选或共享发布中删除。

### 完整分类后分页

`scheduler.rs` 在完整有界缓存上计算分类、摘要与稳定顺序，再应用现有分页和响应预算：

- `snapshot()` 增加插件私有 `filesystemSummary`：主列表、辅助分区、系统挂载及真实告警的准确数量。仍不发送完整拓扑或全部容量。
- `list(kind="filesystems")` 增加可选 `group`：`overview/primary/auxiliary/system`。不传时保留原始完整列表语义，旧字段继续保留。
- `overview` 默认保留根卷、全部真实告警，再按稳定路径顺序补普通存储至三项；真实告警超过三项不折叠。`primary` 返回展开后的全部主列表；另外两组只包含未被风险提升的辅助/系统项。
- 主列表顺序为已确认根卷、风险项、普通存储；风险项按使用率降序，再用远端路径与短身份稳定排序。Windows 未取得系统卷事实时按驱动器路径稳定排序，不猜测 C 盘一定是根卷。
- 每页仍受 32 项、20 KB 项目预算和 24 KiB 总响应上限约束。告警多到需要分页时，保持醒目的完整告警数量和“下一页”，不把未加载告警放进默认隐藏组，也不声称当前页已展示全部风险。
- 确保单项投影满足响应预算；无法包含时明确标记受限，不返回永远不前进的 `nextOffset`。
- 返还实际 `revision/offset/total/nextOffset`；当前页因分组变化而失效时从该组第一页重读。多次分页响应修订不同则丢弃组合结果并重读，避免把不同挂载代次拼成一个列表。

这是同一插件包内的 RPC 数据扩展，核心 API 主版本和共享主题契约不变。内部字段与失效行为在系统状态功能文档说明，不把展示分类写入核心通用 API。

### 紧凑界面与状态

[index.html](../../plugins/system-status/ui/index.html)、[style.css](../../plugins/system-status/ui/style.css) 与 `view.js` 共同实现：

- 每项采用紧凑两行：第一行路径与使用率，第二行已用/总量与短容量条；详情使用原生 details，补完整路径、可用量、类型、只读属性和观察时间。固定镜像不用容量条暗示可写空间风险。
- “其他挂载（N）”默认关闭，展开后区分辅助分区与系统挂载；按需加载并各自保留页游标，不预先把所有隐藏项装进主列表。
- 复用 `navigation.expandedGroups` 保存“其他挂载”的展开状态，后端白名单增加该组。单项详情以短 `mountKey` 在当前 view 内保存；后台刷新保留现有 DOM 节点、焦点和展开状态，不每次 `replaceChildren()` 重建整个区域。
- 真正换挂载、切换服务器或 Context 代次变化时清理对应详情/页游标；页面重挂载恢复组展开，旧请求继续受现有 epoch/Context 检查约束。
- 主列表、隐藏列表的观察时间与失效信息都可见。减少动态、强制颜色、文字告警、键盘/读屏与三主题沿用现有约束。CPU、内存、网络和磁盘 I/O 的业务行为不纳入本次重构。

## 实施顺序与文件

| 顺序 | 文件/入口 | 工作与结束条件 |
| --- | --- | --- |
| 先复现 | `scripts/test-system-status-ui.mjs`，插件 Rust 测试 | 加入截图对应 mountinfo/容量，确认当前 Snap 满容量告警与列表噪声；新增期望先失败 |
| 规则与关联 | 新 `plugins/system-status/src/filesystem.rs`、`main.rs`、`metrics.rs` | 完成类型关联、容量状态、分类与身份变化测试；无双份业务规则 |
| 查询与状态 | `scheduler.rs` | 摘要、先分组后分页、旧调用兼容、游标失效与导航白名单；预算不放大 |
| 界面 | `ui/view.js`、`ui/index.html`、`ui/style.css` | 主列表和其他挂载、紧凑详情、按需分页、原地更新及焦点恢复 |
| 集成回归 | `src-tauri/tests/fixtures/system_status_flow.rs`、`scripts/test-component-communication.mjs` | 实际插件的私有投影与生命周期；文件插件仍能获得完整挂载和有效容量 |
| 文档与交付 | `docs/features/system-status.md`、插件 CHANGELOG | 实现随代码回写正式行为；版本/源码/原始包/实际验证进入插件记录，完成后删除本计划 |

仅在元数据关联确需修正现有解析时修改 `collectors.rs/shared_state.rs`；不新写 Linux mountinfo 解析器。发现规则需要可靠的新平台事实时，保留未知回退并记录设备覆盖，不临时引入远端工具依赖。

## 回归与验收

Rust 回归覆盖：只读 squashfs 100%；ramfs 0/0；普通只读 ext4；名为 `/snap` 的业务卷；正常及偏紧 boot；正常及偏紧 tmpfs；root 为可写 overlay；未知类型；缺失/异常容量；挂载身份/属性变更；部分拓扑失效；超过三项真实告警；第 33 项和后续页风险；长路径触发预算；分页期间修订变化。

固定数据 UI 使用截图中的两项主要存储、七项 Snap、三项运行挂载与两项辅助分区：默认两项主要存储、其他挂载 12 项、真实告警零项。模拟数据盘和 boot 偏紧后，两者提示且 boot 进入主列表；系统项不会重复留在隐藏组。另覆盖辅助风险消退、未知值、旧数据、展开刷新、焦点/键盘、节点保留、重挂载、断线/重连、设备删除、页游标重置和三主题 160/200/240/280/320/400/480px。

实现后按相关范围顺序执行现有命令，避免 Windows target 文件锁冲突：

```sh
npm run check
npm run plugin:check
cargo test --manifest-path plugins/system-status/Cargo.toml --locked
npm run scripts:test
npm run test
npm run web:build
npm run test:prepare -- system-status
npm run system-status:ui:test
npm run system-status:test
```

跨插件联动按[开发指南](../development.md)准备实际需要的测试目录后运行 `npm run communication:test`，验证隐藏项仍可通过共享接口被文件插件读取。正式文档执行 `node scripts/check-docs.mjs`；新增未跟踪 Markdown 另用其导出检查函数显式检查，避免被 `git ls-files` 漏掉。

独立构建入口为 `npm run plugin:build -- --plugin system-status`。实际归档验收使用现有位置参数入口 `node scripts/test-plugin-package.mjs <包含归档和元数据的目录> <本机原生target> <包来源40位提交SHA> system-status`，读取同目录的 `plugin-<target>.json`；不能把已解包目录或任意源码提交当作该输入。执行环境与包来源遵循[分发流程](../distribution.md)。通过现有 GitHub Actions 在 Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 原生编译、打包并验证包内程序启动、挂载/释放与退出，不以交叉编译或旧版本记录代替本次结果。

远端行为至少分别核对真实 Ubuntu/Snap 与现有 Windows SSH 采集；OpenWrt overlay/squashfs、macOS/BSD 缺元数据的回退按实际可用环境记录。原型数据和宿主包生命周期不证明这些远端设备已通过。

## 当前验证、风险与收束

已完成：生产实现与功能文档；30 项插件 Rust、183 项脚本、核心测试、格式/类型、正式前端、三主题七宽度 UI、真实 Windows OpenSSH 及文件/终端/状态联动。隔离 Ubuntu 22.04 实际挂载 core20 Snap 3015、ext4 boot/数据盘、tmpfs、ramfs，GNU 与 BusyBox 分别通过镜像、0/0 和 boot 风险提升验证。剩余：本次五平台原生包验收及正式 Latest 发布；其他真实远端设备范围在 CHANGELOG 明确记录。

主要风险是元数据缺失/身份关联错误造成误隐藏、先分页再分类造成漏告警、刷新重建节点造成焦点丢失，以及增加投影字段突破消息预算。均以保守回退和上述回归阻断。容量轮转可能导致旧读数，继续明确显示限制，不用隐藏时间提示掩盖。

正式行为完成后更新[系统状态](../features/system-status.md)；共享接口或核心架构未变则不更新对应正式文档。README 与文档导航的正式入口不变，临时计划沿用[计划目录](./)。发行证据只进入[插件 CHANGELOG](../../plugins/system-status/CHANGELOG.md)。相关代码、约定验证和文档全部完成后删除本计划、修复引用与清理无用原型附件，历史由 Git 保留。

返回[文档索引](../README.md)。

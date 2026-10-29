# T-023 实施与验证记录

更新日期：2026-10-28 · 状态：工程完成，待用户验收

基线：[代码方案r1](../technical/settings-implementation.md)、[核心r2](../core/settings.md)、[终端r2](../plugins/terminal/features/settings.md)。用户“你可以开始做了。”已授权完整实施。工作保留在v2，没有Git提交、推送或发布；用户验收尚未发生。

## 实际实现

- 右上角齿轮打开应用级单实例原生设置窗口。左侧应用/插件分组与搜索，右侧统一表单；支持跨页草稿、恢复本页默认、应用/确定/取消、错误定位及退出前处理草稿。
- 插件在签名包内声明页面、分区、字段、默认值、校验与生效说明；核心负责发现、呈现、调用路由和订阅。终端字段/业务规则仍在插件内，不在核心特判。
- SDK提供client/provider辅助，支持boolean/integer/enum/text和条件。成组原子保存含CAS修订、操作票据、持久回执、部分保存、冲突保护及结果核对。可选设置贡献损坏不会禁用终端业务。
- 外观迁入设置页，应用后生效；旧文件首次实际保存时逐字节备份后写v3，配置修订与系统高对比等呈现修订分开。应用与数据页展示真实运行模式/位置及固定目录操作，保留插件管理入口。
- 终端0.11.0声明字号和Shell目录集成。字号同步已有/隐藏/独立视图，移动后最新配置优先于旧快照；Shell开关只影响新建或明确重连会话。右键快捷操作走同一公开保存通路。
- 应用退出与同期T-025统一关闭协调衔接；单独关闭设置仍走自身握手。最终输入、保存中、旧票据及通知失联均有处理。

应用2.0.0-dev.18，构建时SDK5.2.0（保留同期T-024增量），设置贡献最低要求^5.1.0；核心数据库仍v6。本包包含终端0.11.0、文件0.8.0、网络隧道0.4.0三个签名插件，不代表其他在途任务已验收。

交付冻结已验证的dev.18 EXE；同期工作区随后已进入dev.19/SDK5.3，未为此回退源码或替换本次核心构建。打包通过`SODECK_T023_RUNTIME_BASELINE`读取已验证构建的版本/哈希，避免把当前源码版本误写为旧EXE版本；`sourceFingerprintPhase=packaging`明确指打包时工作区指纹。原生候选4的启动/移动结果有效，其最初API元数据误读了同期源码5.3，最终包已纠正为该EXE实际构建基线5.2。

## 工程检查

| 层次 | 实际结果与证据 |
| --- | --- |
| npm run check | 通过：契约生成、workspace类型、UI及核心/插件依赖边界；`.qa/settings-check.log` |
| npm run test:contracts | 最终全套通过；早期同期bounded-exec/监控失败已在对应工作区更新后解除；`.qa/settings-contracts.log` |
| npm run test:core | Rust workspace 226通过、31忽略；外部夹具忽略不计通过；`.qa/settings-core.log` |
| 桌面/plugin-manager/communication Rust | 76通过、1忽略；忽略项是父测试实际调用的子进程故障夹具；`.qa/settings-rust.log`。审计/描述专项见`.qa/settings-focused-rust.log` |
| 终端单元 | 最终19个worker和14个Vitest测试通过，包含关闭集成后明确报告unsupported且不启动observer的回归；`.qa/settings-terminal-tests.log`。最后插件修复另通过类型检查和独立签名打包，见`.qa/settings-terminal-final-check.log`、`.qa/settings-terminal-final-pack.log` |
| 浏览器表单 | 最终4个Playwright用例通过：四类控件、条件、多页导航、元数据搜索、整数/IME、部分保存、持久成功后补读失败、只读数据和目录操作失败/恢复；`.qa/settings-ui.log`。目录失败为模拟核心错误，没有改日常剪贴板/Explorer；不替代原生/SSH |
| 构建 | npm全量build及Windows release custom-protocol通过；`.qa/settings-build-all.log`、`.qa/settings-release-build.log`；debug记录`.qa/settings-debug-build.jsonl` |

## 功能与代码验证矩阵

下表相对证据路径以`.qa/t023/`为根；每项按真实层次列出，不把模拟结果当作真实服务验证。

| 用例 | 结果与证据 |
| --- | --- |
| SCODE-01 / SET-07/08 | Rust/TS严格描述验证、未知字段/类型/默认/条件循环/容量及签名通过。旧无声明包可运行；有效签名但坏描述仅禁用设置，实际终端SSH仍可用。契约/Rust日志、`updates/1793175834649/results.json` |
| SCODE-02 / SET-05/06/08 | 共享文件锁、并发CAS/raw写、同ID重放/异载荷、64回执淘汰、满/坏文件和高版本只读通过。write/sync/replace前后强停子进程仅见完整旧/新JSON；Windows拒绝替换保留旧文件。plugin_preferences/atomic_file测试 |
| SCODE-03 / SET-05/06/07 | 真实worker回调落盘后故意不确认同步；其他RPC响应，超时返回saved/pending，磁盘回执可查，补读恢复同步且不重复写。旧代次/ticket/伪来源由存储/契约测试拒绝。`save-fault/1793176653831/results.json` |
| SCODE-04 / SET-01/06/09 | 原生单例、不建工作区、来源/代次校验、5次开关订阅回基线、WM_CLOSE及同进程几何记忆通过。`native/results.json`、`lifecycle/results.json` |
| SCODE-05 / SET-02/03/05/10 | 切页/搜索/取消保留语义、四控件/条件、整数不强转0、错误焦点、部分保存和补读失败通过。运行信息指向隔离数据。UI/model与`native/results.json` |
| SCODE-06 / SET-04/06/08 | 旧外观原字节备份/v3回执/写入失败单元通过；原生三主题与实际Windows高对比不改变配置revision。主题生命周期3轮6表面返回单一主题订阅，连接/PTY/租约归零。`native/`、`lifecycle/`、`appearance-lifecycle/results.json` |
| SCODE-07 / TERM-SET-01～04 | 两个服务器工作区的三个真实Bash会话在10/24字号、隐藏及detach/return下同步，PTY身份保持、末行在网格内；Shell现有/新建/重连语义通过。`terminal/results.json`及修复后`directory-follow/results.json`。旧18/false、非法/缺失及冲突由存储/SDK/native分层覆盖 |
| SCODE-08 / SET-06/09 | 退出取消保留草稿、放弃保留已保存值、当前票据、保存中WM_CLOSE、失联原生回退通过；实际托盘退出及统一关闭协调后的最终原生回归通过。`native/`、`lifecycle/`、`save-fault/` |
| SCODE-09 / SET-07/08、TERM-SET-05 | 同一EXE哈希下A(rc.1，仅字号)→B(rc.2，增加真实Shell设置)仅重启生效；保留18/false/未知值，坏签名不破坏pending，启动失败候选回退B及原数据，高组版本只读。B真实Bash启动按保存值生效。`updates/1793175834649/results.json`及构造源码差异/哈希 |
| SCODE-10 / SET-09 | 三主题/200%文字、小窗口、键盘/减少动态及真实Windows高对比通过；真实微软拼音可信composition/input中文事件单独留证。最终生命周期重跑显式复用该IME证据，未声称每次均重新输入。`native/*.png`、`lifecycle/ime-events.json`、`lifecycle/results.json` |
| SCODE-11 / SET-05～10 | 审计白名单含page/owner/key/revision/保存/同步结果，无值正文、路径或ticket。描述128KiB、页48KiB、值16KiB、feed24KiB、RPC64KiB、设置feed4MiB/128页、回执128KiB及有界读取有边界验证；目录按字节分页、读并发4，关闭释放feed。Rust/SDK审计容量与lifecycle |

## 回归适配与问题处理

- 目录回归发现本次捕获Shell启动偏好时，错误地把“创建目录状态报告器”也限制为已开启集成，导致关闭集成的新会话停留unknown。按既有DR-09/TERM-SET-03修复：仍注册目录来源，无observer时明确报告unsupported，不开启观察器或远端集成命令；真实I/O保持。专项单元和完整真实目录回归均通过。A/B夹具验证先于这次状态修复，最终0.11.0包另经目录及portable验证。

- 旧目录跟随脚本固定等待1.8～2秒即一次定位，QEMU路径确认约3秒，点击时核心仍为旧目录lastConfirmed。保留失败证据，测试改为等待workspace.directory.v1确认后点击；没有改文件插件业务。旧私有Shell设置命令迁到公开settings.commit。
- 同期T-025增加原生窗口关闭审阅和连接目录订阅。外观/目录脚本收到CLOSE_REVIEW_REQUIRED后等待实际合并；主题回归统计主题订阅，不把合法目录订阅当作泄漏。
- 旧外观弹窗已删除，appearance-visual不再是本次入口；settings/settings-lifecycle覆盖设置视觉/输入。旧terminal-geometry依赖旧版本及私有设置，settings-terminal覆盖字号、PTY、移动及末行几何。未把这两个旧脚本记为通过。
- Windows沙箱曾阻止esbuild父目录读取和WebView CDP；自动审批通过后运行相同本地构建/隔离测试。专用EXE、数据、QEMU overlay和22341端口与日常包分开，未停止T020日常实例。

最终相关回归：`directory-follow/results.json`完整通过，一次定位、暂停/恢复、旧/新会话、单视图和整个工作区移窗、两个工作区及最终资源归零；`appearance/results.json`完整通过，3窗口/8表面20次切换p95≤300ms、200次切换/慢消费者、真实写盘失败、原生菜单/合并和重启持久化。测试QEMU PID27748在核对EXE、overlay及22341端口后已停止，见`linux/stopped.json`。

## 交付与体验

交付目录`.local/dev/SoDeck-v2-T023/`，Windows release profile开发包。`Start-SoDeck.cmd`或EXE直接启动，数据在包内`data/`；`build.json`记录EXE/三个插件/资源哈希和打包时工作区来源。

普通portable启动和移动目录后保留设置的最终候选验证已通过：`.qa/t023/package-smoke/1793179252811/results.json`；实际保存完成后截图并正常退出。最终包使用相同EXE和三个插件，不复制测试数据。EXE SHA256为`fae23fba806d45401b9f5ec1cc8e36a15da8142293a51a5b9c9c93c34d5c65d2`，最终终端包为`c204c742362d0738d823d9ca75a4c64e956f602fe3800e7e0c2472c266f25677`。不是安装器验证、正式发布或用户验收。

建议体验：

1. 启动后点击右上角齿轮，再点一次确认只打开一个设置窗口。
2. 选择主题，切终端页改字号；取消确认草稿未生效，再修改并应用。
3. 打开多个终端，应用字号，观察已有终端同步且会话保留。
4. 修改Shell目录集成，比较当前与新建/明确重连会话；有草稿时关闭或退出，体验继续编辑/放弃。

截图见`evidence/`与`.qa/t023/native/`。已查看三主题和200%截图，导航、内容及底部操作可达，无重叠裁切。

## 验证边界

本次环境为Windows/WebView2、单显示器、隔离Linux Bash/OpenSSH。多工作区使用同一SSH主机的独立服务器记录，不声称多台物理服务器、macOS/Linux桌面、混合DPI多显示器或安装器验证。真实IME在独立运行留证；200%文字为原生WebView根字号加倍，减少动态用媒体条件，高对比实际切换Windows后恢复。外观v3不能直接交旧核心降级使用，保留旧文件备份，不接触日常数据。

任务尚待用户体验验收；未创建提交、推送或公开版本。

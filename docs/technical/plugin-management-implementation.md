# T-028：插件管理代码方案

文档就绪：可评审 · 修订：r1 · 日期：2026-10-29
功能唯一来源：[F-CORE-032 r1](../core/plugin-management.md)；确认与结果：[T-028 r1](../tasks/T-028-plugin-management.md)

本文保留r1确认时的代码方案，实际模块、构建与验证已在[T-028结果](../tasks/T-028-results.md)记录；下文“拟新增”等描述是确认时基线。

## 1. 已核实的实现与改动边界

2026-10-29在当前v2工作区只读核实（有大量其他任务未提交内容，不归本任务重置）：

| 当前代码 | 事实与需改之处 |
| --- | --- |
| crates/plugin-manager/src/package.rs | verify强制读取signature且要求TrustedKeys命中；库存数量固定+2。改为完整性必需、签名可选且独立返回状态 |
| crates/plugin-manager/src/store.rs | State只有active/pending/failures；activate_pending会试运行所有候选；active每次验证；没有启用偏好。需在任何试运行前应用启动启用集合 |
| apps/desktop/src-tauri/src/app.rs | prepare_bundled→activate_pending→active→spawn_plugin；spawn先读worker并初始化，再Assets::load读磁盘。需完整字节快照先验证，再执行 |
| crates/plugin-host/src/process.rs | spawn_with_context读取worker路径，768KiB上限，初始化帧传code。可增加已验证源码入口，保持现有帧与预算 |
| apps/desktop/src-tauri/src/plugin_registry.rs | recover_plugin直接active→spawn，必须同样遵守本次启动启用集合 |
| apps/desktop/src-tauri/src/native.rs | Assets已用Arc保存UI内存副本，可改为从验证快照构造，避免重复读取 |
| settings/registry.rs、service.rs及前端settings/root.tsx | 仅外观为可写核心页；插件设置描述通过manager.active读；统一草稿、回执、关闭协调已有，需接入专用管理页 |
| management-dialog.tsx、app.tsx | 插件为独立弹窗，只有版本列表和输入路径准备更新；所有入口改为设置core:plugins |
| workbench_services.rs::initialize_workspace | 空插件集合写“没有可用插件”初始化错误；全部主动停用应改为正常空态 |
| scripts/pack-plugin.mjs | --source仍先读plugins/<name>/manifest；总生成开发签名。需支持独立资源目录和明确无签名模式 |
| contracts/capabilities.rs、SDK | 当前API/SDK6.1；现有四插件声明^6.1，适合6.2增量，不需要改业务库或四插件业务代码 |

不把业务判断移入核心；不改SSH/代理/PTY协议，不引入插件权限审批或互调。UI按已读取apple-design与公共主题/组件规范。

## 2. 模块与文件落点

下表标“新增”的文件尚不存在，只有方案确认后创建实现。

| 模块 | 修改/新增 | 职责 |
| --- | --- | --- |
| 包校验 | 修改package.rs；新增package/integrity.rs、package/signature.rs、package/snapshot.rs | 有界读取、完整清单、可选签名、不可变字节快照、结构化错误 |
| 启用存储 | 新增crates/plugin-manager/src/enablement.rs、atomic_file.rs；修改lib.rs/Cargo.toml | 独立偏好/回执、原子写和损坏恢复；Windows原子替换在本crate实现，不反向依赖desktop |
| 版本管理 | 修改store.rs及store/tests.rs | 候选激活过滤、disabled候选保留、metadata与运行选择分开 |
| 宿主加载 | 修改app.rs、native.rs、plugin_registry.rs；新增plugin_management.rs | 启动快照、实际状态汇总、公开管理命令、一次加载链 |
| worker宿主 | 修改crates/plugin-host/src/process.rs及tests/process.rs | 接受已验证源码，保持已有启动帧/二进制能力和768KiB限制 |
| 设置服务 | 修改settings/{mod,registry,service}.rs、settings_window.rs、settings_exit.rs | core:plugins入口、不可用provider展示、草稿/在途操作关闭协调 |
| 前端 | 新增settings/plugins.tsx、plugin-model.ts、save-coordinator.ts；修改root.tsx/application.tsx/settings.css | 插件列表/草稿适配与现有统一应用流程，避免把所有逻辑继续堆入root |
| 旧入口/空态 | 修改app.tsx、management-dialog.tsx、workbench_services.rs及必要空态组件 | 统一入口、全部停用可用、清理旧插件弹窗分支，资源/调用记录页保持 |
| 契约与SDK | 新增contracts/plugin-management-v1.schema.json、scripts/generate-plugin-management.mjs、SDK plugin-management.ts及生成文件；修改schema/index/package.json/capabilities/state-feed/对应生成器 | 唯一契约源、6.2增量、分页与管理状态feed |
| 打包与交付 | 修改scripts/pack-plugin.mjs；新增scripts/package-plugin-management-dev.mjs、docs/plugin-development.md | 任意编译目录无签名打包、开发说明、独立Windows目录及证据 |
| 验证 | 新增管理器/宿主定向测试、SDK验证、settings/plugin-model测试、tests/ui/plugin-management.spec.ts、tests/desktop/plugin-management*.mjs | PM与四插件专项的实际入口，复用已有隔离harness |

## 3. 完整性与不可变快照

`read_verified_package(root, mode)`返回结构化元数据；需要运行时返回`VerifiedPackageSnapshot`，包含manifest、完整性摘要、SignatureStatus与`Arc<文件路径→Arc<字节>>`。构造器内部可见，未校验输入不能伪造运行快照。包文件128MiB总上限沿用，签名16KiB/清单1MiB，业务worker768KiB；逐插件顺序验证，不并发堆积全部候选副本。UI共享快照字节，不再额外复制整包；候选探测结束释放快照。

读取顺序：有界枚举→普通文件/路径/重复校验→读取一次字节→严格解析完整性清单（重复JSON键拒绝）→全文件哈希及精确库存比对→解析manifest与必需描述→评估签名→兼容检查。完整性覆盖manifest、UI、worker、settings与observability描述；可选签名只有0或1份，库存公式使用实际存在数量，不能让其它未列文件漏过。

目录内替换竞态通过“验证哪个字节就使用哪个字节”关闭：worker UTF-8、UI资源和描述都从已验证快照消费；symlink/reparse-point及最终文件类型/边界继续检查，不把路径前缀判断当作完整验证。失败快照不发布。初始化前先构造Assets与描述，防止worker已执行后才发现UI资源损坏。

增加`PluginProcess::spawn_source_with_context`接收已验证源码。现有路径入口用于已有测试/调用时保留并转调，生产插件加载只用源码入口；wire不改，不启动额外常驻进程。Loaded持有共享快照，故障恢复创建新快照，旧快照按现有资源生命周期释放。

签名状态为`unsigned | verified | developmentVerified | unverified | invalid`，verified只使用随核心提供的公钥，绝不自动信任包内公钥；unknown keyId显示unverified。调整read_keys与TrustedKeys载体保留developmentOnly来源属性，不凭keyId文本猜测开发密钥；旧密钥夹具适配，签名算法/字节不变。缺少公钥资源可用空信任集继续检查未签名包/未知来源包，但记录配置故障，不能显示已验证。malformed和已知wrong signature为拒绝类；完整性失败优先于来源提示。生产密钥管理与撤销后置。

同pluginId/version比较完整文件库存摘要，包含完整性文件及签名文件的实际字节；ZIP压缩方式不同但文件字节相同视为同包，不允许同版仅替换签名/内容偷改身份。现有已签包格式无需重写。

## 4. 启用偏好、版本与启动状态

### 4.1 独立文件

`plugins/enablement.json`格式1：

- formatVersion=1、storageGeneration(UUID)、revision(u64字符串)。
- disabledPluginIds：排序去重ID集合，缺省表示启用；因此新ID和旧安装首次接入默认启用，更新不改变偏好。保留缺失包的禁用ID，随附补装不重启它。
- receipts：最近128个operationId/digest/result，最多128KiB；整个文件最多1MiB。仅存已改变ID/布尔及摘要，不存包正文/秘密。

用同目录唯一临时文件、sync_all、Windows替换/写穿透（Unix rename+同步父目录）一次提交偏好与回执。读失败、坏JSON、重复ID、非法ID、未知格式不作为空配置。atomic_file实现位于plugin-manager，避免crate循环依赖；借鉴desktop现有已验证模式，保持故障注入测试。

缺文件是正常首次初始化，持久化空disabled集合失败则本次禁用所有插件并显示存储错误，不继续默认启用。已有格式1读取不改数据；当前core v7、activation.json、插件私有设置格式不变，不触发数据库备份重建。旧核心会忽略此新文件并可能重新加载停用插件，因此回退必须使用升级前独立目录/备份，交付说明清楚标记这一兼容限制，不宣称旧核心也支持停用。

损坏修复仅支持已识别的损坏格式1/无法解析文件：显式调用恢复命令，校验诊断时文件指纹未变；创建时间戳+UUID备份、同步成功后原子新建，disabled集合取当前管理器已登记全部ID。无法读取原字节或备份失败不重写。高版本不提供重建；修复回执与新generation在同文件提交，重启不丢结果。

### 4.2 运行选择

启动先open管理器恢复既有journal，再load启用偏好并生成`BootEnablement`不可变快照。准备随附包仍可静态验证/登记；activate_pending增加predicate，只对本次启用ID执行候选数据副本/初始化/提交。停用ID的pending不删除、不复制业务数据、不启动任何代码。损坏启用文件时predicate恒false。

运行集合由bootEnabled∧包检查成功∧启动成功得到；期望启用值后续可保存但不修改BootEnablement。实际Loaded和进程存活决定running状态；管理器active仅表示选定版本。现有`active()`保留读取版本/数据的含义，新增明确`load_for_run(id, boot)`门禁用于app启动、recover_plugin和所有后台重建入口，不靠每个调用方自觉记忆检查。

候选初始化失败沿用当前旧版本与数据回退；随后旧版也须重新校验。坏包错误保存/投影到该ID，settings列表不得因verify失败而丢失此项。运行崩溃更新实际状态，不篡改启用偏好或pending。

全部停用时工作台注册空贡献是合法状态；初始化调度不创建任务、不强行恢复终端。没有插件实例不等于核心失败。用户业务资源仍由正常完整退出释放，下一次停用启动不能创建新的监听/会话/采样。settings查询、诊断读取不得触发恢复插件。

### 4.3 偏好与版本不做跨文件事务

导入只改既有版本状态，启停保存只改enablement，两者不需要同一事务。二者在本进程管理器锁内串行发布，解包/哈希在专门有界后台任务中完成，发布前重核版本状态，避免阻塞短设置读取。锁顺序为管理器→其存储内部锁；不同时持有settings、loaded或通信Hub锁，不在锁内执行worker回调。

启动恢复activation journal不能回滚已独立保存的enablement。已停用候选即使暂留pending也不触发重复试运行；正常启用后再按现有dataSchema规则激活，不趁此新增通用数据迁移器。

## 5. API/SDK与状态契约

API/SDK建议6.1.0→6.2.0，新增`plugins.management.v1`、`plugins.optional-signature.v1`，均为兼容发现，不是权限项。现有`plugins.list`、`plugins.metadata.list`、`plugins.prepareUpdate`的严格DTO保持，底层复用新检查与导入服务；旧元数据里的activeVersion仍是选定版本。新界面只用新契约。

| 拟新增方法 | 请求要点 | 返回/约束 |
| --- | --- | --- |
| plugins.management.list | cursor/limit，query可选 | catalogRevision、items、nextCursor；含enabled、bootEnabled、runtimeStatus、selected/pending版本与各自签名/验证、错误和restartRequired |
| plugins.enablement.get | 无参数 | storageStatus/generation/revision及修复所需诊断token；每ID的enabled由分页管理列表返回，不用一个无界响应返回全部ID |
| plugins.enablement.validate | generation、expectedRevision、changes[{pluginId,enabled}] | 无写入；已登记ID与布尔/容量校验，完整性坏包可保存启用意愿但运行仍拒绝 |
| plugins.enablement.commit | 同上+operationId | 原子整组保存、persistedRevision、restartRequired；相同请求幂等 |
| plugins.enablement.operation.get | generation+operationId | running/saved/rejected/unknown；超出回执保留窗不得冒充未提交 |
| plugins.package.prepare | operationId、path可选 | 返回operationId与running；无path走原生文件选择，取消返回cancelled |
| plugins.package.operation.get | operationId | selecting/verifying/prepared/failed/cancelled/unknown；prepared含ID/版本/签名/摘要 |
| plugins.enablement.repair | operationId+diagnosticToken | 原文件备份成功后重建全停用，返回新generation/revision和备份位置 |

新增`plugins.management.v1`应用级state feed仅发布catalogRevision、enablementRevision及storageStatus，客户端按分页重取；不把整个包/列表塞进每次状态通知。修改contracts/state-feed-v1.schema.json、生成产物与communication_services.rs白名单/保活/关闭退订。schema经现有contract-schema工具生成并在Rust/TS双方校验，check脚本加入生成新鲜度。管理列表使用登记状态与本次已完成的验证结果投影，不能每次刷新或feed推送都重新读取/哈希整包；导入、启动和恢复的实际加载边界仍须重新验证。

所有方法走宿主绑定Caller与既有core.record；沿用P-14不设插件能力审批表，保证source/generation有效，不允许客户端伪造来源。设置管理由核心服务持有，插件自身provider只处理其业务设置；公开的核心管理API不等于插件互调。

容量：请求/结果64KiB；list默认50、最多100项，响应60KiB内截断分页；catalog ID上限1024，超过时新ID导入报CAPACITY，不静默截断已有记录；changes最多1024且≤64KiB，enabled保存只发送差异。错误文本≤512字，ID≤128；分页cursor绑定catalogRevision，变化返回CONFLICT，客户端重读并按ID保留草稿。manifest/能力/包现有上限保持。

单个设置保存10秒后显示结果待确认；只允许一个启用保存在途。导入一次一个，受现有大小上限，进度与结果可查询，不因UI超时自动重复执行；回执内存保留本次进程最近128项，进程重启后旧操作返回unknown，客户端通过ID/版本/pending内容摘要核对。设置窗口导入在途沿用“保存中等待”关闭协调；退出前结束或中止未发布的暂存任务，发布原子点后的包保留pending。选择器取消是无副作用路径；不建设额外的后台下载任务。

新增错误码分类：PACKAGE_INTEGRITY_FAILED、PACKAGE_SIGNATURE_INVALID、PACKAGE_INCOMPATIBLE、PLUGIN_DISABLED、ENABLEMENT_UNAVAILABLE、CONFLICT、CAPACITY、STORAGE_ERROR、RESULT_UNKNOWN；外层旧prepareUpdate仍可投影PACKAGE_INVALID以保持旧客户端契约，细节不泄漏包路径。错误与状态字段来自新schema。

## 6. 设置页面与保存协调

插件管理是核心自绘列表，不能塞进普通插件声明式64字段表单，不能要求停用插件提供设置provider。

前端增加`SettingsSaveParticipant`内部适配接口：dirty、validate、commit、reconcile、discard、unresolvedOperations；现有PageDraft适配与plugin-model适配共同参加底栏流程。只提取当前保存协调所需代码，不重构通用表单系统。提交前校验全部参与组，按稳定顺序依次提交，保留现有跨组部分成功语义；插件启用组一次提交不拆成逐行保存。

root的dirty统计、窗口标题/导航标记、report中的dirtyPageIds与unresolvedOperationIds、关闭冻结和应用按钮状态均纳入core:plugins。读取/分页带request序号与abort，旧结果不能覆盖新草稿；feed只更新基线/冲突，不重置用户已修改ID。导入成功新增行使用服务器返回的默认偏好，已有ID草稿保留。

settings registry新增core:plugins条目，不生成虚假的动态SettingField；settings.page.get不承担新列表的读取。ui.settings.open继续可定位core:plugins，设置窗口校验与退出报告接受新ID。旧入口通过统一open调用聚焦同窗，资源管理与调用记录保留原页面。

插件自有设置读取使用包的已验证静态描述；已停用只展示描述和不可用原因，不调用provider.read。坏描述保持原有单贡献隔离规则，缺失描述文件仍为包完整性失败。管理列表独立于设置描述，任何无设置声明/坏包插件都能管理。

UI复用公开公共组件。验证Checkbox现有属性满足disabled/aria/focus需求；如需公共组件改动先记录具体差异，限于本方案。三主题及可访问性由PM-15覆盖。

## 7. 打包与第三方接入

保留官方现有`--plugin terminal`流程与默认开发签名，避免批量破坏已存在的测试/目录包构建。新增`--signing none|development|key`；key模式必须明确--key与--key-id，none模式禁止混入签名参数，签名参数错误立即报错。

仅给`--source`时，先读取该目录manifest，不再读取官方plugins/terminal；默认none。既有同时传--plugin/--source或--key的脚本保持可识别旧语义，并在结果报告实际signing模式。none不生成私钥、公钥文件或signature.json；清单仍自动生成。源目录残留integrity/signature由打包产物重新生成/排除，不修改源资源。

第三方新无签名包manifest最低API6.2且声明plugins.optional-signature.v1，由打包工具校验明确提示，避免给旧核心宣称可用；本次用当前旧包去签名测试新核心兼容是测试夹具，不改变旧包发行声明。官方已有已签包无需变版本或重签。

示例（规划入口，实施后写入docs/plugin-development.md）：
`node scripts/pack-plugin.mjs --source C:/plugin-build/dist --signing none --out C:/plugin-build/demo.sodeck-plugin`

只承诺对已编译资源目录打包和本地导入；SDK当前仍是仓库workspace，不宣称已在npm发布完整第三方脚手架。本次开发说明解释manifest、入口、公开SDK/资源限制、无签名状态及版本兼容，不新增市场发布系统。

## 8. 四插件影响

现有终端/文件/隧道/监控业务代码预计无需变化，启停由核心统一控制；四个当前API6.1原包必须不重编译通过新核心。若因宿主遗漏发现业务插件实际绕过统一生命周期，则先记录具体问题，在不改变已确认业务行为的范围内修复，不能为通过测试塞插件ID硬编码。

每个插件的独立文档列生命周期与交互回归，见F-CORE-032测试索引。独立更新只用真实终端A/B包（B使用测试副本版本和可见版本区别，包含其真实UI/worker），核心二进制哈希保持不变；其他三个插件包哈希保持。测试专用最小worker只验证零执行与故障计数，不作为产品替代。

## 9. 验证、阶段与交付

实现顺序：契约与校验快照→启用存储和启动/恢复门禁→核心管理API/状态→设置统一草稿列表→第三方打包→定向验证→Windows目录交付。每阶段先跑受影响检查，最后一次完整约定回归，不恢复T-024已取消的长压测。

| 关口 | 计划命令/入口 | 对应 |
| --- | --- | --- |
| G1 类型与契约 | 现有npm run check、npm run test:contracts；新增schema生成--check | PM-04/11/21、公开DTO和边界 |
| G2 管理器与进程 | node scripts/cargo.mjs test -p sodeck-plugin-manager -p sodeck-plugin-host；desktop定向设置/工作台/关闭/存储测试 | PM-02～04/07～12/14/17，文件中断与快照真实进程 |
| G3 UI | 拟新增tests/ui/plugin-management.spec.ts；现有settings/connection-home相关回归 | PM-06/11/15/19/21，不代替原生 |
| G4 Windows原生 | 拟新增tests/desktop/plugin-management.mjs、plugin-management-updates.mjs、plugin-management-package.mjs，复用现有harness | PM-01/05/08/10/13/18～20及四插件专项 |
| G5 构建与交付 | 现有npm run build、Windows desktop/host构建；拟新增package-plugin-management-dev.mjs | 全新目录、实际启动、哈希/证据/操作步骤 |

核心数据库/凭据变化校验用隔离副本前后快照，预期不改schema、不重建；保留旧目录。worker/网络资源真实释放按已有harness和协议夹具验证，监控真实Linux由现有隔离环境验证，不把Windows远端监控标成支持。

拟交付 `.local/dev/SoDeck-v2-T028-r1/Start-SoDeck.cmd`、完整exe/资源、build.json来源哈希及PM结果。目录尚不存在，不能现在作为启动链接；目标开发版本顺接dev.20，实施前核对其他在途任务版本避免占用冲突。无安装器、无自动导入日常配置、无用户数据覆盖。结果记录新增T-028-results.md（实施阶段才创建），未运行或必要验证失败不得标工程完成。

## 10. 方案评审与限制

本稿固定：重启启停；无签名与未知签名来源允许；已知错签/损坏签名拒绝；手动签名降级可准备并显示状态；首装默认启用；启用文件异常保留且本次全停用，支持损坏文件显式备份重建。用户确认的是这组可观察行为及PM验证，不是每个内部函数名。

旧核心不理解新的启用偏好；签名状态不代表代码安全或第三方权限沙箱；公开核心调用与调用记录沿用P-14。包完整性不等于持续磁盘监控。上述是具体能力边界，不新增审批弹窗。

r1：2026-10-29，仅源码核实与文档方案。无实现、构建或功能实测结论。

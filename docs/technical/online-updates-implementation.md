# T-029：GitHub发布与独立在线升级代码方案

文档就绪：可评审 · 修订：r2 · 日期：2026-10-29
行为唯一来源：[F-CORE-033](../core/online-updates.md) · 确认/结果：[T-029](../tasks/T-029-online-updates.md)

r2接续前序讨论与实际dev.20/API6.2工作区，细化到模块、契约、持久化提交点和实施关口。r1是前序评审记录，本稿替代其技术细节；用户的两项偏好保持。r2随后已获用户明确实施确认；下文“尚未建立/运行”等是设计提交时的基线，当前dev.21实现、实际依赖配对与分层验证见[T-029结果](../tasks/T-029-results.md)，已建立脚本入口见[发布工具](release-v2.md)。

## 1. 已核实的基线

| 位置 | 当前事实与采用方式 |
| --- | --- |
| v2工作区 | 没有.github目录；tauri bundle.active=false，desktop:build强制--no-bundle，当前交付是dev.20目录包，未实现安装或在线升级 |
| GitHub BgaSol/soDeck | 公开仓库，默认master，2026-10-29核查公开Release只有v1.0.0～1.0.2；v1.0.2有NSIS/MSI、latest.json与.sig |
| origin/master工作流 | release.yml监听v*标签及workflow_dispatch；只参考，不修改v1发布路径或复用其私钥 |
| plugin-manager/store.rs | prepare_bundled只为未安装ID播种；版本不可变，stage/accept分离，pending在重启激活；可复用独立升级 |
| package/snapshot.rs、package.rs | 完整性/可选签名与当前CORE_VERSION/API_VERSION检查耦合；需拆出显式目标兼容上下文以验证“供下一核心使用”的缓存，不能增加任意跳过检查开关 |
| startup_config.rs | 区分development/portable/standard，资源索引与开发覆盖已有；安装版须正确打入resources/themes/brand/宿主，不复制便携标记 |
| app.rs及关闭协调 | 已有退出审查、shutdown和worker/核心资源释放；安装交接必须接在正式退出放行之后 |
| settings/application.tsx、plugins.tsx | 应用与数据页只读版本；插件页本地导入/启停已实现，新增更新页及在线条目，不做第二套插件管理 |
| plugin-manager/lib.rs、app.rs | 插件兼容CORE_VERSION固定2.0.0，桌面安装版本是Cargo的2.0.0-dev.20，runtimeInfo.channel写死development；不可拿兼容常量判断安装更新 |
| window_close_review.rs、app.rs | Action目前只有Exit/Home/Close/Move；app.exit成功后另启150ms延时退出，不能拿普通退出后再安装的顺序拼接 |
| store_bootstrap及observability/schema.rs | core结构7，facts/logs/traffic/diagnostics结构2；只检查core v7不足以防止升级触发观测集合重建 |

本方案不改SSH、PTY、SFTP或隧道业务协议。四插件不各自联网检查，不把更新逻辑塞入worker。已有旧文档的“无联网升级”是T-028范围，本任务为新增范围。

## 2. 发布格式与识别

专用标签`sodeck-v2/{stable|preview}/{六位递增批次}`，例如000001；每渠道独立序号。tag只触发v2的tag-push工作流，提交必须可从origin/main到达（按P-71，原v2分支改为main；产品/标签命名保持）。稳定/预览对应发行版本规则，稳定核心禁止预发布semver，开发版本不发稳定渠道。批次不替代组件semver，不自动修改源码版本或提交代码。

拟新增`release/components.json`声明五个组件及构建入口，`release/channels/{stable,preview}.json`声明产品标识、公钥和平台。核心安装版本以desktop Cargo package为权威，与根package、desktop package和Tauri配置校验一致；插件manifest.version与自身package.json必须一致。版本读取和校验不自动改版。

`RuntimeCompatibility`另含`coreCompatibilityVersion`、`pluginApiVersion`、完整能力列表及平台。它表示插件可依赖的核心契约，安装版本表示二进制发行；两者均写入构建元数据并由同一生成来源供管理器与目录使用。保留当前2.0.0兼容基线，不把预览安装版本去掉后缀后猜成兼容版本。安装升级只比较`appVersion`；插件requires.core匹配明确的兼容版本。API计划6.3.0，coreSchema仍7、四观测分区仍2。新增`contracts/runtime-compatibility.json`及生成/检查入口统一现有散落常量，不改插件requires语义。

每批包含`catalog-v1.json`、`catalog-v1.sig`、本次变化资产及SHA256SUMS；核心更新另有Tauri updater的.sig。目录是完整快照：

| 字段 | 内容/约束 |
| --- | --- |
| schemaVersion、product、repository | 1、sodeck-v2、BgaSol/soDeck，严格匹配 |
| channel、sequence、releaseTag、sourceCommit | 固定渠道/正整数序号/标签/源码SHA，彼此一致 |
| core | appVersion、coreCompatibilityVersion、pluginApiVersion、capabilities摘要及完整有限列表、dataFormats（core及四观测分区、激活/偏好/更新状态读写格式）、各平台Installer记录 |
| plugins | 按pluginId组织的官方组件；版本、requires/core/API/平台/dataSchema、包摘要与资产定位 |
| assets | repository/releaseTag/assetId/name/size/archiveSha256/下载定位；核心还带updater签名与签名版本，插件另带packageDigest和发布keyId；目录自身不在自己的assets内，避免自引用 |
| compatibility history | 每个组件最多保留8个已发布版本引用供选择最高兼容版本；不足时提示需核心更新，不回退到猜测或无限遍历 |

字段来自实际构建和包manifest，不能只相信源码手写版本。ID/版本/能力/数据schema与下载包交叉验证。未变化组件引用上一目录的原始资产和哈希，不重新签出不同的同版本资源包。首次目录必须完整包含核心和四插件，后续只发布变化项。不存在“插件批次号比核心大所以装插件为核心”的比较。

`archiveSha256`是完整下载文件哈希；`packageDigest`是现有VerifiedPackageSnapshot按排序文件名和长度/内容计算的摘要，二者不能混用。发布侧通过Rust验证工具复用管理器计算包内容摘要，Node不再维护第二份算法。目录对每个组件保存`inputFingerprint`、构建工具链和原始资产引用；核心同时记录出厂随附插件清单，区分该安装器内的出厂版本与本批可更新版本。

目录签名覆盖准确UTF-8字节，使用固定域前缀`sodeck-v2-catalog-v1\0`加目录正文的Ed25519签名。内置已知发布公钥，未知目录keyId或坏签名拒绝；密钥轮换先经已有可信核心安装新公钥，再使用新密钥，不自动信任目录携带的任意公钥。该规则不改变用户手动插件的可选签名策略。

客户端使用GitHub Releases列表筛选本产品/渠道，再读取验证签名完整目录，按sequence选择最新；不用全仓库latest。分页每页100，最多10页，超预算明确检查失败且保留已验证缓存，不声称最新。完整枚举预算内的列表后，按标签序号核对最高匹配批次；该批次缺目录/坏签名时报告发布无效，保留上一已验证目录且不冒充最新。ETag逐请求URL记录；304复用签名仍有效的对应缓存。最高接收序号按渠道持久化，低序号/同序号异字节拒绝，缓存丢失后不保留脱离正文的成功状态。没有独立托管服务、gh-pages分支或可变latest资产指针。

只有draft=false、标签/频道正确、目录签名及必需资产完整才可消费。发布工作流用make_latest=false，不触碰v1 latest.json。GitHub关闭/丢失资产不意味可降级替换，显示不可用并保留已安装版本。

## 3. CI与安装产物

拟新增`.github/workflows/v2-check.yml`与`v2-release.yml`，后者只接专用tag push；可复用check工作流。固定Node/Rust版本和Action提交，npm ci/Cargo.lock，Windows MSVC/Perl/OpenSSL构建依赖显式安装，不依赖开发机.local缓存。检查现有契约、单元和浏览器；Windows打包release profile。

模块化脚本建议位于`scripts/release/`：version/inputs（核对版本与组件输入）、plan（与上次目录比对）、build（生成资产）、catalog（严格目录/签名）、publish（草稿/上传/最终公开）。确认后新增，当前不可运行。

发布输入指纹按组件声明完整依赖：核心包含Rust核心/host/manager/updater、外壳、实际使用的SDK/UI、主题/品牌、构建配置和相关锁定依赖；插件包含自身代码/描述、SDK/UI实际构建输入和相关锁定依赖。使用规范化路径/文件内容、构建参数、工具链、正式签名公钥及依赖解析闭包求哈希；锁文件只纳入该组件解析到的包/version/integrity/link，不直接哈希整个根锁文件。UI与worker构建输出的依赖清单校验声明覆盖，不凭目录猜共享依赖。单纯新增未被导入的SDK导出不强迫四包升版，已导入共享代码变化则要求受影响组件升版。

核心指纹排除业务插件源码、插件版本和生成的`.sodeck-plugin`/随附索引；这些作为“出厂插件快照”单独登记。只改插件时完整复用旧核心安装器，不能把新插件塞入同版本安装器；核心确实升版时才嵌入当批兼容原包。构建脚本的相关逻辑变化仍计入指纹。版本变化即构建，输入变化而版本不变即失败；二者皆不变只核对/引用旧资产。只改文档不造更新；整批无组件变化拒绝空发布。发布规划以这些规则保证核心与插件真正独立。

规划/检查job仅contents:read；可信tag构建/签名按需取密钥；最后publish job才contents:write。串行发布相同渠道，公开前重新核对上一sequence，避免同时发布覆盖。失败草稿可重跑，已公开资产不覆盖；检查签名密钥存在后才构建，不退回本地开发签名。

发布步骤固定为：校验标签/完整git ancestry及上一有效目录→无密钥构建/检查→可信tag签名→publish创建或找回同tag/sourceCommit草稿→上传变化资产→取得实际assetId/大小并逐件核验→组装完整catalog正文→签目录并上传目录/签名/校验和→重新检查资产及前序号→一次切换draft=false且make_latest=false。目录签名由该可信发布阶段的专用步骤持有密钥，目录包含最终assetId后才签。已有同名草稿资产必须匹配持久构建记录和哈希才复用；不匹配则在尚未公开的该任务草稿内重新准备，绝不覆盖公开版本。所有签名/发布步骤禁止运行外部PR代码、pull_request_target或拉取任意工件；Action锁定提交，检查工作流加入分支/标签过滤，v1规则不变。

建议Secrets：`SODECK_V2_UPDATER_PRIVATE_KEY`及密码用于Tauri installer签名；`SODECK_V2_RELEASE_PRIVATE_KEY`用于目录与官方插件包签名，公钥进代码。已有v1的TAURI_SIGNING_*是否存在及取值不读取、不复用。生成/保管正式私钥和录入GitHub Secrets作为发布配置动作；不把秘密写进日志、仓库、客户端或对话。

新增发布配置overlay：保留开发标识com.sodeck.v2.dev；stable使用com.sodeck.v2、preview使用com.sodeck.v2.preview，产品/安装目录分离。构建NSIS currentUser安装包和Tauri updater资产；安装器包含宿主exe、四个冻结的随附包、正式公钥/资源索引、主题/品牌/许可证，保留WebView2前置安装检查。安装版不包含portable.json/data或QA材料。首次安装与卸载保留数据策略需原生测试。

核心安装包只在核心版本变化时生成，即使包含出厂插件也不强行升级已有插件；现有prepare_bundled播种规则继续生效。变化插件的独立包走在线服务。主程序/host的资源定位必须由Tauri安装资源目录解析，不能依赖仓库cwd。

正式安装配置通过独立release入口传入，不能直接调用总是注入SODECK_DEV_MODE/开发包/开发公钥的`scripts/desktop.mjs`。beforeBuildCommand只构建桌面及所需共享依赖，插件包由release计划按变化项另行构建。`app.runtimeInfo.get`读取构建渠道，新增安装发行元数据与当前用户安装注册位置核对；现有mode=standard只代表无便携标记，不能单凭它认定EXE来自NSIS安装。便携核心不提供安装按钮，开发版默认不查官方源；QA源/公钥仅在隔离测试构建启用，正式构建不接受环境变量或RPC改更新地址。

## 4. 运行时代码落点

| 模块 | 拟改动及职责 |
| --- | --- |
| 新增crates/updater | catalog/签名、GitHub client、版本/兼容规划、有限下载/cache、更新journal；不依赖UI或业务插件 |
| 新增desktop updates服务 | 绑定调用者/核心审计、状态feed、操作回执、定时检查、用户下载/安装、来源追踪与插件manager适配、安装退出交接 |
| plugin-manager package/store | 显式CompatibilityContext、已验证包及源摘要、供目标核心缓存的验证边界；不向UI开放“忽略兼容性” |
| app.rs/main.rs/startup_config.rs | 注册服务；启动先恢复更新journal、核对实际核心；退出审查后调用安装交接并释放原有资源 |
| settings registry/root/updates.tsx | core:updates页、策略草稿与共同保存协调；读取下载/安装状态，动作不伪装成设置草稿 |
| settings/plugins.tsx | 按ID显示可用/待下载/等待核心/失败版本与单项动作；保留启用及当前/候选签名 |
| contracts/SDK | update-v1 schema、TS生成/严格验证、状态feed；API/SDK预计6.3兼容增量，不提高现有包的requires |
| tauri发布overlay/Cargo/package脚本 | NSIS/resources/updater原生依赖、正式公钥和渠道；保留现有开发命令 |
| 四插件 | 增加专属发布与在线A/B回归，原则上不改业务代码；仅实际改动时升其版本，SDK新增不自动强迫所有插件重编译 |

### 4.1 文件职责与依赖方向

| 拟新增/修改位置 | 可独立实施的职责 |
| --- | --- |
| `crates/updater/src/catalog.rs`、`signature.rs` | 严格解析、域隔离验签、平台/资产交叉验证；拒绝重复键/ID、非法semver、超过预算及未知必需格式 |
| `crates/updater/src/github.rs`、`download.rs` | 固定源HTTP、分页/ETag/限流、受控重定向、流式落盘、哈希/容量/取消；不持有插件运行实例 |
| `crates/updater/src/plan.rs`、`model.rs` | 以VerifiedCatalog、实际RuntimeIdentity、插件/启用/来源快照生成UpdatePlan；纯规划，不安装或联网 |
| `crates/updater/src/store.rs`、`recovery.rs` | 更新journal、source记录、操作幂等/结果核对及自有缓存清理；只接收已验证记录 |
| `apps/desktop/src-tauri/src/updates/{mod,service,plugin,installer,policy}.rs` | 核心更新能力的桌面适配，绑定调用者/审计/feed、单一调度器、管理器事务、Tauri installer及设置保存 |
| `crates/plugin-manager/src/package/{snapshot,compatibility}.rs`、`store.rs` | 拆开完整性快照与环境匹配；提供导出manifest/digest/schema/选择revision的只读快照，以及只接受当前环境的准备入口 |
| `contracts/updates-v1.schema.json`、`scripts/generate-updates.mjs`、`packages/sdk/src/updates{,.generated}.ts` | 方法/结果/feed同源，生成Rust模型与TS验证；加入现有check、公开SDK导出、capabilities与宿主转发/校验表 |
| `apps/desktop/src/settings/{updates,update-model}.tsx/ts`、`root.tsx`、`plugins.tsx` | 同一updatesClient及状态模型；更新页和插件行复用选择/进度，autoCheck进入saveParticipants；不把下载列入设置保存操作 |
| `window_close_review.rs`、`app.rs`、`startup_config.rs` | 退出动作与最终处置分离，增加安装/普通重启的私有动作；启动恢复先于插件候选激活 |
| `scripts/release/{inputs,plan,build,catalog,publish}.mjs`、发布配置与workflow | 五组件构建、实际资产验证、签名及最后公开；新命令纳入开发入口，当前不存在 |

依赖方向：设置UI/插件→公开SDK→桌面调用网关→updates适配→updater纯逻辑/HTTP存储与plugin-manager/Tauri。updater不依赖桌面、业务插件或core数据库；plugin-manager不依赖updater，接收明确的兼容上下文和已验证候选。desktop持有全局服务生命周期，不在SettingsWindow挂载时启动后台服务。网络采用Tokio任务、取消令牌和有界队列，ZIP/文件哈希等阻塞工作移到有界blocking任务；等待任务时不持有std::sync::Mutex。

Tauri updater只由Rust适配层调用，不直接向WebView开放安装命令或任意URL。采用官方Windows安装实现和其签名验证；Tauri updater在Windows安装时会结束父进程，不能从普通按钮直接install。先通过现有关闭审查、冻结新写操作、释放worker/会话/下载与数据锁，再交接；on_before_exit只做已完成异步清理的最终兜底，不把异步业务清理塞入同步回调。

适配层从已验证目录构造Tauri格式的更新元数据，通过`Updater::restore_update`恢复Update，随后`verify`/`install`校验同一缓存字节；不用构造有私有字段的Update，也不架本地更新HTTP服务。2026-10-29只读核实上游v2分支当前标称2.14.1，公开了上述入口，restore_update强制要求签名绑定版本。拟采用具备这些API的已发布精确版本并锁Cargo.lock，搭配支持`signer sign --app-version`的Tauri CLI；当前仓库CLI2.10.0不能仅凭版本号认定满足。先在实施关口G1验证注册表发行包和CLI配对、版本签名以及NSIS A→B，再锁定依赖；不能以关闭signed-version检查绕过失败。

下载走updater流式缓存，安装前在有限blocking任务读一个核心文件（≤256MiB）供Tauri重验；同一时刻仅一个安装交接，内存额外包缓冲最多一个256MiB，不让UI/RPC搬运包字节。Tauri updater仅负责原生安装交接，不再调用其download_and_install把两次用户动作合并。安装参数固定在Rust，仅目标平台currentUser，不允许接口传EXE路径、命令行或自选URL。

## 5. 状态、来源与恢复

`updates/preferences.json`保存自动检查开关，`updates/state.json`保存generation/revision、已验证目录序号、操作摘要、用户选定安装的组件和安装交接，`updates/sources.json`保存按插件ID和准确包摘要绑定的来源/跟随状态，`updates/catalogs/<channel>/<sequence>/`保存目录原字节和签名，`updates/cache/<archiveSha256>`保存完成文件。写入复用原子替换与同步落盘；这些为新增独立JSON状态，不改core.sqlite v7、观测库v2或既有activation.json字段。损坏状态不自动安装/清空，保留诊断原件，重新检查与校验后才能重新准备。来源状态不可核对时视为local，设置仍可打开；autoCheck偏好损坏时暂停后台检查并提示，用户可手动检查。保存策略有并发修订及有界回执。

组件状态：available→用户下载→downloading→verifying→downloaded→用户安装→preparing→ready；另有waitingForCore、blocked、failed、cancelled。downloaded不写插件pending，不授权核心交接；新进程不能把缓存自动提升为ready。核心交接为用户安装/退出放行后handoffPending→handoffAttempted→启动时核对installed/未完成，同步安装错误另记handoffFailed。操作ID及计划revision防重复，迟到结果检查generation；最多128个操作摘要，更新结果跨窗口可核对。发布已准备包的原子点后取消只结束该操作，不能假称已撤销已写pending。

来源记录单独关联pluginId+准确包摘要，值为bundled/official/local。只有受验证的官方目录和实际匹配资产能产生official来源；UI参数不能伪造。手动导入成功后变为local；相同ID不会自然获得官方跟随。来源记录失败按local处理，不扩大自动覆盖范围。旧安装中无法证明来源的包保留local；首次随附包可由安装清单确认。

所有包发布入口（现有prepare、legacy导入、随附播种和新在线安装）统一经过desktop的来源协调器。`sources.json`一次原子写入“发布意图+来源变更+操作ID”，随后才在manager发布锁内accept_staged，最后写回操作结果。意图带前后选择摘要；导入一旦进入提交阶段先暂停该ID官方跟随，即使导入的是字节完全相同的官方包也记为local；安全侧的暂停不因结果未知自动恢复。来源写失败则不发布新pending，返回可重试STORAGE_ERROR，不能先改插件再默默遗漏来源记录。启动按intent和manager实际selected/pending的摘要核对：匹配候选则完成回执，仍是旧选择则记未发布，其他情况保持local/冲突。在线与本地竞争前先比对来源revision；回退旧插件时也只采用旧摘要对应的来源，不继承失败候选的official身份。来源损坏阻止来源提交时仍保留原插件可运行及管理页，重新保存可核对的local来源后才继续导入。

随附来源只在实际播种缺失ID时写入；only_missing跳过已装ID不能把local或已更新official改回bundled。首次正式安装使用隔离的正式/预览数据目录，不自动接管dev.20的开发签名包；更新已公开同版本包的签名钥也属于内容变化，必须升插件版本。

`CompatibilityContext`必须含准确coreCompatibilityVersion/API/capabilities/platform，外层RuntimeIdentity另有appVersion/channel/buildId/dataFormats。future-core包只在updates/cache验证，持久绑定目标核心appVersion/构建身份/资产摘要及用户安装意图，旧manager不会将其写入activation.pending。结构/完整性快照类型不自带执行权限，只有对当前运行环境验证后的`RunnablePackage`才可交给运行/accept_staged入口；目标环境验证结果使用不同类型，只能留在updates。新核心启动只处理用户已选定安装的候选，凭实际版本和能力再次验证、核对启用偏好再交给manager；仅下载未点安装的不自动激活。目标没有安装、版本不同或数据schema不支持时保持阻塞。当前可兼容插件独立更新照常可用，不因为另一个核心下载失败而全部停止。

计划中的插件启动集合优先取已提交pending（含停用偏好变化后将启用的包），没有pending才取selected；本次选定替代包覆盖该ID前须验证且展示依赖。规划返回当前核心可用更新和目标核心可用更新两组，以及每项blockedReasons、requiredComponentIds、下载字节和重启需求。选择不会暗中带选其他组件；核心依赖某包时先展示并由用户勾选依赖。候选仅可用历史中最高兼容且严格高于本机的semver，稳定渠道不选预发布插件，+build元数据不构成更高版本。范围计算复用Rust semver为权威，JS发布器通过同一验证工具出规划并跑共同向量，不允许Node semver与Rust对预发布范围判断分叉。

同一ID若已有手动pending或正在导入，在线服务不覆盖；显示有本地候选，待用户处理后重规划。导入、在线prepare和启停保存使用同一manager发布锁，网络/哈希不持有manager锁；与退出锁按已有顺序交接，不能在锁内调用worker。

安装计划绑定目录revision、插件激活generation及启用偏好的generation/revision；兼容检查以目标启动的已保存启用集合为准。退出审查中保存草稿或另一窗口/插件请求改变状态后，安装交接前必须重新核对，过期计划返回CONFLICT并保留下载。核心依赖的插件安装意图在关闭审查放行后持久化；审查取消不新增此类意图。独立插件“安装所选更新”允许每项成功/失败分开记录，明确已准备项，不能声称跨核心安装器和多个插件写入为一笔原子事务。

管理器当前prepare不会增加activation.generation，故planRevision还必须绑定selected/pending/failures清单的规范化摘要；不能只依靠generation发现导入变化。核心安装门禁同时比对全部dataFormats，任何会触发P-52重建的目标都标blocked/manualRequired，不只检查coreSchema。没有迁移器的插件schema变化同样阻止安装而不是等初始化时才失败。

### 5.1 核心安装、重启与启动恢复

1. 用户选中已下载组件点“安装并重启”：重验缓存、计划、安装身份和数据格式，保存可查询的reviewing操作；调用`Action::InstallUpdate { operationId, planRevision }`。新动作和普通Exit共享设置/连接页/插件prepareClose检查；RestartForUpdates也共享该审查，但只切换已准备插件。
2. 所有参与者放行后，取得提交门闩，重新核对启用/来源/选包/草稿修订；若变动或写盘失败，解除审查并保留原会话。通过后原子保存handoffPending及同次用户选定的依赖包意图，冻结新写操作，再取消/等待未完成下载。
3. 将现有“资源shutdown”和“最终exit/restart/install”拆成两个阶段。先停止worker、PTY/SFTP/隧道/采样、排空记录、销毁窗口，保留安装所需AppHandle与已验证Update。InstallUpdate不调用会启动150ms退出线程的普通app.exit；最终仅由Tauri.install交接并退出。只更新插件的重启则在清理后用Tauri重启能力启动同一路径。
4. app.lock和plugin writer.lock继续持有到父进程实际退出，防止清理与交接间第二实例进入；NSIS升级分支等待旧进程退出后替换文件/重开，重开进程取得锁后才启动。不得提前解锁却仍允许旧状态被写。G1验证安装器等待机制、无第二进程抢占和所有子进程退出；不能依赖固定sleep当成功条件。
5. Tauri调用前记录`handoffAttempted`，它仅表示已尝试启动安装器。上游成功路径会退出父进程，不能承诺后续写回installerStarted；同步返回错误则记录handoffFailed。若资源已关闭，提供原生错误与“重新打开当前版本/关闭”恢复，不声称PTY仍在；重新打开不复用原安装动作、不自动重试安装。
6. 新进程先解析不可变RuntimeIdentity、取得数据锁并核对updates journal/dataFormats，再走原有数据库预检。实际appVersion/channel/buildId与目标吻合才记核心installed；之后在prepare_bundled与activate_pending之间恢复已授权的future候选。旧核心、安装取消、其它版本或坏journal只提示未完成，不自动重跑安装器。插件最终结果靠manager实际selected/pending/failures确认，初始化失败仍保留旧包及数据，不重投同摘要失败候选。

核心依赖与多插件安装不是跨文件原子事务；每项以自己的发布点为准。已成功写pending的当前核心兼容插件保持“重启后生效”，即使同批另一项失败；future候选始终受目标核心门禁约束。UI明确分项结果和剩余操作。缓存/准备可取消阶段使用操作generation，提交点之后返回实际已准备结果，不虚报撤销。

已安装的停用官方插件也可由用户下载并安装为候选，manager的boot enablement始终优先；下载器本身不执行插件初始化。失败版本记录候选摘要，定时检查不重新投递同一失败包；用户显式重试或安装更高版本才重新准备。

在线候选开始启动试运行前持久化`activationAttempt`（启动ID/包摘要/旧选择），成功以manager实际选择核对后结案。若下次启动发现上次试运行中断且尚无成功选择，恢复旧选择并通过manager的条件撤销接口移除匹配摘要pending、记录失败；随后普通activate_pending不得再次自动执行同候选。撤销必须匹配ID/版本/摘要，不能撤掉后来手动导入的不同候选；手动重试再建新的明确意图。停用候选不产生attempt，不因连续重启消耗试运行次数。

网络预算：HTTPS固定GitHub源及其受控资产重定向，连接10秒、元数据单次15秒/整轮60秒；单下载无进度30秒、总15分钟；并发最多2。目录正文≤1MiB、≤1024条组件记录、每组件≤8版本；插件沿用32MiB/展开128MiB/2000文件；核心安装资产≤256MiB、更新cache≤512MiB。不完整part不当成缓存，取消/失败删除；完成后再次hash并验签，安装前重验。清理仅删除updates/cache自有未引用文件，不删除运行包、原插件数据、凭据或任意外部路径。

容量计算含预留下载长度、part、已完成缓存和元数据；future/ready/handoff引用的文件固定保留，无空间即返回CAPACITY，不为新下载驱逐已准备项。同哈希下载去重、不同操作分别保留回执；一次最多选择64组件，队列最多64项，每项结果≤64KiB，活动操作不淘汰。catalog签名侧文件≤16KiB，GitHub单页正文≤8MiB并计入整轮预算。HTTPS只允许api.github.com、github.com及逐项白名单的GitHub资产CDN，最多5次重定向，无用户名/密码、HTTP降级、任意端口或外部主机；不使用宽泛*.githubusercontent.com。尊重操作系统网络代理但不读取SoDeck服务器代理配置，客户端不携带GitHub认证令牌。

## 6. 公开契约与审计

新增能力`updates.management.v1`，API/SDK6.2→6.3兼容增量，首次开发版本拟dev.21；具体发布semver和签名公钥在发行配置时明确。不是权限审批表，沿用既有调用者绑定与逐次记录。

| 方法 | 请求与结果要点 |
| --- | --- |
| updates.status.get / updates.components.list | 状态/渠道/检查时间/策略revision、有限分页组件与版本/原因；分页绑定目录revision |
| updates.plan.get | componentIds、当前快照revision；返回planRevision、逐项下载/安装目标、显式依赖与阻塞原因；不产生下载或安装副作用 |
| updates.check | operationId，返回检查任务；节流、去重，不能指定任意外部源 |
| updates.download | operationId、planRevision、componentIds；解析并展示依赖，下载和验证用户选定项，只产生downloaded缓存 |
| updates.install | operationId、planRevision、componentIds；仅接收已验证且用户明确选定组件，插件写pending/未来核心安装意图，核心进入退出审查 |
| updates.cancel / operation.get | 取消本任务未发布阶段、查询实际回执；超时不自动重发写操作 |
| updates.policy.validate/commit/operation.get | generation/revision、autoCheck，统一设置保存10秒未知结果核对 |
| updates.source.followOfficial | pluginId、expectedDigest、operationId；核对当前来源与目录，不自动降级 |
| app.restartForUpdates | planRevision、operationId；仅处理已点击安装的ready候选，经过原生退出审查；不能把只下载缓存自动改为安装 |

请求/结果上限64KiB，组件列表默认50/最多100并受字节预算限制；operationId为UUID，同调用者+ID+同规范化请求返回原任务，不同请求返回CONFLICT。planRevision由目录摘要、实际运行身份、选中组件、manager选择摘要、启用/来源修订生成；进度变化不让计划失效。operation.get可跨窗口核对已持久任务，RESULT_UNKNOWN不等于失败，也不由UI自动重发安装。

能力仍按P-14对所有合规调用者公开且逐次记录，不能凭UI声称“用户已点击”直接结束进程。shell/settings调用由原生窗口绑定身份；插件发起download/install/restart请求只打开核心设置中待操作的同一计划，返回requiresUserAction，核心按钮才产生绑定operationId/planRevision的本次手势意图，意图不能由RPC布尔值伪造。不添加插件权限表或来源审批弹窗，所有插件适用同一用户操作语义。check/status及普通策略操作沿用公开能力调用约定。

原生状态feed`updates.state.v1`只提供revision/阶段/进度摘要，进度最多每秒4次；页面再查分页列表，不广播清单正文或包字节。退出或消费者销毁回收订阅；UI退出不删除全局下载。错误类别包括UPDATE_SOURCE_INVALID、UPDATE_SIGNATURE_INVALID、UPDATE_HASH_MISMATCH、UPDATE_INCOMPATIBLE、RATE_LIMITED、CAPACITY、CONFLICT、STORAGE_ERROR、RESULT_UNKNOWN，生成类型和Rust/TS检查同源。

记录检查/下载/验证/准备/安装交接及结果，限组件ID、版本、渠道、阶段、字节数、错误码；不记录URL临时查询令牌、私钥、服务器配置或安装包正文。更新网络由核心本地下载器持有，不借用远端服务器SSH/隧道连接。

## 7. 验证及实施顺序

1. 建立最小目录/身份契约及Windows NSIS/Tauri交接实验所需边界；先在隔离标识用测试密钥跑A→B（G1）。失败不提前宣布已具备在线更新。
2. 完整契约/目录/签名/发布规划与单元测试，验证官方原包和本地来源不串用（G2）。
3. 有界下载/journal/兼容计划、旧/目标核心缓存、插件manager适配。
4. 设置页及退出/托盘协调，浏览器测试和真实Windows关闭流程。
5. 四插件专属真实业务、单包A→B和所有启停组合；core v7/DPAPI/插件设置副本哈希前后核对。
6. 发布workflow本地规划检查；得到Git提交/推送及隔离预览验证授权、签名配置就绪后真实运行GitHub Actions与公开预览下载；正式发布另记。

拟新增tests/updater目录、tests/ui/updates.spec.ts、tests/desktop/online-updates.mjs、online-update-installer.mjs及scripts/release测试；名字是方案入口，当前均不存在。准备/操作/预期与清理由OU-01～31和四插件专项定义，不把通过类型检查等同安装完成。

| 关口 | 交付及通过条件 | 失败/停止条件 |
| --- | --- | --- |
| G1 原生安装适配 | 隔离QA currentUser NSIS A/B，测试签名钥；restore_update→verify→install，真实A→B，坏/错版本签名拒绝，退出取消不清理，安装失败能重开A；同账号DPAPI仍可用 | 上游已发行API/CLI不配套、安装重开/锁无法满足则停止安装集成并记录替代细节，不先搭完UI宣称完成；不修改生产密钥或日常安装 |
| G2 发布与契约 | 五组件规划/同源schema/签名/semver共同向量；仅插件改版不重建核心，漏升版拒绝；运行现有check和相关契约测试 | 任一独立性/签名/兼容不变量不成立不能进入发布 |
| G3 状态与恢复 | 下载取消、来源提交各断点、并发导入/设置、future-core、损坏journal及同摘要失败重试用例通过 | 不接受先改预期、清状态或自动禁用来掩盖失败 |
| G4 UI与业务 | 三主题/键盘/200%/窄窗；启用集合16种在规划器覆盖，原生覆盖全开/全关/单插件；四插件实际UI/worker A/B及专用Linux业务 | 不恢复T-024已取消的长压测；测试覆盖新增边界，原生安装与模拟结果分开 |
| G5 外部端到端 | 经对应授权配置Secrets/提交推送、专用预览tag实跑Actions、用真实GitHub资产升级QA预览安装；保留run/release/哈希/数据一致性证据 | 未获外部操作授权时先交付本地实现与测试，T-029整体保留“开发中：外部验证待完成”，不代填正式发布或用户验收 |

现有npm check/test:contracts、定向Rust manager/core与设置回归、前端build、Windows release构建纳入交付。仅编辑Markdown的本轮只做链接、修订及一致性检查，以上测试尚未运行。

## 8. 外部依据与未完成事项

只读核查日期2026-10-29：[GitHub仓库Release](https://github.com/BgaSol/soDeck/releases)、[Tauri updater文档](https://v2.tauri.app/plugin/updater/)、[Windows安装器](https://v2.tauri.app/distribute/windows-installer/)、[GitHub发布API](https://docs.github.com/en/rest/releases/releases)、[Tauri updater原生实现](https://github.com/tauri-apps/plugins-workspace/blob/v2/plugins/updater/src/updater.rs)。参考源可能演进，实现需锁定依赖版本。

尚未配置正式v2密钥/Secrets、生成安装器、部署workflow或发布v2资产。生产证书不是更新签名的替代品；v1密钥不自动用于v2。真实GitHub链路需要后续部署授权，不能用本地HTTP模拟替代实际完成结论。当前仅文档方案。

# 0.10.4

- JSON/JSONC 复用固定版本 Monaco 的内置 tokenizer，修复字段名与冒号分行时误用字符串颜色；保留字面字符串、JSONC 注释及浅色/深色主题。

词法接入、JSON 注释颜色适配和主题映射均在文件插件内；未新增或升级生产依赖，核心 API 不变。

本地文件插件类型/格式、Rust 测试、正式编辑器构建与完整浏览器回归通过，覆盖合法 JSON 跨行字段名、数组字符串、美元符号、JSONC 跨行注释及浅色/深色主题。

包来源：[4bc4ff4f23982b855996b15c16098bb915cfc0e5](https://github.com/BgaSol/soDeck/commit/4bc4ff4f23982b855996b15c16098bb915cfc0e5)；原始独立打包与实包验收：[43130591301](https://github.com/BgaSol/soDeck/actions/runs/43130591301)。Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 五平台全部原生构建，实际归档的架构、资源、初始化、视图创建/释放和退出全部一次通过；五份报告与原包字节及来源提交的绑定校验通过。

[来源提交 Check](https://github.com/BgaSol/soDeck/actions/runs/43130513580) 的五平台源码、文件/SFTP/进程、正式编辑器类型和完整浏览器回归通过。该 run 的无关网络容量回归首次超时、相同源码复验及其余验证范围见[核心记录](../../docs/releases/v0.23.7.md)。

发行：[files 0.10.4](https://github.com/BgaSol/soDeck/releases/tag/plugin-files-v0.10.4)，非 Latest。五份归档和更新清单共六个附件沿用原始 run；发布前只同步说明与清单的验收记录，程序归档字节保持不变。正式发布后附件 ID、大小、摘要和匿名更新清单下载复核一致。

[真实正式版升级 0.10.3 → 0.10.4](https://github.com/BgaSol/soDeck/actions/runs/43133168068) 在五个平台全部一次通过，宿主固定为核心 0.23.7。生产标签发现选中本版并保留完整清单和说明；下载准备期间旧插件继续运行，重启后激活 0.10.4、清除待重启状态，并保留服务器资料与文件设置。五份完成报告和公开清单来源均已复核；仅补充发行文档的夹具来源见[核心记录](../../docs/releases/v0.23.7.md)。

本轮未执行原生远程包组合、编辑器设备、真实 IME/DPI/Wayland、生产服务器或系统剪贴板/默认应用专项。Monaco 升级仍需复验词法模块、注释差异和主题映射。

# 0.10.3

- 编辑器菜单复用已有 Floating UI，避开窗口边缘并在极矮视口内部滚动；保留键盘、焦点和剪贴板时效，关闭时释放定位资源。
- 偏好变更使用 Monaco 公共视图快照，保留多选区和隐藏实例的逻辑阅读位置；跨窗持久状态仍为版本 1。
- 插件客户端使用标准库有界逐行读取，保留超长控制帧立即退出，Interrupted 自动重试。

未新增或升级生产依赖，核心 API 与 RPC 字段保持兼容。

包来源：[90d49f4c7ef509f89025696a71ddddebfec99d51](https://github.com/BgaSol/soDeck/commit/90d49f4c7ef509f89025696a71ddddebfec99d51)；原始独立打包与验收：[43083446157](https://github.com/BgaSol/soDeck/actions/runs/43083446157)。Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 五平台原生归档的架构、摘要、资源、初始化、视图创建/释放与退出全部一次通过。

[来源 Check](https://github.com/BgaSol/soDeck/actions/runs/43083351828) 的五平台源码、文件/SFTP/进程、生产编辑器类型与完整浏览器回归通过，包含菜单边缘/缩窗/键盘/剪贴板时效、多选区和隐藏后偏好恢复。本地实际 WebView2 的 Worker/CSP、搜索、安全保存和退出通过。[原包组合](https://github.com/BgaSol/soDeck/actions/runs/43088114019) 配合核心 0.23.5、terminal 0.8.4、network-proxy 0.4.2 在五平台通过远程流程与重开；组合脚本来自仅增加诊断的 330dc90，程序来源仍为上述提交。

发行：[files 0.10.3](https://github.com/BgaSol/soDeck/releases/tag/plugin-files-v0.10.3)，非 Latest。五份归档和更新清单共六个附件沿用原始 run；发布前只同步说明与清单的验收记录，程序归档字节保持不变，正式发布后附件摘要复核一致。

[真实正式版升级 0.10.2 → 0.10.3](https://github.com/BgaSol/soDeck/actions/runs/43089745312) 在五平台通过，宿主固定为核心 0.23.5。匿名正式附件下载使用本轮流式大小/SHA-256 校验；下载准备期间旧插件继续运行，重启后激活 0.10.3、清除待重启状态并保留服务器资料与文件设置。测试脚本来自仅增加失败诊断的 330dc90，没有改变正式归档。

Windows CI 编辑器专项仍因 WebView2 调试端口不可用而失败，未当作通过；失败与两平台复验见[核心记录](../../docs/releases/v0.23.5.md)。未验证真实 IME/DPI/Wayland、生产服务器、netem 或本轮系统文件剪贴板/默认应用专项。

# 0.10.2

- 修复 JSON/JSONC 字符串误用 shell 插值规则的着色，美元符号保持字符串颜色；保留 JSONC 注释及其他语言原有规则。

[来源提交 Check](https://github.com/BgaSol/soDeck/actions/runs/43058813431) 全部通过，包含五平台核心/插件/共享客户端源码、Linux 完整界面、文件编辑器及普通 OpenSSH/SFTP、终端、网络、状态、Docker和通信回归。Linux 依赖准备遇到临时 DNS 故障后，仅在原 run 重跑未通过任务，成功结果保留。

包来源：[8928e87ddeb66d3bee2da0a1c1c677e9e3f149b5](https://github.com/BgaSol/soDeck/commit/8928e87ddeb66d3bee2da0a1c1c677e9e3f149b5)；原始独立打包与验收：[43058954465](https://github.com/BgaSol/soDeck/actions/runs/43058954465)。Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 五个平台原生打包、独立实际归档验收及来源/摘要绑定全部一次成功，无重跑。实际归档的架构、摘要、资源、初始化、视图创建/释放和进程退出通过。

发行：[files 0.10.2](https://github.com/BgaSol/soDeck/releases/tag/plugin-files-v0.10.2)，非 Latest。五份归档和更新清单共六个附件沿用上述原始 run；发布前只同步更新清单与说明的验收状态，五份程序归档保持原字节。本次独立包验收不包含核心/插件原生远程组合、历史版本在线升级、生产服务器或真实 IME/DPI/Wayland 等专项。

# 0.10.1

- 构建与实际归档验收分任务执行，失败复验保留原包。
- 原生归档统一检查资源、初始化、视图生命周期和退出；发布前绑定来源与摘要，拒绝覆盖不同内容的同版本附件。
- 插件 API 与在线更新协议保持兼容。

包来源：[a52157346409d4457c4b00b5bbd3364df86df36c](https://github.com/BgaSol/soDeck/commit/a52157346409d4457c4b00b5bbd3364df86df36c)；原始打包与验收：[42914256938](https://github.com/BgaSol/soDeck/actions/runs/42914256938)。Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 全部原生构建，最终归档的摘要、资源、初始化、视图释放和退出通过；上传与验收独立执行。

[来源提交 Check](https://github.com/BgaSol/soDeck/actions/runs/42914202961) 全部通过，包含五平台核心/插件/共享客户端源码测试及相关浏览器、SSH/SFTP、终端、网络、Docker、通信回归。首次草稿索引延迟导致收尾失败，重跑 release 成功，未重编译或覆盖包。

发行：[files 0.10.1](https://github.com/BgaSol/soDeck/releases/tag/plugin-files-v0.10.1)，非 Latest。[真实正式版升级 0.10.0 → 0.10.1](https://github.com/BgaSol/soDeck/actions/runs/42919802460) 五平台通过，宿主固定为核心 0.23.1，验证下载校验、升级前原进程保留、重启激活及设置/服务器资料保留。没有将源码或包生命周期测试算作生产服务器、真实 IME/DPI/Wayland 或所有设备兼容验收。

[正式包组合](https://github.com/BgaSol/soDeck/actions/runs/42919779115) 与核心 0.23.1、terminal 0.8.2、files 0.10.1、network-proxy 0.4.1 在五平台通过 SSH/PTY/SFTP、网络、工作台与重开；系统桌面测试的具体范围及 macOS 默认关联收据跳过见[核心记录](../../docs/releases/v0.23.1.md)。

# 0.10.0

移动、剪切与改名在当前后端明确降级为不支持，移除复制后删源。批次回执丢失保留原请求并暂停，确认只查询原操作。编辑器按固定正文修订保存/导出，草稿同步失败可恢复，旧保存不能清除新输入；提交未知时禁止直接重放。

依赖核心 `files.operations.v2`、`files.editor.v1`。 API 主版本保持 1；缺少能力时初始化明确拒绝，不回退到旧流程。本版已独立发行；核心需先升级至 0.23.0 并重启，再升级插件。尚未补齐的故障组合见[剩余验收](../../docs/plans/operation-outcomes.md)。

包来源：[`fcf4f44`](https://github.com/BgaSol/soDeck/commit/fcf4f446d0e588c16da2187473c2c22a63d31f7c)；原始独立打包：[Package plugin #42883698048](https://github.com/BgaSol/soDeck/actions/runs/42883698048)；发行：[files 0.10.0](https://github.com/BgaSol/soDeck/releases/tag/plugin-files-v0.10.0)，本插件最新正式版本。五平台均为原生产物，下载原五份归档并核对 SHA-256、更新清单、包内版本、UI 入口及架构，直接发布同一草稿的六个附件。[Check #42885519869](https://github.com/BgaSol/soDeck/actions/runs/42885519869) 与 [Docker 补测 #42887442456](https://github.com/BgaSol/soDeck/actions/runs/42887442456) 对相同产品源码的五平台 Rust 及相关普通业务复验通过，后续仅修正测试，插件程序未改变。

[Verify native remote plugins #42885589898](https://github.com/BgaSol/soDeck/actions/runs/42885589898) 配合核心 0.23.0、终端 0.8.1、文件 0.10.0 和网络隧道 0.4.0，完成五平台实际包的启动、对应远程业务、移窗、重开及退出。下载的 Windows x86_64 进程另验丢失变更回执、原身份确认不重放和禁止移动回退。未重复历史版本在线升级、netem、生产服务器或 IME/DPI 等深度原生专项。

发布标签触发的[重复打包 #42888175110](https://github.com/BgaSol/soDeck/actions/runs/42888175110) 已取消，不是发行来源；原六个附件的 ID、大小与摘要保持不变。

# 0.9.0

文件编辑器增加当前正文搜索、右侧彩色缩略图、行号和按文件类型语法着色，支持手动更正语言；继续执行 1 MiB 字节上限、UTF-8/BOM/换行保留和远端安全保存。补齐本地待暂存状态、完整选区与搜索/语言恢复，避免旧保存结果清除新修改的未保存标记。新增编辑器依赖仅随文件插件打包。

本版按用户要求发布为 Latest，核心 API 主版本保持 1。实现与本次平台验证见[实施记录](https://github.com/BgaSol/soDeck/blob/8115fdcd5bed4108021a729da605fbde77afc2af/docs/implementation-file-editor-search.md)。

包来源：[`68f8eac0eeb3017806f82a49f29949b6ae48be85`](https://github.com/BgaSol/soDeck/commit/68f8eac0eeb3017806f82a49f29949b6ae48be85)；原始打包：[Package plugin #42799998357](https://github.com/BgaSol/soDeck/actions/runs/42799998357)；源码：[Check #42799928597](https://github.com/BgaSol/soDeck/actions/runs/42799928597)；发行：[files 0.9.0](https://github.com/BgaSol/soDeck/releases/tag/plugin-files-v0.9.0)，Latest。五份原生归档与更新清单直接从同一草稿发布，附件哈希和公开清单核对通过。

五平台原生包与实际核心 0.21.0、终端 0.7.2 的启动、SFTP 文件读写、工作区恢复和退出通过；Windows 两个补测分别为 [42804113763](https://github.com/BgaSol/soDeck/actions/runs/42804113763) / [42804127755](https://github.com/BgaSol/soDeck/actions/runs/42804127755)，其余三个成功目标位于 [42800515265](https://github.com/BgaSol/soDeck/actions/runs/42800515265)。首次 Windows 额外深度专项因 CI 未开启 WebView2 调试端口失败，失败记录保留；本机使用正式包的深度专项通过，不能外推为 CI 或其他平台深度界面通过。上述实施记录保留详细失败诊断。系统 IME、触摸/辅助技术及系统剪贴板的人机验收未执行；完整性能 P95、长期内存与多文件负载预算尚未测量。Windows ARM64、Linux x86_64/ARM64、macOS ARM64 没有同等深度编辑器专项通过证据。

# 0.8.1

共享客户端改用固定 base64 0.23.1；保留标准编码、严格解码、原始字节分页和既有 API。本版同步锁文件并重新独立打包，插件业务和核心 API 主版本不变。

本版为未发布候选，最终按要求仅推送 main。实现见[接入记录](https://github.com/BgaSol/soDeck/blob/8115fdcd5bed4108021a729da605fbde77afc2af/docs/implementation-base64-opener-adoption.md)；核心与插件继续独立发行，核心包不含业务插件。

包来源：[`0abaf3e8ccc2045e437dac723d898eca87759f6c`](https://github.com/BgaSol/soDeck/commit/0abaf3e8ccc2045e437dac723d898eca87759f6c)；原始独立打包：[Package plugin #42779053034](https://github.com/BgaSol/soDeck/actions/runs/42779053034)；[草稿 440084221](https://github.com/BgaSol/soDeck/releases/tag/untagged-7ac77c37e2cbdf46a73a)，保留原五平台归档与更新清单，共六个附件，未正式发布。

五个原生目标独立构建、解包、包内进程启动与退出通过；ARM64 使用原生程序。最终源码 CI、实际核心候选失败及未执行的新版本包组合/升级范围见[核心候选记录](../../docs/releases/v0.21.1.md)。源码 main 的后续测试和 CI 修正不改变这些归档来源，没有重建或覆盖附件。

# 0.8.0

接入核心 0.17.0 的工作台与 UI v2。文件树可在左右侧栏、中央和独立窗口停靠；编辑页参与分屏和移窗，保留同一文件实例、未保存草稿及编辑位置。文件跟随同工作区最近实际操作的终端，跨窗定位复用已有工具与文件。

逻辑创建、只读候选、冻结同步与激活分开，普通草稿同步不会冻结后续下载和输入。请先升级核心，再升级插件并重启；核心 API 主版本保持 1，没有新增生产依赖。0.7.0 的旧草稿属于已撤销的另一轮整改，不复用其归档。

包来源：[`7a3747e`](https://github.com/BgaSol/soDeck/commit/7a3747e009bf0ee51ec5be5d1ba8c72dc678cf2d)；原始独立打包：[Package plugin #42573192012](https://github.com/BgaSol/soDeck/actions/runs/42573192012)；发行：[files 0.8.0](https://github.com/BgaSol/soDeck/releases/tag/plugin-files-v0.8.0)，非 Latest。五平台原生包与包内进程生命周期验收通过；修正测试后的源码检查、实际包远程组合、Windows 四插件原生交互以及未验证范围统一见[核心 0.17.0](../../docs/releases/v0.17.0.md)。插件业务源码与原包一致，复用同一份归档。

# 0.6.0

文件树底部显示实际跟随、忙碌、手动浏览、不可用和目录访问失败状态，展开可查看原因并重新定位。终端执行普通命令后，同一路径不会把手动浏览的位置拉回；自动定位仍可被手动导航取消。

支持终端 0.4.0 的自动目录报告，经核心 0.14.0 校验文件系统标记后定位。无法对应或访问失败时保留原位置；旧核心明确提示升级。源码及本轮发布验收统一见[核心版本记录](../../docs/releases/v0.14.0.md)。

包来源：[`b2ee37f`](https://github.com/BgaSol/soDeck/commit/b2ee37f850c53806bc986e5213b7d11810e17033)；原始独立打包：[Package plugin #42516555138](https://github.com/BgaSol/soDeck/actions/runs/42516555138)；发行：[files 0.6.0](https://github.com/BgaSol/soDeck/releases/tag/plugin-files-v0.6.0)。五平台原生包与实际包远程组合通过，Linux 实际包验证标记匹配及错误标记拒绝；完整源码检查、重试原因和未验证范围见上述核心记录。

# 0.5.0

文件侧栏增加默认开启、可持久保存的“跟随终端”，只跟随同工作区当前活动终端的可靠目录。手动浏览不因同路径报告回跳；关闭跟随、隐藏或目录未知时保留位置，恢复后定位当前有效目标。过期跟随结果不改变选择和滚动，后台采集变化不取消手动定位。

系统状态插件可提供挂载、只读与容量标注；拓扑和容量分别判断过期，未知值不显示为 0。多个来源须选择，容量只与同一来源、同一挂载关联；部分扫描不误报卸载。标注原位更新，窄侧栏可用。

完整联动需核心 0.13.0、终端 0.3.0 与系统状态 0.3.0。旧核心或缺少生产插件时保留原文件功能。UNC、容器与未知文件系统视角不提供位置映射，不新增生产依赖。

本机文件协议、实际 UI、设置、三插件组合通过。五平台原生源码、独立包与实际包 SSH/PTY/文件/网络通过；Linux x86_64 真实目录和挂载容量联动通过。另从草稿下载 Windows 包验证 WebView2 手势、关闭取消、草稿保留与最终释放。

包来源：[`dfdb826b0cedbe07323450692acc21bdc657bfca`](https://github.com/BgaSol/soDeck/commit/dfdb826b0cedbe07323450692acc21bdc657bfca)；原始打包：[Package plugin #42506429286](https://github.com/BgaSol/soDeck/actions/runs/42506429286)；源码：[Check #42506353705](https://github.com/BgaSol/soDeck/actions/runs/42506353705)；实际包：[Verify native remote plugins #42507409785](https://github.com/BgaSol/soDeck/actions/runs/42507409785)；发行：[files 0.5.0](https://github.com/BgaSol/soDeck/releases/tag/plugin-files-v0.5.0)，非 Latest。容器/chroot、UNC 和未知视角映射不支持；设备桌面互操作、netem 与历史升级未重复。

# 0.4.2

加深多巴胺主题的辅助文字，选中文件的大小等信息在黄色背景上达到小字对比度要求。

文件名称字号实际应用于目录树，合法字号与列表密度组合不会裁切文字。上传与上级目录图标更容易区分。

传输任务将成功项收为可展开摘要，冲突、失败和需要核对的结果保持可见，刷新保留冲突选项和焦点。编辑器明确区分服务器当前文件与包含未保存修改的草稿，断线后仍可导出草稿。

# 0.4.1

文件菜单改用短名称和分组，“新建/上传”使用一级子菜单，展开折叠回到目录行。按选择和目标提供操作，空剪贴板与无效来源时粘贴置灰；复制路径不会启用文件粘贴。菜单四边描边完整，小窗口仅内部纵向滚动；左右空间不足的子菜单向上/下展开，避免遮住触发项。

打开后缩窗会重新避让，键盘菜单跟随文件行，右键菜单在来源滚动时关闭。鼠标可跨越翻转后的父子菜单间距；文本菜单在小窗口与对话框内保留选区和可用操作。所需脚本随插件独立打包，使用时无需联网加载。

菜单固定操作目标和选择快照，异步对话框或剪贴板读取期间切换连接会取消旧操作。读取粘贴状态不创建输入或任务。原生本地文件状态依赖核心 `input.revision.hasFiles` 增量；旧核心不猜测菜单可用性，原有显式键盘粘贴及有效远程来源仍可使用。共享 UI 交互通知用于取消过期自动聚焦。

# 0.4.0

新增文件列表和文件编辑器两个独立设置页，支持列表密度、名称字号、目录缩进，以及编辑字体、字号、行高、Tab 宽度和换行显示。三款预置字体离线随包提供，排版变更保留正文和选区。共享三主题滚动条，并在路径输入、编辑器和文本选区提供应用文本菜单；原文件业务右键和传输流程继续有效。组件设置需要核心 0.9.0，请先升级核心，再升级插件并重启。

读取文件正文时按大小和修订核对每个数据块，草稿在读取期间变化会重新读取，避免越界或混合正文。首次读入完成后应用编辑设置并回报，原生包验收等待该回报后才提交保存。

# 0.3.0

增加文件与文件夹的批量上传下载、冲突选择、取消、失败重试和远程创建/改名/删除/复制移动。本地文件剪贴板和原生拖入入口已接入；不实现远程到系统的粘贴或拖出。需要核心 0.8.4 的 `files.operations.v1` / `files.localInputs.v1`。先升级核心，再升级文件插件并重启。原生包与系统桌面互操作的实际验证范围见 [文件管理实施记录](https://github.com/BgaSol/soDeck/blob/8115fdcd5bed4108021a729da605fbde77afc2af/docs/implementation-file-management.md)。

# 0.2.1

跟随核心统一的多巴胺、浅色和深色主题，保持目录树、菜单与状态区域一致。复制文件路径改用核心原生剪贴板桥。请先升级核心至 0.7.0，再升级插件并重启；保留目录树、编辑、草稿、冲突检查和下载流程。

# 0.2.0

恢复旧版紧凑文件侧栏和多巴胺风格。增加多分支目录树、路径定位、独立分页、键盘与右键操作，支持移窗后恢复导航。需要核心 0.6.1 的 files.directoryCursors.v1；保持编辑、草稿、冲突检查和下载流程。

# 0.1.0

真实 SFTP 目录、UTF-8 小文件编辑、版本检查与单文件下载。需要支持 files.v1 的核心。

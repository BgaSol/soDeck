# 0.10.1

- 构建与实际归档验收分任务执行，失败复验保留原包。
- 原生归档统一检查资源、初始化、视图生命周期和退出；发布前绑定来源与摘要，拒绝覆盖不同内容的同版本附件。
- 插件 API 与在线更新协议保持兼容。

包来源：[a52157346409d4457c4b00b5bbd3364df86df36c](https://github.com/BgaSol/soDeck/commit/a52157346409d4457c4b00b5bbd3364df86df36c)；原始打包与验收：[42914256938](https://github.com/BgaSol/soDeck/actions/runs/42914256938)。Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 全部原生构建，最终归档的摘要、资源、初始化、视图释放和退出通过；上传与验收独立执行。

[来源提交 Check](https://github.com/BgaSol/soDeck/actions/runs/42914202961) 全部通过，包含五平台核心/插件/共享客户端源码测试及相关浏览器、SSH/SFTP、终端、网络、Docker、通信回归。首次草稿索引延迟导致收尾失败，重跑 release 成功，未重编译或覆盖包。

发行：[files 0.10.1](https://github.com/BgaSol/soDeck/releases/tag/plugin-files-v0.10.1)，非 Latest。实际正式版升级专项另行记录；没有将源码或包生命周期测试算作生产服务器、真实 IME/DPI/Wayland 或所有设备兼容验收。

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

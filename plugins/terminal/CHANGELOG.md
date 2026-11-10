# 0.7.2

清单取消侧边固定与定位入口推荐。终端仍是中央多实例页面，保留默认“新建终端”行为、自动首开、分屏、移窗及原 PTY 生命周期；没有提高核心 API/UI 主版本或修改业务实现。新版核心也会忽略旧终端清单的侧边定位字段。

本轮代码与本机验证见[工具停靠实施记录](../../docs/implementation-tool-docking.md)。本版独立打包，配合核心 0.21.0；终端仍负责 PTY 与自身界面，位置、排序和持久化由核心管理。

包来源：[`066df9ec5ed6a0d7fea9c4a4d41e25472995d32e`](https://github.com/BgaSol/soDeck/commit/066df9ec5ed6a0d7fea9c4a4d41e25472995d32e)；原始独立打包：[Package plugin #42763703046](https://github.com/BgaSol/soDeck/actions/runs/42763703046)；源码检查：[Check #42763617373](https://github.com/BgaSol/soDeck/actions/runs/42763617373)；五平台实际包：[Verify native remote plugins #42764368478](https://github.com/BgaSol/soDeck/actions/runs/42764368478)。

五目标均使用原生产物，独立归档的包内程序启动与退出全部通过。实际包验收配合核心 0.21.0、既有正式文件 0.8.0 和网络隧道 0.3.0，验证真实终端输入输出、显示恢复、原生窗口接续、资料重开与正常退出，五平台全部通过。另下载原始五份归档，核对 0.7.2 清单取消侧边推荐、原生架构、GitHub 草稿摘要及更新清单；直接发布同一份六个附件。

多屏混合 DPI、用户业务服务器、netem 和历史版本升级未在本轮专项中验证，完整范围见[核心 0.21.0](../../docs/releases/v0.21.0.md)。

Windows 旧工作台测试未等待异步布局完成导致 CI 断言失败；仅修正测试等待和 CI 镜像配置后的 [Check #42766195900](https://github.com/BgaSol/soDeck/actions/runs/42766195900) 完整 Windows UI、公共检查、五平台原生源码和 Linux 真实业务全部通过。插件及核心产品代码与上述原始包来源保持一致。

正式发行：[terminal 0.7.2](https://github.com/BgaSol/soDeck/releases/tag/plugin-terminal-v0.7.2)，保持非 Latest；本轮仓库 Latest 为配套[核心 0.21.0](https://github.com/BgaSol/soDeck/releases/tag/v0.21.0)。

发布后核对标签源码、六个附件与公开更新清单，附件 ID、大小与 SHA-256 保持原始草稿一致。GitHub 补建版本标签触发的[额外打包 #42768600594](https://github.com/BgaSol/soDeck/actions/runs/42768600594) 已取消，不计为发行来源。

# 0.7.1

终端首次打开前先应用宿主主题，滚动视口的初始背景也跟随主题，消除每次打开时默认黑色背景形成的短暂矩形。保留原 PTY、输入与显示恢复流程，没有新增依赖。

Windows x86_64 类型/格式检查、95 项脚本回归、真实 xterm 三主题延迟挂载与输入回归、实际包 OpenSSH 多页/重挂载/跨窗接续通过。[本次源码 Check #42752536107](https://github.com/BgaSol/soDeck/actions/runs/42752536107) 的五平台终端源码、公共检查、Linux 终端及组件协作全部通过；完整 Windows desktop 和 netem 按改动范围跳过。

包来源：[`2c0d37190868c2961b489f7552bf3973c779f4d0`](https://github.com/BgaSol/soDeck/commit/2c0d37190868c2961b489f7552bf3973c779f4d0)；原始独立打包：[Package plugin #42752591628](https://github.com/BgaSol/soDeck/actions/runs/42752591628)；发行：[terminal 0.7.1](https://github.com/BgaSol/soDeck/releases/tag/plugin-terminal-v0.7.1)，按本次要求设为 GitHub Latest。

五目标均使用原生包；[实际包验收 #42753141416](https://github.com/BgaSol/soDeck/actions/runs/42753141416) 配合既有核心 0.20.1、文件 0.8.0，验证真实终端输入输出、显示恢复、原生窗口接续、资料重开和正常退出，五平台全部通过。本机下载原始五份归档，核对包内 0.7.1 版本及启动修复、原生架构、GitHub 草稿摘要和更新清单，沿用同一份附件，不重复打包。

正式发布草稿后 GitHub 补建版本标签触发的[额外打包 #42753954350](https://github.com/BgaSol/soDeck/actions/runs/42753954350) 已取消，不计为发行来源。发布后核对 Latest、标签源码、六个附件和最终更新清单；五份包附件 ID 与 SHA-256 保持原始草稿一致。

网络插件、原生 WebView 人工闪烁观察、多屏/DPI、用户业务服务器、netem 和历史版本升级不在本次专项范围。

# 0.7.0

接入核心 0.17.0 的工作台与 UI v2。终端页面参与嵌套分屏、移组、单页或整组拆窗和返回来源；跨窗复用原 PTY，接续输出与输入草稿，候选恢复完成后才接管输入和远程尺寸。共享目录状态跟随最近实际操作的终端。

逻辑创建、界面读取、冻结同步和激活分开；普通同步继续允许输入，迁移取消后恢复输入。请先升级核心，再升级插件并重启；核心 API 主版本保持 1，没有新增生产依赖。

包来源：[`7a3747e`](https://github.com/BgaSol/soDeck/commit/7a3747e009bf0ee51ec5be5d1ba8c72dc678cf2d)；原始独立打包：[Package plugin #42573183178](https://github.com/BgaSol/soDeck/actions/runs/42573183178)；发行：[terminal 0.7.0](https://github.com/BgaSol/soDeck/releases/tag/plugin-terminal-v0.7.0)，非 Latest。五平台原生包与包内进程生命周期验收通过；修正测试后的源码检查、实际包远程组合、Windows 四插件原生交互以及未验证范围统一见[核心 0.17.0](../../docs/releases/v0.17.0.md)。插件业务源码与原包一致，复用同一份归档。

# 0.6.0

通过清单声明默认“新建终端”行为，由终端插件处理事件并主动申请页面。创建参数支持标题、登录 shell 或指定命令、目录集成策略；指定命令只使用通用 PTY 启动能力，不探测主机 shell、不注入目录脚本。命令退出后“重新打开终端”沿用原配置，创建新页面和 PTY。

同 viewId 的首次挂载只初始化一次，重挂载复用原 PTY，失败不自动重复启动；释放时清理迟到的 PTY。off 模式忽略主机目录报告，避免容器路径带动主机文件侧栏。默认行为需要核心 0.16.0；保留旧核心页面入口与旧清单字段，API 主版本仍为 1，没有新增生产依赖。

源码、原始打包运行、正式发行与本次验证范围统一见[核心 0.16.0](../../docs/releases/v0.16.0.md)。0.5.0 的旧草稿属于已撤销的另一轮整改，本次不复用其归档。

包来源：[`c022271`](https://github.com/BgaSol/soDeck/commit/c02227168791dd2cef4b959474dc5cd8873b17ad)；原始独立打包：[Package plugin #42552644774](https://github.com/BgaSol/soDeck/actions/runs/42552644774)；发行：[terminal 0.6.0](https://github.com/BgaSol/soDeck/releases/tag/plugin-terminal-v0.6.0)，非 Latest。五平台原生源码、独立包、实际包远程组合通过；macOS 首次窗口坐标检查失败及同包重试结果、跳过范围见上述核心记录。

# 0.4.0

新终端默认自动识别远端 Linux Bash（4+）/Zsh 的目录。保留用户启动配置和提示符钩子，通过本次 shell 的目录/忙碌报告联动文件侧栏；进入嵌套环境时暂停，返回后恢复。自动模式不需要主机名，不永久修改远端配置；兼容已有 OSC 7 和关闭模式只影响新终端。

需要核心 0.14.0 与文件 0.6.0 完成自动跟随。旧核心、其他远端 shell 或冲突钩子降级为普通终端并显示原因。无新增生产依赖。源码及本轮发布验收统一见[核心版本记录](../../docs/releases/v0.14.0.md)。

包来源：[`b2ee37f`](https://github.com/BgaSol/soDeck/commit/b2ee37f850c53806bc986e5213b7d11810e17033)；原始独立打包：[Package plugin #42516550281](https://github.com/BgaSol/soDeck/actions/runs/42516550281)；发行：[terminal 0.4.0](https://github.com/BgaSol/soDeck/releases/tag/plugin-terminal-v0.4.0)。五平台原生包与实际包远程组合通过，自动报告的远端范围为 Linux Bash 4+/Zsh；完整源码检查、重试原因和未验证范围见上述核心记录。

# 0.3.0

通过核心可选 `sharedState.v1` 发布交互式终端的可靠目录报告，文件侧栏可以跟随当前活动终端。目录报告主机名在设置的“目录联动”分组配置，留空关闭上报；需要正确配置 OSC 7 的 shell。恢复输出快照不重新发布旧目录，退出、输出缺口、断线与配置改变使目录失效；重连后的旧终端不报告新连接的位置。

完整联动需核心 0.13.0 与文件 0.5.0。原 PTY、输入、外观与独立升级保持兼容。主机名配置只适用于与文件浏览相同的文件系统；同主机名容器、chroot 或嵌套 shell 无法自动识别，进入前关闭跟随。没有修改远端 shell 配置或新增生产依赖。

本机真实 PowerShell、三插件 RPC/PTY/SFTP、隐藏/移窗/重连及原终端回归通过。五平台原生源码、独立包和实际包 SSH/PTY/文件组合通过；Linux x86_64 实际 bash 目录报告通过。另从草稿下载 Windows 包，验证 WebView2 拖动、关闭取消与最终释放。

包来源：[`dfdb826b0cedbe07323450692acc21bdc657bfca`](https://github.com/BgaSol/soDeck/commit/dfdb826b0cedbe07323450692acc21bdc657bfca)；原始打包：[Package plugin #42506418843](https://github.com/BgaSol/soDeck/actions/runs/42506418843)；源码：[Check #42506353705](https://github.com/BgaSol/soDeck/actions/runs/42506353705)；实际包：[Verify native remote plugins #42507409785](https://github.com/BgaSol/soDeck/actions/runs/42507409785)；发行：[terminal 0.3.0](https://github.com/BgaSol/soDeck/releases/tag/plugin-terminal-v0.3.0)，非 Latest。DPI/多显示器、macOS/zsh 实际目录报告、netem 与历史升级未重复。

# 0.2.3

终端在连续调整面板宽度时按当前容器逐帧适配，远程尺寸通知保留一个在途请求和一个可替换等待位置，持续拖动不会积压旧尺寸任务或阻挡本地绘制。隐藏、断线、退出和卸载会清理尺寸任务，恢复后重新测量；保留原终端输出、PTY 和输入顺序。

修正 FitAddon 测量容器的盒模型和终端外框高度，消除横纵裁切及底部黑条。建议同时升级核心 0.12.3，以取得面板拖动预览、保存确认与取消的完整整改。插件 UI v1 与现有设置格式保持兼容，没有新增生产依赖。

本机真实 xterm、阻塞尺寸请求、输入、外观、隐藏与卸载回归，以及隔离 OpenSSH 和实际 Windows x86_64 包内 WebView2 流程通过。Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 的原生源码、独立包构建，以及本次核心 0.12.3 + 终端 0.2.3 + 文件 0.4.2 + 网络隧道 0.2.2 的实际包远程业务验收全部通过。另从本次草稿下载 Windows 核心与终端包，摘要核对、拖动、完整行高度、取消关闭和最终退出复测通过。

包来源：[`d7095e910d298989431913f2a2c1d80df6b552c1`](https://github.com/BgaSol/soDeck/commit/d7095e910d298989431913f2a2c1d80df6b552c1)；原始独立打包：[Package plugin #42492560868](https://github.com/BgaSol/soDeck/actions/runs/42492560868)；源码检查：[Check #42492522325](https://github.com/BgaSol/soDeck/actions/runs/42492522325)；五平台实际包：[Verify native remote plugins #42493134683](https://github.com/BgaSol/soDeck/actions/runs/42493134683)；正式发行：[terminal 0.2.3](https://github.com/BgaSol/soDeck/releases/tag/plugin-terminal-v0.2.3)，不设为仓库 Latest。

发布核对五个原生归档、插件更新清单与 SHA-256，沿用原打包附件。实际 DPI/跨显示器和长时间拖动帧率、用户远端大型交互程序、netem 与历史版本对的在线升级未执行，不计为通过。核心发布和完整验收范围见[核心 0.12.3](../../docs/releases/v0.12.3.md)。

GitHub 正式发布时补建插件标签，自动触发的[额外打包 #42494474875](https://github.com/BgaSol/soDeck/actions/runs/42494474875) 被正式版本不可覆盖检查阻止上传，不计为发行来源；正式六个附件的 ID、大小与摘要仍与原始草稿一致。

# 0.2.2

加深多巴胺主题的辅助文字，保持彩色背景上的阅读对比度。

终端退出后可新建一个独立 shell，原页保留输出；重复点击只创建一次。连接断开时给出工作台重连指引，新建动作等待连接恢复。连接状态提示由核心 0.9.3 的可选 UI 增量提供；旧核心仍可使用终端，无法提供完整断线状态。

# 0.2.1

终端菜单与输入框文本菜单在缩窗时自动避让，长内容只在菜单内部滚动。键盘菜单跟随终端，右键菜单在来源滚动时关闭；保留复制粘贴、选区和鼠标报告行为。菜单资源随插件独立打包，使用时无需联网加载。

# 0.2.0

新增独立的终端外观设置，支持三款离线预置字体及本机字体、字号、行高、底部留白和光标。默认 14px、1.2 倍行高、24px 底部留白；设置在同类终端间共享，应用外观保留 PTY。滚动条跟随核心三主题，保持终端输入、复制粘贴、业务右键和鼠标跟踪行为。组件设置需要核心 0.9.0，请先升级核心，再升级插件并重启。字体及许可证随插件独立打包，使用时无需联网下载。

# 0.1.2

统一工作台主题、页签、菜单和状态栏样式。新增原生剪贴板复制/粘贴与平台快捷键；有选区时复制，无选区时 Ctrl+C 继续中断。防止重复粘贴和异步结果输入到已切换的终端。剪贴板功能需要核心 0.7.0，请先升级核心再升级插件并重启。

# 0.1.1

终端改为紧凑内容区域和底部状态栏，跟随核心的状态区域配色及浅色/深色方案。

# 0.1.0

真实远程终端，通过核心复用工作台连接。支持独立 PTY、多终端、输入/输出、尺寸、退出状态和跨窗显示快照。需要核心的 terminal.v1 与 viewData.v1 能力。

# 终端

终端插件复用当前服务器会话，通过 PTY 打开登录 shell 或创建参数指定的交互命令。中央“＋”触发插件声明的新建行为；侧边工具栏不提供另一套终端定位入口。

## 创建、输入与结束

- 每个新建请求创建独立 PTY；同一请求重试复用原回执。登录 shell 和指定命令是不同创建方式，标题与目录集成策略随创建参数保存。
- Windows 有选区时 Ctrl+C 复制，无选区时仍发送中断；Linux 使用 Ctrl+Shift+C/V，macOS 使用 Cmd+C/V。鼠标报告启用时普通右键交给远端程序，Shift+右键可打开本地菜单。
- 剪贴板与输入通过既有桥接，保留括号粘贴和 IME 规则。失焦、移窗、冻结或连接代次改变后拒绝迟到粘贴，不把文本送到另一个终端。
- 同窗切换、隐藏和移窗沿用原 PTY；目标页面恢复显示后才能输入。字体或面板尺寸改变会重新适配有效尺寸，不重新执行命令。
- 登录 shell 退出后保留输出并提供“新建终端”；指定命令提供“重新打开终端”，保留原创建配置并新建 PTY。断线后先在工作台手动重连。
- 关闭终端释放对应资源；关闭应用后不保留可检索的终端档案。关闭共同规则见[生命周期](lifecycle.md)。

## 目录识别与文件跟随

登录 shell 的目录集成可选自动、手动或关闭。自动模式支持 Bash/Zsh，按用户配置顺序接入 OSC 7 上报，使用本次终端的临时脚本，不持久修改远端 shell 配置；无法自动集成时保留终端并说明限制。指定命令第一版关闭目录脚本注入。

只有有效的主机/路径及当前会话身份才能更新目录。主机身份未知、路径无效、旧代次或命令终端不能提供可信目录时，不猜测路径。手动集成可使用符合协议的 OSC 7；不同 shell/远端系统需各自验证。

终端只发布自身位置，共享状态经核心送达。[文件插件](files.md)决定是否跟随，以及手动浏览、文件系统身份和断线时怎样处理；两插件没有直接调用关系。

## 历史、搜索与彩色预览

保留 100,000 行滚动历史，当前屏幕另计。Windows/Linux 使用 Ctrl+Shift+F，macOS 使用 Cmd+F，也可从终端菜单打开查找。支持普通子串、大小写开关、命中计数、循环前后定位、Unicode 与软换行；不提供正则表达式或远端 shell history 查询。

搜索高亮与正文选区分开，退出或断线页仍可搜索。查询最多 1024 个 UTF-16 单元，仅检索当前终端实际保留的文字；行淘汰或终端重排后丢失的文字不能继续计为命中。历史更新和查询由独立 Worker 处理，过期版本的结果丢弃，隐藏页停止绘制。

常驻彩色预览支持点击、拖动和键盘浏览；正常宽度为 96 px，窄页缩为 40 px。当前源码的阅读框使用透明底、细轮廓和窄位置标记，搜索控件保持居中；这组显示调整尚未发行，见[终端变更记录](../../plugins/terminal/CHANGELOG.md)。

## 恢复与维护约束

核心保存原始输出临时日志、分片显示检查点和小 UI 状态；插件解释文字、颜色和搜索结果。正常输出 ACK 不等待全量快照，移窗恢复查询、有效命中、选择、阅读位置与底部跟随。

- 每流日志和已提交检查点各限 128 MiB，全部流的日志、在途及已提交快照合用 1 GiB 配额，不能让所有终端同时用满各自上限。
- 快照按固定输出水位分片生成，后续输出继续运行；未完成上传不替换旧检查点。旧日志只有被检查点覆盖且无读者需要后才能清理。
- 写盘或额度失败明确报告恢复状态，实时输出可以继续；真正的恢复缺口不得用重新运行命令弥补。
- UI 元数据最多 8 KiB，与输出水位分离。关闭释放临时资源，启动只清理可确定属于已退出进程的恢复目录。
- 当前固定 xterm 6.0.0 与 SerializeAddon 0.14.0，对环形行观察和冻结快照使用受控内部适配；升级必须运行历史回归。

完整历史恢复需要核心 0.22.1 的 `terminal.history.v1`；当前终端遇到缺少能力的核心会提示升级。核心 API 主版本保持 1，创建参数和恢复接口见[插件 API](../plugin-api.md)。字体配置见[设置](settings.md)。

## 实现与回归入口

- 插件：[创建参数](../../plugins/terminal/src/creation.rs)、[目录集成](../../plugins/terminal/src/integration.rs)、[输入](../../plugins/terminal/ui-src/input.js)、[历史](../../plugins/terminal/ui-src/history.js)、[Worker](../../plugins/terminal/ui-src/history-worker.js)、[搜索](../../plugins/terminal/ui-src/search.js)、[快照](../../plugins/terminal/ui-src/snapshot.js)。
- 核心：[terminal.rs](../../src-tauri/src/terminal.rs)、[streams.rs](../../src-tauri/src/streams.rs)、[journal.rs](../../src-tauri/src/streams/journal.rs)。
- 回归：[创建](../../plugins/terminal/tests/creation.rs)、[输入](../../scripts/test-terminal-input.mjs)、[历史](../../scripts/test-terminal-history.mjs)、[搜索 UI](../../scripts/test-terminal-search-ui.mjs)、[真实 shell](../../scripts/test-terminal-shell.py)、[真实 PTY](../../scripts/test-terminal-ui.mjs)。
- 五平台基本实际包与深度搜索性能是不同验证范围。超宽行、多终端、完整进程内存、原生 Worker/IME/DPI 专项及未补齐交互由临时计划跟进，当前不承诺全部性能目标通过；发行证据见插件 CHANGELOG。

返回[文档索引](../README.md)。

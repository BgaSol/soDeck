# 终端目录自动识别实施记录

2026-11-09。对应[方案](plan-terminal-directory.md)。核心 0.14.0、终端 0.4.0、文件 0.6.0；发行来源和最终验收统一记录在 [v0.14.0](releases/v0.14.0.md)。

## 根因与改动

原终端只解析手工 OSC 7，主机名为空便拒绝；文件开关默认开启却没有说明未收到目录，形成“连上后没有效果”。原解析及无报告导航用例可稳定复现，新自动路径不再依赖 hostname。

终端插件在创建时读取设置，使用有界 session.exec 识别远端系统和默认 shell，再通过可选 terminal.launch.v1 在同一 PTY 启动集成。探测失败回退普通登录 shell。启动文件只写入用户私有的随机临时目录，父启动进程在正常退出或 HUP/TERM 时清理；不修改用户配置。终端页面等待宿主挂载确认后再发送尺寸、显示快照和用户输入，旧核心沿用原就绪路径。每个 view 保留同一个 PTY 和集成标识，重挂载不重复启动。

Bash 使用 POSIX 的 ENV 启动入口，进入后立即关闭 POSIX 模式、恢复原 ENV，并按登录 shell 顺序加载 /etc/profile 和首个用户 profile；保留 login_shell 标志。用户配置完成后在 PROMPT_COMMAND 末尾链入报告，保留字符串或数组钩子以及退出状态。已有 DEBUG trap、functrace/extdebug 或 Bash 4 之前版本不接入增强，保留原行为。Zsh 使用本次启动专属 ZDOTDIR 文件转交真实 .zshenv/.zprofile/.zshrc/.zlogin，恢复真实 ZDOTDIR，并追加 precmd/preexec 钩子；禁用 RCS 或全局配置重写启动入口时可能无握手，此时明确降级。

报告通过有界 OSC 633 载荷携带本次标识、真实物理目录及文件系统标记。只有受控 shell 的 PID/子 shell 状态可以报告；自动模式忽略 OSC 7，不混用来源。忙碌为 unknown，文件侧栏保留已处理路径，避免普通命令回到相同目录时覆盖手动浏览。输入提交也使旧就绪事实失效，钩子被移除后不会继续冒充已就绪。

自动报告使用 shell-reported 文件系统视角；核心文件接口要求通过当前 SFTP 服务读到相同临时标记，再检查路径。旧文件插件不接受新视角；旧核心缺少能力时新版插件给出升级说明。普通 session-default 和高级手动 OSC 7 保留原语义。

文件树底部用原生可展开状态展示跟随结果、原因、shell、确认时间和一次性重试。仅当前活动终端触发导航，非终端、隐藏、断线和退出保留位置；取消和异步提交继续使用已有目录树导航序号。短命令不闪切忙碌文案，正常流程不发布消息中心警告。

## 验证入口与边界

- `scripts/test-terminal-shell.py` 直接使用发行脚本，真实 PTY 覆盖 Bash 字符串/数组提示符钩子和 Zsh、配置顺序、退出状态、登录标志、特殊路径/软链接、嵌套 shell、Ctrl-C 和正常退出清理。
- `scripts/component-state.test.mjs` 覆盖报告绑定/编码/上限、旧核心、忙碌后同目录不回跳和显式重试。
- `communication:test` 的 Linux 场景不配置 hostname 或注入 shell 脚本，验证实际三插件 RPC/PTY/SFTP 首次目录跟随、手动浏览、多终端、隐藏、移窗、重连；Windows 验证高级 PowerShell 兼容模式。
- 五平台原生包仍通过原工作流。Linux 实际包另外检查自动报告及 SFTP 对应标记，错误标记必须被拒绝。

远端自动支持范围为 Linux Bash 4+ 和 Zsh。其他系统/shell 保留普通终端；自动跨容器、跨主机、SFTP chroot、不同文件系统映射不支持。现有 DEBUG 调试钩子不做复杂链式改写。SIGKILL、远端掉电等强制中断可能留下当前用户私有的临时目录，未增加扫描历史目录的后台清理器。真实用户所有 shell 配置组合、设备 DPI/多屏与历史版本对在线升级不由本轮 PTY/CI 结果代表。

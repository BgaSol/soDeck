# 终端插件

复用核心的真实 SSH 会话，通过 PTY 打开登录 shell。中央多 Tab，跨窗恢复 xterm 显示快照和后续输出，不重新开启 shell。失联不重放命令，新建终端才创建新 shell。

shell 退出后，原页保留输出并提供“新建终端”；连接断开时先从工作台重新连接。此操作创建新的 PTY，不恢复原 shell 或重放命令。

`node scripts/package-plugin.mjs --plugin terminal` 独立构建原生后端及本地 xterm 资源；安装包无需 Node，不访问 CDN。设置中添加产物目录并重启。终端 0.5.0 需要核心 0.15.0 声明 `terminal.session.v1` / `viewData.v1` / `sharedState.v1`；缺少时明确提示升级。`npm run terminal:test` 使用隔离 OpenSSH 和实际插件包验证。

版本在 Cargo.toml；独立标签 `plugin-terminal-v<版本>`。宿主机会话策略在 `src/session.rs`，Shell/目录集成留在本插件；共同显示、输入、剪贴板和外观源码在 `../shared/terminal`，构建时随本插件独立复制。UI 接入在 ui-src，生成的 ui 不提交；依赖版本及许可随包携带。职责与扩展接口见[抽象设计](../../docs/plan-terminal-abstraction.md)。

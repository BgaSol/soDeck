# 终端插件

复用核心的真实 SSH 会话，通过 PTY 打开登录 shell。中央多 Tab，跨窗恢复 xterm 显示快照和后续输出，不重新开启 shell。失联不重放命令，新建终端才创建新 shell。

`node scripts/package-plugin.mjs --plugin terminal` 独立构建原生后端及本地 xterm 资源；安装包无需 Node，不访问 CDN。设置中添加产物目录并重启。需要核心声明 `terminal.v1` / `viewData.v1`，v0.5.2 不具备这些能力。`npm run terminal:test` 使用隔离 OpenSSH 和实际插件包验证。

版本在 Cargo.toml；独立标签 `plugin-terminal-v0.1.0`。UI 源码在 ui-src，生成的 ui/vendor 不提交；依赖版本及许可随包携带。

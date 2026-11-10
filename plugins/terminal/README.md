# 终端插件

复用核心的真实会话，通过 PTY 打开登录 shell 或创建参数指定的交互命令。中央多 Tab，跨窗恢复 xterm 显示快照和后续输出，不重新启动命令。声明默认“新建终端”行为，收到事件后由插件申请自己的页面。

登录 shell 退出后，原页保留输出并提供“新建终端”；指定命令终端提供“重新打开终端”，保留 launch/title/directoryIntegration 配置并创建新 PTY。连接断开时先从工作台重新连接。指定命令首版关闭目录集成，不探测主机 shell 或注入目录脚本。参数及兼容策略见[插件 API](../../docs/plugin-api.md#终端创建参数)。

当前源码支持 10 万行保留历史、浮动搜索条和常驻彩色预览，尚未发布。操作见[使用说明](../../docs/usage.md#终端历史搜索源码已实现尚未发布)，实现与平台验证范围见[实施记录](../../docs/implementation-terminal-search.md)。

`node scripts/package-plugin.mjs --plugin terminal` 独立构建原生后端及本地 xterm/Worker 资源；安装包无需 Node，不访问 CDN。设置中添加产物目录并重启。当前源码需要核心声明 `terminal.v1` / `viewData.v1` / `terminal.history.v1`，缺少能力时提示升级。`npm run terminal:test` 使用隔离 OpenSSH 和实际插件包验证，历史专项入口见[开发指南](../../docs/development.md)。

版本在 Cargo.toml；独立标签 `plugin-terminal-v0.1.0`。UI 源码在 ui-src，生成的 ui/vendor 不提交；依赖版本及许可随包携带。

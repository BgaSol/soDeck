# 组件交互示例

这是独立的 UI 验证插件，不连接服务器，不执行命令，不写远程文件，不创建转发。它提供文件列表、中央编辑草稿、多终端输入草稿和默认隐藏的转发面板，核心不包含它。

```sh
npm run plugin:build -- --plugin ui-example
```

产物包含目标系统原生进程、plugin.json 和完整 ui/ 资源，可通过设置中的插件管理添加，重启后加载。当前核心已有 SSH 连接、PTY 与 SFTP 原语，真实业务在 terminal/files 插件中；本插件仍只验证交互，不执行远程业务，不增加正式模拟连接按钮。

```sh
# 实际 Rust 核心 + 无头浏览器
npm run ui:test
# 已安装核心的原生窗口验证
# 将 SODECK_TEST_UI_PLUGIN 设为上面产物目录，再运行
node scripts/smoke.mjs /path/to/installed/sodeck
```

viewId 对应插件进程内逻辑状态，ui.mount 幂等返回草稿、光标与滚动位置。收起和同窗切换保留 iframe；跨窗重挂载读取同一逻辑状态。输入 `SODECK_TEST_SAVE_FAILURE` 可复现保存失败并验证草稿保留。示例草稿只在当前进程有效，不承诺崩溃恢复。

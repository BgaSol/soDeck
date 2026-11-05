# 文件插件

侧栏浏览真实 SFTP 目录，中央按规范路径去重打开文本编辑页，支持 1 MiB 内 UTF-8（可含 BOM）、LF/CRLF，以及单文件流式下载。二进制、非法 UTF-8 和混合换行只允许下载，避免无损性问题。

保存校验远端内容摘要、属性和链接目标，先写同目录临时文件，服务器支持 OpenSSH 原子替换才提交。外部修改拒绝覆盖；提交时失联提示结果未知。草稿和终端不随隐藏、断线或移窗而重建；进程崩溃后不承诺草稿恢复。

`node scripts/package-plugin.mjs --plugin files` 独立打包，设置中添加产物目录后重启。需要核心 `files.v1` / `viewData.v1`；v0.5.2 不支持。`npm run files:test` 使用隔离 OpenSSH 与实际插件 UI 验证。版本在 Cargo.toml，标签 `plugin-files-v0.1.0`。

# 已保存服务器密码再次复制：实施与验证记录

日期：2026-10-21。状态：已完成。

本次实现为已有服务器编辑页增加“复制已保存密码”。密码仍不会回显到输入框，也不会通过前端响应、诊断日志或 IPC 返回值传递。前端只提交服务器 ID；Rust 在主窗口中按 ID 读取当前保存的密码，并通过 `write_secret` 写入系统剪贴板。

## 已实现内容

- `src-tauri/src/db.rs` 增加按 ID 的单记录密码读取和数据库测试，保留密码原始字符，不更新配置时间。
- `src-tauri/src/lib.rs` 注册 `copy_saved_server_password`，执行主窗口校验、运行状态检查和敏感剪贴板写入；剪贴板错误映射为固定提示。
- `src/api.ts` 增加只传服务器 ID 的 `Promise<void>` 调用；诊断命令白名单同步更新。
- `src/ServerEditor.tsx` 增加就近复制按钮、复制中/成功/失败状态、连点守卫和过期请求保护。替换或清空尚未保存时，复制的仍是数据库中的旧密码。
- `src/server-editor.css` 增加窄窗口可换行的局部布局和预留反馈空间，沿用现有主题与控件样式。
- `tests/connection-ui.spec.ts`、`src/diagnostics.test.ts` 增加界面、异步边界、权限和敏感数据诊断覆盖。
- `tests/server-password-copy-native.mjs` 提供隔离 Windows 原生验收脚本，不接触生产服务器数据。

## 自动化验证

- `npm run build`：通过。
- `npm test`：29 个文件、186 个测试全部通过。
- `cargo test --offline --manifest-path src-tauri/Cargo.toml db::tests`：16 个通过。
- `npm run test:connection`：150 个通过，覆盖 Chromium/WebKit 的连接、窗口和文件编辑回归。
- 密码复制相关 Chromium/WebKit 用例：40 个通过。
- 新增高对比度用例：Chromium 通过；WebKit 因不支持该模拟而跳过。截图已人工检查。
- 默认应用标识的 `cargo build --offline --manifest-path src-tauri/Cargo.toml --bin sodeck`：通过。
- `git diff --check`：通过。

## Windows 原生验收

使用单独的 Tauri 标识、Vite 端口 `1479`、WebView2 CDP 端口 `9249` 和隔离用户数据目录运行真实应用。测试凭据只写入该隔离配置，完成后删除测试服务器。

复现时，先在一个终端启动独立的前端服务：

```powershell
npm run dev -- --port 1479 --strictPort
```

在另一个终端构建并启动隔离应用，然后执行初始检查：

```powershell
$env:TAURI_CONFIG='{"identifier":"com.sshworkspace.passwordcopyqa","build":{"devUrl":"http://127.0.0.1:1479"}}'
cargo build --offline --manifest-path src-tauri/Cargo.toml --bin sodeck
Remove-Item Env:TAURI_CONFIG
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS='--remote-debugging-port=9249'
$env:WEBVIEW2_USER_DATA_FOLDER=Join-Path $PWD '.qa/password-copy-webview'
$qaApp=Start-Process -FilePath (Join-Path $PWD 'src-tauri/target/debug/sodeck.exe') -WindowStyle Hidden -PassThru
node tests/server-password-copy-native.mjs
```

退出该隔离进程，使用同一命令重新启动应用，再执行重启检查：

```powershell
$env:SODECK_PASSWORD_COPY_PHASE='restart'
node tests/server-password-copy-native.mjs
Remove-Item Env:SODECK_PASSWORD_COPY_PHASE
```

两个阶段完成后退出隔离应用、停止独立前端服务，并恢复默认调试构建：

```powershell
Remove-Item Env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS
Remove-Item Env:WEBVIEW2_USER_DATA_FOLDER
cargo build --offline --manifest-path src-tauri/Cargo.toml --bin sodeck
```

初始阶段通过 5 项检查：重复复制保持原文且密码输入框为空、Windows 敏感剪贴板标记存在、未保存替换复制旧值且保存后复制新值、真实剪贴板占用时失败并可重试、独立工具窗口被拒绝且剪贴板不改变。重启阶段通过 3 项检查：应用进程重启后仍可复制、敏感标记存在、待清空仍复制旧值而保存清空或删除后不再写入。

结果保存在 `.qa/server-password-copy-native-initial.json` 和 `.qa/server-password-copy-native-restart.json`；界面截图保存在 `.qa/server-password-copy-light.png`、`.qa/server-password-copy-dark.png`、`.qa/server-password-copy-narrow.png`、`.qa/server-password-copy-forced-colors.png` 和 `.qa/server-password-copy-native.png`。这些文件被 `.gitignore` 忽略。

原生验收覆盖了真实 Windows 剪贴板、进程重启、主窗口权限和 WebView2 页面反馈；没有在本机验证 macOS/Linux、物理 125%/150% 缩放，也不能保证第三方剪贴板管理器都遵守敏感内容标记。

## 收尾

原生验收使用的调试构建仅用于隔离测试，完成后已停止测试应用和 Vite 服务，并重新生成默认应用标识的 `src-tauri/target/debug/sodeck.exe`。本次未构建安装包、未提交 Git commit，也未发布版本。

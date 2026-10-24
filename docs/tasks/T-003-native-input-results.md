# T-003 原生控制键、焦点和重命名补查

日期：2026-10-25。沿用已确认 TERM-02/04 与前端重命名契约，先固定本流程，再编写独立测试支撑；不修改产品实现，不以合成键盘事件代替 Windows 输入。

## 本轮步骤与预期

1. 启动独立 loopback SSH 回送器和当前已构建的测试 App，使用临时数据目录和独立 CDP 端口。建立同服务器两个真实 PTY，左右分屏并显示实际插件会话列表。服务端只记录本测试通道收到的字节，不执行 shell 命令。
2. 支撑脚本通过 CDP 观察 DOM trusted key/input/composition 事件、工作台/worker 名称和资源 ID；不发送 keyboard、insertText、dispatchEvent 或 Windows 输入。所有真实输入由主任务的 Computer Use `@oai/sky` 完成，按其规范重新观察目标窗口和焦点后操作。
3. 在 A 终端实际点击聚焦，先发送只含 ASCII 的唯一标记，再发送 Enter、Tab、方向键、Ctrl+C。每批前建立观测基线；服务端精确核对 A 收到预期字节且仅一次、B 无新增字节。方向键采用实际 xterm 默认光标模式对应的 ANSI 序列，Ctrl+C 必须为 `03`，不能触发复制或发送字符 c。
4. 实际点击 B，发送另一个标记及控制键；核对只有 B 的通道收到输入。服务端字节、真实 DOM 事件和两条资源身份一起构成证据，截图只作界面辅助。
5. 真实点击会话列表的重命名按钮，在原生输入框编辑 ASCII 测试名称并按 Enter；核对插件列表、worker 会话、工作台 Tab 同步且 sessionId/viewId/ptyId/generation 未改变。再编辑不同名称并按 Esc，核对名称不变且未提交重命名调用；随后回到原终端继续输入。
6. 复制粘贴仅在能完整恢复原剪贴板所有格式时执行。标准浏览器 Clipboard API 不能证明枚举 Windows 全部自定义格式，因此本轮不读取用户剪贴板正文、不覆盖任何格式。选区 Ctrl+Shift+C/V 与工具栏复制/粘贴暂保留未执行；不以合成 clipboard provider 或直接 PTY 写入代替实际剪贴板通过。
7. 保存各批实际输入事件、服务端字节、资源/名称快照与自有测试页面截图；正常退出 App，核对两个通道与连接关闭，结束回送器。测试主机密钥只存在内存，没有测试私钥落盘。

## 支撑协议

`tests/desktop/input-observe.mjs` 输出独立结果目录及 `ready.json`，在该目录监听 `command.json`。每条命令包含唯一 `id` 和 `action`，结果写入 `response-<id>.json`。`mark` 建立字节/事件基线，`observe` 保存当前观察，`verify` 按 A/B 的期望十六进制字节严格核对且保持资源 ID，`activate` 仅通过产品 API 选择 A/B 终端，`finish` 结束并清理；原生按键始终由主任务执行。默认等待不超过 20 分钟，异常会保存失败原因并清理自有进程。

## 实际结果

首轮输入批次未通过，证据 `.qa/input-sky/1792873574831/response-a-check.json` 与 `observation-a-debug.json`：A 收到 `16 61 0d`，Tab 和部分方向键未到 A；随后 Right 与 Ctrl+C 到了 B（`1b5b43 03`）。`sky.type_text` 在此环境实际发出 Ctrl+V，产生 `16`，该工具行为不作为产品普通字符输入失败；后续改用逐键 `press_key`。本轮没有通过 API 读取用户剪贴板内容，也没有执行剪贴板功能验收；不能根据观察脚本未访问剪贴板，推断输入工具内部从未临时使用剪贴板。

此批次前台状态不确定：一次 Computer Use 窗口观察返回了 Codex 前台截图，而窗口标题仍是 SoDeck。没有足够证据将失败归因产品。重新获取并激活 SoDeck、实际点击 A 后，孤立 Tab 的 trusted keydown/keyup 均到 A 的 textarea，服务端 channel 0 收到 `09`。CDP 下所有 WebView 的 `document.hasFocus()` 同时为 true，因此不采用该字段判断原生焦点。本轮未因此修改产品代码，保留原失败证据。

保持目标前台并重新建立基线后：`response-focus-a-check.json` 精确核对 A 为 `61 09 0d 03`（a、Tab、Enter、Ctrl+C），B 无新增；`response-focus-b-check.json` 精确核对 B 为 `1b5b41 1b5b42 1b5b44 1b5b43`（上、下、左、右），A 无新增。两批目标 textarea 的 keydown/keyup 均为 trusted，所有远端资源 ID 保持不变。

首次 Enter 重命名为 `n`、编辑 `x` 后 Esc 的实际名称状态核对通过：worker 会话、工作台 Tab 和插件列表同步，取消后仍为 `n`，两个 PTY 均无新增字节。旧调用计数观察器因 Wry 的 `window.ipc.postMessage` 不可写而未安装成功，所以旧批次的 `renames: []` 不证明零调用。观察器随后改为透传底层可写 `chrome.webview.postMessage`，并检查安装成功后才允许断言调用次数；该修正仅涉及测试支撑。

本轮最终保留上述 4 组实际输入与表单状态断言，没有设置 `renameCount`，不宣称重命名精确请求次数。`result.json` 为 `passed: true`、`normalExit: 0`；回送器收到两个 `shell-close` 和两个 `connection-close`（主机密钥首次挑战连接及实际会话连接），App 进程已结束，支撑脚本退出 0。范围仅覆盖 Windows 原生输入到真实 loopback SSH PTY 的字节回送，不代表 Linux shell、剪贴板功能或额外显示器环境通过。

证据目录：`.qa/input-sky/1792873574831/`。本次执行产物 SHA-256：桌面 `cc240635acc5f20a1b82893eddcde722bfbd7ea43f8370b2294d68ec51f35d7c`；终端包 `591ab37668e4138c93fbf34e821ef0586ceda4f66d5625d76358aa503d8331dd`。最初 `.qa/input-sky/1792873531728/` 因测试回送器未持有 channel 引用而在准备阶段失败，已修正为持有引用后重启；该失败不计产品结果。

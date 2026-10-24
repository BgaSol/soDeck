# T-003 Windows 中文输入法专项验证

日期：2026-10-25。该项补查真实 Microsoft Pinyin 组合输入，不用浏览器合成事件或直接 Unicode 注字代替。测试开始前先固定本流程，再增加独立测试脚本；不修改产品源码或重新构建。

## 测试前提与计划

只读环境检查已确认用户语言为 zh-Hans-CN，Microsoft Pinyin profile 已注册并由用户启用，已加载 HKL 为 `0x08040804`。这些只能证明环境可用，不能证明当前中文模式或实际组合输入。

1. 启动独立 loopback SSH 字节回送器与当前已构建测试 App，使用独立数据目录、调试端口 9393/9394。建立真实 SSH PTY，聚焦实际 xterm 输入框。
2. 原生 helper 先验证 PID 的执行文件属于当前工作区 `target/debug/sodeck-desktop.exe`，再仅选择此进程主窗口。所有输入批次发送前重新核对前台 PID；若焦点转到其他程序，立即停止，不能把按键发到用户程序。
3. 仅向自有聚焦窗口请求 `WM_INPUTLANGCHANGEREQUEST`；读取并记录该窗口原始 HKL/IME 模式。用真实 `SendInput` 字母虚拟键输入 `zhongwen`，不使用 `KEYEVENTF_UNICODE`。记录真实 `compositionstart/update/end`、input 事件及最终终端字节回显；Space 提交后必须出现实际汉字且只有一次。
4. 若仅收到英文，允许在自有聚焦窗口内按一次 Shift 切换模式，再用不同标记 `ceshi` 重试；次数有界。不能凭切换成功返回值判定 IME 通过。
5. 在实际 composition 尚未结束时请求视图跨窗口移动，期望 `MOVE_CANCELLED` 且保持原位置及 PTY。提交完成后继续输入，确认没有冻结、重放或断开。
6. 尽可能保存自有窗口和终端截图。候选窗口位置若截图不能证明，明确保留未验证；不推断多显示器、不同 DPI 或完整可访问性通过。
7. 若改变自有窗口 IME 模式，关闭前恢复；正常退出 App 并结束回送器，删除临时私钥。保存实际结果、错误和诊断，不把未执行场景写成通过。

## 结果

2026-10-25 原生补查通过。证据 `.qa/ime-sky/1792871578155/result.json`、`observation.json`、`before.json`、`after.json`、`response-move-composing.json`、`response-snapshot.json` 与 `terminal.png`。使用当前 `2.0.0-dev.1` debug 程序、Computer Use 原生按键、Microsoft Pinyin、隔离 loopback SSH 回送器；没有注入 composition 事件或 Unicode 汉字。

- 实际逐字按下 `zhongwen`，观察到 trusted compositionstart/update/input。候选框在当前单显示环境中显示于输入光标下方，首选“中文”。
- 组合未提交时请求弹出返回 `MOVE_CANCELLED`，工作台仍为 center，未建立新 PTY。
- Space 后实际 SSH 回显为 `z中文`（前缀 z 是切换模式前的 ASCII 探测），汉字只出现一次。后续 `a` 与 Enter 继续回显为 `z中文a`；前后 PTY ID 完全相同。
- 此 WebView2 环境的 compositionend 标记为 isTrusted=false，而对应的 compositionupdate/input 为 true。旧断言因此误报；现改为同时要求 trusted 原生组合/汉字输入事件、结束数据一致、真实 PTY 回显一次，保持原输入正确性预期。
- 本次对该窗口按两次 Shift，最终回到起始模式；正常退出码 0，停止回送器并删除其 6 个临时私钥。

该结果只覆盖 Windows 本机 Microsoft Pinyin 与 loopback SSH。单显示候选位置已观察；不同显示器/DPI、完整辅助技术与 Linux/OpenSSH 仍未验证，不代表用户验收。旧失败 `.qa/ime/1792871110159/result.json` 保留，不能视为产品汉字丢失的证据。

### 断线后的补查流程

重新执行时使用 Computer Use 的原生按键与窗口观察；测试辅助进程仅建立隔离 SSH、记录 DOM 事件、读取回显和请求移窗，不注入组合事件。分别保存真实 compositionstart/update、input 与 compositionend 的 isTrusted，不因某一结束事件标记就推断输入成功或失败；提交是否成立同时核对汉字输入事件、PTY 实际回显仅一次、后续输入及同一 PTY。候选位置通过实际窗口截图判断。旧失败记录保留。

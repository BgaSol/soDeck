# 服务器工作区与终端二级标签：实现与验证记录

日期：2026-10-22。实现分支：`codex/server-terminal-tabs`。

## 实现结果

- 同一保存配置 `server.id` 在整个应用实例中只登记一个服务器工作区，覆盖主窗口和独立 SSH 窗口。重复进入会定位已有窗口；新建终端在已有工作区内执行。
- 服务器页内增加终端标签：新建、切换、关闭、双击/F2 重命名、拖动或 Alt+左右键排序、键盘选择、溢出列表。关闭活动终端优先选择最近使用的存活终端，默认编号只递增。
- 每个终端独立拥有 SSH/PTY、连接 attempt、认证、输出、重连和历史。切换标签保留后台实例，输入及剪贴板仍绑定具体连接。
- 文件、监控、Docker、NetBird 共享服务器工具上下文，关闭最后一个终端后仍保留；显式验证工具不创建隐藏终端。独立编辑器、日志、容器窗口及已交付的后台任务延续原生命周期。
- 文件浏览位置与活动终端目录解耦。目录事件更新标记，点击“定位到当前终端目录”才导航；目标身份不匹配时禁止定位。
- 配置变化会显示工具仍使用原连接信息。新终端读取新配置，工具需显式更新；验证失败保留旧上下文，旧请求不能写入新面板。
- 拆窗/移回以整个服务器工作区为单位，支持连接中断、等待重连和零终端状态。迁移中阻止交互，所有终端接收完成才原子提交。
- 关闭服务器先展示终端及工具影响，提交时重新检查版本、预检草稿和操作租约。零终端工作区仍阻止删除正在使用的服务器或跳板配置。

## 代码与协议

| 范围 | 主要实现 |
| --- | --- |
| 权威工作区登记 | `src-tauri/src/workspaces/{mod,model,commands,contexts,lifecycle}.rs`；全局唯一索引、幂等请求、窗口 generation、工作区 ownerEpoch、操作租约 |
| 双层界面 | `ServerWorkspace.tsx` 组织共享工具；`TerminalSession.tsx` 保存独立终端状态；`TerminalTabs.tsx` 提供二级导航；`SessionWorkspace.tsx` 保留导出兼容 |
| 连接协调 | `workspaceConnectionQueue.ts` 按工作区串行建立连接及认证；已建立终端独立运行；后台认证从实际展示时起计算交互超时 |
| 工具授权 | 前后端统一使用 `ToolContextRef { id, epoch }`，不接受终端 ID 替代；实际凭据仅保留在 Rust 内存及原有受控存储 |
| 迁移 | `moves/transfer.rs` 与 `useWorkspaceMoves.ts`；V2 快照、精确成员集合、24 MiB 总上限、全部输出消费屏障、全部流预检与提交、25 秒回滚看门狗 |
| 迟到结果保护 | 校验 transferId、owner、generation、attempt 和 context 版本；重复到达/提交幂等；提交响应丢失时查询权威状态；旧失败通知不能取消新事务 |
| 历史兼容 | 保留原数据库字段 `workspace_id`，其中继续保存稳定终端组 ID，即新 `terminalId`；服务器工作区 ID 不进入此历史分组字段 |
| 生命周期接入 | 配置删除、数据库重建、窗口刷新/销毁、退出和更新影响审查均纳入工作区及工具验证；独立任务接管前使用工作区操作租约 |

`terminal_input`、resize、清屏和认证回复继续校验真实终端连接。工具命令改用 context；独立窗口创建后使用各自授权。此调整不改变 SQLite schema，也不提供跨应用重启的在线终端恢复。

## 自动验证结果

以下分别记录单测、IPC 仿真与真实 SSH；它们证明的范围不同，计数不相加。

| 检查 | 实际结果 |
| --- | --- |
| `npm test` | 33 个文件，202 项通过 |
| Rust 默认测试（Windows Common Controls v6 清单脚本） | 196 项通过，42 项按原有环境条件忽略，无失败 |
| `cargo check` | 通过 |
| `npm run build` | TypeScript 与 Vite 构建通过；仍有大于 500 kB 的 bundle 提示 |
| 连接、窗口外观、独立编辑器浏览器全套 | Chromium + WebKit：177 项通过，1 项 WebKit forced-colors 跳过；随后新增迁移迟到通知、空标签栏重开及主题用例另行专项验证 |
| 迁移/恢复专项（最终事务保护） | Chromium + WebKit：8 项通过 |
| 标签宽度、二级标签、溢出及主题专项 | Chromium + WebKit：6 项通过 |
| 工作区工具 | 8 项通过 |
| 资源监控与挂载 | 46 项通过 |
| Docker | 44 项通过 |
| NetBird | 20 项通过 |
| 传输 | 全套最初 62 项通过、2 项原生拖放仿真失败；修正测试的悬停后几何测量和实际落点后，失败的 Chromium/WebKit 两项均通过 |
| 终端剪贴板 | 26 项通过 |
| 终端历史 | 12 项通过 |
| 端口转发 | 34 项通过 |
| 数据库设置 | 8 项通过 |
| 更新 | 8 项通过 |
| 日志与存档 | 最终全套 15 项通过；视口跟随用例单独连续 3 次通过，见下文波动记录 |
| 原生文件树 | Windows Tauri + WebView2 + 实际 SSH/SFTP：7 项通过 |
| 原生多终端及迁移 | `node tests/workspace-native.mjs`：11 组检查通过，未捕获页面异常 |

Rust 新增工作区测试覆盖 16 线程并发打开与 requestId 去重、无效请求不留下预留、零终端唯一登记、配置占用、context/窗口代次拒绝、迁移期成员保护及操作租约。集合迁移测试逐个注入缺失接收者，检查无部分提交、源端重放与关闭流的消费顺序。

显式运行的真实 SSH 测试包括 8 项 `connection::tests::jump_`（排除依赖外部 agent 的项）：跳板与逐跳认证、取消、私钥/断开、SFTP/PTY、共享监控、Docker 任务及不确定结果、删除协议、后台下载和日志存档；另已通过 NetBird、恢复和 SSH 流量专项。这里使用 Paramiko 真实 SSH 传输，Docker/NetBird 命令响应由夹具模拟，不能等同于生产 Docker Engine/NetBird 验收。

## Windows 原生验证证据

隔离应用标识：`com.sshworkspace.workspacetabsqa`；独立 WebView 数据目录；loopback SSH `127.0.0.1:22225`；CDP `9225`。未使用用户保存的生产服务器。

`tests/workspace-native.mjs` 实际检查：

1. 一个服务器页、三个真实 SSH/PTY；远端 shell 打开计数恰为三个。
2. 三个终端输入/输出隔离，文件目录保持，显式定位走真实 SFTP。
3. 超过 24 MiB 的快照被拒绝，取消后原连接全部保留。
4. 部分终端冻结后取消，全体回滚，冻结期间输出恢复。
5. 阻止目标前端挂载后销毁真实原生窗口，源端所有连接恢复且未新开 shell。
6. 三终端整体移出，连接 ID 不变，旧窗口的工具 capability 被拒绝，同服务器重复打开保持全局唯一。
7. 移回主窗口，全部连接 ID 和远端 shell 数不变。
8. 已连接与已断开的终端一起往返移动，不发生额外重连。
9. 零终端工作区往返移动；更新工具上下文只验证连接，不创建 PTY。
10. 历史按稳定 terminalId 分组，迁移与工具验证不增加历史 session。
11. 零终端仍可列目录并阻止删除配置；新建继续编号为终端 4。

文件树全套另外覆盖根到 home 的惰性加载、链接逻辑路径、无权限祖先下的可读子目录、晚到请求、两个终端的 CWD 随整体迁移保留、真实编辑器及文件/目录下载、shell 探测降级、重连及键盘/主题行为。

本地证据（`.qa` 为忽略的测试输出目录）：

- [原生验证结果 JSON](../.qa/server-terminal-tabs-native-result.json)
- [三个终端的原生截图](../.qa/server-terminal-tabs-multiple.png)
- [关闭后重建终端的原生截图](../.qa/server-terminal-tabs-native.png)
- [浅色主题回归截图](../.qa/server-terminal-tabs-light.png)
- [深色主题回归截图](../.qa/server-terminal-tabs-dark.png)

## 复现方式

前端执行 `npm test`、`npm run build` 及上述 `package.json` 内对应的 Playwright 脚本。浏览器用例通过 `tests/workspace-fixture.ts` 模拟后端权威登记，并继续使用各业务原有夹具。

Windows Rust 执行：

```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;" + $env:PATH
$env:SSH_TEST_PYTHON = '<已安装 paramiko 的 Python 路径>'
& tests/port-forwarding/run-windows.ps1 --test-threads=4
cargo check --manifest-path src-tauri/Cargo.toml
```

原生脚本依赖正在运行的隔离 Tauri 应用和 `tests/ssh_fixture.py`。SSH 夹具配置 `SSH_WORKSPACE_FIXTURE_DIR`、`SSH_WORKSPACE_FIXTURE_PORT=22225`、`SSH_WORKSPACE_FIXTURE_HOME=/home/ops`；Tauri 使用隔离标识及 WebView profile，并启用 `--remote-debugging-port=9225`。文件树执行时将 `SSH_WORKSPACE_FIXTURE_DIR` 指向同一夹具目录。原生工作区脚本可通过 `SODECK_WORKSPACE_CDP_URL` 与 `SODECK_WORKSPACE_FIXTURE_LOG` 指定 CDP 和事件文件。

正式应用使用原有 `com.sshworkspace.desktop` 配置，通过 `npm run tauri -- build --no-bundle` 生成 `src-tauri/target/release/sodeck.exe`。此次未制作签名安装包或发布版本。

## 验证范围与记录

- 日志视口跟随用例曾失败：保存计数到 2400，视口停在较早位置。原始 HEAD 的隔离副本单独运行通过；当前代码单独连续三次及最终完整日志套件均通过。没有改动日志渲染逻辑，也未放宽断言；此项记录为测试波动，不能据此承诺不存在偶发视口问题。
- 原生测试期间前端 HMR 曾使连接失效并造成文件树失败；停止修改前端后，原生文件树全套通过。新原生脚本还修正了启动 CDP 的等待和原生焦点检查，最终以完整成功运行记录为依据。
- 本次实际桌面是 Windows WebView2；没有完成 macOS/Linux 原生、系统 125%/150% 缩放、中文 IME、真实全屏终端程序和生产服务器长时间运行验收。
- 未完成三台独立主机各五终端的长时间 CPU/内存基准；不能以三终端夹具结果声称不存在性能泄漏。
- 42 个默认忽略的 Rust 测试包含需要外部 Docker Engine、rsync/OpenSSH、ssh-agent 或独立挂载环境的用例；只有明确列出的 SSH 专项另行执行，不能将其余忽略项计为通过。
- 老的通用原生 `desktop.spec.ts` / `features.spec.ts` 未作为此次完整通过的验收套件；本功能原生证据来自新增工作区脚本与更新后的文件树套件。浏览器 IPC 仿真不能替代上述未执行的平台和真实外部服务验证。

# 首版本机验证记录

验证日期：2026-10-14。环境：Windows x64，Node.js 24，Rust 1.99，MSVC 2022，WebView2 154。

## 已通过

- TypeScript 类型检查与 Vite 生产构建。
- 前端规则测试 4 项：多标签规范化/去重，名称、地址与标签组合筛选，已有 Tab 聚焦与同服务器独立新连接，关闭活动/后台/最后一个 Tab 的选择行为。
- Rust 单元测试代码已增加密码原样保存、清空和单字段更新覆盖；本机应用程序控制策略阻止执行新生成的 Rust 测试二进制，因此本轮只确认编译和 Clippy，通过实际桌面端验证了相同的 SQLite 路径。
- Rust loopback 联调：真实 SSH 握手、未信任/错误主机指纹拒绝、错误密码拒绝、SFTP 目录读取、Docker 检测/列表、bash 缺失后回退 sh。
- 真实 Tauri 桌面端自动化场景 5 项：服务器管理与 SSH/SFTP/容器窗口，Docker 失败状态，多 SSH Tab 状态隔离，失败/待连接/远端退出的会话生命周期，密码保存/自动连接/清空/修改。最终完整回归 5 项全部通过。
- 多会话：不同服务器与同一服务器的额外连接输入输出独立；后台终端继续接收输出；切回 Tab 后目录、文件选中项、Docker 展开状态保留；切换后到达的目录查询结果归属于原页。
- Docker 自动刷新只在当前活动 Tab 执行，切回时更新；关闭活动或后台 Tab 不影响其他 SSH，也不关闭已打开的独立容器窗口。
- 错误密码只影响对应 Tab；认证尚未完成时关闭 Tab，后续返回的连接会释放；远端退出保留页面与输出，重新连接保留历史且不增加 Tab；键盘 Delete 可关闭最后一个 Tab。
- 密码认证配置可在编辑窗口保存、修改和清空；保存密码会在重启后读取，新 Tab 与重新连接自动使用；认证失败后修改密码只更新数据库字段，不影响其他活动 Tab。
- Docker 失败场景：命令缺失、权限不足、服务不可达、查询超时、容器已停止、容器无 bash/sh；主终端保持可用。
- 界面刷新后的 SSH 会话清理；关闭容器窗口不关闭主终端。
- Rust Clippy（warnings 视为错误）。
- npm 全依赖审计：0 个已报告漏洞。
- Windows release 可执行文件构建成功（约 11 MB）。关闭 Vite 开发服务后，内嵌页面、SQLite 读取、同服务器两个独立 SSH Tab、SFTP 目录隔离、Docker 树与独立容器窗口均可使用，无前端运行时错误。深浅主题截图已检查。已验证的文件复制到 `src-tauri/target/release/ssh-workspace.exe`，与显式 Windows target 构建产物哈希一致。

上述协议与桌面联调使用 `tests/ssh_fixture.py`：SSH/SFTP 为真实本地协议服务，Docker 命令输出为受控测试响应，不代表已经验证真实 Docker daemon 的所有版本或配置。测试过程没有连接用户的生产服务器。

桌面回归使用独立的 `com.sshworkspace.qa` 数据库和 WebView2 配置目录。打包验证仅创建本地测试服务器，完成后删除测试记录并关闭连接与测试服务。联调中曾观察到一次测试 SSH 服务的密钥交换失败，诊断日志保存在 `.qa/ssh-paramiko.log`；调整测试服务算法配置后最终完整回归通过。

## 验证范围

Windows 本机构建与交互已执行。macOS、Linux 的构建检查已配置在 GitHub Actions 中，本次没有对应系统的执行环境；尚不能将其标记为实测通过。真实 OpenSSH/Docker 服务器、特殊 SSH Agent、需要 TLS 或 SSH transport 的 Docker context 仍需进一步兼容性验证。

截图位于本地 `.qa/desktop-connected.png`、`.qa/container-independent.png`、`.qa/multiple-ssh-tabs.png` 与 `.qa/release-multiple-tabs{,-dark}.png`，该目录及测试密钥不进入 Git。

## Apple 风格界面优化验证

同日完成全局样式优化：侧栏、会话标签、终端工具栏、文件/服务面板、状态标签、菜单、编辑/认证/指纹弹窗和独立容器窗口。终端颜色与滚动条底色保持一致，深色模式的主按钮白字对比度约为 4.95:1。

- TypeScript/Vite 生产构建通过，前端规则测试 4 项通过，真实桌面端完整回归 5 项通过。
- 在独立的 `com.sshworkspace.appleqa` 数据库中验证三个并发 SSH Tab、主终端输入、SFTP 文件和 Docker 树、独立容器窗口；未出现前端运行时错误。服务器均为本地 SSH 测试服务，Docker 列表来自受控测试响应。
- 检查浅色/深色、空状态、弹窗与菜单，以及 860 × 580 内容视口：无横向溢出，文件和服务内容可独立滚动。
- 手动验证分隔条连续拖动、保留抓取位置、宽高边界和方向键调整；分隔条向左拖动 60px，右侧宽度同步增加 60px。
- 通过 WebView2 的媒体偏好模拟确认减少动态效果、减少透明度和提高对比度生效；减少动态效果时禁用终端光标闪烁。
- Windows release 构建成功。关闭 Vite 开发服务后确认独立程序从 `http://tauri.localhost/` 加载新版内嵌界面、正常读取本地 SQLite，无启动错误。测试记录和连接已清理，测试客户端及服务已关闭；正式数据库仅做读取验证。
- 更新后的可执行文件已复制到 `src-tauri/target/release/ssh-workspace.exe`，与显式 Windows target 的构建产物 SHA-256 一致。

本轮截图位于 `.qa/apple-connected.png`、`.qa/apple-connected-dark.png`、`.qa/apple-small.png`、`.qa/apple-editor-light.png`、`.qa/apple-editor-dark.png`、`.qa/apple-fingerprint.png`、`.qa/apple-menu.png`、`.qa/apple-container.png`、`.qa/apple-release.png` 和 `.qa/apple-release-dark.png`。本轮未在 macOS 或 Linux 上实际运行。

## 三栏布局调整验证

同日将服务器列表移到右侧，将每个会话的文件和 Docker 服务面板移到左侧，终端保持在中间。DOM 顺序和分隔条的拖动、方向键行为同步调整。

- TypeScript/Vite 生产构建、前端规则测试 4 项和 Windows release 构建通过。
- 独立的 `com.sshworkspace.layoutqa` 桌面窗口中检查空状态、连接状态和三栏边界，1360 × 860 内容视口没有横向溢出；实际 SSH 终端和 SFTP 目录读取正常。
- 资源区初始宽度 320px，按右方向键后为 330px，继续向右拖动 60px 后为 390px。
- 检查 860 × 580 最小内容视口和深色主题，三栏没有重叠或横向溢出。
- 本轮没有修改 Rust、正式数据库或 SSH 会话管理逻辑，也没有重新执行完整桌面回归；Docker 测试服务在手动检查中出现过密钥交换失败，因此不将本轮 Docker 协议检查标记为通过。

截图位于 `.qa/layout-empty.png`、`.qa/layout-connected.png`、`.qa/layout-small.png` 和 `.qa/layout-dark.png`。本轮未在 macOS 或 Linux 上实际运行。

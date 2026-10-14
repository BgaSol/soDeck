# SSH Workspace

跨平台 SSH 桌面客户端，采用 Tauri 2、React、TypeScript 和 Rust。配置保存在本机 SQLite 文件中，无需部署后端。

## 已实现

- 右侧平铺服务器列表：新增、编辑、删除，名称/地址搜索，多标签与标签筛选。
- 中间 SSH 交互终端：密码、私钥文件或 SSH Agent 认证，主机指纹确认，窗口尺寸同步。SSH 密码可直接保存到本机 SQLite，后续连接自动使用。
- 多 SSH 会话 Tab：每页独立保存终端、文件目录、Docker 状态；切换时后台连接继续运行，同一服务器可以同时建立多个连接。
- 会话标签可拖动排序；拖出主窗口成为独立窗口，也可拖回主窗口标签栏。移动时保留原 SSH 连接、终端输出和滚动位置、文件目录、Docker 树及面板尺寸。右键菜单提供相同的移动入口。
- 左上 SFTP 文件面板：目录浏览、UTF-8 小文件独立窗口编辑，以及文件和文件夹下载。编辑支持查找、语法高亮、保存冲突提示；下载支持递归结构、进度、取消、重试和同名处理。
- 左下 Docker 树：连接后自动检测，镜像与容器列表，15 秒刷新和手动刷新。
- 运行中容器的独立原生终端窗口。重复进入同一容器聚焦已有窗口；切换主窗口服务器不改变容器目标，关闭窗口只结束对应 SSH/exec 会话。
- 文件编辑器自动识别 Java、JSON/JSONC、Shell、JavaScript/TypeScript、Python、Go、Rust、C/C++、YAML、TOML、INI、PowerShell、HTML/CSS/XML、SQL、Markdown、Dockerfile、Nginx 等语言；底部可手动切换，高亮按需加载，切换保留修改、光标和撤销记录。
- 容器独立日志窗口：运行中实时跟随、停止后读取历史，支持行数选择、查找/复制、时间戳、换行、自动滚动和重连。关闭主会话 Tab 不影响日志接收，同一容器重复打开聚焦已有窗口。
- Apple 风格的桌面界面：中性灰层次、轻磨砂工具栏、圆角会话与面板、统一的弹窗和选中态，界面跟随系统深浅主题。
- 左侧文件/服务区域的宽度和上下比例可拖动调整；分隔条获得焦点后可用方向键调整。支持减少动态效果、减少透明度和提高对比度的系统偏好。

## 开发运行

需要 Node.js 22.12+ 和 Rust stable。首次安装依赖后启动桌面端：

```sh
npm ci
npm run desktop
```

各系统还需要 Tauri 的原生构建依赖：

- **Windows**：Visual Studio 2022 Build Tools 的“使用 C++ 的桌面开发”（包含 Windows SDK），以及 Edge WebView2 Runtime。
- **macOS**：Xcode Command Line Tools（`xcode-select --install`）。
- **Ubuntu 22.04/24.04**：`build-essential libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev patchelf libssl-dev pkg-config`。其他发行版安装对应软件包。

只运行 `npm run dev` 可查看浏览器中的界面；SSH 和数据库功能需要桌面端。

首次安装 Rust 或 C++ 构建工具后重新打开终端，使环境变量生效。Windows 可直接双击已构建的 `src-tauri/target/release/ssh-workspace.exe`，无需启动 Vite。

## 构建与检查

```sh
npm test
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
npm run tauri -- build
```

安装包输出到 `src-tauri/target/release/bundle/`。也可使用 `npm run tauri -- build --no-bundle` 只构建当前系统的可执行文件。Windows、macOS、Linux 分别在对应系统构建；仓库包含三平台 CI 配置。

## 数据与连接行为

数据库名为 `servers.db`，位于 Tauri 应用数据目录：

| 系统    | 默认路径                                                               |
| ------- | ---------------------------------------------------------------------- |
| Windows | `%APPDATA%\com.sshworkspace.desktop\servers.db`                        |
| macOS   | `~/Library/Application Support/com.sshworkspace.desktop/servers.db`    |
| Linux   | `${XDG_DATA_HOME:-~/.local/share}/com.sshworkspace.desktop/servers.db` |

数据库保存服务器配置、标签、SSH 密码和已确认的服务器主机指纹。SSH 密码直接以明文存入本地 SQLite，编辑服务器时可修改或清空；私钥口令只保留在当前连接所需的内存中，私钥文件本身不会复制进数据库。

程序不升级或迁移旧数据库。若打开了缺少当前必需字段的旧库，会显示实际数据库路径，并提供“备份旧库并重建”。确认后由正在运行的程序先将旧库完整备份到同一目录的 `servers.backup-*.db`，再创建空的当前结构；备份失败不会清空旧数据，结构正常的数据库拒绝重建。请以实际双击启动的程序为准验证，开发工具与桌面启动可能访问不同的文件系统视图，路径文字相同不代表是同一个文件。

密码认证的服务器已保存密码时，新建会话和重新连接直接使用该密码。未保存密码时，在连接弹窗输入并自动保存；清空密码后恢复为连接时输入。密码认证失败后，点击“重新连接”可以更正并保存密码。首次连接或主机指纹变化仍需核对指纹。

单击服务器只改变右侧选中项。双击或点击右侧“连接”打开该服务器的会话，已有会话时切换到已有 Tab。右键“新建连接”或 Tab 栏的加号可为选中的服务器再建一个独立会话，标题以 `服务器 · 2` 等编号区分。

切换 Tab 时终端、目录和 Docker 面板一起切换。后台 SSH 保持连接并接收输出，目录路径、选中文件和树展开状态各自保留。只有当前 Tab 的 Docker 面板每 15 秒刷新；切回来时更新状态。关闭一个 Tab 只结束其主 SSH 会话，其他 Tab 和独立容器窗口继续运行。断开后保留 Tab 和终端输出，可手动重新连接。方向键/Home/End 可切换 Tab，聚焦 Tab 时 Delete 可关闭。

拖动标签可以调整顺序。将主窗口的标签拖到窗口外会打开独立会话窗口；将独立窗口的标签拖到主窗口标签栏可放回指定位置。右键标签也可选择“移到独立窗口”或“移回主窗口”。移动期间原 SSH 连接继续运行，终端输出、远程目录和 Docker 展开状态随标签转移。连接或认证尚未完成时，先完成当前步骤再移动。

双击远程文件，或选中文件后点击编辑按钮，可打开独立的在线编辑窗口。仅支持不超过 1 MB 的 UTF-8 文本；二进制、无效 UTF-8 和更大的文件会给出提示。保存前检查远端文件是否已被其他操作修改，冲突时可重新加载或明确覆盖。编辑窗口有未保存内容时关闭会提示；主窗口退出也会提示。远端保存保留原文件权限、UTF-8 BOM 和换行格式，服务器需要支持安全替换文件的 SFTP 操作。

选中文件或文件夹后点击下载按钮，也可从右键菜单下载。文件夹会递归保留层级、隐藏文件与空目录；符号链接和特殊文件会跳过，并在任务结果中计数。下载前选择本地文件夹与同名策略：自动改名、覆盖或跳过。底部任务面板显示进度，可取消或重试失败任务；下载在后台运行，主终端可继续操作。

新增、修改或删除服务器配置不会关闭已有会话；会话显示建立连接时的配置。手动重新连接使用最新保存的配置，已删除的服务器需要重新添加后建立新会话。会话列表与终端输出是运行时数据，不写入数据库。

首次连接或主机指纹变化会提示核对；后续 SFTP、Docker 和容器连接均校验该次连接确认的指纹。容器窗口绑定服务器配置、Docker 引擎地址及完整容器 ID。退出整个应用会关闭所有连接。

远程 Docker 功能面向具有 POSIX shell 和 Docker CLI 的服务器，使用当前 SSH 用户权限；支持 Unix socket/DOCKER_HOST，保留当前上下文解析到的引擎地址。需要上下文专用 TLS 证书或 SSH transport 的 Docker context 尚未完整支持。检测失败、权限不足、没有可用 shell 等情况会显示错误，不会自动 sudo 或修改远端服务。容器进入优先 `/bin/bash`，明确不存在时尝试 `/bin/sh`。

本版本暂不提供上传、跳板机、批量执行和凭据保险库。SSH Agent 是否可用取决于系统和 libssh2 支持的代理接口，可使用密码或私钥文件连接。

## 语法高亮与容器日志

编辑器优先使用手动选择的语言，其次按特殊文件名、扩展名、首行 shebang 识别，无法识别时显示纯文本。支持 `.env.*`、`.bashrc`、`Dockerfile.*` 和 Nginx 目录下的 `.conf`；普通 `.conf` 不强行判断语言。语法分析在本机完成，不执行文件内容。

Docker 树中点击“日志”打开独立窗口。日志使用单独的 SSH 连接执行 `docker logs`，默认读取最近 1000 行，并根据打开时的容器状态决定是否持续跟随。修改行数后点击“重新加载”生效。向上滚动或开始查找会暂停自动滚动，接收仍继续；可点击“跳到最新”恢复。日志最多保留 10000 行或 10 MiB，超过后移除最旧内容并提示；日志仅保存在窗口内存。非 TTY 容器的 stderr 用 `[stderr]` 标记，TTY 容器由 Docker 合并输出。网络断开后保留已接收内容并提供重连；Docker 日志驱动不支持读取时显示返回的错误。

## 本地联调

`tests/ssh_fixture.py` 提供仅监听 `127.0.0.1:22222` 的测试 SSH/SFTP 服务。它提供真实 SSH 传输和 SFTP 协议，Docker 响应是测试数据，不会执行本机 Docker 命令。

```sh
python -m pip install -r tests/requirements.txt
python tests/ssh_fixture.py
```

在客户端添加测试服务器：地址 `127.0.0.1`，端口 `22222`，用户名 `ops`，密码认证，密码 `fixture-password`。测试密钥与文件写入被 Git 忽略的 `.qa/`。可检查主机终端输入输出、目录进入、镜像/容器树、独立窗口与 shell 回退。

Windows 上可运行真实桌面端自动化检查。先在一个终端运行 `npm run dev`，再在另一个 PowerShell 终端启动使用独立数据库与 WebView2 配置目录的测试客户端（仅用于本地测试）：

```powershell
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS='--remote-debugging-port=9223'
$env:WEBVIEW2_USER_DATA_FOLDER=Join-Path (Get-Location) '.qa\webview-tabs'
$env:TAURI_CONFIG='{"identifier":"com.sshworkspace.qa"}'
$env:SSH_WORKSPACE_QA_DOWNLOAD_DIR=Join-Path (Get-Location) '.qa\download-test'
cargo run --manifest-path src-tauri/Cargo.toml
```

先创建 `.qa/download-test`，再保持测试 SSH 服务与客户端运行，执行 `npm run test:desktop`。测试数据库位于 `%APPDATA%\com.sshworkspace.qa\servers.db`，与正常客户端配置分开。`SSH_WORKSPACE_QA_DOWNLOAD_DIR` 只在调试构建中替代原生目录选择器，正式构建始终由用户选择保存位置。测试使用真实 Tauri IPC、SQLite、SSH/SFTP，覆盖会话隔离、容器窗口、远程编辑、递归下载和标签迁移。测试截图输出到 `.qa/`。独立的 Rust 协议联调检查为 `cargo test --manifest-path src-tauri/Cargo.toml loopback_ssh_sftp_and_docker -- --ignored`。

详细功能约定见 [docs/feature-design.md](docs/feature-design.md)。

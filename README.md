# SSH Workspace

跨平台 SSH 桌面客户端，采用 Tauri 2、React、TypeScript 和 Rust。配置保存在本机 SQLite 文件中，无需部署后端。

## 已实现

- 右侧平铺服务器列表：新增、编辑、删除，名称/地址搜索，多标签与标签筛选。
- 单层跳板连接：本机 → A → B，A/B 分别认证及核对指纹；终端、文件、Docker 和日志统一使用该路径。
- 中间 SSH 交互终端：密码、私钥文件或 SSH Agent 认证，主机指纹确认，窗口尺寸同步。SSH 密码可直接保存到本机 SQLite，后续连接自动使用。
- 多 SSH 会话 Tab：每页独立保存终端、文件目录、Docker 状态；切换时后台连接继续运行，同一服务器可以同时建立多个连接。
- 会话标签可拖动排序；拖出主窗口成为独立窗口，也可拖回主窗口标签栏。移动时保留原 SSH 连接、终端输出和滚动位置、文件目录、Docker 树及面板尺寸。右键菜单提供相同的移动入口。
- 左上 SFTP 文件树：以 `/` 为根，首次按路径展开到登录目录，其他分支按需读取；终端目录变化后自动展开并定位。支持 UTF-8 小文件独立窗口编辑，以及文件和文件夹下载。编辑支持查找、语法高亮、保存冲突提示；下载支持递归结构、进度、取消、重试和同名处理。
- 新建远程文件/文件夹、选择多个本地文件上传、拖入本地文件或文件夹上传；可在两个服务器窗口之间拖动远程项目，由源服务器直接执行 scp 传到目标服务器。上传、下载与服务器直传统一显示在底部任务面板。
- 左下 Docker 树：连接后自动检测，镜像与容器列表，15 秒刷新和手动刷新。
- 运行中容器的独立原生终端窗口。重复进入同一容器聚焦已有窗口；切换主窗口服务器不改变容器目标，关闭窗口只结束对应 SSH/exec 会话。
- 文件编辑器自动识别 Java、JSON/JSONC、Shell、JavaScript/TypeScript、Python、Go、Rust、C/C++、YAML、TOML、INI、PowerShell、HTML/CSS/XML、SQL、Markdown、Dockerfile、Nginx 等语言；底部可手动切换，高亮按需加载，切换保留修改、光标和撤销记录。
- 容器独立日志窗口：日志永久保存在本地，支持全历史中文搜索、时间和输出流筛选、分页虚拟列表、等级/JSON/异常/ANSI 高亮、原文复制与导出。关闭主会话 Tab 不影响日志接收；关闭日志窗口停止采集，重新打开保留历史并尝试补采。
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

数据库保存服务器配置、标签、SSH 密码和已确认的服务器主机指纹。SSH 密码直接以明文存入本地 SQLite，编辑服务器时可替换或明确清空；已保存密码不会返回到前端或回填输入框；私钥口令只保留在当前连接所需的内存中，私钥文件本身不会复制进数据库。

程序不升级或迁移旧数据库。若打开了缺少当前必需字段的旧库，会显示实际数据库路径，并提供“备份旧库并重建”。确认后由正在运行的程序先将旧库完整备份到同一目录的 `servers.backup-*.db`，再创建空的当前结构；备份失败不会清空旧数据，结构正常的数据库拒绝重建。请以实际双击启动的程序为准验证，开发工具与桌面启动可能访问不同的文件系统视图，路径文字相同不代表是同一个文件。

重建使用应用内确认弹窗，显示实际数据库路径；取消或按 Esc 不执行重建。处理期间禁止重复提交和关闭弹窗，失败可查看原因并重试，成功后显示备份路径。该流程不依赖 WebView 的 `window.confirm()`。前端回归使用 `npx playwright install chromium webkit` 后运行 `npm run test:database`，以模拟 IPC 检查 Chromium/WebKit 交互；真实 SQLite 备份由 `cargo test --manifest-path src-tauri/Cargo.toml db::tests` 验证。

密码认证的服务器已保存密码时，新建会话和重新连接直接使用该密码。未保存密码时，在连接弹窗输入，认证成功后自动保存；清空密码后恢复为连接时输入。密码认证失败后会在当前连接中再次提示，允许更正两次。首次连接或主机指纹变化先核对指纹，再发送认证信息。

配置跳板时，先保存可直接连接的服务器 A，再编辑目标服务器 B，将“连接方式”设为“通过跳板机”并选择 A。B 的地址使用 A 所在网络能访问的 IP 或域名；本机无需解析该地址。A 必须允许 SSH `direct-tcpip` 转发并能访问 B 的 SSH 端口。不依赖已打开的 A 终端，也不需要在 A 上安装客户端或复制 B 的私钥。

当前只支持单层跳板。不能选择自身、引用另一条跳板路径，或把已被引用的 A 改成跳板连接；删除 A 前须先解除 B 等配置对它的引用。A/B 各自支持密码、私钥及 SSH Agent，指纹按直连/跳板网络分别保存。每项业务独立建立自己的转发连接，关闭主终端不会中断下载、文件编辑、容器终端或日志；退出应用会取消连接与下载并等待收尾、刷新日志存档。

单击服务器只改变右侧选中项。双击或点击右侧“连接”打开该服务器的会话，已有会话时切换到已有 Tab。右键“新建连接”或 Tab 栏的加号可为选中的服务器再建一个独立会话，标题以 `服务器 · 2` 等编号区分。

切换 Tab 时终端、目录和 Docker 面板一起切换。后台 SSH 保持连接并接收输出，目录路径、选中文件和树展开状态各自保留。只有当前 Tab 的 Docker 面板每 15 秒刷新；切回来时更新状态。关闭一个 Tab 只结束其主 SSH 会话，其他 Tab 和独立容器窗口继续运行。断开后保留 Tab 和终端输出，可手动重新连接。方向键/Home/End 可切换 Tab，聚焦 Tab 时 Delete 可关闭。

拖动标签可以调整顺序。将主窗口的标签拖到窗口外会打开独立会话窗口；将独立窗口的标签拖到主窗口标签栏可放回指定位置。右键标签也可选择“移到独立窗口”或“移回主窗口”。移动期间原 SSH 连接继续运行，终端输出、远程目录和 Docker 展开状态随标签转移。连接或认证尚未完成时，先完成当前步骤再移动。

双击远程文件，或选中文件后点击编辑按钮，可打开独立的在线编辑窗口。仅支持不超过 1 MB 的 UTF-8 文本；二进制、无效 UTF-8 和更大的文件会给出提示。保存前检查远端文件是否已被其他操作修改，冲突时可重新加载或明确覆盖。编辑窗口有未保存内容时关闭会提示；主窗口退出也会提示。远端保存保留原文件权限、UTF-8 BOM 和换行格式，服务器需要支持安全替换文件的 SFTP 操作。

远程文件树始终以 `/` 为根，首次只展开到登录目录并读取这条路径的直属内容，其他分支首次展开时读取，收起后保留缓存。路径框可定位任意目录，刷新只读取当前目标的一层。主终端在提示符返回时上报实际目录，成功 `cd` 后自动展开、加载并定位，终端继续接收键盘输入；组合命令需等待整条命令结束。Bash、zsh、fish 提供会话临时集成，不修改远端启动文件；其他 shell 或服务器拒绝集成时使用普通 SSH shell，文件树仍可手动浏览。Bash 使用临时 rc 文件加载登录配置，少数依赖 `login_shell` 标志或 `.bash_logout` 的配置与原生登录 shell 有差异。嵌套 SSH、切换账号和容器内目录暂不映射到宿主 SFTP 文件树。

选中文件或文件夹后点击下载按钮，也可从右键菜单下载。文件夹会递归保留层级、隐藏文件与空目录；符号链接和特殊文件会跳过，并在任务结果中计数。下载前选择本地文件夹与同名策略：自动改名、覆盖或跳过。底部任务面板显示进度，可取消或重试失败任务；下载在后台运行，主终端可继续操作。

选中服务器后可点击列表底部“删除”，也可使用右键“删除服务器”或编辑窗口的“删除”。确认框显示名称和地址；确认后删除本地配置、保存的密码及标签关联，不操作远端服务器和文件。该服务器仍有打开的会话标签页、独立会话、容器终端、日志或文件编辑窗口时，需先关闭相关窗口；未完成的下载需等待结束或取消。删除失败保留配置并显示原因，成功后选中筛选结果中的相邻服务器。

新增、修改服务器配置不会关闭已有会话；会话显示建立连接时的配置，手动重新连接使用最新保存的配置。会话列表与终端输出是运行时数据，不写入数据库。

首次连接或主机指纹变化会提示核对；后续 SFTP、Docker 和容器连接均校验该次连接确认的指纹。文件编辑窗口、容器窗口和日志存档以已验证的连接路径区分目标；容器还绑定 Docker 引擎地址及完整容器 ID。退出整个应用会关闭所有连接。

远程 Docker 功能面向具有 POSIX shell 和 Docker CLI 的服务器，使用当前 SSH 用户权限；支持 Unix socket/DOCKER_HOST，保留当前上下文解析到的引擎地址。需要上下文专用 TLS 证书或 SSH transport 的 Docker context 尚未完整支持。检测失败、权限不足、没有可用 shell 等情况会显示错误，不会自动 sudo 或修改远端服务。容器进入优先 `/bin/bash`，明确不存在时尝试 `/bin/sh`。

本版本暂不提供多层跳板、批量执行和凭据保险库。SSH Agent 是否可用取决于系统和 libssh2 支持的代理接口，可使用密码或私钥文件连接。

## 上传与服务器直传

文件面板顶部的“新建”菜单可创建空文件、文件夹；目录右键菜单也提供入口。同名项目会报错，不会清空已有文件。“上传文件”使用系统文件选择器，支持多选；从 Finder/资源管理器拖入文件或文件夹也可上传。拖到目录行时以该目录为目标，拖到文件树空白处时使用当前目录，普通文件行不接收拖入。

要从 A 复制到 B，先连接两个服务器并将其中一个会话移到独立窗口，在 A 的文件树选中项目后拖到 B 的目录。Cmd/Ctrl 单击多选，Shift 单击或 Shift+上下方向键选择连续项目。确认框固定显示来源、目标服务器与目录，并选择“保留两份”“跳过同名”或“覆盖文件、合并目录”。复制保留源文件；递归保留隐藏文件与空目录，跳过符号链接和特殊文件。覆盖采用临时文件加安全重命名，不预先删除旧文件；目标 SFTP 不支持安全替换时明确报错。

在 B 的服务器编辑窗口填写“真实地址”和“直连端口”：它们是 **A 能直接访问 B 的 SSH 地址**，不改变本机连接 B 的普通地址、端口或跳板路径。支持 IPv4、裸 IPv6 和域名，端口单独填写，默认 22；“使用当前连接地址”可显式复制现有值。未填写时无法启动服务器直传。

直传需要 A 提供 POSIX 环境、Python 3.7+、OpenSSH 9.0+ 的 ssh/scp，并且 A 已有连接 B 相同账号的免交互 SSH 认证，例如 A 上的私钥或 SSH Agent。应用不会把本机保存的密码、私钥或 Agent 转发到 A。应用在 A 的用户主目录创建一次性任务目录，调用 scp 的 SFTP 模式，严格核对本次 B 会话确认的主机公钥；显式禁止跳板、代理命令与 SSH 连接复用。连接失败、认证失败或指纹不一致时直接报错，**文件内容始终 A → B，不经过本机，也不降级中转**。不安装常驻远端服务。

最多同时运行三项传输，对同一目标连接身份的写入串行处理。直传按已提交文件数展示进度；上传和下载展示字节进度。新上传/直传的重试复用原计划、目录名称与已完成结果，未完成文件从头传输，不提供字节断点续传。已有下载流程的重试仍重新开始该次下载。

取消会先停止远端 scp/ssh，再清理临时资源；控制连接关闭、HUP/TERM 或 30 秒心跳丢失也会触发停止。无法确认远端状态时显示“状态待确认”，保留应用数据目录 `transfer-receipts/` 下不含凭据的回执，重启后要求重新连接相关服务器并核对。文件提交确认丢失时，核对后显示具体目标路径，禁止该任务直接重试，避免重复副本；检查目标后可重新选择需要的项目。清理失败会保留记录并提供清理入口。

该版本增加了服务器字段，遵循现有“不迁移旧数据库”的约定；旧库仍须由用户在应用内明确备份重建，不会自动清空。

新功能回归：

```sh
npm run test:transfers
python3 tests/test_transfer_runner.py
# 需要 tests/requirements.txt 中的 Paramiko；可用 SSH_TEST_PYTHON 指定解释器
cargo test --manifest-path src-tauri/Cargo.toml transfers::tests -- --ignored --test-threads=1
```

界面套件在 Chromium/WebKit 中模拟 IPC 与原生拖放事件；传输套件建立独立的回环 SSH/SFTP 端点，由 A 侧进程执行系统 OpenSSH scp。二者不代替各平台真实桌面跨窗口拖放验收。当前 macOS 构建与上述自动测试通过，Windows/Linux 原生行为仍需在对应平台验证；Linux 原生内部拖拽依赖显示后端提供全局指针状态。

## 语法高亮与容器日志

编辑器优先使用手动选择的语言，其次按特殊文件名、扩展名、首行 shebang 识别，无法识别时显示纯文本。支持 `.env.*`、`.bashrc`、`Dockerfile.*` 和 Nginx 目录下的 `.conf`；普通 `.conf` 不强行判断语言。语法分析在本机完成，不执行文件内容。

Docker 树中点击“日志”打开独立窗口，立即显示本地存档。首次采集自动读取远端仍保留的全部历史，成功后衔接实时日志；旧版只取过最近 1000 行的存档也会自动补取历史。历史读取中途关闭或失败，下次重新补取并去重，只有完整读取成功才记录历史同步完成。原始字节、解析记录及 FTS5 trigram 索引保存在应用数据目录 `logs/<来源 ID>/`，SQLite 分片约 256 MiB，轮换不删除历史，也不受服务器配置删除影响。关闭日志窗口停止采集并写完已收到内容；正常退出等待日志任务收尾。重新打开和重连按最后 Docker 时间戳回退一秒补采，远端已轮转的日志无法补回。

去重在完整日志进入档案前进行：同一来源内匹配完整精度 Docker 时间戳、输出流及原始字节，按跨采集批次的出现次数抵扣，保留同一次输出中的真实重复次数。匹配的重放不新增原文、搜索记录或导出内容，不再显示“可能重放”。SHA-256 索引缩小候选范围后仍逐字节核对；缺少有效时间戳的内容保守保留。网络拆包和长行用有界内存组装到持久化暂存区；中断留下的半行可续写原记录，异常退出的暂存尾部在重开采集或离线存档时恢复。旧存档首次重新采集时原地建立去重索引，已有的历史重复记录不自动删除。

窗口打开自动采集，关闭停止；工具栏不再提供行数选择、刷新视图、恢复采集、全历史补取或停止采集。临时断线按 1、2、4、8、16、30 秒退避重试，容器停止时每 5 秒检查并在启动后接续；认证、指纹、Docker 权限/驱动或写盘错误保留档案并显示原因和“重试”。本地存档入口保持离线阅读。

前端使用分页虚拟列表，缓存最多 5000 条或约 16 MiB，向上/下滚动自动从本地加载历史。向上滚动或查找暂停自动滚动，采集和保存继续；“跳到最新”恢复跟随。搜索覆盖全部本地历史，支持中文子串、大小写、时间范围和 stdout/stderr 筛选；一两个字符使用可取消的分段扫描。搜索结果固定在查询开始时的序号范围，有新日志时才显示“更新搜索结果”，点击“上下文”查看附近日志。

高亮默认开启，包括日志等级、时间戳、JSON 字段、异常位置、安全 ANSI 颜色及搜索命中，复杂解析在 Worker 中处理并使用有界缓存。时间戳、换行与高亮开关只影响展示；stderr 独立标记，TTY 元数据随存档保留。超长物理行按 64 KiB 片段保存，显示部分折叠，原始字节完整保留。点击序号、Shift 点击结束序号，或输入范围可复制原文、导出范围；“导出全部”流式导出原始传输内容。复制超过 8 MiB 时提示使用导出。服务器列表上方的“本地日志”可在离线或容器删除后查看、搜索和导出存档。

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

文件树回归单独使用 `npm run test:tree`（配置 `playwright.file-tree.config.ts`），默认连接 CDP 9225 和 SSH 22225。在测试服务的 PowerShell 中设置 `SSH_WORKSPACE_FIXTURE_PORT=22225`、`SSH_WORKSPACE_FIXTURE_HOME=/home/ops`、`SSH_WORKSPACE_FIXTURE_DIR=.qa/tree-fixture`，再启动 `python tests/ssh_fixture.py`。在另一个 PowerShell 中使用上面的桌面 QA 启动流程，将 CDP 改为 9225、WebView2 目录改为 `.qa/tree-webview`、应用标识改为 `com.sshworkspace.treeqa`，下载目录使用 `.qa/tree-download-test`（先创建）。该套件覆盖根到主目录懒加载、缓存、实际终端目录协议、软链接、权限错误、会话隔离、窗口迁移、编辑和下载。可用 `SSH_WORKSPACE_QA_CDP_URL` 等环境变量覆盖测试默认值。

## 跳板连接回归

先运行 `npx playwright install chromium webkit`，再运行 `npm run test:connection`。套件在 Chromium/WebKit 中模拟 IPC，检查逐跳认证提示、连接 ID 返回前取消、认证等待期间取消、键盘切换、配置选择、密码保留/替换/清空，以及应用内放弃修改确认；同时检查深浅色和小窗口布局。真实传输测试使用独立的 A/B SSH 测试服务，各自有独立账号和主机密钥，B 的测试域名只能由 A 解析转发；Docker 命令由测试服务模拟。

```sh
# 先按 tests/requirements.txt 准备 .qa/venv；也可设置 SSH_TEST_PYTHON
cargo test --manifest-path src-tauri/Cargo.toml --lib
# SSH Agent 测试只使用这里创建的临时代理，不修改用户原有 Agent
ssh-agent sh -c 'SSH_WORKSPACE_TEST_AGENT=1 cargo test --manifest-path src-tauri/Cargo.toml --lib connection::tests::jump_ -- --ignored --nocapture'
```

原生 Windows 窗口与跨窗口拖动仍使用既有 `test:desktop`、`test:tree` 套件；浏览器中的 IPC 模拟不能替代这些平台回归。

# T-025 实施记录

更新日期：2026-10-28 · 修订：r1 · 状态：工程完成；用户验收待反馈；未发布

范围与确认：[任务 r3](T-025-connection-home.md)、[代码方案 r1](../technical/connection-home-implementation.md)、[功能 r2](../core/connection-home.md)。用户已明确授权“可以开始做了。”；直接在 v2 工作区实施，保留 T-023、T-024 及其他在途修改，不创建提交或推送。

## 实际实现

- 启动、顶部＋及其他连接入口统一到每窗口一个“新建连接”系统页。左侧保存的服务器目录、搜索和分组、新建 SSH / 新建代理；右侧为对应表单或管理页。旧 ServerPicker / ServerDialog / ProxyDialog / ServerGroups 模态组件已退役。
- SSH 默认端口 22、密码、直连、记住凭据；用户名留空，名称可自动生成。新建支持仅保存/保存并连接，编辑只保存。私钥可以选择文件或输入路径。
- 跳板只组合已保存服务器；内联代理是独立的持久提交，保存后选用，取消目标仍保留代理；分组同样独立保存。新建代理不启动 SSH、工作区或代理服务。
- 草稿、目录筛选、滚动和焦点随页面切换保留。16 份主草稿、512 KiB 非秘密、256 KiB 秘密预算在接受输入前核对；秘密离开认证方式、保存或明确放弃后清除。
- 保存按配置、凭据、打开工作区分阶段推进。未知结果查询原 operationId；凭据失败保留已保存对象和可修正的秘密输入。迟到结果不抢已切走的页面。
- `proxies.catalog.*` 使用共享持久回执；`credentials.configuration.*` 使用调用者/进程绑定的条件上下文、凭据修订和 HMAC 意图比较。凭据已提交但目录发布失败返回 `postCommitPending`，后台只重试发布。
- 原生窗口持有 Home / Workspace / Empty 选中对象；旧 snapshot 保持原投影。Home 隐藏原生插件表面、拒绝迟到 visible/focus，不关闭 PTY 或工作区。
- 设置与连接草稿共同参与两阶段关闭；许可不立即丢稿，任何一处取消保留所有参与者。最终核对 session/revision、窗口与工作区代次及归属。主窗口×仍收托盘。
- 三主题和公共控件来自 `@sodeck/ui`；使用 apple-design 的稳定区域、逐步展开、清晰主操作、就近反馈和无布局跳动的焦点。新增 PasswordField / SearchSelect 可供其他界面复用。

## 版本与交付范围

- App：`2.0.0-dev.19`；API/SDK：`5.3.0`；UI：`1.7.0`；业务数据库保持 **v6**。
- 交付目录：`.local/dev/SoDeck-v2-T025/`，入口 `Start-SoDeck.cmd` / `sodeck-desktop.exe`，数据隔离在该目录的 `data/`。
- 本包是 **debug profile 原生程序 + 生产前端资源**，不是安装器或正式发布。原生程序 SHA-256：`c712470c64cf414fb4b6748cf48caee0f1c245d86b814f73e5e218e7c44a3528`。
- 三个已验证独立签名插件：terminal 0.11.0、files 0.8.0、network-proxy 0.4.0。沿用旧插件包，未强制抬高 manifest 版本；T-024 监控插件不随本包交付。
- 构建清单 `build.json` 保存插件/核心/资源哈希、Git 基线和打包时共享工作区指纹。验证对象是未提交工作区及上述冻结 EXE，不把其他任务的在途源码视作本包验收。

## 分层检查与证据

环境：Windows、Tauri/WebView2、独立 `.qa/t025/` 数据。真实网络使用独立 QEMU/OpenSSH 客户机 `127.0.0.1:22345`、客机内部 2201/2202 两个 sshd；SOCKS 夹具只接本机测试端口。没有使用日常服务器或应用数据。

| 检查 | 结果及证据 |
| --- | --- |
| `npm run check` | 通过；生成契约、TypeScript、UI/依赖边界。`.qa/t025/check-final.log`；不是完整 lint |
| `npm run test:contracts` | 通过；SDK/公开契约及已有插件测试，`.qa/t025/contracts.log` |
| `npm run test:core` | 236 通过、31 个需专用环境的用例显式 ignored；`.qa/t025/rust-all.log`。后续原生关闭调整另跑 desktop：54 通过、1 ignored，`native-model-final.log` |
| 配置故障/准入 | 新配置测试 5 通过、并发准入 1 通过；`core-configuration.log` / `core-admission.log`。覆盖重放/重启、依赖身份、凭据写后失效恢复、清秘密后 DB 失败 |
| 草稿/保存模型 | 5 通过；16 稿、非秘密/秘密字节预算拒绝后保留原输入、独立父稿、部分提交与同 ID 重试；`model-final.log` |
| 浏览器交互 | 新首页/分组/服务器 Tab 15 通过，`home-ui-final.log`；工作台/设置/首页组合 35 通过，`ui-regression-final.log`；保存未决关闭 1 通过，`close-wait.log`。计数有重叠，不相加当成独立覆盖 |
| 其他受影响 UI | 调用记录、命令输入与首页组合 18 通过；`browser-targeted.log` |
| 生产前端、原生构建 | `npm run build`、最终 desktop build、custom-protocol 原生 build 通过；`build-final.log` / `desktop-build-final.log` / `native-build-final.log`。保留已有大 chunk / OpenSSL PDB 链接提示 |
| Windows 真实 SSH / 生命周期 | `tests/desktop/connection-home.mjs` 通过；`native-1793180459598/`、`native-final.log`、`native-latest.json` |
| Windows 真实 SOCKS / 消费者 | `tests/desktop/connection-home-socks.mjs` 最终通过；`socks-1793180631706/`、`socks-final.log`、`socks-latest.json` |
| Windows 主题、DPI、IME、私钥 | `tests/desktop/connection-home-visual.mjs` 通过；`visual-1793180387422/`、`visual.log`、`visual-latest.json` |
| 旧 v6 配置副本 | `tests/desktop/connection-home-compatibility.mjs` 通过；`compatibility-1793180564585/`、`compatibility-latest.json`。复制 T-020 已结束测试的 core 目录，旧档案原始 JSON/ID/修订保持，新代理可由旧 API 读取，源库哈希不变 |
| 目录包正常启动 | `tests/desktop/connection-home-package.mjs` 通过；`.qa/t025/package-smoke/results.json` / `connection-home.png`。无 `SODECK_*` 覆盖，原生版本 dev.19、三插件均激活、零工作区/PTY、SSH/代理默认表单可用，退出码 0；不是安装验证 |

## CH / HCODE 结果映射

以下是实际分层验证的映射，不表示每个历史脚本都重新运行。

| 范围 | 观察结果 |
| --- | --- |
| CH-01～03；HCODE-01/06 | 启动无工作区/PTY、仅保存不连接，左侧单击建立首终端；重复＋不复制系统页；已有工作区定位和原布局模型通过 |
| CH-04；HCODE-05 | 预设私钥被核心保存，仍要求首次指纹确认；原生选择器取消保留路径；不存在/超过 1 MiB 文件保留单一档案、显示凭据阶段失败且不连接；旧 prepare 在身份/秘密变化后被拒绝 |
| CH-05 | 新表单先保存两个节点，再选择 J1→J2→目标；真实路径为 3 段 OpenSSH / 2 桥；无路径内新建节点入口；名称/节点自身路径不纳入显式身份 |
| CH-06～07；HCODE-04/11 | 内联代理保存后选中正确父稿；取消父稿保留代理。真实认证 SOCKS 经目标 SSH 运行终端、SFTP/files 与 network 独立池，网络转发收到 OpenSSH banner；关闭工作区资源归零 |
| CH-08～10；HCODE-02/03/07/08 | 草稿与秘密切换保留，冲突/未知回执不重建；隐藏 Home 的关闭提示仍显示。Home 同意、设置取消后两稿和 PTY 都保留；子窗最后工作区合并取消时不移动，确认后保留 PTY；主窗×收托盘 |
| CH-11；HCODE-10 | 三主题，900×600 目标窗口，100/125/150/200% 原生 DPI；文字 200%、窄窗、键盘、中文 IME、Windows 高对比与 WebView reduced-motion。主操作始终可见、无横向溢出；未发生 IME 误提交 |
| CH-12；HCODE-09/10/11 | 100 次原生 Home/工作区往返保持 PTY ID、连接/租约数量和 mount；Shell DOM 不增长。浏览器 100 次切页仍仅一个目录订阅；1000 服务器/1000 代理分页后 DOM 有界；旧 v6 副本完整 |
| CH-13 | 内联组独立保存、同操作核对、筛选、重命名、非空组删除后服务器归属回未分组；管理页不用嵌套对话框 |

保存未决 10 秒的按钮时序由浏览器时钟测试验证；真实两阶段退出、取消和子窗销毁由 Windows 测试验证。renderer 完全无响应的原生兜底分支经源码检查，未额外故意挂死 WebView；不把这一分支标成原生实测通过。

## 调试中发现与处理

- 保存后同一草稿的 `hasSecret` 提示陈旧：采用凭据保存结果，重入干净草稿时更新档案；对应凭据回归已通过。
- 隐藏 Home 时确认框也被隐藏、旧 closeFinished 能影响新确认：确认框移至窗口层并校验 ticket；Windows 与浏览器复测通过。
- 首次启动原生标题未更新：窗口构建后同步系统页标题；最终 Windows 验证通过。
- 旧测试的 Dialog/标题/下拉选择器与新产品结构不符：迁移首页、目录、组和工作台测试；补现代 route node 分页夹具、请求 ID 对应，保持认证/资源断言。
- 初次真实测试使用了另一任务的调试端口：改用专属 125xx，启动前检查端口。前台焦点被切走时原生按键保护拒绝发送；恢复目标窗口后可信 IME 输入通过，文件取消改为关闭已验证归属的原生选择器。
- SOCKS 消费者一次规则行 UI 等待超时（真实转发已收到 banner）；保留 `socks-1793180504097/failure.*`。相同断言最终重跑通过，未放宽资源/认证条件；该偶发测试现象的根因尚未确认。

## 验收与边界

建议用户在独立目录包中检查：

1. 启动和连续点顶部＋，确认一个连接页、左侧目录与两个创建入口。
2. 填写自己的测试服务器，分别尝试仅保存、列表打开和保存并连接。
3. 在目标表单内保存代理后取消目标，确认代理仍可供其他连接选择。
4. 先仅保存 SSH 节点，再为目标选取/排序；确认没有临时新建节点入口。
5. 填写草稿后切服务器/管理/设置，尝试退出并取消，确认字段保留。

本次没有安装器、安装升级、macOS/Linux 桌面验证或正式发布。旧历史 appearance 模态测试以及其他任务的全部专用 native 脚本未统一迁移/重跑；本次以新首页、设置集成和真实消费者入口验证约定路径，不声称整个仓库所有 UI 脚本通过。用户验收仍待反馈；未提交或推送 Git。

收尾：全部 T-025 测试 App 已正常退出，专用调试端口释放；QEMU 客户机通过自身 `/sbin/poweroff` 正常关机，22345 已释放。清除本次 20 个测试 DPAPI 凭据文件与复制的 fixture key，保留非秘密配置/截图/日志/数据库结果。未停止或修改其他任务的虚拟机、日常 App 或其他目录包。复跑真实网络脚本需重新准备隔离 guest/key。

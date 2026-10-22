# SoDeck 1.0+ 在线更新实施记录

日期：2026-10-22。功能已完成本地自动化验证，现随全仓改动进入 1.0.0 发布流程；本次未覆盖本机安装。

基线：[功能设计](online-update-1.0-design.md)、[代码设计](online-update-1.0-code-design.md)。开发验证时元数据为 0.1.5；经用户授权发布后已统一设置 1.0.0，不增加 0.x 专门迁移。

## 已实现的行为

- 正式构建默认自动检查和后台下载，两项偏好独立。成功检查间隔 24 小时；失败按 15 分钟、1 小时、6 小时退避。启动至少等待 30 秒，开发构建不运行自动任务。
- 下载关闭设置后继续；网络和流读取可取消。取消或失败后按版本持久化抑制自动重试；手动下载或重新打开自动下载可解除抑制。跳过版本独立保存。
- 下载使用 Rust 流式 HTTP，清单限制 1 MiB、包体 512 MiB、说明 64 KiB；无 Content-Length 时仍逐块限制。只接受官方仓库对应 tag 和平台文件名，重定向限制为 HTTPS、GitHub/CDN 白名单和 5 跳。
- 签名及签名版本校验使用锁定的 Tauri Updater 2.14；前端没有 updater/process 插件权限。下载包仅保存在进程内存，退出后重新下载，不做磁盘缓存或断点续传。
- 安装仅由主窗口确认。确认凭据有效 60 秒，绑定候选包指纹、主窗口代次和实际资源身份；文件任务、编辑器、待确认的 Docker/NetBird 操作阻止安装，终端、日志、监控、转发连接列入中断说明。
- 提交时重新验签、联网确认稳定清单中的同一包、检查本地安装路径，再在统一 configuration 闸门内复核实际资源。工作已变化则返回新确认，不关闭连接。复核联网失败仍保留已下载包。
- 持久化 pending 后才关闭工作；安装阶段拒绝新工作和普通退出。清理开始后的失败统一提示“重启 SoDeck 后继续”，不会把已部分关闭的工作区标成恢复正常。
- 新启动实际版本达到目标或更高正式版本才显示完成；否则显示未完成。结果独立保存，后台检查不清除，用户明确关闭才确认已读。
- 界面采用内联确认、持续但克制的状态反馈和分动作等待状态。取消不被偏好保存禁用，后台事件不打开设置、不改变焦点，遵循 reduced-motion。

## 模块与生命周期

`src-tauri/src/updates/` 拆分为 `model`、`manager`、`policy`、`store`、`platform`、`transport`、`install` 和测试。`manager` 是应用级单一协调任务，使用容量 64 的命令队列；网络、阻塞验签及安装工作均有操作编号和终态监督。进度使用 watch 合并并每 150 ms 广播，快照 revision 防止迟到响应覆盖新状态。

阻塞验签无法被强制中止：取消立即撤下进度，但保留任务归属直到工作真正退出，期间不能启动另一份包体下载。任务 panic/JoinError 会转换为失败终态。HTTP 客户端在首次请求前显式初始化 ring TLS 提供器，不依赖旧更新器的检查顺序。

`store` 原子写入 `update-state.json`。未知 schema、损坏文件保留原文并暂停自动任务。偏好 revision 独立于下载进度；保存失败不能把失败偏好当成成功。安装工作只持有冻结的 pending 副本，在清理后写交接标记；这段时间协调任务拒绝持久化变更，避免多写者覆盖。

复用 `lib.rs` 的 configuration/stopping/shutdown_work，并补上日志作业、监控订阅、NetBird 作业和转发连接的实际登记闸门。文件任务与 NetBird 的安装检查使用内存状态，避免在安装闸门内查询 SQLite。转发连接代次参与影响指纹，即使连接数相同但连接已替换，也要求重新确认。远端删除先登记操作再锁任务表，避免与安装复核锁顺序相反。

公开命令：`update_snapshot`、`update_check`、`update_download`、`update_cancel_download`、`update_preferences`、`update_skip_version`、`update_preflight`、`update_install`、`update_ack_notice`、`update_ack_result`、`update_open_release`、`update_open_manual_asset`、`update_restart_recovery`、`update_focus_main`。

前端由 `updateApi.ts` 封装这些命令，`useAppUpdate.ts` 先订阅再取快照；订阅或读取失败会重连。`updatePresentation.ts` 派生动作能力，`SoftwareUpdateSection.tsx` 和 `UpdateInstallReview.tsx` 展示状态与确认。

## 平台边界

| 安装来源                | 当前代码策略                                             | 发布验收                                                                          |
| ----------------------- | -------------------------------------------------------- | --------------------------------------------------------------------------------- |
| Windows x64 NSIS EXE    | 应用内下载、验签、安装器交接                             | 需要正式签名包在隔离环境执行升级、取消/UAC、失败后启动验证                        |
| Linux x64 AppImage      | 应用内下载、验签、替换、重启；检查原 AppImage 与目录可写 | 需要 Linux 实机验证路径权限、替换、失败恢复                                       |
| Windows MSI / Linux DEB | 通用版本发现与对应格式下载                               | 不调用另一安装格式覆盖当前安装                                                    |
| macOS Apple Silicon     | DMG 手动下载                                             | CI 已接 Developer ID/公证凭据；尚未提供凭据或完成实机验收，因此仍未启用应用内安装 |
| 未知安装来源或架构      | 明确不可应用内更新，提供官方发布页                       | 不借用其他平台包，不显示误导性的更新能力                                          |

仍生成并签名 macOS updater 归档以保持发布契约；归档存在不意味着当前客户端已启用该平台应用内安装。Tauri 升级签名与操作系统商业代码签名是两套独立机制。

## 发布链路

`docs/release-notes/<version>.md` 是 Release 与清单 notes 的共同来源，缺失、空白或超过 64 KiB 均阻止发布。1.0.0 说明已准备。

清单包含 `sodeck.schemaVersion: 1` 和五种 `manualAssets` 精确文件名；附件总数仍为 11。发布前验证三种 updater 包的签名、版本绑定、大小以及全部附件哈希，草稿全部完成后才公开。高水位取所有已公开正式版本，不能通过将 latest 调回旧版本绕过版本递增。

公开后从固定稳定入口获取 latest.json，验证版本、schema、手动附件、平台 URL 和签名。探测失败会明确报告“已经公开，需要修复”，不会错误承诺仍是草稿。详见[发布操作说明](releases.md)。

## 本地验证

| 检查                                                            | 结果                                                    |
| --------------------------------------------------------------- | ------------------------------------------------------- |
| `npm test`                                                      | 32 个文件、201 项通过                                   |
| `npm run test:release`                                          | 15 项通过                                               |
| `npm run test:updates`                                          | 8 项通过                                                |
| `npm run test:settings`                                         | Chromium/WebKit 共 14 项通过                            |
| `npm run build`                                                 | 通过；保留既有大于 500 kB chunk 提示                    |
| `cargo test --manifest-path src-tauri/Cargo.toml --locked`      | 188 项通过，42 项依赖外部夹具或性能环境的测试按配置跳过 |
| 额外隔离 SSH 回归（显式运行 ignored 项）                        | 端口转发生命周期与重连、NetBird 操作各 1 项通过         |
| `cargo check`、`cargo fmt -- --check`                           | 通过                                                    |
| `npm run release:check`、`node scripts/release-notes.mjs 1.0.0` | 通过                                                    |

更新专项覆盖包体上限、实际 loopback HTTP 截断/取消、恶意重定向、平台隔离、损坏存储、资源身份、退避/抑制、工作任务异常及验签任务取消归属、启动结果判定；界面覆盖关闭设置继续下载、安装阻止与重新确认、手动入口、验证失败、独立偏好、减少动态效果、不抢焦点、结果关闭和失败恢复。

NetBird 的额外 SSH 测试最初在夹具建数据时失败：旧夹具使用自定非空服务器 ID，与当前工作区的服务器保存规则冲突。已仅调整该测试先新建服务器、采用返回 ID 再建立连接，重新执行通过；没有改回业务保存规则。

已查看 `.qa/updates-confirm-1.0.png` 的实际测试截图，内联确认可见、按钮与正文无裁切。自动化使用模拟 IPC；未把这些结果视为真实安装器、跨平台安装包或签名公证验收。后续发布需依照上表在独立安装环境验收，不直接替换开发者正在使用的 App。

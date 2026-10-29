# T-029 实施结果

日期：2026-10-29 · 基线：dev.21 / API 6.3，v2 未提交工作区 · 所属：[T-029 r4](T-029-online-updates.md)

后续Git状态：用户已另外授权将当前工作提交推送，并把旧master保留为v1、v2更名main，见[分支调整记录](git-main-transition.md)。下文“未提交/推送”是本次测试完成时的历史基线；Release、Secrets与公网升级验证仍未执行。

本地实现、Windows 安装升级和独立试用包已建立。整体仍为“开发中：G5 外部验证待完成”；用户尚未验收，未提交、推送或发布。core v7 和四观测分区 v2 保持。

## 交付入口

- 试用目录：`.local/dev/SoDeck-v2-T029-r2`；双击 `Start-SoDeck.cmd`，进入设置 → 更新。
- 核心 `2.0.0-dev.21`，API/SDK `6.3.0`。随附 T-028 四原包：终端 `0.13.1`、文件 `0.10.0`、监控 `0.3.0`、网络隧道 `0.6.0`，原字节哈希保持。
- 这是 Windows x64 debug 开发目录包，development 身份、独立 data，未配置正式更新源。可体验更新设置、插件管理和原有业务；不能拿它连接未发布的正式在线更新渠道。
- QA A/B 安装器位于 `.qa/t029/release-local/{A,B}/assets`，仅供隔离安装验证；测试公钥、标记插件和 QA 身份不进入试用包或正式渠道配置。
- 交付 r2 EXE SHA256：`4f7ef03cc1e243a5c7d13546d3e7dd9d14c6f9e112616412b4e5ec987d1f8419`。全文件清单、哈希和打包时源码指纹在交付目录 `build.json`。r2包含Windows原生自定义按钮修复；r1仅保留为先前验证产物，当前试用以r2为准。

## 已实现的行为与模块

设置 → 更新提供核心和官方四插件的分项状态、检查、下载、安装及重启入口。自动检查只提示；下载不写插件 pending、不安装核心；插件准备后完整重启激活。核心安装经过既有草稿/关闭审查，再释放资源并交接当前用户 NSIS。正常退出或收起到托盘不会安装核心。

`crates/updater`负责严格目录签名、固定 GitHub 源、有限下载/缓存、版本兼容规划和持久回执。API 6.3 的 schema 同时生成 SDK 与 Rust 契约。实际 appVersion、buildId、分发身份与插件兼容版本分开；所有核心/观测/插件数据格式在安装前核对。

`plugin-manager`支持显式目标兼容检查与不可运行的未来核心候选。手动导入、随附播种和在线准备经过同一来源协调器；同字节官方包经手动导入也转为 local，停止自动跟随。停用插件保持包和配置且不执行候选；中断激活、损坏 journal 和错误来源保留证据，不自动重新试运行同摘要。

`scripts/release/`、`release/`和两个 v2 GitHub workflow 已建立。五组件按实际构建输入分别决定复用/构建，漏升版本拒绝。专用标签触发，先草稿资产、真实 asset ID 和完整签名目录，最后公开且 make_latest=false。普通 v2 提交不发布。命令与公钥/Secrets 前置见[发布工具说明](../technical/release-v2.md)。

## 已执行检查

| 检查 | 结果与证据 |
| --- | --- |
| 类型、生成来源与边界 | `npm run check`通过；`.qa/t029/check.log` |
| 全部 JS/TS 契约 | `npm run test:contracts`通过；`.qa/t029/contracts-final.log` |
| 桌面 Rust | 63 通过、1 个专用子进程入口 ignored；父测试实际启动该入口并验证五个写入断点，非遗漏验证。最终rfd配置重跑同样通过，`.qa/t029/desktop-rfd-final.log` |
| 插件管理器 | 32 通过，含 future-core、初始化失败回退、停用候选及包完整性 |
| updater | 26 通过（8 单元 + 9 持久化 + 9 规划/签名），含 16 种启停集合、来源恢复、各数据格式门禁、错误 buildId、缓存固定引用、真实 loopback HTTP 的取消/错 hash/错长度/429；以上 Rust 统一日志 `.qa/t029/rust-final2.log` |
| 浏览器设置/插件/更新 | 11 通过，含分步动作、草稿跨页、未知保存回执、窄窗/200%文字/键盘/减少动态；`.qa/t029/ui-final.log`。使用注入提供者，和原生结果分开 |
| 发布规则 | 3 通过；`.qa/t029/release-rules-final.log` |
| 五组件输入独立性 | 终端版本和 workspace lock 只影响终端；仅文档/未使用 SDK 导出不影响组件；已使用公共 Button 影响五组件；`.qa/t029/release-local/input-regression.json`。在源码副本修改并恢复 |
| 发布中断模拟 | 真正 publish 脚本 + 完全替换的 fetch：第二资产上传失败只留草稿；重跑复用同字节/ID，Rust 验签完整目录后只公开一次，公开后拒绝覆盖；`.qa/t029/publish-simulation.json`。没有网络写入 |
| 完整构建 | 根 development debug 与 QA release A/B NSIS 均通过；`.qa/t029/build-debug-final.log`及`release-local/build-{A,B}.log`。存在既有未用字段/导入及 OpenSSL PDB 警告，不影响构建 |
| 试用包 | r2包含64个声明文件及 build.json；移动到带空格目录后全哈希、普通启动/独立 data/四原签名包/更新窗口/完整重开和实际 .cmd 启动均通过，`.qa/t029/delivery-latest.json`和`delivery-1793237963457`记录。交付目录不写入测试数据 |

以上都是本地工作区验证，不能替代真实 Actions 或公网升级。

## 原生与安装实测

开发原生验证：`.qa/t029/native-latest.json`。Windows WebView2 实测 development 身份与官方源禁用、任意 URL 拒绝、策略草稿/修订保存、同字节手动导入转 local、重启偏好和四插件保留。

完整安装使用同一 Windows 账号与 `com.sodeck.v2.qa.full.1793235402271`。Linux 服务是本任务 QEMU 写入层 `.qa/t029/linux.qcow2`、loopback SSH 22384，未连接日常服务器。证据分为两轮：

1. `.qa/t029/release-local/validation-1793235562604`：四插件逐项 A→B 全部通过；下载和准备期间 PTY/业务保持、当前版本不变；完整重启后新 UI meta 与真实 worker 标记执行，PTY、SFTP、CPU/内存/挂载、HTTP/SOCKS 和固定转发实际可用；核心与其他包哈希保持，监听在退出后释放。核心第一次真实交接失败，原 result.json 保留为失败。
2. `.qa/t029/release-local/validation-1793236138974`及`.qa/t029/installer-latest.json`：修复后核心 A dev.21→B dev.22 通过。复用原 DPAPI 凭据和字号 18，不重新写入来掩盖保留问题；脏设置取消安装后 PTY 仍可用；安装器实际替换并自动重开准确 B buildId，handoff 和原 operation 均 installed，取得数据锁且不自动恢复远端会话；重新连接后真实业务仍通过。B 的全部停用、仅终端、恢复全部均经完整重启核对运行插件数，更新页保持可用。

3. `.qa/t029/release-local/validation-1793237897234`：最终rfd原生控件修复后重新构建A/B，核心准确版本/回执、取消关闭、DPAPI/字号、真实Linux业务及三种启停组合全部再次通过。此前脚本过早退出导致的保护超时保留在`validation-1793237740157`，已补新界面就绪等待，不修改产品预期。

三主题原生截图在第二轮和最终目录 `updates-soft-light.png`、`updates-soft-dark.png`、`updates-dopamine.png`，已检查；四插件截图在第一轮目录。最终副本随试用包 evidence 提供。

独立机制探针：`.qa/t029/probe-latest.json`。Tauri CLI 2.12.4 / updater 2.14.1 的 NSIS A0.1.0→B0.1.1、缓存验签、缺版本/错版本/篡改拒绝、重开锁与测试文件保持通过。该探针已卸载，其结论与完整应用结果分别保留。

安装暂存失败补验通过，`.qa/t029/installer-failure.json`与`release-local/install-failure-1793237540798`单独记录：仅在 QA 源码副本的真实 `update.install` 调用点注入无效 TEMP/TMP，返回后恢复；根应用和试用包不含故障开关。computer-use核对原生“重新打开/关闭”并点击重新打开，实际A版本重开、失败回执保留且不自动重试。最初“启动前 TEMP 为文件”和“运行中改名临时目录”两种注入分别阻止 WebView 初始化及返回 EPERM，均未进入安装，不计为产品缺陷或安装失败通过；各次 result.json 保留。

## 本轮修复与预期保持

OU-28故障补验发现Windows rfd默认MessageBox后端忽略自定义标签并返回Ok/Cancel，原代码期待Custom导致“重新打开”未执行。启用rfd的common-controls-v6使用TaskDialog，保留已确认的“重新打开/关闭”标签和回执语义；既有超时审查同样使用Custom，随之恢复预期。重建QA故障包及完整A/B、复验失败恢复/正常升级后，再生成r2试用包替代r1。修复属于已确认的原生恢复路径，不改变产品范围；失败证据保留。

最终正常升级复验已安装准确B并取得installed回执，但测试脚本在新WebView的React界面就绪前直接发送app.exit，触发既有“页面暂未响应”保护而等待超时。复验入口补齐与普通launch相同的`#root > *`就绪等待后再执行退出；不绕过关闭审查、不改变产品超时或接受强杀。原失败记录保留。

- 原生首启失败：补齐 Tauri updater 必填 pubkey 配置；实际安装公钥仍来自编译身份，并强制绑定版本验签。
- 核心仅退出未安装：最后 WebView 销毁使默认 ExitRequested 提前结束事件循环；明确交接期间拦住 code=None 的自动退出，保留明确退出/重启。修复后实机 A→B 通过。
- 退出并发：登记后台任务、取消并等待完成；释放 shutdown_lock 后 drain，避免准备任务死锁；提交前再次核对关闭目标、最新计划与取消状态。
- 恢复一致性：成功安装更新原操作回执；损坏 journal 保留 pending，未来 schema 不覆盖；停用候选连续两次启动不初始化，显式启用后才激活。
- 缓存与发布：流式校验、请求/无进度超时、取消优先于缓存；下载不驱逐自身，metadata 预留正文/ETag 槽；发布输入纳入真实依赖闭包、锁文件校验和、host/主题/许可证；草稿查询使用认证并兼容 GitHub 标签斜杠编码。

这些属于已确认 r2 内部修复，未改变自动检查、手动下载/安装、官方来源、数据格式或兼容规则。

## 复现、清理与待完成项

入口：`tests/desktop/online-updates.mjs`、`online-update-installer.mjs`（完整测试，`--core-only`复用已通过四插件/凭据证据）、`online-updates-package.mjs`；构建 QA 夹具见发布工具说明。原生测试串行；先创建隔离环境、测试钥和签名 A/B，不能直接用于日常安装。`scripts/package-online-updates-dev.mjs`只创建全新的 `.local/dev`子目录，存在时拒绝覆盖。

清理结果由`.qa/t029/cleanup.json`维护：关闭测试应用、卸载 QA 安装并默认保留数据；核对 core/插件配置文件哈希，关闭 QEMU，确认 SSH/调试端口释放。保留 QA 数据、磁盘和原始失败证据供复查；T-028 试用目录及日常数据未改。

最终文档检查：12份本次来源/索引的350个本地链接有效，无行尾空白；`git -c core.safecrlf=false diff --check`通过。当前仍在v2，未创建分支、提交或推送。

G5 仍未运行：正式渠道公钥/Secrets、实际 GitHub Actions、预览 Release 上传/发现和公网下载升级。`release/channels`正式公钥保持空值，发布会明确拒绝。依已确认任务的 G5 授权边界，配置 Secrets、提交/推送、创建发布标签和公开预览 Release 需要另行明确授权。全部本地结果不代表已发布或用户验收；Windows Authenticode 证书尚未配置。

尚未声称通过的细项：真实磁盘耗尽/NSIS 文件占用与安装进程被外部强杀场景、真实 GitHub 限流/ETag 网络响应。当前分别有容量/错误回执/中断恢复与本地 HTTP 测试；真实边界在预览安装验证时继续补证。历史发布目录会保留并计入 512 MiB 总预算，空间不足会拒绝新下载/检查，未做历史目录自动清理。

用户试用建议：打开新目录；在更新页修改自动检查并应用，重开核对保存；在插件管理中停用一个插件、完整重开后核对状态；手动导入仍走既有完整性与来源提示。实际在线下载安装体验需配置并验证预览渠道后再验收。

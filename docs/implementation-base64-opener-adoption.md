# Base64 与系统打开库接入记录

方案见 [接入方案](plan-base64-opener-adoption.md)。本轮从干净的 origin/main `be7247d1c41b896af8b562ccb31965cc166a73bc` 开始，两个改动分别提交。最终交付改为推送远端 main，不正式发布；先前准备的候选与草稿保留原始来源，详见[核心候选记录](releases/v0.21.1.md)及各插件 CHANGELOG。

## Base64

核心与共享客户端固定使用 base64 0.23.1（MIT OR Apache-2.0，MSRV 1.71，维护中的标准编解码库），替代三处手写实现。核心直接编码，Docker 使用共享客户端严格解码，出错仍释放原任务，不自动重复命令。保留客户端公开函数、2 MiB 编码文本上限、8192 原始字节分页、偏移与控制帧预算。

旧 Docker 测试已复现 `Zg=a` 被错误接受；修复后共享回归覆盖标准向量、全部字节、8191/8192/8193 边界、非规范填充/尾位与输入上限。核心 SSH/RPC 夹具新增双流二进制分页、重复读取和实际序列化帧预算；Docker 实际进程夹具检查坏编码后的报错、释放及不重放。

共享客户端及五插件各自锁文件同步更新；保留其他库仍需要的旧版 Base64。替代方案是继续维护已有手写实现；本次复用成熟库，减少协议实现的维护责任。

## 系统打开

核心固定使用 tauri-plugin-opener 2.7.2（MIT OR Apache-2.0，MSRV 1.90，Tauri 官方维护）及锁文件中的 open 5.4.5，删除 ShellExecute/AppKit/GTK 三套打开实现。保留原异步接口，通过 spawn_blocking 调用 Rust 自由函数；PathBuf 原样传入 open_path，网页仍只接受绝对 HTTP/HTTPS。目录增加绝对路径、NUL 和 metadata.is_dir 检查，页面、资源、下载目标归属检查继续保留。

没有注册 opener 插件、注入链接处理脚本、增加 npm 依赖或权限声明。原 windows-sys/objc/GTK 依赖仍用于剪贴板、窗口等其他系统功能。替代方案是维持原三系统实现；官方插件统一承担启动器兼容和错误处理。

新增独立 `--desktop-open-test` 烟测，直接委托生产网页和目录入口；系统关联 handler 只用于隔离验收，未进入核心包。Package SoDeck 在五个原生目标调用此入口，Linux 的 DEB 与 AppImage 分别运行。目标是验证完整 URL 与中文/特殊字符目录各一次、非法目标拒绝和正常退出，Windows/Linux 另设启动器失败用例；接入工作流不等于五平台实际包全部通过。独立报告不使用历史 clipboardNative/desktopFilesNative 代替。

Windows 默认浏览器 UserChoice 优先于通用 scheme，夹具解析实际 ProgID，并在退出时恢复修改。目录收据按文件系统身份及原生 basename 接受 Shell 展开的 8.3 别名，仍拒绝其他目录、被替换目标或重复收据；URL 逐字节核对。

macOS 夹具编译临时 AppKit/LaunchServices handler，使用 Apple Events 收件及 ad hoc 签名。runner 保留受保护的默认关联时，`--handler-only` 仅编译夹具并标记 skipped；实际包模式仍调用真实入口，报告系统交付及非法输入拒绝，但设置 `targetsExactlyOnce:false` 和 `receiptSkipReason`。本轮没有确认 macOS 目标收据，不能把这种运行记为 URL/目录已到达各一次。

成功表示系统已经接受交付；Linux 超过每启动器 250ms 才出现的失败、macOS 目标应用在 `/usr/bin/open` 成功之后的退出不由接口确认。受控 handler 不等于人工验证真实浏览器/文件管理器可见窗口；UNC、Wayland、多屏/DPI 和真实用户桌面验收在本轮不外推。

## 验证与代码量

本机 Windows x86_64 的共享客户端、五插件 Rust 测试、核心全套测试、实际 Docker 包进程回归、类型/格式、前端构建和 13 项正式 UI 回归通过。合入既有 CI 分层后，100 项脚本测试、完整 Windows UI 对照、真实 OpenSSH/终端及文件回归通过；netem 等需要专项环境的 ignored 不计为普通套件通过。

[Check #42783681618](https://github.com/BgaSol/soDeck/actions/runs/42783681618) 在 `f0b4002` 全部通过，覆盖核心及五插件的五平台原生源码、Windows UI/SSH、Linux 真实业务及 Docker Engine。[Check #42787813366](https://github.com/BgaSol/soDeck/actions/runs/42787813366) 在 `23d56ed` 的五平台源码和 Windows UI/SSH 通过，但 Linux 文件 UI 在下载完成通知与断线导出步骤竞争，汇总失败；后续等待同一任务的编辑页完成通知，保留字节、BOM/CRLF 与离线导出断言。合并后的最终 CI 结果集中记录到上述核心候选记录。

五个涉及编解码或打开适配的业务文件，排除 `cfg(test)` 后净减少 154 行；新增回归、显式烟测及锁文件另计。opener 接入使核心锁文件新增 36 个包条目（含 open、Linux zbus/异步依赖），不承诺安装包减小。本轮没有获得五平台最终有效核心候选，无法给出可比的最终包体变化。

用户另一个 CI 优化会话多次合入本任务的 main 修正；最终对比时没有独立 Base64/opener 文件差异。其工作流分层、诊断、终端时序与 macOS 按钮验收等待已合入同一工作目录，详情见[CI 实施记录](implementation-ci-optimization.md)。

未完成：最终源码的五平台实际核心包、macOS 默认应用目标收据、真实包组合和历史版本升级。本轮没有再发起发布或包组合；既有部分包成功结果不能代替这些范围。

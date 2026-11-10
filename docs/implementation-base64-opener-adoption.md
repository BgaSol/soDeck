# Base64 与系统打开库接入记录

方案见 [接入方案](plan-base64-opener-adoption.md)。本轮从干净的 origin/main `be7247d1c41b896af8b562ccb31965cc166a73bc` 开始，两个改动分别提交，核心和插件独立打包发布。

## Base64

核心与共享客户端固定使用 base64 0.23.1（MIT OR Apache-2.0，MSRV 1.71，维护中的标准编解码库），替代三处手写实现。核心直接编码，Docker 使用共享客户端严格解码，出错仍释放原任务，不自动重复命令。保留客户端公开函数、2 MiB 编码文本上限、8192 原始字节分页、偏移与控制帧预算。

旧 Docker 测试已复现 `Zg=a` 被错误接受；修复后共享回归覆盖标准向量、全部字节、8191/8192/8193 边界、非规范填充/尾位与输入上限。核心 SSH/RPC 夹具新增双流二进制分页、重复读取和实际序列化帧预算；Docker 实际进程夹具检查坏编码后的报错、释放及不重放。

共享客户端及五插件各自锁文件同步更新；保留其他库仍需要的旧版 Base64。替代方案是继续维护已有手写实现；本次复用成熟库，减少协议实现的维护责任。

## 系统打开

核心固定使用 tauri-plugin-opener 2.7.2（MIT OR Apache-2.0，MSRV 1.90，Tauri 官方维护）及锁文件中的 open 5.4.5，删除 ShellExecute/AppKit/GTK 三套打开实现。保留原异步接口，通过 spawn_blocking 调用 Rust 自由函数；PathBuf 原样传入 open_path，网页仍只接受绝对 HTTP/HTTPS。目录增加绝对路径、NUL 和 metadata.is_dir 检查，页面、资源、下载目标归属检查继续保留。

没有注册 opener 插件、注入链接处理脚本、增加 npm 依赖或权限声明。原 windows-sys/objc/GTK 依赖仍用于剪贴板、窗口等其他系统功能。替代方案是维持原三系统实现；官方插件统一承担启动器兼容和错误处理。

新增独立 `--desktop-open-test` 烟测，直接委托生产网页和目录入口；系统关联 handler 只用于隔离验收，未进入核心包。Package SoDeck 在五个原生目标验证默认 handler 收到完整 URL 与中文/特殊字符目录各一次、非法目标拒绝和正常退出；Linux 的 DEB 与 AppImage 分别运行，Windows/Linux 另检查即时启动器失败。独立报告不使用历史 clipboardNative/desktopFilesNative 代替。

成功表示系统已经接受交付；Linux 超过每启动器 250ms 才出现的失败、macOS 目标应用在 `/usr/bin/open` 成功之后的退出不由接口确认。受控 handler 不等于人工验证真实浏览器/文件管理器可见窗口；UNC、Wayland、多屏/DPI 和真实用户桌面验收在本轮不外推。

最终本机、源码 CI 与原生验证结果将在本轮完成后补齐。发布来源、原始打包运行与 Release 链接统一记入对应核心版本说明及插件 CHANGELOG，避免重复维护交付状态。

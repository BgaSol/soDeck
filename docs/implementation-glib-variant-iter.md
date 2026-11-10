# GLib VariantStrIter 兼容安全补丁

## 可验收行为

- 核心和 GTK3 的直接、间接 GLib 依赖统一使用带上游安全补丁的 0.18.5 源码，不保留第二份 0.20 依赖。
- 优化编译下，字符串数组的正向、反向、混合及空数组迭代返回正确值，不发生空指针崩溃。
- Linux x86_64/ARM64 的 Check 在原生核心测试后运行锁定依赖的优化回归；核心和五插件的其他检查沿用原范围。

## 原因与实现

Dependabot #3 将直接依赖升级到 glib 0.20.0，仍保留 GTK3 间接依赖的 glib 0.18.5。原始 Linux 检查首先在 glib 0.20.0 编译时失败：锁文件中的 gobject-sys 0.20.10 将它无条件引用的 `G_SIGNAL_ACCUMULATOR_FIRST_RUN` 放在尚未启用的 `v2_68` feature 下。当前 GTK3/Tauri 依赖链要求 0.18 API，独立提高直接依赖不能替换告警涉及的旧库，也会混用不同版本的类型。

[RUSTSEC-2024-0429](https://rustsec.org/advisories/RUSTSEC-2024-0429.html) 对应 `VariantStrIter::impl_get`：把不可变指针引用传入会写入输出参数的 C 函数，优化后可能读到空指针。上游 [gtk-rs-core#1343](https://github.com/gtk-rs/gtk-rs-core/pull/1343) 将局部指针改为可变并传入 `&mut p`，但 0.18 分支未发布该修复。

仓库保存经原始 Cargo.lock SHA-256 校验的 glib 0.18.5 发布源码，在 `vendor/glib/src/variant_iter.rs` 回移上述两行补丁。核心 `[patch.crates-io]` 将所有 0.18.5 使用统一指向本地源码，恢复现有 GTK3 类型兼容。原始版本、MIT 许可与版权声明保留，来源和退出条件维护在 [补丁说明](../vendor/glib/README.sodeck.md)。没有新增生产依赖。

`npm run glib:test` 使用独立锁文件，在 release 优化下执行 `vendor/glib/tests/variant_str_iter.rs`。CI 两个原生 Linux 核心任务复用核心 target 目录执行该回归；正常核心构建使用同一份补丁源码。

## 验证与边界

本机 Linux x86_64 容器使用原生 Rust 1.99.1、GLib 2.74.6：未修补源码的 release 回归稳定以 `SIGSEGV` 退出，回移补丁后同一回归通过。Windows 本机的类型/核心格式检查、正式前端构建和宿主脚本测试通过。两个锁文件与源码来源校验独立执行，五平台 CI 结果在 PR 中核对。

首次补丁检查中，五个平台的原生源码、两个 Linux 优化 GLib 回归、Windows UI/SSH 及 Linux 普通业务均通过；公共 `checks` 任务在 Chromium 系统依赖安装时访问 Azure Ubuntu 镜像停滞，耗尽 15 分钟时限。现有软件源修正步骤在该任务中位于浏览器安装之后，因此补充在安装之前调用同一脚本，并增加执行顺序和选择条件回归；此 CI 环境修复单独提交，后续结果仍以确切提交的 PR 检查为准。

Windows/macOS 不编译 Linux GLib 依赖；五平台源码检查和正式发行的原生实际包验收仍按[开发指南](development.md#版本与持续集成)与[分发流程](distribution.md)执行。本次修复不发行新版本。

该补丁需显式维护，不能由机器人独立升级 glib 替代。后续支持的 Tauri/GTK 栈能够使用上游已修复版本时，删除本地覆盖、重新生成锁文件并验证 Linux 原生包。GitHub 的版本匹配仍可能显示旧版本告警，不能仅凭告警消失判定安全修复完成，也不在本次操作中自动忽略或关闭告警。

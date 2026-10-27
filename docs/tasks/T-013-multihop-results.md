# T-013 r2 多级跳板实施结果

日期：2026-10-26；依据 T-013 r2 / F-CORE-014 r3 §7，用户“你做吧。”确认。验证对象为 `v2` 分支当前未提交工作区：核心 2.0.0-dev.10、API/SDK 4.6.0、终端 0.7.1、文件 0.4.1。原 r1 单跳结果保留在 `T-013-results.md`。

## 实现

- 每台服务器引用一个上游档案，核心迭代解析任意有限无环链；保存、预览、依赖和身份失效覆盖所有层。并发修改在同一事务校验循环。
- 每段通过上一台 SSH 的 TCP 转发建立下一层，终端和文件仍使用最终目标租约；中间节点不需 shell/PTY。逐节点身份确认、凭据和失败位置均有完整索引。
- 全链原子预留实际 SSH 会话/桥预算，所有桥先收到取消再由内到外回收。取消和清理不按深度累加等待时间。界面和宿主的命令等待预算随核心链长增长。
- 服务器表单支持连续添加和逐级返回；完整路径、间接依赖、每段中转指标可查看。节点与资源分页；32 KiB 状态通道只发送有界摘要和总数。
- 新 `.v2.` 路由契约独立于严格 v1。旧路由接口遇到多级路径明确拒绝；普通目标连接/PTY/SFTP/exec DTO 保持兼容。schema 2→3 自动备份，实际旧二进制拒绝新数据。

## 实测与证据

证据根目录：`.qa/t013-multihop/`。原生测试用 Windows / WebView2、独立 QEMU Debian / OpenSSH 10.0p2。A→B→C→D 使用网络命名空间和紧邻网络，每个后继的专用域名只由前驱解析；中间 sshd 的 `MaxSessions=0`。未连接业务服务器或修改日常数据。

| 验证 | 结果与证据 |
| --- | --- |
| 类型、生成契约、依赖边界 | `npm run check` 通过，`check.log` |
| SDK/插件契约 | `npm run test:contracts` 通过，含深路径等待预算及旧核心回退，`contracts.log` |
| Rust 工作区 | `node scripts/cargo.mjs test --workspace` 通过，外部夹具用例另外执行，`rust-tests.log` |
| 浏览器回归 | 51 项通过，`browser.log`；这是模拟 IPC，不能替代原生结果 |
| 直连/单跳核心回归 | `ssh_jump.rs` 11 项（含 ignored）通过，`core-all-routes.log` |
| 多级解析与协议 | `ssh_multihop.rs` 验证 1001 节点配置、原子循环检查、兼容写入、3/4/16 节点真实 SSH 协议、各段拒绝/取消、中间密钥变化、256 session 容量、50 次开关及 50 次取消，`core-multihop-final.log` 共8项通过。测试进程耗时42.08秒、CPU9.44秒、峰值260线程/193.0 MiB私有内存（不含Python夹具），见 `core-performance.json`。夹具是 Paramiko，不称为 OpenSSH |
| 64 个运行修订 | 同目标保留 64 条不同配置的四节点路径；摘要≤28 KiB，总数/当前/已变化准确，分页资源包含全部64条；清理归零，`core-state.log` |
| 真实 OpenSSH | 0/1/2/3 层终端执行；三层终端输入输出、文件 stat/SFTP/挂载 exec、cwd 跟随、新旧连接隔离、移窗/托盘、目标关闭后预算归零，`native/results.json` |
| 旧插件与无终端 | 实际装入未重编译终端 0.6.0、文件 0.3.2，经三层使用目标；另仅安装文件 0.4.1，断言无终端插件/PTY，主动读取成功，`native/old-installed.json`、`native/old-results.json`、`native/files-only-results.json` |
| 远端读取回归 | 独立 OpenSSH 直连的 SFTP、有限 exec、共享 PTY、句柄隔离及断线清理通过，`core-consumers.log` |
| Windows 界面 | 四层草稿/密码和返回焦点，逐节点 1/4～4/4 身份确认，三主题、900×600 逻辑窗口、100%/200% DPI、真实微软拼音、高对比、减少动态，`visual/results.json` 与截图。系统 DPI/对比和输入布局恢复 |
| 长路径界面 | 70 节点、64+6 分页、69 台跳板完整预览、中文长名/IPv6、无中间工作区，`visual/long-results.json`、`visual/long-path.png` |
| 迁移与回退 | 实际 r1 包生成单跳档案、信任/受保护凭据；副本升级后无需重新认证；旧包拒绝 schema3；恢复升级前备份后旧包重新连接，`migration/results.json`、`migration/downgrade.log` |

发现并修复：不存在目标误报 `INVALID_ROUTE`，恢复原 `NOT_FOUND`；外层固定命令时限无法覆盖深链，统一采用公开路径摘要预算。测试自身修正了旧主题 `mode`/新主题 ID 混用、异步依赖尚未加载就断言、工厂默认终端导致“旧包/文件独立”场景不真实，以及本机其他应用占用固定调试端口；修正后按真实包版本/无终端条件重新执行。迁移脚本改用自动分配空闲端口，未停止占用端口的其他应用。

## 交付与边界

独立入口：`.local/dev/SoDeck-v2-T013-Multihop/Start-SoDeck.cmd`，验收步骤见 `T-013-multihop-acceptance.md`。移出源码目录后启动、双插件版本、全文件散列/清单、无捆绑档案/凭据、工作区拆合、关闭退出与干净重启均通过，见 `package-smoke.json`。最终打包包含上述证据、截图和独立启动器。

运行预算是全局容量保护，配置没有固定跳数上限。1000 层仅验证配置处理；原生网络实测最多 3 层跳板，协议夹具最多 15 层。未验证 macOS/Linux 原生客户端或正式安装/签名发布；本轮未提交或推送 Git。旧 r1 测试文件删除被自动审批阻止的历史事实仍由原结果维护。

清理：专属VM PID39576正常关机，22323/22324无监听，测试App退出；远端mh_a～mh_d无残留用户进程，三个内部命名空间无已建立连接，证据 `linux/final-remote-audit.txt`。本轮递归删除专用临时数据、测试密钥和overlay的操作被自动审批拒绝（blocked by policy）；未更换工具绕过，`.qa/t013-multihop`临时文件保留且不进入包。见 `cleanup.json`。功能工程验证完成，临时文件删除项保留未完成，用户验收/正式发布仍未进行。

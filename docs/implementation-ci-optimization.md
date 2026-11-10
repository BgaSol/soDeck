# CI 优化实施与验收

日期：2026-11-10。对应 [优化计划](plan-ci-optimization.md)。本轮从 `origin/main` 的 `30d7861` 建立干净工作目录，分支 `codex/ci-optimization`；原目录的系统状态等未提交改动没有带入。

## 首批变更

- 普通 Chromium 回归迁移到 Linux `browser` job，公共格式/脚本检查不安装浏览器；正式 dist、设置和交互回归保留，组内串行。自动 Windows desktop 保留 SSH/系统专项，手动 `desktop` 保留完整 Windows 对照。
- Linux 真实业务拆成网络 x86_64、网络 ARM64、其他业务 x86_64 三行，最多 3 行并行，各自隔离资源；未减少测试入口、连接容量、真实超时或业务断言。
- 新增 `ci:test` 包装现有 npm 命令，保留退出码与实时输出，写入 suite、SHA、attempt、平台、耗时、日志和结果。服务器、工作台、正式产物和真实终端 UI 使用现有 Playwright Library tracing，失败保存、成功丢弃；其他套件先保留日志与结果。
- 修正 workspace UI 复制夹具后的 Unix 执行位，保证 Linux 可以启动真实插件夹具。原有异步移侧/尺寸等待修复保留。
- 工作流相关变化及手动 `all` 执行固定 actionlint 1.7.12；下载前后使用发行 checksum 校验。纯文档仍不安装 npm/Rust 或构建产品。
- 最终 `format` 状态显式汇总 browser，失败、取消、跳过或缺失的选中任务仍拒绝通过。诊断 Artifact 保留 14 天，实际包与发行记录仍保留 30 天。
- browser/desktop 对齐已有原生 `check-tests` 输入；integration 三行复用既有 `network-tests` 输入，没有增加按业务组区分的大缓存键。核心 `core-release-v2`、插件发行缓存与平台隔离保留；未删除远端缓存或调整付费配额。

核心/插件运行时、接口、版本和打包产物未改。五平台原生源码、核心独立构建/签名、实际包启动退出及独立插件组合验收保持原入口；本轮没有发布新版本。

## 验收

- 本机 `npm run check`、`npm run web:build` 通过。
- 本机完整 `npm run scripts:test`：98 项通过，覆盖新 browser 选择/汇总、三组矩阵、退出码/缺失可执行程序与 trace 保留规则。
- 本机 actionlint 1.7.12 的工作流语法/表达式检查通过；工具 Windows 发行包 SHA-256 与官方 checksum 一致。本机没有 ShellCheck，不能外推为 Bash 检查也通过。
- 本机 Windows `npm run ci:test -- ui:test` 完整对照通过，包括服务器 12 项、工作台 21 项、连接、分屏、交互 9 项、正式产物 13 项、桌面策略/终端输入及 settings；成功 trace 已丢弃，结果和日志已保存。
- [首轮 CI 42773155560](https://github.com/BgaSol/soDeck/actions/runs/42773155560) 在 Linux actionlint/ShellCheck 检出核心打包的两个 SC2155：`export VAR="$(find ...)"` 会掩盖赋值命令退出状态。已拆开赋值与导出，不关闭 ShellCheck；这次失败保留，修正后验证结果如下。
- [全量 Check 42773434512](https://github.com/BgaSol/soDeck/actions/runs/42773434512) 全部选中任务通过：Linux 完整 UI、两架构网络、完整业务组、Windows SSH、核心与五插件的五平台原生源码。netem 按原规则跳过。PR head 为 `af309bf0f859e7441ecbd2ad17d59527e055968f`，实际 checkout/报告为 PR merge `86c079025203f552ffca5e2480397befb690168e`；已核对二者 Git tree 相同。
- [核心包 42773476010](https://github.com/BgaSol/soDeck/actions/runs/42773476010) 在同一分支提交完成五平台原生构建/签名和实际包验收，验证修正后的 Linux/macOS 包路径导出；五个平台包及清单 Artifact 齐全。`publish=false`，没有创建或覆盖 Release。
- 实际 Playwright 故障注入生成了 trace ZIP、stderr 和 `status: failure` 结果，保留原非零退出码。CI 的 browser、Windows desktop 和三组 integration 共五份诊断 Artifact 均已上传；下载 browser ZIP 后核对 SHA-256 与 GitHub digest 相同，内部 UI/输入结果为 success，正常 UI 没有保留大 trace。

### 同步 main 后的复核与终端时序修正

同步 `origin/main` 的 `be7247d` 后，提交为 `65a84f6a92b3518143ef3a85dfa33b0d0447bfac`。[Check 42776085105](https://github.com/BgaSol/soDeck/actions/runs/42776085105) 的浏览器、Windows SSH、五平台源码和两架构网络通过，但 business 行在终端移回窗口后的退出恢复按钮上超时；该失败以及未执行的后续业务保留，汇总正确失败。

原因是测试交接夹具只等待核心提交和页面挂载，没有等待下一帧解除只读/inert，也没有等待终端收到 `surfaceReady` 后恢复输入。`.focus()` 只保证节点附着，不保证可输入，因而 `exit` 可以丢失。Windows 本机把返回窗口的交接确认事件延迟 1200ms，复现同一 30 秒超时；trace 中原 PTY 没有收到 `exit`，没有发生业务退出。

修正只涉及测试：共享交接夹具等到目标页面完成核心布局交接，真实终端测试另外等待实际收到输入恢复事件；返回窗口的事件固定延迟 800ms，覆盖该竞争。移回后继续确认输出保留、发送 `exit`、重复恢复只创建一个 PTY。终端接入失败 trace，没有缩短超时、删除断言或自动重试。修改后本机真实 OpenSSH/终端完整回归、格式/类型检查和 98 项脚本测试通过；最终提交的远端结果保留在对应 PR。

[核心包 42776080730](https://github.com/BgaSol/soDeck/actions/runs/42776080730) 在 `65a84f6` 完成全部五平台原生构建、签名及实际包启动退出，五包与清单 Artifact 齐全，`publish=false`。测试修正提交 `1a31d5b` 没有改动核心构建输入、包验收入口或核心/插件运行代码，没有为这处测试修正重复打包。

验证期间 main 又合入 Base64 和官方 opener 的变更，源码及包验收入口有变化。已在 `0af4d91` 合并 `origin/main` 的 `0abaf3e`；本轮相对 main 的差异仍限于 CI、测试工具与对应文档，没有添加生产依赖或修改组件版本。本机格式/类型、前端构建、99 项脚本测试与 actionlint 通过；针对合并后的源码重新执行 Check 和五平台实际打包，最终来源与运行结果以 PR 的交付记录为准。

### 原生验收的平台假设修正

[Check 42779505212](https://github.com/BgaSol/soDeck/actions/runs/42779505212) 的完整 UI、SSH、三业务组和四个平台源码通过；macOS 的目录测试在创建非法 UTF-8 名字时遇到 APFS 的 `EILSEQ`，汇总保留失败。[核心包 42779534067](https://github.com/BgaSol/soDeck/actions/runs/42779534067) 的两个 Linux 及 Windows x86_64 通过，macOS 在缩放后立即读到按钮中心 14pt 而非最终 22pt，Windows ARM64 的默认浏览器 UserChoice 覆盖临时 scheme handler，同时系统把测试目录的 8.3 名字展开为长路径；这次部分结果不算五平台通过。

修正限定在单元测试、隔离夹具和显式烟测入口：物理创建任意字节文件名的目录测试仅在支持它的 Linux 文件系统执行；macOS 在既有 5 秒内等待实际按钮布局，22pt 和横向留位断言不变；默认应用夹具使用真实路径，在一次性 Windows profile 内解析 UserChoice 指向的实际 ProgID，为该关联配置临时 handler，结束后恢复。完整 URL、中文/特殊字符目录各一次、非法目标和启动失败断言继续保留。本机不执行会改变默认应用的原生关联夹具；Windows/macOS 原生结果由一次性 runner 的实际包验收确认。

本机格式/类型、99 项脚本测试和 Windows 目录单元测试通过；五平台的新源码及包验收继续以最后的固定来源提交和 PR 记录为准，不使用旧失败运行的重跑来验证新代码。

main 的 `72f1cbd` / `b59e9e1` 同时修正了文件系统范围和实际默认关联。同步时直接复用这两个文件的 main 实现，保留本轮 macOS 布局等待；PR 不再携带另一套目录或 UserChoice 处理。此前冲突导致 PR 自动 Check 没有启动，解决后在固定合并提交重新验证。

### 首个成功样本的耗时

| 指标 | 既有成功 Check 42766195900 | 新 Check 42773434512 |
| --- | --- | --- |
| 工作流总时间 | 12 分 18 秒 | 8 分 08 秒 |
| 最长真实业务 job | Linux 串行组 11 分 48 秒 | business 组 7 分 37 秒 |
| Windows desktop job | 5 分 34 秒 | 2 分 39 秒，保留 SSH |
| 全部实际运行 job 时间合计 | 48 分 27 秒 | 52 分 10 秒 |

该样本总反馈时间降低约 34%，job 合计增加约 7.7%；合计时间不等于平台计费分钟。新 Linux browser job 为 5 分 20 秒，其中完整 `ui:test` 194 秒、系统依赖 52 秒、Rust 缓存恢复 18 秒、浏览器安装 27 秒。两个网络 job 均为 6 分 59 秒。核心五平台包工作流总时间为 5 分 50 秒。

这只是一次成功样本，runner、排队及缓存条件会变化；不能当作中位数、P95 或永久耗时保证。来源以各 run 的 PR/head 与实际 checkout 信息为准，后续纯文档提交不改变这批运行代码。

## 待完成与风险

新 Linux 完整 UI 已首次实际通过；更多 runner/缓存条件的稳定性仍需观察。并行增加首轮构建与 runner 等待，需积累 3～5 个可比成功样本后比较中位数、范围和总 job 时间。trace 会增加少量运行开销，CI 强制终止时可能只有未完成结果和部分日志。

缓存清理/容量核实、细粒度路径收窄、插件已发布预检、所有 UI 文件的 trace、业务内部子阶段计时及条件性的容器/有界并发尚未完成；先验收本批工作流分工，再推进有收益证据的后续步骤。

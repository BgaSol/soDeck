# T-028 实施与验证记录

日期：2026-10-29 · 规格 F-CORE-032 r1 / 代码方案 r1 · 开发版本 2.0.0-dev.20

本记录针对当前 v2 未提交工作区。用户已确认完整方案；不包含 Git 提交、推送、安装器、发布或用户验收。

## 实现

完整性清单为必需项，精确核对文件库存及 SHA-256，拒绝重复键、缺失、多余及不匹配内容。包一次读取到不可变快照，worker/UI/描述消费该快照；恢复时重新验证。签名区分未签名、已验证、开发签名、来源未验证；格式无效或已知错签拒绝。插件仍以 ZIP 格式的 `.sodeck-plugin` 资源包分发。

核心独立持久化启用选择、代次/修订及最近 128 个回执，原子替换文件。启动使用固定启用集合，停用包不执行候选初始化、worker、provider 或后台任务；保存后的选择在完整重启才生效。损坏配置全部停用且保留原文件，显式恢复先备份再重建为全停用，高版本只读。core 数据库保持 v7。

全部插件入口统一到“设置 → 插件管理”。管理列表显示当前/候选各自的版本、签名、错误及待重启状态，支持搜索、逐项勾选、统一应用/取消、冲突/回执核对和关闭协调。第三方独立编译目录可以无签名打包；开发说明见[插件开发](../plugin-development.md)。API/SDK 为 6.2，现有四插件原包保持不变。

## 已完成的验证

| 层级 | 命令/证据 | 结果 |
| --- | --- | --- |
| 静态 | `npm run check`；`.qa/t028/check.log` | 类型、生成契约、UI 及核心/插件依赖边界通过 |
| JS/TS | `npm run test:contracts`；`.qa/t028/contracts.log` | workspace 测试通过；新增启停草稿、分组保存、严格 DTO 和应用级状态订阅回归 |
| Rust | `node scripts/cargo.mjs test -p sodeck-plugin-manager -p sodeck-plugin-host -p sodeck-desktop --offline`；`.qa/t028/rust.log` | 管理器、真实宿主进程、桌面存储/工作台/关闭检查通过；原有忽略项保持忽略 |
| 管理器 | `.qa/t028/manager.log` | 30 项通过，含签名矩阵、缺失/重复/额外/错哈希、精确容量上限与链接拒绝、候选零初始化/零数据副本、快照抗磁盘修改、回执过期/损坏、不可读/备份失败保留、五个写入阶段终止子进程 |
| 观测摘要 | `.qa/t028/audit-test.log` | 记录 ID/版本/状态/数量，不记录包内容、路径、密钥 |
| 浏览器 | `npx playwright test -c playwright.ui.config.ts plugin-management.spec.ts settings.spec.ts connection-home.spec.ts`；`.qa/t028/ui-final.log` | 17 项通过；105 项列表分页、草稿搜索/导航、结果丢失及10秒超时核对、原设置及连接首页回归、键盘/大字/高对比/减少动态 |
| Windows 启停 | `tests/desktop/plugin-management.mjs`；`.qa/t028/native-latest.json` | 四个 API6.1 原包、草稿关闭、应用/重启、候选停用连续两次启动、坏包不替换、全停用空态、损坏备份/高版本只读、三主题和200%字体通过 |
| Windows 导入 | `tests/desktop/plugin-management-import.mjs`；`.qa/t028/import-latest.json` | 原生选择器取消、在途关闭/并发、外部导入刷新保留草稿、新ID默认启用、未知签名/畸形签名、过期游标/容量、真实第三方启动及选定包篡改拒绝通过 |
| 打包工具 | `tests/desktop/plugin-management-packages.mjs`；`.qa/t028/packages.json` | 官方流程保留开发签名；目录外已编译终端B无签名打包、清单逐文件核对、私钥文件未写、冲突参数拒绝通过 |
| Windows + 真实 Linux | `tests/desktop/plugin-management-updates.mjs`；`.qa/t028/business-latest.json` | Debian/OpenSSH PTY、文件/SFTP、监控采样、HTTP/SOCKS5/固定转发，启停/恢复/重启/独立更新及资源释放通过 |
| 最终目录 | `tests/desktop/plugin-management-package.mjs`；`.qa/t028/delivery-latest.json` | 普通EXE与Start脚本启动、四原包/版本/能力/管理页、旧API6.1核心v7数据/加密凭据/插件设置副本保持、正常退出通过 |

真实 Linux 使用本任务新建 `.qa/t028/linux.qcow2` 写入层，已停止的 T-027 测试盘仅作 backing；管理与测试仅 `127.0.0.1:22383`，未访问用户日常服务器。测试专用 A/B 终端均为真实 UI/worker：0.13.1 → 0.13.2，版本可在管理页核对，字号18跨更新保留；核心 exe 与另外三个原包不变。文件单独连接、监控仅自己启用时先等待连接再主动连接、不加载终端；隧道配置跨停用保存，退出释放监听。业务测试中故障终止终端 worker 后，本次仍可恢复，符合启动启用集合。

发现并修复：SDK 的应用级对象映射漏接插件管理 feed，原生外部导入后未刷新；已补映射及订阅单测并原生复验。统一保存重构后错误字段在禁用期间聚焦无效，改为恢复可编辑后聚焦，原设置回归通过。测试中的初始化/文件读取需等待真实 ready 状态，补足等待后保持原业务断言。

交付前按代码方案§5的10秒约束修正启停保存超时，补充提交与回执查询均无响应的浏览器测试：10秒显示结果待确认，保留草稿并阻止重复提交；恢复查询后用原操作回执核对。属于已确认范围内的实现修复。

兼容测试使用旧API6.1执行文件在隔离目录新建离线服务器、加密凭据和字号设置，再复制给新核心；数据库按表行摘要与schema比较，源DB/WAL和加密文件保持字节相同。SQLite只读备份仍会更新临时SHM读标记，因此SHM不作为持久数据哈希比较项；不放宽业务行、WAL、凭据和插件设置断言。

PM-03/14收尾补测：在现有超限、损坏与原子写失败之外，验证32MiB压缩包、128MiB展开和2000项精确上限，ZIP链接拒绝；Windows独占文件句柄模拟不可读，并在测试专用临时目录暂时禁止创建文件以验证恢复备份失败。权限在测试结束恢复，仅操作测试目录。

## 交付检查

独立目录已生成：`.local/dev/SoDeck-v2-T028-r1/Start-SoDeck.cmd`，开发版本2.0.0-dev.20，API/SDK6.2，core数据库v7。目录不自动导入日常数据，测试无签名夹具不随包交付。随附终端0.13.1、文件0.10.0、网络隧道0.6.0、资源监控0.3.0原签名包，哈希与T-027-r9完全一致。

构建命令：`npm run build --workspace @sodeck/desktop`、`node scripts/cargo.mjs build -p sodeck-desktop -p sodeck-plugin-host --features sodeck-desktop/custom-protocol --offline`、`node scripts/package-plugin-management-dev.mjs`。构建成功；现有第三方库/旧模块警告及缺少OpenSSL调试PDB的链接警告保留在`.qa/t028/build-rust.log`，未阻止生成或运行。

最终普通启动验证目录`.qa/t028/delivery-1793224497479`。EXE SHA-256：`7299ef91bfe2948cc8b201d7ca78436f5c1d007f8050bc2c4a1fbf971243ec5f`。全文件清单见交付目录build.json；证据在evidence中，前序业务结果保留当时的二进制哈希，最终目录检查单独记录最终哈希。普通启动只初始化空数据，未创建测试服务器。旧核心兼容数据位于QA副本，未进入交付data。

最终原生截图：

![插件管理](../../.qa/t028/delivery-1793224497479/plugin-management.png)

测试App均已退出；专用QEMU进程22372已通过guest `systemctl poweroff`正常关闭，loopback22383监听已释放，测试盘和证据保留。没有访问或修改用户日常服务器及数据。

验证边界：浏览器测试不等于原生；原生使用 Windows/WebView2 和真实 worker。Linux 业务使用隔离 Debian/QEMU/OpenSSH。200%字体与浏览器模拟高对比不代表全部系统DPI/输入法组合均有专项实测。没有安装器验证、macOS/Linux桌面验证或用户验收；未恢复此前取消的监控长压测。完整性不等于发布者身份认证；旧核心不识别启停文件，回退须使用升级前独立目录或备份。

## 用户验收步骤

1. 启动目录包，打开设置的插件管理，确认四个插件都有启用复选框。
2. 取消终端勾选并应用，确认当次终端仍可用且显示待重启；应用菜单退出后重开，终端不加载。
3. 将终端重新勾选并应用，完整重启，确认可重新使用；包和设置保留。
4. 如需第三方试用，按开发说明打包并导入；未签名提示应清楚，损坏包应被拒绝。
5. 可将全部插件停用并重启，检查服务器与设置管理仍可操作。

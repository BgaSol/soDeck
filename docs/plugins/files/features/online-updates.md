# F-FILES-005：文件插件独立发布与在线升级接入

文档就绪：可评审 · 修订：r2 · 日期：2026-10-29
所属：[文件插件](../README.md) · 任务：[T-029](../../../tasks/T-029-online-updates.md)
公共行为：[F-CORE-033](../../../core/online-updates.md) · [代码方案](../../../technical/online-updates-implementation.md)

GitHub来源、目录、下载和安装由核心管理，文件插件继续拥有目录/SFTP/跟随/容量行为。官方CI独立生成.sodeck-plugin，核心和其他插件不因文件升级而变版本。已下载不代表已安装；用户安装后完整重启才激活，签名/启停遵循T-028。

代码核对：plugins/files/manifest.json、package.json、vite.config.ts、src/worker.ts、state.ts、view.tsx。预计无业务改动；流水线跟踪本插件及SDK/UI实际输入，变化必须提升本插件版本。测试使用真实UI/worker A/B，不在核心加入文件专用升级逻辑。

r2代码落点：公共release组件表登记`sodeck.files`、UI/worker/diagnostics输入，显式注入并核对manifest/package/UI/worker版本；共享依赖按实际导入闭包判断。文件插件不新增联网检查或核心私有依赖，来源/安装/恢复由[代码r2](../../../technical/online-updates-implementation.md)统一处理。

| 用例 | 准备与操作 | 预期 |
| --- | --- | --- |
| FILES-OU-01 | 核心及其他包固定，真实SFTP目录/容量；下载文件B，暂不安装，再安装并重启 | 下载不改变当前读取和跟随；B读取/导航正确，仅文件包变化 |
| FILES-OU-02 | 终端停用，文件独立连接；准备更新后退出/重开 | 原连接/句柄释放，新版可独立连，无终端依赖，设置/插件数据保持 |
| FILES-OU-03 | 文件停用时在线安装候选；坏哈希/初始化失败/更高核心依赖 | 停用不运行SFTP/provider；坏包拒绝，失败可回退，未来核心候选不提前执行 |
| FILES-OU-04 | 仅文件插件改版；再改变其已使用的公共UI与未使用SDK导出，执行规划和包验证 | 单插件升级只产生文件包；已使用共享输入变化要求对应组件加版，未使用导出不强制文件改版；保留核心/其他包哈希 |

隔离.qa/t029/专用Linux，退出核对SFTP/远端读取/租约/订阅与worker释放，保留哈希与真实读取证据。当前未执行，结果由T-029记录。

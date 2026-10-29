# F-TERM-007：终端独立发布与在线升级接入

文档就绪：可评审 · 修订：r2 · 日期：2026-10-29
所属：[终端插件](../README.md) · 任务：[T-029](../../../tasks/T-029-online-updates.md)
公共行为：[F-CORE-033](../../../core/online-updates.md) · [代码方案](../../../technical/online-updates-implementation.md)

核心拥有GitHub发现/下载/安装、来源与启停；终端不自己检查GitHub、不替换自己的运行文件。继续使用完整性必需/签名可选的包格式、独立版本和重启激活。官方CI用发布密钥打包，手动未签名包保持可加载。仅下载不写pending；用户点击安装才准备新终端，当前PTY仍用旧worker/UI。

代码核对：plugins/terminal/manifest.json、package.json、vite.config.ts、src/worker.ts、terminal-view.tsx和settings.ts。预计无业务逻辑变更；发布输入/独立版本和构建脚本由公共流水线管理，只有终端或其实际构建依赖变化才提升终端版本。线上真实A/B验证在测试副本更改版本及可识别的UI/worker内容，不用空worker代替终端。

r2代码落点：公共release组件表登记`sodeck.terminal`、UI/worker构建入口及settings/diagnostics资源；manifest/package版本、打包注入版本、真实UI/worker版本必须一致。Vite当前默认版本回退不作为发布依据，由发布脚本显式传入并校验。对已安装终端的在线操作全部留在核心，终端不引入下载器或更新设置；包内只纳入实际使用的SDK/UI依赖。完整性/来源/安装协调由[代码r2](../../../technical/online-updates-implementation.md)维护。

| 用例 | 准备与操作 | 预期 |
| --- | --- | --- |
| TERM-OU-01 | 固定核心及其他三包，真实Linux终端A；在线检查、下载B但不安装，重启一次；再点安装并重启 | 只下载仍为A；安装后UI/worker为B，core/其他包哈希不变，PTY可输入输出 |
| TERM-OU-02 | A保存字号/Shell偏好，多终端、弹出窗口和草稿；下载B，取消一次安装退出审查，再放行 | 本次会话未被下载/取消打断；放行后PTY释放，B保留设置；Windows保持普通Shell行为 |
| TERM-OU-03 | 停用终端后下载安装候选；再验证只兼容新核心候选与坏候选 | 停用无provider/PTY/目录报告，未来核心包不提前跑，失败保留旧版/诊断 |
| TERM-OU-04 | 仅终端改版且根锁文件随版本变更，其他输入不变；发布规划、签名包后检查描述/设置资源与版本 | 核心/三插件原资产哈希保持；终端manifest、UI、worker版本一致，设置资源完整；漏加版/注入旧版本构建拒绝 |

隔离.qa/t029与测试SSH；关闭会话/窗口并退出，核对PTY/租约/worker释放，保留版本、设置与包哈希。当前均为待实施流程，结果由T-029记录，不改T-028结果。

# F-PROXY-006：网络隧道独立发布与在线升级接入

文档就绪：可评审 · 修订：r2 · 日期：2026-10-29
所属：[网络隧道插件](../README.md) · 任务：[T-029](../../../tasks/T-029-online-updates.md)
公共行为：[F-CORE-033](../../../core/online-updates.md) · [代码方案](../../../technical/online-updates-implementation.md)

核心负责公开GitHub请求与升级缓存；网络隧道插件不代理更新器下载、不增加发布器或安装权限。规则、HTTP/SOCKS与转发业务归本插件，只有其版本变化才替换对应包，当前已运行规则不因下载停止。正式安装需正常退出资源释放，重启后保留规则，沿用既定重启行为而不承诺会话续接。

代码核对：plugins/network-proxy/manifest.json、package.json、vite.config.ts、src/worker.ts、manager.ts、run.ts、view.tsx。预计不改业务；公共流水线按真实构建输入验证版本，不把核心更新UI导入插件。

r2代码落点：公共release组件表以`sodeck.network-proxy`关联network-proxy构建目录，核对ID、manifest/package/实际UI/worker版本与diagnostics资源；不使用中文显示名或目录名猜更新身份。无需给worker增加更新命令；包来源/候选/安装错误由[代码r2](../../../technical/online-updates-implementation.md)的公共服务处理。

| 用例 | 准备与操作 | 预期 |
| --- | --- | --- |
| PROXY-OU-01 | 核心与其余包固定，HTTP/SOCKS/固定转发均有真实请求；下载B | 三条流保持，未安装版本不切换；再安装重启后仅此包为B，规则配置保持 |
| PROXY-OU-02 | 有规则和设置草稿，点核心安装再取消退出审查；随后放行 | 取消不启动安装器/停止隧道；放行后全部监听/网络租约释放，避免新旧进程抢端口 |
| PROXY-OU-03 | 停用代理后下载安装候选、核心失败、候选初始化失败 | 停用无监听/自动重连；未来核心包不提前跑，失败保留旧规则和明确状态 |
| PROXY-OU-04 | 仅网络隧道改版；同ID手动导入未签名改版后检查官方更新，再恢复官方跟随 | 独立发布仅本包变化；本地来源不被替换，恢复跟随不隐式下载/安装，规则与监听按原生命周期保持；记录摘要后清理测试规则 |

专用Linux和loopback测试端口，无日常代理；结束关闭规则、工作区及App，检查监听/转发/worker归零，保留包哈希和成功请求证据。当前未执行，结果归T-029。

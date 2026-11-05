# 网络隧道实施与验收

本轮新增独立插件 `sodeck.network-proxy` 0.1.0。核心版本尚未提升，没有创建标签或 Release，官方列表已有来源，但正式发布插件前无法从 GitHub 下载。

## 代码边界

- `src-tauri/src/network/`：通用本机监听、当前服务器远端 TCP、有限协商、原生双向复制、取消和有效载荷计量。共享现有 SSH/SOCKS5 混合连接链，不另存凭据。
- `resource_owner.rs`：提取终端已有的资源归属逻辑，终端与文件继续使用相同校验。
- `plugins/network-proxy/`：规则文件、修订冲突、后台编排、SOCKS5 no-auth CONNECT、实际入口检查、列表/详情和编辑弹窗。
- `component_ui/` 与 `src/components.ts`：通用 dialog 视图容器，复用现有关闭协调、移窗和挂载流程。
- `crates/plugin-client`：可克隆的并发客户端，共享有界在途请求；旧插件的调用入口保留。

没有新增网络功能的生产依赖。规则位于插件自己的 `network-rules.json`，schema 1；最多 256 条、512 KiB，写临时文件并同步后替换。坏文件及不支持的 schema 保留原文件并提示错误。运行状态不持久化。

## 可验收行为

已连接工作台默认收起网络侧栏。创建规则、保存或保存并启动；固定转发与 SOCKS5 分开呈现。默认回环地址和自动端口，启动成功后复制实际入口。支持手动绑定其他本机 IP、固定端口与冲突错误。

普通保存保留旧运行配置；保存并重新启动先停止旧监听，确认结束后使用新配置。运行中删除先停止再落盘；落盘失败保留规则。收起侧栏、取消工作区关闭和移窗继续原端口。断线、关闭工作区、插件退出和应用退出清理端口，重开后手动启动。

规则显示连接数、约 3 秒速率和本次运行累计有效载荷。失败的客户端记录普通日志，并在详情展示最近错误；监听失败与服务器断线单独显示。重要中断消息按服务器去重，普通错误不会自动弹通知。

检查实际本地入口并确认远端 TCP 结果；SOCKS5 完成协议协商。检查只代表 TCP 可达。目标域名由最终服务器解析，客户端应选择代理解析（例如 `socks5h://`）。不提供 HTTP、UDP、反向转发、自动重连和系统代理。

## 验证入口

```sh
npm run check
npm run scripts:test
npm run test
cargo test --manifest-path plugins/network-proxy/Cargo.toml --locked
npm run network:test
npm run terminal:test
npm run files:test
npm run web:build
node scripts/package-plugin.mjs --plugin network-proxy
npm run build -- --bundles nsis -- --locked
```

`network:test` 运行真实外置插件，分别通过测试 SSH/SOCKS5 混合链和隔离 OpenSSH 验证双向大块正文、半关闭、分片握手与粘连正文、远端 DNS、精确计量、检查、收起、取消关闭、重挂载和移窗、运行中编辑和删除、断线清理，以及插件正常退出和被结束进程后的端口/活动客户端清理。Playwright 加载真实核心与插件 iframe，验证三主题、小窗滚动、未保存关闭、重挂载继续使用同一入口。

`Verify native remote plugins` 增加可选 `network_run_id`。不填时仍按既有终端/文件流程；填入网络插件的打包运行 ID 后，在五个原生 runner 安装/解包核心并加载独立插件，挂载真实侧栏和弹窗，经隔离 OpenSSH 执行实际 TCP/SOCKS5 往返、移窗后复用和断线端口清理。重开时复用保存规则的修订号。

包内验收另写 `network-smoke-result.json`；脚本每次启动前清除旧报告，必须收到本次网络业务完成结果。旧核心忽略网络插件测试变量时不能凭通用 SSH 报告误判通过。

## Windows x86_64 实测（2026-11-05）

- 类型/核心格式检查、11 项脚本测试、60 项普通 Rust 测试和前端正式构建通过。5 个需要真实外置环境的 Rust 用例由各自入口另外执行。
- 网络插件 5 项测试通过：SOCKS5 分片域名、IPv4/IPv6、拒绝认证/UDP、规则修订与损坏文件、分页删除后的回退及检查结果归属。
- `network:test`、`terminal:test`、`files:test` 通过，均使用实际独立插件和真实连接；终端与文件的页面及业务回归通过。
- 核心 NSIS 和独立网络插件原生包生成成功，插件入口架构和静态运行库检查通过。未覆盖安装本机客户端；从 NSIS 提取实际包内程序到独立测试目录。
- 实际包同时加载终端、文件和网络三个外置插件，经隔离 OpenSSH 完成各自业务、原生界面挂载、窗口移出/移回、断线清理和重开退出；网络报告的 TCP/SOCKS5、移窗和释放标志全部为 true。

回归中复现并修正了“保存并重启规则”与关闭编辑弹窗之间的竞态：关闭协调暂时冻结工作区时，保留待重启操作，待协调完成后继续；真正销毁管理视图时仍取消运行。已纳入真实混合链和 OpenSSH 回归。

## 当前交付限制

本机验证仅代表 Windows x86_64。Windows ARM64、Linux x86_64/ARM64 和 macOS ARM64 的实际包业务验收尚未执行；已提供原生工作流接线。源码检查、原生编译和实际包业务验证分别记录，不能互相替代。发布前仍需运行五目标验收，再分别发布支持能力的核心与插件。

重复包内验收时曾出现一次终端插件初始化等待超时，日志停止在进程启动阶段；相同核心、插件及保存数据重试后通过，尚未稳定复现，未提高产品初始化超时来掩盖它。验收脚本另已修复重试时保留旧 `smoke-error.txt` 的问题，并增加回归测试。Windows WebView 退出偶有 `Chrome_WidgetWin_0` 注销提示，程序和插件均正常退出。

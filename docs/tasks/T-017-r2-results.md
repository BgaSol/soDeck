# T-017 r2 实现与验证记录

日期：2026-10-27。用户否定上一版网络UI，指定学习文件部分的边框、风格和底部并继续修改，随后追加“左侧少了边框”。本次依据任务r2直接实施，r1验证保留为历史，不能视为用户视觉接受。源码保留在v2未提交工作区，未提交、推送或发布。

版本：应用 **2.0.0-dev.14**、公共UI **1.3.1**、网络插件 **0.2.2**。API/SDK4.8.0、终端0.8.0、文件0.5.0、数据库v4保持；终端和文件仍使用原签名包。

## 返修结果

- 网络面板按文件的实际渲染样式统一：连续全宽行、普通16px图标、整行hover/选中底色和选中轮廓，无图标色块、无逐条淡分隔。名称和入口/目标按两行组织，长内容换行，常规条目56px。
- 顶部使用同一公共Toolbar；底栏直接使用文件的StatusBar，三主题的背景、上边框、字体、24px基准高度及左右留白逐项比对一致。左侧数量/运行反馈、右侧插件版本；保持独立滚动。
- 详情取消大图标身份摘要，收紧间距，入口框采用与控件相同的明确描边；业务、创建表单和身份信任语义保持。
- 右侧边框根因是宿主所有sessions-panel都固定画border-right。现按实际停靠边画朝向中央内容的一侧：文件右边、网络左边，同样1px主题色；原生表面从边框内开始测量，防止覆盖。移动后自动切换，不给插件中央页或独立窗追加侧边框。
- 三主题比对发现文件CSS的surface背景被SDK设置的canvas背景覆盖，因此按实际渲染对齐canvas，保留正确的视觉比对预期。

## 检查与证据

环境：Windows、原生WebView2、Playwright Chromium；独立测试数据、本机三节点Paramiko协议夹具。未连接用户192.168.1.16，未修改T015/T017运行包或其data。证据根目录 `.local/test-results/t017-r2/`。

| 检查 | 结果与证据 |
| --- | --- |
| 基础与构建 | check、contracts、前端build、签名网络包通过：`check.log`、`check-final.log`、`contracts.log`、`build.log`、`network-pack-final.log`；离线release/debug构建通过，记录为`release-build-final.log`、`debug-build-final.log`；git diff --check通过 |
| 文件/网络/公共UI | 21项既有界面检查通过；首次3项样式比对发现背景不一致，修正后3项通过。记录为`ui-verified.log`、`visual-match-final.log`。覆盖保存回执/草稿、列表滚动、复制、窄窗/200%文字、文件虚拟树和设计系统 |
| 视觉及分隔边框 | 三主题文件/网络工作面、选中轮廓、底栏实际样式比较，以及左右停靠边界/原生内容几何追加检查6项通过，与上方21项合计27项独立用例。最终记录`borders-visual-final.log`。截图为`files-reference-*`、`network-matched-*`、`detail-matched-*`、`dock-borders-*` |
| 原生网络 | 创建、三跳身份确认、HTTP/SOCKS5、固定转发、独立停止、隐藏/独立窗/中央/右侧移动、关闭清理通过。追加原生右侧边框几何与窗口截图通过，记录`native-final.log`、`network-native-1793059504877/right-panel-border.png` |
| 原生插件更新 | 0.2.1准备0.2.2时旧版本继续运行，重启启用新列表；原规则ID/固定端口保留，不自动启动：`upgrade.log`、`ui-upgrade-latest.json` |
| 独立包启动 | 中文路径、不相关cwd、残留开发变量、无命令窗口、三个插件、托盘、重复实例、缺失资源、损坏数据库提示与退出清理通过：`startup-shell.log`、`startup-shell-1793059577430/results.json` |

交付入口：`.local/dev/SoDeck-v2-T017-r2/sodeck-desktop.exe`，ZIP同目录。最终包采用明确白名单，带README/RESULTS/build.json，不带测试或日常data/私钥；签名使用本地开发密钥。最终16文件清单、ZIP、签名与条目哈希、版本、PE GUI子系统及被测副本运行文件一致性记录在`package-integrity.json`。最终包保持未启动状态，测试清理记录在`cleanup-final.json`。工程检查已完成，进入用户视觉验收。

限制：原生网络使用真实socket与Paramiko，不是本轮Linux/Windows OpenSSH重测；没有改变数据面。多显示器及完整Windows DPI/IME矩阵没有重跑，200%文字与窄窗边界来自浏览器。用户视觉满意度待确认，目录包不等于安装器或正式发布。初次抽取浏览器夹具遗漏expect导入已修正，失败日志保留；npm离线锁更新补齐宿主UI依赖版本后通过。

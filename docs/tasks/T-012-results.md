# T-012 实施与验证

日期：2026-10-26 · 未提交 `v2` 工作区 · 核心2.0.0-dev.9、API/SDK4.5.0、UI1.1.0、终端0.7.0、文件0.4.0。

实施依据：T-012 r3和代码方案r1已获“可以开始做了”确认；随后用户明确要求取消多巴胺阴影、修正顶部留白及黑线，对应视觉r3/代码r2。保留工作区T-013跳板及OpenWrt文件修复。没有创建分支、提交、推送或安装器。

## 已实现

- `themes/catalog.json`默认多巴胺；三套色板及补充表面色独立维护。公共UI的foundation/profile统一生成CSS/TS，生成漂移、非法值、插件样式边界均纳入check。插件保留虚拟布局、xterm适配和业务组合，通用控件由公开UI提供。
- 外壳、终端、文件迁移到公共控件；三套主题可切换。多巴胺取消按钮、菜单、弹窗、拖动反馈和Logo投影；顶部单分界、Tab边线与面板标题共用基线。浅/深色保留柔和材质。
- schema1 light/dark备份后映射到schema2 themeId，保存失败不改变已提交主题。旧静态/旧实时插件分别显示兼容信息；新能力和旧接口形状并存。
- 应用入口改为图标、文字、箭头，宿主持有透明输入屏障和可滚动菜单。菜单不隐藏插件、不增加主题订阅、不进入任务栏/Alt+Tab工具窗口列表、不全局置顶；完整消费取消的鼠标手势，继承宿主字号。
- Pocket Deck母版、微型、单色、PNG/ICO和真实窗口/托盘资源。缺失测试图标回退到内置命令图标。任务栏截图与从EXE、三个窗口提取的图标已核对。

## 实测

Windows原生Tauri/WebView2；真实SSH/SFTP仅为专属QEMU Linux `127.0.0.1:22281`，不使用业务服务器。浏览器层使用模拟IPC，明确与原生结果分开。

| 检查 | 结果及证据（工作区相对路径） |
| --- | --- |
| 参数/类型/依赖/样式 | `npm run check`通过；`.qa/t012/check.log`。参数与边界负例通过，`.qa/t012/boundaries.log`；公开UI/旧入口最小构建在`.qa/t012/minimal-plugin` |
| 契约 | `npm run test:contracts`通过；`.qa/t012/contracts.log` |
| Rust | workspace测试通过；外部夹具的既有ignored不计通过。最终桌面36项通过；`.qa/t012/rust.log`、`rust-desktop-final.log` |
| 浏览器 | 全量43项通过，随后主题作用域修正相关9项（含新增嵌套主题测试）通过，含真实公共组件目录、三主题对比度、无投影、焦点/IME、文件树和终端业务；`.qa/t012/browser.log`、`ui/contrast.json` |
| 三窗主题 | 3窗口8表面、30次同步；200次连续切换及10秒停读消费者；PTY/连接/DOM/路径草稿保持。p95为5.04ms，见`.qa/t012/native/results.json`（门槛300ms） |
| 菜单压力 | 三窗交替200次，资源/订阅回基线；伪造token和popup任意RPC被拒绝；`.qa/t012/native/results.json` |
| 原生视觉和输入 | 三主题×实际100/150/200%DPI×1280×820/760×480；20px文字、减少动态；Microsoft Pinyin真实组合输入跨主题保持；实际系统高对比覆盖并恢复；`.qa/t012/visual/results.json`及截图 |
| 原生菜单 | 可见终端序号继续推进；Escape、外部完整点击、同入口点击均不透传；另菜单/主题取消；owned/toolwindow非topmost属性断言通过；`.qa/t012/visual.log`；补充Enter/Space/Down、管理操作、收起/最小化/移动/销毁/退出通过，`menu-lifecycle/results.json` |
| 兼容与故障 | 4种新旧静态插件组合、旧live、浅/深schema1迁移与重复启动、损坏偏好/主题、品牌fallback、菜单创建失败、旧核心拒绝新包均通过；`.qa/t012/compatibility/results.json` |
| 生命周期 | 三轮6表面创建/换色/合并/关闭，回到一条主题订阅耗时13.8、14.4、16.6ms，PTY/租约/连接归零；`.qa/t012/lifecycle/results.json` |
| 资源完整性 | PNG10尺寸、ICO9帧、manifest哈希和EXE/原生窗口提取；`.qa/t012/brand/`。T010/T011/T013旧包共143个已声明文件哈希全部保持，`old-package-integrity.json` |

修复记录：独立终端窗口不属于工作区window表，曾被应用菜单来源校验误拒；已按有效detached视图核验。Tao显示窗口会重设样式，toolwindow标志改为显示后在UI线程设置；shown只在配置完成后置位，失焦通知重新确认当前焦点，最终三窗200次无重试通过。原生Destroyed后直接移除主题owner，修复第二轮关闭残留订阅。错误预期未放宽。

## 交付与未验证范围

独立开发包位置：`.local/dev/SoDeck-v2-T012/`，入口`Start-SoDeck.cmd`；目录包复制到带空格的新路径的启动、版本、独立数据、换色持久化、拆合与退出已通过，由`package-smoke.json`记录。包内容与源码指纹见`build.json`。

当前单显示器；混合DPI双屏无法实测。Alt+Tab专门截图、Windows浅色任务栏及屏幕阅读器语音朗读未完成，提取原生图标与DOM语义不代替这些测试。品牌原生验证已覆盖当前深色任务栏、EXE和三窗口，不声称全系统组合通过。正式安装器、签名发布、跨平台不在本次范围。

视觉/主流程可试用，但上述原生验收矩阵仍有缺项，任务保留开发中并明确环境限制；用户验收另记。系统缩放/高对比恢复本次捕获值（200%、flags126），输入法恢复；隔离App与VM清理见`.qa/t012/cleanup.json`。

清理实测：本轮App数量0、22281和所用桌面调试端口无监听；QEMU PID20792已通过guest poweroff正常退出，磁盘保留。未改动旧开发包，未使用用户服务器。最后补入结果/清理证据仅更新交付文档，最终二进制/插件哈希再次与搬迁启动测试核对。

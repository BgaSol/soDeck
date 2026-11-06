# 桌面一致性实施记录

2026-11-06，源码实现；尚未发行或替换用户安装版。整体目标和依赖选型见 [实施规划](plan-desktop-consistency.md)。

## 用户可见的变化

核心、文件和网络页面使用三主题共享滚动条：普通轨道 10px，可见滑块 6px，圆角、透明轨道，悬停和拖动时增强颜色。文件树移除单独的宽度规则，网络页面补齐共享样式。两级标签条及工作台入口轨道保留隐藏滚动条的设计。

终端使用 xterm 6 的公开 `scrollbarSliderBackground`、`scrollbarSliderHoverBackground` 和 `scrollbarSliderActiveBackground`，以透明边距形成相同的 6px 可见滑块。保留 xterm 原有 14px 拖动区域，避免更改终端布局、缓冲区滚动和列数；主题切换保留选区、滚动位置及行列数。原生 DOM 滚动条在强制颜色模式回退到系统绘制。

浏览器默认行为统一由核心管理，包含主窗口、拆出窗口和插件 iframe。取消默认右键菜单、刷新、网页查找、打印、打开网页文件、保存网页、查看源码、页面缩放、历史导航和开发者工具；补齐 F12、Windows Control 组合与 macOS Command 组合。输入框编辑快捷键、Tab/Shift+Tab、输入法、业务右键及终端控制键继续有效。

核心与文件、网络页面提供应用自己的文本菜单，复用现有原生剪贴板接口：剪切、复制、粘贴、全选，以及日志选区复制。密码、只读和禁用字段按实际状态限制菜单，显示菜单时保留选区，剪切和粘贴保留编辑撤销。异步剪贴板返回前切换焦点、关闭视图或改变上下文，不会继续修改旧目标。终端继续使用其原有菜单和鼠标跟踪约定。

## 实现与依赖

- 新增并固定 `tauri-plugin-prevent-default = "=6.0.1"`，MIT；选型时核对上游 2026-10-05 发布的 6.0.1 及公开源码。用途是复用快捷键脚本生成和 Windows WebView2 原生选项，随核心打包。它不是 SoDeck 业务插件。
- `desktop_behavior.rs` 排除上游 `FOCUS_MOVE`，避免取消 Shift+Tab；复用 `Plugin::initialization_script` 与 `webview_created`，通过 Tauri `js_init_script_on_all_frames` 覆盖 WebKit 子 frame。补充策略只取消默认动作，不停止业务事件传播。
- Windows 原生设置关闭浏览器 accelerators、默认菜单、devtools、缩放、捏合缩放及导航手势；Tauri 配置和动态窗口另明确关闭 devtools。普通开发运行也使用这一策略。
- 滚动条颜色由 `themes.css` 定义，经 `theme.ts` 增量发送；共享样式及文本菜单由现有插件打包入口复制，核心与插件继续独立发行，未改应用或插件版本。

替代方案是核心自行维护全部脚本与原生适配。上游插件已有这部分实现，复用公开 API 的成本更低。OverlayScrollbars 2.17.0 经评估未引入：需要逐容器初始化及销毁，仍不能接管 xterm；当前共享 CSS 和 xterm 自身接口已覆盖实际用例。具体来源见 [prevent-default 6.0.1](https://github.com/ferreira-tb/tauri-plugin-prevent-default/tree/6.0.1)。

## 验证

`npm run desktop:test` 使用实际编译的 Tauri 插件导出初始化脚本及 all-frames 元数据，验证主页面和子 frame 的快捷键取消、Shift+Tab、文本编辑/撤销、本地菜单、密码/只读限制、过期剪贴板、弹窗菜单和三主题纵横滚动。随后加载同一策略验证打包的 xterm 输入、粘贴、鼠标模式、主题几何和滑块形态。

Windows x86_64 已构建 NSIS 安装包，隔离烟测通过启动、移窗、关闭、资料保存、重开及退出；烟测读取主窗口与拆出窗口的实际 WebView2 设置，确认默认菜单、开发者工具和缩放关闭。退出日志出现 WebView2 `Failed to unregister class ... 1412`，未使烟测失败。

| 检查 | 结果 |
| --- | --- |
| `npm run check` / `npm run plugin:check` | TypeScript 与核心/三个插件的 Rust 格式检查通过 |
| `npm run scripts:test` | 13 项通过 |
| `npm run test` | 提交前复核 89 项通过；5 项依赖真实 OpenSSH/插件包的测试按入口设计忽略，相关插件测试另行执行 |
| `npm run ui:test` | 服务器、工作区、连接、正式前端产物、桌面行为/终端输入及组件设置的完整页面回归通过，无页面错误 |
| `npm run terminal:test` | 独立打包与真实 OpenSSH PTY、xterm 输入输出、多 Tab 和重挂载恢复通过 |
| `npm run files:test` | 独立打包、真实 OpenSSH/SFTP 及实际文件界面回归通过 |
| `npm run network:test` | 独立打包、本机网络/真实 OpenSSH 转发及实际网络界面回归通过 |
| `npm run build -- --bundles nsis -- --locked` | Windows x86_64 NSIS 安装包构建通过 |
| `node scripts/smoke.mjs src-tauri/target/release/sodeck.exe` | 实际核心程序启动、移窗、持久化重开和退出通过；主/子窗口的原生浏览器设置验证通过 |

完整回归首次发现工作区夹具未提供 Tauri 原生事件回调，已补齐夹具的事件注册/注销接口，再运行整套 UI 测试通过。并行任务修复其 Windows 测试程序的 Common Controls manifest 后，完整 Rust 测试也已重新通过；早期 DLL/重复 manifest 失败不作为最终验证结果。

## 平台与交付边界

本轮尚未执行 Windows ARM64、Linux x86_64/ARM64、macOS ARM64 的原生包和实际交互验收。源码的 all-frames 路径及 macOS 配置解析通过回归，不等同于这些系统运行验证；触控板手势、混合 DPI 和辅助技术仍需实际设备验收。

Chromium 测试夹具存在一项可复现的焦点边界：关闭原生 HTML 弹窗后，直接对无编辑焦点的 iframe 区域发送滚轮，可能收到未取消的事件而不滚动。改动前的 HEAD 样式、不加载新策略或文本菜单时也能复现；重新进入 iframe 输入框后消失。正式回归不再通过重载页面跳过弹窗场景，而是验证重新进入插件输入框后，继续进行滚轮、PageDown 与横向滚动。该被动滚轮边界未在实际 WebView2/WebKit 设备上确认，不能描述为已修复。

当前仓库同时包含组件设置和在线更新改动，按用户要求一并提交。本记录仅描述桌面行为、共享文本菜单和滚动条的范围；尚未发布或替换用户安装版。

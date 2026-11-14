---
name: "SoDeck 核心界面"
description: "面向运维的紧凑服务器会话工作台；多巴胺与 IDEA 明暗三主题共用核心几何。"
colors:
  light-canvas: "#ffffff"
  light-subtle: "#f7f8fa"
  light-text: "#27282e"
  light-muted: "#5a5d6b"
  light-border: "#dfe1e5"
  light-accent: "#3574f0"
  light-accent-bg: "#d4e2ff"
  light-hover: "#ebecf0"
  light-good: "#1e6b33"
  light-warning: "#a46704"
  light-error: "#ad2b38"
  light-primary: "#315fbd"
  light-primary-hover: "#2c559f"
  light-pressed: "#c9dbfc"
  light-success-bg: "#e3f7e7"
  light-error-bg: "#ffebec"
  light-scrim: "#18203438"
  dark-canvas: "#1e1f22"
  dark-panel: "#2b2d30"
  dark-text: "#dfe1e5"
  dark-muted: "#b4b8bf"
  dark-border: "#4e5157"
  dark-accent: "#6b9bfa"
  dark-accent-bg: "#2e436e"
  dark-hover: "#393b40"
  dark-good: "#89cc8e"
  dark-warning: "#f2c55c"
  dark-error: "#eb938d"
  dark-primary: "#315fbd"
  dark-primary-text: "#ffffff"
  dark-primary-hover: "#2c559f"
  dark-success-bg: "#253627"
  dark-error-bg: "#402929"
  dark-scrim: "#080b1280"
  dopamine-canvas: "#fbf1e8"
  dopamine-panel: "#fffbe6"
  dopamine-subtle: "#fffef5"
  dopamine-text: "#20211f"
  dopamine-muted: "#585c52"
  dopamine-accent: "#5346a6"
  dopamine-accent-bg: "#ffde75"
  dopamine-hover: "#fff0bc"
  dopamine-good: "#176b4f"
  dopamine-warning: "#7a4a08"
  dopamine-error: "#a12c38"
  dopamine-status-bg: "#d5f1e5"
  dopamine-navigation: "#ffb7cf"
  dopamine-tools: "#42c69b"
  dopamine-primary-hover: "#ffe795"
  dopamine-pressed: "#f5dea3"
  dopamine-error-bg: "#ffd7cc"
  dopamine-scrim: "#20211f66"
  dopamine-selection: "#c8bdf4"
  dopamine-disabled-bg: "#ece8d9"
typography:
  body:
    fontFamily: "-apple-system, BlinkMacSystemFont, \"Segoe UI\", \"Noto Sans\", sans-serif"
    fontSize: "13px"
  control-idea:
    fontFamily: "-apple-system, BlinkMacSystemFont, \"Segoe UI\", \"Noto Sans\", sans-serif"
    fontSize: "13px"
    fontWeight: 500
  control-dopamine:
    fontFamily: "-apple-system, BlinkMacSystemFont, \"Segoe UI\", \"Noto Sans\", sans-serif"
    fontSize: "13px"
    fontWeight: 600
  title-idea:
    fontFamily: "-apple-system, BlinkMacSystemFont, \"Segoe UI\", \"Noto Sans\", sans-serif"
    fontSize: "17px"
    fontWeight: 600
  title-dopamine:
    fontFamily: "-apple-system, BlinkMacSystemFont, \"Segoe UI\", \"Noto Sans\", sans-serif"
    fontSize: "17px"
    fontWeight: 700
  label:
    fontFamily: "-apple-system, BlinkMacSystemFont, \"Segoe UI\", \"Noto Sans\", sans-serif"
    fontSize: "12px"
  metadata:
    fontFamily: "-apple-system, BlinkMacSystemFont, \"Segoe UI\", \"Noto Sans\", sans-serif"
    fontSize: "11px"
  badge:
    fontFamily: "-apple-system, BlinkMacSystemFont, \"Segoe UI\", \"Noto Sans\", sans-serif"
    fontSize: "10px"
  connection-title:
    fontFamily: "-apple-system, BlinkMacSystemFont, \"Segoe UI\", \"Noto Sans\", sans-serif"
    fontSize: "12px"
    fontWeight: 600
    lineHeight: "18px"
  endpoint:
    fontFamily: "Consolas, \"SFMono-Regular\", monospace"
    fontSize: "11px"
    lineHeight: "18px"
  log:
    fontFamily: "ui-monospace, SFMono-Regular, Consolas, monospace"
    fontSize: "12px"
  metric:
    fontFamily: "-apple-system, BlinkMacSystemFont, \"Segoe UI\", \"Noto Sans\", sans-serif"
    fontSize: "22px"
    fontWeight: 550
rounded:
  badge: "4px"
  navigation: "5px"
  connection-node: "6px"
  core-control: "7px"
  shared-control: "8px"
  menu: "10px"
spacing:
  step-3: "3px"
  step-4: "4px"
  step-5: "5px"
  step-6: "6px"
  step-8: "8px"
  step-9: "9px"
  step-10: "10px"
  step-12: "12px"
  step-14: "14px"
  step-16: "16px"
  step-18: "18px"
  step-20: "20px"
  step-24: "24px"
components:
  button-primary-light:
    backgroundColor: "{colors.light-primary}"
    textColor: "{colors.light-canvas}"
    typography: "{typography.control-idea}"
    rounded: "{rounded.core-control}"
    padding: "6px 12px"
  button-primary-hover-light:
    backgroundColor: "{colors.light-primary-hover}"
  button-secondary-light:
    backgroundColor: "{colors.light-canvas}"
    textColor: "{colors.light-text}"
    typography: "{typography.control-idea}"
    rounded: "{rounded.core-control}"
    padding: "6px 12px"
  button-primary-dark:
    backgroundColor: "{colors.dark-primary}"
    textColor: "{colors.dark-primary-text}"
    typography: "{typography.control-idea}"
    rounded: "{rounded.core-control}"
    padding: "6px 12px"
  button-primary-hover-dark:
    backgroundColor: "{colors.dark-primary-hover}"
  button-secondary-dark:
    backgroundColor: "{colors.dark-panel}"
    textColor: "{colors.dark-text}"
    typography: "{typography.control-idea}"
    rounded: "{rounded.core-control}"
    padding: "6px 12px"
  button-primary-dopamine:
    backgroundColor: "{colors.dopamine-accent-bg}"
    textColor: "{colors.dopamine-text}"
    typography: "{typography.control-dopamine}"
    rounded: "{rounded.core-control}"
    padding: "6px 12px"
  button-primary-hover-dopamine:
    backgroundColor: "{colors.dopamine-primary-hover}"
  button-secondary-dopamine:
    backgroundColor: "{colors.dopamine-panel}"
    textColor: "{colors.dopamine-text}"
    typography: "{typography.control-dopamine}"
    rounded: "{rounded.core-control}"
    padding: "6px 12px"
  button-icon-light:
    textColor: "{colors.light-text}"
    rounded: "{rounded.shared-control}"
    padding: "5px"
    size: "28px"
  field-search-light:
    backgroundColor: "{colors.light-canvas}"
    textColor: "{colors.light-text}"
    typography: "{typography.label}"
    rounded: "{rounded.core-control}"
    padding: "6px 10px 6px 31px"
    height: "32px"
  navigation-console-selected-light:
    backgroundColor: "{colors.light-accent-bg}"
    textColor: "{colors.light-text}"
    typography: "{typography.body}"
    rounded: "{rounded.navigation}"
    padding: "7px 11px"
  badge-running-light:
    backgroundColor: "{colors.light-success-bg}"
    textColor: "{colors.light-good}"
    typography: "{typography.badge}"
    rounded: "{rounded.badge}"
    padding: "3px 6px"
  workspace-tab-selected-light:
    backgroundColor: "{colors.light-canvas}"
    textColor: "{colors.light-text}"
    typography: "{typography.label}"
    height: "30px"
  connection-node-selected-light:
    backgroundColor: "{colors.light-accent-bg}"
    textColor: "{colors.light-text}"
    rounded: "{rounded.connection-node}"
    padding: "5px 9px"
    height: "52px"
  card-pending-light:
    backgroundColor: "{colors.light-accent-bg}"
    textColor: "{colors.light-text}"
    rounded: "{rounded.shared-control}"
    padding: "16px"
---

# Design System: SoDeck 核心界面

## Overview

**Creative North Star: "服务器会话工作台"**

SoDeck 的核心界面是一张紧凑、可持续工作的服务器会话工作台。桌面壳维持应用导航与连接身份，管理台集中维护资料和偏好，工作台承载会话中的插件页面与工具。信息层级依靠区域、结构线、选中底和可读状态，不以大标题或装饰性卡片组织每个操作。

用户已确认保留多巴胺主题，浅色和深色采用 IDEA 默认配色方向。三主题共享控件几何与操作路径；多巴胺通过暖底、粉色导航、黄色选中、绿色工具底和更粗的结构线表达个性，IDEA 明暗主题以中性面和蓝色焦点服务长时间阅读。

这份文档从当前正式核心源码提取，服务后续核心界面维护。色值与公共控件以 [themes.css](packages/ui/themes.css)、[shared.css](packages/ui/shared.css) 为准；核心覆盖以 [console.css](src/console.css) 为准。已批准的 [界面原型](prototypes/interface/DESIGN.md) 只提供方向参考。文档不宣布本次源码已经完成发行验收。

范围为桌面壳、管理台、服务器连接目录、会话标签和插件 Surface 宿主。文件等插件的内部展示与操作冻结；业务功能与失败行为仍由 [产品定义](docs/PRODUCT.md) 链接的功能文档维护。

**Key Characteristics:**

- 核心导航、服务器会话和插件工具入口分区明确。
- 三主题共享紧凑几何，主题差异通过角色颜色、描边和字重表达。
- 路径与日志采用等宽字，状态带文字，键盘焦点可见。

## Colors

frontmatter 只收录核心中复用的实际颜色；同一主题中的同值角色合并为一个条目。它是本文件的机器可读色值层，运行时的唯一色值定义仍是 [themes.css](packages/ui/themes.css)。[design.json](.impeccable/design.json) 的 themeRoles 将 CSS 角色映射到这些条目，tokenSources 记录来源。文件、终端 ANSI 色表和一次性装饰值不在此次提取范围。

### Primary

IDEA 蓝用于主操作，accent 用于焦点、访问路线与选中边缘。明暗主题的 accent 与主按钮底是两个既有角色；主操作必须使用 sd-primary 与 sd-primary-text，不能由 accent 与 panel 推算。文本动作和发行说明链接独立使用 sd-link；不把主按钮背景色同时用作深色链接。多巴胺的主按钮使用黄色和深字，紫色 accent 用于焦点与路线。

### Secondary

多巴胺粉色 navigation 承载管理导航，绿色 tools 承载工具和工作标签区域，暖色 rail 承载窄侧栏。IDEA 浅色管理导航使用 accent-bg；IDEA 深色导航、工具区和侧栏使用 panel。顶栏实际使用 panel，不采用原型的浅色深顶栏值。

### Tertiary

good、warning、error 分别辅助成功、需关注和错误文字。success-bg 与 error-bg 提供内联状态底；状态点、消息标签和连接状态必须同时显示文字。普通日志级别不决定是否显示用户通知，行为依据 [日志与消息](docs/features/logs-messages.md)。

### Neutral

| CSS 角色 | 主要用途 |
| --- | --- |
| canvas / panel / subtle | 工作底、内容面与次要区域。 |
| text / muted / border | 正文、说明与结构分隔。 |
| hover / sd-pressed / sd-disabled-bg | 悬停、按下与禁用底。 |
| sd-tab-selected / sd-server-selected | 工作页面与服务器会话的选中底，两者分别读取。 |
| sd-selection | 一般文本选区。 |
| status-bg / sd-scrim | 底栏与模态遮罩。 |

主题 ID、中文名称及明暗属性登记在 [themes.json](packages/ui/themes.json)。跟随系统由 [theme.ts](src/theme.ts) 解析为 light 或 dark，不是第四套配色。对外主题 token 名维护在 [token-names.ts](packages/ui/token-names.ts)；核心私有的 sd-server-selected、sd-symbol 等变量不能因本文件出现而自动成为插件 API。forced-colors 模式采用源码中的系统颜色与原生滚动条处理。

**The 主题同构 Rule.** 使用同一布局和交互，颜色、描边和字重通过现有主题角色切换；不复制三套页面结构。

## Typography

**Body Font:** frontmatter 的 body 系统字体栈，不打包字体。
**Label/Mono Font:** endpoint 用于地址；log 用于日志。两者保持源码中的独立回退顺序。

界面默认字号为 body（13px），根元素没有指定统一行高；段落使用已实现的行高（1.7）。普通区域标题使用 title（17px）与 section 标题（14px）；标题字重由主题角色控制，IDEA 为 title-idea，多巴胺为 title-dopamine。控件字重分别为 control-idea 和 control-dopamine。

导航、工作标签与字段说明通常使用 label（12px）；底栏与地址使用 metadata（11px），状态标签使用 badge（10px）。连接节点名称固定使用 connection-title（12px / 600 / 18px），第二行地址使用 endpoint（11px / 18px），不套用全局标题字重。插件与更新维护标题为（22px），在 ≤800px 时为（20px），以 [updates.css](src/updates.css) 的局部规则为准。应用状态数值使用 metric（22px / 550）。这些局部字号和字重来源见 [shell.css](src/shell.css)、[servers.css](src/servers.css) 与 [console.css](src/console.css)。

长名称、地址和标签在紧凑节点中保持单行省略，通过 title 与右侧详情查看完整内容；错误和消息正文允许换行，不把路径文本当成本地 Windows 路径处理。

## Layout

核心壳占满视口（100dvh），顶栏与底栏固定在中间工作区域之外。当前核心 #app 覆盖为顶栏（44px）、底栏（26px）、工作标签条（38px）、状态条（26px）、常用点击尺寸（28px）。公共 shared.css 的工作标签条为（36px）、状态条为（32px），插件仍使用公共值。公共 token 与核心尺寸的边界见 [公共界面源码说明](packages/ui/README.md)。

管理台使用功能导航（156px）加弹性内容，路径栏（42px）位于内容上沿。普通页面内边距为（20px 24px）；设置还保留内部导航（160px）。工作台默认是左右入口栏（各 42px）加弹性中央区，侧面板宽度由宿主布局计算，CSS 的有效宽度回退为（220px）。布局依据 [console.css](src/console.css)、[workspaces.css](src/workspaces.css) 与 [components.css](src/components.css)。

连接目录上方是紧凑工具栏；列表和详情各自滚动，默认详情宽度为 max（290px，36%）。节点固定高（52px），紧凑密度为（48px），当前紧凑样式只修改高度。节点间隔（4px），内边距（5px 9px），层级缩进（20px）；列表内边距（12px 14px 24px）。本机起点占（32px）并留（8px）下间距。连接树几何依据 [servers.css](src/servers.css)。

| 当前断点 | 源码中的变化 |
| --- | --- |
| ≤1000px | 连接详情固定为 280px，详情和工具栏减小内边距。 |
| ≤900px | 管理台功能导航缩为 128px。 |
| ≤880px | 设置、插件管理、消息和日志的旧页面布局进入窄窗规则。 |
| ≤820px | 顶栏消息和设置快捷入口保留图标，缩小管理入口留白。 |
| ≤800px | 更新与插件维护页缩减内边距、重排操作列，更新网络表单改为单列；矮窗（≤600px 高）进一步压缩纵向留白。 |
| ≤760px | 连接列表与详情互切；详情模式隐藏目录工具栏，节点层级缩进为 16px。 |
| ≤640px | 管理台导航改为横向滚动的顶行，隐藏导航图标与底部说明；路径栏为 36px，普通内容内边距为 12px。 |
| ≤500px | 连接对话框内边距为 14px，路线动作和密钥选择允许换行。 |

分屏收束由宿主根据可用空间与最小内容尺寸决定，不能从原型的固定断点推定正式行为。服务器会话和页面标签保留横向滚动与关闭按钮；底部连接身份超长时省略。详细操作仍以 [工作台](docs/features/workbench.md) 为准。

**The 几何分层 Rule.** 公共控件几何与核心壳尺寸分别读取 shared.css 和 #app 覆盖；核心壳尺寸不得作为插件主题快照发送。

## Elevation & Depth

主工作面使用背景层级和结构线；菜单、连接对话框及旧核心浮层读取 shadow。IDEA 浅色与深色分别使用源码中的主题阴影，多巴胺为 none。准确值收录在 sidecar 的 shadows，遮罩通过 sd-scrim 切换。工作标签选中不加阴影，当前聚焦分屏仅以底边 accent 标识。

按钮状态采用已实现的背景过渡（100ms ease-out）。旧核心浮层采用 appear（120ms ease-out），从向上位移（4px）及透明进入；prefers-reduced-motion 下关闭动画与过渡。这里不引入原型的弹簧、抽屉或新位移动效。源码依据 [shared.css](packages/ui/shared.css)、[shell.css](src/shell.css) 与 [components.css](src/components.css)。

**The 浮层才抬升 Rule.** 工作面靠背景和结构线分区；菜单与对话框使用已有 shadow 角色，多巴胺保持无阴影。

## Shapes

拼接区域使用矩形和结构线，控件采用短半径圆角。状态标签为 badge（4px），管理导航为 navigation（5px），连接节点为 connection-node（6px），当前核心普通按钮和字段为 core-control（7px），公共 sd-* 控件为 shared-control（8px），菜单为 menu（10px），待重启容器沿用 shared-control（8px）。工作页面标签沿用上圆角（8px）并在底边接入内容面；不要将它简化为四角相同的卡片。

IDEA 的结构线与控件描边为（1px），图标线宽（2），控件/标题字重（500 / 600）；多巴胺对应为（2px）、（2.25）与（600 / 700）。公共控件使用透明边框预算（2px）及伪元素描边，主题切换保持内容几何。普通功能 SVG 默认（17px），公共图标按钮中的 SVG 为（16px），标签关闭图标为（12px）；路径与形状来自 [ui.ts](src/ui.ts)。

## Components

### Buttons

当前核心普通按钮使用 core-control 圆角、最小高（32px）、内边距（6px 12px）、panel 底与 text。这里是最小高度，实际高度还受文字和主题实边框影响；公共 sd-control 的透明描边预算不能套用于裸按钮。主按钮使用 sd-primary / sd-primary-text，悬停改为 sd-primary-hover。不叠加额外亮度滤镜。文本按钮使用 sd-link。源码见 [shell.css](src/shell.css)、[updates.css](src/updates.css)。

图标按钮使用公共点击尺寸（28px）与 shared-control 圆角，透明底、内边距（5px）。quiet 与底栏动作使用透明底；悬停使用 hover，按下使用 sd-pressed，禁用透明度为（0.45）。侧栏、顶栏和关闭按钮的局部尺寸以各自选择器为准。通用键盘焦点为 accent 轮廓（2px）和内偏移（-2px），连接节点单独使用外偏移（2px）。

### Chips

状态标签使用 badge 字号与圆角、内边距（3px 6px），普通底为 hover，成功/错误底使用语义浅底；运行中和失败标签添加 currentColor 小点（5px）。它们是文字状态标签，不能赋予不存在的点击或筛选动作。实际模板见 [views.ts](src/views.ts) 与 [plugin-settings.ts](src/plugin-settings.ts)。

### Cards / Containers

实际待重启容器使用 shared-control 圆角、accent-bg 底、内边距（16px）和可换行布局，正文保留重启会结束连接与任务的明确提示；它没有额外描边。管理台和工作面仍按区域拼接，不把每段信息自动包进卡片。相关行为见 [更新](docs/features/updates.md)，模板与样式见 [updates.ts](src/updates.ts)、[updates.css](src/updates.css)。

### Inputs / Fields

普通字段高（34px），panel 底、text 字、core-control 圆角、内边距（6px 10px）。目录搜索字段高（32px），左侧图标占位让左内边距增为（31px）。字段字号继承父级，目录搜索继承 label（12px）。输入光标使用 accent，占位文字使用 muted；焦点、required 和数值范围继续使用现有字段规则，不新增虚构的错误字段样式。来源为 [shell.css](src/shell.css)、[console.css](src/console.css)、[servers.css](src/servers.css) 与 [server-views.ts](src/server-views.ts)。

### Navigation

顶栏 SoDeck 是管理台入口；主窗口的加号进入服务器连接页。管理台以功能导航显示服务器连接、插件、设置、消息、日志、状态和更新，当前项使用 aria-current、sd-server-selected 底与内嵌描边；顶栏继续保留服务器会话标签。管理台隐藏整个工作台宿主及插件窄侧栏，返回工作台或选择会话后恢复。

宿主可见性切换保留会话、页面、Surface 与草稿，不等于关闭或销毁。左右窄侧栏只承载 sidebar 插件贡献；终端由中央新建动作和工作标签使用。宿主按贡献容器类型处理隐藏与关闭，规范依据 [shell.ts](src/shell.ts)、[workspaces.ts](src/workspaces.ts) 与 [components.ts](src/components.ts)，完整行为见 [工作台](docs/features/workbench.md)。

### Workspace Tabs

工作标签条读取核心高度（38px），标签本体高（30px），上圆角与透明描边预算沿用公共 tab。当前页面底为 sd-tab-selected，标题和关闭分别是按钮；标题单行省略，未保存页以已有脏标记呈现，关闭的 sidebar 页面显示隐藏语义。分屏聚焦与页面选中分别表达。模板见 [components.ts](src/components.ts)，样式见 [shared.css](packages/ui/shared.css) 与 [components.css](src/components.css)。

### Connection Tree

节点用两行组织名称、协议、状态，以及账号地址与自身标签；状态保留独立显示空间。目标选中使用 accent-bg 底与 accent 边，完整祖先路线的连线使用 accent；当前源码没有为所有祖先节点统一添加 accent 边的样式，不能照搬原型说明。键盘焦点使用外轮廓，树中的层级、展开状态与选中状态保留对应 ARIA。

搜索与多标签 AND 筛选保留完整祖先路线，并用“路径节点”区分非命中祖先；仅目标计入匹配。目录语义与连接动作见 [连接功能](docs/features/connections.md)，模板和模型来源为 [server-views.ts](src/server-views.ts) 与 [connection-tree.ts](src/connection-tree.ts)。

### Dialogs

连接编辑器采用原生 dialog，默认宽为 min（720px，窗口减 32px）、圆角（14px）、panel 底与 shadow；小确认对话框为 min（520px，窗口减 32px）。表单正文独立滚动，页脚动作右对齐，取消和关闭恢复现有发起焦点。核心关闭确认与插件 dialog 宿主使用自身已有尺寸；本文件不重定义插件 iframe 内部界面。来源为 [servers.css](src/servers.css)、[servers.ts](src/servers.ts) 与 [components.css](src/components.css)。

[design.json](.impeccable/design.json) 提供九种当前控件的自包含 HTML/CSS 片段：主按钮、次按钮、图标按钮、搜索字段、管理导航、状态标签、待重启容器、工作标签与连接节点。片段的 ds-* 名仅用于预览隔离，CSS 变量保持运行时绑定，并以当前浅色值提供离线回退；sources 标明真实选择器和模板来源。预览不执行连接、管理导航、插件状态机或模态焦点逻辑，生成的 tonalRamp 仅用于面板展示，不新增主题 token。

## Do's and Don'ts

### Do:

- Do 使用 packages/ui 的现有主题角色，并保持多巴胺、IDEA 浅色和 IDEA 深色的同构布局。
- Do 将核心功能放在管理台导航，保留左右窄侧栏给 sidebar 插件贡献。
- Do 让终端通过中央新建动作与工作标签使用，切换管理台时保留会话、Surface 和草稿。
- Do 为连接、插件和消息状态保留可读文字，为图标按钮提供名称。
- Do 使用当前系统字体、等宽地址和已有焦点处理，并按当前窗口宽度保留滚动与省略。

### Don't:

- Don't 将原型的数值、演示数据或简化文件操作直接视作当前核心实现。
- Don't 把核心 #app 尺寸下发到插件，或随主题切换改变控件布局。
- Don't 在本次核心界面规范中重定义文件、终端、Docker 等插件的内部展示和操作。
- Don't 用圆点或颜色代替连接和失败文字，也不把请求已发出写成操作成功。

---
name: "SoDeck 界面原型"
description: "服务器会话、工作页面与侧工具共用的三主题桌面原型"
colors:
  dark-accent: "#3574f0"
  dark-canvas: "#1e1f22"
  dark-panel: "#2b2d30"
  dark-raised: "#393b40"
  dark-text: "#dfe1e5"
  dark-muted: "#9da0a8"
  dark-control-border: "#4e5157"
  dark-selected: "#2e436e"
  dark-good: "#89cc8e"
  dark-warning: "#f2c55c"
  dark-error: "#eb938d"
  dark-good-bg: "#253627"
  dark-warning-bg: "#3d3223"
  dark-error-bg: "#402929"
  dark-toolbar-muted: "#b4b8bf"
  dark-code-green: "#6aab73"
  dark-code-blue: "#56a8f5"
  dark-code-orange: "#cf8e6d"
  light-accent: "#3574f0"
  light-canvas: "#ffffff"
  light-panel: "#f7f8fa"
  light-text: "#27282e"
  light-muted: "#5a5d6b"
  light-border: "#dfe1e5"
  light-control-border: "#c9ccd6"
  light-selected: "#d4e2ff"
  light-hover: "#ebecf0"
  light-good: "#1e6b33"
  light-warning: "#a46704"
  light-error: "#ad2b38"
  light-good-bg: "#e3f7e7"
  light-warning-bg: "#fff5db"
  light-error-bg: "#ffebec"
  light-toolbar-selected: "#383a42"
  light-toolbar-hover: "#494b57"
  light-code-green: "#067d17"
  light-code-blue: "#0033b3"
  light-code-orange: "#9e4b00"
  light-code-muted: "#6c707e"
  dopamine-accent: "#5346a6"
  dopamine-canvas: "#fbf1e8"
  dopamine-panel: "#fffbe6"
  dopamine-raised: "#fffef5"
  dopamine-text: "#20211f"
  dopamine-muted: "#585c52"
  dopamine-selected: "#ffde75"
  dopamine-hover: "#fff0bc"
  dopamine-good: "#176b4f"
  dopamine-warning: "#7a4a08"
  dopamine-error: "#a12c38"
  dopamine-good-bg: "#d5f1e5"
  dopamine-error-bg: "#ffd7cc"
  dopamine-toolbar: "#ffb7cf"
  dopamine-toolbar-hover: "#ffd3e2"
typography:
  headline:
    fontFamily: "\"Segoe UI\", \"Microsoft YaHei UI\", \"PingFang SC\", system-ui, sans-serif"
    fontSize: "21px"
    fontWeight: 650
    lineHeight: 1.4
  title:
    fontFamily: "\"Segoe UI\", \"Microsoft YaHei UI\", \"PingFang SC\", system-ui, sans-serif"
    fontSize: "16px"
    fontWeight: 600
    lineHeight: 1.5
  dialog-title:
    fontFamily: "\"Segoe UI\", \"Microsoft YaHei UI\", \"PingFang SC\", system-ui, sans-serif"
    fontSize: "17px"
    fontWeight: 600
    lineHeight: 1.5
  body:
    fontFamily: "\"Segoe UI\", \"Microsoft YaHei UI\", \"PingFang SC\", system-ui, sans-serif"
    fontSize: "13px"
    fontWeight: 400
    lineHeight: 1.5
  label:
    fontFamily: "\"Segoe UI\", \"Microsoft YaHei UI\", \"PingFang SC\", system-ui, sans-serif"
    fontSize: "12px"
    fontWeight: 400
    lineHeight: 1.5
  metadata:
    fontFamily: "\"Segoe UI\", \"Microsoft YaHei UI\", \"PingFang SC\", system-ui, sans-serif"
    fontSize: "11px"
    fontWeight: 400
    lineHeight: 1.5
  mono:
    fontFamily: "\"JetBrains Mono\", Consolas, \"SFMono-Regular\", \"Liberation Mono\", monospace"
    fontSize: "13px"
    fontWeight: 400
    lineHeight: 1.8
  metric:
    fontFamily: "\"Segoe UI\", \"Microsoft YaHei UI\", \"PingFang SC\", system-ui, sans-serif"
    fontSize: "22px"
    fontWeight: 500
    lineHeight: 1.3
rounded:
  row: "4px"
  control: "5px"
  menu: "6px"
  connection-node: "6px"
  drawer: "7px"
  dialog: "8px"
spacing:
  tight: "4px"
  compact: "6px"
  inline: "8px"
  small: "12px"
  section: "16px"
  dialog: "24px"
  page: "32px"
  tree-indent: "20px"
components:
  button-primary-idea:
    backgroundColor: "{colors.dark-accent}"
    textColor: "{colors.light-canvas}"
    rounded: "{rounded.control}"
    padding: "5px 11px"
  button-primary-dopamine:
    backgroundColor: "{colors.dopamine-selected}"
    textColor: "{colors.dopamine-text}"
    rounded: "{rounded.control}"
    padding: "5px 11px"
  button-secondary-dark:
    backgroundColor: "{colors.dark-raised}"
    textColor: "{colors.dark-text}"
    rounded: "{rounded.control}"
    padding: "5px 11px"
  field-dark:
    backgroundColor: "{colors.dark-canvas}"
    textColor: "{colors.dark-text}"
    typography: "{typography.label}"
    rounded: "{rounded.control}"
    padding: "6px 9px"
  badge-dark:
    backgroundColor: "{colors.dark-selected}"
    textColor: "{colors.dark-text}"
    typography: "{typography.metadata}"
    rounded: "{rounded.row}"
    padding: "1px 6px"
  sidebar-item-dark:
    backgroundColor: "{colors.dark-selected}"
    textColor: "{colors.dark-text}"
    rounded: "{rounded.control}"
    padding: "8px 10px"
  tool-pane-dark:
    backgroundColor: "{colors.dark-panel}"
    textColor: "{colors.dark-text}"
    width: "272px"
  dialog-dark:
    backgroundColor: "{colors.dark-raised}"
    textColor: "{colors.dark-text}"
    rounded: "{rounded.dialog}"
    width: "620px"
  connection-node-dark:
    backgroundColor: "{colors.dark-canvas}"
    textColor: "{colors.dark-text}"
    rounded: "{rounded.connection-node}"
    padding: "5px 10px"
    minHeight: "52px"
---

# Design System: SoDeck 界面原型

## Overview

**Creative North Star: "服务器会话工作台"**

这份系统只描述已构建的可点击原型。服务器会话是操作上下文，终端、文件编辑和容器详情是会话内的工作页面；目录、工作页面和侧工具各占明确区域，连接身份留在底栏。顶栏 SoDeck 打开完整管理台，核心功能在管理台内部导航，左右窄侧栏只承载插件入口。正式应用尚未采用这套界面。

用户指定保留多巴胺主题，明暗两主题直接使用 IDEA 默认配色。三主题共用紧凑标准控件、信息层级与交互；差异集中在颜色、结构线宽和控件字重。系统界面字体服务操作阅读，等宽字体服务路径、终端输出和代码。

文件插件的设计以现有正式插件为基线，仅作视觉优化；操作和展示逻辑的保留范围见[重构计划](../../docs/plans/interface-refactor.md#目标与边界)。原型的简化文件演示不作为新的交互规范。

原型只使用合成数据和本地演示状态，不读取真实凭据、不连接服务器、不执行远端命令；没有新增生产依赖。系统窗口、多窗口迁移、完整 xterm/编辑器及真实操作仍需正式重构与原生验收。浏览器原型不能替代五个正式目标的实际包验收。

依据：[产品定义](../../docs/product.md)、[方向契约](../../.impeccable/surfaces/prototypes-interface-index-html.md)。实际 token 提取自 [style.css](./style.css)，组件及演示行为见 [app.js](./app.js)，合成数据见 [data.js](./data.js)；待接入正式前端的范围见 [重构计划](../../docs/plans/interface-refactor.md)。

**Key Characteristics:**

- 会话身份、核心导航与插件工具区域清楚分离。
- 三主题同构，颜色差异不改变操作路径。
- 紧凑标准控件、文字状态与可见键盘焦点。

## Colors

IDEA 明暗主题以 Gray 组织层级，用 Blue 标记操作与焦点；多巴胺以暖底、粉色导航、黄色选区和绿色工具底承载同一界面。frontmatter 收录实际复用颜色，同主题的相同值共用一个条目；sidecar 的 themeRoles 将实际 CSS 角色映射到这些 token。

### Primary

- IDEA Blue 用于明暗主题主按钮、焦点环、页面下划线与指标线。
- 多巴胺主按钮使用黄色选区和深色文字；紫色 accent 用于焦点、页面下划线和代码变量。

### Secondary

- 多巴胺粉色承载顶栏及目录导航区域，浅绿色承载侧工具背景。

### Tertiary

- good / warning / error 及语义浅底用于连接、健康状态与内联提示；状态始终带文字。
- code-green / code-blue / code-orange / code-muted 区分提示符、变量与数字、关键字、注释和终端元信息。

### Neutral

| 区域 | IDEA 深色 | IDEA 浅色 | 多巴胺 |
| --- | --- | --- | --- |
| 工作底 / 面板 | dark-canvas / dark-panel | light-canvas / light-panel | dopamine-canvas / dopamine-panel |
| 正文 / 次要文字 | dark-text / dark-muted | light-text / light-muted | dopamine-text / dopamine-muted |
| 顶栏 / 工具区 | dark-panel / dark-panel | light-text / light-panel | dopamine-toolbar / dopamine-good-bg |
| 选中底 | dark-selected | light-selected | dopamine-selected |

IDEA 来源：[官方深色主题](https://raw.githubusercontent.com/JetBrains/intellij-community/master/platform/platform-resources/src/themes/expUI/expUI_dark.theme.json)、[官方浅色主题](https://raw.githubusercontent.com/JetBrains/intellij-community/master/platform/platform-resources/src/themes/expUI/expUI_light.theme.json)。深色核心采用 Gray1 / Gray2 / Gray12 / Blue6；浅色采用 Gray2 / Gray13 / Gray14 / Blue4，浅色顶栏也使用 darkGray2。多巴胺按角色取值自仓库 [themes.css](../../src/themes.css)。

深色连接树选中节点、容器选中项、待重启横条、活动导航和对话框的次要文字使用既有 Gray10（dark-toolbar-muted），使小字在 selected 和 raised 底保持至少 4.5:1 的对比。其他次要文字和代码注释继续使用原有角色。

多巴胺管理台粉色侧栏底部说明使用既有 text 深色角色，保持小字对比；粉色背景不变。

**The 主题同构 Rule.** 主题切换保留布局与交互。IDEA 明暗主题遵循原型中的官方 Gray/Blue 角色；多巴胺保留粉色导航、黄色选中和绿色工具区域。

## Typography

**Body Font:** frontmatter 的 body 字体栈，按系统可用字体回退，不打包字体。
**Label/Mono Font:** mono 字体栈；终端初始以 Consolas 优先，再回退到同一等宽栈。

界面没有展示型大字。正文默认 13px / 1.5，路径及元信息采用 11–12px。页面标题 21px / 650，≤480px 时为 19px；普通区域标题 16px / 600，对话框标题 17px / 600，工具标题 12px / 600。指标数值使用 22px / 500 / 1.3；数值与时间采用等宽数字，消息段落最大宽度为 75ch。

终端与文件编辑器默认 13px / 1.8；终端演示设置可调整字号、行高与字体。编辑器叠加透明 textarea 和着色预览，行号、预览与输入区同步滚动。它仍是原型的小文本编辑展示。

## Layout

桌面壳占满 100dvh：顶栏 44px、底栏 26px，中间区域独立滚动。默认工作台依次为 48px 插件页面入口栏、240px 文件区、可伸缩工作页面、272px 侧工具区和 42px 插件工具入口栏。管理台占满顶栏与底栏之间，不展示插件窄侧栏，使用 192px 功能导航、46px 页面路径栏和 26px / 32px 内容内边距；工具标题与工作标签条高 38px。

常用间距见 frontmatter 的 4 / 6 / 8 / 12 / 16 / 24 / 32px。表单默认两列、14px 间距，窄屏单列；连接节点默认最小高 52px、内边距 5px / 10px、节点间隔 4px、层级缩进 20px。紧凑密度把节点最小高降为 48px、垂直内边距降为 3px，插件行也缩减垂直留白。树区域内边距为 14px / 14px / 18px。连接页直接以紧凑工具栏开始：150px 资料类型选择、搜索、管理标签、刷新和新增；标签位于工具栏下方，树与详情各自滚动。页面身份由管理台导航和路径栏表达，内容区不重复大标题、教程和独立分类标签行。

| 断点 | 实际变化 |
| --- | --- |
| ≤1200px | 文件区 214px、工具区 250px；连接详情宽 260px。 |
| ≤980px | 侧工具收起，入口打开固定侧工具；文件区 190px、管理台导航 172px。 |
| ≤760px | 管理台导航移到内容上方并换行，路径栏 42px；文件区收起，目录列表与详情互切，文件入口展开侧工具；分屏改为上下各半。 |
| ≤480px | 主入口栏 40px、工具入口栏 34px；文件工具对齐左入口栏，右侧工具占剩余宽度；连接工具栏两行，上行资料类型和新增，下行搜索、管理标签和刷新；标签默认收起，详情隐藏目录控件；表单单列、动作换行。 |

窄窗的文件与会话工具通过同一 toolOpen 状态展开；从文件子项打开页面时收起文件工具，避免持续遮挡新页面。会话栏可横向滚动，底栏连接身份对溢出文本显示省略号。

**The 会话上下文 Rule.** 切换会话时，工作页面、侧工具和底部连接身份一同切换；上下文不能只依赖顶栏名称。

## Elevation & Depth

普通工作面使用背景层级、结构线和选中底分区。普通控件与工具面板不加阴影；对话框使用 `0 16px 54px #00000044`，短提示使用 `0 6px 24px #00000022`，对话框另有主题 scrim。抽屉和菜单靠边框与覆盖位置表达浮层。

仅在 prefers-reduced-motion: no-preference 时播放对话框 140ms / 5px 和抽屉 160ms / 8px 的进入位移，统一使用 `cubic-bezier(.16, 1, .3, 1)`；减少动画偏好下不播放这些进入动画。

## Shapes

结构区沿矩形边缘拼接；列表用底色、工作标签用下划线表达选中。小行和标签圆角 4px、按钮和字段 5px、提示和菜单 6px、主题样卡和抽屉 7px、对话框 8px。明暗主题结构及控件线宽 1px、控件字重 500；多巴胺线宽 2px、控件字重 600。功能 SVG 默认 18px、描边 1.7；文件图标 16px、工作标签图标 15px。

连接节点用 6px 圆角、独立边框与 18px 协议图标表达节点感；实际测量节点位置绘制折线和接点。选择节点后，完整祖先路径的连线和节点边框使用 accent，目标节点再使用 selected 底色。中转节点与目标保持同等文字可读性，以“路径节点”文字区分。

## Components

### Buttons

默认按钮最小高 30px，内边距 5px / 11px；次按钮使用 raised 底和 control-border。IDEA 主按钮使用 Blue 白字，多巴胺主按钮使用黄色、深字和深边。quiet / 图标按钮透明底，悬停用主题 hover；禁用控件透明度 0.48。图标按钮通常宽 30px，入口栏和标签关闭按钮使用各自紧凑尺寸。

### Chips

标签使用 11px 字号、1px / 6px 内边距、4px 圆角与 selected 底；错误标签用 error-bg / error。状态为 6px 圆点加文字，能分辨连接失败、连接中、断开和未连接。插件待重启采用 selected 横条和文字说明。

### Cards / Containers

工具容器宽 272px、内容内边距 16px，以线分隔主机摘要与指标。主题选择样卡使用 7px 圆角和 2px 外轮廓标记当前主题。内联提示采用 10px / 12px 内边距、5px 圆角和语义浅底。

系统状态的网卡选择直接显示网卡名称及 IP/掩码位数，例如 `eth0 · 10.20.0.24/24`，便于选择前识别网段；选择框使用面板可用宽度，地址详情与读数随选中网卡切换，选择按会话保留。原型地址、掩码和读数均为合成示例；正式展示使用服务器实际返回的地址与前缀，不推测未知掩码。

### Inputs / Fields

字段为 canvas 底、control-border、5px 圆角和 6px / 9px 内边距；标签 12px、间距 6px。搜索框组合图标与无边输入，在组合边缘显示焦点；checkbox / radio 使用 accent。表单使用浏览器 required 与数值范围校验，原型没有另造错误字段状态。

### Navigation

会话顶栏用 toolbar-selected 表达活动会话；工作页面使用 3px accent 下划线，页面选择与关闭分别为按钮。目录导航和文件行用 selected 底。窄窗保留入口栏，通过入口展开工具；工作标签支持左右箭头、Home 和 End 切换及焦点移动。

顶栏把品牌与管理入口合为一个 SoDeck 按钮，点击后在主区域打开完整管理台，默认进入服务器连接。管理台内的功能导航按服务器连接、插件管理、设置、消息中心、诊断日志、应用状态和软件更新排列；活动项标明 aria-current。服务器连接页把标签筛选、代理分类、搜索、连接树和详情放在同一工作面。消息与设置保留右上角快捷入口。

顶部会话始终保留；点击会话或管理台的“返回工作台”回到对应会话，文件草稿继续保留。顶栏加号也进入同一个服务器连接页面。管理台导航在窄窗移到页面上方并换行，所有核心功能仍可直接选择。

**The 插件入口 Rule.** 左右窄侧栏只在会话工作台展示，并只承载插件贡献；核心功能使用管理台内的完整导航。终端通过新建页面和标签页使用，不占侧栏按钮位。原型侧栏按钮标明所属插件，按运行状态显示，插件启停仍在重启后生效。

### Connection Tree

每个节点以两行呈现：首行放名称、协议和状态或“路径节点”标记，第二行并排放账号地址和自身标签。名称、地址和标签超出可用宽度时省略，悬停标题及右侧详情提供完整内容；协议和状态保留显示空间，不换行。本机起点合为“本机 / 连接起点”一行，减少树顶部留白。≤480px 层级缩进为 16px，树左右内边距为 10px，节点左右内边距为 7px；层级与连线始终保留。

树按每个目标的显式有序 SSH/SOCKS5 路线构建，共享相同路径前缀。同一跳板可以出现在不同路径位置；引用跳板只取其地址与认证资料，不递归展开它自己的路线。这与正式连接服务的显式路径语义一致，见 [连接功能](../../docs/features/connections.md)。

多选标签使用 AND：只匹配目标自身的全部所选标签；搜索匹配目标名称、地址、账号和自身标签。命中目标保留完整祖先路径，非命中祖先标为“路径节点”且不计入匹配，不继承标签。筛选期间自动展开路径并禁用折叠，清除筛选恢复原折叠状态；代理页不应用服务器标签选择。

进入管理台时，若选中目标位于视野外，树滚动到完整显示该节点。≤480px 标签筛选通过展开控件使用，收起后仍显示 AND 规则、选择数量与清除入口；详情模式隐藏工具栏和标签，返回列表恢复这些控件。连线画布按实际布局高度测量，连续缩窄再放宽不保留旧画布空白。

详情优先显示完整当前路径，再通过可展开的“连接资料与认证”查看字段。双击或 Enter 仅连接命中的 SSH 目标，路径节点和 SOCKS5 资料没有单独连接入口。树支持上下键、Home/End 移动，左右键展开、收起或进入父子节点。编辑器按顺序添加已有 SSH 跳板和 SOCKS5 代理，可移动或删除步骤；禁止自身和重复引用，取消编辑保留原路线。仍被路线引用的资料不能删除。

资料类型选择明确区分服务器和 SOCKS5 代理，切换后同步更新搜索名称及新增动作。代理仍只有查看、编辑和路线引用；界面不提供单独连接入口。

多标签字段目前只存在于原型合成数据。正式应用仍有分组字段；采用新界面前需要定义兼容迁移与保存 API，原型未修改正式数据模型。

### Dialogs and Editing

实际对话框使用原生 `dialog.showModal()`；普通宽 620px，连接编辑器宽 720px，最大宽为窗口减 32px，内容可滚动，右对齐动作间隔 8px。取消和 Escape 关闭后恢复发起控件；路线步骤和认证方式变更保留表单草稿及焦点；未保存文件提供取消、放弃更改、保存并关闭，断线时禁用保存操作。

通用焦点为 2px accent 外轮廓、偏移 2px；搜索框使用组合焦点，终端输入用底部细线，代码输入用内嵌 1px 环。一般文本选区使用 selected 底和 text 字；编辑器透明输入的选区只混合 selected 底，继续显示底层着色文字。

**The 焦点连续 Rule.** 重绘后用稳定操作身份恢复对应新节点的焦点；关闭或取消对话框后回到发起控件，文本控件同时保留可恢复的选区。

[design.json](./.impeccable/design.json) 提供九种实际组件的三主题 HTML/CSS：主按钮、次按钮、图标按钮、字段、侧栏、状态标签、工具容器、对话框和连接节点。样例自包含；预览容器只安排展示，不运行原型状态机或原生模态焦点逻辑。生成的 tonalRamp 只供面板预览，不是新增主题 token。

## Do's and Don'ts

### Do:

- **Do** 使用原型主题角色值，并保留浅色主题的深色顶栏。
- **Do** 给连接、容器和插件状态提供可读文字，圆点和颜色只辅助识别。
- **Do** 保持键盘焦点可见，关闭对话框后恢复发起控件，保存草稿时保留输入选区。
- **Do** 使用内联 SVG 功能图标，图标按钮提供可读名称。
- **Do** 把服务器、插件管理、设置、消息和日志入口放在核心导航区域，保留窄侧栏给插件。

### Don't:

- **Don't** 把三主题改成三套页面结构或用颜色替代状态文字。
- **Don't** 在深色选中底和对话框上使用较暗的普通次要文字；这些上下文采用既有 Gray10。
- **Don't** 将原型演示结果写成正式应用已经交付的能力或原生窗口行为。

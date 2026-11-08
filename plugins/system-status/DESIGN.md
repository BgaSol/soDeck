---
name: "SoDeck 系统状态"
description: "当前服务器的紧凑数据面，沿用宿主三主题。"
colors:
  light-blue: "#245b9a"
  light-green: "#23734f"
  light-focus: "#7661d1"
  light-warning: "#8a590f"
  light-error: "#b53950"
  light-background: "#f8f9fb"
  light-panel: "#fff"
  light-text: "#242936"
  light-muted: "#586174"
  light-border: "#dce1eb"
  light-hover: "#eff0f5"
  light-selected: "#f0ecfc"
  dark-blue: "#86b5ff"
  dark-green: "#95d5b4"
  dark-focus: "#b3a2f7"
  dark-warning: "#eac386"
  dark-error: "#f1a4b6"
  dark-background: "#1e2028"
  dark-panel: "#22252e"
  dark-text: "#e2e5ee"
  dark-muted: "#b3bcd0"
  dark-border: "#394356"
  dark-hover: "#30343f"
  dark-selected: "#353047"
  dopamine-blue: "#245b9a"
  dopamine-green: "#176b4f"
  dopamine-focus: "#5346a6"
  dopamine-warning: "#7a4a08"
  dopamine-error: "#a12c38"
  dopamine-background: "#fffbe6"
  dopamine-panel: "#fffbe6"
  dopamine-text: "#20211f"
  dopamine-muted: "#585c52"
  dopamine-border: "#20211f"
  dopamine-hover: "#fff0bc"
  dopamine-selected: "#ffde75"
typography:
  body: { fontFamily: 'system-ui, -apple-system, "Segoe UI", sans-serif', fontSize: "13px", fontWeight: 400, lineHeight: 1.5 }
  title: { fontFamily: 'system-ui, -apple-system, "Segoe UI", sans-serif', fontSize: "13px", fontWeight: 650, lineHeight: 1.4 }
  host-title: { fontFamily: 'system-ui, -apple-system, "Segoe UI", sans-serif', fontSize: "14px", fontWeight: 650, lineHeight: 1.5 }
  metric: { fontFamily: 'system-ui, -apple-system, "Segoe UI", sans-serif', fontSize: "25px", fontWeight: 600, lineHeight: 1.15, letterSpacing: "-0.025em" }
  network: { fontFamily: 'system-ui, -apple-system, "Segoe UI", sans-serif', fontSize: "18px", fontWeight: 600, lineHeight: 1.3 }
  label: { fontFamily: 'system-ui, -apple-system, "Segoe UI", sans-serif', fontSize: "11px", fontWeight: 400, lineHeight: 1.5 }
  disclosure: { fontFamily: 'system-ui, -apple-system, "Segoe UI", sans-serif', fontSize: "12px", fontWeight: 500, lineHeight: 1.5 }
  path: { fontFamily: "Consolas, ui-monospace, monospace", fontSize: "11px", fontWeight: 600, lineHeight: 1.5 }
rounded:
  control: "5px"
  tooltip: "4px"
  meter: "2px"
spacing:
  section-inset: "6px"
  field-gap: "8px"
  detail-inset: "12px"
  header-inset: "16px"
  content-inset: "18px"
  narrow-content-inset: "11px"
components:
  status-action: { backgroundColor: "transparent", textColor: "{colors.light-muted}", rounded: "{rounded.control}", padding: "0", size: "27px" }
  page-button: { backgroundColor: "{colors.light-panel}", textColor: "{colors.light-text}", rounded: "{rounded.control}", padding: "3px 7px", height: "28px" }
  interface-select: { backgroundColor: "{colors.light-panel}", textColor: "{colors.light-text}", typography: "{typography.label}", rounded: "{rounded.control}", padding: "2px 5px" }
  metric-section: { textColor: "{colors.light-text}", padding: "6px 0" }
  filesystem-meter: { rounded: "{rounded.meter}", height: "4px" }
  detail-summary: { textColor: "{colors.light-text}", typography: "{typography.disclosure}", height: "39px" }
---

# Design System: SoDeck 系统状态

## Overview

**Creative North Star: "技术期刊数据图表"**

这份规范仅适用于系统状态插件的 ui/，记录正式实现中的视觉规则。它采用用户确认的“技术期刊数据图表”语言：紧凑、平静，让当前值、近期曲线和辅助说明沿同一连续数据面排列。

宿主提供浅色、深色和多巴胺主题，插件共用结构和指标语义。数据来自当前服务器的实际采样；读取中、不可用、过期与断线各有文字，详情按需展开。

**Key Characteristics:**

- 连续分区，数值、曲线与时间对齐。
- 三主题继承宿主 token，指标类别保持一致。
- 保留真实历史与缺口，来源和规格按需展开。

## Colors

前置数据记录宿主已实现的三主题值，来自 ../../src/themes.css；蓝色对应 terminal-blue，绿色对应 good，其余名称对应同名语义变量。ui/view.js 在挂载和主题变更时写入宿主 token；ui/style.css 的独立打开回退色不是另一套正式色板。组件 token 描述浅色默认态，实际主题由 CSS 变量继承。sidecar 色阶仅用于色板预览。

### Primary

- **指标蓝**：CPU 曲线，接收图例与曲线。
- **焦点紫**：键盘焦点轮廓与控件按下反馈，沿用宿主 accent。

### Secondary

- **指标绿**：内存与发送曲线、正常容量条，以及连接和采集的正常状态点。

### Tertiary

- **注意琥珀**：空间偏紧、暂停、过期与断线说明。
- **错误红**：操作或采集失败说明、断线状态点；状态文字同时保留。

### Neutral

- **背景与面板**：数据面使用 file-tree-bg，控件使用 panel。
- **文字与参考线**：text 承担标题和读数，muted 承担单位、时间与说明；分区线和曲线网格分别将 border 混合为（75% / 45%）。
- **控件反馈**：一般按钮与选择器使用 hover；一般按钮按下和文本选择使用 accent-bg。

**The Series Identity Rule.** CPU 与接收共享蓝色；内存与发送共享绿色。主题改变颜色值，不改变对应关系。

## Typography

界面使用前置数据中的系统字体栈，路径使用等宽字体。全部数字继承 tabular-nums；没有展示性标题字阶。

- **标题与身份**：分组标题（13px / 650），主机名（14px / 650）。
- **当前值**：CPU 与内存（25px / 600）；百分比单位（13px / 500），使用辅助文字色。
- **双列速率**：网络值（18px / 600），单位与图例（11px）；展开的磁盘读写值降为（13px）。
- **说明与详情**：正文（13px），展开标题（12px / 500），元数据（11px），挂载路径（11px / 600）。长名称和路径允许断行。

**The Numeric Alignment Rule.** 读数、容量和时间使用等宽数字；单位退到辅助字号，标题不与当前值争夺权重。

## Layout

顶部标题与操作区（最小 40px）及底部采集状态（最小 31px）位于滚动区外；中间独立纵向滚动。内容水平留白（18px），指标分区上下留白（6px），双向速率共用两列和（8px）间距。CPU / 内存曲线高（30px），网络曲线高（22px），时间说明紧邻曲线。

容器宽度不超过（240px）时，内容留白改为（11px），标题和页脚留白改为（10px）；主值降至（22px）、网络值至（15px），最小文字仍为（11px）。隐藏右侧曲线刻度和展开摘要，单位可换行，设备详情转为单列。正式宿主提供（160–480px）侧栏宽度；不继承原型外部演示窗口的断点。

## Elevation & Depth

本数据面不使用阴影。主题背景、细分区线与文字层级表达结构。标题和页脚、控件边框继承宿主结构/控件描边：浅色和深色（1px），多巴胺（2px）；内部数据分区仍为（1px）。采样提示以 text 作底、panel 作字。焦点轮廓使用 accent（2px），外移（2px）。

**The Continuous Section Rule.** 状态指标在同一滚动面中以细线分组；层次来自对齐、字阶与间距。

## Shapes

小圆角服务于原生控件（5px）、采样提示（4px）和容量条（2px）。数据分区没有独立容器轮廓。图标使用内联线性 SVG，默认描边（1.6），圆头与圆连接。

## Components

### 操作与选择器

刷新和暂停为紧凑方形图标按钮（27 × 27px），图标（15px），背景与边框透明；保留 title、可访问名称和焦点轮廓，暂停后切换为恢复。分页按钮最小高度（28px），禁用透明度（0.5）。一般按钮和原生 select 的悬停/按下反馈见 Colors；顶部透明图标按钮不填充这两种背景。按钮颜色过渡沿用 shared.css（100ms ease-out）。

接口选择器使用原生 select（11px，2px 5px 内边距），在标题行最小宽度（64px）、最大宽度（55%）；设备选择器不超过所在行（75%）。选择同步当前值与所选设备的历史，设备不可用时禁用。

### 趋势与数据状态

当前值、辅助说明、五分钟曲线和时间说明构成一组。CPU / 内存使用（0–100）百分比刻度，网络按所选接口历史峰值缩放；曲线描边（1.6），仅画真实折线。指针、方向键和 Home / End 查看采样，Escape 隐藏提示；缺口断线，未知值明确标注，不补零。

暂停、断线或过期保留最后读数，曲线变为浅色虚线；历史窗口仍随当前时间前移，五分钟外的曲线退出，当前旧值与成功时间继续说明。首次读取无有效数值时显示采样中或不可用，不展示百分比刻度。失败说明持续保留，不因设备切换、分页或展开而消失；各采集组保留各自成功时间。

### 挂载容量

每行显示挂载点、已用 / 总量、容量条、可用量与百分比。容量条为只读 meter（4px），仅在使用率已知时出现；达到（85%）使用注意色和“空间偏紧”文字。未知容量使用明确文本，大容量按二进制单位表示；多挂载分页，不堆成独立卡片。

### 按需详情与提示

磁盘 I/O、设备信息和采集详情使用原生 details；summary 最小高度（39px），箭头打开后旋转（90°），过渡（180ms ease-out）。正文退为辅助字号，宽面板名称 / 数值双列、窄面板单列。减少动态效果时关闭过渡；强制颜色模式使用系统曲线色，发送线增加虚线，容量条使用 Highlight。

状态提示使用小圆角、说明文字和注意/错误色的（10%）混合底色，位于身份摘要之后；页脚持续给出动态采样状态与成功时间。

## Do's and Don'ts

### Do:

- Do 继承宿主三主题的颜色与描边 token，保持曲线和图例的类别对应。
- Do 显示明确单位、容量分母和成功采样时间；注意与错误状态同时使用文字。
- Do 使用原生 select、details 和只读 meter，保留可访问名称、键盘焦点与减少动态效果设置。

### Don't:

- Don't 用零值代替未知数据，或用连续线跨过采样缺口。
- Don't 用新的数据卡片、阴影或装饰动效替代当前连续分区。
- Don't 将原型的独立色值或宿主其他组件规则扩展为本插件规范。

未规范化：ui/style.css 中未被 ui/charts.js 创建的 area 样式不作为填充图表规则；本次没有发现需要另行继承的视觉缺陷，也未修改 UI。

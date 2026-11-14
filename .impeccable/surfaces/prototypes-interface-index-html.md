---
version: 1
slug: "prototypes-interface-index-html"
primary_target: "prototypes/interface/index.html"
related_targets: ["prototypes/interface/app.js","prototypes/interface/style.css"]
---

# SoDeck 界面原型

面向多服务器运维人员的 Operate 桌面原型；入口为 `prototypes/interface/index.html`。现有正式功能文档是业务边界，全部示例数据为合成数据。先评审原型，再接入正式前端。

## Direction contract

THESIS: 服务器会话是操作上下文，插件页面是会话内的工作对象；核心管理台与会话工作台在同一主区域切换，底部连接身份始终可见。左右窄侧栏只在工作台供插件使用，终端通过工作页签使用；管理台隐藏插件侧栏，核心功能在完整管理台内导航。

OWN-WORLD: 用户已指定 IDEA 默认明暗配色，保留原多巴胺主题。明暗主题使用 JetBrains 官方 Gray/Blue 系列；多巴胺保留粉色会话栏、黄色选择和绿色工具区域。使用系统界面字体、代码等宽字体、紧凑标准桌面控件。

STORY: 点击顶部 SoDeck 进入管理台的服务器连接页，沿显式路径树选择节点，用标签 AND 和搜索定位目标并保留祖先，核对指纹并连接，在同一会话切换终端、文件及容器；管理台也承载插件、设置、消息、日志、状态与更新，返回会话时保留草稿。

FIRST VIEWPORT: 44px 顶栏含统一 SoDeck 管理台入口、服务器会话及快捷动作，底栏 26px 显示连接。管理台使用 192px 功能导航和完整页面，默认展示服务器连接；会话工作台使用 48px 左插件入口栏、240px 文件树、中央工作标签、272px 状态区及右插件工具栏。窄窗的管理台导航移到内容上方并换行，工作台工具通过入口展开，分屏改为上下布局。

SIGNATURE INTERACTION: 连接节点使用 6px 圆角和两行布局，首行为名称、协议及状态，第二行为账号地址与自身标签；默认最小高 52px、紧凑密度 48px、间隔 4px、桌面缩进 20px，窄窗缩进 16px。本机起点为单行，长内容省略并通过悬停标题与详情展示。真实连线描绘共享显式路径前缀。选择节点高亮完整祖先路径；筛选只匹配目标且自动展开祖先，清除后恢复折叠。非命中祖先标为路径节点，不能单独连接。右侧优先展示完整路线；路线编辑器按序添加、移动、移除 SSH/SOCKS5 步骤，保留草稿与焦点。节点操作即时反馈，不增加自动播放的装饰动画。

CONNECTION TOOLBAR: 内容区直接展示资料类型、搜索、管理标签、刷新和新增，不重复管理台已表达的大标题、教程和独立分类标签行。资料类型选择同步搜索名称及新增动作，服务器和 SOCKS5 代理的原有行为保留。

NARROW WINDOW: ≤480px 工具栏上行资料类型和新增，下行搜索、管理标签和刷新。标签默认收起，保留 AND 规则、选择数量与清除；进入管理台显示完整选中目标。详情模式隐藏目录控件，返回列表恢复。

MODEL BOUNDARY: 正式连接服务的路线是显式有序引用，不递归展开被引用跳板的路线。多标签目前只在原型合成数据中；正式模型/API 和五平台包验收留给后续实际重构。

FILE PLUGIN BOUNDARY: 文件插件仅作视觉优化，操作逻辑与展示逻辑以现有正式实现为准；保留范围统一维护于[重构计划](../../docs/plans/interface-refactor.md#目标与边界)。简化文件原型不替代现有交互规范。

FORM: 用户指定的 IDEA 桌面工具配色与操作惯例优先；视觉来源只贡献配色、字号密度和反馈，不改变产品能力。SEED REFERENCE seed:011b12831303aff1；本轮用户指定方向覆盖抽取方向。

FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance

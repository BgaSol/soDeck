# 公共界面源码

核心直接导入 shared.css 和 themes.css；独立插件打包时复制 shared.css + legacy.css 到 ui/shared.css，运行时通过 UI v1/v2 token 消息获得主题。颜色只定义在 themes.css，ID/名称/明暗属性只登记在 themes.json，对外 token 名只维护在 token-names.ts。新增主题登记一项并补齐 CSS 与主题验收。

核心打包与终端独立 UI 构建共用 package.mjs 的 packageStyles，生成自包含 CSS，不依赖核心安装目录。sd-link 与 sd-primary 分别表达文字链接和主按钮背景，避免明暗主题以同一颜色兼任两种对比度要求。

已发行 token 的名称和几何语义保持兼容。核心尺寸只在 #app 作用域声明，不下发给插件。legacy.css 冻结旧 pathbar/tree-menu/primary 选择器；所有正式插件迁移到 sd-* 并退出旧版本支持后才删除。核心不导入 legacy.css。

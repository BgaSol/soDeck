# Docker 变更记录

## 0.1.1（2026-11-10）

- Docker 默认固定在右侧工具栏，仍按需展开；已有布局偏好保留，可通过“恢复推荐布局”回到右侧。
- 工作台入口、插件列表和组件设置使用标准鲸鱼图标；容器详情和日志页使用宿主支持的对应图标。请配套更新核心至 0.20.1。

发行来源：

- 插件版本：0.1.1，核心 API 1、UI API 2；配套核心 0.20.1。
- 包来源提交：[`e6aa6c1c35ca0d67f10bcec5e4ca65de0f7bdd32`](https://github.com/BgaSol/soDeck/commit/e6aa6c1c35ca0d67f10bcec5e4ca65de0f7bdd32)。
- 原始打包：[Package plugin #42748870815](https://github.com/BgaSol/soDeck/actions/runs/42748870815)，五个原生目标的构建、归档解包、包内进程协议与退出全部通过。
- 源码与业务：[Check #42748827332](https://github.com/BgaSol/soDeck/actions/runs/42748827332)，五平台源码、公共前端/脚本、Windows 完整 UI/SSH、Linux 普通网络及 x86_64 Docker Engine/OpenSSH、UI 和归档回归全部通过；netem 跳过。
- Release：[Docker 0.1.1](https://github.com/BgaSol/soDeck/releases/tag/plugin-docker-v0.1.1)，按本次要求设为 GitHub Latest。

本机下载本次五份草稿归档，核对实际文件 SHA-256、GitHub 摘要和更新清单，检查各包内 0.1.1 版本、右侧默认与鲸鱼图标声明，并运行下载的 Windows x86_64 程序验收通过。没有重建已通过的包，也没有覆盖 0.1.0 正式附件。原生 WebView 人工交互、多屏/DPI、用户业务服务器和历史版本升级不在本次专项范围。

## 0.1.0（2026-11-10）

- 新增容器侧栏、主区域详情和最近日志三个独立视图，支持现有工作台的分屏与移窗协议。
- 支持容器搜索、状态/健康筛选、Compose 项目排序、隐藏环境变量值、日志快照搜索与复制。
- 支持单容器启动、停止、重启、暂停、恢复及停止后的删除，保留挂载卷。
- 支持按需资源快照；复用现有短命令能力，中断操作不自动重发。

远端先支持 Linux Docker Engine，不包含容器终端、实时日志跟随、文件导出、批量操作和镜像/网络/卷管理。日志两路输出不保证严格全局顺序；取消本地等待不会回滚已发送操作。本机行为与详细边界见[实施记录](../../docs/implementation-docker-plugin.md)。

发行来源：

- 插件版本：0.1.0，核心 API 1、UI API 2。
- 包来源提交：[`d32b60a798f593df5a3b89d4a5807cc12d1bc546`](https://github.com/BgaSol/soDeck/commit/d32b60a798f593df5a3b89d4a5807cc12d1bc546)。
- 原始打包：[Package plugin #42741187586](https://github.com/BgaSol/soDeck/actions/runs/42741187586)，五个原生目标的构建、归档解包、包内进程协议和退出全部通过。
- Release：[Docker 0.1.0](https://github.com/BgaSol/soDeck/releases/tag/plugin-docker-v0.1.0)，本次按用户要求标记 GitHub Latest。
- 源码与业务：[Check #42741108087](https://github.com/BgaSol/soDeck/actions/runs/42741108087)。五平台核心/插件源码及公共检查通过；Linux Docker Engine、UI 与归档业务验证通过。首轮 Windows 主界面因官方名单固定为四项失败，`369000d` 修正测试预期。[desktop 补测 #42742706703](https://github.com/BgaSol/soDeck/actions/runs/42742706703) 两次暴露既有本地添加流程与后台刷新重叠时的焦点竞态；`1c71367` 增加三行核心前端焦点恢复和稳定回归，[Check #42745621259](https://github.com/BgaSol/soDeck/actions/runs/42745621259) 的 Windows 完整 UI 和公共检查全部通过。Docker 插件运行代码未改变，继续复用原始包。

本次另外下载草稿的五份归档和更新清单，核对大小、GitHub 摘要、清单五平台 URL/SHA-256，并在 Windows x86_64 运行下载归档内的真实进程验收。原生 WebView 手势、多屏/DPI、用户业务服务器及安装升级没有在本次验证。核心仍为既有 0.20.0，没有重发核心安装包。

上述本地添加焦点修正已进入 main，将在下次核心发行生效；当前 0.20.0 极快完成本地选择时可能需要重新按 Tab 或点击详情控件。官方插件安装流程和 Docker 业务不依赖该修正。

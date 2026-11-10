# Docker 变更记录

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

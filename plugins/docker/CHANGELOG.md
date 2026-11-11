# 0.1.4

- 插件客户端使用标准库有界逐行读取，保留超长控制帧立即退出，Interrupted 自动重试；远端操作和视图生命周期保持原有契约。

未新增或升级生产依赖，核心 API 与 RPC 字段保持兼容。

包来源：[90d49f4c7ef509f89025696a71ddddebfec99d51](https://github.com/BgaSol/soDeck/commit/90d49f4c7ef509f89025696a71ddddebfec99d51)；原始独立打包与验收：[43083474805](https://github.com/BgaSol/soDeck/actions/runs/43083474805)。Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 五平台原生归档的架构、摘要、资源、初始化、视图创建/释放与退出全部一次通过。

[来源 Check](https://github.com/BgaSol/soDeck/actions/runs/43083351828) 的五平台源码、真实 Docker Engine/进程和真实 CSSOM 三主题界面回归通过。

发行：[docker 0.1.4](https://github.com/BgaSol/soDeck/releases/tag/plugin-docker-v0.1.4)，非 Latest。六个附件沿用原始 run，发布前只同步说明与更新清单的验收记录，五份程序归档字节保持不变。未执行本版历史版本升级、五平台远程包组合、netem、生产服务器或真实 IME/DPI/Wayland 专项。

# 0.1.3

- 构建与实际归档验收分任务执行，失败复验保留原包。
- 原生归档统一检查资源、初始化、视图生命周期和退出；发布前绑定来源与摘要，拒绝覆盖不同内容的同版本附件。
- 插件 API 与在线更新协议保持兼容。

包来源：[a52157346409d4457c4b00b5bbd3364df86df36c](https://github.com/BgaSol/soDeck/commit/a52157346409d4457c4b00b5bbd3364df86df36c)；原始打包与验收：[42914278659](https://github.com/BgaSol/soDeck/actions/runs/42914278659)。Windows x86_64/ARM64、Linux x86_64/ARM64、macOS ARM64 全部原生构建，最终归档的摘要、资源、初始化、视图释放和退出通过；上传与验收独立执行。

[来源提交 Check](https://github.com/BgaSol/soDeck/actions/runs/42914202961) 全部通过，包含五平台核心/插件/共享客户端源码测试及相关浏览器、SSH/SFTP、终端、网络、Docker、通信回归。首次草稿索引延迟导致收尾失败，重跑 release 成功，未重编译或覆盖包。

发行：[docker 0.1.3](https://github.com/BgaSol/soDeck/releases/tag/plugin-docker-v0.1.3)，非 Latest。[真实正式版升级 0.1.2 → 0.1.3](https://github.com/BgaSol/soDeck/actions/runs/42919823620) 五平台通过，宿主固定为核心 0.23.1，验证下载校验、升级前原进程保留、重启激活及设置/服务器资料保留。没有将源码或包生命周期测试算作生产服务器、真实 IME/DPI/Wayland 或所有设备兼容验收。

# 0.1.2

命令输出使用标准严格 Base64 解码；非法输入明确报错，失败后释放原任务，不重复执行 Docker 操作。

独立分发；启用中的插件升级在重启后生效，核心安装包不包含插件。

包来源：[0abaf3e8ccc2045e437dac723d898eca87759f6c](https://github.com/BgaSol/soDeck/commit/0abaf3e8ccc2045e437dac723d898eca87759f6c)。原始五平台打包：[42779070485](https://github.com/BgaSol/soDeck/actions/runs/42779070485)，均使用原生产物，包内进程启动、生命周期和退出通过。

[Check 42807748154](https://github.com/BgaSol/soDeck/actions/runs/42807748154) 复验相同插件业务源码的五平台测试和真实业务；后续核心修复不修改这份插件程序。下载原五份归档及清单，核对版本、原生架构、SHA-256 和更新地址；Windows x86_64 原归档再次通过实际包进程测试。直接发布原六个附件。

未重复历史版本在线升级、用户生产服务器或全平台人工界面操作。正式发行：[docker 0.1.2](https://github.com/BgaSol/soDeck/releases/tag/plugin-docker-v0.1.2)，非 Latest。

2026-11-11 随核心 0.23.0 发行再次核对：0.1.2 继续为 Docker 最新正式版本，运行源码、共享客户端与打包输入未变，保留原始包来源和五平台验收。下载五份正式归档并核对 SHA-256、更新清单、包内版本、UI 与原生架构；原 Windows x86_64 程序的实际进程回归通过。[Check #42885519869](https://github.com/BgaSol/soDeck/actions/runs/42885519869) 与 [Docker 补测 #42887442456](https://github.com/BgaSol/soDeck/actions/runs/42887442456) 再验相同 Docker 源码的五平台测试及 Linux Engine/OpenSSH 普通业务。没有重新构建、覆盖或另升空版本；Docker 不在本轮四组件远程包组合入口内。

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

远端先支持 Linux Docker Engine，不包含容器终端、实时日志跟随、文件导出、批量操作和镜像/网络/卷管理。日志两路输出不保证严格全局顺序；取消本地等待不会回滚已发送操作。本机行为与详细边界见[实施记录](https://github.com/BgaSol/soDeck/blob/8115fdcd5bed4108021a729da605fbde77afc2af/docs/implementation-docker-plugin.md)。

发行来源：

- 插件版本：0.1.0，核心 API 1、UI API 2。
- 包来源提交：[`d32b60a798f593df5a3b89d4a5807cc12d1bc546`](https://github.com/BgaSol/soDeck/commit/d32b60a798f593df5a3b89d4a5807cc12d1bc546)。
- 原始打包：[Package plugin #42741187586](https://github.com/BgaSol/soDeck/actions/runs/42741187586)，五个原生目标的构建、归档解包、包内进程协议和退出全部通过。
- Release：[Docker 0.1.0](https://github.com/BgaSol/soDeck/releases/tag/plugin-docker-v0.1.0)，本次按用户要求标记 GitHub Latest。
- 源码与业务：[Check #42741108087](https://github.com/BgaSol/soDeck/actions/runs/42741108087)。五平台核心/插件源码及公共检查通过；Linux Docker Engine、UI 与归档业务验证通过。首轮 Windows 主界面因官方名单固定为四项失败，`369000d` 修正测试预期。[desktop 补测 #42742706703](https://github.com/BgaSol/soDeck/actions/runs/42742706703) 两次暴露既有本地添加流程与后台刷新重叠时的焦点竞态；`1c71367` 增加三行核心前端焦点恢复和稳定回归，[Check #42745621259](https://github.com/BgaSol/soDeck/actions/runs/42745621259) 的 Windows 完整 UI 和公共检查全部通过。Docker 插件运行代码未改变，继续复用原始包。

本次另外下载草稿的五份归档和更新清单，核对大小、GitHub 摘要、清单五平台 URL/SHA-256，并在 Windows x86_64 运行下载归档内的真实进程验收。原生 WebView 手势、多屏/DPI、用户业务服务器及安装升级没有在本次验证。核心仍为既有 0.20.0，没有重发核心安装包。

上述本地添加焦点修正已进入 main，将在下次核心发行生效；当前 0.20.0 极快完成本地选择时可能需要重新按 Tab 或点击详情控件。官方插件安装流程和 Docker 业务不依赖该修正。

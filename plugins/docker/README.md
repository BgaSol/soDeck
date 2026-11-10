# Docker

SoDeck 外置插件。复用当前服务器认证会话，通过远端 Docker CLI 管理 Linux Docker Engine，不需要额外保存连接凭据。

默认从工作台右侧鲸鱼图标打开 Docker，筛选容器或按 Compose 项目排序，双击容器在主区域打开详情。详情可查看配置、环境变量、端口、挂载、网络和健康状态，并打开最近日志；环境变量值默认隐藏。日志搜索覆盖已缓存快照，复制当前显示页。已有布局偏好继续保留，可在组件菜单选择“恢复推荐布局”回到右侧。

启动、停止、重启、暂停、恢复和删除均需确认。只删除已停止容器，保留挂载卷；容器可写层会被删除。停止/重启宽限为 10 秒，每条命令最多等待 30 秒。取消等待或连接中断不会回滚远端操作，结果未确认时先刷新核对，再决定下一次操作。

```sh
npm run plugin:build -- --plugin docker
npm run docker:process:test
npm run docker:ui:test
npm run docker:package:test
npm run docker:test
```

在设置 → 插件管理 → 官方插件刷新列表，选择 Docker 安装并重启。也可以本地构建后添加 `artifacts/plugins/sodeck-docker_0.1.1_<target>`。需要核心提供 `session.exec.v1`、UI v2、`ui.openView.v1` 和 `settings.v1`；鲸鱼图标需要核心 0.20.1。发行来源使用 `plugin-docker-v` 标签，正式包见[Docker 0.1.1](https://github.com/BgaSol/soDeck/releases/tag/plugin-docker-v0.1.1)。

远端需要当前用户能直接运行 Docker CLI 并访问 Unix socket，默认 `unix:///var/run/docker.sock`。Rootless Docker 可在浏览器的连接选项指定其他绝对 socket 地址；先关闭同服务器的详情与日志页并等待请求完成。不自动安装 Docker、使用 sudo 或修改用户组。

第一版不含容器终端、实时日志跟随、导出文件、批量操作或镜像/网络/卷管理。资源指标按需读取，详情与日志手动刷新；容器列表按组件设置的间隔刷新。具体配额、测试证据和平台验收边界见[实施记录](../../docs/implementation-docker-plugin.md)，变化见[CHANGELOG](CHANGELOG.md)。

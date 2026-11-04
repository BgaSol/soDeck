# 外置插件 API v1

插件是独立原生程序，核心不需要识别具体插件 ID。插件按可信扩展使用，没有权限清单、授权或签名流程。

## 目录与清单

解压后选择含 `plugin.json` 的目录：

```json
{
  "id": "sodeck.hello",
  "name": "Hello 插件",
  "version": "0.1.0",
  "apiVersion": "1",
  "target": "x86_64-pc-windows-msvc",
  "entry": "bin/sodeck-hello.exe"
}
```

每个包包含一个目标的程序，五个 target 见分发文档。Unix 入口需要执行位。核心只做必需字段、版本、API 主版本、目标及目录内入口的基本检查。同一 ID 只能有一个活动实例；卸载后可重新选择新目录替换记录。

## 通信

stdin/stdout 每行一个 UTF-8 JSON-RPC 2.0 对象，以 LF 结尾，最大 64 KiB。stdout 只输出协议，stderr 作为诊断日志。请求必须带 ID，响应使用相同 ID；通知省略 ID。参数错误返回 `-32602`，未知方法返回 `-32601`；持久化等执行失败也返回 error，调用方应检查响应，不根据日志推测调用成功。

核心发起：

```json
{"jsonrpc":"2.0","id":"initialize","method":"initialize","params":{"coreApiVersion":"1","dataDirectory":"<系统应用数据目录>/plugin-data/sodeck.hello"}}
{"jsonrpc":"2.0","id":"initialize","result":{"apiVersion":"1"}}
```

核心等待初始化 5 秒。插件收到初始化响应后可调用以下方法。插件不能自行声称来源，核心附加 pluginId、instanceId、receivedAt。

```json
{"jsonrpc":"2.0","id":1,"method":"logging.write","params":{"level":"info","message":"服务已启动","fields":{"port":8080}}}
{"jsonrpc":"2.0","id":1,"result":{"accepted":true}}
{"jsonrpc":"2.0","id":2,"method":"messages.publish","params":{"title":"服务就绪","body":"现在可以连接","level":"success","notify":true,"dedupeKey":"service-ready"}}
{"jsonrpc":"2.0","id":2,"result":{"id":"<核心分配的消息 ID>"}}
```

日志级别：debug/info/warn/error。全局每秒最多接收 200 条插件日志，超量返回 accepted=false，并合并记录丢弃数量。单条日志最多 8 KiB，超长正文截断，过大 fields 移除。日志文件每份最多 10 MiB，保留 5 份；内存回退最多 5000 条，界面查询最多 500 条。普通 error 日志不自动弹通知。

消息级别：info/success/warning/error，notify 默认 false；消息持久化成功才返回 ID。最多保留 1000 条。同一插件非空 dedupeKey 在 5 分钟内合并，更新内容、实例、时间和次数并标为未读；合并期间不重复弹提示。标题最多 1024 字节，正文最多 16 KiB。

卸载时核心发送：

```json
{"jsonrpc":"2.0","id":"shutdown","method":"shutdown","params":{}}
{"jsonrpc":"2.0","id":"shutdown","result":{}}
```

插件应结束任务、发送收尾日志并退出。核心在 3 秒内继续处理收尾 RPC；超时结束进程组/Windows Job 中的进程，并确认主进程结束。停止期间不能创建新业务任务。stdin 关闭时插件也应自行退出。

当前没有插件业务视图、SSH 或代理 API。新增可选参数保持 API v1；不兼容变化增加 API 主版本。

## 本地构建 Hello

```sh
npm run plugin:build
# 指定目标时仍需要相应编译环境
npm run plugin:build -- --target aarch64-apple-darwin
# 已加载的插件不能原位覆盖；可卸载后构建，或使用另外的输出目录
npm run plugin:build -- --out-dir artifacts/another-build
```

产物位于 `artifacts/plugins/`，包含清单和原生入口的目录与压缩归档。构建脚本在 Hello 工程目录调用 Cargo，使其 Windows 静态运行库配置生效；归档前检查原生架构和额外的 Visual C++ 运行库依赖。Hello 自己的版本只在 `plugins/hello/Cargo.toml` 中修改，独立于核心版本；用户运行时不需要 Rust、Node.js 或额外安装 Visual C++ 运行库。

# 插件包开发与导入

T-028 · API/SDK 6.2 · 更新日期：2026-10-29

SoDeck 插件使用 `.sodeck-plugin` 文件，本质是带清单的 ZIP 资源包，由程序导入；不是 EXE/MSI 安装器。现有 SDK 是仓库 workspace，尚未作为完整第三方脚手架发布到 npm。

一个已编译目录至少包含 `manifest.json`、声明的 UI 入口及 worker 入口。UI 使用独立 WebView；worker 在独立受限 JavaScript 进程执行，通过公开 SDK 调用核心。业务代码与资源随插件独立更新，不支持随包安装任意原生扩展。

```text
dist/
  manifest.json
  ui/index.html
  ui/assets/…
  worker/main.js
  settings/…        # 声明设置时需要
  diagnostics/…     # 声明观测描述时需要
```

清单字段和贡献以[插件协议](technical/plugin-protocol.md)及仓库 `plugins/*/manifest.json` 为例。`pluginId` 使用稳定的小写标识，版本使用 semver；UI/worker 入口必须为包内相对路径。第三方无签名包要求 `requires.pluginApi` 的最低版本不低于 6.2.0，并包含 `plugins.optional-signature.v1` 能力。其他功能需要的能力须同时声明。

在仓库根目录运行：

```powershell
node scripts/pack-plugin.mjs --source C:/my-plugin/dist --signing none --out C:/my-plugin/demo.sodeck-plugin
```

只提供 `--source` 时，工具从该目录读取 manifest，默认不签名，不读取官方插件源目录、不生成签名密钥。清单和可选签名在输出包中重新生成，源目录保持不变。`--plugin terminal` 等官方流程仍默认使用开发签名；自有 Ed25519 密钥使用 `--signing key --key C:/private/key.pem --key-id publisher-key-1`。包内密钥不会被核心自动加入信任集合。

`integrity.json` 必须存在，并准确列出全部内容文件的 SHA-256。缺失、额外文件、重复清单键、哈希不符或非法路径均拒绝加载。签名可省略：未签名包显示“未签名”，未知 keyId 显示“签名来源未验证”，两者完整性及兼容检查通过即可加载；已知公钥验签失败或签名格式无效会拒绝。完整性清单本身不证明发布者身份。签名实际策略见[插件管理](core/plugin-management.md)。

资源限制：压缩包最多 32 MiB，展开最多 128 MiB、2000 个文件，worker 源码最多 768 KiB；不允许符号链接、路径穿越、Windows 特殊名称及大小写冲突。包版本不可变，同一 pluginId/version 的文件字节不同会拒绝，应发布新版本。

在“设置 → 插件管理 → 安装或更新插件”选择包。新插件默认启用，准备成功后完整退出并重新启动应用才执行。停用插件的候选会保留但不执行初始化；重新勾选并应用，再完整重启后才加载。导入是独立动作，取消设置不会撤销已准备的包。停用保留包与业务配置，不会删除数据。

旧核心不识别启停配置，回退时应使用升级前的独立目录或备份。勿将已有数据目录交给旧核心并假定停用状态仍有效。没有插件市场、联网自动升级或正式签名密钥分发服务。

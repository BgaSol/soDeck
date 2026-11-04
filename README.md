# SoDeck

当前阶段只做一个显示 **Hello SoDeck** 的桌面应用，先跑通安装和打包，再开始运维功能。

## 本地运行

需要 Node.js 24、Rust 及 [Tauri 系统构建依赖](https://v2.tauri.app/start/prerequisites/)。

```sh
npm ci
npm run dev
```

检查与打包：

```sh
npm run check
npm run build -- --bundles nsis
```

Windows 使用 `nsis`，Linux 使用 `deb`，macOS 使用 `dmg`。
产物在 `src-tauri/target/release/bundle/`；指定 `--target` 后则位于 `src-tauri/target/<target>/release/bundle/`。

界面是静态 HTML，Rust 只有 Tauri 窗口入口。无业务插件、服务器连接或自动更新。程序图标沿用旧版 SoDeck 的品牌资源。

## GitHub 打包

在 Actions 中选择 **Package SoDeck → Run workflow**，完成后下载对应的 Artifact。支持：

| 系统 | 架构 | 安装包 |
| --- | --- | --- |
| Windows | x86_64、ARM64 | `.exe` |
| Linux | x86_64、ARM64 | `.deb` |
| macOS | ARM64 | `.dmg` |

发布版本只修改 `src-tauri/tauri.conf.json` 中的 `version`，提交后推送匹配的标签，例如：

```sh
git tag v0.1.0
git push origin v0.1.0
```

同一工作流会生成安装包并上传到对应的 Release 草稿。各目标独立执行，一个失败不会取消其他目标。手动运行只生成下载产物，不创建 Release。

发布工作流完成后，在 Releases 检查草稿附件，点击 **Publish release** 即可发布。

工作流还会安装或解包产物，启动包内程序并等待页面加载后退出。这个检查在各目标的原生 Runner 上完成，不能代替真实设备上的界面体验验收。

升级时退出程序，运行新版 Windows 安装包、安装新版 Linux 包或替换 macOS 应用。保持相同应用标识，用户数据目录独立于安装目录。

Windows 安装包未配置商业代码签名；macOS 使用 ad-hoc 签名。构建成功和实际安装测试分别记录，不承诺没有系统来源提示。

## 已验证

2026-11-05，[首次 GitHub 五目标打包](https://github.com/BgaSol/soDeck/actions/runs/41871792829)全部通过：

| 目标 | 安装包 | 包内程序加载页面并正常退出 |
| --- | --- | --- |
| Windows x86_64 | 通过 | 通过，静默安装后启动 |
| Windows ARM64 | 通过 | 通过，静默安装后启动 |
| Linux x86_64 | 通过 | 通过，解包后在虚拟显示器中启动 |
| Linux ARM64 | 通过 | 通过，解包后在虚拟显示器中启动 |
| macOS ARM64 | 通过 | 通过，挂载 DMG 后启动 |

ARM64 构建和上述检查均在原生 ARM64 Runner 执行。本机 Windows x86_64 另外验证了安装、打开窗口、关闭退出和卸载。其他系统尚未做真实设备人工界面验收，覆盖升级尚未做跨版本实测。

`v0.1.0` 的[标签发布工作流](https://github.com/BgaSol/soDeck/actions/runs/41873423553)也已全部通过，五份安装包已上传到 [Release 草稿](https://github.com/BgaSol/soDeck/releases/tag/untagged-2c2b22214ff1e9c8adc8)。其中 Linux x86_64 和 macOS ARM64 在首次尝试长时间停留后取消重试，第二次完成；应用和工作流代码未作修改。草稿尚未正式发布。

## 后续设计

- [产品范围](docs/product.md)
- [架构草案](docs/architecture.md)
- [安装与分发](docs/distribution.md)
- [旧版功能和界面提取](docs/legacy-review.md)

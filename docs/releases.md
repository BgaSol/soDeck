# GitHub 发布流程

代码、Actions 和安装包统一由 GitHub 仓库 https://github.com/BgaSol/soDeck 管理。本地 origin 应指向 https://github.com/BgaSol/soDeck.git。

## 发布新版本

先完成业务改动和正式安装包验收，准备 `docs/release-notes/1.0.0.md` 并提交，确保工作区干净。下一个正式版本从 1.0.0 开始：

```powershell
npm run release:prepare -- 1.0.0
npm run release:check -- v1.0.0
node scripts/release-notes.mjs 1.0.0
git add package.json package-lock.json src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/tauri.conf.json
git commit -m "chore: release v1.0.0"
git tag -a v1.0.0 -m "SoDeck v1.0.0"
git push --atomic origin HEAD refs/tags/v1.0.0
```

执行前按下文核实升级签名及平台资格。此处记录待执行步骤，不表示 1.0.0 已发布；已公开的版本不能覆盖。每次后续发布同时新增对应版本说明文件。

只在本地创建 tag 不会触发 Actions，必须推送到 GitHub。不要用 GitHub 的“Publish release”手工提前发布；让工作流负责上传附件并最后公开。

版本脚本同步 package.json、package-lock.json（含根包）、tauri.conf.json、Cargo.toml、Cargo.lock 中的应用版本，不升级依赖。支持正式版本 vMAJOR.MINOR.PATCH；当前不支持 beta/rc/build-metadata 标签。受 Windows MSI 限制，major/minor 不超过 255，patch 不超过 65535。

## 自动执行的流程

### CI 优化与当前发布平台

普通分支推送只在 Linux 执行前端、Rust、updater/诊断界面以及真实 rsync、Docker 集成检查。Pull request 在 Linux 执行同一套完整检查，并在 Windows x64、Apple Silicon macOS、Linux x64 编译 debug 桌面程序；debug 编译验证跨平台代码，不生成 Release 优化包。tag 发布执行完整 Linux 检查，同时并行构建三个目标平台的 Release 安装包；发布仍须所有检查与构建成功后才会公开。公共测试集中运行一次，平台 job 只承担自身编译或打包。

Intel macOS 从新 CI 构建、Release 附件和应用内升级清单中移除；Intel Mac 不再属于新版本支持范围。此前已发布的 Intel 安装包仍保留在旧 Release 中。

### v0.1.4 发布前验证（2026-10-21）

本版包含本地诊断和文件编辑器界面改进。版本一致性、10 项发布脚本测试、185 项前端单测、前端生产构建、Rust 格式检查、169 项 Rust 测试均通过；40 项依赖外部夹具的 Rust 测试按配置跳过。更新界面 3 项、诊断界面 5 项、Chromium/WebKit 连接与编辑器回归 126 项、正式设置回归 14 项通过。拖拽测试已补充诊断追踪字段断言，解决编辑器实现记录中的 4 项历史失败；正式设置测试的历史依赖下载阻塞也已解除。跨平台安装包由 tag 触发的 GitHub Actions 构建。

1. 推送 v* tag 后，Release 工作流确认 tag 是合法正式版本，并校验所有版本文件一致以及对应版本说明存在、非空且不超过 64 KiB。非法 tag、版本不一致或缺少说明会直接失败。
2. 复用 Desktop checks：三个平台构建；Linux 执行发布脚本、前端与 Rust 检查，并执行真实 SSH/rsync 和 Docker Engine 集成测试。
3. 在各平台生成安装包与 updater 签名，验证文件存在、签名有效且签名版本一致，再保存为 Actions artifacts（保留 14 天）。缺少签名私钥或任一平台构建失败，都不会公开 Release。
4. 汇总五个安装包、一个 macOS 应用升级归档、三个升级签名，生成 latest.json 与 SHA256SUMS.txt；Release 与清单共用 `docs/release-notes/<version>.md`，上传并校验共十一个附件，全部成功后自动公开。升级包不得超过 512 MiB；清单包含 `sodeck.schemaVersion: 1` 与五种手动安装附件映射。
5. 已公开的同名 Release 不允许覆盖；稳定版发布全局串行执行，版本必须高于全部已公开正式版本。上传中断留下的草稿可以在同一提交上重试。
6. 公开后探测固定 latest.json 入口，核对版本、schema、手动附件、各平台 URL 及签名。探测失败会将本次运行标为失败，但此时 Release 已公开。

普通分支 push 和 pull request 只执行检查及编译，不发布 Release。tag 的检查由 Release 内部调用，不会额外再启动一套独立的 Desktop checks。

## 下载内容

新版本 Release 包含：

| 平台                | 附件                                                               |
| ------------------- | ------------------------------------------------------------------ |
| Windows x64         | SoDeck_<版本>_windows_x64-setup.exe、SoDeck_<版本>_windows_x64.msi |
| macOS Apple Silicon | SoDeck_<版本>_macos_arm64.dmg                                      |
| Linux x64           | SoDeck_<版本>_linux_x64.deb、SoDeck_<版本>_linux_x64.AppImage      |
| 文件校验            | SHA256SUMS.txt                                                     |

下载页：https://github.com/BgaSol/soDeck/releases

执行记录：https://github.com/BgaSol/soDeck/actions

Linux 包在 Ubuntu 24.04 构建，面向 Ubuntu 24.04 或更新的兼容环境；AppImage 不代表兼容所有旧版 Linux。下载 AppImage 后需添加执行权限。

已发布的旧版本可能包含 Intel macOS 安装包；新版本只支持 Apple Silicon macOS。当前没有配置 Windows 商业代码签名。Apple 凭据全部缺失时仍采用 ad-hoc 签名；配置完整 Developer ID 与公证凭据后才进入正式签名流程。安装时系统可能显示发布者或安全提示。

1.0+ 客户端只对 Windows x64 NSIS、Linux x64 AppImage 开放应用内升级；MSI、DEB、macOS Apple Silicon 提供对应手动安装包。macOS 需完成签名、公证与实机验收后另行启用应用内安装。未知架构或安装来源只提供官方发布页，不借用其他格式安装器。各平台正式升级验收仍是发布前步骤，详见[1.0 实施记录](online-update-1.0-implementation.md)。

## 失败与重试

- v0.1.2 的 macOS 构建仅指定 DMG，缺少 updater 的 `.app.tar.gz`，收集校验失败，未公开 Release。v0.1.3 改为同时指定 `app,dmg`；保留原标签及失败记录。
- 在 Actions → Release → 失败的运行中查看具体步骤；网络、上传等临时故障可以选择 Re-run failed jobs。
- 构建修复涉及源码变更时，提交修复并发布新版本 tag；不要移动或覆盖已发布的 tag。
- 上传失败时 Release 保持草稿。重试只替换该草稿中的本流程附件，全部校验通过后才公开；已公开版本始终拒绝覆盖。
- 公开后的稳定清单探测失败时，先查看该 Release 与固定 `latest/download/latest.json` 是否一致。如果是 GitHub/CDN 延迟，等待后重新核实公开资源；重新运行发布脚本不会覆盖已公开版本。如果是实际附件或代码问题，发布更高修复版本，不移动 tag 或用旧版本覆盖。不要将这种失败当成“仍是草稿”。
- 如果 artifacts 已超过保留期，需要 Re-run all jobs，重新生成完整安装包。
- workflow_dispatch 仅允许选择已有版本 tag；选择分支会在校验阶段失败。
- 不要一次推送大量版本 tag；按上述命令每次推送一个发布 tag。

## 凭据与仓库设置

本地第一次推送需要登录有该仓库写权限的 GitHub 账号。CI 使用 GitHub 自动提供的 GITHUB_TOKEN，只有最终发布 job 申请 contents: write；无需把个人 Token 存入仓库。仓库需要允许 GitHub Actions 运行。

原 release/ 下的本地安装包保留在磁盘，但已停止 Git 跟踪并加入忽略规则。原 Git 历史保留；新安装包只通过 Releases 分发。

## 应用内升级签名

发布前配置仓库 Actions Secrets：

- `TAURI_SIGNING_PRIVATE_KEY`：Tauri 签名私钥文件的完整内容；不能填写本机文件路径。
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`：私钥密码；无密码的密钥使用空值。

公钥位于 src-tauri/tauri.conf.json 的 plugins.updater.pubkey。私钥必须与此公钥匹配，保存在仓库外并做好离线备份；不要重新生成密钥后直接替换已发布客户端使用的公钥。普通分支和 PR 构建不签名、不读取签名 Secret，发布构建在打包步骤使用它。

本次实现生成的密钥保存在本机用户目录下的 .sodeck-signing/updater.key；发布者已确认将私钥保存为上述 Secret。GitHub Actions 发布时无需读取本机文件，但仍需在密码管理器或加密离线介质中备份私钥及其密码（如有）；GitHub 设置页无法取回 Secret 原文。不得提交私钥或把它打印到日志。

更新清单固定入口为 GitHub Releases 的 latest/download/latest.json，清单内各包使用具体版本 tag 地址。Windows 目标为 windows-x86_64-nsis；macOS 仅支持 darwin-aarch64；Linux 为 linux-x86_64。签名绑定应用版本，清单标注版本与签名不一致时拒绝发布和安装。

Windows 本地签名构建可以把 TAURI_SIGNING_PRIVATE_KEY 环境变量设为仓库外私钥文件路径，再执行下方打包命令。GitHub Secret 则必须是文件内容。

详见 [在线升级实现记录](online-update-implementation.md)。

## Apple Developer ID 与公证

CI 已接入以下可选 Actions Secrets；本次开发没有新增真实凭据：

- `APPLE_CERTIFICATE`：Developer ID Application 证书的 base64 编码 `.p12` 内容。
- `APPLE_CERTIFICATE_PASSWORD`：导出证书密码；无密码证书允许为空。
- `APPLE_SIGNING_IDENTITY`：完整 `Developer ID Application: ...` 身份。
- `APPLE_ID`、`APPLE_PASSWORD`：Apple 账号及 App 专用密码。
- `APPLE_TEAM_ID`：开发团队 ID。

全部不配置时保留手动 ad-hoc 分发；部分配置会在构建前失败，避免误把签名但未公证的产物标成正式分发。完整配置时 Tauri 执行签名、公证，成功构建后发布说明才采用对应描述。凭据不打印到日志。还应在 macOS 实机验证 `codesign`、`spctl`、stapler 结果、首次安装和升级；这些步骤尚未在本次 Windows 环境执行。配置凭据不会自动改变客户端的 macOS 手动更新策略。

## 本地验证

```powershell
npm run release:check
npm run test:release
npm run test:updates
npm test
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml --locked
npm run tauri -- build --ci --bundles nsis,msi -- --locked
```

最后一条适用于 Windows；macOS 和 Linux 的完整打包由对应 GitHub runner 验证。

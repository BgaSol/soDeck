# GitHub 发布流程

代码、Actions 和安装包统一由 GitHub 仓库 https://github.com/BgaSol/soDeck 管理。本地 origin 应指向 https://github.com/BgaSol/soDeck.git。

## 发布新版本

先完成业务改动并提交，确保工作区干净。下面以发布 0.1.1 为例：

~~~powershell
npm run release:prepare -- 0.1.1
npm run release:check -- v0.1.1
git add package.json package-lock.json src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/tauri.conf.json
git commit -m "chore: release v0.1.1"
git tag -a v0.1.1 -m "SoDeck v0.1.1"
git push --atomic origin HEAD refs/tags/v0.1.1
~~~

当前首次发布版本为 0.1.0，版本文件已一致；提交发布配置后，可直接执行最后两步并将版本替换为 v0.1.0。

只在本地创建 tag 不会触发 Actions，必须推送到 GitHub。不要用 GitHub 的“Publish release”手工提前发布；让工作流负责上传附件并最后公开。

版本脚本同步 package.json、package-lock.json（含根包）、tauri.conf.json、Cargo.toml、Cargo.lock 中的应用版本，不升级依赖。支持正式版本 vMAJOR.MINOR.PATCH；当前不支持 beta/rc/build-metadata 标签。受 Windows MSI 限制，major/minor 不超过 255，patch 不超过 65535。

## 自动执行的流程

1. 推送 v* tag 后，Release 工作流确认 tag 是合法正式版本，并校验所有版本文件一致。非法 tag 或版本不一致会直接失败。
2. 复用 Desktop checks：四个平台执行前端测试、发布脚本测试、Rust 格式及测试。Linux 继续执行已有真实 SSH/rsync 和 Docker Engine 集成测试。
3. 在各平台生成安装包，验证文件存在且非空，再保存为 Actions artifacts（保留 14 天）。任何必要检查或平台构建失败，都不会公开 Release。
4. 汇总六个安装包，生成 SHA256SUMS.txt；创建 Release 草稿、生成变更说明、上传并校验七个附件，全部成功后自动公开。
5. 已公开的同名 Release 不允许覆盖；同一 tag 的发布任务串行执行。上传中断留下的草稿可以在同一提交上重试。

普通分支 push 和 pull request 只执行检查及编译，不发布 Release。tag 的检查由 Release 内部调用，不会额外再启动一套独立的 Desktop checks。

## 下载内容

以 0.1.1 为例，Release 包含：

| 平台 | 附件 |
| --- | --- |
| Windows x64 | SoDeck_0.1.1_windows_x64-setup.exe、SoDeck_0.1.1_windows_x64.msi |
| macOS Apple Silicon | SoDeck_0.1.1_macos_arm64.dmg |
| macOS Intel | SoDeck_0.1.1_macos_x64.dmg |
| Linux x64 | SoDeck_0.1.1_linux_x64.deb、SoDeck_0.1.1_linux_x64.AppImage |
| 文件校验 | SHA256SUMS.txt |

下载页：https://github.com/BgaSol/soDeck/releases

执行记录：https://github.com/BgaSol/soDeck/actions

Linux 包在 Ubuntu 24.04 构建，面向 Ubuntu 24.04 或更新的兼容环境；AppImage 不代表兼容所有旧版 Linux。下载 AppImage 后需添加执行权限。

当前没有配置 Windows 商业代码签名、Apple Developer ID 签名或公证。macOS 采用 ad-hoc 签名；安装时系统可能显示发布者或安全提示。这些安装包不包含应用内自动更新功能。

## 失败与重试

- 在 Actions → Release → 失败的运行中查看具体步骤；网络、上传等临时故障可以选择 Re-run failed jobs。
- 构建修复涉及源码变更时，提交修复并发布新版本 tag；不要移动或覆盖已发布的 tag。
- 上传失败时 Release 保持草稿。重试只替换该草稿中的本流程附件，全部校验通过后才公开；已公开版本始终拒绝覆盖。
- 如果 artifacts 已超过保留期，需要 Re-run all jobs，重新生成完整安装包。
- workflow_dispatch 仅允许选择已有版本 tag；选择分支会在校验阶段失败。
- 不要一次推送大量版本 tag；按上述命令每次推送一个发布 tag。

## 凭据与仓库设置

本地第一次推送需要登录有该仓库写权限的 GitHub 账号。CI 使用 GitHub 自动提供的 GITHUB_TOKEN，只有最终发布 job 申请 contents: write；无需把个人 Token 存入仓库。仓库需要允许 GitHub Actions 运行。

原 release/ 下的本地安装包保留在磁盘，但已停止 Git 跟踪并加入忽略规则。原 Git 历史保留；新安装包只通过 Releases 分发。

## 本地验证

~~~powershell
npm run release:check
npm run test:release
npm test
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml --locked
npm run tauri -- build --ci --bundles nsis,msi -- --locked
~~~

最后一条适用于 Windows；macOS 和 Linux 的完整打包由对应 GitHub runner 验证。

# v2 发布工具使用说明

本地实现所属：[T-029](../tasks/T-029-online-updates.md)。规格以[在线升级方案](online-updates-implementation.md)为准；这里记录已建立的工具入口，不代表已运行 GitHub Actions 或已授权发布。

## 组件与密钥

`release/components.json`登记核心和四个官方插件。应用版本由根 package、desktop package、desktop Cargo、Tauri 配置保持一致；插件 manifest/package 版本各自一致。组件输入改变而未加版时规划失败。工作流锁定 Node 24.22.0、Rust 1.99.1；依赖由两个锁文件固定。

`release/channels/stable.json`与`preview.json`分别保存公开渠道身份、目录签名公钥、Tauri updater公钥。当前公钥为空，会阻止正式发布。独立QA测试密钥只在忽略的`.qa/t029`使用，不能复制进正式渠道；客户端不带GitHub令牌。

需要后续授权配置的Secrets：`SODECK_V2_RELEASE_PRIVATE_KEY`（Ed25519 PEM）、`SODECK_V2_UPDATER_PRIVATE_KEY`（Tauri signer私钥）、可选`SODECK_V2_UPDATER_PASSWORD`。密钥正文不写入文档、对话或构建artifact。工作流将目录签名钥短暂写入RUNNER_TEMP并在finally清理；只有发布job获得contents:write。

## 本地工具

- `node scripts/release/plan.mjs --tag <专用标签> --out <目录>`：核对版本、工作区、上次已签名目录及实际构建输入，输出plan和五组件输入证据；首次本地QA必须显式传隔离`--config`。
- `node scripts/release/discover.mjs --tag <专用标签> --out <目录>`：从固定仓库发现本渠道最后一个发布，验证签名，规划变化组件。需要重建核心时下载并核对随附的旧插件资产。
- `node scripts/release/build.mjs --plan <plan.json>`：再次核对输入，以正式钥签插件、构建Windows x64 current-user NSIS、签署准确应用版本，输出built.json/安装器/资源包。构建结束再次核对输入，拒绝在规划后被改动的源码。
- `scripts/release/publish.mjs`：只允许本仓库专用标签的可信push工作流调用。先上传草稿资产，按真实asset ID形成并签署完整目录，全部就绪后公开；不覆盖已发布版本。

本地build读取`SODECK_V2_RELEASE_KEY_FILE`、`SODECK_V2_UPDATER_PRIVATE_KEY`及可选密码。需要Windows Rust/MSVC、Perl与Tauri打包依赖；检查脚本不读取v1密钥。

## GitHub入口

`.github/workflows/v2-check.yml`针对main提交/PR和发布调用运行检查；`.github/workflows/v2-release.yml`只响应`sodeck-v2/stable/000001`或`sodeck-v2/preview/000001`这类六位递增序号标签。普通main提交不会发布。工作流核对标签提交属于origin/main，稳定渠道拒绝预发布组件版本。产品仍称v2，发布标签前缀不随Git分支更名改变。

插件单独升级会生成新的完整目录快照，并引用原核心安装包；旧安装器的出厂插件保持原样。所有公开Release使用make_latest=false，不改v1 latest、latest.json或默认分支。重跑会核对已有草稿资产，内容不一致时保留草稿并报错，须先核对失败原因。

## 验证入口和边界

`tests/updater/release-fixture.mjs`创建独立源码副本；`release-inputs.mjs`验证单插件、文档、未使用SDK和实际使用UI四类变化；`build-installer-fixtures.mjs`构建测试A/B。`tests/desktop/online-update-installer.mjs`在QA安装标识、测试签名目录/缓存和本任务Linux服务上验证完整应用。测试B中的UI/worker标记仅存在于QA副本，不进入正式插件。

本地签名目录夹具不证明公网发现、Actions权限、Secrets配置或实际资产上传。G5必须在获得对应Git操作和预览发布授权后真实执行，结果记录在[T-029结果](../tasks/T-029-results.md)。NSIS更新签名也不代表已配置Windows Authenticode证书。

补充本地验证：`tests/updater/publish-fixture.mjs`以完全替换的fetch运行真实发布脚本，覆盖上传失败留草稿、重跑复用、完整签名目录和禁止覆盖公开版本，没有公网写入。`build-install-failure-fixture.mjs`只在QA源码副本的安装调用点注入TEMP故障，配合`tests/desktop/online-update-install-failure.mjs`及原生“重新打开”验证；根应用不包含故障开关。该测试发现并修复Windows rfd自定义按钮需要common-controls-v6的问题。

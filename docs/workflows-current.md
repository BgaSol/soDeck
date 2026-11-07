# GitHub Actions 工作流全景图（优化前快照）

2026-11-08 优化前快照。依据提交 `3a415ca6821ce45552933d5cc3cf1561d3a8d5e6` 的五个工作流和对应脚本。图中保留优化前的重测试、netem 和缓存行为，用于对照；源码测试移出打包后的结构见[提速第一步实施记录](implementation-release-performance.md)，当前运行入口见[开发指南](development.md)。

阅读方式：先看全景，再看各工作流内部。普通箭头表示执行依赖或产物流转；标明“手动”的虚线表示维护者选择，不代表工作流自动联动。同一任务内各步骤顺序执行，分叉的平台任务可并行。实际耗时是历史样本，单独标明来源。

## 1. 全局：五个入口如何连接

```mermaid
flowchart TB
  S["源码、锁文件、版本和发布说明"]
  S --> P["push main 或 pull_request"]
  S --> C["核心标签 v* 或手动运行"]
  S --> T["插件标签 plugin-*-v* 或手动运行"]
  P --> CHECK["Check<br/>源码检查、部分 UI、网络与弱网"]
  CHECK --> CS["检查状态和日志"]
  C --> CORE["Package SoDeck<br/>前置 → 五平台 → 汇总"]
  CORE --> CA[("核心 Artifact<br/>安装包、更新包、签名、来源信息")]
  CORE --> CM[("Core-update-manifest")]
  CORE --> CD["需要 publish 时<br/>核心 Release 草稿"]
  T --> PLUG["Package plugin<br/>选插件 → 五平台 → 发布任务"]
  PLUG --> PA[("插件 Artifact<br/>归档、平台元数据")]
  PLUG --> PD["标签或 publish=true<br/>独立插件 Release 草稿"]
  CD -.->|"维护者正式发布"| R[("正式 GitHub Releases")]
  PD -.->|"维护者正式发布"| R
  CA -.->|"手动指定版本或 run"| N["Verify native remote plugins"]
  PA -.->|"手动指定版本或 run"| N
  R -->|"明确版本的现成包"| N
  N --> NR["五平台组合验收结果与汇总"]
  R -->|"脚本指定的旧版和新版"| U["手动 Verify published release upgrade"]
  U --> UR["五平台升级结果和日志"]
  R --> CLIENT["应用发现正式版本<br/>下载、校验、重启应用更新"]
```

**没有自动连接的部分：**

- Check 与两个打包工作流没有 `needs` 或 `workflow_run` 连接；Check 成功不会自动启动打包，打包也不自动等待 Check。
- 核心与各插件没有构建依赖。核心安装包不携带业务插件，但核心打包中的 UI 测试会使用终端前端依赖。
- 两个验收工作流都是手动入口。验收结果不会自动把草稿发布为正式 Release。
- 草稿不供应用发现。正式发布是单独动作。
- 当前没有定时任务，没有纯文档路径过滤，也没有自动 main 预热打包。

## 2. 入口、权限、超时与平台

### 2.1 触发和输入

| 工作流 | 当前入口 | 手动输入及默认值 |
| --- | --- | --- |
| [Check](../.github/workflows/check.yml) | main push；pull_request | 无 workflow_dispatch |
| [Package SoDeck](../.github/workflows/build.yml) | v* 标签 push；workflow_dispatch | publish=false；core_run_id 为空 |
| [Package plugin](../.github/workflows/plugin.yml) | plugin-*-v* 标签 push；workflow_dispatch | plugin=terminal，必填字符串；publish=false |
| [Verify native remote plugins](../.github/workflows/remote-integration.yml) | 仅 workflow_dispatch | core/terminal/files/network 各自的 version、run_id；verification_run_id |
| [Verify published release upgrade](../.github/workflows/release-upgrade.yml) | 仅 workflow_dispatch | 无版本输入；版本写在脚本中 |

组合验收的核心、终端和文件各须提供版本或 run ID；网络两项均空则不参与。重放时填 verification_run_id，其他输入留空。

### 2.2 五个平台

| 平台任务名 | runner | 原生 Rust target | 核心格式 | 插件格式 |
| --- | --- | --- | --- | --- |
| windows-x86_64 | windows-2025 | x86_64-pc-windows-msvc | NSIS EXE | ZIP |
| windows-aarch64 | windows-11-arm | aarch64-pc-windows-msvc | NSIS EXE | ZIP |
| linux-x86_64 | ubuntu-22.04 | x86_64-unknown-linux-gnu | DEB + AppImage | tar.gz |
| linux-aarch64 | ubuntu-22.04-arm | aarch64-unknown-linux-gnu | DEB + AppImage | tar.gz |
| macos-aarch64 | macos-15 | aarch64-apple-darwin | DMG + app.tar.gz | tar.gz |

五个平台是并行的矩阵任务，实际启动仍受 runner 排队和配额影响。所有矩阵均设置 fail-fast=false，一个目标失败不会主动取消其他目标。

### 2.3 控制规则

| 工作流 / job | 超时 | 权限和并发 |
| --- | --- | --- |
| Check / format | 30 分钟 | contents:read；同一分支或 PR 新运行取消旧 Check |
| Check / network 两个目标 | 各 60 分钟 | 同上；与 format 没有 needs |
| 核心 preflight / package / metadata | 10 / 各 40 / 10 分钟 | 默认 contents:read、actions:read；只有 metadata 获得 contents:write |
| 插件 select / package / release | 10 / 各 20 / 10 分钟 | 默认 contents:read；只有 release 获得 contents:write |
| 组合 resolve / verify / summary | 10 / 各 20 / 5 分钟 | contents:read、actions:read |
| 升级 upgrade | 各 30 分钟 | contents:read |

核心 metadata 按核心标签串行，插件 release 按插件标签串行，两者 cancel-in-progress=false。这是发布写入任务的并发控制，不会把所有组件或所有平台串成一条流水线。

## 3. Check：三个并行任务，任务内串行

```mermaid
flowchart TB
  E["main push / pull_request<br/>当前不区分代码和纯文档"]
  E --> F["format<br/>Ubuntu x86_64 · 30 分钟上限"]
  E --> N1["network<br/>Ubuntu x86_64 · 60 分钟上限"]
  E --> N2["network<br/>Ubuntu ARM64 · 60 分钟上限"]
  F --> FR["普通检查结果"]
  N1 --> R1["网络结果 + 弱网报告"]
  N2 --> R2["网络结果 + 弱网报告"]
  FR --> END["GitHub 汇集任务状态<br/>没有单独的 summary job"]
  R1 --> END
  R2 --> END
  NEW["同一分支或 PR 的新 Check"] -.->|"自动取消旧运行"| E
```

### 3.1 format 内部

任务名虽然叫 format，实际覆盖编译、测试和浏览器回归。

```mermaid
flowchart TB
  A["checkout"]
  A --> B["Rust 1.99.1 + rustfmt"]
  B --> C["Node 24.22.0<br/>恢复 npm 下载缓存：根 + 终端锁文件"]
  C --> D["npm ci"]
  D --> E["npm run check<br/>TypeScript + 核心 cargo fmt"]
  E --> F["plugin:check<br/>三个插件 cargo fmt"]
  F --> G["plugin-client cargo fmt"]
  G --> H["web:build<br/>tsc + Vite"]
  H --> I["scripts:test<br/>scripts/*.test.mjs"]
  I --> J["修正 Ubuntu 软件源<br/>安装 WebKit、编译器、SSL 等"]
  J --> K["恢复 check-tests Rust 缓存<br/>核心 + network-proxy + files"]
  K --> L["npm run test<br/>核心 cargo test + test-fixtures"]
  L --> M["network-proxy 单元测试"]
  M --> N["files 单元测试"]
  N --> O["安装 Chromium 和系统依赖"]
  O --> P["interactions:test"]
  P --> Q["terminal:input:test<br/>安装终端锁定前端依赖"]
  Q --> R["settings:test"]
  R --> S["成功且 ref=main 时<br/>按 action 规则保存 Rust 缓存"]
```

### 3.2 network 内部：两个 Linux 目标各执行一遍

```mermaid
flowchart TB
  A["checkout → Rust 1.99.1 → Node"]
  A --> B["软件源修正 + 系统依赖<br/>WebKit、OpenSSH、CMake、iproute2"]
  B --> C["恢复 network-tests Rust 缓存<br/>核心 + network-proxy"]
  C --> D["npm ci<br/>此任务未配置 setup-node npm 缓存"]
  D --> E["安装 Chromium 和系统依赖"]
  E --> F["network:test<br/>启动隔离 OpenSSH、构建真实网络插件"]
  F --> G["核心网络集成、独立 SOCKS 服务端<br/>curl 互操作、协议与生命周期、网络 UI"]
  G --> H["network:netem<br/>独立 namespace + tc netem"]
  H --> I["按顺序执行 6 组网络条件"]
  I --> J["生成 report.json"]
  J --> K["always 上传 network-netem 平台报告<br/>缺报告仅 warn，未显式设置保留天数"]
  H -.->|"失败时仍尝试上传已有报告"| K
```

netem 配置顺序来自 [test-network-netem.mjs](../scripts/test-network-netem.mjs)：

| 配置 | 延迟 | 抖动 | 丢包 | 尝试次数 |
| --- | --- | --- | --- | ---: |
| baseline | 0 ms | 0 ms | 0% | 10 |
| remote | 50 ms | 10 ms | 0% | 10 |
| cross-region | 150 ms | 30 ms | 1% | 20 |
| poor | 300 ms | 50 ms | 3% | 20 |
| severe | 500 ms | 100 ms | 5% | 20 |
| blackhole | 0 ms | 0 ms | 100% | 2 |

这六组在每个 network 任务内顺序执行。两个 Linux 任务并行，不应将两个约 25 分钟的 netem 相加成 50 分钟的总体等待。

## 4. 核心打包：preflight → 五平台 → metadata

### 4.1 任务依赖与复用分支

```mermaid
flowchart TB
  E["v* 标签 / 手动 publish、core_run_id"]
  E --> PF["preflight · Ubuntu · 上限 10 分钟"]
  PF --> CFG["版本、说明、标签、草稿状态检查<br/>如复用：确认原 run 和有效 Artifact"]
  CFG --> QC["安装 Rust、npm ci<br/>check + scripts:test"]
  QC --> D{"core_run_id 是否为空"}
  D -->|"是：本次构建"| W1["Windows x86_64"]
  D -->|"是：本次构建"| W2["Windows ARM64"]
  D -->|"是：本次构建"| L1["Linux x86_64"]
  D -->|"是：本次构建"| L2["Linux ARM64"]
  D -->|"是：本次构建"| M["macOS ARM64"]
  W1 --> A[("成功验证后的核心 Artifact")]
  W2 --> A
  L1 --> A
  L2 --> A
  M --> A
  D -->|"否：跳过整个 package 矩阵"| OLD[("原 run 的核心 Artifact")]
  A --> META["metadata<br/>下载、核对、生成清单"]
  OLD --> META
  META --> PUB{"标签构建或 publish=true"}
  PUB -->|"否"| OUT["仅 Artifact + 更新清单"]
  PUB -->|"是"| DRAFT["集中准备核心 Release 草稿"]
```

preflight 输出 tag、version、publish、source_sha、run_id。复用时 source_sha 和 run_id 指向原始产物；当前 checkout 的发布脚本仍执行自身检查，不将当前脚本提交冒充旧包来源。

metadata 的条件是：前置成功、未取消，即使 package 有失败或被恢复模式跳过，也会尝试运行。它依赖矩阵结束，但不要求矩阵整体 success。

### 4.2 每个 package 任务的顺序

```mermaid
flowchart TB
  A["checkout"]
  A --> B["Node + npm 下载缓存<br/>仅 Windows x86_64 还包含终端锁文件"]
  B --> C["Linux：安装编译与桌面依赖<br/>当前为直接 apt，不调用镜像修正脚本"]
  C --> D["Rust 1.99.1 + 原生 target + rustfmt"]
  D --> E["恢复 core-package 缓存<br/>src-tauri/target"]
  E --> F["npm ci"]
  F --> G["npm run test<br/>五平台均编译并运行核心测试"]
  G --> H["仅 Windows x86_64<br/>准备 OpenSSH → ssh:test"]
  H --> I["Windows x86_64 / Linux x86_64 / macOS<br/>5 个原生宿主脚本回归"]
  I --> J["仅 Windows x86_64<br/>安装 Chromium → 完整 ui:test"]
  J --> K["tauri-action：正式构建<br/>web:build → Cargo release → 打包和签名"]
  K --> L["对应系统安装／解包实际产物<br/>架构、签名、启动、重开和退出"]
  L --> M["collect-core.mjs<br/>收集预期文件，生成来源和摘要"]
  M --> N["上传 SoDeck-平台 Artifact<br/>30 天"]
  N --> O["仅 main 的成功任务可保存 Rust 缓存"]
```

图中“仅……”步骤在其他平台跳过，其余顺序保持。五个平台各有自己的依赖安装和编译目录，并不共享同一个 runner 文件系统。

原生宿主脚本子集为 collect-core、core-release、plugin-settings-assets、release-metadata、smoke 的测试文件。完整 scripts:test 已在 preflight 执行一次。

### 4.3 完整 UI 回归还会展开哪些命令

```mermaid
flowchart TB
  UI["npm run ui:test"]
  UI --> S["test-server-ui.mjs"]
  S --> W["test-workspace-ui.mjs"]
  W --> C["test-connection-ui.mjs"]
  C --> I["interactions:test"]
  I --> P["ui:production:test"]
  P --> PB["再次 web:build<br/>加载 dist 做正式前端回归"]
  PB --> D["desktop:test"]
  D --> DI["npm ci --prefix plugins/terminal<br/>再运行 test-desktop-ui.mjs"]
  DI --> SET["settings:test"]
  FIX["多个 UI 脚本使用<br/>cargo build + test-fixtures 的真实核心夹具"]
  FIX -.-> S
  FIX -.-> W
  FIX -.-> C
  FIX -.-> SET
```

这解释了核心出包为什么会安装终端前端依赖。终端业务插件没有因此被打进核心安装包。

### 4.4 实际包验证按系统展开

```mermaid
flowchart TB
  BUILD["正式构建完成"]
  BUILD --> W["Windows<br/>NSIS 静默安装到 Program Files"]
  W --> W1["检查 HKLM 安装登记和目录"]
  W1 --> W2["verify-binary 原生架构<br/>smoke + 更新包签名验证"]
  W2 --> W3["仅 x86_64 再做<br/>包内 OpenSSH 验证"]
  BUILD --> L["Linux<br/>dpkg-deb 解包 DEB"]
  L --> L1["检查 ELF 架构<br/>dbus + xvfb 运行 DEB 内程序"]
  L1 --> L2["另运行实际 AppImage<br/>携带更新包验证"]
  BUILD --> M["macOS<br/>只读挂载 DMG"]
  M --> M1["检查 Mach-O 架构<br/>运行 app 内程序"]
  M1 --> M2["使用 app.tar.gz 验证更新签名<br/>退出后卸载 DMG"]
  W3 --> DONE["验证成功才收集并上传该平台 Artifact"]
  L2 --> DONE
  M2 --> DONE
```

smoke 通常启动两次：首次验证窗口与资料保存，再重开验证持久资料，均要求正常退出。包内业务验收另按传入的插件和 SSH 配置启用。

### 4.5 metadata 与 Release 写入

```mermaid
flowchart TB
  A["下载指定 run 的 SoDeck-* Artifact<br/>合并至 artifacts/core"]
  A --> B["核对组件、版本、原源码提交<br/>平台、文件名、大小、SHA-256 和签名文件"]
  B --> C["生成 core-update.json<br/>仅列实际有效产物"]
  C --> D{"需要草稿"}
  D -->|"否"| H["上传 Core-update-manifest<br/>30 天"]
  D -->|"是"| E["同标签串行<br/>检查标签指向和既有草稿"]
  E --> F["已有同名包须摘要一致<br/>正式 Release 拒绝写入"]
  F --> G["创建／补齐草稿<br/>上传包与签名，清单最后写入并复核"]
  G --> H
```

核心汇总允许处理部分平台成功的产物，记录缺失平台。这不等于该版本已经完成五平台交付；后面的组合验收选包要求更严格。

## 5. 插件打包：每次只处理所选插件

```mermaid
flowchart TB
  E["插件标签或手动 plugin + publish"]
  E --> S["select · Ubuntu · 10 分钟<br/>安装 Rust 和 Node"]
  S --> V["release-metadata 选择组件<br/>Cargo.toml 版本 + release.json + CHANGELOG"]
  V --> F["仅所选插件 cargo fmt"]
  F --> P["五个平台并行 package<br/>每个平台上限 20 分钟"]
  P --> ENV["checkout → Node/npm 缓存 → Rust<br/>恢复 plugin-名称 缓存 → npm ci"]
  ENV --> NET{"所选插件是 network-proxy"}
  NET -->|"是"| TEST["先 cargo test --locked"]
  NET -->|"否"| PKG["package-plugin.mjs"]
  TEST --> PKG
  PKG --> UI["需要构建 UI 的插件：安装锁定依赖并构建<br/>当前为终端"]
  UI --> R["cargo build --release --locked --target"]
  R --> RES["复制 UI，组装共享菜单、样式<br/>字体与设置资源"]
  RES --> BIN["复制本机二进制、检查架构<br/>macOS 二进制临时签名"]
  BIN --> ZIP["生成 plugin.json<br/>ZIP / tar.gz + 平台元数据和 SHA-256"]
  ZIP --> A[("Plugin-名称-平台 Artifact<br/>30 天")]
  A --> D{"标签或 publish=true<br/>且 select 成功、未取消"}
  D -->|"否"| END["结束：只保留 Artifact"]
  D -->|"是"| REL["release · Ubuntu · 10 分钟<br/>同插件标签串行"]
  REL --> META["下载本次插件 Artifact<br/>核对并生成 plugin-update.json"]
  META --> OLD["查询既有 Release<br/>已正式发布则失败，否则创建／使用草稿"]
  OLD --> UP["上传归档和清单到独立草稿<br/>当前插件脚本使用 --clobber"]
```

插件 release 等待矩阵结束，但条件不要求全部 package success；有效平台元数据可以生成部分平台清单。插件当前没有 core_run_id 式跨运行恢复参数。

插件打包检查归档元数据和二进制架构，不启动真实插件远程业务。运行、退出和协作行为由源码集成或后面的实际包验收覆盖。

插件草稿上传采用现有 --clobber，和核心“先逐包比对既有摘要再复用”的实现不同。正式版本仍会被前置检查拒绝覆盖。

## 6. 原生远程组合验收：先固定输入，再用现成包

### 6.1 每个组件怎样选包

```mermaid
flowchart TB
  E["手动输入<br/>核心、终端、文件，网络可选"]
  E --> RE{"是否指定 verification_run_id"}
  RE -->|"是"| SNAP["读取旧验收输入快照<br/>重新验证来源仍有效"]
  RE -->|"否"| EACH["逐组件处理"]
  EACH --> NET{"网络版本与 run 都为空"}
  NET -->|"是"| SKIP["网络记为未参与"]
  NET -->|"否或必需组件"| RUN{"指定组件 run_id"}
  RUN -->|"是"| ART["限定本仓库对应打包 workflow<br/>运行已结束、五平台 job 成功<br/>五份 Artifact 唯一且未过期"]
  RUN -->|"否"| FORMAL{"精确版本正式 Release 可用"}
  FORMAL -->|"是"| RELEASE["核对标签源码、版本/API<br/>五平台清单、包、签名或摘要"]
  FORMAL -->|"没有正式版本"| FIND["在版本／标签源码约束下查候选 run"]
  FIND --> UNIQUE{"完整候选恰好一个"}
  UNIQUE -->|"是"| ART
  UNIQUE -->|"无或多个"| FAIL["明确失败<br/>多个候选须改填 run_id"]
  RELEASE --> API["组件 API 主版本兼容检查"]
  ART --> API
  SKIP --> API
  API --> SAVE["保存 native-inputs.json<br/>组件版本、提交、来源 ID、文件和摘要"]
  SNAP --> SAVE
```

正式 Release 已存在但附件损坏、缺包、清单错误或下载失败时直接报错，不自动换成实验包。组件可以来自不同源码提交。仅发布汇总失败的构建，只要五平台 package 和 Artifact 合格，仍可作为验收输入。

### 6.2 工作流执行图

```mermaid
flowchart TB
  R["resolve · Ubuntu · 10 分钟<br/>解析或重放输入"]
  R --> IN[("Native-verification-inputs<br/>30 天")]
  IN --> V["五平台 verify 并行<br/>每个平台 20 分钟"]
  V --> S["读取固定快照<br/>选取当前 target 的输入"]
  S --> K{"每组件的来源"}
  K -->|"Artifact"| A["按固定 Artifact ID 下载"]
  K -->|"正式 Release"| B["按固定附件 ID 下载"]
  A --> PREP["prepare-native-packages<br/>来源、大小/摘要、归档路径、清单、入口、架构"]
  B --> PREP
  PREP --> ENV["设置核心包路径、插件目录<br/>预期版本、target 和隔离数据目录"]
  ENV --> RT["安装运行依赖和隔离 OpenSSH"]
  RT --> OS["Windows 安装 EXE<br/>Linux 解包 DEB<br/>macOS 挂载 DMG"]
  OS --> BIN["核对已安装核心的原生架构"]
  BIN --> TEST["test-openssh --packaged<br/>实际核心 + 独立插件"]
  TEST --> FLOW["终端 PTY、文件读写、可选 TCP/SOCKS5<br/>实际窗口、移窗、重开和退出"]
  FLOW --> REC["always 记录平台结果<br/>结合步骤状态和实际运行报告"]
  REC --> RES[("Native-verification-result-平台<br/>30 天；重跑允许覆盖报告")]
  RES --> SUM["summary · Ubuntu · 5 分钟<br/>always 汇总五个平台"]
  IN --> SUM
  SUM --> FINAL[("Native-verification-summary<br/>30 天")]
```

源码不在 verify 任务中重新编译；这里消费已经产出的核心和插件包。Linux 使用 DEB 内程序，当前这条组合验收不另跑 AppImage。

Windows ARM64 缺少 sshd 时下载固定版本微软 ARM64 便携包并校验摘要；Windows x86_64 使用系统可选组件。测试 SSH 服务只监听隔离回环环境，不随产品发行。

summary 的成功条件是：五个平台有效 job 均成功、各报告与本次测试提交匹配，且运行报告确认版本、平台、桌面和 SSH 行为完成；可选网络参与时还要求网络报告成功。缺报告、未启动、取消都不能当作通过。

同一验收 run 重试会复用已固定输入；新建 run 可用 verification_run_id 重放。原包失效时失败，不偷偷选择另一份包。

## 7. 已发布版本升级验收：当前是固定版本专项

当前 [test-release-upgrade.mjs](../scripts/test-release-upgrade.mjs) 中的版本为：

| 组件 | 旧版本 | 新版本 |
| --- | --- | --- |
| 核心 | 0.9.2 | 0.9.3 |
| terminal | 0.2.1 | 0.2.2 |
| files | 0.4.1 | 0.4.2 |
| network-proxy | 0.2.1 | 0.2.2 |

即使当前核心已是 0.9.4，手动运行该工作流也不会自动变成 0.9.3 → 0.9.4。

```mermaid
flowchart TB
  E["手动启动<br/>无版本输入参数"]
  E --> M["五平台 upgrade 并行<br/>各 30 分钟上限"]
  M --> ENV["checkout → Node → Rust<br/>Linux 安装编译与运行依赖"]
  ENV --> C["尝试恢复 core-package 缓存<br/>save-if=false"]
  C --> FIX["npm ci + check<br/>编译 release-upgrade-fixture"]
  FIX --> SSH["准备隔离 OpenSSH"]
  SSH --> OLD["下载正式旧核心与旧插件<br/>验证 SHA-256、清单和原生架构"]
  OLD --> BASE["安装／解包旧程序 → smoke<br/>fixture seed 建立资料和插件基线"]
  BASE --> INSTALL["fixture install<br/>调用生产插件更新服务与官方 updater"]
  INSTALL --> BYTES["核对实际程序被替换为下载的新字节<br/>等待上限 180 秒"]
  BYTES --> REOPEN["新程序重开、验证旧资料<br/>fixture activate 检查插件激活"]
  REOPEN --> DEB["Linux 额外验证<br/>旧 DEB 系统安装 → 新 DEB 系统升级"]
  DEB --> BUSINESS["新建隔离业务数据<br/>运行升级后的实际核心与插件远程烟测"]
  BUSINESS --> REPORT["写入 baseline、prepared、activated<br/>业务结果、摘要和日志"]
  REPORT --> UP["always 上传 Release-upgrade-平台<br/>30 天；没有独立 summary job"]
```

该流程由测试夹具驱动生产更新服务，包含实际安装字节和重开验证；不等于在真实设备上操作完整更新页面、管理员确认和交互式安装界面。

## 8. 缓存：存的是什么，谁能读写

### 8.1 当前 Rust 缓存关系

```mermaid
flowchart LR
  F["Check / format"] -->|"成功且 main 时保存"| CF[("check-tests<br/>核心 + network-proxy + files")]
  CF -->|"恢复"| F
  N["Check / network 两平台"] -->|"成功且 main 时保存"| CN[("network-tests<br/>核心 + network-proxy")]
  CN -->|"恢复"| N
  P["核心 package 五平台"] -->|"成功且 main 时保存"| CP[("core-package<br/>核心 debug 测试 + release 构建依赖")]
  CP -->|"恢复"| P
  CP -->|"仅恢复，不保存"| U["升级夹具<br/>需要 debug/test-fixtures 构建"]
  PP["所选插件 package 五平台"] -->|"成功且 main 时保存"| CPP[("plugin-名称<br/>该插件的构建依赖")]
  CPP -->|"恢复"| PP
  NONE["原生组合验收<br/>下载现成包，无 Rust 构建缓存"]
```

这些是缓存族，不是一个跨平台通用文件。action 自动把主机、架构、Rust 工具链、相关 Cargo 输入和编译环境纳入键。

| 使用点 | shared-key / 内容 | 附加输入 | 保存条件 |
| --- | --- | --- | --- |
| Check format | check-tests；三个 workspace 的 target 和 Cargo 下载内容 | 显式共享客户端清单摘要 | main 且任务成功，按 action 命中规则保存 |
| Check network | network-tests；核心与网络插件 workspace | 未配置 format 中那个附加环境摘要 | 同上 |
| 核心 package | core-package；src-tauri/target | 默认自动键 | 同上 |
| 插件 package | plugin-插件名；该插件 target | 显式共享客户端清单摘要 | 同上 |
| 升级 upgrade | core-package；src-tauri/target | 默认自动键 | false，永不保存 |

不同缓存族可能包含同一批核心依赖，但占用独立缓存空间。同一个 src-tauri/target 目录在不同 runner 上也不是同一份目录。

### 8.2 main 与标签的作用域

```mermaid
flowchart TB
  MAIN["main 上的成功运行"]
  MAIN --> W["满足 save-if<br/>写入默认分支缓存"]
  W --> CACHE[("main 缓存<br/>按平台、工具链、依赖等区分")]
  CACHE --> PR["PR / 分支：恢复适用缓存"]
  CACHE --> TAG["v* 或插件标签：恢复适用缓存"]
  TAG --> READ["当前 Rust save-if 不满足<br/>标签运行不保存共享缓存"]
  MANUAL["在 main 手动运行打包"] --> MAIN
  PUSH["普通 main push"] --> CHECK["当前只触发 Check"]
  CHECK --> MAIN
  PUSH -.->|"没有自动触发"| NO["当前核心／插件打包<br/>不会因普通 main push 自动预热"]
```

GitHub 会限制缓存可见的 ref。仅把标签构建改成“也保存”，并不能让下一个不同标签直接使用这份缓存。缓存也受未访问过期和仓库容量淘汰规则影响。

截至本次会话 2026-11-08 的查询：37 条缓存，约 9.89 GiB；核心只列出 Linux x86_64 和 ARM64 两份，未列出 Windows 和 macOS 核心条目。该快照是容量和覆盖证据，不能独立证明过去发生了哪一种淘汰。

### 8.3 npm 与未缓存内容

| 任务 | npm 下载缓存 |
| --- | --- |
| Check format | 根 package-lock + terminal/package-lock |
| Check network | 未配置 |
| 核心 preflight | 根 package-lock |
| 核心 package | Windows x86_64 根 + 终端；其他平台只有根 |
| 插件 package | terminal 时根 + 终端；其他插件只有根 |
| 核心 metadata、插件 select/release、组合验收、升级验收 | 未配置 |

缓存不是完整 runner 镜像。npm ci、Cargo、系统依赖安装和资源生成仍会执行；当前没有显式缓存整个 node_modules、APT 安装状态或 Playwright 浏览器。缓存命中不表示测试已通过，也不能代替发行 Artifact。

## 9. Artifact、Release 和缓存的去向

```mermaid
flowchart TB
  CACHE[("依赖缓存<br/>可失效、被淘汰")]
  CACHE --> BUILD["加速安装与编译<br/>源码和构建命令仍执行"]
  BUILD --> CORE[("SoDeck-平台 Artifact<br/>包、签名、core-artifact.json")]
  BUILD --> PLUGIN[("Plugin-名称-平台 Artifact<br/>归档、plugin-target.json")]
  CORE --> SUMMARY["核心汇总"]
  PLUGIN --> PS["插件汇总"]
  SUMMARY --> MANIFEST[("Core-update-manifest Artifact")]
  SUMMARY --> CR["核心草稿附件<br/>包、签名、core-update.json"]
  PS --> PR["插件草稿附件<br/>归档、plugin-update.json"]
  CORE --> VERIFY["固定输入组合验收"]
  PLUGIN --> VERIFY
  CR -.->|"维护者发布"| REL[("正式 Release 附件")]
  PR -.->|"维护者发布"| REL
  REL --> APP["用户安装和应用更新"]
  REL --> UPGRADE["正式包升级验收"]
```

| 对象 | 内容 / 用途 | 保留 |
| --- | --- | --- |
| SoDeck-平台 | 原生核心包、更新签名、版本/平台/提交/文件摘要 | 30 天 |
| Core-update-manifest | core-update.json | 30 天 |
| Plugin-名称-平台 | ZIP 或 tar.gz、平台元数据 | 30 天 |
| Native-verification-inputs | 固定输入快照 | 30 天 |
| Native-verification-result-平台 | 平台结果和失败阶段 | 30 天 |
| Native-verification-summary | 五平台汇总 | 30 天 |
| Release-upgrade-平台 | 升级结果、基线和日志 | 30 天 |
| network-netem-平台 | 弱网 report.json | workflow 未指定，使用适用的默认保留设置 |
| 正式 Release 附件 | 用户安装和更新的稳定下载来源 | 不跟随临时 Artifact 到期 |
| npm / Rust cache | 加速后续构建 | 受缓存作用域、容量和淘汰规则控制 |

核心 Windows 更新指向 EXE；Linux 更新指向 AppImage，DEB 用于系统包安装；macOS 更新指向 app.tar.gz，DMG 用于安装。插件按本机原生 target 选择 ZIP 或 tar.gz。

核心清单有官方 updater 签名；插件清单有组件 ID、版本、API 版本、目标和 SHA-256。宿主测试夹具不进入发行包。

## 10. 失败、取消、重试的实际边界

```mermaid
flowchart TB
  F{"失败发生在哪里"}
  F -->|"Check"| C["查看失败任务<br/>新提交会取消同分支旧 Check"]
  F -->|"核心 preflight"| PF["package 不启动<br/>metadata 条件不满足"]
  F -->|"某个平台打包"| P["其他平台继续<br/>成功 Artifact 保留"]
  P --> M["汇总仍可能处理成功平台<br/>部分清单不等于五平台通过"]
  F -->|"核心 metadata"| R["重跑失败 metadata<br/>复用同一 run 的现成包"]
  R --> FIX["若需要修复发布脚本<br/>新手动运行填写 core_run_id"]
  FIX --> SKIP["重新前置核对，跳过平台编译"]
  F -->|"组合输入解析"| RES["verify 不启动<br/>summary 记录无有效输入"]
  F -->|"组合某平台运行"| V["重跑失败平台与汇总<br/>继续使用固定输入"]
  F -->|"升级专项"| U["重跑失败平台<br/>仍使用脚本指定版本"]
```

| 容易混淆的状态 | 当前含义 |
| --- | --- |
| package 某目标失败 | fail-fast=false，其他目标继续；不保证汇总自动失败 |
| 核心 metadata 成功 | 已处理所下载的有效产物；可能仍有缺失平台 |
| 插件 release 成功 | 可生成有效平台清单；当前条件不强制五平台全部成功 |
| 原生组合 summary 成功 | 五平台有效任务和实际运行报告都通过 |
| 核心恢复成功 | 可以只是复用旧包完成发布收尾，并非重新测试当前 checkout 的产品 |
| core_run_id 指向整体失败的 run | 若失败只在收尾而包有效，可能仍可复用；之后仍逐包验证 |
| Artifact 过期或包摘要变化 | 恢复或验收明确失败，需要处理固定来源；不能用缓存冒充包 |
| 草稿改成正式 Release | 当前是维护者动作；没有自动审批或自动发布链 |

## 11. 实际耗时：核心最长路径

下图为 [核心 run 42355132557](https://github.com/BgaSol/soDeck/actions/runs/42355132557)，时间为北京时间 2026-11-07。第二次运行只恢复 metadata；平台任务没有重编译。

```mermaid
gantt
  title 核心构建与汇总的实际时间
  dateFormat YYYY-MM-DDTHH:mm:ss
  axisFormat %H:%M
  section 前置
  preflight 20秒 :a, 2026-11-07T16:21:27, 2026-11-07T16:21:47
  section 五平台并行
  Windows x86_64 18分33秒 :crit, b, 2026-11-07T16:21:50, 2026-11-07T16:40:23
  Windows ARM64 11分03秒 :c, 2026-11-07T16:21:51, 2026-11-07T16:32:54
  Linux x86_64 5分49秒 :d, 2026-11-07T16:21:51, 2026-11-07T16:27:40
  Linux ARM64 5分57秒 :e, 2026-11-07T16:21:53, 2026-11-07T16:27:50
  macOS ARM64 10分31秒 :g, 2026-11-07T16:21:51, 2026-11-07T16:32:22
  section 发布收尾
  第一次 metadata 失败 :crit, h, 2026-11-07T16:40:26, 2026-11-07T16:40:40
  重试 metadata 成功 :i, 2026-11-07T16:42:10, 2026-11-07T16:43:12
```

Windows x86_64 内部主要顺序：

```mermaid
flowchart LR
  A["Rust 测试及编译<br/>328 秒"] --> B["完整 UI 回归<br/>217 秒"]
  B --> C["正式构建和打包<br/>489 秒"]
  C --> D["实际安装包验证<br/>16 秒"]
  D --> E["加上环境等其他步骤<br/>整项 1113 秒"]
```

其余平台已较早完成，但 metadata 要等整个矩阵结束。这次从初次触发至重试完成约 21 分 47 秒，包含任务调度和两次汇总之间的间隔；不能只用成功重试的 62 秒代表整次出包。

## 12. 实际耗时：Check 最长路径

下图来自另一次 [Check 42356855256](https://github.com/BgaSol/soDeck/actions/runs/42356855256)，不是上图核心构建的同一次运行。此提交只修改发布记录，当前仍触发全部检查。

```mermaid
gantt
  title Check 三个并行任务的实际时间
  dateFormat YYYY-MM-DDTHH:mm:ss
  axisFormat %H:%M
  section 普通检查
  format 4分13秒 :a, 2026-11-07T16:46:15, 2026-11-07T16:50:28
  section 网络与弱网
  Linux x86_64 31分31秒 :crit, b, 2026-11-07T16:46:15, 2026-11-07T17:17:46
  Linux ARM64 31分36秒 :crit, c, 2026-11-07T16:46:18, 2026-11-07T17:17:54
```

| 任务 | 整项 | 普通网络步骤 | netem 步骤 |
| --- | ---: | ---: | ---: |
| network x86_64 | 1891 秒 | 261 秒 | 1536 秒 |
| network ARM64 | 1896 秒 | 298 秒 | 1526 秒 |

普通网络步骤还包含浏览器安装、真实插件与夹具构建等开销。Check 约 32 分钟的整体等待取决于最慢任务，不是三项相加。

## 13. 用这组图检查当前边界

1. 日常 Check 仍含长耗时 netem，且纯文档提交也会执行。
2. 核心 package 同时承担测试编译和正式编译；Windows x86_64 额外承担完整 UI 回归。
3. 核心、插件的构建相互独立；原生组合验收另消费包。
4. 测试、打包、恢复发布是不同结果，不能只看一个总绿灯就互相替代。
5. Rust 缓存主要由 main 写入；现有打包默认只有标签和手动入口，因此并不自动持续补齐 main 的五平台核心缓存。
6. 当前 core-package 同时容纳测试和发行构建依赖；不同 Check 缓存族还会重复保存核心依赖。
7. 构建 Artifact 与正式 Release 附件分工明确；已经实现的核心恢复可以避免发布收尾失败后重新编译。
8. 当前升级专项固定验证 0.9.2 → 0.9.3，不能据此声称更新版本已经完成升级验收。

本文只做现状图示。未改工作流或测试，未触发构建、清理缓存或发布版本。

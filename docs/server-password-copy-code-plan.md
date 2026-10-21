# 已保存服务器密码再次复制：编码计划

日期：2026-10-21。状态：已实施并完成验证。实现记录见 [server-password-copy-implementation.md](C:/Users/bgaso/Documents/works/soDeck/docs/server-password-copy-implementation.md)。

功能依据：本会话上一轮功能计划。已有服务器的编辑页保留空白密码输入框和“已保存”状态，新增“复制已保存密码”；点击后由 Rust 将当前保存的密码写入系统剪贴板，前端接收成功或错误。

设计依据：[apple-design/SKILL.md](C:/Users/bgaso/.codex/skills/apple-design/SKILL.md)。采用就近操作、直接命名、即时反馈、可恢复错误、清晰状态和克制动效；延续现有服务器编辑器的排版、主题和控件。

## 1. 实施目标与范围

交付路径：选择已有服务器 → 编辑 → 复制已保存密码 → 粘贴使用。复制不要求输入或重新保存密码。

- 已保存密码保持隐藏；本次不增加明文查看接口、眼睛按钮或掩码输入值。
- 密码输入框仍只表示用户本次输入的新密码。
- 复制操作不修改配置，不影响 dirty 判断，不调用 save_server。
- 保留 passwordChange 的 keep / set / clear 语义，以及现有服务器配置复制的 inherit / set / clear 语义。
- 不修改数据库结构、凭据存储方式、SSH 认证过程、终端复制流程和服务器详情页。
- 使用现有 React、原生 dialog、CSS 变量、lucide 图标及 Tauri 剪贴板插件，不引入依赖和全局状态。

## 2. 当前代码依据

| 文件                                            | 当前实现                                                                            | 本次关联                                               |
| ----------------------------------------------- | ----------------------------------------------------------------------------------- | ------------------------------------------------------ |
| src/ServerEditor.tsx                            | passwordChange 初始 keep；密码框只有 set 时才显示本次输入；有已保存、替换和清空状态 | 新增局部复制入口与反馈，不改变密码框绑定               |
| src/types.ts                                    | Server 只有 hasSavedPassword；SaveServerRequest 带显式密码变更动作                  | 维持现有模型，不增加 password 字段                     |
| src/api.ts                                      | 服务器列表、保存、配置复制及删除通过 invokeDiagnosed 调用                           | 新增仅传 id 的复制接口                                 |
| src-tauri/src/model.rs                          | ServerConfig 包含凭据且不序列化；ServerView 只包含保存状态                          | 维持前后端数据边界                                     |
| src-tauri/src/db.rs                             | 存储密码；get 通过 list 查找完整服务器                                              | 新增按 ID 查询凭据的方法，避免为一次复制读取所有服务器 |
| src-tauri/src/lib.rs                            | main_only 校验主窗口；已初始化剪贴板插件；已有 blocking 工具                        | 实现、注册专用复制命令，沿用窗口校验和阻塞任务工具     |
| src-tauri/src/diagnostics/commands_allowlist.rs | 过滤可记录的命令名和错误种类                                                        | 加入新命令名，使用已有安全错误分类                     |
| tests/connection-ui.spec.ts                     | 已覆盖密码保留、替换、清空、配置复制及 IPC 模拟                                     | 扩展测试夹具和回归用例                                 |

当前锁定的 tauri-plugin-clipboard-manager 2.5.0 提供 ClipboardExt 和 write_secret。后者会设置敏感剪贴板提示；Windows 实现包含排除历史和云同步的标记。实际效果受操作系统及剪贴板管理器支持影响，不能承诺阻止所有外部读取。

## 3. 前后端接口

### 3.1 前端 API

在 src/api.ts 增加：

```ts
copySavedServerPassword: (id: string) =>
  invoke<void>("copy_saved_server_password", { id }),
```

请求只含服务器 ID，以及现有诊断封装附带的追踪 ID。成功响应为 void；不返回密码、掩码、密码长度或服务器完整配置。

### 3.2 Rust 命令

在 src-tauri/src/lib.rs 的服务器命令区域增加并注册：

```rust
#[tauri::command]
async fn copy_saved_server_password(
    window: WebviewWindow,
    state: TauriState<'_, Arc<State>>,
    id: String,
) -> AppResult<()>;
```

执行顺序：

1. 先 main_only(&window)，非主窗口立即失败。
2. 克隆 State 的 Arc 和 AppHandle，进入已有 blocking 工具管理的任务，避免数据库和系统剪贴板操作阻塞 UI。
3. 检查 state.ensure_running()。
4. 在 database 锁内读取该 ID 当前保存的密码；取得一个一致的读取快照后释放数据库锁。
5. 用 ClipboardExt 调用 app.clipboard().write_secret(password)。
6. 写入成功才返回 Ok(())；剪贴板失败映射为固定、安全的 clipboard 错误。

复制的是数据库在本次读取时的值；读取后其他操作再替换或清空密码，不追溯改变已取得的快照。系统剪贴板写入期间不持有数据库锁，也不新增配置写锁。

本命令不调用 save / save_password，不更新 updated_at，不修改连接或转发状态，不广播凭据事件。

### 3.3 数据库方法

在 src-tauri/src/db.rs 增加：

```rust
pub fn saved_password_for_copy(&self, id: &str) -> AppResult<String>;
```

先校验 schema，再使用参数化 SQL，按主键查询 auth_type、password。该返回值只在 Rust 内部使用。

| 条件                         | 行为                                                    |
| ---------------------------- | ------------------------------------------------------- |
| ID 为空                      | validation；不写剪贴板                                  |
| ID 不存在                    | notFound，提示“服务器不存在，请重新打开编辑窗口。”      |
| 数据库认证方式不是 password  | unavailable，提示“此服务器当前未使用密码认证。”         |
| 已保存密码为空               | unavailable，提示“此服务器没有已保存的密码。”           |
| 密码非空                     | 原样返回给 Rust 命令，不 trim、不编码转换、不规范化内容 |
| 数据库不可用或 schema 不兼容 | 沿用现有数据库错误分类，保留草稿                        |
| 系统写入失败                 | 命令返回 clipboard，提示“密码复制失败，请重试。”        |

只判断真正的空字符串；前后空格、非 ASCII 字符、引号、反斜杠等必须原样保留。

## 4. 编辑器状态与交互

### 4.1 入口条件

在 src/ServerEditor.tsx 的 password-field 区域添加按钮，紧邻“已保存”状态，名称固定为“复制已保存密码”。使用 Copy 图标加文字，type="button"，避免提交表单。

满足下列条件才展示：

- 当前是普通编辑，initial.id 非空，且没有 duplicateSource；
- initial.authType 为 password，initial.hasSavedPassword 为 true；
- 当前 draft.authType 仍为 password。

首次打开不查询密码，不写剪贴板。新增配置和复制配置弹窗不出现此按钮。副本保存后，重新打开其普通编辑页即可使用。

### 4.2 与密码草稿的关系

| 当前 passwordChange       | 复制入口 | 复制内容及说明                                                             |
| ------------------------- | -------- | -------------------------------------------------------------------------- |
| keep                      | 可用     | 当前数据库中已保存的密码                                                   |
| set                       | 可用     | 仍复制当前已保存的密码；就近显示“复制的是已保存密码，新密码保存后才生效。” |
| clear                     | 可用     | 清空尚未保存，仍可复制原值；说明“保存后清空，当前仍可复制已保存密码。”     |
| 保存中或确认放弃草稿中    | 禁用     | 跟随现有 fieldset 的 disabled 状态                                         |
| draft 切换为 Agent 或私钥 | 隐藏     | 不读取隐藏认证区域的凭据                                                   |

本次不提供“复制新密码”入口，避免增加用户必须区分的动作。保留原有输入、清空、撤销清空、保存与取消行为。

### 4.3 局部复制状态

状态放在 ServerEditor 内，不写入 draft、initial 或 passwordChange，不增加公共 Server 字段。

```ts
type PasswordCopyState =
  | { phase: "idle" }
  | { phase: "copying" }
  | { phase: "copied" }
  | {
      phase: "failed";
      kind: "unavailable" | "clipboard" | "other";
      message: string;
    };
```

- 点击后立即切到 copying，按钮显示“复制中…”；仅锁定该复制动作。
- ref 守卫同步阻止连点重复发起，不能仅依赖 React 下一次渲染禁用按钮。
- 成功后显示“密码已复制”，按钮恢复可再次点击。成功状态不自动关闭弹窗，也不抢焦点。
- clipboard 失败时显示固定错误，允许原位重试；数据库错误保留草稿并提示稍后重试。
- notFound 或 unavailable 归为当前复制能力不可用：提示具体原因，停用该复制按钮；不继续把该入口呈现为可复制，也不修改草稿的保存状态。重新打开编辑页重新获取配置。
- 复制状态不参与 dirty，单纯复制后可直接关闭，不触发放弃修改确认。
- 新一次复制、密码草稿或认证方式变化时清除旧的完成/错误提示；已确认的“当前不可用”事实保留到重新加载配置。

### 4.4 异步结果归属

每次请求捕获服务器 ID 和请求代次。关闭弹窗、组件卸载、切换认证方式或进入放弃修改确认后，旧请求结果不再改变当前可见 UI；同步守卫在请求结束时释放。

复制期间允许正常输入、保存和关闭。用户变更草稿后，迟到的完成提示也应按代次丢弃，避免“新密码已复制”的误解。请求发出后，关闭弹窗不承诺撤销已经开始的系统剪贴板写入；不使用清空剪贴板来补偿。

不增加自动清空计时器，不读取系统剪贴板，不为本功能申请额外的前端剪贴板读写权限。

## 5. apple-design 的具体落点

| 原则                 | 编码要求                                                               |
| -------------------- | ---------------------------------------------------------------------- |
| Purpose / Simplicity | 一个明确的“复制已保存密码”入口，保留现有输入方式和表单层级             |
| Grouping & mapping   | 按钮与保存状态相邻；反馈放在同一密码区域                               |
| Response             | 按下立即有 :active 反馈，点击立即进入 copying；仅操作完成后报告成功    |
| Agency               | 复制不需要额外确认；失败可重试；草稿可以继续编辑或取消                 |
| Responsibility       | 密码不进入前端响应、日志和自动展示；使用 write_secret 标记敏感内容     |
| Craft                | 复用现有语义颜色、字体和按钮形态；复制与清空两种操作保持清楚的视觉间距 |
| Flexibility          | 键盘可操作、清楚的可访问名称、深浅主题、高对比度和窄窗口均可用         |

样式集中在 src/server-editor.css，补充局部按钮组和反馈布局。现有基础密码样式位于 src/styles.css，优先复用，不进行整体迁移。

- 按钮保持次级样式，保存仍是表单主操作；不额外添加高饱和强调或玻璃面板。
- 成功反馈使用文字和 Check 图标，不只靠颜色；普通进度/完成反馈通过 role="status" 温和播报，错误用独立 role="alert"。
- 复制期间按钮使用 aria-busy；Tab、Enter、Space 正常工作，无额外快捷键和焦点跳转。
- 预留局部反馈空间，长文案换行；按钮组窄屏自然换行，避免挤压输入框或覆盖底部操作栏。
- 沿用平台系统字体和现有字号层级；新间距优先使用 rem，保证文本放大后的可读性。
- 状态变化可使用简短颜色/透明度反馈；本功能没有手势、位移或弹性动效，不新增动画库。减少动态效果时保留即时文字和颜色反馈。

## 6. 诊断与权限

- 在 src-tauri/src/diagnostics/commands_allowlist.rs 的 known_command 中加入 copy_saved_server_password。
- 复用现有 notFound / unavailable / clipboard / validation / permission / cancelled 错误分类，无需扩大错误码集合。
- invokeDiagnosed 只记录命令、追踪 ID、耗时和结果分类；不传入密码，也不记录剪贴板内容或长度。
- 剪贴板插件的原始错误不直接拼入用户消息，不打印凭据，也不把完整 ServerConfig 交给诊断代码。
- main_only 是新命令必须执行的后端校验。现有前端剪贴板 capability 不代替这个检查。
- 通过 Rust 已初始化插件调用 write_secret；不添加前端 allow-write-secret 或 read-text 权限。
- src/types.ts、model.rs、Cargo.toml、package.json 和 capability 文件预计无需变更。

## 7. 按依赖顺序编码

| 步骤        | 修改文件                                                | 完成条件                                                             |
| ----------- | ------------------------------------------------------- | -------------------------------------------------------------------- |
| A. 数据读取 | src-tauri/src/db.rs                                     | 完成单记录参数化读取及数据库单元测试；覆盖空值、缺失和认证方式       |
| B. 原生命令 | src-tauri/src/lib.rs、diagnostics/commands_allowlist.rs | 主窗口校验、blocking 执行、write_secret 写入、固定错误和命令注册齐全 |
| C. 前端接口 | src/api.ts                                              | Promise<void> 接口只提交 id，类型检查通过                            |
| D. 编辑器   | src/ServerEditor.tsx、src/server-editor.css             | 展示条件、复制状态、连点守卫、过期反馈处理和可访问性完成             |
| E. 回归验证 | tests/connection-ui.spec.ts、src/diagnostics.test.ts    | 界面、IPC 边界和诊断测试完成，密码编辑/配置复制流程保持正确          |
| F. 原生验收 | 运行桌面应用，使用专门的测试服务器配置                  | 真实系统剪贴板、重启读取、非主窗口拒绝和主题/缩放检查完成            |

每步只改当前功能所需代码，不顺带重构其他剪贴板模块、权限体系或凭据存储。

## 8. 测试计划

### 8.1 Rust 数据库测试

在现有 db::tests 中扩展：

1. 读取密码与原值逐字一致，包括首尾空格、中文和特殊字符。
2. 空 ID、服务器不存在、密码为空、非密码认证分别返回约定分类。
3. 密码替换后再读取取得新值；清空、删除或切换认证方式后读取被拒绝。
4. 读取前后服务器配置及 updated_at 不变。
5. 已有 ServerView 序列化测试继续证明响应不包含 password。

### 8.2 界面测试与夹具

在 connection-ui.spec.ts 的现有 setup 增加 copy_saved_server_password 模拟分支，记录本次复制的服务器 ID、调用次数和可控完成/失败；默认成功只返回空值。需要密码内容时使用独立的测试专用凭据映射，不能把 password 加到 servers 数组。

覆盖场景：

1. 编辑已保存密码服务器可复制；打开页面时不自动发命令，输入框保持空值。
2. 请求只有当前 initial.id，没有新密码、hasSavedPassword 或整个草稿；响应不含密码。
3. 成功原位反馈后可以再次复制；无 save_server / duplicate_server 调用；仅复制后关闭无放弃确认。
4. 输入尚未保存的新密码时仍复制已保存值，保留 set；待清空时仍复制已保存值，保留 clear。
5. 新建、未保存密码、Agent、私钥和复制配置窗口没有入口。
6. 延迟请求期间连续点击只发起一次；剪贴板失败可重试；服务器删除或密码被移除后显示不可用。
7. 延迟请求期间关闭、保存、切换认证方式、修改密码草稿或进入放弃确认，完成后不出现过期反馈或页面错误。
8. 原有密码 keep / set / clear，以及配置复制的继承和取消测试继续通过。
9. 键盘操作不会意外提交表单；进度/成功/错误反馈有对应可访问语义。
10. Chromium / WebKit 下，深浅主题、860×580 和现有最小有效视口内，无重叠、整页横向滚动或操作栏遮挡。

在 diagnostics.test.ts 补充新命令的成功与失败诊断用例：只保留操作元数据，不出现测试密码、剪贴板正文或任意错误原文。

### 8.3 原生桌面检查

使用专门创建的测试配置和已知测试密码，避免触碰用户现有凭据：

- 复制并粘贴到测试输入区域，核对完整内容；复制两次和应用重启后复制均正确。
- 替换、清空后重新打开编辑页，确认复制入口及实际内容与数据库一致。
- 从独立工具窗口调用新命令时被 main_only 拒绝，剪贴板不改变。
- 验证操作失败时不报告成功，重试不改变服务器配置。
- 在支持的 Windows 环境检查敏感内容标记对剪贴板历史的效果；其他平台按可用环境记录结果，不把插件提示当成全面保护保证。
- 125% / 150% 缩放、高对比度、减少动态效果及键盘操作下检查可用性。

浏览器 IPC 模拟不能证明真实系统剪贴板写入成功；实施记录须区分自动化结果与原生实测结果。

## 9. 验证命令与交付

按改动执行以下检查，测试失败先修复本功能问题：

```text
npm run build
npm test
cargo test --manifest-path src-tauri/Cargo.toml db::tests
npm run test:connection -- --grep "password|密码|copy|复制"
```

选定用例通过后运行一次完整 npm run test:connection，覆盖共用编辑器的连接、配置复制和窗口交互。若新增命令或诊断用例暴露其他关联问题，再运行对应测试；不无条件扩大为所有功能套件，也不把安装包构建当成复制功能实测。

交付时记录代码变更、自动化测试结果、实际剪贴板验证结果及未具备环境的原生检查；具体结果见实现记录。

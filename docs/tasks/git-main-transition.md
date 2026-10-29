# GIT-MAIN-20261029：提交当前工作并调整主线

日期：2026-10-29 · 用户已直接授权执行 · 状态：分支调整已完成，当前工作纳入本次交付提交

用户要求：“现在把仓库提交推送一下，将目前的main分支切换到v1分支，将现在的v2切换到main。”远端实际默认分支是master，没有main；按其旧主线/新主线的意图执行master→v1、v2→main，并将main设为默认分支。

## 执行前核实

- 仓库：`BgaSol/soDeck`，当前工作目录`soDeck-v2`，只有本工作区，没有其他关联worktree。
- 旧默认master：`436cad2bc92d99eceed9367273e74543ec3c6266`。
- 远端v2：`d550c76e6d45868e698c87fa65c2a92da06136a3`；本地v2：`fc533e8b74f8d6a655753b3d2562952a92912bd2`，已有一个未推送提交，保持原样纳入历史。
- 本次将现有源码、文档、测试和构建配置统一提交；`.qa`、`.local`、node_modules、构建产物、本地测试密钥及数据保持忽略，不加入提交。新增vendored libssh2的15个密钥夹具与其公开libssh2-1.11.1上游逐件哈希核对一致，属于公开测试样例，随依赖保留。
- GitHub凭据具有仓库管理和推送权限，仅通过既有Git凭据助手在内存中使用，不输出或写入仓库。

## 操作与验证流程

1. 先更新DEVELOPMENT、AGENTS和产品决定；同步检查工作流main分支过滤及发布规划origin/main祖先门禁。发布标签前缀sodeck-v2保持，普通推送仍不触发Release。
2. 核对提交文件、`git diff --check`，运行`npm run check`及既有发布规则单测。应用代码沿用[T-029本地验证](T-029-results.md)，分支调整不改变已验证的应用产物。
3. 更名前再次核对远端SHA；通过GitHub分支rename接口保留旧master为v1、旧v2为main，设置默认分支main。两个旧tip必须在新名称下保持，不强推改写历史。
4. 本地v2更名main，设置origin/main上游，创建跟踪origin/v1的本地v1；工作目录和当前文件不迁移。一个新提交保存全部当前工作并推送main，同时保留先前未推送提交。
5. 最后核对本地/远端main相同、v1仍指旧master、远端只保留main/v1、默认分支main、工作区干净。GitHub响应与最终SHA存入忽略的`.qa/git-main-transition`，最终提交号以Git历史和交付消息为准。

## 授权边界

本次是用户明确要求的提交、推送、分支更名和默认分支设置，不要求再次批准。T-029的正式公钥/Secrets、Release、发布标签和公网安装验证仍未授权；本次只可能触发普通main检查工作流。历史任务里的“未提交/推送”说明的是当时实测基线，后续Git状态以本记录和仓库为准。

## 执行结果

提交前检查：`npm run check`和3项发布规则单测通过。全量暂存首次检查发现此前未跟踪文件的行尾/末尾空白，按本次提交整理范围只做格式清理；Markdown双空格换行改为显式反斜杠换行，保持呈现语义。第三方libssh2的INSTALL_CMAKE标题下划线被Git识别为冲突标记，另有chacha.h末尾空行，均保留上游原件并单列，不为消除提示改第三方源码。本项目文件须通过暂存diff检查。

GitHub分支更名和默认分支已核对：master→v1仍为`436cad2bc92d99eceed9367273e74543ec3c6266`，v2→main更名时仍为`d550c76e6d45868e698c87fa65c2a92da06136a3`，默认分支main。远端旧master/v2引用已不存在，两代历史未合并或强制覆盖；GitHub响应保存在`.qa/git-main-transition/rename.json`。

本地当前分支main跟踪origin/main，v1跟踪origin/v1，origin/HEAD指向origin/main。本记录与当前工作一同纳入一个新提交，推送同时包含原有未推送提交`fc533e8`；最终提交号和远端一致性由Git历史及`.qa/git-main-transition/final.json`记录，不将提交或推送解释为用户验收或实际Release。

# 工作记录（Work Log）

> **这份文档的用途**：每完成一轮工作，就在这里追加一条记录，写清**做了什么、没做到什么**。
>
> 之所以单独成文：`docs/remediation-report.md` 这类报告是**一次性**的深度文档，
> 写完之后就沉在仓库里；而"这一轮到底交付了什么、哪些没做到"是**需要按时间线连着看**的。
> 两者互补：这里给索引与结论，细节指向对应文档。
>
> **约定**（后续每轮都遵守）：
> 1. 新增一节，标题为 `## 第 N 轮 · <日期> · <一句话主题>`；
> 2. 必须有 **做了** / **没做到** / **怎么验证的** / **相关文档** 四部分；
> 3. "没做到"必须如实写，包括没验证到的（写"未验证"并说明原因），不许用"后续优化"含糊过去；
> 4. 同一轮里发现的、但没修的问题也要记在这里，避免下次重新踩；
> 5. 记录完成后随代码一起提交并推送到 `PLA0185/lumen`。

---

## 第 13 轮 · 2026-09-25 · 0.4.0 发布前重复任务数据安全与 UI 收口（阶段进度）

BASE=`a56e29d250f4c9c32199fefcb09a71c2b3210782`，应用版本仍是 `0.3.0`。本轮没有打 0.4.0 tag 或发布安装包；最终提交 SHA 和 CI 以推送后的交付回执为准。

### 做了

- 为重复发生增加历史触碰标记、统一重建安全判断和正式迁移。重建只硬删真正纯物化实例，带附件副本、子任务、提醒、依赖、焦点、标签、实际耗时或回收站状态的发生会保留为例外；结果提示保留数量。
- 「删除此次及以后」使用 UTC occurrence key 停止未来物化；COUNT/UNTIL 和系列结束字段约束有效。编辑已有系列规则支持频率、间隔、星期、月日期、第 N 个星期、结束条件、时区和预览；旧时区历史保留。
- 实机验收找出「整个系列」的新规则被旧未来分段覆盖的问题，改成较新版本覆盖旧分段后续计划，增加回归测试。
- 项目/分类合并、解除归属、级联软删及标签合并/删除同步重复系列的持久模板或停止边界。
- TaskEditor 折叠周期、标记和 Markdown 备注，缩短时间字段说明；Release 工作流改为先跑完整质量门禁，再检查签名密钥。
- 验收脚本成功时校验临时目录并删除 profile，失败时保留；新增重复规则真实 UI 验收。

### 没做到

- 任务书 16 步重复任务实机链路尚未完整覆盖重启、维护、UI 添加并保存子任务/提醒/附件/依赖/副本。
- 六个异步视图的受控延迟回写测试未齐；AI 无真实密钥，预览与 Apply 全链未实测。
- 大回收站、大量项目/标签、100+ 今日任务和大型报告的人工可用性检查未完成。0.4.0 发布条件尚未全部满足，因此没有升版本或发布。

### 怎么验证的

- 新增回归测试曾在修复前复现附件/回收站数据丢失与终止后再生成；修复后专项测试通过。Rust 全量测试与前端门禁以本轮最终交付回执为准。
- 隔离桌面验收：architecture 24/24、recurrence 14/14、scale 16/16（种子 2,000 条）、remediation3 42/42；四次成功运行的测试 profile 均已删除。remediation3 的受控在飞请求、目录联接删除分支和 AI provider 独立状态有脚本注明的未确认边界。

### 相关文档

- `docs/ui-ux-audit-0.4.0.md`：路径审计、设计取舍、未实测项与发布阻断条件。

---

## 第 12 轮 · 2026-09-24 · 全仓库架构收口（基线 `d6e76fe`）

BASE=`d6e76feec660d8532e4ddfafe7e99a22daa4fa97`。本节先记录代码与本机证据；
最终 HEAD、提交范围与 CI run 以提交推送后的交付回执为准。
2026-09-25 继续完成暂停时未验证的改动，并补跑全套门禁与隔离实机验收。

本轮代码与验收快照 HEAD=25ba1a6d7373def24860b023a970d3ba3bb38ade；已推送到 GitHub main。
GitHub CI [run 36063728654](https://github.com/PLA0185/lumen/actions/runs/36063728654)：frontend、rust 均为 success。
此处记录的是追加最终文档提交前的快照；最终 HEAD 与 CI 以交付回执为准。

```text
BASE=d6e76feec660d8532e4ddfafe7e99a22daa4fa97
HEAD=25ba1a6d7373def24860b023a970d3ba3bb38ade
```

```text
git log --oneline d6e76fe..HEAD
25ba1a6 test(scale): 覆盖 2000 条任务与依赖服务端搜索
acdd718 docs: 如实记录 PDF 实机复测未完成
d9eb7b2 fix(release): 发版门禁任一步失败立即终止
c439d78 chore(acceptance): 隔离实机验收并收紧发布门禁
622d2ba fix(recurrence): 统一范围语义、持久模板与事务保存
175b644 refactor(async): 统一最新请求生效并明确规模边界
758a207 refactor(data): 集中声明变更域并统一发布数据失效
```

```text
git diff --stat d6e76fe..HEAD
 .github/workflows/ci.yml                          |   3 +
 .github/workflows/release.yml                     |  28 +-
 docs/work-log.md                                  |  71 +++
 package.json                                      |   1 +
 src-tauri/migrations/0006_recurrence_template.sql |  57 ++
 src-tauri/src/ai_features.rs                      | 118 +++-
 src-tauri/src/commands.rs                         | 162 +++++-
 src-tauri/src/commands_e2e.rs                     |  33 +-
 src-tauri/src/db.rs                               |  13 +-
 src-tauri/src/lib.rs                              |   3 +-
 src-tauri/src/organize.rs                         |   7 +
 src-tauri/src/recurrence.rs                       |  68 +++
 src-tauri/src/recurrence_e2e.rs                   | 603 +++++++++++++++++++-
 src-tauri/src/recurrence_service.rs               | 660 ++++++++++++++++------
 src/App.tsx                                       |  62 +-
 src/components/AttachmentList.tsx                 |   2 +
 src/components/BoardView.tsx                      |  15 +-
 src/components/CalendarView.tsx                   |  46 +-
 src/components/DependencyEditor.tsx               |  57 +-
 src/components/FloatingToday.tsx                  |  59 +-
 src/components/FocusPanel.tsx                     |  24 +-
 src/components/OrganizeView.tsx                   |  19 +-
 src/components/QuickAdd.tsx                       |   4 +-
 src/components/RecurringTaskDialog.tsx            |   5 +-
 src/components/ReminderEditor.tsx                 |   2 +
 src/components/RuleEditor.tsx                     |   1 +
 src/components/ScopeDialog.tsx                    |  13 +-
 src/components/SettingsView.tsx                   |  17 +-
 src/components/StatsView.tsx                      |  34 +-
 src/components/SubtaskList.tsx                    |   2 +
 src/components/TaskCard.tsx                       |  14 +-
 src/components/TaskEditor.tsx                     | 117 +++-
 src/lib/ai-ipc.ts                                 |   2 +-
 src/lib/attachment-ipc.ts                         |   2 +-
 src/lib/backup-ipc.ts                             |   2 +-
 src/lib/board-paging.ts                           |   5 +-
 src/lib/data-change.test.ts                       |  74 +++
 src/lib/data-change.ts                            | 145 +++++
 src/lib/focus-ipc.ts                              |   2 +-
 src/lib/ipc.ts                                    |  27 +-
 src/lib/organize-ipc.ts                           |   2 +-
 src/lib/recurrence-ipc.ts                         |  18 +-
 src/lib/reminder-ipc.ts                           |   2 +-
 src/lib/request-gate.test.ts                      |  16 +
 src/lib/request-gate.ts                           |  20 +
 src/lib/stats-ipc.ts                              |   2 +-
 src/lib/store.ts                                  |  26 -
 src/lib/window-ipc.ts                             |   2 +-
 tools/check-version.mjs                           |  18 +
 tools/requirements-acceptance.txt                 |   1 +
 tools/run_acceptance.py                           | 104 ++++
 tools/verify_architecture.py                      | 320 +++++++++++
 tools/verify_remediation3.py                      |  77 ++-
 tools/verify_scale.py                             | 144 +++++
 54 files changed, 2948 insertions(+), 383 deletions(-)
```


### 做了

- **Mutation architecture**：十个前端 IPC 模块统一通过 `data-change.ts` 调用；集中声明
  mutation 影响域，在后端成功返回后同步通知本窗口，并经 Tauri event 通知其它窗口。
  Main、Board、Calendar、Floating、Stats、Organize、Focus、PDF watcher 与任务详情订阅相应域。
  组件中的 `notifyTasksChanged()` 业务调用已移除。
- **Recurrence**：普通 task 更新和批量操作不能绕过重复范围；TaskEditor 编辑、主列表删除接入
  ScopeDialog。自动物化在启动、每日维护、Calendar 查询时执行。展开按请求范围扫描，
  分段按 `[anchor, next anchor)` 生效；系列模板与标签独立持久化，v5 数据升级回填。
  ThisAndFuture 重建使用持久游标与明确错误；WholeSeries 改规则重建未来，后端强制历史确认；
  ThisOnly 改时间和提醒同事务。单次例外采用整体保护模型。
- **Async**：通用 request gate 用于 Calendar、Stats、Organize、Floating、Focus；过期请求
  不得覆盖新结果。
- **Scale**：Floating 显示真实总数与前 100 项提示；DependencyEditor 使用服务端搜索；
  Calendar 最多渲染 1000 条且展示总数与截断提示；移除生产 `task_report_all` IPC。
- **Atomic editor**：普通任务 `task_save` 在同一事务更新字段、标签和提醒，失败回滚。
  重复任务实例状态可单独保存，范围内容变动必须选 scope；「仅此次」还可原子编辑
  备注、链接、项目、分类、标签、周期与清空预计耗时。缺少分段模型的这些字段在更大范围
  编辑时明确报错，不会悄悄丢失。
- **拖拽与 AI Apply**：日历格子中的拖放提示不再因子元素间的 `dragleave` 反复卸载，
  真实鼠标拖拽可改期；AI 把第一条子任务应用到空父任务时修正 SQLite 排序值类型，
  并新增重复任务 AI 改期标记例外的回归测试。
- **Backup**：restore 发布 `all` 失效域，设置界面明确要求重启并提供重启按钮，
  提示附件恢复的语义边界。
- **Acceptance**：补充可自启、自停应用且每次创建独立临时 profile 的启动器；
  seed 失败可按前缀找回 ID，清理串行重试并断言残留为零。
- **Release**：CI 与 Release 检查三处版本一致；Release 校验 tag 与版本，复用完整质量门禁；
  门禁脚本用 bash 的 `set -euo pipefail` 确保任一步失败就停；移除把 main 当作 tag 的
  手动触发路径。仍保持 0.3.0，未发布 0.4.0。

### 没做到 / 尚未验证

- 重复任务备注、链接、归属、标签与周期目前只支持「仅此次」；「此次及以后」和「整个系列」
  没有这些字段的分段持久化模型，后端与界面均明确拒绝。状态与内容同时编辑需分开保存。
- 实机验收覆盖 QuickAdd → Main/Board/Calendar/Floating、Board 与 Calendar 真实鼠标拖拽、
  重复任务编辑/删除 scope、backup restore、回收站与分页。AI Apply 用无密钥的后端
  数据库回归测试验证了子任务创建和重复任务改期；真实服务商调用与 UI Apply 未验证。
  PDF 取消、上限及导出期间变化触发取消仍无本轮实机记录，仅有既有单元测试。
- 本轮另尝试在隔离 profile 中复测普通 PDF 导出：保存对话框确实弹出，测试任务清理后
  残留为 0，但系统对话框没有接受自动确认，未生成文件；因此本轮 PDF 实机导出也记为未验证。
- 旧版 AI provider 实机测试的四个密钥状态都为“未配置”，无法验证混合密钥状态。
- 目录联接验收证明扫描未触及目录外文件，但该联接没有进入实际删除分支。
- 0.4.0 版本更新、安装包签名与 updater assets 尚未执行；须在完整复审通过后单独处理。

### 怎么验证的

- `pnpm install --frozen-lockfile`、`pnpm typecheck`、`pnpm test`、`pnpm build`、
  `pnpm lint`、`pnpm check:version` 通过；前端 152 项测试通过。Rust
  `cargo fmt --check`、`cargo check --all-targets`、`cargo test --lib`（352 项）、
  `cargo clippy --all-targets --all-features -- -D warnings` 通过。
- 隔离 profile 实机脚本：回收站与分页 42/42，残留 0；跨视图架构 24/24，残留 0。
  规模链路脚本 16/16：2000 条当日任务下，Board 显示 200/2000 加载入口、Calendar
  显示前 1000/2000 警示、Floating 显示前 100/2000 提示；依赖编辑器分页并经服务端
  搜索命中末尾任务；残留 0。
- 新增/扩展回归：mutation 成功与失败通知、request gate、`task_save` 回滚、旧系列 3000+
  次发生、分段边界、WholeSeries 规则/标题重建、v5 升级模板回填、历史确认、并发物化、
  恢复跳过发生、仅此次字段/标签的事务性、AI Apply 空父任务与重复任务改期。

### 相关文档

- 本轮任务书：《Lumen 全仓库架构收口任务书》（外部接手说明）。
- `docs/remediation-report.md` 保留前轮整改背景；本节作为第 12 轮证据索引。

---

## 第 9 轮 · 2026-09-24 · 第三轮整改（按《Lumen 第三轮整改任务书》）

外部审查（网页版 GPT 直连仓库）交回一份第三轮任务书，基线是 `8aa8d8b`：
两个数据破坏级 P0 + 五个 P1 + 三项 P2。本轮逐条处理。

其中 P0-①（回收站范围）在收到任务书之前，我已经在送审前自查时发现并动手修了——
**但第一版只修了一半**（把计数换成后端 count，却没让删除用同一套条件）。
任务书重新点出了这个缺口，本轮的正式修法是"按筛选条件删除 + 并发一致性校验"。

### 做了

**P0-① 回收站「确认集合」与「实际删除集合」必须完全一致**

- 第 8 轮把按钮与弹窗里的数量从"已加载条数"改成了后端计数，**方向是对的**，
  但漏了一层：那个计数是**当前筛选条件**下的计数。
- 后端原先执行的是无条件的 `DELETE FROM tasks WHERE deleted_at IS NOT NULL`。
- 于是：在回收站里搜个词（比如匹配 30 条），界面写「永久删除 30 项」、
  弹窗也说 30 项，**实际删掉回收站里的全部 78 条**。
- 本轮明确产品语义为**任务书 §1.2 的方案 B「删除当前筛选结果」**（理由见下面的取舍），
  并让四者统一到同一套条件（`apply_task_filters`）：
  - `task_purge_all_deleted` 接收可选 `query`，DELETE 的范围与 `task_count` / `task_list` 完全一致；
  - 附件路径的收集也换成同一套条件（`copied_paths_of_matching_tasks`），
    否则"文件清理范围 ≠ 记录删除范围"；
  - 后端**强制** `deleted_only = true`——无论调用方传什么条件，
    这条命令都不允许碰到未删除的任务（有测试守着）；
  - 弹窗如实写出两个数字："只删除筛选结果里的 N 项；回收站共 M 项，其余会保留"。
- **并发一致性（任务书 §1.4）**：确认时把显示给用户的数字一起传下去，
  后端在**同一事务**里重新计数，对不上就整体取消并提示"回收站内容已发生变化"，
  绝不静默多删。

**P0-② 附件删除绝不能跟随符号链接删错目标**

- 原实现是 `canonicalize(candidate)` → 删**解析后的路径**。若
  `attachments/B.pdf` 是指向 `attachments/A.pdf` 的链接，而 A.pdf 属于另一个仍在使用的任务，
  那么删 B 的时候会把 **A 的真实文件**删掉，B 的链接还留着。
- 修法（任务书 §2.4）：新增统一函数 `safe_remove_managed_copy(root, candidate)`，
  用 `symlink_metadata`（**不跟随**）判断类型：
  - 符号链接 / junction（reparse point）→ **只删链接自身**，绝不跟随目标，并记 warn；
  - 普通文件 → 校验**父目录**在受控目录内，再删原路径（不是 canonicalized 的结果）；
  - 不存在 → `Ok`（幂等）；越界 / 目录 / 受控目录本身 → 拒绝。
- 任务删除、清空回收站、孤儿清理、单个附件删除**四处全部改走这一个函数**（§2.5）。
- 大小写：Windows 路径大小写不敏感，新增 `path_lexically_inside` / `same_path`
  做大小写不敏感比较，避免"数据目录改过大小写后，受控目录内的文件被误判成越界而删不掉"。

**P1-③ Provider 的 API Key 状态按 provider 独立**

- 前端切服务商时传的是**当前** provider 的 `hasApiKey`，于是"DeepSeek 存过 Key，
  切到 OpenAI 也显示已配置"。
- 后端新增 `ai_provider_key_status()`，逐 provider 从凭据管理器读真实状态；
  前端切换时用**目标 provider** 的状态，保存/清除密钥后刷新状态表。
- 原来那条断言错误行为的测试「已保存密钥的状态在切换提供商时保留」
  已改为「不同 Provider 的密钥状态相互独立」。

**P1-④ 分页的旧请求不得污染新视图**

- `reload()` / `loadMore()` 都没有"请求世代"概念：搜 A 的第二页如果最后才返回，
  会被追加进搜 B 的结果里。
- store 新增 `queryGeneration`：视图、搜索、状态筛选、排序、逾期五个入口都会 +1；
  请求开始时记下代数，回来时先比对，**对不上就丢弃**（`reload` 与 `loadMore` 都做了）。

**P1-⑤ OFFSET 分页在并发增删时的漏项**

- OFFSET 天生不稳定：已加载 1..200，别处删掉第 50 条，再用 OFFSET 200 取下一页
  就会漏掉原来的第 201 条。
- 本轮采用任务书 §5.4 明确允许的**临时方案**：跨窗口收到"数据变了"时调用
  `handleExternalChange()` —— 先把 `queryGeneration + 1`（作废所有在飞的请求），
  再从第一页重取（等于清空后续页）。**keyset 分页没有做**，见"没做到"。

**P1-⑥ 看板空页不再无限加载**

- 按钮的出现条件 `total > tasks.length` 里的 `total` 会过期，空页之后按钮可能永远在，
  点下去却什么也不发生。
- 看板改为与主列表一致的 `hasMore`：取到空页立刻停下并刷新一次真实计数；
  `hasMore` 为假但总数仍大于已加载时，渲染的是**真能用的刷新入口**而不是死按钮。

**P1-⑦ 大报告导出不再把 10 万个完整任务塞进 React**

- 原路径：后端一次返回最多 10 万条完整 Task → 前端塞进 React state → 渲染 10 万个 `<tr>`。
  大文本场景下 IPC 序列化、内存、渲染三重风险。
- 新增 `src/lib/report-export.ts`：**分页流式**取数（每页 500 条），
  逐行构造 DOM 挂到 body 上（在 `.app` 之外），**JS 里不留行数组**；
  过程中回报"已处理 / 总数"，导出按钮显示「准备中 3500/20000」；
  行数之外再加一道**总字符数上限**（2000 万字符，任务书 §7.4 要求不能只看行数）；
  结束后 `dispose()` 释放 DOM。
- 旧的 `PrintReport.tsx` 已删除；`store.reportRows/reportRowsAll` 保留但注释已更新为
  "PDF 导出不再经过它们"。

**P2-⑧ updater 的历史明文密码迁移默认安全**

- 原来迁移后还会问一次"是否删除明文"，选 N 就保留 → "DPAPI 密文 + 明文密码"并存，加密形同虚设。
- 现在：密文写入后立即做解密往返校验，通过就**立即删除**明文；
  校验失败则回滚密文并保留明文、以非零退出码结束；删除失败同样非零退出并提示"安全迁移未完成"。
- `-Status` 检测到 legacy 明文文件时输出"不安全"并返回**非零退出码**（0 = 私钥/密文就绪且无明文残留）。

**P2-⑨ 过时注释清理**

- `App.tsx` / `store.ts` 里"后端按 500 条一页读到取完""分页取全量"等描述
  与现在的"一次取 `REPORT_MAX_ROWS + 1` 条"不一致，已按实现改写。

**附：主列表的「加载更多」也不再是死按钮**

- `hasMore` 是上一次查询的快照。并发写入让它过期后，界面会渲染出「加载更多」，
  而 `store.loadMore()` 因为 `!hasMore` 直接 return——按钮存在但什么都不发生。
- 修法：`loadMore()` 先按当前条件**刷新一次总数**再决定要不要继续
  （计数很便宜，拿它当唯一依据最稳）；界面在"总数说还有、但已停止加载"时
  不再渲染死按钮，而是给一个**真能用的**刷新入口并如实说明数字对不上。

### 一次真实的数据误删事故（必须记在最显眼的地方）

**结论先说：用户回收站里原有的 18 条任务被我的验收脚本永久删除了，随后已从迁移前快照逐行恢复，数据完好。**

- **经过**：新写的实机验收脚本 `tools/verify_purge_scope.py` 第一版为了读弹窗文案，
  采用"替换 `window.confirm` → 点按钮 → **同步**恢复 `window.confirm`"的写法。
  但按钮的处理函数是 **async** 的（前面要先 `await` 一次计数查询），
  于是恢复动作发生在真正弹框之前：
  1. 脚本改回原生 `confirm`；
  2. 异步流程随后调用**原生** confirm，无头环境下它被自动接受（返回 true）；
  3. 此时脚本已进入 `finally` 并清空了搜索框，于是那次删除按**无筛选条件**执行，
     把回收站整个清空了。
- **影响**：用户回收站里原有的 18 条任务被永久删除（未删除的 4 条未受影响）。
- **恢复**：迁移前快照 `pre-migrate-20260924T024432Z.db` 里正好是"全部 22 / 回收站 18 /
  未删除 4"，与误删前完全一致。恢复步骤：
  1. 关闭应用，把当前库（含 WAL）整份复制到临时目录做保底；
  2. 只读方式附加快照，确认这 18 条**没有任何关联数据**（标签/子任务/提醒/附件全为 0）；
  3. 校验 `tasks` 表列一致后，在单个事务内按 id 插回缺失的 18 行；
  4. 验证：全部 4 → 22、回收站 0 → 18、未删除仍为 4、`integrity_check = ok`、
     逐行内容与快照一致（不一致行数 0）。
- **脚本已修**：改成**全程接管** `window.confirm`——脚本一开始就替换掉它并默认返回
  `false`（阻止一切真实删除），只有明确要执行删除的那一步才切到 `accept`，
  执行完立刻切回，直到脚本结束才还原。同时把这条教训写进了脚本的文件头：
  **异步 UI 里没有"读过就算完"的同步拦截**。
- 顺带说明：脚本的"前置断言不过就绝不下发删除"那条设计**起了作用**——
  它确实跳过了删除，问题出在同步拦截失效导致按钮的异步流程自己走了下去。

### 没做到

| 项 | 说明 |
| --- | --- |
| **keyset / cursor 分页（§5 长期方案）** | 未做。本轮按任务书 §5.4 采用临时方案（数据变化即作废在飞请求并从第一页重取）。**OFFSET 深分页的性能问题也仍在**：列表翻到几万条以后会变慢 |
| **文件符号链接（symlink）场景的真实测试** | 本机与 CI 都**没有** `SeCreateSymbolicLinkPrivilege`（`symlink_file` 报 "Administrator privilege required"）。已用 **junction**（普通用户可建、同样是 reparse point、走同一段 `is_symlink()` 代码路径）覆盖目录链接场景；**文件级 symlink 未实测**，只做了 junction |
| **报告压力测试（任务书 §7.6 的四种数据形态）** | 没有真跑"100000 条短文本 / 10000 条 20k description / 10000 条 20k note / 标签很多"这四组；流式改造的正确性目前由实机导出一份中等规模报告 + 字符上限的单元测试支撑 |
| **`-Status` 的往返校验失败分支** | 由子任务实现并在本机自测：正常与删除失败两条路径都验过；"密文写入成功但读回不一致"这个分支**无法在本机构造**，只有静态审阅 |
| **实机脚本仍跑在用户真实数据库上** | 事故暴露的深层问题没有根治：脚本仍靠"前缀 + 清理 + 基线比对"降低影响，没有做临时数据目录隔离 |
| **§12.4 界面行为没有实机证据（4/5 失败）** | 脚本在 AI 面板里没能定位到"服务商"那个 `<select>`（读到值为 `null`），四条切换断言都没跑成。**更根本的是：本机四个 provider 都没有配置密钥（全为 false），即使切换成功也"无法区分"是否真的按 provider 独立**——脚本自己也把这一点标成了"无法确认"。因此这条目前**只有单元测试证据**（前端 `configForProvider` 的 per-provider 行为、Rust `provider_key_status_covers_each_provider_independently`），没有实机证据。对应任务书 §18 的发布阻断条件第 3 条，**不能算已验证** |
| **§12.3 没有真正走到符号链接分支** | 孤儿清理改用 `symlink_metadata` 识别链接后，junction 会走到"记 warn 并跳过"那一支（保守语义），所以实机只证明了"扫描没有碰目录外的目标文件"，没有证明"走到并执行了链接分支的删除逻辑"。文件级 symlink 需要管理员权限，本机与 CI 都没有 |
| **§12.2 的"快速切换"只证明了最终状态** | 三组切词场景断言的是"用户看到的结果属于 B、没有 A 的残留"，但**无法强制制造"A 的请求恰好在切换那一刻仍在飞"的时序窗口**，所以 `queryGeneration` 的代际作废分支没有被真正触发过（该分支由前端单测覆盖） |
| **列表虚拟化（§12 P3）** | 与上一轮相同，未做 |
| **没有发布新版本** | 用户机器上仍是 0.3.0，本轮修复要发新版才用得上 |

### 新发现问题

1. **"按条件删除"这类动作要成对检查**：本轮修的是清空回收站。批量操作 `task_bulk`
   只对传入 id 生效、不在这个风险面内，但**以后只要新增"按筛选条件执行的批量动作"，
   都要按"确认范围 = 执行范围"的标准过一遍**。
2. **验收脚本自身的危险性被低估**：这些脚本会在真实库上执行破坏性操作。
   真正该有的是"隔离数据目录 + 只在隔离环境里跑破坏性验收"，
   而不是靠每个脚本各自小心。
3. **PowerShell 会把 UTF-8 文件读成 GBK 再写回**：本轮用 PowerShell 做了一次
   文本替换，结果把 `remediation2_e2e.rs` 的中文全变成乱码、换行也被吃掉，
   只能从 git 恢复重做。**所有文件内容修改一律走编辑工具**，不要用 shell 替换。
4. **serde 的 `rename_all = "snake_case"` 会把 `OpenAI` 变成 `open_a_i`**：
   `Provider` 枚举里 `OpenAI` 是连续大写，serde 逐字母插下划线，
   而前端 `AiProvider` 写的是 `'open_ai'` —— 上一轮的 `ai_provider_defaults`
   因此对 OpenAI **永远匹配不到默认值**（切过去会提示"当前版本没有该服务商的默认配置"）。
   这是本轮做 Provider 密钥状态时被 Rust 测试当场抓出来的，已加
   `#[serde(rename = "open_ai", alias = "open_a_i")]` 修正（alias 保证库里旧值仍可读）。
   **教训**：枚举值序列化到前端的契约要有测试钉住，不能只靠"看起来一样"。
5. **`REPORT_PAGE_SIZE` 仍挂着 `#[allow(dead_code)]`**（沿用上一轮记录，尚未清理）。

### 怎么验证的

```powershell
pnpm typecheck        # 通过
pnpm test             # 113 passed（第 8 轮 101）
pnpm build / lint     # 通过 / 0 problems
cargo fmt --check     # 通过
cargo test --lib      # 331 passed（第 8 轮 323）
cargo clippy --all-targets --all-features -- -D warnings   # 通过（无豁免）
```

**新增的自动化回归**

- Rust `remediation2_e2e`（+2）：
  - `purge_all_respects_filters_and_never_touches_live_tasks` —— 带搜索条件只删匹配的；
    传"看起来不限删除状态"的条件也碰不到未删除的任务；
  - `purge_all_refuses_when_the_set_changed_after_confirmation` —— 确认后集合变了必须拒绝执行，
    并用新数量重新确认后可以正常删（任务书 §1.4 / 验收用例 C）。
- Rust `attachments`（+5，其中 3 项 Windows 专属）：
  `safe_remove_refuses_outside_paths_and_is_idempotent`（越界/UNC/受控目录本身/幂等）、
  `safe_remove_handles_case_variants_safely`（大小写变体 + 同前缀兄弟目录）、
  `junction_to_outside_is_only_unlinked`、`junction_to_live_copy_dir_never_deletes_the_file_behind_it`、
  `delete_copied_files_only_unlinks_and_keeps_normal_files_safe`。
  **后四项在本机真跑了 junction**（不是跳过）。
- 前端 `store.pagination.test.ts`（+4）：旧 reload 结果不能覆盖新查询、
  旧 loadMore 结果不能追加进新条件、外部变化会作废在飞请求并从第一页重取、
  清空回收站会把筛选条件一起传下去。
- 前端 `report-export.test.ts`（+5）：字符上限的边界（`>=` 而非 `>`）、
  长描述会先于行数触顶、上限值本身有限且合理。

**实机验收 `tools/verify_remediation3.py`（38/42 通过，2 项"无法确认"）**

四段覆盖任务书 §12.1~§12.4。**关键结论：**

```
A. 回收站删除范围（§12.1，9/9）
✅ 按钮上的数字 = 搜索命中的条数（不是回收站总数） —— '永久删除 2 项'（命中 2 条 / 回收站共 23 条）
✅ 确认弹窗同时写出「筛选结果 N 项 / 回收站共 M 项」
✅ 命中的 2 条已被永久删除 / 未命中的 3 条原封不动
✅ 回收站总数只减少了命中的条数 —— 23 → 21 条

B. 分页链路（§12.2，17/17）
✅ 首屏 DOM 卡片数等于一页 —— DOM 里 200 张卡片，后端总数 2000
✅ 连续点「加载更多」能取到第 501 条 —— 当前 600 张
✅ 连续点「加载更多」能取到第 1201 条 —— 当前 1400 张
✅ 连续点「加载更多」能取到第 2000 条 —— 当前 2000 张
✅ 一直取到第 2000 条，不多不少 / 加载到底后写明「已加载全部 2000 条」
✅ 三组「快速切换搜索词」场景全部通过：列表是 B 的结果、没有 A 的残留

C. 附件 junction 不跟随链接（§12.3，8/8）
✅ attachment_cleanup_orphans 返回完整统计
✅ 对照：普通 UUID 命名的孤儿副本确实被清理（证明扫描真的跑了）
✅ 联接指向的目录外文件仍然存在 —— victim-a/b/c 全部为 True
✅ 目录外的目标目录本身仍然存在
✅ 清理只删掉 UUID 命名的托管副本，没有碰用户放进来的文件

D. AI Key 状态按 provider 独立（§12.4，1/5）—— 见下方"没做到"
```

**实机暴露的产品问题（已修）**：`cleanup_orphans_impl` 原先用 `path.is_file()` 做候选判定，
而它对目录联接会**跟随链接**去判断类型、返回 false，于是这类条目被**静默记成 skipped**：
既没有走到安全删除分支，也没有任何日志信号。现已改为先用 `symlink_metadata`（不跟随）
识别链接与 junction，**显式记一条 warn 说明"已跳过且未跟随目标"**（任务书 §2.3 允许的
"更保守：跳过并记录安全告警"分支）。

**第一次跑实机时脚本自身的一个缺陷也已修**：加载到 2000 条后「加载更多」按钮会正常消失
（`hasMore=false` 时渲染的是"已加载全部 N 条"），脚本却把它当成"找不到元素"抛异常、
中断了后续断言。修正后 B 段 17 条全通过。

### 相关文档

- `tools/verify_remediation3.py` —— 本轮实机验收脚本（42 项断言，A/B/C/D 四段）
- `tools/verify_purge_scope.py` —— 上一节那次事故之后重写的回收站范围验收（`window.confirm` 全程接管）
- `src-tauri/src/remediation2_e2e.rs` —— 回收站范围与并发一致性的回归
- `src-tauri/src/attachments.rs` —— `safe_remove_managed_copy` 与 5 项链接/越界测试
- `src/lib/report-export.ts` —— 分页流式报告构建（取代 `PrintReport.tsx`）
- `docs/remediation-report.md` —— 「第三轮整改」章节（含"第二轮三个完成结论不充分"的逐条说明）

### 交付清单（任务书 §20 要求）

```
基线 commit：8aa8d8b5d7be63933178b0bc0d76de3d48dbbd97
当前 HEAD：  见仓库 main（本轮代码提交为 ebbd15f）
```

```
$ git log --oneline 8aa8d8b..HEAD
ebbd15f fix(attachments): 孤儿清理遇到符号链接/junction 时显式告警，不再静默跳过
eb3e0d3 fix(ai): API Key 状态按 provider 独立读取，并修掉 provider 序列化成 open_a_i 的缺陷
fedbb8d fix(pagination,report): 分页旧请求不再污染新视图；大报告改为流式构建，不再把 10 万条塞进 React
daf3019 security(attachments): 删除受控副本时绝不跟随符号链接，四处删除统一到一个安全函数
3728b90 fix(trash): 回收站确认范围与实际删除范围完全一致，并加并发一致性校验
25b6bcc security(updater): 明文密码迁移改为默认安全，并把 -Status 的退出码变成有意义的状态

$ git diff --stat 8aa8d8b..HEAD
19 files changed, 3248 insertions(+), 419 deletions(-)
（含删除 src/components/PrintReport.tsx 193 行、新增 tools/verify_remediation3.py 1115 行）
```

**逐项完成情况（按任务书 §17 DoD 分类，如实标注证据等级）**

| 条目 | 状态 | 证据等级 |
| --- | --- | --- |
| P0-① 回收站确认集合 = 实际删除集合 | 完成 | 单元测试 + **实机 A 段 9/9** |
| P0-① 并发变化不静默多删 | 完成 | 单元测试（实机未构造出并发窗口） |
| P0-② 附件删除不跟随 symlink 删目标 | 完成 | 单元测试（junction 真跑）+ **实机 C 段：外部目标完好** |
| P0-② 受控目录之外文件绝不删除 | 完成 | 单元测试（越界/UNC/大小写）+ 实机 C 段 |
| P1-③ Key 状态按 provider 独立 | **代码完成，实机未验证** | 仅单元测试（前端 + Rust）。本机四个 provider 都无密钥，界面**无法区分**；脚本 D 段 4/5 未能定位到下拉框 |
| P1-④ 旧请求不污染新视图 | 完成 | 单元测试（三条）+ 实机 B 段切词 3 场景（**只证明最终状态**，未触发代际分支） |
| P1-⑤ OFFSET 并发漏项 | 临时方案已实施 | 单元测试（`handleExternalChange`）。**keyset 分页未做** |
| P1-⑥ 看板空页停止 | 完成 | 单元测试 + 代码同构于主列表（主列表已实机验证） |
| P1-⑦ 大报告不再全量进 React | 完成 | 代码 + 字符上限单测；**压力测试未做** |
| P2-⑧ updater 明文迁移默认安全 | 完成 | 脚本自测（正常 / 明文残留 / 删除失败三条路径） |
| P2-⑨ 过时注释 | 完成 | — |
| §10 高风险专项测试 | 完成（缺 AI 实机） | 见上 |
| §12 实机验收 | **38/42** | 失败 4 条 + 2 条"无法确认"已逐条说明 |
| §13 CI 门禁不降低 | 完成 | 见下方 CI 结果 |
| §14 work-log 第 9 轮 | 完成 | 本文件 |
| §15 remediation-report 追加第三轮 | 完成 | `docs/remediation-report.md` |
| 不删库 / 不清配置 / 不降低门禁 | 完成 | 本轮无任何数据库迁移 |

**仅据 CI 或文档声称、没有独立证据的部分**：本机与 CI 的测试数量、clippy 结果由
`cargo`/`pnpm` 的真实输出支撑（见上方命令块）；**CI 的最终结论以 GitHub Actions 页面的
真实运行为准**，本文件不代替它。

---

---

## 第 11 轮 · 2026-09-24 · 最终收口（基线 `0e681c7`）

> 本轮起点是 `0e681c7`。目标是把 0.4.0 的三个绝对发布阻断项修掉，并补齐
> "发布前必须能自己证明自己"的部分。

### 做了

**P0-① 单条永久删除的"检查后恢复仍被删"竞态（§2/§3/§4）**

旧实现是"先 `SELECT` 看 `deleted_at` 是不是 NULL，再 `DELETE WHERE id = ?`"，
两步之间有真实窗口：窗口 1 查到 A 在回收站 → 窗口 2 把 A 恢复 → 窗口 1 继续删，
**A 已经回到正常列表却仍被永久删除**。

现在把"仍在回收站"写进 DELETE 的 WHERE（条件删除），并检查 `rows_affected`：
只要期间有人恢复过它，就是 0 行，整体回滚并返回冲突。
副本路径改在同一事务内取（`copied_paths_of_task_tx`），避免"文件集合 ≠ 记录集合"。

**P0-② 旧的危险批量删除从 IPC 层彻底移除（§5~§7）**

上一轮只是把 `task_purge_all_deleted` 标注成"已弃用"，但它**仍然注册在
`generate_handler` 里**、前端也还留着命令常量与 wrapper —— 任何代码依旧能
`invoke("task_purge_all_deleted")` 走到那条旧路径（事务外取附件路径 + 只校验数量 +
按动态 query 删除）。现在：去掉 `#[tauri::command]` 与注册、删掉前端 CMD 常量与封装，
只保留内部 helper 给集成测试当反例。生产路径只剩两阶段。

加了两道**静态护栏**（行为测试覆盖不到"接口还在不在"）：Rust 侧对
`include_str!("lib.rs")` 与 `include_str!("../../src/lib/ipc.ts")` 断言不含旧命令名，
前端侧新增 `ipc-surface.test.ts` 做同样的检查，并反向断言两阶段命令仍在。

**P0-③ 附件删除的顺序反了（§23~§25）**

旧实现是"先删副本文件、再删 DB 记录"。第一步成功、第二步失败时会留下
**"活记录指向一个不存在的文件"**：用户看到附件还在、点开却打不开，
而且没有任何机制能自愈。

现在先删记录、**成功之后**才删文件——最坏情况只是留一个没人引用的孤儿副本，
那是 `cleanup_orphans` 能扫掉的可恢复状态。同时把逻辑抽成
`attachment_remove_impl(db, id)` 以便单测。

**P1-④ store：`fetchProgress` 那个 await 的窗口终于被测到了（§8~§10）**

代码上一轮已经改成"`await fetchProgress` 之后再查一次代数"，但**测试只卡住了
`listTasks`，没卡住 `fetchProgress`**。本轮补了两条：reload 卡在 fetchProgress
与 loadMore 卡在 fetchProgress，各自验证"旧结果不得落地/不得追加、旧进度不得混入、
`totalCount` 不得回退、`loadingMore` 不得卡住"。
突变验证：把两处二次检查注释掉 → 2 项立刻红（旧 reload 整份覆盖新结果；
旧第二页 200 条被追加到新结果上），恢复后绿。

**P1-⑤ 同窗口的变更此前根本传不到看板和 PDF（§11~§18）**

`bus.onTasksChanged` 默认忽略"当前窗口自己发出的事件"（对主列表是对的，
避免自己刷新自己），但**看板与 PDF 导出各自有独立状态**——
"主窗口在看板里用 QuickAdd 新建任务"这件事压根不会让看板刷新。

`onTasksChanged(cb, { includeSelf?: boolean })`：默认仍是 `false`（现有调用行为不变），
看板与 PDF 导出改为 `includeSelf: true`。

**P1-⑥ 看板普通 reload 不作废在飞 loadMore（§14~§16）**

上一轮明确承认了这个边界。现在 `reload()` 开头也递增 `generation`（方案 A），
`handleExternalChange` 去掉自己那次递增（避免双增导致 reload 自己的结果被判过期）。

**P1-⑦ PDF 取消与上限语义（§19~§22）**

- 取消检查从"每页开始"下沉到**每一行写入之前**（点取消不用等整页 500 行跑完）；
- **写文件前的最后一次检查**：原来第一次检查之后还有"两帧 + 180ms"，用户在这段时间
  点取消仍会落下一个 PDF；现在 `exportPdf` 之前再查一次；
- 上限语义统一为"写入前预判"：累计**恰好等于**上限的内容可完整写入，超过才停
  （原来是"达到即停"）。抽出 `shouldStop` / `rowGate` / `consumeRows` 三个纯函数，
  让构建器与测试共用同一份判定，页脚按真实原因写（取消 / 行数超限 / 字符超限）。

**P1-⑧ 破坏性验收彻底封死生产 profile（§26~§29）**

- **删掉 `--allow-production` 绕过开关**（连同 `PRODUCTION_WARNING` 等符号）；
- 目录判定从 `abspath + normcase` 升级为 **`realpath + normcase`**：
  以前把 `C:\Temp\fake-test` 做成指向生产目录的 junction 就能骗过校验，现在会被识别并拒绝；
- 抽出纯函数 `decide_profile()`（原因码 `read-failed` / `empty-data-dir` /
  `production` / `expect-mismatch`），并加了隐藏自测入口，22 项全通过
  （含**真的用 `mklink /J` 建 4 个 junction 别名再删掉**）；
- 反向验证：把判定换回旧的 `abspath` 写法 → 同样的自测 18/22、exit 1，
  红的正是那 4 条 junction 用例。

**顺手补的东西**

- 应用侧新增 `LUMEN_TEST_DATA_DIR`（只认环境变量，不接受命令行参数），
  启用时打一条 warn 明确说明"本次不使用用户真实数据目录"。

### 实机验收（§33）

**在隔离 profile 下跑通了**（这是上一轮欠的账）：

```
$env:LUMEN_TEST_DATA_DIR = "$env:TEMP\lumen-acceptance"
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=9222"
pnpm tauri dev
python tools/verify_remediation3.py --expect-data-dir "$env:TEMP\lumen-acceptance"
```

启动日志确认走的是隔离目录：

```
[WARN] 已启用隔离 profile（LUMEN_TEST_DATA_DIR）：C:\Users\win\AppData\Local\Temp\lumen-acceptance
       —— 本次运行**不使用**用户真实数据目录 C:\Users\win\AppData\Roaming\com.pla0185.lumen
[INFO] Lumen 启动，数据目录：C:\Users\win\AppData\Local\Temp\lumen-acceptance
```

脚本自己也确认了：`dataDir = …\lumen-acceptance（生产目录=False）`，
**没有被拒绝**（说明"隔离就放行"这条路径是通的），B 段 17 项、C 段 8 项全通过。
跑完关掉实例并清理隔离目录；**用户真实数据目录全程未被触碰**。

**本轮结果 35/42**，7 项失败里有 4 项是上一轮就存在的 D 段问题（本机四个 provider
都没配密钥，界面**无法区分**"按 provider 独立"，脚本也没能定位到那个 `<select>`），
另外 3 项是**脚本自身的清理缺陷**：A 段残留 1 条、结束时残留 1460 条、
"用户原有数据未被改动"因此判负。这三项不是产品缺陷，但必须记下来：

- 好在这次跑在隔离目录里，残留只是临时目录里的垃圾（已整目录删除）；
  这也正好说明"隔离 profile"这条要求不是形式主义。

### 没做到

| 项 | 说明 |
| --- | --- |
| **§33 的 A/C/D 三项没有实机验收** | 单条 purge 竞态（A）、PDF 同窗口变化（C）、PDF 手动取消（D）都**只有单元测试证据**，没有在真实界面上制造出对应时序。A 的竞态需要两个连接同时操作，实机难以稳定复现；C/D 需要在真实导出过程中点击/并发写入，本轮没做 |
| **验收脚本的清理缺陷没修** | 上面那 3 项失败（残留 1460 条）只在隔离目录里复现过，没有去修脚本的清理逻辑，也没有再跑一遍验证 |
| **symlink（`mklink /D`）形态未实测** | 需要管理员或开发者模式；junction 与 symlink 在 `realpath` 里走同一条解析路径，但这是推断 |
| **`LUMEN_TEST_DATA_DIR` 指向生产 junction 别名**未实测 | 判定逻辑上会被"别名 → 生产目录"那条覆盖，但没有真的这样启动过 |
| **PDF 逐行取消的真实时延**未实测 | 只能证明判定逻辑正确，证明不了"点取消后立即停下"的实际观感 |
| **Board 的多窗口真机投递**未实测 | bus 那一层用替身事件总线真跑通了（`vi.stubGlobal` + 内存总线），但真实 Tauri emit 是否确实投递给发送者窗口没在本机核实 |
| **keyset 分页仍未做**；文件级 symlink、报表四组压力数据、updater `-Build`/`-Clear` 同前两轮 | 沿用未做清单 |
| **没有发布 0.4.0** | 本轮修完了三个发布阻断项，但按任务书 §40 还需复审通过才允许发布 |

### 新发现问题

1. **"标注为 deprecated"不等于"不可达"**：上一轮把危险命令标了"已弃用"就以为安全了，
   但它仍注册在 IPC 里。**接口是否暴露要单独验证**——所以本轮加了读源码文本的静态护栏。
2. **竞态要修在"条件的原子性"上，不是"检查得更仔细"上**：把"仍在回收站"写进
   DELETE 的 WHERE，比在它前面加多少次 `SELECT` 都管用。这与上一轮"数量 vs 身份"是同一类教训。
3. **删除顺序决定失败模式可不可恢复**：先删文件再删记录 → 失败留下"活记录指向不存在的文件"
   （不可自愈）；反过来 → 最坏只是一个能被扫掉的孤儿。**选可恢复的那一侧。**
4. **默认忽略自己的事件，对"有独立状态的消费者"是错的**：同一份事件语义，
   主列表要 `false`、看板与 PDF 要 `true`。这类差异必须由调用方显式声明。
5. **验收脚本自身也要被验收**：本轮它跑了 42 项、通过了 35 项，却没清理干净自己的数据。
   脚本的"收尾断言"这次真的抓到了它自己的问题——这正是那两条断言存在的意义。

### 怎么验证的

```powershell
pnpm typecheck        # 通过
pnpm test             # 148 passed（第 10 轮 125）
pnpm build / lint     # 通过 / 0 problems
cargo fmt --check     # 通过
cargo test --lib      # 340 passed（第 10 轮 335）
cargo clippy --all-targets --all-features -- -D warnings   # 通过（无豁免）
```

**新增的自动化回归**

- Rust（+5）：`single_purge_refuses_task_restored_after_initial_check`（检查后恢复 →
  条件删除 0 行、任务仍在、`deleted_at` 为 NULL、副本文件仍在、真实入口报冲突）、
  `dangerous_bulk_purge_command_is_not_exposed_anymore`（静态源码护栏）、
  `attachment_remove_deletes_the_record_before_the_copied_file`、
  `attachment_remove_keeps_the_file_when_the_db_delete_fails`（用触发器让 DELETE 必然失败，
  断言文件仍在、记录仍在）、`attachment_remove_never_touches_the_referenced_original`。
- 前端（+23）：store 两条 fetchProgress 竞态；board 五条（含 reload 作废在飞 loadMore、
  includeSelf 开/关两条路径）；report-export 十二条（行数/字符各三档边界、取消三例、
  取消优先于超限）；ipc-surface 四条静态护栏。
- **突变验证**（证明测试不是摆设）：store 二次检查注释掉 → 2 红；
  report 字符判定改回 `>=` → 3 红；取消检查只查第一行 → 1 红；
  验收脚本判定改回 `abspath` → 4 条 junction 用例红、exit 1。

### 相关文档

- `docs/remediation-report.md` —— 「最终收口」章节（含本轮新发现的 7 条）
- `tools/verify_remediation3.py` —— 实机验收脚本（已无任何生产绕过开关）

---

## 第 10 轮 · 2026-09-24 · 第三轮**收口**整改（基线 `dd950d5`）

> 本轮起点是 `dd950d5`。`8aa8d8b → dd950d5` 那 7 个提交属于**上一轮**，
> 不能拿来充当本轮的完成证据——本轮提交全部落在 `dd950d5..HEAD` 区间内。

复审又找出一批"看着闭环、其实还能被绕过"的点，其中两个是数据破坏级。

### 做了

**P0-① 永久删除改为"精确 ID 快照"两阶段（§2/§3/§4/§5/§6）**

上一轮用"确认时数量 == 执行时数量"防并发多删。**数量相等不代表集合相同**：

```text
确认时 = {A}，count = 1
执行前：A 被恢复、B 被移入且同样命中筛选
执行时 = {B}，count 仍然是 1   ← 校验通过，用户确认删 A、后端删掉 B
```

现在拆两阶段：`task_prepare_purge_deleted(query)` 返回 `{ taskIds, count }`（当前命中的**精确**集合）；
`task_commit_purge_deleted(query, taskIds)` **只删这批 ID**，并在同一事务内先核对集合是否仍是这一批。

- commit 顺序严格为：`BEGIN` → 重新取命中集合与 `taskIds` **逐个比对** →
  基于**同一批 ID**、在**同一事务**里查 copied 附件路径 → 只 DELETE 这批 ID → `COMMIT` → 最后才删文件。
- 集合一旦不同（ID 被恢复、消失、或被别的任务替换）→ 整体回滚，**零任务零附件被删**。
- 不采用服务端 `operationId → IDs` 会话状态：ID 交给调用方持有、commit 时原样传回，
  进程重启与窗口刷新都不会让快照失效。
- 旧命令（只校验数量）保留但标注**已弃用**，前端不再调用。

**P0-② legacy 绝对路径的身份比较不再看大小写（§14/§15/§16）**

库里可能存着第二轮之前写入的**绝对** `stored_path`，盘符/目录大小写与当前 `data_dir` 不同、
或用 `/` 分隔。原比较走 `strip_prefix`（逐组件精确比较），匹配不上就回退成绝对路径字符串，
于是**仍被引用的 live 副本被判定为孤儿并删除**。新增 `normalize_managed_identity()`
（词法归一化 + Windows 下统一小写与分隔符），只用于"是不是同一个附件"，
**绝不用来决定 symlink 的删除目标**。

**P1-③ 分页：`fetchProgress` 之后必须再查一次代数（§9/§10）**

原写法 `set({ tasks, progressMap: await fetchProgress(tasks) })` 把代数检查放在 `await` **之前**，
留下窗口：检查通过 → 挂起 → 用户切条件且新结果落地 → 旧结果返回并覆盖。
现在 `await` 之后再查一次，`reload` 与 `loadMore` 都改了。

**P1-④ 看板响应跨窗口变化（§12/§13）**

看板原先没接 `bus.onTasksChanged`，别的窗口删任务后继续用旧 offset 翻页会漏项/重复。
现在把分页与代际作废抽成不依赖 DOM 的状态机 `src/lib/board-paging.ts`
（`createBoardPaging` + 纯函数 `nextBoardState`），组件用 `useSyncExternalStore` 读快照；
外部变化时代数 +1、清空已加载页、重载第一页；在飞的 `loadMore` 结果被丢弃且不会卡在「正在加载…」。

**P1-⑤ 报告：两道上限逐行检查 + 真正可取消（§17~§21）**

新增 `REPORT_MAX_ROWS`（10 万行），与字符上限一起构成两道闸；检查点从"一页渲染完"
提前到**每一行 append 之前**（按页检查时"500 条 × 每条 20 万字符"会先被整个塞进 DOM，上限形同虚设）；
「取消导出」是真取消（每次取下一页、写下一行之前都会问，取消后不写出文件）；
**导出期间数据变化即中止**（否则会交付一份前半旧、后半新的自相矛盾报告）。

**P2-⑥ updater `-Status` 变成完整健康码（§22/§23）**

原来只有"私钥与密文**同时**缺失"才非零、**解密失败只打印不改码**。现在四项独立计分
（私钥存在 / 密文存在 / 能解密 / 无 legacy 明文），任一不达标即 `exit 1`。

**P2-⑦ 破坏性验收必须跑隔离 profile（§24/§25/§26）**

上一轮那次误删 18 条数据的教训。应用侧：设置 `LUMEN_TEST_DATA_DIR` 即改用该目录
（只认环境变量，不接受命令行参数）。脚本侧：**造任何数据之前**先读 `app_data_paths`，
规范化比较后若是生产目录就打印拒绝说明并 `exit 1`，**一个写操作都不发**；
新增 `--expect-data-dir` 与 `--allow-production`（后者必须显式传、首尾各警告一次）。
> **第 11 轮更新**：`--allow-production` 已被**彻底删除**（任务书 §26/§27 —— 本项目真实误删过数据，
> 这类绕过开关不该存在），目录判定也升级为 `realpath + normcase`。上面这句描述的是第 10 轮当时的状态。
confirm 接管、`ZZR3-` 前缀、按 id 清理全部保留，但降级为第二层。

### 没做到

| 项 | 说明 |
| --- | --- |
| **隔离 profile 下的实机验收没跑** | 应用侧开关与脚本拒绝逻辑都做完并各自验证过，但**没有真的在隔离 profile 下跑过完整验收**——A/B/C/D 四段断言在新代码下没有实机证据。这是 §24~§26 的核心要求，**如实记为未验证** |
| **keyset / cursor 分页仍未做** | 与上一轮相同。看板还有个已知边界：`reload` 不作废在飞的 `loadMore`，拖拽后的重载与在飞翻页重叠时可能带进一条过时任务，下次重载即修正 |
| **文件级 symlink 未实测** | 本机与 CI 都没有 `SeCreateSymbolicLinkPrivilege`，仍只用 junction 覆盖同一段代码路径 |
| **报表四组压力数据没跑** | 100000 短文本 / 10000×20k 描述 / 10000×20k 备注 / 多标签 |
| **updater `-Build` / `-Clear` 未跑** | `-Build` 会真的触发 `pnpm tauri build`；本轮只验证 `-Status` 的六种场景 |
| **"数据变化即取消导出"没有端到端验证** | 逻辑与接线都在，但没在真实导出过程中制造一次并发写入来观察它中止 |
| **没有发布新版本** | 用户机器上仍是 0.3.0 |

### 新发现问题

1. **"数量一致"很容易被当成"集合一致"**：上一轮就在这里闭环失败。凡是"确认一个集合、
   之后按它执行"的动作，都必须把**身份**带下去，数量只能当快速失败的预检。
2. **`await` 之后的守卫会漏**：只在 `await` 前检查代数是"看着对、其实有窗口"的典型写法。
3. **上限检查的位置决定它是不是摆设**：按页检查与按行检查，在极端数据下完全不同。
4. **`path.is_file()` 会跟随链接**：本轮又在孤儿清理上踩到一次。

### 怎么验证的

```powershell
pnpm typecheck        # 通过
pnpm test             # 125 passed（第 9 轮 113）
pnpm build / lint     # 通过 / 0 problems
cargo fmt --check     # 通过
cargo test --lib      # 333 passed（第 9 轮 331）
cargo clippy --all-targets --all-features -- -D warnings   # 通过（无豁免）
```

**新增的自动化回归**

- Rust（+2，`remediation2_e2e.rs`）：
  `commit_purge_refuses_when_count_is_same_but_identity_set_changed`（回收站 3 条 → prepare →
  **恢复 1 条再补 1 条、数量仍是 3** → commit 报冲突、零删除、确认过的那条仍在；
  **这正是上一轮"只校验数量"能通过的场景**）与
  `commit_purge_deletes_exactly_the_confirmed_ids_and_their_copies`（集合变化被拒时
  **附件文件原封不动**；重新确认后记录与副本一起清理）。
- Rust（+2，`attachments.rs`）：`normalize_managed_identity` 的大小写/分隔符/`.`/`..` 变体、
  `legacy_absolute_stored_path_with_different_case_is_still_referenced`。
- 前端（+11，`board-paging.test.ts`）：外部删除后重载再翻页**无重复无遗漏**、
  在飞 `loadMore` 被丢弃且不卡住、`countTasks`/`listTasks` 之间发生外部变化则整份作废、
  空页停下、过期 `loadMore` 不擦掉新一代 loading、卸载后不回写。
- 前端（+2，`store.pagination.test.ts`）：两阶段删除原样带上确认时的 ID；冲突时提示并刷新。
- **突变验证**：看板去掉 `generation += 1` 红 2 项、去掉 `loadMore` 最后一次代数检查红 3 项；
  附件改回旧写法则 legacy 测试红（live 文件确实被删）——证明这些测试不是摆设。

### 相关文档

- `tools/verify_remediation3.py` —— 实机验收脚本（现在默认只跑隔离 profile）
- `src/lib/board-paging.ts` —— 看板分页状态机（不依赖 DOM，可直接单测）
- `docs/remediation-report.md` —— 「第三轮收口」章节

---

## 第 8 轮 · 2026-09-24 · 第二轮整改（按《Lumen 第二轮整改任务书》）

上一轮把"看起来实现了"提到"数据语义明确、异常不静默出错"，这一轮处理复审后剩下的
**配置闭环、数据可见性、删除范围一致性、附件清理、导出边界与工程约束**问题。
任务书 §20 列的 8 项优先级全部处理；前 4 项（OpenAI 配置死锁、500 条硬上限、
回收站确认范围、附件孤儿文件）都已完成。

### 做了

**P1-§2　Provider 默认值收敛到唯一来源**

- 问题现场：后端已经把 OpenAI 默认模型改成空字符串（未经验证不填），
  前端 `ai-ipc.ts` 里却还留着一份 `PROVIDER_DEFAULT_MODEL`，OpenAI 那格写着
  一个 **Azure** 的模型 ID。用户在界面上切到 OpenAI 会被自动填入它，
  **而且保存不报错**——漂移是静默的。
- 处置：新增后端命令 `ai_provider_defaults`，把 4 个提供商的
  `label / baseUrl / model / timeoutSeconds / maxOutputTokens / dataPolicyNote /
  keyEntry` 一次性给出；前端删掉 `PROVIDER_DEFAULT_MODEL`、`PROVIDER_DEFAULT_BASE`、
  `PROVIDER_LABELS` 与数据政策文案，只保留 `configFromDefaults` /
  `configForProvider` 两个纯映射函数。
- 加了跨端护栏测试 `frontend_has_no_duplicate_provider_defaults`：
  扫描 `src/**/*.ts(x)`（跳过测试文件），一旦再次出现那几个标识符就失败。
  这条护栏在本轮**当场生效过一次**——它先抓出了写在注释里的同一个词。

**P1-§3　"空模型 → 无法保存 Key → 无法拉模型列表"死锁**

- 根因是一个 `validate()` 同时承担"保存校验"与"运行校验"两件事。
- 拆成三个方法，职责不再重叠：
  - `validate_endpoint()`：只校验 Base URL（唯一的端点判断入口）；
  - `validate_for_save()`：保存时用，**允许模型为空**；
  - `validate_for_run()`：真正发起调用前用，模型为空直接拒绝并提示
    "请先获取模型列表"。
- `ai_list_models` 也改成只校验端点，不再因为模型为空而不可用。
- 界面同步：模型为空时给出"该服务商默认模型未经核实，请先保存 Key 再拉列表"的说明；
  「测试连接」在没选模型时禁用并说明原因（它内部会真正发请求，空模型必然失败）。

**P1-§4 / §10　任务列表不再硬截 500 条**

- 后端把筛选条件抽成 `apply_task_filters`，新增 `task_count(query)`，
  **count 与 list 共用同一份 where 子句**——任务书 §10.1 点名的那种
  "界面显示 1200、列表只有 1137"不可能再发生。
- 前端 store 增加 `totalCount / hasMore / loadingMore / nextOffset` 与 `loadMore()`；
  一页 `PAGE_SIZE = 200`。首屏只取一页，条件一变就 `offset = 0` 并整体替换结果。
- 界面底部常驻"已显示 X / 共 Y 条"与「加载更多（还有 N 条）」，滚动到底自动加载
  （IntersectionObserver，`rootMargin: 400px`）。看板视图同样按页加载并如实显示总数。
- 顺带修掉一个**实机验收才发现**的问题：搜索词只在按回车时才刷新，
  输入过程中列表与总数都停留在旧条件上。现在输入即刷新（250ms 防抖），
  条件变化必然把 offset 归零（任务书 §4.5）。

**P1-§5　回收站确认数量与实际删除数量一致**

- 确认弹窗与按钮文案改用后端 `task_count`（真实总数），不再用 `tasks.length`。
- 实测：回收站 1218 条、界面只加载 200 条时，按钮写的是
  **「永久删除 1218 项」**，弹窗明确写出"当前界面只加载了前 200 条，
  删除范围为回收站中的全部 1218 条"；成功提示用后端返回的 `purged`。

**P2-§6　永久删除时清理 copied 附件，并补上孤儿清理能力**

- 顺序按任务书 §6.4：事务**前**取出副本路径 → 删除任务并提交 → 提交**之后**再删文件。
  文件删除失败只记日志，不回滚已经完成的删除（回滚会退化成"记录没了、文件还在"，更糟）。
- reference 模式的原文件**绝不触碰**（有专门的断言守着）。
- 新增 `attachment_cleanup_orphans()`：扫描受控附件目录，删除"命名是 Lumen 生成的
  UUID 形式 + 数据库无引用"的文件；用户自己放进目录的文件与子目录一律跳过并计数。
- 删除前的越界校验做了两层：先**词法归一化**（挡住 `../` 与目录外绝对路径，
  不依赖文件是否存在），再 `canonicalize`（挡住符号链接）。第二层是必需的：
  只做后者时，"越界但文件不存在"的路径会被记成"已清理"，掩盖数据库被改坏的事实。

**P2-§7　报告恰好 100000 条时不再误报截断（顺带修掉一个真实的性能问题）**

- 截断判断改成"**多读一条**"：只有确实读到第 100001 条才算截断，
  恰好 100000 条时如实报 `truncated = false`。
- 实现时发现上一轮的"按 500 条一页读到取完"在 10 万条时是 **O(n²)**：
  OFFSET 深分页每页都要重新排序并跳过前面所有行。
  **实测单次导出 55.9 秒**。报告本来就是"全都要"，分页毫无收益，
  于是改成**一次取全量**（`REPORT_MAX_ROWS + 1` 条），并给标签查询分了块
  （一次拼 10 万个 id 会超过 SQLite 的变量上限）。
  改完同一份数据的导出耗时 **1.58 秒**（约 35 倍）。

**P2-§8　CI 与文档的 clippy 门禁对齐**

- CI 里原先带着 `-A clippy::too_many_arguments -A clippy::type_complexity`
  两条豁免，文档写的却是"所有警告全部禁止"，两者不是同一套规则。
- 实测去掉豁免并加上 `--all-features` 后仓库全绿，因此采用任务书推荐的**方案 A**：
  CI 与本地跑同一条命令 `cargo clippy --all-targets --all-features -- -D warnings`。
  `AGENTS.md`、本文件、`docs/remediation-report.md` 的描述已统一。

**P2-§9　更新签名密码不再明文与私钥并排存放**

- 新增 `tools/updater-secret.ps1`：密码用 **Windows DPAPI**（当前用户范围）加密，
  存成 `lumen-updater-password.dpapi`；`-Status` / `-Set` / `-Build` 分别对应
  查看状态、交互式录入、载入签名信息并构建。
- 已在本机完成迁移：明文 `lumen-updater-password.txt` 已删除，
  迁移时做了 DPAPI 往返校验，确认能解密之后才删的。
- 如实说明边界（也写进了 `AGENTS.md`）：DPAPI 挡的是"文件被拷走 / 别的账户读到 /
  误提交进仓库"，**挡不住**本机同账户下的恶意进程；那种威胁模型需要硬件密钥，本轮没做。

**§16 / §17　文档**

- 本文件新增第 8 轮；`docs/remediation-report.md` 新增「第二轮复审整改」章节（不覆盖上一轮）。

### 没做到

| 项 | 说明 |
| --- | --- |
| **列表虚拟化（§12，P3）** | 未做。任务书允许"先只实现分页 + Load More"，本轮走的就是这条。实测 1200 条时首屏渲染 200 张卡片已可用，但**没有测量**过 DOM 数量对滚动帧率的影响 |
| **10 万条时的界面表现** | 性能数字全部来自 Rust 侧集成测试（后台路径）。前端把 10 万条卡片渲染出来的情况没测过，也不打算支持（分页就是为此存在的） |
| **多显示器 / 托盘组合的完整枚举** | 与上一轮相同，仍未逐一手工走一遍 |
| **WCAG 对比度数值测量** | 仍未用工具测过 |
| **代码签名证书** | 安装包仍未签名，SmartScreen 会提示"未知发布者" |
| **本地模型部署指引** | 仍是文档缺口 |
| **`App.tsx` / `commands.rs` / `ai.rs` 大文件拆分** | §13 列为不强制项，未做；`commands.rs` 本轮又长了约 200 行 |
| **DPAPI 密码轮换** | 只做了"密码怎么存"，没做"怎么换、换了之后旧版本还能不能升级"的验证 |
| **前端没有组件级测试** | 仓库里没有 jsdom / testing-library，本轮新增的前端测试仍是纯函数与 store 状态机。按钮文案、弹窗内容这类断言只能靠实机脚本 |
| **没有发布新版本** | 本轮改动只落在仓库里；用户机器上安装的仍是 **0.3.0**（不含分页、回收站确认数量、附件清理这些修复）。要真正用上，需要发一个 0.4.0 并走一次应用内升级。本轮没做，因为任务书没要求发版，而构建发布要动签名密钥与 Release |

### 新发现问题（本轮发现、**没有修**或只修了一部分的）

1. **搜索输入不触发刷新**——本轮发现并已修（见 §4），但它同时说明界面里可能还有
   别的"改了状态不刷新"的地方；本轮只排查了搜索。
2. **列表分页在深分页时同样是 O(n²)**：`task_list` 仍用 OFFSET 翻页，
   1200 条无感，但翻到很深处（几万条）会变慢。报告路径已改成一次取全量，
   **列表分页没改**。这是取舍：列表一次只取 200 条、用户手工翻页，
   真要解决需要 keyset 分页，本轮没做。
3. **附件与备份的关系没变**：备份文件仍不含附件本体，搬机器要手工复制
   `attachments` 目录。本轮只加了孤儿清理，没做"附件随备份一起打包"。
4. **`REPORT_PAGE_SIZE` 现在是个只读常量**（带 `#[allow(dead_code)]`），
   保留是为了说明历史语义；下一轮若确定不需要，应当删掉而不是挂着豁免。

### 怎么验证的

**本机实跑（命令与结果）**

```powershell
pnpm install --frozen-lockfile
pnpm typecheck        # 通过
pnpm test             # 101 passed（原 85，新增 16）
pnpm build            # 通过
pnpm lint             # 0 problems
cd src-tauri
cargo fmt --check     # 通过
cargo test --lib      # 323 passed（原 310，新增 13）
cargo clippy --all-targets --all-features -- -D warnings   # 通过（无豁免）
```

**10 万条报告导出的实测数字**（`remediation2_e2e.rs` 的计时输出）：

| 阶段 | 耗时 |
| --- | --- |
| 用递归 CTE 插入 99999 条 | 0.80 s |
| 导出 99999 条（改前） | **55.9 s** |
| 导出 99999 条（改后） | **1.58 s** |
| 导出 100000 条 | 2.75 s |
| 导出 100001 条（应截断） | 3.24 s |

**新增的自动化回归（`src-tauri/src/remediation2_e2e.rs`，11 项）**

- 1200 条逐页读全：不重不漏、并集等于插入集合；
- 排序键全部并列时两遍读取顺序一致，换页大小也不变；
- **16 种筛选条件下 `count` 与列表条数逐一相等**（§10.1 那条不变量的直接断言）；
- 回收站 1200 条：确认数量 = 后端 count = `purge_all` 返回的 `purged`，
  且未删除的任务不受影响；
- copied 附件随永久删除消失、reference 原文件保留、清空回收站清理多个副本、
  用户自己放进目录的文件不删、三种越界写法一律拒绝；
- 报告 99999 / 100000 / 100001 三个边界；
- 2000 条数据量专项：13 个视图逐一断言 `count == 实际读到条数`，分页无重复项。

前端新增 16 项（`src/lib/ai-ipc.test.ts` 7 项、`src/lib/store.pagination.test.ts` 9 项）：
Provider 默认值映射、切换不残留旧模型、分页追加不重复、空页会停下、
条件变化 offset 归零、加载失败不破坏已加载内容、搜索输入自动刷新。

**实机验收（`tools/verify_remediation2.py`，15/15 通过）**

在真实实例上造了 1200 条任务，逐项断言并用真实鼠标事件点击：

```
✅ 首屏没有一次渲染全部（分页生效） —— DOM 里 200 张卡片，远小于 1200
✅ 界面明确写出「已显示 X / 共 Y」 —— 实际文案：'已显示 200 / 1200 条'
✅ 滚动到底部会自动加载下一页 —— 200 → 400 张卡片
✅ 点「加载更多」能继续往下取 —— 400 → 600 张卡片
✅ 重新加载后首屏顺序稳定 —— 前 50 张卡片逐项一致
✅ 回收站按钮显示的数量等于后端真实总数 —— '永久删除 1218 项'，后端 count = 1218
✅ 确认数量不是「已加载条数」 —— 界面已加载 200 条，按钮写的是 1218 条
✅ 二次确认弹窗里也是真实总数
✅ 附件孤儿清理命令可用且返回完整统计
✅ 用户原有数据未被改动 —— 全部 22→22，回收站 18→18
```

脚本在 `finally` 里逐个按 id 永久删除自己造的数据（**刻意不用"清空回收站"**，
那会连用户自己回收站里的任务一起删掉），并在结束时断言这批数据归零。

**GitHub Actions：一次"本地全绿、CI 红"的排查**

第一次推送（`1b51ee2`）后 CI 的 `rust` job 在 **clippy 步骤失败**，而本机跑
**完全相同的命令**是绿的。排查过程与结论：

| 步骤 | 结果 |
| --- | --- |
| 本机确认版本 | `rustc 1.98.1`、`clippy 0.1.98`，与 CI 的 stable 一致 |
| 关闭增量编译 + touch 源文件重跑 | 仍然全绿（16 s，确实重新检查了） |
| 清理本 crate 产物后重跑 | 仍然全绿 |
| 把仓库根的 `dist/` 改名后再跑 | **复现失败**：`proc macro panicked: The frontendDist configuration is set to "../dist" but this path doesn't exist` |

**根因**：`cargo clippy --all-features` 会启用 tauri 的 `custom-protocol`，
该 feature 下 `tauri::generate_context!()` 必须把前端产物嵌进二进制；
而 CI 的 rust job 原先只编译 Rust、**不产出前端**。
`cargo check --all-targets`（不带 `--all-features`）不触发这条路径，
所以只有 clippy 这一步红——这就是"看起来莫名其妙的 CI 专属失败"。

**修复**：rust job 增加 `pnpm install --frozen-lockfile` + `pnpm build`；
`AGENTS.md` 的门禁把 `pnpm build` 标成"不可省的一步"，
并在「项目特有的坑」表里新增这一条。

**顺带改进了可诊断性**：Actions 的**原始日志下载需要认证**，
未登录时网页也不渲染日志内容，于是让 clippy 步骤在失败时把尾部输出写进
**job summary**（页面上可见）。这次能快速排除"版本差异"这类猜测，
靠的就是这条铺垫。

修好后的提交 **`862f423`** 上 CI `frontend` 与 `rust` 两个 job **均为 success**。
（此后若只有文档改动，提交号会继续前进；以仓库 HEAD 的 Actions 记录为准。）

### 相关文档

- `docs/remediation-report.md` —— 「第二轮复审整改」章节（逐项状态、修改文件、未解决问题）
- `tools/updater-secret.ps1` —— 更新签名密码的 DPAPI 管理脚本
- `tools/verify_remediation2.py` —— 本轮的实机验收脚本
- `src-tauri/src/remediation2_e2e.rs` —— 本轮新增的 11 项集成回归
- `AGENTS.md` —— clippy 门禁与 updater 密钥约定的更新

---

## 第 7 轮 · 2026-09-24 · 建立"每轮留工作记录"的机制

用户提出："以后任务做完了之后同步整理一份文档到 GitHub，用于记录你做了什么没做到什么。"
这一轮做的事就是**把这个习惯固定下来**，并且把过去几轮补上。

### 做了

- 新建 **`docs/work-log.md`**：把第 1～6 轮按时间线补齐，每轮都写
  **做了 / 没做到 / 怎么验证的 / 相关文档** 四部分。历史内容来自各轮的
  提交记录、`docs/remediation-report.md`、`docs/test-report.md` 与实机验收脚本输出，
  没有凭印象编造；当时如实列过"没做到"的，原样保留（例如第 1 轮的 26 项缺口）。
- 新建 **`AGENTS.md`**（仓库根，人和自动化代理都先读它）：把工作记录约定、
  "不许假实现/假测试/假结论"、数据安全底线、提交习惯、提交前门禁、
  以及这个项目已经踩过的坑（`Separated::push`、`COALESCE(MAX())`、`withoutProject`、
  打印报告 DOM 位置、`installMode` 等 9 条）沉淀成长期约定。
- README 的「文档索引」里加了 `docs/work-log.md`。
- 把这条要求同时写进了运行时记忆，后续轮次不必再提醒。

### 没做到

- **第 1～6 轮的记录是回溯整理，不是当轮实时写的**。回溯依据是仓库里的提交、
  报告和脚本输出，个别细节（例如某一轮具体花在哪、当时的中间思路）没有留痕，
  补不回来。从第 7 轮起改为**当轮结束即写**。
- 没有做自动化校验：目前靠约定约束，**没有**任何 CI 检查"这一轮是否补了工作记录"，
  漏写不会被机器发现。
- 这份记录里的"没做到"清单**没有被转成待办**（没有 issue、没有任务跟踪），
  下一轮要不要做仍取决于人的选择。

### 怎么验证的

- 提交前门禁本机实跑全绿：`pnpm typecheck` / `pnpm test`（**85 passed**）/
  `pnpm lint`（0 problems）/ `cargo fmt --check` /
  `cargo test --lib`（**310 passed**）/ `cargo clippy --all-targets -- -D warnings`。
- 隐私自查：`git ls-files` 无 `.key/.db/.log/.exe/.sig`、无 `dist/`、无 `src-tauri/target/`；
  `minisign encrypted secret key` 只在 `AGENTS.md` 与 `docs/remediation-report.md`
  里作为**自查模式文字**出现，没有私钥块。
- 推送后在 GitHub Actions 上核对最终提交：`CI` = **success**。

### 相关文档

- `AGENTS.md` —— 上述长期约定
- `docs/remediation-report.md`、`docs/test-report.md` —— 第 1～6 轮记录的原始出处

---

## 第 6 轮 · 2026-09-24 · 项目整改（0.3.0）

按《Lumen 项目整改任务书》做的一轮质量整改：不堆新功能，把"看起来实现了"提到
"数据语义明确、异常时不静默出错、升级不冒险丢数据"。

### 做了

**P1（全部完成）**
- **收件箱筛选语义**：`projectId=null` 过 IPC 会变成 Rust 的 `None`，与"不限制项目"同义，
  收件箱因此退化成"所有未完成任务"。改为显式的 `withoutProject`，三种语义分开。
- **任务时间与提醒原子化**：原来先 commit 再重算、重算失败只写日志就返回成功；
  现在同一事务内完成，失败整体回滚。`task_update` / `task_reschedule` / 重复任务"仅本次"编辑一并改造。
- **WAL 安全的迁移前备份**：原来 `copy lumen.db` 会漏掉还在 `-wal` 里的最新提交。
  改用 `VACUUM INTO` 一致性快照（不修改源库），失败降级为 checkpoint+复制并做完整性校验，
  再失败则**中止升级**；备份保留 5 份；无待执行迁移时不产生备份。
- **OpenAI Provider 拆分**：三条协议彻底分开，OpenAI 独立走 Responses API
  （`input` / `instructions` / `max_output_tokens` / `text.format`）。
  顺带修掉 IPv6 本机地址（`http://[::1]:11434`）被误判为"非本机"而拒绝的漏洞。

**P2**
- **提醒停用原因**：新增 `disabled_reason`（migration 0005），区分"用户主动关闭"与
  "系统因缺少依赖时间暂停"；后者在时间补回后自动恢复，前者任何自动逻辑都不打开。
- **报告全量导出**：原来固定 `limit=1000` 且不提示；现在按 500 条一页读到取完（上限 10 万并如实告知），
  并修掉分页排序并列时可能重复/漏项的隐患（ORDER BY 追加唯一键）。
- **持续集成**：新增 `.github/workflows/ci.yml`（前端 + Rust 两个 job），与 Release 分离。
- **输入校验**：`link_url` 与预计/实际耗时在创建与更新两条路径统一校验（此前更新路径完全没有）。
- **AI 错误脱敏**：剥 HTML、打码疑似密钥、截断，UI 与日志共用同一处理结果。
- **附件路径**：受控副本改存相对数据目录的路径，搬数据目录/换机器仍有效；老绝对路径继续可用。
- **四个专项回归组**：重复任务（含 DST、闰日、月末）、提醒（过滤/顺序/上限/超窗作废）、
  窗口托盘（默认安全、四种显隐组合、越界收敛、老配置兼容）、更新器（实机跨版本升级 + 私钥卫生）。

**顺带发现并修复的真实问题**
- **「每月最后一天」根本无法表达**：`BYMONTHDAY` 只接受正数，`-1` 直接解析报错。
  已支持 RFC 5545 负数写法，并在规则编辑器加了「最后一天」按钮。
- **`pnpm lint` 一直是坏的**：ESLint 10 需要扁平配置而仓库里没有。补上配置后立刻抓出两个真 bug——
  快速添加的依赖数组漏了 `periodType`（先选周期再回车会存成上一次的值）、更新检查重新抛错时丢失 `cause`。
- **全部 clippy 告警**：34 条，已清理，`cargo clippy -D warnings` 成为真门禁。

### 没做到

| 项 | 说明 |
| --- | --- |
| **OpenAI 官方文档核实** | 本机对 OpenAI 全部官方域名返回 HTTP 403，另派独立子代理复核同样拿不到。OpenAI 分支因此建立在微软官方（Azure OpenAI）文档之上，`gpt-6-astra` 是 **Azure** 的模型 ID。**默认模型刻意留空**，让用户点「拉取模型列表」从自己账号读 |
| **三家 AI 的真实调用** | 本机没有任何可用 API Key，协议实现只有官方文档核实 + 单元测试支撑，**未验证**真的能调通 |
| **§12 超大文件拆分（P3）** | 按任务书写明的要求未做；本轮新增代码已按职责分文件 |
| **1000+ 条任务的滚动性能** | 只为报告导造过 1201 条数据，列表滚动的性能曲线没有测量 |
| **多显示器断连找回 / 任务栏与托盘四种组合的逐一枚举** | 有单元测试与部分实机脚本，没有把四种组合全部手工走一遍 |
| **文字对比度数值测量** | 没有用工具测过 WCAG AA |
| **代码签名证书** | 安装包仍未签名，SmartScreen 会提示"未知发布者" |
| **本地模型部署指引** | 文档缺口，未写 |

### 怎么验证的

- 本机实跑：`pnpm install --frozen-lockfile` / `typecheck` / `test`（85）/ `build` / `lint`（0 problems），
  `cargo fmt --check` / `cargo test --lib`（**310 passed**）/ `cargo clippy -D warnings`。
- GitHub Actions：最终提交 `3a64b1e` 上 `CI` **frontend=success、rust=success**。
- 实机升级（用户真实数据）：0.2.3 → 0.3.0 走应用内升级完成；日志显示
  "检测到 1 个待执行迁移，已生成迁移前一致性备份"；升级后迁移记录 `[1,2,3,4,5]`、
  22 条任务一条未丢、`integrity_check = ok`。
- 针对性测试举例：WAL 备份做了**对照实验**（先证明"只复制 .db"确实读不到数据）；
  提醒原子性用**故障注入**（把 reminders 表改名迫使重算失败，断言任务时间保持原值）；
  v4→v5 就地升级测试断言老数据无损且有备份可退。

### 相关文档

- `docs/remediation-report.md` —— 整改报告（逐项状态、修改文件、新增测试、未解决问题、提交清单）
- `docs/test-report.md` —— 测试报告
- `docs/acceptance-checklist.md` —— 功能验收表
- `docs/api-research*.md` —— 三家 AI 服务商的官方文档核实记录

---

## 第 5 轮 · 2026-09-24 · 修复应用内升级不生效（0.2.3）

### 做了
- 用户反馈"点完升级还是旧版"。做了**对照实验**：在应用运行中分别用 `/P`（passive）
  与 `/S`（silent）跑同一个安装包 —— `/P` 会出现安装窗口且文件未替换，`/S` 正常替换。
  据此把 `plugins.updater.windows.installMode` 从 `passive` 改为 `quiet`（映射 `/S`）。
- 更新失败时的提示中文化：原来只有一句 `Could not fetch a valid release JSON from the remote`，
  现在会说明常见原因，并常驻一个「手动下载安装包」按钮作为确定可用的退路。

### 没做到
- 没有从根上解释"为什么 `/P` 在应用自触发时替换失败"（只做到"换成可用方案 + 对照实验证据"）。

### 怎么验证的
- `tools/verify_update_install.py`：0.2.2 → 0.2.3 走完整升级链路（检查 → 下载 → 签名校验 →
  静默安装 → 重启），并断言安装目录里的**文件版本**确实变了。

### 相关文档
- `docs/test-report.md` §3.14「应用内升级"文件没被替换"的定位与修复」

---

## 第 4 轮 · 2026-09-23 · 顶栏悬浮窗开关（0.2.2）

### 做了
- 用户反馈"悬浮窗在哪开启，我都找不到"。原来只有「设置 → 窗口」和托盘菜单两个入口，
  都不在主界面上。主界面顶栏新增一键开关，与设置页/托盘**双向同步**。

### 没做到
- 无。（这一轮范围很小）

### 怎么验证的
- `tools/verify_floating_toggle.py`：用 CDP 派发**真实鼠标事件**点击按钮，
  断言"开→窗口出现、关→窗口隐藏、aria-pressed 同步"。
- 曾经在这里发现过一个真实缺陷并修复：顶栏既是拖动区又放按钮，`mousedown` 先触发
  `startDragging()`，导致 `click` 永不发生——即用户报的"置顶/穿透/关闭点了没反应"。

### 相关文档
- `docs/screenshots/ui-floating-toggle.png`

---

## 第 3 轮 · 2026-09-23 · 统一图标集（0.2.1）

### 做了
- 用户反馈"小图标太丑"。原来侧边栏用的是 emoji + 文字符号混排（`☀ 📅 ⑦ ◲ ①`），
  粗细、大小、基线全对不齐，配色不受控（emoji 自带颜色）。
- 换成 **40 个自绘 SVG 图标**（`src/components/Icons.tsx`）：统一 24×24 视口、线宽 1.7、
  `currentColor`，深浅主题与选中态自动适配；周/月/季/年共用同一外框、只在框内区分。
- 顺手修掉侧边栏左上角还留着改名前的字母 **A**。
- 全项目 TSX 里的 emoji 已清零。

### 没做到
- Rust 侧与文档里的少量符号（如日志中的 `→`）没有统一——不影响界面。

### 怎么验证的
- `tools/screenshot.py` 通过 CDP 抓**真实渲染结果**（`docs/screenshots/ui-icons-*.png`），
  并断言侧边栏 17 个导航项全部是 SVG、文本里不再出现 emoji 区段字符。

---

## 第 2 轮 · 2026-09-23 · 第二批功能（0.2.0）

### 做了
- **自动更新**：`tauri-plugin-updater` + minisign 签名校验，设置页可检查/下载/安装；
  配套 `latests.json` 生成脚本与 GitHub Actions 工作流。
- **PDF 导出**：走应用自身 WebView2 的 `PrintToPdf`，中文由系统字体渲染；
  报告含统计概览、按逾期/进行中/已完成分组、逾期标红。
- **列表内手动拖拽排序**、**任务复制为副本**、**项目/分类/标签合并入口**。
- **悬浮窗增强**：置顶/穿透/隐藏按钮、不透明度滑块、右下角拖拽改大小、
  就地改名与就地新增、**打开与主窗口完全相同的完整编辑表单**。
- **提醒条可点击跳转任务**（桌面端通知没有点击回调，这是可用替代路径）。
- **跨窗口实时同步**（主窗口 / 悬浮窗 / 快速添加窗）。

### 没做到
- **应用内升级当时不生效**（0.2.1 → 0.2.2 应用退出重启了但文件没换）——
  当轮未定位，留到第 5 轮解决。
- 通知点击回调：桌面端需要应用具备打包身份，**平台限制**，用应用内提醒条替代。

### 怎么验证的
- 新增 `commands_e2e` 集成测试（复制/排序/合并的语义边界）。
- CDP 实机脚本：`verify_v020.py`（16/16）、`verify_floating.py`（18/18，用真实鼠标事件）、
  `verify_pdf.py`（导出 90 KB、中文可提取）、`verify_update.py`（真实更新端点）。

### 修掉的关键缺陷（都由测试或实机验收发现）
- **所有"编辑保存"失败**：`Separated::push` 本身会写分隔符，52 处生成 `title = , ?1` 这类非法 SQL。
- **全新数据库创建第一条任务失败**：`COALESCE(MAX(sort_order),0)+1` 求值成 INTEGER，按 f64 解码报错。
- **拖拽排序在触发重新编号后用过期坐标算位置**，会把任务插到列表最前。
- **悬浮窗顶栏按钮点了没反应**（`mousedown` 被 `startDragging` 抢走）。
- **导出的 PDF 是空白页**：打印报告被渲染在 `.app` 内部，跟着主界面一起被隐藏。

---

## 第 1 轮 · 2026-09-23 · 初始版本（0.1.0）

### 做了
按《AI 智能 Todo 桌面软件开发任务书》§1–§12 完成首个可安装版本：
本地优先 SQLite（WAL）· 任务 CRUD 与组合筛选 · 复杂重复规则引擎（系列/分段/例外/跳过）·
提醒调度与去重 · 托盘与全局快捷键 · 悬浮今日小窗与快速添加窗 · 可选接入大模型
（DeepSeek / OpenAI / Claude / 自定义，密钥存系统凭据管理器）· 备份/恢复/CSV/Markdown 导出 ·
安装包构建与 GitHub 仓库发布。

### 没做到（当时如实列在验收表附二，共 26 项）
账号与云同步（含服务端）· 第三方任务导入 · PDF 导出 · 自动更新 · 首次启动引导与样例数据 ·
"极简模式"窗口 · 提醒声音 · 全局主题色 · 自定义背景 · 任务复制 · 列表内拖拽排序 ·
项目/分类合并入口 · 个人目标关联 · 主子任务联动 · 定时自动备份 · 应用内清空数据 ·
通知点击跳转 · 真实 API Key 端到端 · 1000+ 条性能实测 · 代码签名 · 本地模型部署指引 等。

### 怎么验证的
243 项 Rust 测试 + 85 项前端测试；`tools/*.py` 端到端脚本（数据库结构、提醒调度、
窗口恢复、周期口径）；Win32 API 直接读窗口扩展样式位（不靠界面自述）；
静默安装 → 启动 → 卸载全流程实测。

### 相关文档
- `docs/acceptance-checklist.md`、`docs/test-report.md`、`docs/data-structure.md`、
  `docs/design-recurrence.md`、`docs/tech-selection.md`

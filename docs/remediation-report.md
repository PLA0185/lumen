# Lumen 项目整改报告

> 对应《Lumen 项目整改任务书》。基线：`main`；整改完成时间：2026-09-24。
> 仓库：https://github.com/PLA0185/lumen
>
> **阅读提示**：本报告只写实际做过并验证过的事。凡是没做的、没验到的，
> 都在「尚未解决的问题」里逐条写明，不写"测试通过"来充数。

---

## 0. 结论速览

| 编号 | 问题 | 优先级 | 状态 |
| --- | --- | --- | --- |
| §2 | 收件箱筛选语义丢失 | P1 | ✅ 已修复 + 测试 |
| §3 | OpenAI Provider 协议混用 | P1 | ⚠️ 已按可用证据整改，**OpenAI 官方文档在本机不可达**，默认模型刻意留空 |
| §4 | WAL 模式下迁移前备份不一致 | P1 | ✅ 已修复 + 对照实验测试 |
| §5 | 任务时间与提醒重算不一致 | P1 | ✅ 已修复（同事务）+ 回滚测试 |
| §6 | 提醒"自动暂停"与"用户关闭"混同 | P2 | ✅ 已修复（新增 migration 0005）+ 场景 A/B 测试 |
| §7 | 报告静默截断 1000 条 | P2 | ✅ 已修复（分页全量）+ 1201 条测试 |
| §8 | 缺少持续集成 | P2 | ✅ 已新增 `ci.yml`，与 Release 分离 |
| §9 | 输入校验不统一 | P2 | ✅ 已补齐链接与耗时字段（创建/更新两条路径） |
| §10 | AI 错误回显第三方正文 | P2 | ✅ 已脱敏（HTML、密钥、超长） |
| §11 | 备份恢复链路 | P2 | ✅ 附件改相对路径；API Key 不进备份已锁测试 |
| §12 | 超大文件拆分 | P3 | ❌ 本轮未做（任务书明确"不要求大重构"） |
| §13 | 错误处理与事务 | P2 | ⚠️ 核心路径已改；非核心路径保留 best-effort（见 §13 说明） |
| §14 | 重复任务专项回归 | P2 | ✅ 11 条回归测试；**顺带发现并修复「每月最后一天」无法表达** |
| §15 | 提醒专项回归 | P2 | ✅ 3 条回归测试（过滤/顺序/上限/超窗作废） |
| §16 | Window / Tray 专项回归 | P2 | ✅ 单元测试 + 实机脚本 |
| §17 | Updater 专项回归 | P2 | ✅ 实机脚本 + 私钥卫生检查 |

**测试规模**：Rust **309** 项、前端 **85** 项，全部通过。
**新增迁移**：1 个（`0005_reminder_disabled_reason.sql`，纯加列，向后兼容）。
**是否需要用户手动操作**：不需要。不删库、不重装、不改配置。

---

## 1. §2 收件箱筛选语义（P1）

### 问题
前端用 `projectId = null` 表达"没有项目的任务"。`null` 经 IPC 反序列化成 Rust 的
`None`，与"压根没传这个条件"完全等价，后端那句
`if let Some(pid) = query.project_id...` 直接把条件丢掉——**收件箱退化成"所有未完成任务"**。

### 修改
| 文件 | 改动 |
| --- | --- |
| `src-tauri/src/models.rs` | `TaskQuery` 新增显式字段 `without_project: bool` |
| `src-tauri/src/commands.rs` | `without_project` 优先 → `AND project_id IS NULL`；否则按指定项目过滤 |
| `src/lib/store.ts` | 收件箱改用 `q.withoutProject = true` |
| `src/lib/types.ts` | 前端类型同步 |

三种语义现在互不混淆：不传 = 不限；`withoutProject` = 只要没项目的；`projectId` = 指定项目。

### 新增测试
- `commands_e2e::inbox_shows_only_tasks_without_project` —— 按任务书 §2.4 的 A/B/C 三条任务验收，
  并额外断言"全部任务仍能看到 B""按项目筛选不受影响""两个条件同时传时行为确定"。
- `commands_e2e::none_project_id_alone_means_unlimited` —— 反向保护：不传条件时必须返回全部。

---

## 2. §3 AI Provider（P1）

### 先做核实（任务书 §3.3 强制要求）
联网核实结果写入 `_research/ai-providers-current.md`：

| | DeepSeek | Anthropic | OpenAI |
| --- | --- | --- | --- |
| 一手官方文档 | ✅ 全部拿到 | ✅ 全部拿到 | ❌ **全部域名 403（Cloudflare）** |
| Base URL | `https://api.deepseek.com`（**无 `/v1`**） | `https://api.anthropic.com` | 未能核实 |
| 鉴权 | `Authorization: Bearer` | `x-api-key` + `anthropic-version: 2023-06-01` | 未能核实 |
| 端点 | `/chat/completions` | `/v1/messages` | 未能核实 |
| 默认模型 | `deepseek-flash` | `claude-sonnet-5` | **留空** |
| 输出上限字段 | `max_tokens` | `max_tokens`（必填） | 未能核实 |
| JSON 输出 | `response_format.type` | `output_config.format` | 未能核实 |

OpenAI 的旁证来自**微软官方文档（Azure OpenAI）**：明确写
"**Recommended**: Send tool-calling requests to the Responses API"，
并列出 `gpt-6-astra`（Azure 目录，版本 2026-09-03）。

### 修改
| 文件 | 改动 |
| --- | --- |
| `src-tauri/src/ai.rs` | 三条协议彻底分开；新增 `build_openai_responses_body` / `parse_openai_responses_response`；`endpoint()` 按 provider 分支；新增 `Provider::uses_openai_responses()` |

- **OpenAI 独立走 Responses**（`/v1/responses`）：`input` / 顶层 `instructions` /
  `max_output_tokens` / `text.format`，响应从 `output[].content[].output_text` 取文本，
  截断看 `status == "incomplete"`。不再复用 DeepSeek/Custom 的 Chat Completions 代码。
- **DeepSeek 与 Custom 保持 Chat Completions**，并在代码注释里写明"为什么这里不是 Responses"。
- **OpenAI 默认模型留空**：官方文档不可达，唯一能找到的 `gpt-6-astra` 是 Azure 的 ID。
  任务书 §3.5 明确"不得填写未经验证的模型"，因此宁可留空，让用户点「拉取模型列表」
  从自己账号读真实 ID（`GET /v1/models`），或手动填写。
- 顺带修掉一个真实漏洞：`http://[::1]:11434/v1`（IPv6 本机地址）因按 `:` 切分主机名
  被误判为"非本机"而拒绝，与任务书 §3.6"http 只允许 localhost / 127.0.0.1 / ::1"不符。

### 新增测试（§3.7 清单逐项覆盖，共 20 条）
端点 URL ×4、四类请求头、三家的 JSON 输出配置、空响应、refusal、
401/402/403/429/500/503/529 分类、错误 Base URL、API Key 未配置、
截断状态（Responses 的 `incomplete` / DeepSeek 的 `finish_reason=length` /
Anthropic 的 `stop_reason=max_tokens`）、token usage（含 DeepSeek 缓存字段与
Responses 的 `input_tokens_details.cached_tokens`）、超时与输出上限收敛。

> ⚠️ **未验证**：三家都没有用真实 API Key 跑过端到端调用（本机没有可用密钥）。
> 协议字段全部来自官方文档核实与单元测试，但"真的能调通"没有证据。

---

## 3. §4 WAL 安全的迁移前备份（P1）

### 问题
库跑在 WAL 模式下，最新提交可能还在 `lumen.db-wal` 里。原实现
`std::fs::copy(&db_path, &backup)` 只复制主库文件，得到的备份**缺最近的操作**。
一旦迁移失败再恢复，用户就丢数据。

### 修改
| 文件 | 改动 |
| --- | --- |
| `src-tauri/src/db.rs` | 新增 `pending_migrations()` / `pre_migration_snapshot()` / `verify_backup()` / `prune_pre_migrate_backups()`；`Db::init` 改为"先判断有无待执行迁移，再决定是否备份" |
| `src-tauri/src/error.rs` | 新增 `DbError::PreMigrationBackup` 与对应的可读提示 |

启动流程：
1. 用**临时连接**读 `_sqlx_migrations`，列出待执行迁移（不做任何写操作、不跑迁移）；
2. **没有待执行迁移 → 不备份**（避免磁盘满/文件被占用这类与迁移无关的原因挡住启动）；
3. 有待执行迁移 → 生成一致性快照：优先 `VACUUM INTO`（**不修改源库**，
   SQLite 官方推荐的备份方式），失败则降级为 `wal_checkpoint(TRUNCATE)` + 复制，
   并跑一次 `PRAGMA integrity_check`；
4. 快照失败 → **中止升级**（任务书 §4.5 的推荐策略），错误提示说明"本次不会改结构"；
5. 清理旧备份，只保留最近 5 份；**清理失败只记日志**，不影响启动。

### 新增测试
- `db::tests::pre_migration_snapshot_includes_uncheckpointed_wal_data` —— **对照实验**：
  先证明"只复制 .db"确实读不到 WAL 里的数据（对照组失败），再证明新快照能读到，
  并校验快照 `integrity_check = ok`、源库未被破坏。
- `db::tests::prune_keeps_recent_pre_migrate_backups_only` —— 保留策略、目录不存在时不报错。
- `db::tests::no_pending_migrations_after_init` —— 无待执行迁移时**不产生任何备份文件**。

---

## 4. §5 任务时间与提醒的原子一致性（P1）

### 问题
`task_update` 先 `commit`，再调用提醒重算，且重算失败只 `log::warn!` 就返回成功。
结果是库里可能出现"任务截止时间 = 新值、提醒时刻 = 旧值"，而界面显示保存成功。

### 修改
| 文件 | 改动 |
| --- | --- |
| `src-tauri/src/reminders.rs` | 新增 `recompute_task_reminders_tx(&mut Transaction, task_id)`；原 `recompute_task_reminders` 改为它的薄封装；`on_task_time_changed` **返回 `AppResult` 而不是吞掉错误** |
| `src-tauri/src/commands.rs` | `task_update` 在**同一个事务里**完成 UPDATE + 重算 + commit；`task_reschedule` 同样改造 |
| `src-tauri/src/recurrence_service.rs` | "仅本次"编辑的时间变更改为传播错误 |

### 新增测试
- `commands_e2e::changing_due_time_moves_relative_reminder_atomically` ——
  截止 18:00 + 提前 30 分 → `remind_at=17:30`；改到次日 20:00 → `19:30`；
  清空截止 → 提醒自动暂停并记录原因。
- `commands_e2e::failed_reminder_recompute_rolls_back_task_update` —— **故障注入**：
  把 `reminders` 表临时改名让重算必然失败，断言"更新整体失败"且
  **任务截止时间保持原值**（没有半新半旧）。

---

## 5. §6 提醒的"自动暂停"与"用户关闭"（P2）

### 修改
| 文件 | 改动 |
| --- | --- |
| `migrations/0005_reminder_disabled_reason.sql` | 新增列 `disabled_reason`，取值 `NULL` / `user` / `missing_base_time`；老数据里 `is_enabled = 0` 的一律标记为 `user`（保守，不擅自打开用户关掉的提醒） |
| `src-tauri/src/reminders.rs` | 重算时：依赖时间恢复 → **只恢复系统暂停的**；用户关闭的只更新时间、不打开。`reminder_set_enabled` 同步写原因 |

### 新增测试
- `scenario_a_auto_pause_then_auto_resume` —— 清空 due → 自动暂停并记 `missing_base_time`；
  恢复 due → 自动重新启用且时刻跟着更新。
- `scenario_b_user_disabled_stays_disabled` —— 用户关闭后改时间/清空再填回，
  **都不得自动打开**，且原因不会被系统原因覆盖。
- `custom_reminders_are_not_touched_by_recompute` —— 自定义提醒不受任务时间影响。
- `migration_marks_legacy_disabled_as_user_disabled` —— 老数据的兼容行为。

---

## 6. §7 报告全量导出（P2）

### 修改
| 文件 | 改动 |
| --- | --- |
| `src-tauri/src/commands.rs` | 新增 `task_report_all` / `report_all_impl`：**每页 500 条循环读取直到取完**；每页各自解析归属名称，避免超长 `IN (...)`；总量安全上限 10 万并在结果里用 `truncated` 如实标记；`ORDER BY` 统一追加 `id` 作为唯一 tie-breaker |
| `src/lib/ipc.ts` / `store.ts` / `App.tsx` | 改用全量接口；导出全程由 `exporting` 锁住按钮；完成提示显示**真实条数**，被上限截断时明确说明 |

**额外发现**：原来的排序在并列时（同一个 `sort_order`、同一个 `created_at`）
**顺序不稳定**，分页会重复或漏项。已通过追加 `id ASC` 得到确定全序。

### 新增测试
- `report_export_returns_all_rows_beyond_page_limit` —— 造 **1201** 条，
  先确认单页确实只有 1000 条，再断言全量导出拿到 1201 条且无重复。
- `report_pagination_is_stable_with_equal_sort_orders` —— 1200 条 `sort_order` 全相同，
  分页仍不得重复或漏项。

---

## 7. §8 持续集成（P2）

| 文件 | 说明 |
| --- | --- |
| `.github/workflows/ci.yml` | 新增。push/PR 到 main 与手动触发；`frontend` 与 `rust` 两个 job，都跑 Windows（与实际发布环境一致） |
| `.github/workflows/release.yml` | 保持"打包/签名/发布"职责；无签名密钥时**优雅跳过并留 notice**，不再产生失败通知 |
| `eslint.config.mjs` | **新增**——此前 `pnpm lint` 一直是坏的（ESLint 10 需要扁平配置，仓库里没有） |
| 全仓库 rustfmt | 跑了一次 `cargo fmt`，使 `cargo fmt --check` 能作为门禁 |

前端 job：`pnpm install --frozen-lockfile` → `typecheck` → `test` → `build` → `lint`。
Rust job：`cargo fmt --check` → `cargo check --all-targets` → `cargo test --lib` → `cargo clippy -- -D warnings`。

**CI 与 Release 分离**：Release 缺签名密钥时不再意味着"连测试都不跑"。

修 lint 时发现两个真实问题：
1. `QuickAdd` 的 `save` 回调依赖数组漏了 `periodType`——**先选周期再回车会存成上一次的值**；
2. 更新检查里重新抛错时丢失了原始 `cause`，排查困难。两处都已修。

> CI 的 clippy 门禁第一次跑是**红的**（34 条历史告警）。已全部清理，
> 现在 `cargo clippy --all-targets --all-features -- -D warnings` 本地与 CI 均通过。

---

## 8. §9 输入校验（P2）

| 字段 | 整改前 | 现在 |
| --- | --- | --- |
| `link_url` | 创建/更新都**直接落库**（可写 `javascript:`、超长串、控制字符） | 新增 `validate_link_url`：只接受 http(s)、限长 2000、拒绝控制字符 |
| `estimated_minutes` | 只在创建时校验 | 创建/更新都走 `validate_minutes` |
| `actual_minutes` | **完全没有校验** | 同上 |
| title / description / note / priority | 已有 | 保持 |
| recurrence interval / count、reminder offset、窗口不透明度与尺寸、timeoutSeconds、maxOutputTokens | 已有 | 补充了 timeouts/maxTokens 的边界测试 |

### 新增测试
- `create_rejects_dangerous_links_and_out_of_range_minutes`
- `update_rejects_invalid_link_and_minutes` —— 并断言失败后**数据保持原样**

---

## 9. §10 AI 错误信息安全（P2）

新增 `sanitize_provider_error()`：剥 HTML 标签 → 打码疑似密钥
（`sk-` / `sk-ant-` / `Bearer` / `api-key:` / ≥32 位裸 token）→ 折叠空白 → 截断。
UI 与日志共用同一份处理结果，避免"界面干净、日志里躺着密钥"。

新增测试 `provider_error_bodies_are_sanitized`（7 种形态：回显 Bearer、
裸 key、长 token、HTML 错误页、超长正文、正常短错误、多余空白）。

---

## 10. §11 备份恢复链路（P2）

### 附件路径
`stored_path` 原本存**绝对路径**，换机器/搬数据目录就失效。现在写入时存
**相对数据目录**（`attachments/<id>.ext`），读取时用 `resolve_stored_path()`
拼回；老数据里的绝对路径**原样使用**，升级不影响已有附件。
`attachment_reveal` / `attachment_remove` / `attachment_check` 三处消费点全部改为先解析。

### API Key
新增测试 `ai_config_is_backed_up_but_never_contains_the_key`：
锁定"provider 配置随备份走、备份里不得出现任何密钥字面量、恢复后配置完整可用"。

### 新增测试
`stored_path_is_relative_and_resolves_after_moving_data_dir`、
`absolute_legacy_stored_path_still_resolves`、`stored_is_relative_detects_windows_paths`。

---

## 11. §12 超大文件拆分（P3）

**本轮未做**。任务书明确"不要求大重构，后续新增功能时不应继续无限堆代码"。
本轮新增的代码已经按此原则放置：报告相关逻辑独立成 `report_all_impl` +
`build_report_rows`，PDF 在独立的 `pdf.rs`，更新在独立的 `update-ipc.ts` / `UpdatePanel.tsx`。

---

## 12. §13 错误处理与事务（P2）

已按要求改造的核心路径：
- **任务数据写入**：`task_update` / `task_reschedule` 全程事务，重算失败即整体回滚（§5）；
- **reminder 与任务关联更新**：不再 swallow 错误；
- **recurrence 规则更新**："仅本次"编辑的时间变更会传播错误；
- **migration**：备份失败即中止（§4）；
- **backup / restore**：失败向上返回（原本就是）。

仍保留 best-effort 的地方（任务书 §13.1 允许）：窗口状态恢复、托盘刷新、
日志清理、非关键 UI 同步事件。

---

## 13. §14 重复任务专项回归（P2）

新增 `recurrence::regression_task_book_14`（11 条），按任务书清单逐条对应：
每天 / 每 2 天 / 每周一 / 周一三五 / 每 2 周 / 每月 1 日 / **每月 31 日在短月跳过** /
**每月最后一天** / 每年固定日期 / **闰日只在闰年** / DST（`America/New_York`，
断言本地墙上时刻恒为 09:00，而 UTC 偏移从 -5 变 -4）。

修改范围（仅本次 / 此次及以后 / 整个系列）与历史保护由既有的
`recurrence_e2e`（5 条）覆盖。

> **回归测试直接抓出一个真实缺口**：`BYMONTHDAY` 只接受正数，
> `BYMONTHDAY=-1`（每月最后一天）会**解析报错**——也就是说"每月最后一天"
> 这条最常用的规则**根本无法表达**。现已支持 RFC 5545 的负数写法
> （按当月实际天数解析），规则描述读作"每月最后一天"，
> 并在规则编辑器里加了对应按钮（否则就是"后端有能力、界面没入口"）。

---

## 14. §15 提醒专项回归（P2）

把调度循环的过滤条件抽成 `due_reminders()` 以便直接测试，新增 3 条：
- `only_unfired_enabled_and_not_done_tasks_are_due` —— 7 种任务状态组合，
  只有"到点 + 启用 + 未触发 + 任务未完成未删除"的那条会被发出
  （覆盖"已完成不提醒""回收站不提醒""已触发不重复"）；
- `due_list_is_ordered_and_capped` —— 按时间顺序、单次有上限；
- `missed_reminders_beyond_grace_are_marked_expired` —— 超窗标记为 `expired`
  且不再发出，窗口内的未被误伤。

提醒时刻推导（到期时 / 到期前 N 分 / 计划时 / 计划前 N 分 / 自定义）由原有 12 条覆盖。

---

## 15. §16 Window / Tray 专项回归（P2）

新增 4 条单元测试 + 沿用原有 8 条：
- `defaults_never_lock_the_user_out` —— 默认必须能看见、能找回；
- `no_recovery_path_when_both_entries_disabled` —— 任务栏 × 穿透四种组合下，
  "托盘 + 快捷键都关"一律判定为无恢复路径；
- `size_and_opacity_are_always_within_bounds` —— 0/负数/NaN/无穷都收敛；
- `old_config_json_without_new_fields_keeps_user_settings` —— 新增配置项时
  老配置**不会整体回落默认值**。

实机验证（已有脚本）：`tools/verify_window.py`（Win32 读窗口扩展样式位）、
`tools/verify_floating.py`（真实鼠标事件点按钮、拖不透明度、改尺寸并检查溢出）。

---

## 16. §17 Updater 专项回归（P2）

- `tools/verify_update.py` —— 对着**真实 GitHub 端点**检查更新；
- `tools/verify_update_install.py` —— **完整跨版本升级**：0.2.2 → 0.2.3
  在实机上走完"检查 → 下载 → 签名校验 → 静默安装 → 重启"，
  并断言安装目录里的文件版本确实变了；
- `tools/serve_update.py` —— 本地假清单，用来验证"签名校验会拒绝伪造包"。

**私钥卫生**（本轮复查）：
- 仓库文件里搜不到私钥内容（搜索 `minisign encrypted secret key`）；
- git 全历史没有提交过 `*.key`；
- `.gitignore` 覆盖 `*.key` / `*.keystore` / `*.db*` / `target/` / `dist/`；
- 私钥存放于 `%USERPROFILE%\.tauri\lumen-updater.key`（仓库之外）。

---

## 17. 数据库迁移与用户数据影响

**唯一新增迁移**：`0005_reminder_disabled_reason.sql`

```sql
ALTER TABLE reminders ADD COLUMN disabled_reason TEXT
  CHECK (disabled_reason IS NULL OR disabled_reason IN ('user', 'missing_base_time'));
UPDATE reminders SET disabled_reason = 'user' WHERE is_enabled = 0;
```

- 纯加列，不改任何既有列的类型或语义；
- 老数据里"已停用"的提醒一律保守地视为**用户主动关闭**，
  绝不会因为升级而被自动打开；
- 旧版本程序读到多出来的列也没有影响（SQLite 允许）。

**对旧用户数据的影响**：无破坏性影响。任务、重复系列、提醒、附件、项目、
分类、标签、设置、AI 配置全部保持兼容；不需要删库、清空配置或重装。

**唯一需要用户知晓的行为变化**：新写入的附件 `stored_path` 改为相对路径
（老记录仍是绝对路径，两种都能读）。

---

## 18. 测试结果（本机实跑，命令与输出一致）

| 命令 | 结果 |
| --- | --- |
| `pnpm install --frozen-lockfile` | ✅ 锁定文件一致 |
| `pnpm typecheck` | ✅ 0 错误 |
| `pnpm test` | ✅ 85 passed |
| `pnpm build` | ✅ 构建成功 |
| `pnpm lint` | ✅ 0 problems（此前该命令本身是坏的） |
| `cargo fmt --check` | ✅ 无差异 |
| `cargo test --lib` | ✅ **309 passed; 0 failed** |
| `cargo clippy --all-targets --all-features -- -D warnings` | ✅ 通过 |

GitHub Actions：`CI` 工作流在 push 到 main 时真实执行上述检查
（`frontend` 与 `rust` 两个 job）。

---

## 19. 尚未解决的问题

### 19.1 未完成
1. **§12 超大文件拆分（P3）**：本轮按任务书要求未做。
2. **真实 API Key 端到端调用**：三家都没有可用密钥，协议实现只有官方文档
   核实 + 单元测试支撑，**没有真实调用证据**。
3. **OpenAI 官方一手文档不可达**：本机对 OpenAI 全部官方域名返回 HTTP 403，
   已另派独立子代理复核，同样拿不到。因此 OpenAI 分支建立在
   Microsoft Learn（Azure OpenAI）这份**官方但非 OpenAI 本体**的文档之上，
   且默认模型刻意留空。若你手上有 OpenAI 账号，用「拉取模型列表」验证一次即可确认。

### 19.2 已知限制（非本轮引入）
4. 安装包未做代码签名，SmartScreen 会提示"未知发布者"。
5. 桌面端系统通知没有点击回调，已用应用内可点击提醒条替代。

### 19.3 验证缺口
6. 多显示器断连后的窗口找回、任务栏/托盘四种显隐组合的逐一实机枚举：
   有单元测试与部分实机脚本，但没有把四种组合全部手工走一遍。
7. 1000+ 条任务的列表/搜索性能实测：本轮为报告导出造了 1201 条数据，
   但列表滚动的性能曲线没有测量。

---

## 20. 提交记录（按整改类别分开）

```
fix(inbox): 用显式的 withoutProject 保留「无项目」筛选语义
fix(db): 迁移前备份改为 WAL 安全的一致性快照
fix(reminders): 任务时间与相对提醒同事务更新，并区分自动暂停与用户关闭
fix(ai): 按官方文档拆分三家协议，OpenAI 独立走 Responses API，错误正文脱敏
fix(report): 报告改为分页全量导出，不再静默截断 1000 条
ci: 新增持续集成工作流（前端 + Rust），并让 lint/fmt 真正可运行
fix(validation): 链接与耗时字段在后端统一校验（创建与更新两条路径）
fix(backup): 附件改用相对数据目录的路径，并锁定 API Key 不进备份
test(recurrence): 补齐任务书 §14 专项回归，并支持「每月最后一天」
test(reminders): 补 §15 专项回归：过滤条件、顺序与上限、超窗作废
test(window): 补 §16 专项回归：默认安全、四种显隐组合、越界值收敛、老配置兼容
style: 清理 clippy 全量告警，CI 的 clippy 门禁改为真正生效
docs: 新增整改报告
```

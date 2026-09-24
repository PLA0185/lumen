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

---
---

# 第二轮复审整改

> 依据：《Lumen 第二轮整改任务书》
> 复审基线提交：`1bbcc4623fe64137e0b721d863ef87d622356d91`
> 本章是**追加**的，不修改上面第一轮的任何内容。

## 0. 结论速览（第二轮）

| 任务书条目 | 优先级 | 状态 |
| --- | --- | --- |
| §2 Provider 默认值前后端不一致 | P1 | ✅ 完成（默认值收敛到后端唯一来源） |
| §3 OpenAI"空模型 → 不能存 Key"死锁 | P1 | ✅ 完成（保存校验与运行校验拆分） |
| §4 普通列表硬截 500 条 | P1 | ✅ 完成（后端分页 + 前端加载更多 + 总数） |
| §5 回收站确认数量与实际不一致 | P1 | ✅ 完成（用后端 count，实机验证 1218 条） |
| §6 永久删除留下 copied 附件孤儿 | P2 | ✅ 完成（含孤儿扫描清理） |
| §7 恰好 100000 条误报 truncated | P2 | ✅ 完成（多读一条判定），另修掉一个 35 倍的性能问题 |
| §8 CI 与文档的 clippy 门禁不一致 | P2 | ✅ 完成（采用方案 A，无豁免） |
| §9 updater 密码与私钥并排明文存放 | P2 | ✅ 完成（DPAPI，明文文件已删除） |
| §10 任务总数接口 | P2 | ✅ 完成（`task_count`，与 list 共用筛选） |
| §11 2000 条数据量专项回归 | P2 | ✅ 完成（Rust 2000 条 + 实机 1200 条） |
| §12 列表虚拟化 | P3 | ❌ 未做（任务书允许先做"分页 + Load More"） |
| §13 大文件拆分等 | 不强制 | ❌ 未做，见「尚未解决」 |
| §14 必须新增/更新测试 | 强制 | ✅ 完成（Rust +13、前端 +16、实机 15 项） |

## 1. §2 Provider 默认值只有一个来源（P1）

### 问题

后端 `Provider::default_model()` 里 OpenAI 已经刻意留空（未经验证不填），
而前端 `src/lib/ai-ipc.ts` 同时维护着 `PROVIDER_DEFAULT_MODEL`，
OpenAI 那格写的是 `gpt-6-astra` —— 一个 **Azure** 的模型 ID。

`AiPanel.tsx` 切换 Provider 时直接读前端常量，于是用户切到 OpenAI 会被自动填入它，
**而保存不会报错**。漂移是静默的，这正是它危险的地方。

### 修改

- 新增后端命令 **`ai_provider_defaults`**（`src-tauri/src/ai.rs`），
  返回 `ProviderDefaults`：`provider / label / baseUrl / model / timeoutSeconds /
  maxOutputTokens / dataPolicyNote / modelMustBeChosen / keyEntry`，
  由 `ProviderConfig::with_defaults()` + `Provider::label()` + `data_policy_note()`
  派生，**结构上不可能与后端行为不一致**。
- 前端删除 `PROVIDER_DEFAULT_MODEL`、`PROVIDER_DEFAULT_BASE`、`PROVIDER_LABELS`
  与 `providerPolicyNote()`（四份重复来源），只保留纯映射函数
  `findDefaults` / `configFromDefaults` / `configForProvider`。
- `AiPanel.tsx` 在挂载时同时取默认值表与已保存配置；切换 Provider 用
  `configForProvider()`，取不到就提示更新而不是猜一个默认值。

### 防漂移护栏

新增测试 `frontend_has_no_duplicate_provider_defaults`：递归扫描 `src/**/*.ts(x)`
（跳过 `*.test.*`），出现 `PROVIDER_DEFAULT_MODEL` / `PROVIDER_DEFAULT_BASE` /
`gpt-6-astra` 任一即失败。

> 这条护栏在本轮**当场抓到过一次**：它先拦下了写在注释里的同一个词。

## 2. §3 空模型死锁（P1）

### 问题

一个 `validate()` 同时承担两件事：

- 保存配置时要它 —— 于是模型为空就保存不了；
- 而"获取模型列表"又要求 `hasApiKey === true`。

结果：模型为空 → 不能存 Key → 拿不到模型列表 → 选不了模型。

### 修改

按任务书 §3.3 / §3.4 拆成三个方法，职责互不重叠：

| 方法 | 用在哪 | 校验内容 |
| --- | --- | --- |
| `validate_endpoint()` | 所有场景的公共部分 | Base URL 非空、http(s)、非本机禁明文 http |
| `validate_for_save()` | `ai_set_config` | = 端点校验，**允许 model 为空** |
| `validate_for_run()` | `chat()`（真正发请求） | = 端点校验 + **model 必须已选** |

`ai_list_models` 改为只校验端点（它的存在意义就是帮用户在"还没有模型"时把模型选出来）。
界面同步：模型为空时给出说明；「测试连接」在没选模型时禁用并说明原因。

### 对照 §3.5 的三个场景

| 场景 | 结果 |
| --- | --- |
| A：OpenAI + model 为空 + 有效 Key | ✅ 可保存（`empty_model_is_savable_but_not_runnable`） |
| B：Key 已存、model 为空时拉模型列表 | ✅ 可用（`ai_list_models` 不再要求模型） |
| C：真正调用时空模型 | ✅ 拒绝，提示"请先在设置中获取模型列表或手动填写模型 ID" |

## 3. §4 + §10 列表分页与总数（P1）

### 问题

`buildQuery()` 写死 `limit: 500`，而主列表没有分页、加载更多或无限滚动。
501 条之后的任务**数据库里有、界面上永远看不到**——这是数据可见性问题，不是性能问题。

### 修改

**后端**（`src-tauri/src/commands.rs`）：

- 把筛选条件抽成 `apply_task_filters(b, query)`；
- 新增 `task_count(query) -> { total }`，**与 `task_list` 共用同一份 where 子句**；
- 抽出 `run_task_query(db, query, limit, offset)` 与 `push_task_order(b, query)`，
  让"列表分页"与"报告全量"走同一份筛选与排序。

**前端**（`src/lib/store.ts`）：

- `PAGE_SIZE = 200`；
- 新增状态 `totalCount / hasMore / loadingMore / nextOffset`；
- `reload()` 同时取列表与总数（`Promise.all`），`offset` 恒为 0；
- `loadMore()` 按 `nextOffset` 追加、**按 id 去重**、以总数判断是否还有更多；
  若这一页一条新的都没拿到（并发删除等）就停下，避免无限请求同一页。

**界面**（`src/App.tsx` / `src/components/BoardView.tsx`）：

- 底部常驻「已显示 X / 共 Y 条」+「加载更多（还有 N 条）」；
- 滚动到底自动加载（IntersectionObserver，`rootMargin: 400px`）；
- 看板视图同样分页并如实显示总数。

### 实机验收发现并修掉的一个问题

**搜索词只在按回车时才刷新**：`setSearch` 只改状态、不重新查询，
输入过程中列表与 `totalCount` 都停留在旧条件上——分页之后这会直接导致
"搜索框里写着 A、计数还是全量"。现改为输入即刷新（250ms 防抖），
条件一变必然 `offset = 0` 且结果整体替换（任务书 §4.5）。

## 4. §5 回收站确认数量（P1）

### 问题

```tsx
确定永久删除回收站中的全部 ${tasks.length} 项任务吗？
```

`tasks.length` 只是**已加载**的条数；后端 `DELETE ... WHERE deleted_at IS NOT NULL`
删的是全部。分页后这个差距会变得很大。

### 修改

- 按钮与弹窗文案改用后端 `task_count`（与列表同一套条件）；
- 数量大于已加载条数时，弹窗**额外说明**范围：
  "当前界面只加载了前 200 条，删除范围为回收站中的全部 1218 条"；
- 成功提示使用后端返回的 `purged`（而不是本地猜测）。

### 实机证据

| 观察项 | 实测值 |
| --- | --- |
| 后端 `task_count(deletedOnly)` | 1218 |
| 界面已加载卡片数 | 200 |
| 按钮文案 | 「永久删除 1218 项」 |
| 弹窗文案 | 「将永久删除回收站中的 1218 项任务。此操作不可撤销。…」 |

## 5. §6 copied 附件孤儿文件（P2）

### 问题

`attachments.task_id` 是 `ON DELETE CASCADE`，记录会随任务消失，
但 copied 模式的**实体文件**留在 `<数据目录>/attachments/` 里，
于是产生"数据库无记录、磁盘上还占着"的孤儿。

### 修改（`src-tauri/src/attachments.rs` + `commands.rs`）

顺序严格按任务书 §6.4：

```
事务前：SELECT stored_path ... WHERE task_id = ? AND storage_mode = 'copied'
   ↓
删除任务并提交（附件记录随 CASCADE 消失）
   ↓
提交后再删文件；失败只记日志，**不回滚**已完成的删除
```

- `copied_paths_of_task()` / `copied_paths_in_trash()` 取待清理路径；
- `delete_copied_files()` 执行删除，四重保护：
  1. **词法归一化**后必须仍在受控目录内（不依赖文件存在，挡住 `../` 与目录外绝对路径）；
  2. 文件不存在视为"已完成"（幂等）；
  3. 存在的文件再 `canonicalize` 校验（挡住符号链接）；
  4. 删除失败只计数，不影响调用方。
- 新增 `attachment_cleanup_orphans()` 命令：扫描受控目录，删除
  "命名是 Lumen 生成的 UUID 形式 + 数据库无引用"的文件；
  用户自己放进去的文件与子目录跳过并计数；**数据库读取失败时直接报错、不删任何东西**。
- 设置页「数据与备份」新增"扫描并清理孤儿副本"入口，并写明它只动什么。

## 6. §7 报告截断边界（P2）与随之暴露的性能问题

### 边界修正

判断依据从"行数达到上限"改成"**真的多读到了一条**"：

```rust
let mut tasks = run_task_query(db, &query, REPORT_MAX_ROWS + 1, 0).await?;
let truncated = tasks.len() as i64 > REPORT_MAX_ROWS;
if truncated { tasks.truncate(REPORT_MAX_ROWS as usize); }
```

| 数据量 | 期望 | 实测 |
| --- | --- | --- |
| 99999 | `truncated = false`，total 99999 | ✅ |
| 100000 | `truncated = false`，total 100000 | ✅（原实现误报 true） |
| 100001 | `truncated = true`，total 100000 | ✅ |

### 顺带修掉的性能问题

实现过程中实测发现：上一轮的"按 500 条一页读到取完"在 10 万条时是 **O(n²)**——
OFFSET 深分页每页都要重新排序整表再跳过前面的行。

| 阶段 | 耗时 |
| --- | --- |
| 插入 99999 条（递归 CTE） | 0.80 s |
| 导出 99999 条（**改前**） | **55.9 s** |
| 导出 99999 条（**改后**） | **1.58 s** |
| 导出 100000 条 | 2.75 s |
| 导出 100001 条（截断） | 3.24 s |

改法：报告本来就是"全都要"，不再分页，一次取 `REPORT_MAX_ROWS + 1` 条；
`build_report_rows` 的标签查询按 500 个 id 分块（一次拼 10 万个 id 会超过
SQLite 默认 999 个变量的上限）。

> **注意**：列表分页仍然是 OFFSET。这是取舍——列表一次只取 200 条、用户手工翻页，
> 深分页需要 keyset 分页才能根治，本轮没做（已记入"尚未解决"）。

## 7. §8 CI 与文档的 clippy 门禁一致（P2）

### 问题

| 位置 | 命令 |
| --- | --- |
| 文档（AGENTS.md / work-log / 本报告） | `cargo clippy --all-targets --all-features -- -D warnings` |
| CI 实际 | `cargo clippy --all-targets -- -D warnings -A clippy::too_many_arguments -A clippy::type_complexity` |

两者不是同一套规则，属于**文档失真**。

### 修改（采用任务书推荐的方案 A）

先实测：`cargo clippy --all-targets --all-features -- -D warnings` 在本仓库**全绿**，
说明那两条豁免早已不必要。于是 CI 改成与文档完全一致、且与本地相同的命令，
不再有任何豁免。`AGENTS.md`、`docs/work-log.md`、本报告的描述已统一。

### 改完之后 CI 还是红了一次——根因不在 lint

第一次推送后 CI 的 `rust` job 在 clippy 步骤失败，而本机同一条命令是绿的。
按"以 CI 为准"的约定把它当成本轮未完成来查，最终定位到：

```
error: proc macro panicked
   --> src\lib.rs:402:14
    = help: message: The `frontendDist` configuration is set to `"../dist"`
                    but this path doesn't exist
```

`--all-features` 会启用 tauri 的 **`custom-protocol`**，该 feature 下
`tauri::generate_context!()` 必须把前端产物嵌进二进制；而 CI 的 rust job
原先只编译 Rust、**不产出前端**。`cargo check --all-targets` 不带 `--all-features`，
不触发这条路径，于是只有 clippy 这一步红。

**处置**：

1. rust job 增加 `pnpm install --frozen-lockfile` + `pnpm build`；
2. `AGENTS.md` 的门禁把 `pnpm build` 标为"不可省的一步"，
   并在「项目特有的坑」表里新增该条；
3. clippy 步骤失败时把尾部输出写进 **job summary**——
   Actions 的原始日志下载需要认证，未登录时网页不渲染日志，
   没有这一步就只能靠猜。

## 8. §9 updater 密码存储（P2）

### 修改

新增 `tools/updater-secret.ps1`：

| 动作 | 作用 |
| --- | --- |
| `-Status` | 报告私钥/密码文件是否存在、能否解密（**不打印密码**），并警告残留的明文文件 |
| `-Set` | 交互式录入（不回显、二次确认），用 DPAPI 加密后写入 `.dpapi` |
| `-Build` | 读取私钥与密码 → 设置环境变量 → `pnpm tauri build`，结束后清理环境变量 |
| `-Clear` | 删除密码文件（不动私钥） |

加密方式是 `ConvertFrom-SecureString`（未指定 `-Key` 即 **DPAPI 当前用户范围**）：
换用户、换机器、拷走文件都无法解密。

**本机迁移已执行**：明文 `lumen-updater-password.txt` 已删除；删除前做了
DPAPI 往返校验（解密结果与原文逐字比对一致才删）。CI 侧不使用本脚本，
仍走 GitHub Actions Secrets。

### 如实说明边界

DPAPI 挡的是"文件被拷走 / 被别的账户读到 / 误提交进仓库"；
**挡不住**本机同一账户下的恶意进程。那种威胁模型需要硬件密钥，本轮没做。

## 9. §11 + §14 回归测试

### 新增 `src-tauri/src/remediation2_e2e.rs`（11 项）

| 测试 | 覆盖 |
| --- | --- |
| `pagination_reads_all_1200_rows_without_gaps_or_duplicates` | 1200 条逐页读全，不重不漏 |
| `pagination_order_is_stable_when_sort_keys_tie` | 排序键全并列时顺序稳定，页大小无关 |
| `count_matches_list_under_every_filter` | **16 种筛选条件下 count 与列表条数逐一相等** |
| `trash_count_drives_purge_confirmation_and_purge_result` | 回收站 1200 条：确认数 = count = `purged` |
| `purge_single_task_refuses_live_task` | 未进回收站的任务不能被永久删除 |
| `purge_task_removes_copied_copy_but_never_touches_original` | copied 删除 / reference 原文件保留 |
| `purge_all_removes_every_copied_file` | 清空回收站清理多个副本，未删除任务不受影响 |
| `orphan_cleanup_only_removes_managed_unreferenced_files` | 只删"受控目录 + UUID 命名 + 无引用" |
| `delete_copied_files_refuses_paths_outside_the_controlled_dir` | 三种越界写法一律拒绝 |
| `report_truncation_is_exact_at_the_export_limit` | 99999 / 100000 / 100001 三个边界 |
| `regression_2000_rows_across_every_view` | **2000 条**（500 未完成 + 500 已完成 + 500 回收站 + 500 带归属）跨 13 个视图 |

### 新增 `src-tauri/src/ai.rs` 测试

- `provider_defaults_have_a_single_source` —— 默认值表与 `with_defaults()` 逐字段一致；
- `frontend_has_no_duplicate_provider_defaults` —— 跨端防漂移护栏；
- `empty_model_is_savable_but_not_runnable` —— §3.5 场景 A 与 C。

### 新增前端测试

| 文件 | 项数 | 覆盖 |
| --- | --- | --- |
| `src/lib/ai-ipc.test.ts` | 7 | 默认值映射、切换不残留旧模型、表里没有就返回 null、政策文案取自后端 |
| `src/lib/store.pagination.test.ts` | 9 | 首屏一页、追加不重复、空页停下、条件变化 offset 归零、失败不破坏已加载内容、搜索自动刷新 |

### 实机验收 `tools/verify_remediation2.py`（15/15）

在真实实例上造 1200 条任务并逐项断言（含"用户原有数据未被改动"）：

```
✅ 首屏没有一次渲染全部（分页生效） —— DOM 里 200 张卡片，远小于 1200
✅ 界面明确写出「已显示 X / 共 Y」 —— '已显示 200 / 1200 条'
✅ 滚动到底部会自动加载下一页 —— 200 → 400 张卡片
✅ 点「加载更多」能继续往下取 —— 400 → 600 张卡片
✅ 重新加载后首屏顺序稳定
✅ 回收站按钮显示的数量等于后端真实总数 —— '永久删除 1218 项'
✅ 确认数量不是「已加载条数」 —— 已加载 200 条 vs 按钮 1218 项
✅ 二次确认弹窗里也是真实总数
✅ 附件孤儿清理命令可用且返回完整统计
✅ 用户原有数据未被改动 —— 全部 22→22，回收站 18→18
```

脚本在 `finally` 中逐个按 id 永久删除自己造的数据（刻意**不用**"清空回收站"，
那会连用户自己回收站里的任务一起删掉），并断言残留为 0。

## 10. 数据库与用户数据影响（第二轮）

**本轮没有新增任何数据库迁移**，schema 与第一轮结束时完全一致（`[1,2,3,4,5]`）。

- 旧库直接可用，不需要删库、清空配置或重装；
- 界面上唯一可见的行为变化：
  1. 列表**按页加载**（首屏 200 条 + 底部加载更多），不再是"最多 500 条"；
  2. 搜索输入即刷新（此前要按回车）；
  3. 回收站按钮文案从"清空回收站（N）"改为"永久删除 N 项"，N 是真实总数；
  4. 设置页新增「附件副本清理」入口。

## 11. 测试结果（第二轮，本机实跑）

| 命令 | 结果 |
| --- | --- |
| `pnpm install --frozen-lockfile` | ✅ |
| `pnpm typecheck` | ✅ 0 错误 |
| `pnpm test` | ✅ **101 passed**（第一轮 85，新增 16） |
| `pnpm build` | ✅ |
| `pnpm lint` | ✅ 0 problems |
| `cargo fmt --check` | ✅ 无差异 |
| `cargo test --lib` | ✅ **323 passed; 0 failed**（第一轮 310，新增 13） |
| `cargo clippy --all-targets --all-features -- -D warnings` | ✅ 通过（**无豁免**，与 CI 同一条命令） |
| GitHub Actions `CI`（提交 `862f423`） | ✅ `frontend` = success、`rust` = success |

> 上一轮本表里也写着"`--all-features` 通过"，但当时 CI 实际带着两条豁免。
> 本轮把 CI 改成同一命令后，这句话与 CI 行为**真正一致**了；
> 而为让 CI 真的能跑这条命令，rust job 还补上了 `pnpm build`（见 §7 末尾）。

## 12. 尚未解决的问题（第二轮）

### 12.1 未完成

1. **§12 列表虚拟化（P3）**：未做。任务书允许先做"分页 + Load More"，
   本轮走的这条。1200 条时首屏渲染 200 张卡片可用，但**没有测量**滚动帧率。
2. **大文件拆分**：`App.tsx` / `commands.rs` / `ai.rs` 仍未拆；
   `commands.rs` 本轮又长了约 200 行。
3. **DPAPI 密码轮换流程**：只做了"怎么存"，没验证"怎么换、换了之后旧版本还能不能升级"。

### 12.2 已知限制（本轮未修）

4. **列表分页仍是 OFFSET**：深分页是 O(n²)。1200 条无感，
   几万条会变慢；根治需要 keyset 分页。
5. **附件不随备份打包**：备份文件仍不含附件本体，搬机器要手工复制
   `attachments` 目录。
6. **前端没有组件级测试**：仓库无 jsdom / testing-library，
   按钮文案这类断言只能靠实机脚本。
7. 安装包仍未做代码签名（沿用第一轮结论）。

### 12.3 验证缺口

8. **AI 三家仍没有真实 API Key 的端到端调用证据**（沿用第一轮结论）。
   本轮修的是"配置能不能存下去、模型能不能拉出来"这条链路，
   它同样只到单元测试与实机界面为止。
9. **10 万条时的界面表现**：性能数字全部来自 Rust 侧集成测试；
   前端渲染 10 万张卡片的情况没测（分页就是为规避它而存在的）。

## 13. 提交记录（第二轮，按整改类别分开）
```
fix(ai): Provider 默认值收敛到后端唯一来源，并补齐跨端防漂移护栏
fix(ai): 拆分保存校验与运行校验，解除「空模型无法保存密钥」死锁
fix(tasks): 列表改为分页加载并提供真实总数，不再硬截 500 条
fix(search): 输入即刷新并重置分页偏移
fix(trash): 清空回收站的确认数量改用后端真实总数
fix(attachments): 永久删除后清理 copied 副本，并新增孤儿扫描清理
fix(report): 修正 100000 条时的截断误报，并把全量导出从 56s 降到 1.6s
ci: clippy 门禁去掉豁免，与文档和本地命令完全一致
security(updater): 签名密码改用 DPAPI 存储，删除明文文件
test(remediation2): 新增分页/回收站/附件/报告边界/2000 条数据量回归
docs(work-log): 记录第二轮整改结果
```

---
---

# 第三轮整改

> 依据：《Lumen 第三轮整改任务书》
> 复审基线：`8aa8d8b`（第二轮整改完成后的 HEAD）
> 本章是**追加**的修正，不修改上面任何历史结论。

## 0. 先承认：第二轮有三个"完成"结论不充分

任务书 §11 明确要求把这件事写清楚，而不是悄悄改掉上一轮的自评：

| 第二轮的自评 | 第三轮复审的结论 | 当时为什么会误判完成 |
| --- | --- | --- |
| §5 回收站确认数量 ✅ 完成 | **不充分**：确认数量确实换成了后端 `task_count`，但那个 count 是**带筛选条件**的；后端执行的仍是无条件的 `DELETE ... WHERE deleted_at IS NOT NULL`。回收站里搜索后点清空：界面说删 30 项，实际删掉整个回收站 | 验收只覆盖了"回收站**无筛选**"这一种状态。分页改动引入了"计数带条件"，但没把它和"删除范围"联系起来——**只验证了数量来源，没验证范围一致性** |
| §6 copied 附件清理 ✅ 完成 | **不充分**：越界校验只考虑了"目录外穿越"与"链接指向目录外"，漏掉**受控目录内部**的 symlink 指向另一个 live 副本：`canonicalize` 跟随链接，于是删 B 时删掉了 A 的真实文件 | 危险场景发生在"目录**内**"，而测试全在盯"目录**外**"；本机创建 symlink 需要管理员权限，当时连一条相关用例都没有 |
| §14 新增测试 ✅ 完成 | **不充分**：数量确实增加了（Rust +13、前端 +16、实机 15 项），但**高风险边界**覆盖不全：并发一致性、symlink、旧请求污染新视图、大文本报表都缺 | 把"测试数量增长"当成了"风险覆盖充分"的判据。第三轮改为按**破坏性场景**逐条列测试要求 |

结论：第二轮的"完成"是在当时的测试视角下成立，一经第三方从攻击面看就站不住。
这条教训保留在这里，不删。

## 1. P0-① 回收站：确认集合 = 实际删除集合

**语义选择**：任务书 §1.2 给了 A（永远清空整个回收站）与 B（删除当前筛选结果）两个方案，
要求二选一并把 UI / count / DELETE / 附件清理全部统一。本轮选 **B**，理由：

- 方案 A 要在按钮上写"整个回收站 128 项"，而列表里只显示搜索命中的 3 条——
  用户看到的和按钮说的是两回事，更容易误操作；
- 方案 B 下四者天然是同一个集合，验证方式也直接（"看到什么就删什么"）。

| 位置 | 改动 |
| --- | --- |
| `task_purge_all_deleted` | 接收可选 `TaskQuery`；DELETE 走 `apply_task_filters`，与 `task_count` / `task_list` **完全同一套条件** |
| 后端保护 | **强制** `deleted_only = true`——无论调用方传什么，这条命令都碰不到未删除的任务 |
| 附件清理 | 新增 `copied_paths_of_matching_tasks`，用同一套条件取副本路径，避免"记录删了、文件按另一套条件清" |
| 并发一致性 | 命令新增 `expected_count`：同一事务里重新计数，与确认值不符则整体回滚并返回冲突提示 |
| 前端 | 传当前筛选条件 + 确认时显示的数量；弹窗同时写出"筛选结果 N 项 / 回收站共 M 项" |

**测试**：`purge_all_respects_filters_and_never_touches_live_tasks`、
`purge_all_refuses_when_the_set_changed_after_confirmation`（对应任务书验收用例 A/B/C）。

## 2. P0-② 附件删除绝不跟随符号链接

根本原则（任务书 §2.2）：删除**数据库记录指向的那个目录项本身**，不是它解析后的目标。

新增统一函数 `safe_remove_managed_copy(root, candidate)`，四处删除全部改走它
（任务删除、清空回收站、孤儿清理、单个附件删除）：

| 情况 | 处理 |
| --- | --- |
| 词法越界 / 受控目录本身 | 拒绝 |
| 不存在 | `Ok`（幂等） |
| 符号链接 / junction | 用 `symlink_metadata` 识别 → **只删链接自身**，绝不跟随；记 warn |
| 普通文件 | 校验**父目录**在受控目录内，删**原路径**（不是 canonicalized 结果） |
| 目录等其它类型 | 拒绝 |

顺带修掉一个同族问题：Windows 路径大小写不敏感，而 `Path::starts_with` 是精确比较，
"数据目录改过大小写"会让受控目录内的文件被误判成越界而删不掉。
新增 `path_lexically_inside` / `same_path` 做大小写不敏感比较。

**测试**：5 项（越界/UNC/幂等、大小写变体、junction→外部、junction→live 副本目录、
混合批量删除）。**junction 场景在本机真跑了**；文件级 symlink 需要管理员权限，
本机与 CI 都没有，已在 work-log 的"没做到"里如实登记。

## 3. P1 Provider Key 状态按 provider 独立

`ai_provider_key_status()` 逐 provider 从凭据管理器读真实状态；前端切换服务商时用
**目标 provider** 的状态，不再继承旧 provider 的 `hasApiKey`。
原来那条断言错误行为的测试已改为「不同 Provider 的密钥状态相互独立」。

## 4. P1 分页：旧请求不得污染新视图

store 新增 `queryGeneration`：视图 / 搜索 / 状态筛选 / 排序 / 逾期五个入口都会 +1；
`reload()` 与 `loadMore()` 在写回前比对代数，**对不上就丢弃**。
跨窗口数据变化走 `handleExternalChange()`：先作废在飞请求，再从第一页重取
（任务书 §5.4 允许的临时方案；keyset 分页未做）。

## 5. P1 看板空页停止

看板改为与主列表一致的 `hasMore`：空页立刻停下并刷新真实计数；
`hasMore` 为假而总数仍大于已加载时，渲染**真能用的刷新入口**而不是点了没反应的按钮。

## 6. P1 大报告导出改为流式

原路径把最多 10 万条完整 Task 经 IPC 塞进 React state，再渲染 10 万个 `<tr>`。
现改为 `src/lib/report-export.ts`：分页取数（每页 500）→ 逐行构造 DOM 挂到 body →
JS 不留行数组；进度显示"准备中 X/Y"；**行数之外再加总字符数上限**（2000 万字符）；
结束后 `dispose()`。旧的 `PrintReport.tsx` 已删除。

## 7. P2 updater 明文迁移默认安全

密文写入 → 解密往返校验 → 通过即**立即删除**明文；校验失败回滚密文并保留明文、非零退出；
删除失败同样非零退出并提示"安全迁移未完成"。`-Status` 见到 legacy 明文即输出"不安全"并返回非零。

## 8. P2 过时注释

`App.tsx` / `store.ts` 里"按 500 条一页读到取完""分页取全量"等描述与实现不符，
已按"后端一次取 `REPORT_MAX_ROWS + 1` 条"改写。

## 9. 测试结果（第三轮，本机实跑）

| 命令 | 结果 |
| --- | --- |
| `pnpm typecheck` | ✅ 0 错误 |
| `pnpm test` | ✅ **113 passed**（第二轮 101） |
| `pnpm build` / `pnpm lint` | ✅ / 0 problems |
| `cargo fmt --check` | ✅ |
| `cargo test --lib` | ✅ **331 passed; 0 failed**（第二轮 323） |
| `cargo clippy --all-targets --all-features -- -D warnings` | ✅ 通过（无豁免） |

## 10. 尚未解决（第三轮）

1. **keyset / cursor 分页未做**（任务书 §5.4 允许的临时方案已实施）；OFFSET 深分页的性能问题仍在。
2. **文件级 symlink 未实测**：本机与 CI 都没有创建权限，只用 junction 覆盖了同一段代码路径。
3. **报表压力测试的四组数据形态没跑**（100000 短文本 / 10000×20k 描述 / 10000×20k 备注 / 多标签）。
4. **验收脚本仍在真实数据库上跑**：事故暴露的隔离问题没有根治。
5. 列表虚拟化、大文件拆分、代码签名、本地模型指引：沿用前两轮结论，未做。

## 11. 提交记录（第三轮）

见 `docs/work-log.md` 第 9 轮的提交清单与 GitHub Actions 记录。

---
---

# 第三轮收口

> 依据：《Lumen 第三轮收口整改任务书》
> 复审基线：`dd950d5`（第三轮整改后的 HEAD）
> 本章同样是**追加**，不修改上面任何历史结论。

## 0. 先承认：第三轮有三处"闭环"其实还能被绕过

| 第三轮的自评 | 收口复审的结论 | 当时为什么会漏 |
| --- | --- | --- |
| §1 并发一致性 ✅ 完成 | **不成立**：只校验了"确认时数量 == 执行时数量"。**数量相等不代表集合相同**——A 被恢复、B 被移入且同样命中筛选时 count 不变，于是用户确认删 A、后端删掉 B | 把"数量"当成了"集合"的等价物。要防的是**身份**漂移，数量只是它的一个投影 |
| §2 附件清理 ✅ 完成 | **不成立**：越界与 symlink 都处理了，但**身份比较**没做大小写/分隔符归一化。legacy 绝对路径与当前 `data_dir` 大小写不同时 `strip_prefix` 匹配失败，**仍被引用的 live 副本被当孤儿删除** | 只想了"路径在不在受控目录内"，没想"两条路径说的是不是同一个文件"。Windows 上后者天然要处理大小写与分隔符 |
| §4 旧请求不污染 ✅ 完成 | **不完整**：代数检查都在 `await` **之前**，而 `fetchProgress` 本身是一次 `await`——检查通过后挂起、期间用户切条件、旧结果回来照样覆盖 | 检查点的位置错了。"过了检查"不等于"写回时仍然成立" |

教训统一成一句：**凡是"确认一个集合、之后按它执行"的动作，带下去的都必须是身份，而不是它的计数；
凡是跨越 `await` 的守卫，都必须在最后一个 `await` 之后重新确认一次。**

## 1. P0-① 精确 ID 快照（§2~§6）

两阶段：`task_prepare_purge_deleted(query)` → `{ taskIds, count }`；用户确认后
`task_commit_purge_deleted(query, taskIds)` **只删这批 ID**。

commit 内的顺序（§5 的核心要求）：

```text
BEGIN
  1. 重新取当前命中集合，与确认时的 taskIds 逐个比对 —— 不一致就回滚（零删除）
  2. 基于同一批 taskIds、在同一事务里查 copied 附件路径
  3. 只 DELETE 这批 taskIds
COMMIT
  4. 提交之后才删文件（失败只记日志，不回滚已提交的删除）
```

- **不用**服务端 `operationId → IDs` 会话状态：ID 交给调用方持有、commit 时原样传回，
  重启与窗口刷新都不会让快照失效。
- ID 分批（每批 500）读写以避开 SQLite 变量上限，但**全程在同一事务内**。
- 旧命令 `task_purge_all_deleted`（只校验数量）保留并标注**已弃用**，前端不再调用。

## 2. P0-② legacy 路径身份归一化（§14~§16）

新增 `normalize_managed_identity(data_dir, stored_or_abs)`：`resolve_stored_path` →
词法归一化 → Windows 下统一小写与分隔符。孤儿清理的**引用集合**与**扫描 key** 都走它。
它只用于"是不是同一个附件"的判断，**绝不参与 symlink 的删除目标决策**
（那条路径仍然只经过 `safe_remove_managed_copy`）。

## 3. P1 分页守卫的位置修正（§9~§11）

`reload` / `loadMore` 在 `fetchProgress` 之后**再查一次** `queryGeneration`。
测试覆盖"旧 reload 后返回不覆盖新结果""旧 loadMore 不追加进新条件"。

## 4. P1 看板跨窗口同步（§12~§13）

新增 `src/lib/board-paging.ts`：不依赖 DOM 的分页状态机（`createBoardPaging` +
纯函数 `nextBoardState`），组件用 `useSyncExternalStore` 读快照，
`bus.onTasksChanged` 时代数 +1、清空已加载页、重载第一页。11 项 vitest 覆盖
（含"外部删除后重载再翻页无重复无遗漏""在飞 loadMore 被丢弃且不卡在正在加载"）。
**已知边界**：`reload` 不作废在飞的 `loadMore`，彻底解决要 keyset 分页。

## 5. P1 报告的两道上限与真取消（§17~§21）

`REPORT_MAX_ROWS`（10 万）+ `REPORT_MAX_CHARS`（2000 万字符）**逐行**检查
（在 append 之前，而不是渲染完一页之后）；「取消导出」通过 `isCancelled` 在每次取页、
写行之前生效，取消后不写出文件；导出期间 `bus.onTasksChanged` 触发即中止。

## 6. P2 updater 健康码（§22~§23）

四项独立计分（私钥存在 / 密文存在 / 能解密 / 无 legacy 明文），任一不达标 `exit 1`。
六组场景实测，并用 HEAD 基线脚本对照证明修前场景 1/2/3/3b 都是 `0`。

## 7. P2 破坏性验收的隔离 profile（§24~§26）

- 应用侧：`LUMEN_TEST_DATA_DIR` 环境变量（只认环境变量，不接受命令行参数），
  启用时打 warn 说明"本次不使用真实数据目录"。
- 脚本侧：造数据**之前**先读 `app_data_paths` 并规范化比较，是生产目录就
  打印拒绝说明、`exit 1`、**一个写操作都不发**；`--expect-data-dir` 用于声明期望目录，
  `--allow-production` 必须显式传且首尾各警告一次。
  > **后续更新（最终收口轮）**：`--allow-production` 已被彻底删除，目录判定改用
  > `realpath + normcase` 以识别 junction 别名。见本文件「最终收口」一节。
- 原先的 confirm 接管 / 前缀 / 按 id 清理降级为**第二层**防护。

## 8. 测试结果（收口轮，本机实跑）

| 命令 | 结果 |
| --- | --- |
| `pnpm typecheck` | ✅ 0 错误 |
| `pnpm test` | ✅ **125 passed**（第三轮 113） |
| `pnpm build` / `pnpm lint` | ✅ / 0 problems |
| `cargo fmt --check` | ✅ |
| `cargo test --lib` | ✅ **335 passed; 0 failed**（第三轮 331） |
| `cargo clippy --all-targets --all-features -- -D warnings` | ✅ 通过（无豁免） |

## 9. 尚未解决（收口轮）

1. **隔离 profile 下的实机验收没跑**：应用侧开关与脚本拒绝逻辑都做完并各自验证过，
   但 A/B/C/D 四段断言在新代码下**没有实机证据**（§24~§26 的核心要求，不得算已验证）。
2. **keyset / cursor 分页仍未做**；看板的 `reload` 与在飞 `loadMore` 重叠是已知边界。
3. **文件级 symlink 未实测**（无管理员权限，仅 junction）。
4. **报表四组压力数据形态没跑**。
5. **updater `-Build` / `-Clear` 未跑**；"数据变化即取消导出"没有端到端验证。
6. 列表虚拟化、大文件拆分、代码签名、本地模型指引：沿用前两轮结论，未做。
7. **没有发布新版本**：用户机器上仍是 0.3.0。

---
---

# 最终收口

> 依据：《Lumen 最终收口整改任务书》
> 复审基线：`0e681c7`（第三轮收口后的 HEAD）
> 本节同样是**追加**，不修改上面任何历史结论。

## 0. 本轮的新发现（任务书 §35 要求逐条记录）

第三轮收口把"数量 vs 身份"和"符号链接"都修了，但复审又找出一批**同类但更深一层**的问题：

| # | 新发现 | 为什么上一轮没发现 |
| --- | --- | --- |
| 1 | **单条永久删除的"检查"与"DELETE"不原子**：`SELECT` 看 `deleted_at` 之后再 `DELETE WHERE id = ?`，中间有人恢复就仍会被删 | 上一轮把注意力全放在**批量**删除的身份快照上，单条那条路径被认为"只有一条，没什么可错的"——恰恰是它少了条件删除 |
| 2 | **被标注为"已弃用"的旧批量删除仍然暴露在 IPC 上**：`#[tauri::command]` 与 `generate_handler` 注册都还在，前端 CMD 常量与 wrapper 也在 | 把"标注 deprecated"当成了"不可达"。**接口是否暴露必须单独验证**，行为测试覆盖不到 |
| 3 | **Board/PDF 的 bus self-ignore 造成同窗口状态不同步**：默认忽略"自己窗口发出的事件"对主列表是对的，但看板与 PDF 有独立状态，主窗口自己在看板里新建任务不会让看板刷新 | 上一轮只测了"跨窗口"，天然不会覆盖"同窗口"这条路径 |
| 4 | **Board 普通 `reload()` 不作废在飞 `loadMore`**：上一轮明确写进了"已知边界"就算交代了 | 把已知边界写进注释 ≠ 可以不做——它会真实地 append 进一条过时任务 |
| 5 | **PDF 的 cancel 在 `exportPdf` 之前存在时间窗**：第一次检查之后还有"两帧 + 180ms"，用户在这段时间点取消仍会写出文件 | 只检查了"取消能不能被感知到"，没检查"最后一次检查离写文件有多远" |
| 6 | **`attachment_remove` 先删文件再删 DB**：删文件成功、DB 失败 → "活记录指向不存在的文件"，不可自愈 | 前面所有讨论都围绕"删任务时怎么清理副本"，没有人回头看"单独删一个附件"的顺序 |
| 7 | **破坏性验收仍保留 `--allow-production`**，且目录判定用 `abspath + normcase`，junction 别名可绕过 | 上一轮刚做完隔离就留了一个"逃生舱"；而 `abspath` 不解析重解析点这件事，只有把 junction 真的建出来才会暴露 |

这七条的共性：**上一轮修的是"当时想到的攻击面"，本轮补的是"同一类问题的其它入口"**。

## 1. P0-① 单条永久删除：把条件写进 DELETE

```text
BEGIN
  1. SELECT deleted_at（确认存在且在回收站）
  2. 同事务取 copied 附件路径
  3. DELETE FROM tasks WHERE id = ? AND deleted_at IS NOT NULL
  4. rows_affected != 1 → ROLLBACK + conflict
COMMIT
  5. 提交后才删文件
```

关键是第 3 步：**把"仍在回收站"变成原子条件**，而不是在它前面多查几次。
回归测试用真两连接模拟"检查之后被恢复"，断言条件删除命中 0 行、任务仍在、
`deleted_at` 为 NULL、副本文件仍在、真实入口返回冲突。

## 2. P0-② 旧危险 IPC 彻底移除

- 去掉 `#[tauri::command]` 与 `generate_handler` 注册；
- 删掉前端 `CMD.taskPurgeAllDeleted` 与 `purgeAllDeleted` 包装；
- 保留内部 helper 给集成测试当反例；
- **加静态护栏**：Rust 侧 `include_str!` 断言 `lib.rs` 与 `src/lib/ipc.ts` 不含旧命令名，
  前端 `ipc-surface.test.ts` 做同样检查，并反向断言两阶段命令仍在。

## 3. P0-③ 附件删除顺序

先删 DB 记录（失败即返回、一个文件都不碰），成功之后才删副本。
测试用 SQLite 触发器让 DELETE 必然失败，断言**文件仍在**——这正是旧实现会留下
"活记录指向不存在文件"的地方。

## 4. P1 异步一致性

| 位置 | 改动 |
| --- | --- |
| store | 补"卡在 `fetchProgress`"的两条测试（reload / loadMore），突变验证 2 红 |
| bus | `onTasksChanged(cb, { includeSelf })`，默认 false 保持兼容 |
| Board | 用 `includeSelf: true`；`reload()` 递增 generation 以作废在飞 loadMore |
| PDF | 用 `includeSelf: true`；`exportPdf` 前最后一次 cancel 检查 |
| report-export | 取消下沉到每一行；`shouldStop`/`rowGate`/`consumeRows` 三个纯函数统一判定；上限语义统一为"恰好等于仍可写入" |

## 5. P1 验收安全

- 删除 `--allow-production`（无任何绕过开关）；
- `canonical_dir` 用 `realpath + normcase`，junction/symlink 别名无法伪装隔离目录；
- `decide_profile()` 纯函数 + 隐藏自测入口，22 项通过（含真实 junction 别名），
  换回旧写法 18/22 且 exit 1。

## 6. 测试结果（最终收口轮，本机实跑）

| 命令 | 结果 |
| --- | --- |
| `pnpm typecheck` | ✅ 0 错误 |
| `pnpm test` | ✅ **148 passed**（第三轮收口 125） |
| `pnpm build` / `pnpm lint` | ✅ / 0 problems |
| `cargo fmt --check` | ✅ |
| `cargo test --lib` | ✅ **340 passed; 0 failed**（第三轮收口 335） |
| `cargo clippy --all-targets --all-features -- -D warnings` | ✅ 通过（无豁免） |

## 7. 隔离 profile 实机验收（§33）

在 `LUMEN_TEST_DATA_DIR=%TEMP%\lumen-acceptance` 下启动并跑完 `verify_remediation3.py`：
脚本确认 `生产目录=False`（隔离放行路径通），B/C 两段全通过；
**结果 35/42**，失败项为 D 段的 4 条"provider 状态无法区分"与脚本自身清理缺陷 3 条。
用户真实数据目录全程未被触碰，跑完已清理隔离目录。详细记录见
`docs/work-log.md` 第 11 轮的"实机验收"一节（含未实机验收的 A/C/D 三项）。

## 8. 尚未解决（最终收口轮）

1. **§33 的 A/C/D 未实机验收**（单条 purge 竞态、PDF 同窗口变化、PDF 手动取消），只有单元测试证据。
2. **验收脚本的清理缺陷未修**（隔离目录里残留 1460 条，未复现修验证）。
3. symlink（`mklink /D`）形态、`LUMEN_TEST_DATA_DIR` 指向生产别名，均未实测。
4. keyset 分页、文件级 symlink、报表四组压力数据、updater `-Build`/`-Clear`：沿用未做。
5. **0.4.0 未发布**：本轮修完三个发布阻断项，但按任务书 §40 需复审通过后才允许发布。

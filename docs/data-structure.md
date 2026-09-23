# 数据结构说明

> 对应任务书 §12「数据结构说明、迁移与同步策略」。
> 表结构以 `src-tauri/migrations/` 下的 SQL 为准，本文档说明设计意图与口径。

---

## 一、整体约定

### 时间存储

- 所有时间戳以 **UTC ISO-8601 固定宽度字符串**存储：`YYYY-MM-DDTHH:MM:SS.sssZ`（24 字符）。
- 固定宽度的意义：**字符串字典序等价于时间序**，SQLite 无需时间类型即可正确排序与区间查询。
- 本地时区仅用于展示与"今天/本周"归属计算，不持久化。

### 只删除标记，不物理删除

除回收站的"永久删除"外，一切删除都是设置 `deleted_at`。
这样"删除"可撤销，也让统计能区分"从未存在"与"被删除"。

### 三个时间维度互不冲突

| 字段 | 含义 | 刚性 | 谁在用 |
| --- | --- | --- | --- |
| `period_type` | 我在这个周期内完成 | 柔性，允许无日期 | 「周期任务」视图 |
| `planned_at` | 我打算这一天的这个时刻做 | 刚性 | 「今天」「日历」 |
| `due_at` | 我必须在此之前交 | 硬约束 | 逾期判定、统计 |

**关键规则**：`period_type ≠ 'none'` 且 `planned_at IS NULL` 的任务
**不会出现在「今天」视图**——否则"这周做完就行"的任务会每天骚扰用户。

### "仅日期"与"精确时间"

用 `has_planned_time` / `has_due_time` 布尔位区分，而**不是**用"时间部分是否为 00:00"
这种脆弱约定。仅日期任务的逾期判断取**当日结束**，因此全天任务不会在凌晨就被标成逾期。

---

## 二、表清单

### 2.1 核心：任务

```sql
CREATE TABLE tasks (
  id          TEXT PRIMARY KEY,
  title       TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  note_md     TEXT NOT NULL DEFAULT '',     -- Markdown 备注
  link_url    TEXT,

  status      TEXT NOT NULL DEFAULT 'todo'
                CHECK (status IN ('todo','doing','waiting','done','archived')),
  priority    INTEGER NOT NULL DEFAULT 0 CHECK (priority BETWEEN 0 AND 3),

  project_id  TEXT REFERENCES projects(id)   ON DELETE SET NULL,
  category_id TEXT REFERENCES categories(id) ON DELETE SET NULL,

  planned_at       TEXT,                     -- 计划执行时间（决定今天/日历归属）
  has_planned_time INTEGER NOT NULL DEFAULT 0 CHECK (has_planned_time IN (0,1)),
  due_at           TEXT,                     -- 截止时间（决定是否逾期）
  has_due_time     INTEGER NOT NULL DEFAULT 0 CHECK (has_due_time IN (0,1)),

  estimated_minutes INTEGER,
  actual_minutes    INTEGER NOT NULL DEFAULT 0,

  completed_at TEXT,                         -- 完成时间必须真实记录
  created_at   TEXT NOT NULL,
  updated_at   TEXT NOT NULL,
  deleted_at   TEXT,

  sort_order  REAL    NOT NULL DEFAULT 0,
  is_pinned   INTEGER NOT NULL DEFAULT 0 CHECK (is_pinned IN (0,1)),
  is_favorite INTEGER NOT NULL DEFAULT 0 CHECK (is_favorite IN (0,1)),

  period_type TEXT NOT NULL DEFAULT 'none'
                CHECK (period_type IN ('none','day','week','month','quarter','year')),

  -- 重复实例身份（详见 design-recurrence.md）
  series_id      TEXT REFERENCES task_series(id) ON DELETE SET NULL,
  occurrence_key TEXT,                       -- 该次的**原始**计划发生时间（UTC）
  occurrence_index INTEGER,
  occurrence_kind  TEXT CHECK (occurrence_kind IN ('single','generated','exception')),
  is_exception     INTEGER NOT NULL DEFAULT 0 CHECK (is_exception IN (0,1)),

  -- 为将来同步预留，本轮未使用
  sync_rev   INTEGER NOT NULL DEFAULT 0,
  sync_state TEXT    NOT NULL DEFAULT 'local'
);
```

**状态取值**：`todo` 待办 / `doing` 进行中 / `waiting` 等待 / `done` 完成 / `archived` 归档。
「未完成」的判定是 `status IN ('todo','doing','waiting')`。

**优先级**：0 无 / 1 低 / 2 中 / 3 高。

**`occurrence_key` 的语义**：它代表「这一次**原本**该在什么时候发生」，
是实例的稳定身份。用户改期只改 `planned_at`，**不动** `occurrence_key`，
因此不会在旧日期再生成副本。唯一索引强制这一点：

```sql
CREATE UNIQUE INDEX idx_tasks_occurrence
  ON tasks (series_id, occurrence_key)
  WHERE series_id IS NOT NULL AND occurrence_key IS NOT NULL;
```

### 2.2 重复任务（三层模型）

| 表 | 用途 | 关键约束 |
| --- | --- | --- |
| `task_series` | 规则 + 时区 + DTSTART + `rule_version` | `rrule` 为 RFC 5545 风格字符串 |
| `task_series_segments` | 规则分段（"此次及以后"） | `UNIQUE (series_id, rule_version)` |
| `task_series_skips` | 单次跳过（"单次取消"） | `UNIQUE (series_id, occurrence_key)` |

详见 `docs/design-recurrence.md`。

### 2.3 组织维度

| 表 | 含义 | 与任务的关系 |
| --- | --- | --- |
| `projects` | 任务集合 / 列表 | 一对一（任务最多属于一个项目） |
| `categories` | 统计归类维度 | 一对一（与项目正交，可同时设置） |
| `tags` + `task_tags` | 跨项目横向标记 | 多对多 |

三张表的名称唯一性都只在**未删除范围内**生效（部分唯一索引），
因此删除后可以重新使用同名，符合直觉：

```sql
CREATE UNIQUE INDEX idx_projects_name_alive
  ON projects (name) WHERE deleted_at IS NULL;
```

删除项目/分类时的关联任务处理由调用方选择：
`detach`（只解除归属，默认）或 `cascade_soft_delete`（一并进回收站）。

### 2.4 子任务与依赖

| 表 | 为什么独立 |
| --- | --- |
| `subtasks` | 用独立表而非 `tasks.parent_id` 自引用，避免"主子关系"与"依赖关系"混淆 |
| `task_dependencies` | 表达"A 必须先于 B"，只存先后关系；循环检测在应用层（数据库无法表达递归约束） |

`task_dependencies` 有 `CHECK (task_id <> depends_on_id)` 拒绝自依赖；
其余循环（多节点）由应用层 DFS 检测。

### 2.5 提醒

```sql
CREATE TABLE reminders (
  id          TEXT PRIMARY KEY,
  task_id     TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
  kind        TEXT NOT NULL DEFAULT 'custom'
                CHECK (kind IN ('at_due','before_due','at_planned','before_planned','custom')),
  offset_minutes INTEGER,        -- 相对提醒的提前量
  remind_at   TEXT NOT NULL,     -- 绝对 UTC 时刻（由规则在时间变更时重算）
  is_enabled  INTEGER NOT NULL DEFAULT 1,
  fired_at    TEXT,              -- 触发时刻；'expired' 表示因超补发窗作废
  snoozed_until TEXT,
  created_at  TEXT NOT NULL,
  updated_at  TEXT NOT NULL
);
```

**`fired_at` 是去重的关键**：发出前先写它，因此重启、休眠唤醒都不会重复触发。
写入 `'expired'` 表示"因超出补发窗口而作废"——是**可见地作废**而不是静默丢弃。

### 2.6 附件

```sql
CREATE TABLE attachments (
  id            TEXT PRIMARY KEY,
  task_id       TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
  file_name     TEXT NOT NULL,
  mime_type     TEXT,
  byte_size     INTEGER,
  sha256        TEXT,           -- 用于核对副本与原件一致
  storage_mode  TEXT NOT NULL DEFAULT 'reference'
                  CHECK (storage_mode IN ('reference','copied')),
  external_path TEXT,           -- 用户原文件路径（仅记录，永不删除）
  stored_path   TEXT,           -- 复制模式下的受控副本路径
  created_at    TEXT NOT NULL
);
```

**安全约定**：`reference` 模式只在数据库里记录指向用户原文件的信息；
`copied` 模式才复制到数据目录。删除附件或任务**都不会碰用户原文件**，
只有 `copied` 模式会删除自己的副本，且删除前会 `canonicalize` 校验路径
确实位于受控目录内。

### 2.7 专注会话

```sql
CREATE TABLE focus_sessions (
  id             TEXT PRIMARY KEY,
  task_id        TEXT REFERENCES tasks(id) ON DELETE SET NULL,
  kind           TEXT NOT NULL DEFAULT 'pomodoro'
                   CHECK (kind IN ('pomodoro','stopwatch')),
  state          TEXT NOT NULL DEFAULT 'idle'
                   CHECK (state IN ('idle','running','paused','finished','interrupted')),
  planned_seconds INTEGER NOT NULL DEFAULT 1500,
  elapsed_seconds INTEGER NOT NULL DEFAULT 0,
  started_at      TEXT,
  last_resumed_at TEXT,     -- 实际经过 = elapsed + (now - last_resumed_at)
  ended_at        TEXT,
  created_at      TEXT NOT NULL,
  updated_at      TEXT NOT NULL
);
```

**计时以数据库为准**：不依赖内存定时器，因此程序被杀掉后能恢复或明确显示"已中断"。
结束时会话时长累加到 `tasks.actual_minutes`（统计页"投入时间"的来源）。

### 2.8 设置、目标与元数据

| 表 | 用途 | 说明 |
| --- | --- | --- |
| `settings` | 键值对 + JSON 值 | 存窗口配置、AI 配置、成长配置等。**API Key 不在这里** |
| `goals` | 个人目标 | 进度按"目标创建之后完成的任务数"计算 |
| `app_meta` | 应用级元数据 | `schema_version`、`created_by` |
| `_sqlx_migrations` | 迁移记录 | 由 sqlx 维护 |

**API Key 的存储位置**：Windows 凭据管理器（`keyring`），
`settings` 里只有 `has_api_key` 布尔标记。因此**备份文件天然不含密钥**
（任务书 §10 要求"密钥不随备份泄露"）。

---

## 三、索引设计

索引按查询模式而非"每个字段都加"来设计，共有三类：

| 类型 | 例子 | 用途 |
| --- | --- | --- |
| 部分索引（带 `WHERE deleted_at IS NULL`） | `idx_tasks_planned`、`idx_tasks_due` | 绝大多数查询只看未删除数据，部分索引体积更小 |
| 组合索引 | `idx_tasks_alive (deleted_at, status)` | 列表视图的默认筛选 |
| 唯一部分索引 | `idx_projects_name_alive`、`idx_tasks_occurrence` | 表达业务约束（同名检测、实例身份） |

另有 3 个触发器自动维护 `updated_at`，避免应用层遗漏。

---

## 四、外键与级联行为

| 关系 | 行为 | 理由 |
| --- | --- | --- |
| `tasks.project_id` | `ON DELETE SET NULL` | 删项目不应删任务（应用层还提供"解除归属"策略） |
| `tasks.series_id` | `ON DELETE SET NULL` | 删系列时任务保留，但解除关联以免悬挂 |
| `task_tags`、`subtasks`、`task_dependencies`、`reminders`、`attachments` | `ON DELETE CASCADE` | 这些是任务的从属数据，任务没了它们失去意义 |
| `focus_sessions.task_id` | `ON DELETE SET NULL` | 专注记录是历史，任务删除后仍应保留时长统计 |

外键在连接池选项中显式开启（SQLite 默认关闭）。
有测试验证外键确实生效（插入引用不存在任务的子任务会被拒绝）。

---

## 五、迁移策略

| 版本 | 文件 | 内容 |
| --- | --- | --- |
| 1 | `0001_init.sql` | 初始结构：14 表 + 索引 + 触发器 + 应用元数据 |
| 2 | `0002_recurrence_skips.sql` | `task_series_skips` + occurrence 覆盖索引 |
| 3 | `0003_goals.sql` | `goals` |
| 4 | `0004_period_type.sql` | `tasks.period_type` + 部分索引 |

机制：

- 迁移文件由 `sqlx::migrate!` 在**编译期嵌入二进制**，随程序分发；
- 启动时自动执行未应用的迁移；
- **迁移前自动复制数据库**到 `backups/pre-migrate-<时间戳>.db`；
- 迁移失败会中止启动并给出可读错误（不会带着半套结构继续运行）。

---

## 六、完整性保证

| 机制 | 保证 |
| --- | --- |
| 全部写操作走事务 | 不会留下"半条规则" |
| WAL 模式 | 崩溃后可恢复；读并发更好 |
| `PRAGMA foreign_keys = ON` | 引用完整性 |
| `busy_timeout = 10s` | 并发访问时等待而非立即失败 |
| 单实例插件 | 避免两个进程同时写同一个数据库文件 |
| 导出先写临时文件再改名 | 中途失败不会留下"看起来完整"的半截备份 |
| 恢复使用 `VACUUM INTO` 快照 | WAL 中未落盘的内容也会包含，比直接复制 `.db` 更安全 |

---

## 七、同步策略（未实现）

任务书 §9 允许"可选择启用"账号与云同步。本轮按用户决定**不做同步**。

为避免将来加同步时做破坏性迁移，`tasks` 表已预留
`sync_rev` 与 `sync_state` 两个字段（当前恒为 `0` / `'local'`）。

若将来实现，需要补充的设计至少包括：

- 离线修改的合并规则与冲突检测（任务书要求"冲突时不静默覆盖数据"）
- 重复系列的冲突解决——分段与跳过的合并语义（不是简单的行级合并）
- 删除的同步策略（软删除如何传播、是否需要墓碑）
- 附件同步（体积、去重、断点续传）
- 身份与访问控制、服务端部署与备份

**这些均未设计，不应被视为已就绪。**

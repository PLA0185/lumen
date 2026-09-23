-- =============================================================================
-- AiTodo 初始数据库结构  (migration 0001)
--
-- 设计约定（任务书 §4.1 / §5 / §5 时间存储方式）：
--  1) 所有时间戳一律以 **UTC** 存储为 TEXT(ISO-8601, 'YYYY-MM-DDTHH:MM:SS.sssZ')。
--     本地时区仅用于展示与"今天/本周"归属计算，不做持久化。
--  2) "计划时间/截止时间/提醒时间"是三个完全独立的字段，任一可为空（§4.1）。
--  3) "仅日期" 与 "精确时间" 用 has_*_time 布尔位区分，避免把全天任务
--     当作凌晨到期处理（§4.3）。凡 has_*_time = 0，时间部分必须为 00:00:00。
--  4) 优先级共四级：0 无 / 1 低 / 2 中 / 3 高（§4.1"四级优先级"）。
--  5) 一律软删除（deleted_at），回收站据此实现（§4.1）。
-- =============================================================================

PRAGMA foreign_keys = ON;

-- -----------------------------------------------------------------------------
-- 项目（任务书 §4.2：项目的含义是"任务集合/列表"）
-- -----------------------------------------------------------------------------
CREATE TABLE projects (
  id           TEXT PRIMARY KEY NOT NULL,
  name         TEXT NOT NULL,
  description  TEXT NOT NULL DEFAULT '',
  color        TEXT,
  icon         TEXT,
  sort_order   INTEGER NOT NULL DEFAULT 0,
  is_favorite  INTEGER NOT NULL DEFAULT 0 CHECK (is_favorite IN (0, 1)),
  is_archived  INTEGER NOT NULL DEFAULT 0 CHECK (is_archived IN (0, 1)),
  archived_at  TEXT,
  created_at   TEXT NOT NULL,
  updated_at   TEXT NOT NULL,
  deleted_at   TEXT
);
CREATE INDEX idx_projects_archived ON projects (is_archived, sort_order);
CREATE UNIQUE INDEX idx_projects_name_alive
  ON projects (name) WHERE deleted_at IS NULL;

-- -----------------------------------------------------------------------------
-- 分类（与项目语义不同：用于"类别占比"统计，§4.2 + §7）
-- -----------------------------------------------------------------------------
CREATE TABLE categories (
  id          TEXT PRIMARY KEY NOT NULL,
  name        TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  color       TEXT,
  icon        TEXT,
  sort_order  INTEGER NOT NULL DEFAULT 0,
  created_at  TEXT NOT NULL,
  updated_at  TEXT NOT NULL,
  deleted_at  TEXT
);
CREATE UNIQUE INDEX idx_categories_name_alive
  ON categories (name) WHERE deleted_at IS NULL;

-- -----------------------------------------------------------------------------
-- 标签（任务上的多个标签，§4.1 / §4.2）
-- -----------------------------------------------------------------------------
CREATE TABLE tags (
  id         TEXT PRIMARY KEY NOT NULL,
  name       TEXT NOT NULL,
  color      TEXT,
  sort_order INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  deleted_at TEXT
);
CREATE UNIQUE INDEX idx_tags_name_alive ON tags (name) WHERE deleted_at IS NULL;

-- -----------------------------------------------------------------------------
-- 重复系列（任务书 §5：重复任务作为独立核心模块）
--
-- 一个"系列"仅在任务确实重复时才存在（tasks.series_id 指向它）。
-- rrule 为 RFC 5545 RRULE 字符串；dtstart_local 是系列首次发生的"本地"
-- 墙上时间，配合 tzid 才能在 DST 变化下稳定展开。
-- rule_version/segments 支撑"此次及以后"修改：不追改历史（§5）。
-- -----------------------------------------------------------------------------
CREATE TABLE task_series (
  id                  TEXT PRIMARY KEY NOT NULL,
  rrule               TEXT NOT NULL,
  tzid                TEXT NOT NULL DEFAULT 'Asia/Shanghai',
  dtstart_local       TEXT NOT NULL,
  has_start_time      INTEGER NOT NULL DEFAULT 0 CHECK (has_start_time IN (0, 1)),
  recurrence_end_kind TEXT NOT NULL DEFAULT 'never'
                        CHECK (recurrence_end_kind IN ('never', 'until', 'count')),
  recurrence_until    TEXT,
  recurrence_count    INTEGER,
  rule_version        INTEGER NOT NULL DEFAULT 1,
  created_at          TEXT NOT NULL,
  updated_at          TEXT NOT NULL
);

-- -----------------------------------------------------------------------------
-- 规则分段（§5："修改未来"须通过分段或版本化规则实现）
--
-- 语义：本系列在 effective_from_occurrence 及其之后的发生，改用
-- new_rrule / 各字段覆盖值。此前的历史实例不受影响。
-- -----------------------------------------------------------------------------
CREATE TABLE task_series_segments (
  id                       TEXT PRIMARY KEY NOT NULL,
  series_id                TEXT NOT NULL REFERENCES task_series (id) ON DELETE CASCADE,
  rule_version             INTEGER NOT NULL,
  effective_from_occurrence TEXT NOT NULL,
  new_rrule                TEXT,
  override_title           TEXT,
  override_description     TEXT,
  override_priority        INTEGER,
  override_project_id      TEXT,
  override_category_id     TEXT,
  override_estimated_minutes INTEGER,
  created_at               TEXT NOT NULL,
  UNIQUE (series_id, rule_version)
);
CREATE INDEX idx_segments_series ON task_series_segments (series_id, effective_from_occurrence);

-- -----------------------------------------------------------------------------
-- 任务（§4.1 完整字段集）
--
-- series_id / occurrence_key 说明（§5 "稳定身份"）：
--   occurrence_key = 该次发生的**原始计划发生时间**（UTC ISO 字符串）。
--   即使实例被改期到别的日子，occurrence_key 保持不变，
--   因此"系列 ID + occurrence_key"是稳定身份，不会在旧日期再生成副本。
-- -----------------------------------------------------------------------------
CREATE TABLE tasks (
  id          TEXT PRIMARY KEY NOT NULL,
  title       TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  note_md     TEXT NOT NULL DEFAULT '',
  link_url    TEXT,

  status      TEXT NOT NULL DEFAULT 'todo'
                CHECK (status IN ('todo', 'doing', 'waiting', 'done', 'archived')),
  priority    INTEGER NOT NULL DEFAULT 0 CHECK (priority BETWEEN 0 AND 3),

  project_id  TEXT REFERENCES projects (id) ON DELETE SET NULL,
  category_id TEXT REFERENCES categories (id) ON DELETE SET NULL,

  -- 计划执行时间（决定"今天"与日历中的位置）
  planned_at    TEXT,
  has_planned_time INTEGER NOT NULL DEFAULT 0 CHECK (has_planned_time IN (0, 1)),
  -- 截止时间（用于逾期判断），可单独为空（§4.1、§4.2）
  due_at        TEXT,
  has_due_time  INTEGER NOT NULL DEFAULT 0 CHECK (has_due_time IN (0, 1)),

  estimated_minutes INTEGER,
  actual_minutes    INTEGER NOT NULL DEFAULT 0,

  completed_at TEXT,
  created_at   TEXT NOT NULL,
  updated_at   TEXT NOT NULL,
  deleted_at   TEXT,

  -- 排序（§4.1 手动排序）与置顶（§4.2 置顶任务与窗口置顶无关）
  sort_order   REAL NOT NULL DEFAULT 0,
  is_pinned    INTEGER NOT NULL DEFAULT 0 CHECK (is_pinned IN (0, 1)),
  is_favorite  INTEGER NOT NULL DEFAULT 0 CHECK (is_favorite IN (0, 1)),

  -- 重复实例身份（§5）
  series_id      TEXT REFERENCES task_series (id) ON DELETE SET NULL,
  occurrence_key TEXT,
  occurrence_index INTEGER,
  occurrence_kind TEXT CHECK (occurrence_kind IN ('single', 'generated', 'exception')),
  is_exception   INTEGER NOT NULL DEFAULT 0 CHECK (is_exception IN (0, 1)),

  -- 未来 9 启用账号同步时使用；本轮不启用同步，但预留字段避免将来破坏性迁移
  sync_rev    INTEGER NOT NULL DEFAULT 0,
  sync_state  TEXT NOT NULL DEFAULT 'local'
);

-- "稳定身份"唯一性：同一系列的同一原始发生时刻只能有一条任务
CREATE UNIQUE INDEX idx_tasks_occurrence
  ON tasks (series_id, occurrence_key)
  WHERE series_id IS NOT NULL AND occurrence_key IS NOT NULL;

CREATE INDEX idx_tasks_alive       ON tasks (deleted_at, status);
CREATE INDEX idx_tasks_planned     ON tasks (planned_at)     WHERE deleted_at IS NULL;
CREATE INDEX idx_tasks_due         ON tasks (due_at)         WHERE deleted_at IS NULL AND due_at IS NOT NULL;
CREATE INDEX idx_tasks_project     ON tasks (project_id)     WHERE deleted_at IS NULL;
CREATE INDEX idx_tasks_category    ON tasks (category_id)    WHERE deleted_at IS NULL;
CREATE INDEX idx_tasks_status_done ON tasks (status, completed_at) WHERE deleted_at IS NULL;
CREATE INDEX idx_tasks_pinned      ON tasks (is_pinned, sort_order) WHERE deleted_at IS NULL;
CREATE INDEX idx_tasks_series      ON tasks (series_id)      WHERE series_id IS NOT NULL;

-- 任务 ↔ 标签（多对多）
CREATE TABLE task_tags (
  task_id TEXT NOT NULL REFERENCES tasks (id) ON DELETE CASCADE,
  tag_id  TEXT NOT NULL REFERENCES tags  (id) ON DELETE CASCADE,
  PRIMARY KEY (task_id, tag_id)
);
CREATE INDEX idx_task_tags_tag ON task_tags (tag_id);

-- -----------------------------------------------------------------------------
-- 子任务（§4.1：子任务、进度显示、主子任务完成规则）
-- 用独立表而非 parent_id 自引用，避免"主子关系"与"依赖关系"混淆。
-- -----------------------------------------------------------------------------
CREATE TABLE subtasks (
  id         TEXT PRIMARY KEY NOT NULL,
  task_id    TEXT NOT NULL REFERENCES tasks (id) ON DELETE CASCADE,
  title      TEXT NOT NULL,
  is_done    INTEGER NOT NULL DEFAULT 0 CHECK (is_done IN (0, 1)),
  sort_order REAL NOT NULL DEFAULT 0,
  completed_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX idx_subtasks_task ON subtasks (task_id, sort_order);

-- -----------------------------------------------------------------------------
-- 任务依赖（§4.1：支持任务依赖及循环依赖校验）
-- ON DELETE CASCADE：删除前置任务时依赖边随之消失，不会留下悬挂引用。
-- 循环校验在应用层完成（数据库无法表达递归约束）。
-- -----------------------------------------------------------------------------
CREATE TABLE task_dependencies (
  task_id      TEXT NOT NULL REFERENCES tasks (id) ON DELETE CASCADE,
  depends_on_id TEXT NOT NULL REFERENCES tasks (id) ON DELETE CASCADE,
  created_at   TEXT NOT NULL,
  PRIMARY KEY (task_id, depends_on_id),
  CHECK (task_id <> depends_on_id)
);
CREATE INDEX idx_deps_depends_on ON task_dependencies (depends_on_id);

-- -----------------------------------------------------------------------------
-- 提醒（§4.3：到期、提前、自定义多个提醒，可单独开关）
-- remind_at 为绝对 UTC 时刻，由规则在"计划/截止时间变更"时重算。
-- kind: at_due / before_due / at_planned / before_planned / custom
-- -----------------------------------------------------------------------------
CREATE TABLE reminders (
  id          TEXT PRIMARY KEY NOT NULL,
  task_id     TEXT NOT NULL REFERENCES tasks (id) ON DELETE CASCADE,
  kind        TEXT NOT NULL DEFAULT 'custom'
                CHECK (kind IN ('at_due', 'before_due', 'at_planned', 'before_planned', 'custom')),
  offset_minutes INTEGER,
  remind_at   TEXT NOT NULL,
  is_enabled  INTEGER NOT NULL DEFAULT 1 CHECK (is_enabled IN (0, 1)),
  -- fired_at 用于去重：重启/休眠唤醒后不得重复轰炸（§4.3）
  fired_at    TEXT,
  -- 稍后提醒：记录被推迟后的新时刻
  snoozed_until TEXT,
  created_at  TEXT NOT NULL,
  updated_at  TEXT NOT NULL
);
CREATE INDEX idx_reminders_due ON reminders (is_enabled, remind_at)
  WHERE fired_at IS NULL;
CREATE INDEX idx_reminders_task ON reminders (task_id);

-- -----------------------------------------------------------------------------
-- 附件（§4.1：受控存储，删除任务不得误删用户原文件）
-- 设计：external_path 指向用户原始文件，仅记录引用；
--       删除附件记录不删除原文件。复制模式下的副本存 stored_path。
-- -----------------------------------------------------------------------------
CREATE TABLE attachments (
  id            TEXT PRIMARY KEY NOT NULL,
  task_id       TEXT NOT NULL REFERENCES tasks (id) ON DELETE CASCADE,
  file_name     TEXT NOT NULL,
  mime_type     TEXT,
  byte_size     INTEGER,
  sha256        TEXT,
  storage_mode  TEXT NOT NULL DEFAULT 'reference'
                  CHECK (storage_mode IN ('reference', 'copied')),
  external_path TEXT,
  stored_path   TEXT,
  created_at    TEXT NOT NULL
);
CREATE INDEX idx_attachments_task ON attachments (task_id);

-- -----------------------------------------------------------------------------
-- 专注会话（§4.4：任务绑定的番茄钟、可暂停/恢复、时间记录）
-- 断电/退出后据此恢复计时或明确说明中断。
-- -----------------------------------------------------------------------------
CREATE TABLE focus_sessions (
  id             TEXT PRIMARY KEY NOT NULL,
  task_id        TEXT REFERENCES tasks (id) ON DELETE SET NULL,
  kind           TEXT NOT NULL DEFAULT 'pomodoro'
                   CHECK (kind IN ('pomodoro', 'stopwatch')),
  state          TEXT NOT NULL DEFAULT 'idle'
                   CHECK (state IN ('idle', 'running', 'paused', 'finished', 'interrupted')),
  planned_seconds  INTEGER NOT NULL DEFAULT 1500,
  elapsed_seconds  INTEGER NOT NULL DEFAULT 0,
  started_at     TEXT,
  last_resumed_at TEXT,
  ended_at       TEXT,
  created_at     TEXT NOT NULL,
  updated_at     TEXT NOT NULL
);
CREATE INDEX idx_focus_task ON focus_sessions (task_id);

-- -----------------------------------------------------------------------------
-- 设置（§8 窗口配置、§3 主题、通知开关等）
-- 键值对 + JSON 值，便于配置迁移（§2.4）。
-- 注意：API Key 不存这里，走系统凭据管理器（§6）。
-- -----------------------------------------------------------------------------
CREATE TABLE settings (
  key        TEXT PRIMARY KEY NOT NULL,
  value_json TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

-- -----------------------------------------------------------------------------
-- 迁移记录（sqlx 自建 _sqlx_migrations，此处额外记录应用级 schema 版本）
-- -----------------------------------------------------------------------------
CREATE TABLE app_meta (
  key        TEXT PRIMARY KEY NOT NULL,
  value      TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
INSERT INTO app_meta (key, value, updated_at)
VALUES ('schema_version', '1', strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
       ('created_by', 'aitodo/0.1.0', strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));

-- -----------------------------------------------------------------------------
-- updated_at 自动维护触发器
-- -----------------------------------------------------------------------------
CREATE TRIGGER trg_tasks_updated_at
AFTER UPDATE ON tasks FOR EACH ROW
WHEN OLD.updated_at = NEW.updated_at
BEGIN
  UPDATE tasks SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = NEW.id;
END;

CREATE TRIGGER trg_projects_updated_at
AFTER UPDATE ON projects FOR EACH ROW
WHEN OLD.updated_at = NEW.updated_at
BEGIN
  UPDATE projects SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = NEW.id;
END;

CREATE TRIGGER trg_tags_updated_at
AFTER UPDATE ON tags FOR EACH ROW
WHEN OLD.updated_at = NEW.updated_at
BEGIN
  UPDATE tags SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = NEW.id;
END;

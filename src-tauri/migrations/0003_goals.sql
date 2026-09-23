-- =============================================================================
-- 个人目标  (migration 0003)
--
-- 任务书 §7 要求「提供个人目标及可手动或按任务关联的进度」。
--
-- 设计说明：目标进度按「目标创建之后完成的任务数」计算，而不是全部历史。
-- 原因：老用户新建一个"完成 10 项"的目标时，如果按全部历史计数会立刻
-- 显示 100%，目标就失去意义了。因此 created_at 是进度计算的分界线。
--
-- 只存"目标数量 + 起始时间"，不把任务硬绑到目标上：
-- 硬绑定会让用户必须逐个指派任务，而 §7 只要求"可手动或按任务关联的进度"。
-- 若将来需要精确关联，可再加一张 goal_tasks 关联表，不必改现有结构。
-- =============================================================================

CREATE TABLE goals (
  id            TEXT PRIMARY KEY NOT NULL,
  title         TEXT NOT NULL,
  -- 目标要完成的任务数量
  target_count  INTEGER NOT NULL CHECK (target_count > 0),
  -- 可选截止日期（本地日期 YYYY-MM-DD）
  due_date      TEXT,
  created_at    TEXT NOT NULL
);

CREATE INDEX idx_goals_created ON goals (created_at DESC);

-- 推进应用级 schema 版本
UPDATE app_meta
SET value = '3', updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE key = 'schema_version';

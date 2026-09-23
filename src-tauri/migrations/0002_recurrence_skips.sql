-- =============================================================================
-- 重复任务的"单次跳过"记录  (migration 0002)
--
-- 任务书 §5 明确要求：「单次取消显示为该次跳过，不意外取消整条系列」。
--
-- 为什么不复用 tasks 表加一个 skipped 状态？
--   任务表里的每一行都会出现在列表、日历、统计中，而"被跳过的那一次"
--   是一段**不存在**的发生，把它当任务存储会让所有查询都要额外排除它，
--   而且统计的分母会被污染（§7 要求"清楚标注分母"）。
--   独立一张表语义清晰：跳过的发生既不是任务，也不是数据缺失。
--
-- occurrence_key 的语义与 tasks 表完全一致：
--   该次的**原始计划发生时间**（UTC ISO-8601）。即使系列规则后来变了，
--   只要这次跳过的原始时刻不变，记录就持续有效——这正是"稳定身份"。
-- =============================================================================

CREATE TABLE task_series_skips (
  id              TEXT PRIMARY KEY NOT NULL,
  series_id       TEXT NOT NULL REFERENCES task_series (id) ON DELETE CASCADE,
  -- 被跳过的原始计划发生时刻（UTC）
  occurrence_key  TEXT NOT NULL,
  -- 该次在系列中的序号（便于排查与展示）
  occurrence_index INTEGER,
  -- 跳过原因：user = 用户主动取消这一次；rule_mismatch = 规则变更后该次不再发生
  reason          TEXT NOT NULL DEFAULT 'user'
                    CHECK (reason IN ('user', 'rule_mismatch')),
  created_at      TEXT NOT NULL,
  -- 同一系列的同一次只能跳过一次
  UNIQUE (series_id, occurrence_key)
);

CREATE INDEX idx_series_skips_series
  ON task_series_skips (series_id, occurrence_key);

-- 为方便"按系列查实例"的惰性展开，补一个覆盖索引：
-- 物化时需要按 (series_id, occurrence_key) 判断实例是否已存在。
CREATE INDEX IF NOT EXISTS idx_tasks_series_occurrence
  ON tasks (series_id, occurrence_key)
  WHERE series_id IS NOT NULL;

-- 记录应用级 schema 版本
UPDATE app_meta
SET value = '2', updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE key = 'schema_version';

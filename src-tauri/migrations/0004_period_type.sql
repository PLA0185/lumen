-- =============================================================================
-- 任务的周期跨度  (migration 0004)
--
-- ## 解决什么问题
--
-- 原先把「本周」「本月」只当作**时间范围筛选**（查 planned_at 落在区间内的任务），
-- 于是无法表达一种很常见的需求：
--
--   「这件事这周做完就行，不用定到具体哪一天」
--   「这个月内完成即可，别每天催我」
--
-- 这类任务既没有具体计划日，也不适合当成"今天待办"。加上 period_type 后，
-- 任务可以只声明"我在本周/本月内完成"，日历与「今天」视图不会因为它而
-- 每天弹出提示，但「周任务」「月任务」视图能集中显示。
--
-- ## 与 planned_at / due_at 的关系（三者互补，不冲突）
--
--   period_type  我在这个周期内完成（柔性，允许没有具体日期）
--   planned_at   我打算在这一天的这个时刻做（刚性，决定日历位置）
--   due_at       我必须在此之前交（硬约束，决定是否逾期）
--
-- 关键规则：**period 型任务没有 planned_at 时，不会出现在「今天」视图**。
-- 否则"这周做完就行"的任务会每天骚扰用户，反而比不定时间更糟。
--
-- ## 为什么用 TEXT + CHECK 而不是整数枚举
--
-- 与项目里其它状态字段（status、priority 用的是 INTEGER/TEXT）保持一致：
-- TEXT 在数据库里自解释，排查问题时一眼能看懂 'week' 与 'month' 的区别，
-- 而 2 和 3 需要回查代码。
-- =============================================================================

ALTER TABLE tasks ADD COLUMN period_type TEXT NOT NULL DEFAULT 'none'
  CHECK (period_type IN ('none', 'day', 'week', 'month', 'quarter', 'year'));

-- 周期类型 + 状态的组合查询是「周任务」「月任务」视图的主要访问路径
CREATE INDEX IF NOT EXISTS idx_tasks_period
  ON tasks (period_type, status)
  WHERE deleted_at IS NULL AND period_type <> 'none';

-- 推进应用级 schema 版本
UPDATE app_meta
SET value = '4', updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE key = 'schema_version';

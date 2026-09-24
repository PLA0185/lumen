-- -----------------------------------------------------------------------------
-- 0005：区分「用户主动关闭提醒」与「系统因依赖时间缺失临时停用」
--
-- 背景（整改任务书 §6）：原实现只有 `is_enabled` 一个开关。任务清空截止时间时
-- 系统会把相对型提醒置为 `is_enabled = 0`（合理），但用户之后重新填上时间，
-- 提醒虽然重算了 `remind_at`，却仍然保持停用——**永久不再触发**，而且界面上
-- 看不出原因。
--
-- 反过来，如果简单地"时间恢复就自动启用"，又会把**用户自己关掉的**提醒
-- 偷偷打开。两种状态必须分开记录，所以新增 `disabled_reason`：
--
--   NULL                 → 未被停用（正常启用）
--   'user'               → 用户主动关闭，任何自动逻辑都不得重新打开
--   'missing_base_time'  → 系统因缺少 planned_at / due_at 而临时停用，
--                          依赖时间恢复后由系统自动恢复启用
--
-- 兼容性：加列不影响既有数据与旧版本读取；已有的 `is_enabled = 0` 记录
-- 无法判断原因，一律保守地当作 'user'（宁可让用户手动打开，
-- 也不要擅自把用户关掉的提醒打开）。
-- -----------------------------------------------------------------------------

ALTER TABLE reminders ADD COLUMN disabled_reason TEXT
  CHECK (disabled_reason IS NULL OR disabled_reason IN ('user', 'missing_base_time'));

UPDATE reminders SET disabled_reason = 'user' WHERE is_enabled = 0;

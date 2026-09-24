-- A generated occurrence stops being disposable as soon as a user changes
-- instance state. Existing related rows are also checked at rebuild time.
ALTER TABLE tasks ADD COLUMN is_user_modified INTEGER NOT NULL DEFAULT 0
  CHECK (is_user_modified IN (0, 1));

-- UTC occurrence keys form a stable exclusive cutoff. A local UNTIL date is
-- not an adequate representation of "delete this and all later occurrences".
ALTER TABLE task_series ADD COLUMN terminated_from_occurrence_key TEXT;

-- A timezone change starts a new segment. Historical rules retain their own
-- timezone so later range requests cannot reinterpret old occurrence keys.
ALTER TABLE task_series_segments ADD COLUMN new_tzid TEXT;

-- Legacy rows may have been changed before this migration. A timestamp change
-- is conservative: preserving too many old rows is safer than losing data.
UPDATE tasks SET is_user_modified = 1
WHERE series_id IS NOT NULL AND occurrence_kind = 'generated'
  AND (updated_at <> created_at OR status <> 'todo' OR completed_at IS NOT NULL
       OR actual_minutes <> 0 OR is_pinned <> 0 OR is_favorite <> 0
       OR deleted_at IS NOT NULL);

-- This covers all task-state writers, including direct IPC and future code.
-- Series-wide template fields are intentionally absent: their updates are
-- generated from a scope choice and must not pin every future occurrence.
CREATE TRIGGER trg_preserve_generated_instance_state
AFTER UPDATE OF status, planned_at, has_planned_time, due_at, has_due_time,
                actual_minutes, is_pinned, is_favorite, sort_order,
                deleted_at, period_type ON tasks
FOR EACH ROW
WHEN OLD.series_id IS NOT NULL AND OLD.occurrence_kind = 'generated'
 AND OLD.is_user_modified = 0
 AND (OLD.status IS NOT NEW.status
      OR OLD.planned_at IS NOT NEW.planned_at
      OR OLD.has_planned_time IS NOT NEW.has_planned_time
      OR OLD.due_at IS NOT NEW.due_at
      OR OLD.has_due_time IS NOT NEW.has_due_time
      OR OLD.actual_minutes IS NOT NEW.actual_minutes
      OR OLD.is_pinned IS NOT NEW.is_pinned
      OR OLD.is_favorite IS NOT NEW.is_favorite
      OR OLD.sort_order IS NOT NEW.sort_order
      OR OLD.deleted_at IS NOT NEW.deleted_at
      OR OLD.period_type IS NOT NEW.period_type)
BEGIN
  UPDATE tasks SET is_user_modified = 1 WHERE id = NEW.id;
END;

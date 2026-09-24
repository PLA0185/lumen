-- Persistent source for future occurrences. A mutable task is never a template.
CREATE TABLE task_series_template (
  series_id TEXT PRIMARY KEY NOT NULL REFERENCES task_series (id) ON DELETE CASCADE,
  title TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  note_md TEXT NOT NULL DEFAULT '',
  link_url TEXT,
  priority INTEGER NOT NULL DEFAULT 0,
  project_id TEXT REFERENCES projects (id) ON DELETE SET NULL,
  category_id TEXT REFERENCES categories (id) ON DELETE SET NULL,
  estimated_minutes INTEGER
);

CREATE TABLE task_series_tags (
  series_id TEXT NOT NULL REFERENCES task_series (id) ON DELETE CASCADE,
  tag_id TEXT NOT NULL REFERENCES tags (id) ON DELETE CASCADE,
  PRIMARY KEY (series_id, tag_id)
);

-- A committed rule change that still needs regeneration is durable and
-- retried by recurrence maintenance after a crash or transient failure.
CREATE TABLE task_series_rebuilds (
  series_id TEXT PRIMARY KEY NOT NULL REFERENCES task_series (id) ON DELETE CASCADE,
  range_start_utc TEXT NOT NULL,
  range_end_utc TEXT NOT NULL,
  requested_at TEXT NOT NULL,
  last_error TEXT
);

-- Old data has no canonical template. Prefer an uncompleted generated task,
-- then any generated task, then an exception. A later generated task is more
-- likely to reflect a whole-series edit than completed history.
WITH ranked AS (
  SELECT t.*,
    ROW_NUMBER() OVER (
      PARTITION BY series_id
      ORDER BY
        CASE WHEN occurrence_kind = 'generated' AND status <> 'done'
                       AND completed_at IS NULL AND deleted_at IS NULL THEN 0
             WHEN occurrence_kind = 'generated' THEN 1 ELSE 2 END,
        occurrence_key DESC
    ) AS template_rank
  FROM tasks t WHERE t.series_id IS NOT NULL
)
INSERT INTO task_series_template
  (series_id, title, description, note_md, link_url, priority,
   project_id, category_id, estimated_minutes)
SELECT series_id, title, description, note_md, link_url, priority,
       project_id, category_id, estimated_minutes
FROM ranked WHERE template_rank = 1;

-- A legacy series can have only exceptions; retain those tags too. No task
-- rows or occurrence keys are changed by this migration.
INSERT OR IGNORE INTO task_series_tags (series_id, tag_id)
SELECT DISTINCT t.series_id, tt.tag_id
FROM tasks t JOIN task_tags tt ON tt.task_id = t.id
WHERE t.series_id IS NOT NULL;

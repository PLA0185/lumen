ALTER TABLE tasks
    ADD COLUMN subtask_completion_mode TEXT NOT NULL DEFAULT 'free'
    CHECK (subtask_completion_mode IN ('free', 'sequential'));

ALTER TABLE task_series_template
    ADD COLUMN subtask_completion_mode TEXT NOT NULL DEFAULT 'free'
    CHECK (subtask_completion_mode IN ('free', 'sequential'));

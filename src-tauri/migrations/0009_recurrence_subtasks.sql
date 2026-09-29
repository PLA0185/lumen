-- Existing subtasks remain single-occurrence data until an explicit scope choice.
ALTER TABLE task_series_template ADD COLUMN subtasks_json TEXT NOT NULL DEFAULT '[]'
    CHECK (json_valid(subtasks_json) AND json_type(subtasks_json) = 'array');
ALTER TABLE subtasks ADD COLUMN series_template_id TEXT;
CREATE UNIQUE INDEX idx_subtasks_template_instance ON subtasks(task_id, series_template_id)
    WHERE series_template_id IS NOT NULL;

-- Reminders generated from a recurrence rule must be distinguishable from
-- reminders the user added to a single occurrence, so rule rebuilds can
-- replace the former while preserving the latter.
ALTER TABLE reminders
  ADD COLUMN series_default INTEGER NOT NULL DEFAULT 0
  CHECK (series_default IN (0, 1));

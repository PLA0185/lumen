-- Scene rules and their visual reference images are local-only. They are
-- included in full Lumen backups but intentionally excluded from cloud sync.
CREATE TABLE scene_templates (
    id TEXT PRIMARY KEY NOT NULL,
    mime TEXT NOT NULL CHECK(mime IN ('image/png', 'image/jpeg')),
    data_base64 TEXT NOT NULL,
    byte_size INTEGER NOT NULL CHECK(byte_size > 0 AND byte_size <= 2097152),
    sha256 TEXT NOT NULL CHECK(length(sha256) = 64),
    width INTEGER NOT NULL CHECK(width > 0 AND width <= 1024),
    height INTEGER NOT NULL CHECK(height > 0 AND height <= 1024),
    created_at TEXT NOT NULL
);

CREATE TABLE scene_rules (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL CHECK(length(trim(name)) BETWEEN 1 AND 120),
    enabled INTEGER NOT NULL DEFAULT 0 CHECK(enabled IN (0, 1)),
    priority INTEGER NOT NULL DEFAULT 0,
    program_allowlist_json TEXT NOT NULL,
    window_title_contains_json TEXT NOT NULL DEFAULT '[]',
    match_mode TEXT NOT NULL CHECK(match_mode IN ('image', 'ocr', 'image_and_ocr')),
    template_id TEXT REFERENCES scene_templates(id) ON DELETE RESTRICT,
    template_config_json TEXT NOT NULL DEFAULT '{}',
    ocr_required_terms_json TEXT NOT NULL DEFAULT '[]',
    ocr_excluded_terms_json TEXT NOT NULL DEFAULT '[]',
    headline TEXT NOT NULL CHECK(length(trim(headline)) BETWEEN 1 AND 240),
    body_md TEXT NOT NULL DEFAULT '',
    memo_id TEXT REFERENCES memo_documents(id) ON DELETE SET NULL,
    step_id TEXT,
    stable_hits INTEGER NOT NULL DEFAULT 2 CHECK(stable_hits BETWEEN 1 AND 10),
    stable_leave_ms INTEGER NOT NULL DEFAULT 6000 CHECK(stable_leave_ms BETWEEN 1000 AND 60000),
    default_snooze_minutes INTEGER NOT NULL DEFAULT 10 CHECK(default_snooze_minutes BETWEEN 1 AND 1440),
    revision INTEGER NOT NULL DEFAULT 1 CHECK(revision > 0),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT,
    CHECK(step_id IS NULL OR memo_id IS NOT NULL)
);
CREATE INDEX idx_scene_rules_enabled_priority
    ON scene_rules(enabled, priority DESC, updated_at DESC)
    WHERE deleted_at IS NULL;
CREATE INDEX idx_scene_rules_template ON scene_rules(template_id);

CREATE TABLE scene_runtime (
    rule_id TEXT PRIMARY KEY NOT NULL REFERENCES scene_rules(id) ON DELETE CASCADE,
    state TEXT NOT NULL DEFAULT 'outside' CHECK(state IN ('outside', 'entering', 'visible', 'acked', 'snoozed', 'exiting')),
    window_identity_hash TEXT,
    last_seen_at TEXT,
    last_fired_at TEXT,
    last_ack_at TEXT,
    snooze_until TEXT,
    updated_at TEXT NOT NULL
);

CREATE TABLE scene_events (
    id TEXT PRIMARY KEY NOT NULL,
    rule_id TEXT REFERENCES scene_rules(id) ON DELETE SET NULL,
    occurred_at TEXT NOT NULL,
    event_type TEXT NOT NULL,
    result_code TEXT NOT NULL,
    image_score REAL,
    elapsed_ms INTEGER NOT NULL CHECK(elapsed_ms >= 0)
);
CREATE INDEX idx_scene_events_time ON scene_events(occurred_at DESC, id DESC);

CREATE TRIGGER scene_rules_clear_flow_step_before_memo_delete
BEFORE DELETE ON memo_documents
BEGIN
    UPDATE scene_rules SET memo_id = NULL, step_id = NULL, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    WHERE memo_id = OLD.id;
END;

CREATE TABLE scene_monitor_settings (
    id TEXT PRIMARY KEY NOT NULL CHECK(id = 'default'),
    enabled INTEGER NOT NULL DEFAULT 0 CHECK(enabled IN (0, 1)),
    paused INTEGER NOT NULL DEFAULT 0 CHECK(paused IN (0, 1)),
    consented_at TEXT,
    updated_at TEXT NOT NULL
);
INSERT INTO scene_monitor_settings (id, enabled, paused, consented_at, updated_at)
VALUES ('default', 0, 0, NULL, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));

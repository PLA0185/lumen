ALTER TABLE memo_documents ADD COLUMN sync_event_id TEXT;
CREATE TABLE memo_sync_events (
    id TEXT PRIMARY KEY NOT NULL,
    memo_id TEXT NOT NULL,
    parents_json TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    uploaded INTEGER NOT NULL DEFAULT 0 CHECK(uploaded IN (0,1))
);
CREATE INDEX memo_sync_events_memo ON memo_sync_events(memo_id,created_at,id);
CREATE TABLE memo_sync_parents (
    event_id TEXT NOT NULL REFERENCES memo_sync_events(id) ON DELETE CASCADE,
    parent_id TEXT NOT NULL REFERENCES memo_sync_events(id),
    PRIMARY KEY(event_id,parent_id)
);
-- Device identity, network budget and status are local; they are not business history.
CREATE TABLE memo_sync_runtime (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    device_id TEXT NOT NULL,
    head_generation INTEGER NOT NULL DEFAULT 1,
    published_generation INTEGER NOT NULL DEFAULT 0,
    restore_pending INTEGER NOT NULL DEFAULT 0,
    budget_start INTEGER NOT NULL DEFAULT 0,
    request_count INTEGER NOT NULL DEFAULT 0,
    device_count INTEGER NOT NULL DEFAULT 1,
    retry_until INTEGER NOT NULL DEFAULT 0,
    last_scan TEXT,
    last_upload TEXT,
    last_error TEXT
);
CREATE TABLE cloud_records (
    id TEXT PRIMARY KEY NOT NULL,
    table_name TEXT NOT NULL,
    key_json TEXT NOT NULL,
    base_json TEXT,
    selected_event TEXT
);
CREATE TABLE cloud_events (
    id TEXT PRIMARY KEY NOT NULL,
    record_id TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    packet_id TEXT,
    uploaded INTEGER NOT NULL DEFAULT 0 CHECK(uploaded IN (0,1))
);
CREATE INDEX cloud_events_record ON cloud_events(record_id);
CREATE TABLE cloud_parents (
    event_id TEXT NOT NULL REFERENCES cloud_events(id) ON DELETE CASCADE,
    parent_id TEXT NOT NULL REFERENCES cloud_events(id),
    PRIMARY KEY(event_id,parent_id)
);
CREATE TABLE cloud_packets (
    id TEXT PRIMARY KEY NOT NULL,
    payload_json TEXT NOT NULL,
    uploaded INTEGER NOT NULL DEFAULT 0 CHECK(uploaded IN (0,1))
);
CREATE TABLE cloud_packet_parents (
    packet_id TEXT NOT NULL REFERENCES cloud_packets(id) ON DELETE CASCADE,
    parent_id TEXT NOT NULL REFERENCES cloud_packets(id),
    PRIMARY KEY(packet_id,parent_id)
);
CREATE TABLE cloud_files (
    id TEXT PRIMARY KEY NOT NULL,
    body_json TEXT NOT NULL
);
CREATE TABLE cloud_aliases (
    table_name TEXT NOT NULL,
    local_id TEXT NOT NULL,
    canonical_id TEXT NOT NULL,
    PRIMARY KEY(table_name,local_id)
);

-- 独立备忘与业务流程，不进入任务统计；升级沿用迁移前一致性快照。
CREATE TABLE memo_documents (
    id TEXT PRIMARY KEY NOT NULL,
    title TEXT NOT NULL CHECK (length(trim(title)) > 0),
    category TEXT NOT NULL DEFAULT '',
    kind TEXT NOT NULL CHECK (kind IN ('memo', 'flow')),
    body_md TEXT NOT NULL DEFAULT '',
    steps_json TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(steps_json)),
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision > 0),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT
);
CREATE INDEX idx_memo_documents_visible ON memo_documents(deleted_at, updated_at, id);

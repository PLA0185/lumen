-- Immutable, self-contained media referenced by lumen-asset:UUID in Markdown.
CREATE TABLE content_assets (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    mime TEXT NOT NULL,
    data_base64 TEXT NOT NULL,
    byte_size INTEGER NOT NULL CHECK(byte_size >= 0 AND byte_size <= 20971520),
    sha256 TEXT NOT NULL CHECK(length(sha256) = 64),
    created_at TEXT NOT NULL
);

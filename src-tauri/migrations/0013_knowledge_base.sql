-- Original knowledge files remain immutable content assets. Extracted text and
-- chunks are local derived data; the FTS index is rebuilt by chunk triggers.
CREATE TABLE knowledge_sources (
    id TEXT PRIMARY KEY NOT NULL,
    asset_id TEXT NOT NULL UNIQUE REFERENCES content_assets(id),
    title TEXT NOT NULL CHECK(length(title) BETWEEN 1 AND 255),
    mime TEXT NOT NULL,
    sha256 TEXT NOT NULL UNIQUE CHECK(length(sha256) = 64),
    extracted_text TEXT NOT NULL DEFAULT '',
    warnings_json TEXT NOT NULL DEFAULT '[]',
    status TEXT NOT NULL CHECK(status IN ('ready', 'unreadable')),
    error TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CHECK((status = 'ready' AND error IS NULL) OR status = 'unreadable')
);

CREATE INDEX idx_knowledge_sources_status_title
    ON knowledge_sources(status, title COLLATE NOCASE, id);

CREATE TABLE knowledge_chunks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_id TEXT NOT NULL REFERENCES knowledge_sources(id) ON DELETE CASCADE,
    locator TEXT NOT NULL,
    heading TEXT NOT NULL DEFAULT '',
    content TEXT NOT NULL,
    start_offset INTEGER NOT NULL CHECK(start_offset >= 0),
    end_offset INTEGER NOT NULL CHECK(end_offset >= start_offset)
);

CREATE INDEX idx_knowledge_chunks_source ON knowledge_chunks(source_id, start_offset, id);

CREATE VIRTUAL TABLE knowledge_chunks_fts USING fts5(
    title,
    heading,
    content,
    tokenize = 'trigram'
);

CREATE TRIGGER knowledge_chunks_fts_insert
AFTER INSERT ON knowledge_chunks
BEGIN
    INSERT INTO knowledge_chunks_fts(rowid, title, heading, content)
    SELECT new.id, source.title, new.heading, new.content
    FROM knowledge_sources AS source
    WHERE source.id = new.source_id;
END;

CREATE TRIGGER knowledge_chunks_fts_delete
AFTER DELETE ON knowledge_chunks
BEGIN
    DELETE FROM knowledge_chunks_fts WHERE rowid = old.id;
END;

CREATE TRIGGER knowledge_chunks_fts_update
AFTER UPDATE ON knowledge_chunks
BEGIN
    DELETE FROM knowledge_chunks_fts WHERE rowid = old.id;
    INSERT INTO knowledge_chunks_fts(rowid, title, heading, content)
    SELECT new.id, source.title, new.heading, new.content
    FROM knowledge_sources AS source
    WHERE source.id = new.source_id;
END;

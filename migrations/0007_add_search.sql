CREATE TABLE chunks (
  paper_id  TEXT NOT NULL REFERENCES papers(id) ON DELETE CASCADE,
  seq       INTEGER NOT NULL,        -- 0 = synthetic title+abstract chunk
  page      INTEGER,                 -- NULL for seq 0
  text      TEXT NOT NULL,
  PRIMARY KEY (paper_id, seq)
);

-- Deliberately NO foreign key: a row may outlive its paper and act as a
-- tombstone telling the indexer to remove Tantivy/Qdrant entries.
CREATE TABLE search_index (
  paper_id           TEXT PRIMARY KEY,
  content_hash       TEXT NOT NULL,
  meta_hash          TEXT NOT NULL,
  chunk_count        INTEGER NOT NULL DEFAULT 0,
  fts_indexed_at     TEXT,
  vectors_indexed_at TEXT,
  embed_model        TEXT,
  last_error         TEXT,
  attempts           INTEGER NOT NULL DEFAULT 0,
  last_attempt_at    TEXT
);

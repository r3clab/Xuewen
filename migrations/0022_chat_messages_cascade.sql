-- chat_messages was the only papers child table without ON DELETE CASCADE,
-- so deleting a paper row required callers to clear its chat rows first.
-- SQLite cannot ALTER a foreign key: rebuild the table (ids copied, so the
-- ORDER BY id thread ordering survives) and recreate its index, which the
-- DROP takes down with the old table.
CREATE TABLE chat_messages_new (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  paper_id   TEXT NOT NULL REFERENCES papers(id) ON DELETE CASCADE,
  role       TEXT NOT NULL CHECK (role IN ('user', 'assistant')),
  content    TEXT NOT NULL,
  model      TEXT,               -- model label, assistant rows only
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  tools_json TEXT
);
INSERT INTO chat_messages_new (id, paper_id, role, content, model, created_at, tools_json)
  SELECT id, paper_id, role, content, model, created_at, tools_json FROM chat_messages;
DROP TABLE chat_messages;
ALTER TABLE chat_messages_new RENAME TO chat_messages;
CREATE INDEX chat_messages_paper ON chat_messages(paper_id, id);

-- Per-paper chat threads: one thread per paper, insertion-ordered.
CREATE TABLE chat_messages (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  paper_id   TEXT NOT NULL REFERENCES papers(id),
  role       TEXT NOT NULL CHECK (role IN ('user', 'assistant')),
  content    TEXT NOT NULL,
  model      TEXT,               -- model label, assistant rows only
  created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX chat_messages_paper ON chat_messages(paper_id, id);

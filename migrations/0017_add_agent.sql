-- Agent Ask: per-paper attached code repository, plus tool-activity chips
-- on chat messages.
CREATE TABLE paper_code (
  paper_id   TEXT PRIMARY KEY REFERENCES papers(id) ON DELETE CASCADE,
  repo_url   TEXT NOT NULL,
  commit_sha TEXT,
  status     TEXT NOT NULL CHECK (status IN ('cloning', 'ready', 'error')),
  error      TEXT,
  cloned_at  TEXT,
  size_bytes INTEGER
);

ALTER TABLE chat_messages ADD COLUMN tools_json TEXT;

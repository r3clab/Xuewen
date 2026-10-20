-- Cached LLM parses of a paper's extracted reference strings. Derived data;
-- safe to drop. One row per paper: `refs` stores the exact extracted input
-- (JSON array of strings) — a cache hit requires it to match, so a changed
-- PDF (different extraction) re-parses.
CREATE TABLE citation_parses (
  paper_id   TEXT PRIMARY KEY REFERENCES papers(id) ON DELETE CASCADE,
  refs       TEXT NOT NULL,   -- JSON: ["raw entry", ...] (the input)
  parsed     TEXT NOT NULL,   -- JSON: [{authors,title,venue,year,doi,arxiv_id,url}|null, ...]
  model      TEXT,            -- model that produced it
  created_at TEXT NOT NULL
);

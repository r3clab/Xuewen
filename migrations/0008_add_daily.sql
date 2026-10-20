CREATE TABLE daily_runs (
  batch_date   TEXT PRIMARY KEY,  -- YYYY-MM-DD (UTC) of the run
  status       TEXT NOT NULL,     -- 'ok' | 'empty' | 'failed'
  papers_found INTEGER NOT NULL,  -- candidates after dedup, before top-N
  error        TEXT,              -- populated when status = 'failed'
  ran_at       TEXT NOT NULL
);

CREATE TABLE daily_papers (
  batch_date TEXT NOT NULL,
  rank       INTEGER NOT NULL,    -- 1-based, by descending score
  arxiv_id   TEXT NOT NULL,       -- versionless
  title      TEXT NOT NULL,
  authors    TEXT NOT NULL,       -- JSON array
  abstract   TEXT NOT NULL,
  categories TEXT NOT NULL,       -- JSON array
  score      REAL NOT NULL,
  tldr       TEXT,                -- NULL when generation failed
  abs_url    TEXT NOT NULL,
  pdf_url    TEXT NOT NULL,
  PRIMARY KEY (batch_date, rank)
);

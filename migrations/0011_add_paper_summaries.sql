-- Per-paper LLM summaries. Derived data (rebuildable from PDFs via the summary
-- sweep / `xuewen summarize`); safe to drop and regenerate.
CREATE TABLE paper_summaries (
  paper_id     TEXT PRIMARY KEY REFERENCES papers(id) ON DELETE CASCADE,
  summary      TEXT NOT NULL,   -- JSON: {tldr, problem, approach, results, limitations}
  model        TEXT,            -- model that produced it
  generated_at TEXT NOT NULL
);

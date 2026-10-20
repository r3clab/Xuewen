-- Failed summary-generation attempts, so the sweep backs off a failing paper
-- (rotates it out of the window) instead of retrying it every tick. A success
-- clears the row. Derived/rebuildable; cascades when the paper is purged.
CREATE TABLE summary_failures (
  paper_id        TEXT PRIMARY KEY REFERENCES papers(id) ON DELETE CASCADE,
  attempts        INTEGER NOT NULL,
  last_attempt_at TEXT NOT NULL
);

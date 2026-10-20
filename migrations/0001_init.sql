CREATE TABLE papers (
  id            TEXT PRIMARY KEY,
  content_hash  TEXT UNIQUE,
  rel_path      TEXT,
  title         TEXT,
  abstract      TEXT,
  authors       TEXT,
  venue         TEXT,
  year          INTEGER,
  doi           TEXT UNIQUE,
  arxiv_id      TEXT UNIQUE,
  dblp_key      TEXT,
  url           TEXT,
  source        TEXT,
  status        TEXT NOT NULL,
  added_at      TEXT NOT NULL
);

CREATE INDEX idx_papers_status ON papers(status);
CREATE INDEX idx_papers_year   ON papers(year);

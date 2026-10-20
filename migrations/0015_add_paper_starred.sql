ALTER TABLE papers ADD COLUMN starred INTEGER NOT NULL DEFAULT 0;
CREATE INDEX idx_papers_starred ON papers(starred) WHERE starred = 1;

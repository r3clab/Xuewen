CREATE TABLE projects (
  id         TEXT PRIMARY KEY,
  name       TEXT NOT NULL,
  note       TEXT,
  created_at TEXT NOT NULL
);
CREATE UNIQUE INDEX idx_projects_name ON projects(name COLLATE NOCASE);

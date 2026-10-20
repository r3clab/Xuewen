CREATE TABLE paper_projects (
  paper_id   TEXT NOT NULL REFERENCES papers(id)   ON DELETE CASCADE,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  added_at   TEXT NOT NULL,
  PRIMARY KEY (paper_id, project_id)
);
CREATE INDEX idx_paper_projects_project ON paper_projects(project_id);

-- The system/tool/benchmark name a paper proposes ("RVSpec"), set manually
-- from the web UI only — ingest, identify, and refresh never write it.
ALTER TABLE papers ADD COLUMN name TEXT;

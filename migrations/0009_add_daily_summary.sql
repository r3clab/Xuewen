ALTER TABLE daily_papers ADD COLUMN summary  TEXT;  -- JSON: {tldr, problem, approach, results, limitations}
ALTER TABLE daily_papers ADD COLUMN code_url TEXT;

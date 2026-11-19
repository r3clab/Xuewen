-- A cite key names one paper (it is the `.bib` entry key), so the database
-- enforces it rather than the filing code alone. Rows that already share a
-- key keep it for the oldest; later ones become `<key>-<n>`. Generated keys
-- are ASCII alphanumerics, so the `-` cannot collide with one; the next
-- refresh re-files those papers under a regular key. `rel_path` is left alone:
-- the files have not moved. The ranks are snapshotted first because the
-- rewrite changes the very column they partition on.

CREATE TEMP TABLE cite_key_rank AS
SELECT id, cite_key, ROW_NUMBER() OVER (PARTITION BY cite_key ORDER BY added_at, id) AS rank
FROM papers
WHERE cite_key IS NOT NULL;

UPDATE papers
SET cite_key = (
  SELECT r.cite_key || '-' || (r.rank - 1) FROM cite_key_rank r WHERE r.id = papers.id
)
WHERE id IN (SELECT id FROM cite_key_rank WHERE rank > 1);

DROP TABLE cite_key_rank;

CREATE UNIQUE INDEX idx_papers_cite_key ON papers(cite_key);

-- Identifiers are stored canonical (`model::canonical_doi`/`canonical_arxiv`):
-- DOIs lowercase, arXiv ids without their version suffix, so the UNIQUE
-- columns dedupe case and version variants and lookups compare with plain `=`.
-- Where canonicalising makes rows collide, the oldest row keeps the
-- identifier; each later one loses it and goes back to review, where the user
-- decides whether it is a duplicate. Losers are cleared before the winners are
-- rewritten: SQLite checks UNIQUE per row, mid-statement.

UPDATE papers SET doi = NULL, status = 'needs_review'
WHERE id IN (
  SELECT id FROM (
    SELECT id, ROW_NUMBER() OVER (PARTITION BY lower(doi) ORDER BY added_at, id) AS rank
    FROM papers
    WHERE doi IS NOT NULL
  )
  WHERE rank > 1
);

UPDATE papers SET doi = lower(doi) WHERE doi <> lower(doi);

-- `canonical_arxiv`: trim, then drop a trailing `v<digits>` that follows at
-- least one character (`r` is the id with its trailing digits removed).
CREATE TEMP TABLE arxiv_canon AS
SELECT id, arxiv_id, ROW_NUMBER() OVER (PARTITION BY arxiv_id ORDER BY added_at, id) AS rank
FROM (
  SELECT id, added_at,
    CASE
      WHEN substr(r, -1) IN ('v', 'V') AND length(r) > 1 AND length(r) < length(t)
        THEN substr(r, 1, length(r) - 1)
      ELSE t
    END AS arxiv_id
  FROM (
    SELECT id, added_at, trim(arxiv_id) AS t, rtrim(trim(arxiv_id), '0123456789') AS r
    FROM papers
    WHERE arxiv_id IS NOT NULL
  )
);

UPDATE papers SET arxiv_id = NULL, status = 'needs_review'
WHERE id IN (SELECT id FROM arxiv_canon WHERE rank > 1);

UPDATE papers
SET arxiv_id = (SELECT c.arxiv_id FROM arxiv_canon c WHERE c.id = papers.id)
WHERE id IN (SELECT id FROM arxiv_canon WHERE rank = 1);

DROP TABLE arxiv_canon;

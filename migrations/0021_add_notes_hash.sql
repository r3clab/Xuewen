-- The FTS tier also carries a paper's annotation notes. Stamping their hash
-- alongside the FTS stamp lets the planner tell "a note changed" (rewrite the
-- Tantivy doc from the chunks already stored) from "the paper changed"
-- (re-extract the PDF and re-chunk it).
--
-- '' means "no notes" — the same value `notes_hash("")` produces — so papers
-- without annotations don't churn through a reindex on upgrade.
ALTER TABLE search_index ADD COLUMN notes_hash TEXT NOT NULL DEFAULT '';

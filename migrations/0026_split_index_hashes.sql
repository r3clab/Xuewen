-- `meta_hash` covered title, abstract, venue, year and authors, and any change
-- re-extracted the PDF and re-embedded every chunk. Each tier now stamps a
-- hash of exactly what it consumed instead: the vector tier the seq-0
-- (title + abstract) text it embedded, the FTS tier the metadata and notes it
-- wrote into the Tantivy doc. Year feeds neither.
--
-- '' is "unknown", so every row is revisited once: the FTS doc is rewritten
-- from the stored chunks (no PDF extraction), and the seq-0 point is only
-- re-embedded when the stored seq-0 text differs from the current one.
ALTER TABLE search_index DROP COLUMN meta_hash;
ALTER TABLE search_index ADD COLUMN summary_hash TEXT NOT NULL DEFAULT '';
ALTER TABLE search_index DROP COLUMN notes_hash;
ALTER TABLE search_index ADD COLUMN doc_hash TEXT NOT NULL DEFAULT '';

-- Written on every re-chunk, never read.
ALTER TABLE search_index DROP COLUMN chunk_count;

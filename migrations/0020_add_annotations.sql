-- Reader annotations (highlight/underline/strikeout/squiggly/sticky note),
-- drawn with @embedpdf/plugin-annotation registered `autoCommit: false` so
-- PDFium never writes back: the library PDF stays byte-identical and
-- `papers.content_hash` (the SHA-256 of those bytes, which drives ingest
-- dedupe and `_unsorted/<hash>.pdf` naming) stays true. USER data, not a
-- derived cache like citation_parses — never rebuilt, never invalidated.
--
-- `id` is the annotation id minted by the plugin, which round-trips
-- unchanged through its own exportAnnotations()/importAnnotations() pair.
-- Uniqueness is only claimed within one document, so the key is composite.
--
-- The typed columns are a queryable PROJECTION (sidebar order, CLI listing,
-- the `notes` search field); `payload` — the verbatim AnnotationTransferItem
-- — is the authoritative source for reconstructing the mark, so fields this
-- schema does not model are preserved rather than dropped on the next load.
CREATE TABLE annotations (
  paper_id    TEXT NOT NULL REFERENCES papers(id) ON DELETE CASCADE,
  id          TEXT NOT NULL,
  page_index  INTEGER NOT NULL,
  kind        TEXT NOT NULL,   -- highlight|underline|strikeout|squiggly|text_comment
  color       TEXT NOT NULL,   -- amber|rose|green|blue|violet
  quoted_text TEXT,            -- the marked-up text; NULL for a bare sticky note
  note        TEXT,            -- the user's comment; NULL when never written
  payload     TEXT NOT NULL,   -- JSON: verbatim AnnotationTransferItem
  created_at  TEXT NOT NULL,
  updated_at  TEXT NOT NULL,
  PRIMARY KEY (paper_id, id)
);

-- Both readers of this table (the reader sidebar and the search indexer's
-- per-paper notes blob) filter by paper and want page order.
CREATE INDEX idx_annotations_paper_page ON annotations(paper_id, page_index);

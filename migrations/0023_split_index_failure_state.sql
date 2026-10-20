-- The FTS and vector tiers fail independently, but failure bookkeeping was a
-- single shared (attempts, last_error, last_attempt_at) slot: one tier's
-- backoff delayed the other tier's healthy work, and either tier's success
-- reset the other's counter. Split it per tier.
ALTER TABLE search_index ADD COLUMN fts_last_error TEXT;
ALTER TABLE search_index ADD COLUMN fts_attempts INTEGER NOT NULL DEFAULT 0;
ALTER TABLE search_index ADD COLUMN fts_last_attempt_at TEXT;
ALTER TABLE search_index ADD COLUMN vec_last_error TEXT;
ALTER TABLE search_index ADD COLUMN vec_attempts INTEGER NOT NULL DEFAULT 0;
ALTER TABLE search_index ADD COLUMN vec_last_attempt_at TEXT;

-- Existing failure state cannot be attributed to a tier after the fact;
-- charge each tier that still has pending work, so the backoff carries over
-- instead of resetting. A tier whose stamp is current gets nothing: no pass
-- will ever run for it, so an error charged there could never be cleared.
UPDATE search_index SET
  fts_last_error = CASE WHEN fts_indexed_at IS NULL THEN last_error END,
  fts_attempts = CASE WHEN fts_indexed_at IS NULL THEN attempts ELSE 0 END,
  fts_last_attempt_at = CASE WHEN fts_indexed_at IS NULL THEN last_attempt_at END,
  vec_last_error = CASE WHEN vectors_indexed_at IS NULL THEN last_error END,
  vec_attempts = CASE WHEN vectors_indexed_at IS NULL THEN attempts ELSE 0 END,
  vec_last_attempt_at = CASE WHEN vectors_indexed_at IS NULL THEN last_attempt_at END;

ALTER TABLE search_index DROP COLUMN last_error;
ALTER TABLE search_index DROP COLUMN attempts;
ALTER TABLE search_index DROP COLUMN last_attempt_at;

-- NULL = active; an RFC-3339 timestamp = trashed (soft-deleted).
ALTER TABLE papers ADD COLUMN deleted_at TEXT;

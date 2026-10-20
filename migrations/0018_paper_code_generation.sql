-- A monotonically increasing token bumped on every (re)attach. A background
-- clone captures the value when it is spawned; its terminal writes are guarded
-- on it, so a stale job that finishes after a newer attach can no longer
-- overwrite the newer row's status.
ALTER TABLE paper_code ADD COLUMN clone_gen INTEGER NOT NULL DEFAULT 0;

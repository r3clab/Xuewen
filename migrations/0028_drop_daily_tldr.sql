-- `tldr` was written as a copy of `summary.tldr`; the summary is now its one
-- source. A copy the summary somehow lacks is folded in first. Rows with a
-- `tldr` and no summary at all predate 0009 and lose their one-liner; the
-- widget shows the abstract instead, as for any paper without a summary.
UPDATE daily_papers SET summary = json_set(summary, '$.tldr', tldr)
WHERE tldr IS NOT NULL AND json_valid(summary) AND json_type(summary, '$.tldr') IS NOT 'text';

ALTER TABLE daily_papers DROP COLUMN tldr;

-- Per-entry provenance becomes a tagged object, and every row a
-- `heuristic-v<gen>[+<model>]` provenance:
--
--   "src": "h<gen>"     -> "src": {"heuristic": <gen>}
--   "src": "llm:<model>" -> "src": {"llm": "<model>"}
--   no "src"            -> "src": {"llm": ...}
--
-- An entry without `src` predates per-entry provenance: a pure-LLM-era parse,
-- possibly carried into a later row. It is stamped as the LLM's — the row's
-- model where the row names one — so a heuristic re-parse never replaces it.
-- A pure-LLM-era row (its `model` a bare model name, or NULL) becomes
-- generation 0, so its nulls get one pass of the current heuristics.
UPDATE citation_parses SET
  parsed = (
    SELECT json_group_array(json(CASE
      WHEN e.type = 'null' THEN NULL
      WHEN e.value ->> '$.src' LIKE 'h%' THEN
        json_set(e.value, '$.src',
          json_object('heuristic', CAST(substr(e.value ->> '$.src', 2) AS INTEGER)))
      WHEN e.value ->> '$.src' LIKE 'llm:%' THEN
        json_set(e.value, '$.src', json_object('llm', substr(e.value ->> '$.src', 5)))
      ELSE
        json_set(e.value, '$.src', json_object('llm', CASE
          WHEN citation_parses.model IS NULL THEN 'unknown'
          WHEN citation_parses.model NOT LIKE 'heuristic-v%' THEN citation_parses.model
          WHEN instr(citation_parses.model, '+') > 0 THEN
            substr(citation_parses.model, instr(citation_parses.model, '+') + 1)
          ELSE 'unknown'
        END))
    END) ORDER BY e.key)
    FROM json_each(citation_parses.parsed) AS e
  ),
  model = CASE
    WHEN model IS NULL THEN 'heuristic-v0'
    WHEN model NOT LIKE 'heuristic-v%' THEN 'heuristic-v0+' || model
    ELSE model
  END
WHERE json_valid(parsed) AND json_type(parsed) = 'array'
  AND NOT EXISTS (
    SELECT 1 FROM json_each(citation_parses.parsed) WHERE type NOT IN ('object', 'null')
  );

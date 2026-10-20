/// Subsequence fuzzy match. Returns null when `query` is not a subsequence
/// of `text` (case-insensitive); otherwise a score favoring consecutive
/// runs (+3 per adjacent hit vs +1) and a match starting at index 0 (+2).
export function fuzzyScore(query: string, text: string): number | null {
  const q = query.toLowerCase();
  const t = text.toLowerCase();
  if (q.length === 0) return 0;
  let qi = 0;
  let score = 0;
  let last = -2;
  for (let ti = 0; ti < t.length && qi < q.length; ti++) {
    if (t[ti] === q[qi]) {
      score += last === ti - 1 ? 3 : 1;
      if (ti === 0) score += 2;
      last = ti;
      qi++;
    }
  }
  return qi === q.length ? score : null;
}

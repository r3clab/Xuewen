/// Parse a page-number input into a clamped 1-based page, or null when the
/// text isn't numeric (empty/letters) or the document has no pages.
export function clampPage(raw: string, totalPages: number): number | null {
  const text = raw.trim();
  if (text === '' || totalPages < 1) return null;
  const n = Math.round(Number(text));
  if (!Number.isFinite(n)) return null;
  return Math.min(totalPages, Math.max(1, n));
}

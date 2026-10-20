import { listPapers } from '$features/library/api';
import { library } from '$features/library/state.svelte';
import type { Reference } from './references';
import type { PaperSummary } from '$shared/api/generated/PaperSummary';

/** Lowercase, replace every non-alphanumeric char with a space, collapse ws.
 *  Mirrors `normalize_title` in src/metadata/matching.rs so both ends agree. */
export function normalizeTitle(s: string): string {
  return s
    .replace(/[^a-zA-Z0-9]+/g, ' ')
    .trim()
    .toLowerCase()
    .replace(/\s+/g, ' ');
}

// Minimum normalized-title length (chars) to attempt a substring match. Short
// titles ("On It") appear by chance in prose and cause false positives.
const MIN_TITLE_LEN = 12;

/** Normalized-title lookup structures for a set of papers — built once,
 *  matched against many times (`matchReferences` used to rebuild this on
 *  every call, twice per document open). */
export interface TitleIndex<P> {
  normed: { paper: P; title: string }[];
  byNormTitle: Map<string, P>;
}

export function buildTitleIndex<P extends { id: string; title: string | null }>(
  papers: P[],
): TitleIndex<P> {
  const normed = papers
    .filter((p) => !!p.title)
    .map((p) => ({ paper: p, title: normalizeTitle(p.title as string) }))
    .filter((p) => p.title.length >= MIN_TITLE_LEN);

  // First match wins — same tie-break as the substring fallback in
  // matchReferences.
  const byNormTitle = new Map<string, P>();
  for (const p of normed) {
    if (!byNormTitle.has(p.title)) byNormTitle.set(p.title, p.paper);
  }
  return { normed, byNormTitle };
}

/** Whole-library title index shared by every open PDF tab: one fetch + one
 *  normalization pass serves all of them (each tab used to refetch and
 *  re-normalize the full library independently). Invalidate when the
 *  library changes (import/delete/identify). */
let libraryIndexPromise: Promise<TitleIndex<PaperSummary>> | null = null;
let libraryIndexRevision = 0;

export function libraryTitleIndex(): Promise<TitleIndex<PaperSummary>> {
  if (libraryIndexRevision !== library.revision) {
    libraryIndexRevision = library.revision;
    libraryIndexPromise = null;
  }
  libraryIndexPromise ??= listPapers({ q: '', status: 'all', sort: 'year_desc', project: 'all' })
    .then(buildTitleIndex)
    .catch((e) => {
      libraryIndexPromise = null; // failed fetches must not be cached
      throw e;
    });
  return libraryIndexPromise;
}

/**
 * For each reference, the first index paper whose normalized title occurs
 * verbatim inside the normalized reference text (or matches the structured
 * title if available). References almost always contain the cited paper's
 * exact title, so substring containment is reliable and needs no parsing.
 */
export function matchReferences<P extends { id: string; title: string | null }>(
  refs: Reference[],
  { normed, byNormTitle }: TitleIndex<P>,
): Map<number, P> {
  const out = new Map<number, P>();
  for (const r of refs) {
    const parsedTitle = r.structured?.title ? normalizeTitle(r.structured.title) : null;
    const exact = parsedTitle && parsedTitle.length >= MIN_TITLE_LEN ? byNormTitle.get(parsedTitle) : undefined;
    if (exact) {
      out.set(r.index, exact);
      continue;
    }
    const hay = normalizeTitle(r.rawText);
    const hit = normed.find((p) => hay.includes(p.title));
    if (hit) out.set(r.index, hit.paper);
  }
  return out;
}

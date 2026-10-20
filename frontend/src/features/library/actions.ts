import { getPaper, getStats, listPapers, setStar } from './api';
import { searchPapers } from '$features/library/search/api';
import { hasSearchTerms, parseQuery } from '$features/library/search/query';
import { filters, searchMeta, searchOpts } from '$features/library/search/state.svelte';
import { cachedDetail, library, patchDetail, setDetail, stats } from './state.svelte';
import { toast } from '$shared/ui/toasts.svelte';
import type { PaperDetail } from '$shared/api/generated/PaperDetail';

export async function loadStats(): Promise<void> {
  try {
    stats.value = await getStats();
  } catch (e) {
    console.error(e);
  }
}

let seq = 0;
export async function loadPapers(opts?: { keywordOnly?: boolean }): Promise<void> {
  const my = ++seq;
  library.loading = true;
  library.error = null;
  try {
    const parsed = parseQuery(filters.q);
    if (!hasSearchTerms(parsed)) {
      // Qualifier-only (or empty) query → plain filtered list. The parsed
      // filters are already cached on `filters`; q itself must not leak in.
      const papers = await listPapers({ ...filters, q: '' });
      if (my !== seq) return; // a newer request superseded this one
      library.papers = papers;
      searchMeta.byId = {};
    } else {
      const keywordOnly = Boolean(opts?.keywordOnly) || !searchOpts.semantic;
      const resp = await searchPapers(filters.q, { ...searchOpts }, keywordOnly);
      if (my !== seq) return;
      library.papers = resp.results.map((r) => r.paper);
      searchMeta.byId = Object.fromEntries(resp.results.map((r) => [r.paper.id, r.match]));
      searchMeta.semantic = { available: resp.semantic.available, reason: resp.semantic.reason };
    }
  } catch (e) {
    if (my === seq) library.error = (e as Error).message;
  } finally {
    if (my === seq) {
      library.loading = false;
      library.loaded = true;
    }
  }
}

export async function loadDetail(id: string): Promise<PaperDetail> {
  const cached = cachedDetail(id);
  if (cached) return cached;
  const d = await getPaper(id);
  setDetail(id, d);
  return d;
}

/// Flip a paper's starred flag optimistically: patch the row/cached detail
/// first so the star moves instantly, then call the API and roll back (with
/// an error toast) if it rejects. When the starred filter is active the list
/// itself may need to drop/gain the paper, so it reloads after the call.
export async function toggleStar(paperId: string): Promise<void> {
  const row = library.papers.find((p) => p.id === paperId);
  const prev = row?.starred ?? cachedDetail(paperId)?.starred ?? false;
  const next = !prev;
  if (row) row.starred = next;
  patchDetail(paperId, { starred: next });
  try {
    await setStar(paperId, next);
  } catch (e) {
    if (row) row.starred = prev;
    patchDetail(paperId, { starred: prev });
    toast('error', `Couldn't update star: ${(e as Error).message}`);
    return;
  }
  if (filters.starred !== undefined) await loadPapers();
}
